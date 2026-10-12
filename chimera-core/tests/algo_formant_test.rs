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
use chimera_core::dsp::dc_blocker::DcBlocker;
use chimera_core::dsp::voice::DC_HZ;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::formant::{
    carrier_peak_before_norm, db, fundamental_cents, grain_fc, harmonic_db, level_db_vs_r, mean,
    off_grid_db, one, op6_peak, render_algo, render_algo_with, sideband_energy_db, spectrum_db,
    steps_10ms_db,
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

/// Operator 1 FRMT under T1, operator 2 its modulator at `ratio_coarse`
/// and `level`.
fn modded(ratio_coarse: u8, level: u8, frmt: u8, bw: u8, s: u8) -> AlgoParams {
    let mut p = one(OpKind::Formant, frmt, bw, s);
    p.alg_a = AlgoId::T1.get(); // op 2 → op 1
    (p.ops[1].coarse, p.ops[1].level) = (ratio_coarse, level);
    p
}

#[test]
fn pm_never_cuts_a_grain() {
    for c in [4u8, 8] {
        // ratio 1, ratio 2
        for s in 0..4u8 {
            let (out, cuts) = render_algo_with(&modded(c, 99, 80, 72, s), A3, 400, |_, _, _, _| {});
            // No `clicks`: full-level PM is bright, which it hears (Task 11 ruling).
            assert_eq!(cuts, 0, "{c} {s}");
            assert!(out.iter().all(|x| x.abs() <= 1.0), "{c} {s}");
        }
    }
}

#[test]
fn pm_enters_grain_and_carrier() {
    let mut p = modded(4, 83, 80, 0, 0); // about 1 cycle of PM
    // FRMT at ratio 2, so the ratio-1 sidebands fall between its harmonics.
    p.ops[0].coarse = 8;
    let out = render_algo(&p, A3, 800);
    let held = &out[BLOCK_SIZE * 100..];
    // Sidebands round the harmonics.
    let side = sideband_energy_db(held, 440.0, 220.0);
    assert!(side > -30.0, "{side}");
}

/// Operator 6's peak, dB, FRMT 80 at LEVEL 70 and FDBK `fb` into op 5
/// under T1, at each BW step.
fn modulator_peaks(fb: u8) -> Vec<f32> {
    [0u8, 36, 72, 108, 144, 180, 216]
        .map(|bw| {
            let mut p = AlgoParams::single(WaveId::W1);
            (p.ops[0].level, p.ops[4].level) = (0, 99);
            p.try_set_kind(5, OpKind::Formant).unwrap();
            let o = &mut p.ops[5];
            (o.frmt, o.bw, o.level, o.feedback) = (80, bw, 70, fb);
            db(op6_peak(&p, A3, 400, 100))
        })
        .into()
}

fn spread(v: &[f32]) -> f32 {
    let (lo, hi) = v
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    hi - lo
}

#[test]
fn bw_does_not_change_modulation_depth() {
    // The modulator's own peak, op 6's tap into op 5 under T1: a carrier's
    // output can't give peak depth back (Task 11 ruling).
    let v = modulator_peaks(0);
    assert!(spread(&v) < 0.5, "{v:?}");
    // ADR F: under its own FDBK the peak is referenced to the drive's bound,
    // which short grains don't reach: a regression bound, measured 16.92 dB.
    let v = modulator_peaks(5);
    assert!(spread(&v) < 17.0, "{v:?}");
    assert!(v.iter().all(|&p| p <= 0.0), "{v:?}");
}

/// FDBK and a modulator's LEVEL glided up from 0 on short grains, where
/// the peak reference moves most: the gains follow continuously.
#[test]
fn drive_glides_do_not_click() {
    for bw in [72u8, 144] {
        let (out, cuts) = render_algo_with(&modded(4, 0, 80, bw, 1), A3, 600, |b, _, live, _| {
            let t = b as f32 / 300.0;
            live.feedback[0] = 3.0 * t.min(1.0);
            live.level[1] = 60.0 * chimera_core::dsp::clampf(t - 1.0, 0.0, 1.0);
        });
        assert_eq!(cuts, 0, "{bw}");
        assert!(common::clicks(&out).is_empty(), "{bw}");
        assert!(out.iter().all(|x| x.abs() <= 1.0), "{bw}");
    }
}

#[test]
fn feedback_moves_the_formant_not_the_pitch() {
    let mut p = one(OpKind::Formant, 80, 72, 1);
    let a = render_algo(&p, A3, 400);
    // FDBK 5: 7 is chaotic, so no pitch reads to 0.1 cent (Task 11 ruling).
    p.ops[0].feedback = 5;
    let b = render_algo(&p, A3, 400);
    let c = fundamental_cents(&b, 220.0);
    assert!(c.abs() < 0.1, "{c} cents");
    // The harmonic nearest 2·FC: between harmonics only noise would read.
    let h = (2.0 * frmt_hz(80.0) / 220.0).round() * 220.0;
    let at = |x: &[f32]| harmonic_db(&x[BLOCK_SIZE * 50..], h);
    assert!(at(&b) - at(&a) >= 6.0, "{} vs {}", at(&b), at(&a));
}

#[test]
fn a_modulated_high_formant_does_not_alias() {
    // FC ≈ 5 kHz: A3 at LEVEL 99, C3 at 60, 70 and 83; FC 1 kHz at A3 and
    // C3, LEVEL 83 and 99 (Task 11 rulings).
    for (frmt, n, level) in [
        (158u8, A3, 99u8),
        (158, C3, 60),
        (158, C3, 70),
        (158, C3, 83),
        (80, A3, 83),
        (80, C3, 83),
        (80, A3, 99),
        (80, C3, 99),
    ] {
        let p = modded(4, level, frmt, 72, 0);
        let out = render_algo(&p, n, 800);
        // PM at ratio 1 stays on the harmonic grid.
        let grid: Vec<f32> = (1..)
            .map(|k| k as f32 * f0(n))
            .take_while(|&h| h < 24_000.0)
            .collect();
        let off = off_grid_db(&out[BLOCK_SIZE * 100..], &grid);
        assert!(off < -60.0, "{frmt} {n} {level}: {off}");
    }
}

#[test]
fn formant_operators_are_stable() {
    let mut p = AlgoParams::default();
    p.alg_a = AlgoId::A17.get();
    p.try_set_kind(0, OpKind::Formant).unwrap();
    for o in &mut p.ops {
        (o.level, o.feedback, o.frmt, o.bw, o.skirt) = (99, 7, 168, 216, 3);
    }
    let out = render_algo(&p, C3, 30 * 750);
    assert!(out.iter().all(|x| x.is_finite()));
    // DC after the voice's 5 Hz blocker (ADR 0060), which removes an
    // engine's: FDBK skews each grain, so the raw output isn't DC-free.
    let mut dc = DcBlocker::new(DC_HZ, SR);
    let blocked: Vec<f32> = out.iter().map(|&x| dc.process(x)).collect();
    let m = mean(&blocked[blocked.len() - 480_000..]);
    assert!(m.abs() < 1e-3, "{m} (raw {})", mean(&out));
    // 10 s against 10 s: this chaotic chain's 1 s RMS wanders (Task 11 ruling).
    let (last, first) = (rms(&out[out.len() - 480_000..]), rms(&out[48_000..528_000]));
    assert!(first > 0.0 && db(last / first) < 0.5, "{first} {last}");
    let peak = carrier_peak_before_norm(&p, C3);
    assert!(peak <= 1.0, "{peak}");
}

#[test]
fn morph_across_a_formant_carrier_is_smooth() {
    let mut p = one(OpKind::Formant, 80, 72, 1);
    (p.alg_a, p.alg_b) = (AlgoId::T1.get(), AlgoId::A1.get());
    p.try_set_kind(1, OpKind::Formant).unwrap();
    // LEVEL 50: at 90, plain FM alone trips `clicks` (Task 11 ruling).
    p.ops[1].level = 50;
    let (out, _) = render_algo_with(&p, C3, 750, |b, _, live, _| {
        live.morph = (b as f32 / 750.0 * 127.0).min(127.0)
    });
    assert!(common::clicks(&out).is_empty());
    let steps = steps_10ms_db(&out);
    assert!(steps.iter().all(|d| d.abs() <= 1.0), "{steps:?}");
}

#[test]
fn grains_stay_odd_under_pm() {
    let out = render_algo(&modded(4, 99, 34, 0, 1), C5, 800); // FC ≈ 267 Hz at C5
    let m = mean(&out[BLOCK_SIZE * 100..]);
    assert!(m.abs() < 1e-4, "{m}");
}
