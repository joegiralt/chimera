//! 2040, the SSM2040 four-pole, and its VOICE (spec § 3, ADR 0096; plan
//! Task 8 and its amendment): REV2 the Prophet-5 Rev 1/2, EXP the Kobol.

mod common;

use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::filter::coef::g_of;
use chimera_core::dsp::filter::k2040::{
    ALPHA_EXP, ALPHA_REV2, OUT_2040, R_2040, STAGE_SCALE_EXP, k2040_k, make_up, sat_2040,
};
use chimera_core::dsp::filter::{
    Filter, FilterKind, FilterMode, FilterMods, K2040, K2040Voice, KIND_NAMES, level_gain,
    routed_cutoff,
};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::storage::RecordTag;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::codec_util::{decode, encode, filter_payload, sound_file as file};
use common::filter_ref::{OSC_BAND_DB, a_track, grid_input, k2040_ref, mag_db, rms_db, thd};

const SR: u32 = 48_000;
const FS: f32 = SR as f32;
const VOICES: [K2040Voice; 2] = K2040Voice::ALL;

fn params(voice: K2040Voice, fc: f32, res: f32, level: f32) -> FilterParams {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::K2040);
    (p.cutoff, p.resonance, p.level, p.voice) = (fc, res, level, voice);
    p
}

/// `n` samples through a fresh 2040 at a held setting, fed `x(i)`.
fn render(p: &FilterParams, n: usize, x: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut f = K2040::new();
    let mut out = Vec::with_capacity(n);
    for b in 0..n.div_ceil(BLOCK_SIZE) {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        f.process(&mut buf, p, SR, &FilterMods::NONE);
        out.extend_from_slice(&buf);
    }
    out.truncate(n);
    out
}

fn sine(amp: f32, f: f32) -> impl Fn(usize) -> f32 {
    move |i| amp * (core::f32::consts::TAU * f * i as f32 / FS).sin()
}

