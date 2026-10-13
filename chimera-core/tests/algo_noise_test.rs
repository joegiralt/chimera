//! The NOIS operator in the engine (spec § 4): a band of noise centred on
//! CENTRE, WIDTH wide, at the −12 dB reference; RES blends in the tone;
//! seeded per voice, never reset at note-on.

mod common;

use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::{AlgoParams, bw_hz, frmt_hz};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::hw::Cost;
use chimera_core::modulation::ModState;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::taps::TapRows;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::formant::{harmonic_db, level_db_vs_r, one, render_algo, render_algo_with};
use common::rig::Src;
use common::sweep::power;
use common::{Rig, SR, VEL, rms};

fn nois(frmt: u8, bw: u8, res: u8) -> AlgoParams {
    let mut p = one(OpKind::Noise, frmt, bw, 1);
    p.ops[0].res = res;
    p
}

/// The band's centre and −3 dB full width, Hz. Spec § 4's band, two
/// one-poles ring-modulated to FC (`L(f − FC) + L(f + FC)`, the image
/// folded through 0 Hz included), is fitted in dB to the averaged
/// 8,192-point spectra within four corners of the centre, then read at its
/// half-power points. The noisy spectrum's own maximum would read every
/// band narrow.
fn band_peak_and_width(x: &[f32]) -> (f32, f32) {
    const N: usize = 8192;
    let pw = power(x, N);
    let bin = f64::from(SR) / N as f64;
    let band = |f: f64, fc: f64, fl: f64| {
        let l = |d: f64| 1.0 / (1.0 + (d / fl).powi(2)).powi(2);
        l(f - fc) + l(f + fc)
    };
    // A first guess: the loudest bin, and the corner from a half-power
    // walk (0.6436 corners each side are −3 dB).
    let top = (1..pw.len())
        .max_by(|&a, &b| pw[a].total_cmp(&pw[b]))
        .unwrap();
    let walk = |step: isize| {
        let mut k = top as isize;
        while k > 1 && (k as usize) < pw.len() - 1 && pw[k as usize] > pw[top] / 2.0 {
            k += step;
        }
        (k - top as isize).unsigned_abs() as f64 * bin
    };
    let (mut fc, mut fl) = (top as f64 * bin, (walk(-1) + walk(1)) / (2.0 * 0.6436));
    let (lo, hi) = (
        ((fc - 4.0 * fl) / bin).max(1.0) as usize,
        ((fc + 4.0 * fl) / bin) as usize,
    );
    let bins: Vec<(f64, f64)> = (lo..hi.min(pw.len()))
        .map(|k| (k as f64 * bin, pw[k].max(1e-300).ln()))
        .collect();
    // Squared dB error with the level at its best (the residuals' mean).
    let err = |fc: f64, fl: f64| {
        let r: Vec<f64> = bins
            .iter()
            .map(|&(f, y)| y - band(f, fc, fl).ln())
            .collect();
        let m = r.iter().sum::<f64>() / r.len() as f64;
        r.iter().map(|v| (v - m) * (v - m)).sum::<f64>()
    };
    let golden = |mut a: f64, mut b: f64, e: &dyn Fn(f64) -> f64| {
        for _ in 0..60 {
            let (c, d) = (b - 0.618 * (b - a), a + 0.618 * (b - a));
            if e(c) < e(d) { b = d } else { a = c }
        }
        (a + b) / 2.0
    };
    for _ in 0..4 {
        let w = fl;
        fc = golden(fc - w, fc + w, &|c| err(c, fl));
        fl = golden((w / 2.0).ln(), (w * 2.0).ln(), &|l| err(fc, l.exp())).exp();
    }
    // The fitted band's half-power points, on a 0.5 Hz grid.
    let f: Vec<f64> = (0..SR as usize).map(|i| i as f64 * 0.5).collect();
    let s: Vec<f64> = f.iter().map(|&v| band(v, fc, fl)).collect();
    let peak = (0..s.len()).max_by(|&a, &b| s[a].total_cmp(&s[b])).unwrap();
    let (mut a, mut b) = (peak, peak);
    while a > 0 && s[a] >= s[peak] / 2.0 {
        a -= 1;
    }
    while b < s.len() - 1 && s[b] >= s[peak] / 2.0 {
        b += 1;
    }
    (((f[a] + f[b]) / 2.0) as f32, (f[b] - f[a]) as f32)
}

