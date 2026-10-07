//! LADDER, the Moog transistor ladder (ADR 0104): its derived Xpander
//! modes against the linear prototype, its ring, LEVEL as drive, the LED,
//! bounds under audio-rate modulation, TYPE changes and its codes.

mod common;

use chimera_core::block::DiskCode;
use chimera_core::dsp::filter::coef::g_of;
use chimera_core::dsp::filter::k2040::k2040_k;
use chimera_core::dsp::filter::ladder::{TRIM_LADDER, Y1_MAX, ladder_bound, pade_tanh};
use chimera_core::dsp::filter::{
    Filter, FilterKind, FilterMode, FilterMods, Ladder, routed_cutoff,
};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::storage::RecordTag;
use chimera_core::taps::TapLane;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::codec_util::{decode, encode, filter_payload, sound_file as file};
use common::filter_ref::{mag_db, rms_db, thd};

const SR: u32 = 48_000;
const FS: f32 = SR as f32;
const MODES: [FilterMode; 5] = [
    FilterMode::Lp12,
    FilterMode::Lp24,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
];

fn params(mode: FilterMode, fc: f32, res: f32, level: f32) -> FilterParams {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::Ladder);
    assert!(p.set_mode(mode));
    (p.cutoff, p.resonance, p.level) = (fc, res, level);
    p
}