/// Gain at `f` for a sine of `amp`, dB: read over 1 s after 0.5 s.
fn gain_db(p: &FilterParams, amp: f32, f: f32) -> f32 {
    let y = render(p, (1.5 * FS) as usize, sine(amp, f));
    mag_db(&y[(0.5 * FS) as usize..], f, FS) - 20.0 * amp.log10()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// The linear prototype: four TPT one-poles at `g` under feedback `k`,
/// `H = H₁⁴/(1 + k·H₁⁴)`, `H₁ = g(1 + z⁻¹)/((1 + g) − (1 − g)z⁻¹)`.
fn proto_db(fc: f32, res: f32, f: f64) -> f64 {
    let g = f64::from(g_of(fc, FS));
    let k = f64::from(k2040_k(res));
    let w = core::f64::consts::TAU * f / f64::from(FS);
    let (c, s) = (w.cos(), -w.sin()); // z⁻¹
    let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let div = |a: (f64, f64), b: (f64, f64)| {
        let d = b.0 * b.0 + b.1 * b.1;
        ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
    };
    let h1 = div(
        (g * (1.0 + c), g * s),
        ((1.0 + g) - (1.0 - g) * c, -(1.0 - g) * s),
    );
    let h4 = mul(mul(h1, h1), mul(h1, h1));
    let h = div(h4, (1.0 + k * h4.0, k * h4.1));
    db(h.0.hypot(h.1))
}

/// The prototype's −3 dB point (re its DC gain), above its peak.
fn proto_corner(fc: f32, res: f32) -> f64 {
    let dc = proto_db(fc, res, 1.0);
    let (mut lo, mut hi) = (f64::from(fc) * 0.3, f64::from(fc) * 3.0);
    // From the top down, so a resonant peak above −3 dB isn't the answer.
    assert!(proto_db(fc, res, hi) < dc - 3.0 && proto_db(fc, res, lo) > dc - 3.0);
    for _ in 0..60 {
        let mid = (lo * hi).sqrt();
        if proto_db(fc, res, mid) > dc - 3.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

#[test]
fn k2040_small_signal_corner_matches_the_linear_prototype() {
    for v in VOICES {
        for fc in [250.0f32, 1000.0, 4000.0] {
            for res in [0.0f32, 0.2, 0.4, 0.6, 0.8] {
                let p = params(v, fc, res, 0.5);
                let f3 = proto_corner(fc, res);
                // EXP's make-up lifts the whole response.
                let up = if v.make_up() {
                    20.0 * make_up(k2040_k(res)).log10()
                } else {
                    0.0
                };
                let dc = proto_db(fc, res, 1.0) as f32 + up;
                let (below, above) = (
                    gain_db(&p, 0.01, (f3 * 0.95) as f32) - dc,
                    gain_db(&p, 0.01, (f3 * 1.05) as f32) - dc,
                );
                assert!(
                    below > -3.0 && above < -3.0,
                    "{v:?} {fc} Hz RES {res}: corner {f3:.0} Hz, {below:.2} / {above:.2} dB"
                );
            }
        }
    }
}

/// The loop's bass loss, `−20·log₁₀(1 + k)`, under EXP's make-up gain.
fn bass_want(v: K2040Voice, res: f32) -> f32 {
    let k = k2040_k(res);
    let up = if v.make_up() { make_up(k) } else { 1.0 };
    20.0 * (up / (1.0 + k)).log10()
}

/// The full-scale bass gain against `bass_want`, and its resonant
/// peak against small signal, RES ≤ 0.8 (EXP is meant to compress there):
/// the worst measured, plus margin.
const K2040_FULL_DB: [f32; 2] = [1.0, 1.0];
/// The ±1 V clip holds a full-scale sine at the peak: −9.11 dB on REV2 and
/// −14.66 on EXP at RES 0.8, the worst (2026-10-06), plus 0.5.
const K2040_PEAK_DB: [f32; 2] = [9.6, 15.2];
/// The bass at RES 1 against `bass_want`, dB: REV2 −1.2 small, −0.6 full;
/// EXP −1.1 small.
const RES1_DB: f32 = 1.5;

#[test]
fn k2040_bass_loss_is_the_loops_under_the_make_up() {
    for v in VOICES {
        for amp in [0.01f32, 1.0] {
            let mut last = f32::INFINITY;
            for r in 0..=10 {
                let res = r as f32 / 10.0;
                let tol = match (v, r, amp == 1.0) {
                    // Past onset REV2 rings at the clip (its 2020 is
                    // linear), which ducks the bass: −1.2 dB measured. EXP
                    // reaches the clip too since ADR 0191: −1.1.
                    (_, 10, _) => RES1_DB,
                    (_, _, true) => K2040_FULL_DB[v as usize],
                    _ => 1.0,
                };
                let got = gain_db(&params(v, 2000.0, res, 0.5), amp, 100.0);
                let want = bass_want(v, res);
                println!("{v:?} amp {amp} RES {res}: {got:.2} dB (want {want:.2})");
                assert!(got < last, "{v:?} amp {amp} RES {res}: {got} after {last}");
                assert!(
                    (got - want).abs() <= tol,
                    "{v:?} amp {amp} RES {res}: {got}"
                );
                last = got;
            }
            let at = gain_db(&params(v, 2000.0, 0.95, 0.5), amp, 100.0);
            let tol = if amp == 1.0 {
                K2040_FULL_DB[v as usize]
            } else {
                1.0
            };
            let want = bass_want(v, 0.95);
            assert!(
                (at - want).abs() <= tol,
                "{v:?} amp {amp} RES 0.95: {at} vs {want}"
            );
        }
        // The resonant peak, at the prototype's peak frequency.
        for r in 1..=8 {
            let res = r as f32 / 10.0;
            let peak_f = (100..4000)
                .map(|f| f as f64)
                .max_by(|a, b| proto_db(2000.0, res, *a).total_cmp(&proto_db(2000.0, res, *b)))
                .unwrap() as f32;
            let p = params(v, 2000.0, res, 0.5);
            let (small, full) = (gain_db(&p, 0.01, peak_f), gain_db(&p, 1.0, peak_f));
            println!("{v:?} RES {res} peak {peak_f} Hz: small {small:.2}, full {full:.2}");
            assert!(
                (full - small).abs() <= K2040_PEAK_DB[v as usize],
                "{v:?} RES {res}: {full:.2} vs {small:.2}"
            );
        }
    }
}

fn impulse(i: usize) -> f32 {
    if i == 0 { 1.0 } else { 0.0 }
}

/// RES 1 from one impulse, read over 0.5 s once it has grown in: 0.25 s,
/// or 120 periods of `fc` where that is longer.
fn oscillation(v: K2040Voice, fc: f32) -> Vec<f32> {
    let settle = (0.25 * FS).max(120.0 * FS / fc) as usize;
    render(
        &params(v, fc, 1.0, 0.5),
        settle + (0.5 * FS) as usize,
        impulse,
    )
    .split_off(settle)
}

/// The pitch of `x`, Hz, from its upward zero crossings, interpolated.
fn pitch(x: &[f32]) -> f64 {
    let ups: Vec<f64> = x
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
        .map(|(i, w)| i as f64 + f64::from(w[0] / (w[0] - w[1])))
        .collect();
    assert!(ups.len() > 10, "no oscillation: {} crossings", ups.len());
    f64::from(FS) * (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0])
}

fn cents(f: f64, want: f64) -> f64 {
    1200.0 * (f / want).log2()
}

/// REV2's self-oscillation, dBFS RMS (−5.8 to −5.9 measured, −4.9 at 8
/// kHz; ± 1 dB): the ±1 V clip's level, its 2020 linear.
const REV2_OSC_DB: f32 = -5.4;
/// EXP's (−5.1 to −5.3 measured; ± 0.5 dB): at `L_2040` ½ its ring
/// reaches the ±1 V clip, then the make-up's +6.9 dB at RES 1 (ADR 0191;
/// −17.1 at ¼ with none), in REV2's band.
const EXP_OSC_DB: f32 = -5.2;
/// Absolute pitch, cents: REV2 −2.5 to +0.3 measured, trimmed; EXP −4.5
/// to +4.1 (ADR 0191's clip-bound ring drifts flat up the top octaves).
const PITCH_CENTS: f64 = 5.0;

#[test]
fn k2040_self_oscillates_in_band_and_on_pitch() {
    let freqs: Vec<f32> = (0..7)
        .map(|o| 100.0 * (1 << o) as f32)
        .chain([8000.0])
        .collect();
    for v in VOICES {
        let (lo, hi) = OSC_BAND_DB;
        for &fc in &freqs {
            let y = oscillation(v, fc);
            let level = rms_db(&y);
            let wobble = y
                .chunks(4800)
                .map(|w| (rms_db(w) - level).abs())
                .fold(0.0f32, f32::max);
            println!("{v:?} RES 1 {fc} Hz: {level:.2} dBFS ±{wobble:.2}");
            assert!(wobble <= 1.0, "{v:?} {fc} Hz: ±{wobble:.2} dB");
            assert!(level > lo && level < hi, "{v:?} {fc} Hz: {level:.2} dBFS");
            // Pinned, so each voice's element shows.
            let (want, tol) = match v {
                K2040Voice::Rev2 => (REV2_OSC_DB, 1.0),
                K2040Voice::Exp => (EXP_OSC_DB, 0.5),
            };
            assert!(
                (level - want).abs() <= tol,
                "{v:?} {fc} Hz: {level:.2} dBFS"
            );
        }
        // FM2 (NOTE → CUTOFF) at +100 %: C2..C6 from FREQ at C4, two bases.
        for base in [261.626f32, 1250.0] {
            let mut last: Option<f64> = None;
            for note in (36..=84).step_by(12) {
                let fc = routed_cutoff(base, (note as f32 - 60.0) / 120.0);
                let want = f64::from(base) * 2f64.powf((note as f64 - 60.0) / 12.0);
                if want > 5000.0 {
                    continue;
                }
                let f = pitch(&oscillation(v, fc));
                let c = cents(f, want);
                println!("{v:?} base {base} note {note}: {c:.1} cents");
                assert!(
                    c.abs() <= PITCH_CENTS,
                    "{v:?} base {base} note {note}: {c:.1} cents"
                );
                if let Some(l) = last {
                    let o = cents(f, l) - 1200.0;
                    assert!(
                        o.abs() <= 10.0,
                        "{v:?} base {base} note {note}: octave {o:.1}"
                    );
                }
                last = Some(f);
            }
        }
        // The onset, RES 0.95 ± 0.01. Near it the four-pole's ring decays
        // slowly (at RES 0.94, 1.6 % a period at 250 Hz): 2.5 s.
        let n = (2.5 * FS) as usize;
        for fc in [250.0, 1000.0, 4000.0] {
            let at = |res: f32| {
                let y = render(&params(v, fc, res, 0.5), n, impulse);
                let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let late = y[n - (0.1 * FS) as usize..]
                    .iter()
                    .fold(0.0f32, |m, v| m.max(v.abs()));
                20.0 * (late / peak).log10()
            };
            let (below, above) = (at(0.94), at(0.96));
            assert!(below < -60.0, "{v:?} {fc} Hz RES 0.94: {below:.1} dB");
            assert!(above > -20.0, "{v:?} {fc} Hz RES 0.96: {above:.1} dB");
        }
    }
}

#[test]
fn k2040_unity_at_level_half_res_0() {
    for v in VOICES {
        let g = gain_db(&params(v, 20_000.0, 0.0, 0.5), 0.1, 100.0);
        println!("{v:?}: {g:.3} dB");
        assert!(g.abs() <= 0.5, "{v:?}: {g} dB");
    }
}

/// A saw at 0.8 of full scale, 1/218 a sample.
fn saw(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.8 * ((i % 218) as f32 / 109.0 - 1.0))
        .collect()
}

/// The per-sample `g`, `k` and `α·gain` tracks `process` ramps through,
/// one `(cutoff, RES)` a block, at a held LEVEL `gain`, for `k2040_ref`.
type Tracks = (Vec<f64>, Vec<f64>, Vec<f64>);
fn tracks(v: K2040Voice, blocks: &[(f32, f32)], gain: f32) -> Tracks {
    let (mut g, mut k, mut a) = (Vec::new(), Vec::new(), Vec::new());
    let ga = gain * v.alpha();
    for (b, &(fc, res)) in blocks.iter().enumerate() {
        let (from, from_res) = blocks[b.saturating_sub(1)];
        g.extend(a_track(g_of(from * v.trim(), FS), g_of(fc * v.trim(), FS)));
        k.extend(a_track(k2040_k(from_res), k2040_k(res)));
        a.extend(a_track(ga, ga));
    }
    (g, k, a)
}

/// `(cutoff, RES)` a block through the 2040, and through `k2040_ref` in
/// f64 with the ramps per sample: max |y − y_ref| / max |y_ref|.
fn jump_error(v: K2040Voice, blocks: &[(f32, f32)]) -> f64 {
    let mut p = params(v, 1000.0, 0.0, 0.5);
    let x = saw(blocks.len() * BLOCK_SIZE);
    let mut f = K2040::new();
    let mut y = Vec::new();
    for (b, &(fc, res)) in blocks.iter().enumerate() {
        (p.cutoff, p.resonance) = (fc, res);
        let mut buf: [f32; BLOCK_SIZE] = x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        y.extend_from_slice(&buf);
    }
    let (g, k, a) = tracks(v, blocks, 1.0);
    let r = k2040_ref::<f64>(&x, &g, &k, &a, v, v.post());
    let peak = r.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let err = y
        .iter()
        .zip(&r)
        .fold(0.0f64, |m, (&a, b)| m.max((f64::from(a) - b).abs()));
    err / peak
}

/// The exact output bound: every stage within ±R, so `|out| ≤ R/α·OUT`,
/// times EXP's make-up gain at its largest, RES 1.
fn bound(v: K2040Voice) -> f32 {
    let up = if v.make_up() {
        make_up(k2040_k(1.0))
    } else {
        1.0
    };
    R_2040 / v.alpha() * OUT_2040 * up + 1e-6
}

#[test]
fn k2040_one_block_jumps_and_bounds() {
    let held = |a: f32, b: f32| -> Vec<f32> { [[a; 6], [b; 6]].concat() };
    let env: Vec<f32> = (0..12usize)
        .map(|b| {
            let t = (b.saturating_sub(4) * BLOCK_SIZE) as f32 / (0.001 * FS);
            routed_cutoff(100.0, t.min(1.0))
        })
        .collect();
    for v in VOICES {
        for (name, cutoffs) in [
            ("100 → 20k", held(100.0, 20_000.0)),
            ("20k → 100", held(20_000.0, 100.0)),
            ("ENV", env.clone()),
        ] {
            let blocks: Vec<(f32, f32)> = cutoffs.iter().map(|&fc| (fc, 0.0)).collect();
            let e = jump_error(v, &blocks);
            assert!(e <= 1e-3, "{v:?} {name}: {e:.2e}");
            // RES 1: self-oscillating, bounded.
            let mut p = params(v, 1000.0, 1.0, 0.5);
            let x = saw(cutoffs.len() * BLOCK_SIZE);
            let mut f = K2040::new();
            for (b, &fc) in cutoffs.iter().enumerate() {
                p.cutoff = fc;
                let mut buf: [f32; BLOCK_SIZE] =
                    x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
                f.process(&mut buf, &p, SR, &FilterMods::NONE);
                assert!(buf.iter().all(|y| y.is_finite() && y.abs() <= bound(v)));
            }
        }
        // RES 0 → 0.9 → 0.2 in single blocks at a held 2 kHz.
        let blocks: Vec<(f32, f32)> = [0.0f32, 0.9, 0.2]
            .iter()
            .flat_map(|&res| [(2000.0, res); 4])
            .collect();
        let e = jump_error(v, &blocks);
        assert!(e <= 1e-3, "{v:?} RES jumps: {e:.2e}");
        // The bound grid × LEVEL 0, ½, 1.
        let mut max = 0.0f32;
        for fi in 0..24 {
            let fc = 20.0 * 1000f32.powf(fi as f32 / 23.0);
            for ri in 0..=10 {
                for level in [0.0, 0.5, 1.0] {
                    let p = params(v, fc, ri as f32 / 10.0, level);
                    for which in 0..5 {
                        let mut f = K2040::new();
                        for b in 0..20 {
                            let mut buf: [f32; BLOCK_SIZE] =
                                core::array::from_fn(|i| grid_input(which, b * BLOCK_SIZE + i));
                            f.process(&mut buf, &p, SR, &FilterMods::NONE);
                            for y in buf {
                                assert!(y.is_finite(), "{v:?} {fc} RES {ri} LEVEL {level}");
                                max = max.max(y.abs());
                            }
                        }
                    }
                }
            }
        }
        println!("{v:?} grid: max |y| {max}, bound {}", bound(v));
        assert!(max <= bound(v), "{v:?}: {max} > {}", bound(v));
    }
}

#[test]
fn k2040_recovers_from_a_nan_block() {
    for v in VOICES {
        let p = params(v, 1000.0, 0.5, 0.5);
        let mut f = K2040::new();
        f.process(&mut [f32::NAN; BLOCK_SIZE], &p, SR, &FilterMods::NONE);
        let mut quiet = [0.0f32; BLOCK_SIZE];
        f.process(&mut quiet, &p, SR, &FilterMods::NONE);
        assert!(quiet.iter().all(|&y| y == 0.0), "{v:?}: {quiet:?}");
        let x = saw(2 * BLOCK_SIZE);
        let mut b = [0.0f32; BLOCK_SIZE];
        for c in x.chunks(BLOCK_SIZE) {
            b.copy_from_slice(c);
            f.process(&mut b, &p, SR, &FilterMods::NONE);
        }
        assert!(b.iter().all(|y| y.is_finite()), "{v:?}: {b:?}");
        assert!(b.iter().any(|&y| y != 0.0), "{v:?}");
    }
}

#[test]
fn kind_codes_round_trip() {
    assert_eq!(KIND_NAMES.len(), FilterKind::BUILT.len());
    assert_eq!(KIND_NAMES, ["CLEAN", "2040", "DIRTY", "LADDER"]);
    assert_eq!(FilterKind::K2040.disk_code(), 4);
    assert_eq!(FilterKind::K2040.disk_ident(), "2040");
    assert_eq!(FilterKind::K2040.modes(), [FilterMode::Lp24]);
    assert_eq!(FilterKind::K2040.default_mode(), FilterMode::Lp24);
    assert_eq!(FilterKind::K2040.mode_labels(), ["LP4"]);
    for v in VOICES {
        let mut s = Sound::init(EngineType::Algo);
        s.params.filter.set_kind(FilterKind::K2040);
        s.params.filter.voice = v;
        let d = decode(&encode(&s)).unwrap();
        assert!(d.bits_eq(&s), "{v:?}");
        assert_eq!(d.params.filter.kind(), FilterKind::K2040);
        assert_eq!(d.params.filter.mode(), FilterMode::Lp24);
        assert_eq!(d.params.filter.voice, v);
        assert_eq!(K2040Voice::from_disk_code(v.disk_code()), Some(v));
    }
    assert_eq!(K2040Voice::Exp.disk_code(), 1);
    assert_eq!(K2040Voice::labels(), &["REV2", "EXP"]);
    assert_eq!(K2040Voice::default(), K2040Voice::Rev2);
    let engine = (RecordTag::Engine, vec![EngineType::Algo.disk_code()]);
    let code = |c: u32| c.to_le_bytes();
    // A 2040 file without VOICE, and one with an unknown code: REV2.
    for voice in [None, Some(code(2)), Some(code(255)), Some(code(1 << 20))] {
        let mut e = vec![(6, code(4)), (0, 800f32.to_le_bytes())];
        e.extend(voice.map(|c| (13, c)));
        let s = decode(&file(&[
            engine.clone(),
            (RecordTag::Block, filter_payload(&e)),
        ]))
        .unwrap();
        assert_eq!(s.params.filter.kind(), FilterKind::K2040);
        assert_eq!(s.params.filter.voice, K2040Voice::Rev2, "{voice:?}");
    }
    let e = [(6, code(4)), (13, code(1))];
    let s = decode(&file(&[
        engine.clone(),
        (RecordTag::Block, filter_payload(&e)),
    ]))
    .unwrap();
    assert_eq!(s.params.filter.voice, K2040Voice::Exp);
    // Every engine's neutral Sound stores REV2.
    for e in EngineType::ALL {
        assert_eq!(Sound::neutral(e).params.filter.voice, K2040Voice::Rev2);
    }
    // VOICE writes by index, held in range.
    let mut p = FilterParams::default();
    for (v, want) in [
        (1.0, K2040Voice::Exp),
        (0.0, K2040Voice::Rev2),
        (9.0, K2040Voice::Exp),
    ] {
        p.set(FilterParams::VOICE, v);
        assert_eq!(p.voice, want);
    }
}

/// The resonant-peak gain at full scale less that at 0.01, dB: RES 0.8,
/// FREQ 1 kHz, a 110 Hz saw, read at its 9th harmonic (990 Hz).
fn compression(v: K2040Voice) -> f32 {
    let p = params(v, 1000.0, 0.8, 0.5);
    let at = |amp: f32| {
        let x = |i: usize| amp * (2.0 * ((i as f32 * 110.0 / FS) % 1.0) - 1.0);
        let n = (1.5 * FS) as usize;
        let y = render(&p, n, x);
        let xs: Vec<f32> = (0..n).map(x).collect();
        let from = (0.5 * FS) as usize;
        mag_db(&y[from..], 990.0, FS) - mag_db(&xs[from..], 990.0, FS)
    };
    at(1.0) - at(0.01)
}

/// Each voice's compression (± 0.3 dB): REV2's linear 2020 stays under
/// the clip here (2026-10-06), EXP's 3080 squelches, −2.65 dB at `L_2040`
/// ½ (ADR 0191; −6.42 at ¼).
const COMPRESSION_DB: [f32; 2] = [-0.02, -2.65];

#[test]
fn k2040_exp_squelches_under_drive() {
    let (rev2, exp) = (compression(K2040Voice::Rev2), compression(K2040Voice::Exp));
    println!(
        "compression: REV2 {rev2:.2} dB, EXP {exp:.2} dB, margin {:.2}",
        rev2 - exp
    );
    assert!(exp <= rev2 - 2.0, "REV2 {rev2:.2}, EXP {exp:.2}");
    for (v, got) in [(K2040Voice::Rev2, rev2), (K2040Voice::Exp, exp)] {
        let want = COMPRESSION_DB[v as usize];
        assert!((got - want).abs() <= 0.3, "{v:?}: {got:.2} vs {want}");
    }
}

#[test]
fn k2040_post_is_exp_only() {
    // Bit for bit against the reference in f32, over moving and steady
    // blocks.
    let blocks = [
        (300.0f32, 0.3f32),
        (300.0, 0.3),
        (3000.0, 0.9),
        (3000.0, 0.9),
        (800.0, 0.5),
    ];
    for v in VOICES {
        for post in [false, true] {
            let mut p = params(v, 1000.0, 0.0, 0.8);
            let x: Vec<f32> = (0..blocks.len() * BLOCK_SIZE)
                .map(sine(1.0, 220.0))
                .collect();
            let mut f = K2040::new();
            let mut y = Vec::new();
            for (b, &(fc, res)) in blocks.iter().enumerate() {
                (p.cutoff, p.resonance) = (fc, res);
                let mut buf: [f32; BLOCK_SIZE] =
                    x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
                f.process(&mut buf, &p, SR, &FilterMods::NONE);
                y.extend(buf.map(f64::from));
            }
            let (g, k, a) = tracks(v, &blocks, level_gain(0.8));
            let r = k2040_ref::<f32>(&x, &g, &k, &a, v, post);
            // The voice's own `post()` is not the oracle: EXP alone runs it.
            let want = post == (v == K2040Voice::Exp);
            assert_eq!(y == r, want, "{v:?} against post {post}");
        }
    }
    // EXP's 3080 below the clip: y₄ peaks at 0.95·R.
    let amp = 0.95 * R_2040 / ALPHA_EXP;
    let t = |v: K2040Voice| {
        let y = render(
            &params(v, 20_000.0, 0.0, 0.5),
            (1.0 * FS) as usize,
            sine(amp, 100.0),
        );
        thd(&y[(0.5 * FS) as usize..], 100.0, FS)
    };
    let (rev2, exp) = (t(K2040Voice::Rev2), t(K2040Voice::Exp));
    println!(
        "THD at 0.95 R: REV2 {:.3} %, EXP {:.3} %",
        rev2 * 100.0,
        exp * 100.0
    );
    assert!((0.008..=0.02).contains(&(exp - rev2)), "{rev2} {exp}");
}

#[test]
fn k2040_voice_flip_keeps_level_and_codes() {
    for (from, to) in [
        (K2040Voice::Rev2, K2040Voice::Exp),
        (K2040Voice::Exp, K2040Voice::Rev2),
    ] {
        let x = |i: usize| 0.1 * (2.0 * ((i as f32 * 110.0 / FS) % 1.0) - 1.0);
        let run = |flip: bool, start: K2040Voice| {
            let mut p = params(start, 1000.0, 0.0, 0.5);
            let mut f = Filter::new();
            let mut out = Vec::new();
            for b in 0..40 {
                if flip && b == 20 {
                    p.voice = to;
                }
                let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
                f.process(&mut buf, &p, SR, &FilterMods::NONE);
                out.extend_from_slice(&buf);
            }
            out
        };
        let (flipped, stayed, own) = (run(true, from), run(false, from), run(false, to));
        let at = 20 * BLOCK_SIZE;
        assert!(common::clicks(&flipped).is_empty(), "{from:?} → {to:?}");
        // The next 64 samples' level against the note's own there (a 64-sample
        // window is a seventh of the saw's period, so not the block before).
        let (after, before) = (
            rms_db(&flipped[at..at + BLOCK_SIZE]),
            rms_db(&stayed[at..at + BLOCK_SIZE]),
        );
        println!("{from:?} → {to:?}: {after:.3} dBFS against {before:.3}");
        assert!(
            (after - before).abs() <= 0.5,
            "{from:?} → {to:?}: {before} → {after}"
        );
        // Linear content is exact: the flip lands on the new voice's own
        // output (the trims move the corners: REV2's −13 cents, EXP's +36).
        let peak = own.iter().fold(0.0f32, |m, y| m.max(y.abs()));
        let err = flipped[at..]
            .iter()
            .zip(&own[at..])
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(err <= 0.04 * peak, "{from:?} → {to:?}: {err} of {peak}");
    }
}

#[test]
fn k2040_stages_hold_the_chip_swing() {
    for v in VOICES {
        let mut max = 0.0f32;
        for fi in 0..8 {
            let fc = 20.0 * 1000f32.powf(fi as f32 / 7.0);
            for res in [0.0, 0.8, 1.0] {
                for level in [0.0, 0.5, 1.0] {
                    let p = params(v, fc, res, level);
                    for which in 0..5 {
                        let mut f = K2040::new();
                        for b in 0..20 {
                            let mut buf: [f32; BLOCK_SIZE] =
                                core::array::from_fn(|i| grid_input(which, b * BLOCK_SIZE + i));
                            f.process_traced(&mut buf, &p, SR, |_, y| {
                                for &s in y {
                                    max = max.max(s.abs());
                                }
                            });
                        }
                    }
                }
            }
        }
        println!("{v:?}: max |y| {max}, R {R_2040}");
        assert!(max <= R_2040 + 1e-6, "{v:?}: {max}");
        assert!(
            max >= R_2040 * 0.999,
            "{v:?}: the grid never reaches the swing"
        );
    }
}

/// THD of a full-scale 100 Hz sine at RES 0, FREQ 20 kHz and `level`.
fn thd_at(v: K2040Voice, level: f32) -> f64 {
    let y = render(
        &params(v, 20_000.0, 0.0, level),
        (1.0 * FS) as usize,
        sine(1.0, 100.0),
    );
    thd(&y[(0.5 * FS) as usize..], 100.0, FS)
}

#[test]
fn k2040_clip_onset_matches_the_circuits() {
    let rev2 = |l| thd_at(K2040Voice::Rev2, l);
    let exp = |l| thd_at(K2040Voice::Exp, l);
    let (r70, r80, e45, e55) = (rev2(0.70), rev2(0.80), exp(0.45), exp(0.55));
    println!("THD: REV2 0.70 {r70:.4}, 0.80 {r80:.4}; EXP 0.45 {e45:.4}, 0.55 {e55:.4}");
    assert!(r70 < 0.003, "REV2 0.70: {r70}");
    assert!(r80 - r70 >= 0.01, "REV2: {r70} → {r80}");
    assert!(e55 - e45 >= 0.01, "EXP: {e45} → {e55}");
    for v in VOICES {
        let y = render(
            &params(v, 20_000.0, 0.0, 1.0),
            (0.5 * FS) as usize,
            sine(1.0, 100.0),
        );
        let peak = y.iter().fold(0.0f32, |m, y| m.max(y.abs()));
        assert!(peak <= bound(v), "{v:?}: {peak}");
    }
}

/// The 2040's LED reads its own stage clip: EXP at LEVEL ½ is at its swing
/// and lights; REV2 lights from about LEVEL 0.75 (Task 7 review F5).
#[test]
fn k2040_led_reads_the_stage_clip() {
    let clip = |v: K2040Voice, level: f32| {
        let p = params(v, 20_000.0, 0.0, level);
        let mut f = K2040::new();
        let mut most = 0.0f32;
        for b in 0..40 {
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| sine(1.0, 100.0)(b * BLOCK_SIZE + i));
            most = most.max(f.process(&mut buf, &p, SR, &FilterMods::NONE).get());
        }
        most
    };
    let (r50, r70, r80, r100) = (
        clip(K2040Voice::Rev2, 0.5),
        clip(K2040Voice::Rev2, 0.7),
        clip(K2040Voice::Rev2, 0.8),
        clip(K2040Voice::Rev2, 1.0),
    );
    let (e40, e50) = (clip(K2040Voice::Exp, 0.4), clip(K2040Voice::Exp, 0.5));
    println!("LED: REV2 {r50} {r70} {r80} {r100}; EXP 0.4 {e40}, 0.5 {e50}");
    assert_eq!((r50, r70, e40), (0.0, 0.0, 0.0));
    assert!(r80 > 0.0 && r100 > r80 && e50 > 0.0);
}