/// The magnitude `harmonic_db` reads for a sine carrying all of `x`'s
/// energy: half its amplitude (`common::goertzel` divides by the length).
fn db_total(x: &[f32]) -> f32 {
    20.0 * (rms(x) * core::f32::consts::FRAC_1_SQRT_2).log10()
}

/// `note` struck on `e` and held `blocks` blocks, then released (the
/// release isn't rendered).
fn play_note(e: &mut AlgoEngine, p: &AlgoParams, note: u8, blocks: usize) -> Vec<f32> {
    let out = play_note_held(e, p, note, blocks);
    e.note_off();
    out
}

/// `note` struck on `e`, sounding or not, and held `blocks` blocks.
fn play_note_held(e: &mut AlgoEngine, p: &AlgoParams, note: u8, blocks: usize) -> Vec<f32> {
    e.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(VEL).unwrap(),
        p,
        SR,
    );
    let live = AlgoLive::from_params(p);
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut blk = [0.0; BLOCK_SIZE];
    for _ in 0..blocks {
        e.render(&mut blk, p, &live, SR, &mut TapRows::new([false; 2]));
        out.extend_from_slice(&blk);
    }
    out
}

#[test]
fn noise_band_centre_and_width() {
    for bw in [48u8, 111, 183] {
        let out = render_algo(&nois(104, bw, 0), 48, 3000);
        let (peak_hz, width_hz) = band_peak_and_width(&out[BLOCK_SIZE * 10..]);
        println!("BW {bw}: centre {peak_hz:.1} Hz, width {width_hz:.1} Hz");
        assert!((peak_hz / frmt_hz(104.0) - 1.0).abs() < 0.05, "{bw}");
        assert!((width_hz / bw_hz(bw as f32) - 1.0).abs() < 0.15, "{bw}");
    }
}

#[test]
fn noise_level_is_normalised() {
    for bw in [0u8, 111, 183] {
        for res in [0u8, 50, 100] {
            // A 20 Hz band's RMS over 1.87 s spreads ±0.6 dB: 30 s for BW 0.
            let blocks = if bw == 0 { 22_600 } else { 1500 };
            let out = render_algo(&nois(104, bw, res), 48, blocks);
            let held = &out[BLOCK_SIZE * 100..];
            println!("BW {bw} RES {res}: {:+.3} dB vs R", level_db_vs_r(held));
            assert!(level_db_vs_r(held).abs() < 0.5, "{bw} {res}");
            assert!(held.iter().all(|x| x.abs() <= 1.0));
            if res == 100 {
                // 1,400 blocks: an f32 Goertzel over 30 s loses its bin.
                let tone = &held[..1400 * BLOCK_SIZE];
                assert!(harmonic_db(tone, frmt_hz(104.0)) > -0.05 + db_total(tone));
            }
        }
    }
}

/// A WIDTH jump from 3,948 Hz to 20 Hz rescales only new noise (spec § 4):
/// over 64 jumps (8 seeds × 8), the 10 ms after each holds no more energy
/// than the settled 20 Hz band, +1 dB. A single run can't judge it: the
/// 20 Hz band's own peaks reach +10 dB over 10 ms.
#[test]
fn width_jumps_do_not_burst() {
    // 183 for 100 blocks, then 0 for 100: a jump at block 100 + 200·m.
    const HALF: usize = 100;
    let (mut jump, mut steady) = (0.0f64, 0.0f64);
    for v in VoiceIdx::ALL {
        let (out, _) = render_algo_with(&nois(104, 183, 0), 48, 16 * HALF, |b, p, _, e| {
            if b == 0 {
                e.seed(v);
            }
            p.ops[0].bw = if (b / HALF).is_multiple_of(2) { 183 } else { 0 };
        });
        assert!(out.iter().all(|x| x.abs() <= 1.0));
        let energy = |a: usize, n: usize| {
            out[a..a + n]
                .iter()
                .map(|&x| f64::from(x) * f64::from(x))
                .sum::<f64>()
                / n as f64
        };
        for j in (HALF..16 * HALF).step_by(2 * HALF) {
            jump += energy(j * BLOCK_SIZE, SR as usize / 100);
            // Settled: 60 blocks (80 ms) on, eight poles' time constants.
            steady += energy((j + 60) * BLOCK_SIZE, 40 * BLOCK_SIZE);
        }
    }
    let db = 10.0 * (jump / steady).log10();
    println!("10 ms after a jump: {db:+.2} dB over the settled band");
    // +27.1 dB scaled after the poles (nois.py).
    assert!(db <= 1.0);
}