/// `n` samples through a fresh LADDER at a held setting, fed `x(i)`.
fn render(p: &FilterParams, n: usize, x: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut f = Ladder::new();
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

/// Gain at `f` for a sine of `amp`, dB: read over the whole periods in
/// about 0.5 s, after 0.25 s.
fn gain_db(p: &FilterParams, amp: f32, f: f32) -> f32 {
    let skip = (0.25 * FS) as usize;
    let n = ((0.5 * f).round().max(1.0) / f * FS).round() as usize;
    let y = render(p, skip + n, sine(amp, f));
    mag_db(&y[skip..], f, FS) - 20.0 * amp.log10()
}

type C = (f64, f64);
fn mul(a: C, b: C) -> C {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}
fn div(a: C, b: C) -> C {
    let d = b.0 * b.0 + b.1 * b.1;
    ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
}

/// The linear prototype, from the transfer functions, not the code's
/// mixes: four TPT one-poles `H₁ = g(1 + z⁻¹)/((1 + g) − (1 − g)z⁻¹)` under
/// feedback `k`, mode `m`'s numerator over `(1 + s)⁴` read in `H₁ =
/// 1/(1 + s)`, so `s = 1/H₁ − 1`: LP2 `H₁²`, LP4 `H₁⁴`, BP2 `s·H₁⁴·(1 +
/// s)²`, BP4 `s²·H₁⁴`, HP4 `s⁴·H₁⁴`, each over `1 + k·H₁⁴`; times the
/// mode's gain, its RES-0 normalisation (BP2 ×2, BP4 ×4) lerped in `k` to
/// its ring's (LP2 and BP2 ×½, BP4 ×1) at the onset.
fn proto_db(m: FilterMode, fc: f32, res: f32, f: f64) -> f64 {
    let g = f64::from(g_of(fc * TRIM_LADDER, FS));
    let k = f64::from(k2040_k(res));
    let w = core::f64::consts::TAU * f / f64::from(FS);
    let (c, s) = (w.cos(), -w.sin());
    let h1 = div(
        (g * (1.0 + c), g * s),
        ((1.0 + g) - (1.0 - g) * c, -(1.0 - g) * s),
    );
    let sv = div((1.0, 0.0), h1);
    let sv = (sv.0 - 1.0, sv.1);
    let h2 = mul(h1, h1);
    let h4 = mul(h2, h2);
    let (num, g0, g4) = match m {
        FilterMode::Lp12 => (h2, 1.0, 0.5),
        FilterMode::Lp24 => (h4, 1.0, 1.0),
        FilterMode::Bp12 => (mul(sv, h2), 2.0, 0.5),
        FilterMode::Bp24 => (mul(mul(sv, sv), h4), 4.0, 1.0),
        _ => (mul(mul(mul(sv, sv), mul(sv, sv)), h4), 1.0, 1.0),
    };
    let norm = g0 + (g4 - g0) * (k / 4.0).min(1.0);
    let h = div(num, (1.0 + k * h4.0, k * h4.1));
    20.0 * (norm * h.0.hypot(h.1)).log10()
}

/// The mixes are derived (ADR 0104), so each mode's small-signal response
/// is held to the prototype at RES 0 and ½, within 0.3 dB, at ⅛ … 8 ×
/// FREQ; and to its shape: the low-passes 0 dB in the pass band and −12
/// (LP2) or −24 dB/oct (LP4) over 4–8 × FREQ, HP4 +24 dB/oct over ⅛–¼ ×
/// FREQ and 0 dB from 16 × FREQ, the band-passes peaking at FREQ (RES 0) at 0 dB,
/// ±6 (BP2) or ±12 dB/oct (BP4) on each skirt, slopes read an octave
/// further out than 2–4 × (the four-pole is 2.7 dB short of its asymptote
/// there).
#[test]
fn modes_respond_as_derived() {
    let amp = 0.01;
    for fc in [250.0f32, 1000.0, 4000.0] {
        for m in MODES {
            for res in [0.0f32, 0.5] {
                let p = params(m, fc, res, 0.5);
                for r in [0.125f32, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0] {
                    let f = fc * r;
                    if f > 0.4 * FS {
                        continue;
                    }
                    let want = proto_db(m, fc, res, f64::from(f)) as f32;
                    if want < -70.0 {
                        continue;
                    }
                    let got = gain_db(&p, amp, f);
                    assert!(
                        (got - want).abs() <= 0.3,
                        "{m:?} {fc} Hz RES {res} at {r}×: {got:.2} vs {want:.2} dB"
                    );
                }
            }
            let p = params(m, fc, 0.0, 0.5);
            let at = |r: f32| gain_db(&p, amp, fc * r);
            let slope = |a: f32, b: f32| at(b) - at(a);
            match m {
                FilterMode::Lp12 | FilterMode::Lp24 => {
                    let want = if m == FilterMode::Lp12 { -12.0 } else { -24.0 };
                    assert!(at(0.0625).abs() <= 0.1, "{m:?} {fc}: pass band");
                    if fc * 8.0 < 0.2 * FS {
                        let s = slope(4.0, 8.0);
                        assert!((s - want).abs() <= 2.5, "{m:?} {fc}: {s:.1} dB/oct");
                    }
                }
                FilterMode::Hp24 => {
                    let s = slope(0.125, 0.25);
                    assert!((s - 24.0).abs() <= 3.0, "HP4 {fc}: {s:.1} dB/oct");
                    if fc * 16.0 < 0.2 * FS {
                        assert!(at(16.0).abs() <= 0.15, "HP4 {fc}: pass band");
                    }
                }
                _ => {
                    let want = if m == FilterMode::Bp12 { 6.0 } else { 12.0 };
                    let peak = at(1.0);
                    assert!(peak.abs() <= 0.1, "{m:?} {fc}: peak {peak:.2} dB");
                    let top = (85..=115)
                        .map(|i| i as f32 / 100.0)
                        .max_by(|&a, &b| at(a).total_cmp(&at(b)))
                        .unwrap();
                    assert!((top - 1.0).abs() <= 0.03, "{m:?} {fc}: peak at {top}×");
                    let lo = slope(0.0625, 0.125);
                    assert!((lo - want).abs() <= 1.5, "{m:?} {fc}: low skirt {lo:.1}");
                    if fc * 16.0 < 0.2 * FS {
                        let hi = slope(8.0, 16.0);
                        assert!((hi + want).abs() <= 1.5, "{m:?} {fc}: high skirt {hi:.1}");
                    }
                }
            }
        }
    }
}

/// The Moog's bass loss, uncompensated (owner's ruling): LP4 at 100 Hz
/// with FREQ 2 kHz follows `−20·log₁₀(1 + k)` within 0.5 dB at small
/// signal, RES 0 to 1 in tenths, falling monotonically.
#[test]
fn ladder_bass_loss_is_the_circuits() {
    let mut last = f32::INFINITY;
    for t in 0..=10 {
        let res = t as f32 / 10.0;
        let got = gain_db(&params(FilterMode::Lp24, 2000.0, res, 0.5), 0.01, 100.0);
        let want = -20.0 * (1.0 + k2040_k(res)).log10();
        assert!(
            (got - want).abs() <= 0.5,
            "RES {res}: {got:.2} vs {want:.2}"
        );
        assert!(got < last + 1e-3, "RES {res}: rises");
        last = got;
    }
}

/// LP4 at LEVEL ½, RES 0 is unity in the pass band, at small signal and on
/// a full-scale sine (the input pair bends it 3 % there).
#[test]
fn ladder_unity_at_level_half_res_0() {
    for amp in [0.01f32, 1.0] {
        let g = gain_db(&params(FilterMode::Lp24, 8000.0, 0.0, 0.5), amp, 100.0);
        assert!(g.abs() <= 0.3, "{amp}: {g:.2} dB");
    }
}

fn impulse(i: usize) -> f32 {
    if i == 0 { 1.0 } else { 0.0 }
}

/// RES 1 from one impulse, read over 0.5 s once grown in.
fn oscillation(m: FilterMode, fc: f32) -> Vec<f32> {
    let settle = (0.5 * FS).max(200.0 * FS / fc) as usize;
    render(
        &params(m, fc, 1.0, 0.5),
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

/// LADDER's ring at RES 1, dBFS RMS (−15.5 to −16.1 measured, every mode
/// and FREQ; ± 1 dB): the input pair's level, its own band as EXP's is
/// (ADR 0104). The modes' gains bring each within 0.5 dB of LP4's.
const LADDER_OSC_DB: f32 = -15.6;
/// Absolute pitch at RES 1, cents, over C2–C6 and FREQ 100 Hz–5 kHz.
const PITCH_CENTS: f64 = 5.0;

#[test]
fn ladder_self_oscillates_in_band_and_on_pitch() {
    for m in MODES {
        for fc in [100.0f32, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0] {
            let y = oscillation(m, fc);
            let level = rms_db(&y);
            let wobble = y
                .chunks(4800)
                .map(|w| (rms_db(w) - level).abs())
                .fold(0.0f32, f32::max);
            println!("{m:?} RES 1 {fc} Hz: {level:.2} dBFS ±{wobble:.2}");
            assert!(wobble <= 1.0, "{m:?} {fc} Hz: ±{wobble:.2} dB");
            assert!(
                (level - LADDER_OSC_DB).abs() <= 1.0,
                "{m:?} {fc} Hz: {level:.2} dBFS"
            );
        }
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
            let f = pitch(&oscillation(FilterMode::Lp24, fc));
            let c = cents(f, want);
            println!("base {base} note {note}: {c:.1} cents");
            assert!(c.abs() <= PITCH_CENTS, "base {base} note {note}: {c:.1}");
            if let Some(l) = last {
                let o = cents(f, l) - 1200.0;
                assert!(o.abs() <= 5.0, "base {base} note {note}: octave {o:.1}");
            }
            last = Some(f);
        }
    }
    // The onset, RES 0.95 ± 0.01.
    let n = (2.5 * FS) as usize;
    for fc in [250.0, 1000.0, 4000.0] {
        let at = |res: f32| {
            let y = render(&params(FilterMode::Lp24, fc, res, 0.5), n, impulse);
            let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let late = y[n - (0.1 * FS) as usize..]
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs()));
            20.0 * (late / peak).log10()
        };
        let (below, above) = (at(0.94), at(0.96));
        assert!(below < -60.0, "{fc} Hz RES 0.94: {below:.1} dB");
        assert!(above > -20.0, "{fc} Hz RES 0.96: {above:.1} dB");
    }
}