/// A TYPE change into the 2040 crossfades from a warm start (Task 10):
/// from the fade's end it runs as the 2040 that was always there, within
/// 1 % once its ring has settled; `reset` then rests every type.
#[test]
fn a_type_change_into_the_2040_lands_on_its_own_output() {
    let x = saw(40 * BLOCK_SIZE);
    let mut p = params(K2040Voice::Exp, 700.0, 0.9, 0.6);
    let mut always = Filter::new();
    let mut want = Vec::new();
    for c in x.chunks(BLOCK_SIZE) {
        let mut b: [f32; BLOCK_SIZE] = c.try_into().unwrap();
        always.process(&mut b, &p, SR, &FilterMods::NONE);
        want.extend_from_slice(&b);
    }
    let mut f = Filter::new();
    let mut got = Vec::new();
    for (i, c) in x.chunks(BLOCK_SIZE).enumerate() {
        p.set_kind(if i < 4 {
            FilterKind::Svf
        } else {
            FilterKind::K2040
        });
        let mut b: [f32; BLOCK_SIZE] = c.try_into().unwrap();
        f.process(&mut b, &p, SR, &FilterMods::NONE);
        got.extend_from_slice(&b);
    }
    let tail = 30 * BLOCK_SIZE;
    let peak = want[tail..].iter().fold(0.0f32, |m, y| m.max(y.abs()));
    let err = got[tail..]
        .iter()
        .zip(&want[tail..])
        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(err <= 0.01 * peak, "{err} of {peak}");
    assert!(f.rings());
    f.reset();
    assert!(!f.rings());
}