/// The band plays the live CENTRE, WIDTH and VOWEL, as a route or a knob
/// on its way sets them (Task 12), not the stored ones.
#[test]
fn the_band_follows_live_centre_width_and_vowel() {
    let mut p = nois(80, 48, 0);
    p.ops[0].vslot = chimera_core::dsp::algo::vowel::VowelSlot::F2.code();
    p.vowel = 50;
    let (out, _) = render_algo_with(&p, 48, 3000, |_, _, live, _| {
        (live.frmt[0], live.bw[0], live.vowel) = (104.0, 111.0, 100.0)
    });
    let (peak_hz, width_hz) = band_peak_and_width(&out[BLOCK_SIZE * 10..]);
    let want = chimera_core::dsp::algo::vowel::fc_hz(
        104.0,
        100.0,
        Some(chimera_core::dsp::algo::vowel::Formant::F2),
        0.0,
        0.0,
    );
    assert!((peak_hz / want - 1.0).abs() < 0.05, "{peak_hz} vs {want}");
    assert!((width_hz / bw_hz(111.0) - 1.0).abs() < 0.15, "{width_hz}");
}

/// A square LFO on WIDTH, 3,948 Hz and 20 Hz at 4 Hz, rescales only new
/// noise: Task 9's ruled statistic over 64 jumps (8 seeds × 8), the 10 ms
/// after each 183 → 0 jump holds no more energy than the settled 20 Hz
/// band, +1 dB, and never a sample past full scale.
#[test]
fn width_square_lfo_does_not_burst() {
    const HALF: usize = 94;
    let (mut jump, mut steady) = (0.0f64, 0.0f64);
    for v in VoiceIdx::ALL {
        let (out, _) = render_algo_with(&nois(104, 183, 0), 48, 16 * HALF, |b, _, live, e| {
            if b == 0 {
                e.seed(v);
            }
            live.bw[0] = if (b / HALF).is_multiple_of(2) {
                183.0
            } else {
                0.0
            };
        });
        assert!(out.iter().all(|x| x.abs() <= 1.0));
        let energy = |a: usize, n: usize| {
            out[a..a + n]
                .iter()
                .map(|&x| f64::from(x) * f64::from(x))
                .sum::<f64>()
                / n as f64
        };
        for j in (HALF..16 * HALF).step_by(2 * HALF) {
            jump += energy(j * BLOCK_SIZE, SR as usize / 100);
            // Settled: 60 blocks (80 ms) on.
            steady += energy((j + 60) * BLOCK_SIZE, (HALF - 60) * BLOCK_SIZE);
        }
    }
    let db = 10.0 * (jump / steady).log10();
    println!("10 ms after a jump: {db:+.2} dB over the settled band");
    assert!(db <= 1.0);
}

#[test]
fn repeated_notes_do_not_repeat_their_noise() {
    let mut e = AlgoEngine::new();
    let p = nois(104, 111, 0);
    let a = play_note(&mut e, &p, 48, 20);
    let b = play_note(&mut e, &p, 48, 20);
    assert_ne!(a, b);
}

#[test]
fn renders_are_reproducible_from_a_seed() {
    let p = nois(104, 111, 0);
    let mut e1 = AlgoEngine::new();
    e1.seed(VoiceIdx::ALL[3]);
    let mut e2 = AlgoEngine::new();
    e2.seed(VoiceIdx::ALL[3]);
    assert_eq!(
        play_note(&mut e1, &p, 48, 50),
        play_note(&mut e2, &p, 48, 50)
    );
    let mut e3 = AlgoEngine::new();
    e3.seed(VoiceIdx::ALL[4]);
    assert_ne!(
        play_note(&mut e1, &p, 48, 50),
        play_note(&mut e3, &p, 48, 50)
    );
}

#[test]
fn kind_swaps_under_a_held_note_do_not_click() {
    let (out, _) = render_algo_with(
        &one(OpKind::Table(WaveId::W1), 104, 111, 1),
        48,
        1200,
        |b, p, _, _| {
            if b % 75 == 0 {
                let k = if p.kinds().get(0) == OpKind::Noise {
                    OpKind::Table(WaveId::W1)
                } else {
                    OpKind::Noise
                };
                p.try_set_kind(0, k).unwrap();
            }
        },
    );
    assert!(out.iter().any(|x| x.abs() > 0.1));
    assert!(common::clicks(&out).is_empty());
}