/// LEVEL drives the ladder: `F(2x) ≠ 2F(x)`. A 200 Hz sine through LP4 at
/// FREQ 2 kHz, RES ½: at LEVEL 0 (×¼) it is clean, each LEVEL step of ¼
/// (×2) raises the THD, by at least 10 dB from ½ to 1, and the
/// output at ×2 differs from twice the output at ×1.
#[test]
fn level_changes_the_harmonics() {
    let thd_at = |level: f32| {
        let y = render(
            &params(FilterMode::Lp24, 2000.0, 0.5, level),
            (0.75 * FS) as usize,
            sine(1.0, 200.0),
        );
        20.0 * thd(&y[(0.25 * FS) as usize..], 200.0, FS).log10()
    };
    let t: Vec<f64> = [0.0f32, 0.25, 0.5, 0.75, 1.0].map(thd_at).to_vec();
    println!("THD by LEVEL 0..1: {t:.1?} dB");
    assert!(t[0] < -50.0, "LEVEL 0: {:.1}", t[0]);
    assert!(t.windows(2).all(|w| w[1] > w[0]), "{t:?}");
    assert!(t[4] - t[2] >= 10.0, "{t:?}");
    let y1 = render(
        &params(FilterMode::Lp24, 2000.0, 0.5, 0.5),
        9600,
        sine(1.0, 200.0),
    );
    let y2 = render(
        &params(FilterMode::Lp24, 2000.0, 0.5, 0.75),
        9600,
        sine(1.0, 200.0),
    );
    let rel = common::rms_diff(
        &y2[4800..],
        &y1[4800..].iter().map(|v| 2.0 * v).collect::<Vec<_>>(),
    ) / common::rms(&y2[4800..]);
    println!("F(2x) vs 2F(x): {:.1} dB apart", 20.0 * rel.log10());
    assert!(rel > 0.003, "{rel}");
}