/// A 2040 voice at RES 1 rings on after its engine ends, then fades over
/// one block without a click, and its next note starts from rest.
#[test]
fn a_2040_voice_ending_mid_oscillation_fades_and_restarts_from_rest() {
    for v in VOICES {
        let mut p = ParamSnapshot::for_engine(EngineType::Algo);
        p.filter.set_kind(FilterKind::K2040);
        (p.filter.cutoff, p.filter.resonance, p.filter.voice) = (500.0, 1.0, v);
        let mut r = Rig::new(SR);
        r.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
        let mut out = Vec::new();
        let mut b = [0.0f32; BLOCK_SIZE];
        let mut ended = false;
        for blk in 0..20_000 {
            if blk == 375 {
                r.note_off();
            }
            r.render(&mut b, &p, &ModState::new());
            out.extend_from_slice(&b);
            if !r.is_active() {
                ended = true;
                break;
            }
        }
        assert!(ended, "{v:?}: never ended");
        let step = |w: &[f32]| {
            w.windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let n = out.len();
        let (ring, end) = (
            step(&out[n - 40 * BLOCK_SIZE..n - BLOCK_SIZE]),
            step(&out[n - BLOCK_SIZE - 1..]),
        );
        println!("{v:?}: ring step {ring:.4}, end step {end:.4}");
        // The amp gate (ADR 0110) has brought the ring down by now.
        assert!(end <= ring * 1.1 + 1e-5, "{v:?}: end {end}, ring {ring}");
        assert!(out[n - 1].abs() <= ring + 1e-5, "{v:?}");
        // The next note's first block is a fresh voice's.
        let first = |r: &mut Rig| {
            let mut b = [0.0f32; BLOCK_SIZE];
            r.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
            r.render(&mut b, &p, &ModState::new());
            b.map(f32::to_bits)
        };
        assert_eq!(first(&mut r), first(&mut Rig::new(SR)), "{v:?}");
    }
}

#[test]
fn sat_2040_is_the_c1_cubic() {
    assert_eq!(sat_2040(0.0), 0.0);
    assert!((sat_2040(1.5) - 1.0).abs() < 1e-6);
    assert_eq!(sat_2040(3.0), sat_2040(1.5));
    assert_eq!(sat_2040(-0.7), -sat_2040(0.7));
    // Slope 1 at 0, 0 at the knee.
    assert!((sat_2040(1e-3) - 1e-3).abs() < 1e-9);
    assert!((sat_2040(1.5) - sat_2040(1.499)).abs() < 1e-5);
    assert_eq!(ALPHA_EXP, 2.0 * ALPHA_REV2);
}

/// EXP's stages bend at the Kobol's circuit scale (ADR 0105): 3.04× REV2's
/// per volt of stage error, derived from the node's resistors. `k2040_ref`
/// runs the same scale per voice, so `k2040_post_is_exp_only` and the jump
/// tests hold the loop to it bit for bit. The bend is quiet (the probe
/// measured at most 0.2 dB on any metric); the retune it forces is the
/// pitch test's (`TRIM_EXP`).
#[test]
fn k2040_exp_stage_bend_is_at_the_circuit_scale() {
    assert!((STAGE_SCALE_EXP - 3.04).abs() < 0.01);
    assert_eq!(K2040Voice::Rev2.stage_scale(), 1.0);
    assert_eq!(K2040Voice::Exp.stage_scale(), STAGE_SCALE_EXP);
}

/// A held saw's fundamental through a fresh 2040, dB: the last 40 periods
/// of 2 s.
fn h1_db(p: &FilterParams, amp: f32, f0: f32) -> f32 {
    let y = render(p, 2 * SR as usize, |i| {
        let t = i as f32 * f0 / FS;
        amp * (2.0 * (t - t.floor()) - 1.0)
    });
    let m = (40.0 * FS / f0) as usize;
    mag_db(&y[y.len() - m..], f0, FS)
}

/// The RSF Kobol Expander's held A1 (its VCO 1, 56.4 Hz), cutoff open:
/// the fundamental against RES 0 at RES knob 5 and max (kobol-session,
/// 2026-10-20), at the EXP RES the comparison matched to each by bump.
const KOBOL_BASS_DB: [(f32, f32); 2] = [(0.4, -3.3), (1.0, -7.0)];

/// ADR 0191: EXP's fundamental follows the Kobol's curve, full scale and
/// 20 dB down: RES 0 untouched, knob 5 and max within 0.5 dB, falling all
/// the way. Before the make-up EXP read −8.3 and −13.9 there.
#[test]
fn k2040_exp_bass_follows_the_kobol() {
    // RES 0: the make-up is exactly 1, so EXP is its loop, bit for bit.
    assert_eq!(make_up(k2040_k(0.0)), 1.0);
    for amp in [1.0f32, 0.1] {
        let at = |res: f32| h1_db(&params(K2040Voice::Exp, 9401.0, res, 0.5), amp, 56.413);
        let base = at(0.0);
        for (res, want) in KOBOL_BASS_DB {
            let got = at(res) - base;
            println!("EXP amp {amp} RES {res}: {got:+.2} dB (Kobol {want:+.1})");
            assert!(
                (got - want).abs() <= 0.5,
                "EXP amp {amp} RES {res}: {got:.2} dB"
            );
        }
        let mut last = 0.0f32;
        for r in 1..=10 {
            let res = r as f32 / 10.0;
            let got = at(res) - base;
            assert!(
                got < last,
                "EXP amp {amp} RES {res}: {got:.2} after {last:.2}"
            );
            last = got;
        }
    }
}

/// The make-up lifts EXP's whole response, so its resonant bump over its
/// bass is the loop's (the Kobol's +5.2 dB at knob 5 is EXP's at RES 0.4):
/// small signal, the peak re 30 Hz, against the linear prototype's at the
/// trimmed FREQ.
#[test]
fn k2040_exp_make_up_keeps_the_bump_over_the_bass() {
    let v = K2040Voice::Exp;
    for fc in [250.0f32, 1000.0, 4000.0] {
        for res in [0.4f32, 0.85] {
            let ft = fc * v.trim();
            let peak_f = (20..8000)
                .map(|f| f as f64)
                .max_by(|a, b| proto_db(ft, res, *a).total_cmp(&proto_db(ft, res, *b)))
                .unwrap() as f32;
            let p = params(v, fc, res, 0.5);
            let got = gain_db(&p, 0.01, peak_f) - gain_db(&p, 0.01, 30.0);
            let want = (proto_db(ft, res, f64::from(peak_f)) - proto_db(ft, res, 30.0)) as f32;
            println!("EXP {fc} Hz RES {res} peak {peak_f} Hz: bump {got:+.2} dB (loop {want:+.2})");
            assert!(
                (got - want).abs() <= 0.5,
                "EXP {fc} RES {res}: {got:.2} vs {want:.2}"
            );
        }
    }
}

/// At RES 1 EXP rings over a full-scale saw, as the Kobol rings over its
/// VCO (+11 dB re the fundamental) and REV2 does (+8): at ¼ its 3080
/// squelched it (−4 dB). FREQ 1 kHz, A1, the ring's strongest line re H1.
#[test]
fn k2040_exp_rings_over_a_full_scale_saw() {
    let f0 = 56.413f32;
    let y = render(
        &params(K2040Voice::Exp, 1000.0, 1.0, 0.5),
        2 * SR as usize,
        |i| {
            let t = i as f32 * f0 / FS;
            (2.0 * (t - t.floor()) - 1.0) + if i == 0 { 0.01 } else { 0.0 }
        },
    );
    let t = &y[y.len() - (40.0 * FS / f0) as usize..];
    let h1 = mag_db(t, f0, FS);
    let ring = (300..2000)
        .map(|f| mag_db(t, f as f32, FS))
        .fold(f32::MIN, f32::max);
    println!("EXP ring re H1: {:+.2} dB", ring - h1);
    assert!(ring - h1 >= 3.0, "EXP ring {:.2} dB re H1", ring - h1);
}

/// REV2 is untouched by EXP's make-up and limit (ADR 0191): its output over
/// held, moving and gliding blocks, every FREQ, RES and LEVEL step, hashed
/// bit for bit, pinned at origin/main 791ce07e, before the change.
#[test]
fn k2040_rev2_is_bit_identical_to_before_the_make_up() {
    let mut p = params(K2040Voice::Rev2, 1000.0, 0.0, 0.5);
    let mut f = K2040::new();
    let (mut h, mut n) = (0xcbf2_9ce4_8422_2325u64, 0usize);
    for b in 0..600usize {
        let t = b / 20;
        (p.cutoff, p.resonance, p.level) = (
            [80.0f32, 400.0, 2000.0, 9000.0, 18000.0][t % 5],
            [0.0f32, 0.3, 0.6, 0.85, 0.95, 1.0][t % 6],
            [0.25f32, 0.5, 0.9][t % 3],
        );
        let mods = FilterMods {
            glide: b % 7 == 3,
            ..FilterMods::NONE
        };
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|_| {
            n += 1;
            let x = (n % 437) as f32 / 218.5 - 1.0;
            if (n / 9000) % 2 == 0 { x } else { 0.1 * x }
        });
        f.process(&mut buf, &p, SR, &mods);
        for y in buf {
            h = (h ^ u64::from(y.to_bits())).wrapping_mul(0x0100_0000_01b3);
        }
    }
    assert_eq!(h, 0x6122_82d7_20ce_15d8);
}