#[test]
fn a_res_change_on_retrigger_ramps() {
    // Two engines alike to the retrigger, one struck again at RES 100.
    let p = nois(104, 111, 0);
    let mut tone = p;
    tone.ops[0].res = 100;
    let (mut e, mut same) = (AlgoEngine::new(), AlgoEngine::new());
    let a = play_note(&mut e, &p, 48, 40);
    play_note(&mut same, &p, 48, 40);
    // A retrigger, without release.
    let b = play_note_held(&mut e, &tone, 48, 40);
    let kept = play_note_held(&mut same, &p, 48, 40);
    assert!(common::clicks(&[a, b.clone()].concat()).is_empty());
    // What RES changed starts at nothing and grows across the first block:
    // a snap would put it all on the first samples.
    let d: Vec<f32> = b.iter().zip(&kept).map(|(x, y)| (x - y).abs()).collect();
    let peak = |r: &[f32]| r.iter().fold(0.0f32, |m, &x| m.max(x));
    let (start, block) = (peak(&d[..4]), peak(&d[..BLOCK_SIZE]));
    println!("RES change: first 4 samples {start:.4}, first block {block:.4}");
    assert!(block > 0.01 && start < 0.25 * block);
}

/// Ruling 7: SKIRT latches at note-on on an idle operator, so a FRMT
/// operator struck at SKIRT ≥ 1 and stored back to 0 still runs its two
/// lanes: the voice bills the difference until it ends.
#[test]
fn a_latched_skirt_bills_over_a_stored_zero() {
    let mut p = one(OpKind::Formant, 72, 72, 1);
    let mut e = AlgoEngine::new();
    assert_eq!(e.held_skirt_extra(&p), Cost::ZERO);
    play_note_held(&mut e, &p, 48, 2);
    assert_eq!(e.held_skirt_extra(&p), Cost::ZERO);
    p.ops[0].skirt = 0;
    assert_eq!(
        e.held_skirt_extra(&p),
        Cost(AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_FRMT_ONE.0)
    );
    // Struck again while sounding: the latch holds.
    play_note_held(&mut e, &p, 48, 2);
    assert_ne!(e.held_skirt_extra(&p), Cost::ZERO);
}

/// Spec § 4: each voice's noise is its own from boot. Voices built as the
/// Instrument builds them (`Voice::init_in_place` with their id) play one
/// NOIS patch differently, and the same id alike.
#[test]
fn boot_built_voices_play_their_own_noise() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = nois(104, 111, 0);
    let play = |v: usize| {
        let mut r = Rig::at(SR, VoiceIdx::ALL[v], Src::None);
        r.note_on(
            MidiNote::new(48).unwrap(),
            Velocity::new(VEL).unwrap(),
            &s.params,
        );
        let mut out = Vec::new();
        let mut blk = [0.0; BLOCK_SIZE];
        for _ in 0..20 {
            r.render(&mut blk, &s.params, &ModState::new());
            out.extend_from_slice(&blk);
        }
        assert!(out.iter().any(|x| x.abs() > 1e-3));
        out
    };
    assert_eq!(play(1), play(1));
    assert_ne!(play(0), play(1));
}

/// Spec § 8: a kind swapped in starts closed, a NOIS's poles at 0. At
/// WIDTH 20 Hz the poles take about 10 ms to open, so the 4 ms after each
/// swap back into NOIS (its old poles still in the kernel without the
/// reset) hold far less than the band does settled.
#[test]
fn a_swapped_in_noise_starts_closed() {
    const EVERY: usize = 75;
    let (out, _) = render_algo_with(
        &one(OpKind::Table(WaveId::W1), 104, 0, 1),
        48,
        8 * EVERY,
        |b, p, _, _| {
            if b > 0 && b % EVERY == 0 {
                let k = if p.kinds().get(0) == OpKind::Noise {
                    OpKind::Table(WaveId::W1)
                } else {
                    OpKind::Noise
                };
                p.try_set_kind(0, k).unwrap();
            }
        },
    );
    let energy = |a: usize, b: usize| {
        let x = &out[a * BLOCK_SIZE..b * BLOCK_SIZE];
        x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64
    };
    // Into NOIS at 75, 225, 375 and 525: ducked that block, rising the
    // next. From 225 on the kernel has played noise before.
    let (mut opening, mut settled) = (0.0, 0.0);
    for s in [225, 375, 525] {
        opening += energy(s + 1, s + 4);
        settled += energy(s + 40, s + EVERY);
    }
    let db = 10.0 * (opening / settled).log10();
    println!("4 ms after a swap into NOIS: {db:+.1} dB against the settled band");
    assert!(db < -10.0);
}