/// The LED reads the input pair: off at LEVEL ½ on a full-scale sine (u
/// peaks at α), lit at LEVEL 1 (×4 past the knee of 1), rising.
#[test]
fn ladder_led_reads_the_input_pair() {
    let clip = |level: f32| {
        let p = params(FilterMode::Lp24, 20_000.0, 0.0, level);
        let mut f = Ladder::new();
        let mut most = 0.0f32;
        for b in 0..40 {
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| sine(1.0, 100.0)(b * BLOCK_SIZE + i));
            most = most.max(f.process(&mut buf, &p, SR, &FilterMods::NONE).get());
        }
        most
    };
    let (l50, l75, l90, l100) = (clip(0.5), clip(0.75), clip(0.9), clip(1.0));
    println!("LED: {l50} {l75} {l90} {l100}");
    assert_eq!((l50, l75), (0.0, 0.0));
    assert!(l90 > 0.0 && l100 > l90);
}

/// Audio-rate FREQ and Q taps at full depth, random FREQ, RES and LEVEL
/// each block, on white noise and ±10 impulses: every sample finite and
/// within the mode's bound, every state within its swing plus `2g`.
#[test]
fn ladder_stays_bounded_under_audio_rate_modulation() {
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / (1u32 << 24) as f32
    };
    for m in MODES {
        let mut f = Ladder::new();
        let lim = ladder_bound(m, 10.0);
        let mut p = params(m, 1000.0, 0.5, 0.5);
        for b in 0..1500 {
            (p.cutoff, p.resonance, p.level) = (20.0 * 1000f32.powf(next()), next(), next());
            let rows: [[f32; BLOCK_SIZE]; 2] =
                core::array::from_fn(|_| core::array::from_fn(|_| 2.0 * next() - 1.0));
            let amt = [2.0 * next() - 1.0, 2.0 * next() - 1.0];
            let lane = TapLane {
                rows: &rows,
                from: amt,
                to: [2.0 * next() - 1.0, 2.0 * next() - 1.0],
            };
            let mods = FilterMods {
                freq: Some(lane),
                q: Some(lane),
                level_routed: false,
            };
            let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                if (b * BLOCK_SIZE + i).is_multiple_of(4001) {
                    if next() < 0.5 { 10.0 } else { -10.0 }
                } else {
                    2.0 * next() - 1.0
                }
            });
            f.process(&mut buf, &p, SR, &mods);
            for &v in &buf {
                assert!(
                    v.is_finite() && v.abs() <= lim,
                    "{m:?} block {b}: {v} past {lim}"
                );
            }
            let s = f.states();
            let top = Y1_MAX + 4.0 * g_of(FS, FS);
            assert!(s.iter().all(|v| v.abs() <= top), "{m:?} block {b}: {s:?}");
        }
    }
}

