//! The FRMT operator in the engine (spec § 3): a two-lane grain train on
//! the operator's unmodulated clock, each grain latched at its start.

mod common;

use chimera_core::budget::FORMANT_N;
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::formant::{NYQUIST_HZ, Skirt, formant_law, width_floor};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::{AlgoParams, frmt_hz};
use chimera_core::dsp::algo::vowel::{Formant, VOWEL_OCT, VowelSlot, fc_hz};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::formant::{
    grain_fc, level_db_vs_r, off_grid_db, one, render_algo, render_algo_with, spectrum_db,
};
use common::{SR, rms};
use std::f64::consts::TAU;

const A3: u8 = 57;
const C1: u8 = 24;
const C2: u8 = 36;
const C3: u8 = 48;
const C4: u8 = 60;
const C5: u8 = 72;

fn f0(n: u8) -> f32 {
    440.0 * 2f32.powf((n as f32 - 69.0) / 12.0)
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// FRMT and the VOWEL that put operator 1's FC at `fc` Hz exactly: FRMT
/// the stored step at or under it, the rest from slot F2's offset along
/// the path's A–E or E–I leg, where it rises monotonically.
fn at_fc(fc: f32, bw: u8, s: u8) -> (AlgoParams, f32) {
    let oct = (f64::from(fc) / 100.0).log2();
    let frmt = f64::from(chimera_core::dsp::clampf(
        (oct * 24.0).floor() as f32,
        0.0,
        168.0,
    ));
    let d = (oct - frmt / 24.0) as f32;
    let (a, e, i) = (VOWEL_OCT[2][1], VOWEL_OCT[3][1], VOWEL_OCT[4][1]);
    assert!((a..=i).contains(&d), "{fc} Hz is past F2's reach");
    let vowel = if d <= e {
        50.0 + 25.0 * (d - a) / (e - a)
    } else {
        75.0 + 25.0 * (d - e) / (i - e)
    };
    let got = fc_hz(frmt as f32, vowel, Some(Formant::F2), 0.0, 0.0);
    assert!((got / fc - 1.0).abs() < 1e-5, "{got} for {fc}");
    let mut p = one(OpKind::Formant, frmt as u8, bw, s);
    p.ops[0].vslot = VowelSlot::F2.code();
    p.vowel = vowel.round() as u8;
    (p, vowel)
}

/// Operator 1 alone with FC at `fc` Hz (`at_fc`), held at `note`.
fn render_fc(fc: f32, bw: u8, s: u8, note: u8, blocks: usize) -> Vec<f32> {
    let (p, vowel) = at_fc(fc, bw, s);
    render_algo_with(&p, note, blocks, |_, _, live, _| live.vowel = vowel).0
}

/// The held second's level within ±0.5 dB of R.
fn lvl_ok(fc: f32, bw: u8, s: u8, note: u8) {
    let out = render_fc(fc, bw, s, note, 1500);
    let l = level_db_vs_r(&out[BLOCK_SIZE * 750..]);
    assert!(
        l.abs() < 0.5,
        "FC {fc} BW {bw} SKIRT {s} note {note}: {l:+.2} dB"
    );
}

/// The peak nearest `hz` in `spectrum_db`'s Hann spectrum, against `hz`,
/// in cents. The two bins of Hann's main lobe give the offset exactly:
/// their ratio `α` is `(1 + δ)/(2 − δ)`.
fn peak_bin_cents(s: &[f32], hz: f32) -> f32 {
    let bin = SR as f32 / (2 * s.len()) as f32;
    let c = (hz / bin).round() as usize;
    let k = (c - 3..=c + 3)
        .max_by(|&a, &b| s[a].total_cmp(&s[b]))
        .unwrap();
    let mag = |i: usize| 10f64.powf(f64::from(s[i]) / 20.0);
    let (side, sign) = if s[k + 1] >= s[k - 1] {
        (k + 1, 1.0)
    } else {
        (k - 1, -1.0)
    };
    let alpha = mag(side) / mag(k);
    let f = (k as f64 + sign * (2.0 * alpha - 1.0) / (alpha + 1.0)) * f64::from(bin);
    (1200.0 * (f / f64::from(hz)).log2()) as f32
}

/// The loudest bin in the middle half between neighbouring partials of
/// `f0`, dB re the spectrum's peak.
fn between_partials_db(s: &[f32], f0: f32) -> f32 {
    let bin = SR as f32 / (2 * s.len()) as f32;
    s.iter()
        .enumerate()
        .filter(|&(i, _)| {
            let r = (i as f32 * bin / f0).fract();
            (0.25..=0.75).contains(&r)
        })
        .map(|(_, &v)| v)
        .fold(f32::MIN, f32::max)
}

/// The Hann-windowed amplitude of `hz` in `x`.
fn hann_amp(x: &[f32], hz: f32) -> f64 {
    let n = x.len() as f64;
    let w = TAU * f64::from(hz) / f64::from(SR);
    let (mut re, mut im) = (0.0, 0.0);
    for (i, &v) in x.iter().enumerate() {
        let h = 0.5 - 0.5 * (TAU * i as f64 / n).cos();
        re += h * f64::from(v) * (w * i as f64).cos();
        im -= h * f64::from(v) * (w * i as f64).sin();
    }
    re.hypot(im)
}

/// The formant's −3 dB full width as harmonic `h` of `note` sees it, Hz:
/// FRMT stepped in stored steps across `h·f0` ± 700 Hz, the harmonic's
/// level read at each, and the half-power points interpolated between
/// steps.
fn transfer_width(note: u8, h: u32, bw: u8, s: u8) -> f32 {
    let hz = h as f32 * f0(note);
    let pts: Vec<(f32, f64)> = (0..=168u8)
        .map(|v| (v, frmt_hz(f32::from(v))))
        .filter(|&(_, f)| (f - hz).abs() <= 700.0)
        .map(|(v, f)| {
            let out = render_algo(&one(OpKind::Formant, v, bw, s), note, 160);
            (f, 20.0 * hann_amp(&out[BLOCK_SIZE * 40..], hz).log10())
        })
        .collect();
    let top = (0..pts.len())
        .max_by(|&a, &b| pts[a].1.total_cmp(&pts[b].1))
        .unwrap();
    let half = pts[top].1 - 3.0103;
    let cross = |step: isize| {
        let mut i = top as isize;
        while pts[(i + step) as usize].1 > half {
            i += step;
        }
        let (a, b) = (pts[i as usize], pts[(i + step) as usize]);
        a.0 + (b.0 - a.0) * ((a.1 - half) / (a.1 - b.1)) as f32
    };
    cross(1) - cross(-1)
}

#[test]
fn harmonics_are_exact() {
    let out = render_algo(&one(OpKind::Formant, 80, 72, 1), A3, 800);
    let s = spectrum_db(&out[BLOCK_SIZE * 100..], 32_768);
    for k in 1..20 {
        let c = peak_bin_cents(&s, k as f32 * 220.0);
        assert!(c.abs() < 0.1, "harmonic {k}: {c} cents");
    }
    let floor = between_partials_db(&s, 220.0);
    assert!(floor < -60.0, "{floor} dB between partials");
}

#[test]
fn formant_peak_stays_put_across_pitch() {
    for n in [C2, C3, C4] {
        // BW 1,008 Hz, SKIRT 0: d < 1, single lane.
        let out = render_algo(&one(OpKind::Formant, 80, 120, 0), n, 400);
        let fc = grain_fc(&out[BLOCK_SIZE * 50..]);
        assert!((fc / frmt_hz(80.0) - 1.0).abs() < 0.01, "{n}: {fc}");
    }
}

#[test]
fn key_100_tracks_the_note() {
    for key in [50u8, 100] {
        for n in [C3, C4, C5] {
            // BW 72: at C3 KEY 100 (400 Hz) BW 120 leaves under one cycle a grain.
            let mut p = one(OpKind::Formant, 72, 72, 0);
            p.ops[0].key = key;
            let want = fc_hz(72.0, 50.0, None, key as f32, n as f32);
            let got = grain_fc(&render_algo(&p, n, 400)[BLOCK_SIZE * 50..]);
            assert!(
                (got / want - 1.0).abs() < 0.01,
                "KEY {key} note {n}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn first_grain_sounds_at_note_on() {
    for n in [C1, C2, C3, C4, C5] {
        let out = render_algo(&one(OpKind::Formant, 72, 72, 1), n, 2);
        assert!(out[..64].iter().any(|x| x.abs() > 1e-3), "{n}");
    }
}

#[test]
fn level_is_normalised() {
    for s in 0..4u8 {
        for bw in [111u8, 120, 131, 139] {
            lvl_ok(1000.0, bw, s, C3); // 524 Hz–2 kHz
        }
        for bw in [135u8, 144, 155, 168] {
            lvl_ok(1000.0, bw, s, C4); // 1,047 Hz–4 kHz
        }
        for fc in [270.0, 300.0] {
            lvl_ok(fc, 72, s, C5);
        }
    }
}

#[test]
fn peak_never_exceeds_one() {
    for n in [24u8, 36, 48, 60, 72, 84, 96] {
        for bw in [0u8, 72, 144, 216] {
            for s in 0..4u8 {
                for fr in [0u8, 69, 111, 168] {
                    let out = render_algo(&one(OpKind::Formant, fr, bw, s), n, 200);
                    assert!(out.iter().all(|x| x.abs() <= 1.0), "{n} {bw} {s} {fr}");
                }
            }
        }
    }
    // The ceiling binds.
    let out = render_fc(1000.0, 216, 1, C3, 400);
    let l = level_db_vs_r(&out[BLOCK_SIZE * 100..]);
    assert!((l + 5.3).abs() < 0.2, "{l:+.2} dB");
}

#[test]
fn width_floor_keeps_skirt_a_shape() {
    // The formant's envelope, sampled by one fixed harmonic: at A3, BW 0,
    // hold harmonic 5 (1,100 Hz) while FRMT steps across 1,100 Hz ± 700 Hz
    // in stored steps. Its −3 dB width is BW_eff.
    let want = formant_law(220.0, 1100.0, 20.0, Skirt::S1).bw_eff;
    let widths: Vec<f32> = (0..4u8).map(|s| transfer_width(A3, 5, 0, s)).collect();
    for w in &widths {
        assert!((w / want - 1.0).abs() < 0.10, "{widths:?} vs {want}");
    }
    let (lo, hi) = widths
        .iter()
        .fold((f32::MAX, 0f32), |(a, b), &w| (a.min(w), b.max(w)));
    assert!(hi / lo < 1.10, "{widths:?}");
    // Below the floor, each BW step widens it.
    assert!(transfer_width(C2, 9, 100, 1) > transfer_width(C2, 9, 80, 1));
}

#[test]
fn bw_jumps_never_cut_a_grain() {
    for s in 1..4u8 {
        for off in 0..7 {
            let (out, cuts) =
                render_algo_with(&one(OpKind::Formant, 80, 0, s), A3, 700, |b, p, _, _| {
                    if (b + off) % 7 == 0 {
                        // 20 ↔ 2,000 Hz
                        p.ops[0].bw = if p.ops[0].bw == 0 { 144 } else { 0 };
                    }
                });
            assert_eq!(cuts, 0, "{s} {off}");
            assert!(common::clicks(&out).is_empty(), "{s} {off}");
        }
    }
}

#[test]
fn pitch_jumps_never_cut_a_grain() {
    for s in [1u8, 3] {
        let (_, cuts) = render_algo_with(&one(OpKind::Formant, 80, 0, s), A3, 700, |b, _, _, e| {
            if b % 7 == 0 {
                e.set_pitch(if b % 14 == 0 { 12.0 } else { 0.0 });
            }
        });
        assert_eq!(cuts, 0, "{s}");
    }
}

#[test]
fn high_formants_do_not_alias() {
    for n in [A3, C3] {
        for fc in [6000.0, 8000.0, 10_000.0] {
            let out = render_fc(fc, 216, 0, n, 800);
            let grid: Vec<f32> = (1..)
                .map(|k| k as f32 * f0(n))
                .take_while(|&h| h < 24_000.0)
                .collect();
            // A3's folds land 40 Hz off the grid.
            let off = off_grid_db(&out[BLOCK_SIZE * 100..], &grid);
            assert!(off < -60.0, "{n} {fc}: {off}");
        }
    }
}

#[test]
fn formant_edge_fades() {
    let b = width_floor(f0(C3));
    let m = Skirt::S0.m_q();
    let silent = render_fc(NYQUIST_HZ - m * b / 2.0, 0, 0, C3, 200);
    let r = rms(&silent[BLOCK_SIZE * 50..]);
    assert!(r < 1e-6, "{r}");
    let half = render_fc(NYQUIST_HZ - m * 0.75 * b, 0, 0, C3, 200);
    let full = render_fc(NYQUIST_HZ - m * 2.0 * b, 0, 0, C3, 200);
    let d = db(rms(&half[BLOCK_SIZE * 50..]) / rms(&full[BLOCK_SIZE * 50..]));
    assert!((d + 6.02).abs() < 0.5, "{d}");
}

#[test]
fn formant_extremes_stay_finite() {
    for n in [0u8, 127] {
        for co in [0u8, 63] {
            for tr in [-24i8, 24] {
                for fr in [0u8, 168] {
                    for bw in [0u8, 216] {
                        for s in 0..4u8 {
                            let mut p = one(OpKind::Formant, fr, bw, s);
                            p.ops[0].coarse = co;
                            p.transpose = tr;
                            p.ops[0].key = 100;
                            let out = render_algo(&p, n, 60);
                            assert!(
                                out.iter().all(|x| x.is_finite() && x.abs() <= 1.0),
                                "{n} {co} {tr} {fr} {bw} {s}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn retrigger_on_a_sounding_formant_voice_does_not_click() {
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N.min(3) as usize {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    p.alg_a = AlgoId::A1.get();
    let (out, cuts) = render_algo_with(&p, C3, 400, |b, _, _, e| {
        if b == 188 {
            // 250 ms
            e.note_on(
                MidiNote::new(C3).unwrap(),
                Velocity::new(100).unwrap(),
                &p,
                SR,
            );
        }
    });
    assert!(common::clicks(&out).is_empty());
    assert_eq!(cuts, 0);
}

#[test]
fn kind_swaps_under_a_held_note_do_not_click() {
    let order = [OpKind::Table(WaveId::W1), OpKind::Formant, OpKind::Noise];
    let (out, _) = render_algo_with(
        &one(OpKind::Table(WaveId::W1), 80, 72, 1),
        C3,
        1200,
        |b, p, _, _| {
            if b % 75 == 0 {
                p.try_set_kind(0, order[(b / 75) % 3]).unwrap();
            }
        },
    );
    assert!(common::clicks(&out).is_empty());
}