/// A NaN block leaves no trace: the next block starts from rest.
#[test]
fn ladder_recovers_from_a_nan_block() {
    let p = params(FilterMode::Lp24, 1000.0, 0.9, 0.5);
    let mut f = Ladder::new();
    let mut b = [f32::NAN; BLOCK_SIZE];
    f.process(&mut b, &p, SR, &FilterMods::NONE);
    let mut b: [f32; BLOCK_SIZE] = core::array::from_fn(|i| sine(0.5, 200.0)(i));
    f.process(&mut b, &p, SR, &FilterMods::NONE);
    f.process(&mut b, &p, SR, &FilterMods::NONE);
    assert!(b.iter().all(|y| y.is_finite()) && b.iter().any(|&y| y != 0.0));
}

fn saw(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.8 * (2.0 * ((i as f32 * 110.0 / FS) % 1.0) - 1.0))
        .collect()
}

/// A TYPE change into LADDER, from each other type, crossfades from a warm
/// start: from the fade's end it runs as the LADDER that was always there,
/// within 1 % once its ring has settled (RES 0.9 rings on: 1.1 % at 30
/// blocks, so read from 50). From the 2040, whose state slot it
/// takes, the 2040 plays out the fade on its carried stages.
#[test]
fn a_type_change_into_ladder_lands_on_its_own_output() {
    for from in [FilterKind::Svf, FilterKind::K2040, FilterKind::Dirty] {
        let x = saw(60 * BLOCK_SIZE);
        let mut p = params(FilterMode::Lp24, 700.0, 0.9, 0.6);
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
            p.set_kind(if i < 6 { from } else { FilterKind::Ladder });
            assert!(p.mode() == FilterMode::Lp24 || p.set_mode(FilterMode::Lp24) || i < 6);
            let mut b: [f32; BLOCK_SIZE] = c.try_into().unwrap();
            f.process(&mut b, &p, SR, &FilterMods::NONE);
            got.extend_from_slice(&b);
        }
        assert!(
            common::clicks(&got).is_empty(),
            "{from:?}: {:?}",
            common::clicks(&got)
        );
        let tail = 50 * BLOCK_SIZE;
        let peak = want[tail..].iter().fold(0.0f32, |m, y| m.max(y.abs()));
        let err = got[tail..]
            .iter()
            .zip(&want[tail..])
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        println!("{from:?} → LADDER: {:.2} % off", 100.0 * err / peak);
        assert!(err <= 0.01 * peak, "{from:?}: {err} of {peak}");
        assert!(f.rings());
        f.reset();
        assert!(!f.rings());
    }
}

/// LADDER ↔ the 2040 share one state slot; each way, held mid-note on a
/// resonant saw, the fade neither clicks nor steps past the louder side.
#[test]
fn ladder_and_2040_trade_their_slot_without_a_click() {
    for (a, b) in [
        (FilterKind::K2040, FilterKind::Ladder),
        (FilterKind::Ladder, FilterKind::K2040),
    ] {
        let x = saw(60 * BLOCK_SIZE);
        let mut p = params(FilterMode::Lp24, 900.0, 0.85, 0.7);
        let mut f = Filter::new();
        let mut out = Vec::new();
        for (i, c) in x.chunks(BLOCK_SIZE).enumerate() {
            p.set_kind(if (i / 10) % 2 == 0 { a } else { b });
            let mut buf: [f32; BLOCK_SIZE] = c.try_into().unwrap();
            f.process(&mut buf, &p, SR, &FilterMods::NONE);
            out.extend_from_slice(&buf);
        }
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(
            common::clicks(&out).is_empty(),
            "{a:?}→{b:?}: {:?}",
            common::clicks(&out)
        );
    }
}

/// A LADDER voice at RES 1 rings after its engine ends, then fades without
/// a click, and its next note starts from rest.
#[test]
fn a_ladder_voice_ending_mid_oscillation_fades_and_restarts_from_rest() {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.set_kind(FilterKind::Ladder);
    (p.filter.cutoff, p.filter.resonance) = (500.0, 1.0);
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
    assert!(ended, "never ended");
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
    assert!(ring > 0.001, "no ring ({ring})");
    assert!(end <= ring * 1.1, "end {end}, ring {ring}");
    let first = |r: &mut Rig| {
        let mut b = [0.0f32; BLOCK_SIZE];
        r.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
        r.render(&mut b, &p, &ModState::new());
        b.map(f32::to_bits)
    };
    assert_eq!(first(&mut r), first(&mut Rig::new(SR)));
}

/// KIND 1 is LADDER, ident "LADDER", its five modes in CLEAN's order,
/// LP4 the default; each mode round-trips, a mode it lacks loads as LP4.
#[test]
fn ladder_codes_round_trip() {
    let k = FilterKind::Ladder;
    assert_eq!((k.disk_code(), k.disk_ident()), (1, "LADDER"));
    assert_eq!(FilterKind::from_disk_code(1), Some(k));
    assert_eq!(k.modes(), MODES);
    assert_eq!(k.default_mode(), FilterMode::Lp24);
    assert_eq!(k.mode_labels(), ["LP2", "LP4", "BP2", "BP4", "HP4"]);
    for m in MODES {
        let mut s = Sound::init(EngineType::Algo);
        s.params.filter.set_kind(k);
        assert!(s.params.filter.set_mode(m));
        let d = decode(&encode(&s)).unwrap();
        assert!(d.bits_eq(&s), "{m:?}");
        assert_eq!((d.params.filter.kind(), d.params.filter.mode()), (k, m));
    }
    let engine = (RecordTag::Engine, vec![EngineType::Algo.disk_code()]);
    let load = |e: &[(u8, [u8; 4])]| {
        let block = (RecordTag::Block, filter_payload(e));
        let f = decode(&file(&[engine.clone(), block]))
            .unwrap()
            .params
            .filter;
        (f.kind(), f.mode())
    };
    let code = |c: u32| c.to_le_bytes();
    // NOTCH, PHASER, LP6 and HP12 aren't LADDER's: LP4.
    for mode in [6u32, 7, 0, 9, 255] {
        assert_eq!(
            load(&[(6, code(1)), (7, code(mode))]),
            (k, FilterMode::Lp24),
            "MODE {mode}"
        );
    }
}

/// The Padé 3/2 tanh: within 0.024 of tanh, odd, monotone, flat at ±1
/// from ±3, so `Y1_MAX` is its inverse of ±1.
#[test]
fn pade_tanh_is_close_and_bounded() {
    let mut x = -5.0f32;
    let mut last = -2.0f32;
    while x < 5.0 {
        let y = pade_tanh(x);
        assert!((y - x.tanh()).abs() <= 0.024, "{x}: {y}");
        assert_eq!(pade_tanh(-x), -y);
        assert!(y >= last && y.abs() <= 1.0, "{x}");
        last = y;
        x += 0.0173;
    }
    assert_eq!(pade_tanh(Y1_MAX), 1.0);
    assert_eq!(pade_tanh(7.0), 1.0);
}
