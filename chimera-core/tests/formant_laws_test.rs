//! The formant grain's pure laws (spec § 3): the window, its SKIRT
//! constants and curves, measured here from the window's definition and
//! pinned against `formant.rs`.

mod common;

use chimera_core::dsp::algo::formant::{
    BRACKET_FLOOR, DELAY_SLEW, NYQUIST_HZ, R_REF, Skirt, cycles_to_phase, formant_law,
    formant_law_pm, grain_delay, lane_peak, pm_reach, rho, width_floor, window,
};
use chimera_core::dsp::algo::kinds::{Kinds, OpKind};
use chimera_core::dsp::algo::noise::{
    NOISE_CLAMP, NOISE_GAIN, noise_coef, noise_edge, noise_seed, res_mix,
};
use chimera_core::dsp::algo::vowel::{
    Formant, VOWEL_HZ, VOWEL_OCT, VowelSlot, fc_hz, resolve_slots, vowel_delta,
};
use chimera_core::dsp::algo::waves::WaveId;
use common::SR;
use common::formant::{grain_fc, off_grid_db, one, render_algo, spectrum_db, synth_level};
use std::f64::consts::TAU;

/// Midpoint integration over the window's length.
const POINTS: usize = 4000;
/// The ν scan's step, in cycles per window length.
const STEP: f64 = 0.005;
/// The curves' grids: ρ over ν = 2·FC·L in [0, 12], p over FC·L in [0, 16].
const RHO_NODES: usize = 49;
const PEAK_NODES: usize = 65;
const NODE: f64 = 0.25;

fn w(x: f64, q: u32) -> f64 {
    (4.0 * x * (1.0 - x)).powi(q as i32)
}

fn midpoints() -> impl Iterator<Item = f64> {
    (0..POINTS).map(|j| (j as f64 + 0.5) / POINTS as f64)
}

/// `∫ g(x)·cos(2πf(x − ½)) dx` over the window: real, as `g` is symmetric.
fn transform(g: &[f64], f: f64) -> f64 {
    midpoints()
        .zip(g)
        .map(|(x, &v)| v * (TAU * f * (x - 0.5)).cos())
        .sum::<f64>()
        / POINTS as f64
}

fn closed_form_c_q(q: u32) -> f64 {
    let fact = |n: u32| (1..=n).map(f64::from).product::<f64>();
    4f64.powi(2 * q as i32) * fact(2 * q).powi(2) / fact(4 * q + 1)
}

/// The single-lane peak of `W·sin` with the carrier's zero at the centre:
/// a 4,000-point scan, refined by a parabola through the largest sample.
fn true_lane_peak(s: Skirt, fcl: f32) -> f32 {
    let q = s.q();
    let y = |x: f64| (w(x, q) * (TAU * f64::from(fcl) * (x - 0.5)).sin()).abs();
    let h = 1.0 / POINTS as f64;
    let (j, _) = (1..POINTS)
        .map(|j| (j, y(j as f64 * h)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    let x = j as f64 * h;
    let (a, b, c) = (y(x - h), y(x), y(x + h));
    let d = a - 2.0 * b + c;
    let peak = if d < 0.0 {
        b - (a - c).powi(2) / (8.0 * d)
    } else {
        b
    };
    peak as f32
}

struct Measured {
    /// −3 dB (half-power) full width, in cycles per window length.
    k: f64,
    c_q: f64,
    /// The −60 dB upper extent from the centre, in cycles per window length.
    m60: f64,
    sidelobe_db: f64,
    rho_grid: Vec<(f32, f32)>,
    peak_grid: Vec<(f32, f32)>,
}

/// The crossing of `level` between scan points `f − STEP` and `f`.
fn cross(f: f64, before: f64, after: f64, level: f64) -> f64 {
    f - STEP + STEP * (before - level) / (before - after)
}

fn measure(s: Skirt) -> Measured {
    let q = s.q();
    let g: Vec<f64> = midpoints().map(|x| w(x, q)).collect();
    let g2: Vec<f64> = g.iter().map(|v| v * v).collect();
    let t0 = transform(&g, 0.0);
    let scan: Vec<f64> = (0..8000)
        .map(|i| transform(&g, i as f64 * STEP) / t0)
        .collect();
    let at = |i: usize| i as f64 * STEP;
    let half = 0.5f64.sqrt();
    let i3 = scan.iter().position(|&v| v < half).unwrap();
    let k = 2.0 * cross(at(i3), scan[i3 - 1], scan[i3], half);
    let null = scan.iter().position(|&v| v < 0.0).unwrap();
    let null2 = null + scan[null..].iter().position(|&v| v > 0.0).unwrap();
    let side = scan[null..null2].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let i60 = scan.iter().rposition(|v| v.abs() >= 1e-3).unwrap() + 1;
    let m60 = cross(at(i60), scan[i60 - 1].abs(), scan[i60].abs(), 1e-3);
    let e = g2.iter().sum::<f64>();
    let rho_at = |nu: f64| transform(&g2, nu) * POINTS as f64 / e;
    Measured {
        k,
        c_q: e / POINTS as f64,
        m60,
        sidelobe_db: 20.0 * side.log10(),
        rho_grid: (0..RHO_NODES)
            .map(|i| {
                let nu = i as f64 * NODE;
                (nu as f32, rho_at(nu) as f32)
            })
            .collect(),
        peak_grid: never_under(s),
    }
}

/// The p nodes a linear read never reads under the true peak through: each
/// is the true peak raised by the most either neighbouring segment's read
/// falls short of it (on 50 points a segment).
fn never_under(s: Skirt) -> Vec<(f32, f32)> {
    const FINE: usize = 50;
    let at = |i: usize| (i as f64 * NODE) as f32;
    let p: Vec<f64> = (0..PEAK_NODES)
        .map(|i| f64::from(true_lane_peak(s, at(i))))
        .collect();
    let short: Vec<f64> = (0..PEAK_NODES - 1)
        .map(|i| {
            (1..FINE)
                .map(|j| {
                    let u = j as f64 / FINE as f64;
                    let f = ((i as f64 + u) * NODE) as f32;
                    let d = f64::from(true_lane_peak(s, f)) - (p[i] + (p[i + 1] - p[i]) * u);
                    // Node 0 stays 0, so node 1 alone covers segment 0.
                    if i == 0 { d / u } else { d }
                })
                .fold(0.0, f64::max)
        })
        .collect();
    (0..PEAK_NODES)
        .map(|i| {
            let raise = match i {
                0 => 0.0,
                _ if i == PEAK_NODES - 1 => short[i - 1],
                _ => short[i - 1].max(short[i]),
            };
            (at(i), (p[i] + raise).min(1.0) as f32)
        })
        .collect()
}

#[test]
fn window_is_the_polynomial_bell() {
    for s in Skirt::ALL {
        assert_eq!(window(0.5, s), 1.0);
        assert_eq!(
            [
                window(0.0, s),
                window(1.0, s),
                window(-0.2, s),
                window(1.3, s)
            ],
            [0.0; 4]
        );
        assert!((window(0.25, s) - 0.75f32.powi(s.q() as i32)).abs() < 1e-6);
        assert_eq!(window(f32::NAN, s), 0.0);
    }
}

#[test]
fn skirt_codes_and_exponents() {
    assert_eq!(Skirt::ALL.map(Skirt::q), [2, 4, 8, 16]);
    assert_eq!(
        [0u8, 1, 2, 3, 9].map(Skirt::from_code),
        [Skirt::S0, Skirt::S1, Skirt::S2, Skirt::S3, Skirt::S3]
    );
}

#[test]
fn laws_are_pinned() {
    for s in Skirt::ALL {
        let m = measure(s);
        assert!(
            (m.k / f64::from(s.k()) - 1.0).abs() < 0.005,
            "{s:?} K {}",
            m.k
        );
        assert!(
            (m.c_q - closed_form_c_q(s.q())).abs() < 1e-5
                && (f64::from(s.c_q()) - m.c_q).abs() < 1e-5,
            "{s:?} c_q {}",
            m.c_q
        );
        assert!(
            (m.m60 / f64::from(s.k()) / f64::from(s.m_q()) - 1.0).abs() < 0.005,
            "{s:?} m60 {}",
            m.m60
        );
        assert!(
            (m.sidelobe_db - [-27.7, -38.5, -56.4, -87.0][s as usize]).abs() < 0.3,
            "{s:?} sidelobe {}",
            m.sidelobe_db
        );
        for (nu, want) in m.rho_grid {
            assert!(
                (rho(s, nu) - want).abs() < 1e-4 + 0.005 * want.abs(),
                "{s:?} ρ({nu}) {want}"
            );
        }
        for (fcl, want) in m.peak_grid {
            assert!(
                (lane_peak(s, fcl) - want).abs() < 1e-4 + 0.005 * want,
                "{s:?} p({fcl}) {want}"
            );
        }
    }
}

#[test]
fn curves_are_safe_past_the_table() {
    for s in Skirt::ALL {
        assert_eq!((rho(s, 12.5), lane_peak(s, 16.5)), (0.0, 1.0));
        assert_eq!((rho(s, f32::NAN), lane_peak(s, f32::NAN)), (0.0, 1.0));
        for i in 0..4000 {
            let fcl = i as f32 * 0.01; // to 40
            assert!(
                lane_peak(s, fcl) >= true_lane_peak(s, fcl) - 1e-4,
                "{s:?} {fcl}"
            );
        }
    }
}

#[test]
fn cycles_to_phase_wraps() {
    assert_eq!(cycles_to_phase(7.25), 0x4000_0000);
    assert_eq!(cycles_to_phase(-3.25), 0xC000_0000);
    assert_eq!(cycles_to_phase(0.0), 0);
    assert!(cycles_to_phase(1e6 + 0.5).abs_diff(0x8000_0000) < 1 << 12);
    assert_eq!(cycles_to_phase(-1e-10), 0);
    assert_eq!(
        [1e20, -1e20, f32::INFINITY, f32::NAN].map(cycles_to_phase),
        [0; 4]
    );
}

/// A grain train at `fc` Hz, one window of `len` samples per `period`.
fn train(fc: f32, period: usize, len: usize, s: Skirt) -> Vec<f32> {
    let sr = common::SR as f32;
    (0..48_000)
        .map(|n| {
            let t = (n % period) as f32;
            let x = t / len as f32;
            window(x, s) * (TAU as f32 * fc * (t - len as f32 / 2.0) / sr).sin()
        })
        .collect()
}

#[test]
fn the_helpers_read_a_known_train() {
    // At len 130 SKIRT 0 the edge lobes top 1 %, either side of the gap.
    for (len, s) in [(150, Skirt::S1), (130, Skirt::S0)] {
        let fc = grain_fc(&train(1000.0, 218, len, s));
        assert!((fc / 1000.0 - 1.0).abs() < 0.01, "{len} {s:?} {fc}");
    }
    let out = train(1000.0, 218, 150, Skirt::S1);
    let grid: Vec<f32> = (1..)
        .map(|k| k as f32 * common::SR as f32 / 218.0)
        .take_while(|&h| h < 24_000.0)
        .collect();
    assert!(
        off_grid_db(&out, &grid) < -80.0,
        "{}",
        off_grid_db(&out, &grid)
    );
    let spec = spectrum_db(&out, 8192);
    assert_eq!(spec.len(), 4096);
    assert_eq!(spec.iter().cloned().fold(f32::MIN, f32::max), 0.0);
    let sine = render_algo(&one(OpKind::Table(WaveId::W1), 80, 72, 1), 57, 40);
    assert_eq!(sine.len(), 40 * chimera_hal::BLOCK_SIZE);
    assert!(common::rms(&sine) > 0.1);
}

const C1: f32 = 32.70;
const C2: f32 = 65.41;
const C3: f32 = 130.81;
const A3: f32 = 220.0;
const C5: f32 = 523.25;

#[test]
fn width_floor_is_skirt_free() {
    assert!((width_floor(A3) - 343.3).abs() < 0.5);
    assert!((width_floor(98.0) - 152.9).abs() < 0.5);
    for s in Skirt::ALL {
        assert!((formant_law(A3, 1000.0, 20.0, s).bw_eff - 343.9).abs() < 0.5);
    }
}

#[test]
fn a_window_never_exceeds_two_periods() {
    for (s, dmax) in Skirt::ALL.into_iter().zip([0.881, 1.111, 1.468, 2.0]) {
        for f0 in [33.0, 110.0, 440.0, 2093.0] {
            assert!(formant_law(f0, 1000.0, 0.0, s).d <= dmax + 1e-3);
        }
    }
}

#[test]
fn rms_gain_uses_the_exact_energy() {
    for s in Skirt::ALL {
        // Past 2 kHz at C3 the ceiling binds: spec § 3's level-held domain.
        for bw in [524.0, 1000.0, 1500.0, 2000.0] {
            assert!(synth_level(C3, 1000.0, bw, s).0.abs() < 0.02, "{s:?} {bw}");
        }
    }
    // +1.56 dB with the curves flat past 4.
    assert!(synth_level(C3, 1000.0, 524.0, Skirt::S3).0.abs() < 0.02);
    assert!(synth_level(C5, 270.0, 160.0, Skirt::S1).0.abs() < 0.02);
    assert!(synth_level(C5, 300.0, 160.0, Skirt::S3).0.abs() < 0.02);
    let l = formant_law(C3, 1000.0, 4000.0, Skirt::S1);
    let draft = 1.0 / (l.d * Skirt::S1.c_q()).sqrt() * R_REF;
    // The first draft's law was 7.47 dB off the exact gain here; g_c is the
    // ceiling, so it compares against the uncapped gain.
    let bracket = (1.0 - rho(Skirt::S1, 2.0 * l.fc * l.d / C3)).max(BRACKET_FLOOR);
    let exact = R_REF / (l.d * Skirt::S1.c_q() * bracket).sqrt();
    assert!((20.0 * (draft / exact).log10()).abs() > 6.0);
}

#[test]
fn the_ceiling_bounds_the_peak() {
    for f0 in [C1, C2, C3, 261.6, C5, 1046.5, 2093.0] {
        for fc in [270.0, 730.0, 1000.0, 2440.0, 8000.0, 12_800.0] {
            for bw in [20.0, 160.0, 2000.0, 10_000.0] {
                for s in Skirt::ALL {
                    assert!(
                        synth_level(f0, fc, bw, s).1 <= 1.0 + 1e-3,
                        "{f0} {fc} {bw} {s:?}"
                    );
                }
            }
        }
    }
    // 1.22 with p̂ flat past 4.
    assert!(synth_level(C1, 12_800.0, 2000.0, Skirt::S3).1 <= 1.0 + 1e-3);
    // The edge law narrows BW_eff to 8,742 Hz (−5.83 unnarrowed); p̂ sits
    // 0.75 % over the true peak at FC·L 0.198 (never under).
    let (l, pk) = synth_level(C3, 1000.0, 10_000.0, Skirt::S1);
    assert!(
        (l + 5.32).abs() < 0.1 && (pk - 1.0).abs() < 0.01,
        "{l} {pk}"
    );
    let l = synth_level(C2, 1000.0, 2000.0, Skirt::S1).0;
    assert!((l + 2.23).abs() < 0.5, "{l}");
    let pk = synth_level(C3, 1000.0, 1000.0, Skirt::S1).1;
    assert!((pk - 0.746).abs() < 0.02, "{pk}");
    let pk = synth_level(C5, 270.0, 160.0, Skirt::S1).1;
    assert!((pk - 0.287).abs() < 0.02, "{pk}");
}

#[test]
fn the_edge_narrows_then_fades() {
    let m = Skirt::S0.m_q();
    // (24 kHz − 8 kHz)/m_q: 2,894 at the measured m_q (2,914 at the
    // first draft's 5.49).
    let l = formant_law(C3, 8000.0, 10_000.0, Skirt::S0);
    assert!((l.bw_eff - (NYQUIST_HZ - 8000.0) / m).abs() < 5.0 && l.edge == 1.0);
    let b = width_floor(C3);
    assert!(
        formant_law(C3, NYQUIST_HZ - m * b / 2.0, 0.0, Skirt::S0)
            .edge
            .abs()
            < 1e-6
    );
    assert!((formant_law(C3, NYQUIST_HZ - m * 0.75 * b, 0.0, Skirt::S0).edge - 0.5).abs() < 0.01);
}

/// PM's reach is the plain deviation levered by the grain delay, `|FC/f0 −
/// 1|`, never under 1 (the clamped delay leaves the carrier its own PM).
#[test]
fn pm_reach_levers_the_deviation() {
    assert_eq!(pm_reach(0.0, 220.0, 9600.0), 0.0);
    assert!((pm_reach(100.0, 220.0, 2200.0) - 900.0).abs() < 1e-2);
    assert_eq!(pm_reach(100.0, 220.0, 330.0), 100.0);
    assert_eq!(pm_reach(100.0, 440.0, 220.0), 100.0);
}

/// Under PM the links' share `pm` keeps `FC + m_q·B_min + pm·reach` under
/// Nyquist, the band narrowed to `B_min`, and the delay's slew under
/// `DELAY_SLEW`; the output never fades for PM, only at the edge. No PM is
/// `formant_law`, at a share of 1.
#[test]
fn pm_is_scaled_never_the_output() {
    let m = Skirt::S0.m_q();
    let b = width_floor(C3);
    for (fc, bw) in [(1000.0, 500.0), (8000.0, 10_000.0), (20_000.0, 0.0)] {
        let l = formant_law_pm(C3, fc, bw, Skirt::S0, 0.0, false);
        assert_eq!(l, formant_law(C3, fc, bw, Skirt::S0));
        assert_eq!(l.pm, 1.0);
    }
    // Slow PM with room to spare: the full depth, the band under the reach.
    let fc = 8000.0;
    let dev = 0.5 * C3;
    let reach = pm_reach(dev, C3, fc);
    let l = formant_law_pm(C3, fc, 10_000.0, Skirt::S0, dev, false);
    assert!(l.pm == 1.0 && l.edge == 1.0);
    assert!((l.bw_eff - (NYQUIST_HZ - reach - fc) / m).abs() < 5.0);
    // Too little room: the share that leaves exactly `B_min`, at full level.
    let spare = NYQUIST_HZ - fc - m * b;
    let lever = fc / C3 - 1.0;
    for over in [2.0f32, 10.0] {
        let dev = over * spare / lever;
        let l = formant_law_pm(C3, fc, 10_000.0, Skirt::S0, dev, false);
        let want = (1.0 / over).min(DELAY_SLEW * C3 / dev);
        assert!(
            (l.pm / want - 1.0).abs() < 1e-4,
            "{over}: {} vs {want}",
            l.pm
        );
        assert!(l.edge == 1.0 && l.bw_eff >= b - 1e-3);
    }
    // Fast PM on a low formant: the slew binds, `pm·dev = DELAY_SLEW·f0`.
    let dev = 20.0 * C3;
    let l = formant_law_pm(C3, 300.0, 500.0, Skirt::S1, dev, false);
    assert!((l.pm * dev / C3 - DELAY_SLEW).abs() < 1e-4, "{}", l.pm);
    // In the edge's fade already: no PM at all, the fade as without it.
    let fc = NYQUIST_HZ - m * 0.75 * b;
    let l = formant_law_pm(C3, fc, 0.0, Skirt::S0, 1000.0, false);
    assert_eq!(l.pm, 0.0);
    assert_eq!(l.edge, formant_law(C3, fc, 0.0, Skirt::S0).edge);
}

/// The grain delay is smooth onto [0, s]: `s/2` at rest with slope 1,
/// never outside, odd about its rest; the reset grain's 0 at rest, delay
/// only, flat at 0.
#[test]
fn the_grain_delay_saturates_smoothly() {
    for s in [0.0f32, 0.1, 0.9, 1.6] {
        let h = s / 2.0;
        assert_eq!(grain_delay(h, s, 0.0), h);
        assert_eq!(grain_delay(0.0, s, 0.0), 0.0);
        for k in -400..=400 {
            let pm = k as f32 * 0.05;
            let o = grain_delay(h, s, pm);
            assert!((0.0..=s).contains(&o), "{s} {pm}: {o}");
            assert!((o - h + (grain_delay(h, s, -pm) - h)).abs() < 1e-6);
            let r = grain_delay(0.0, s, pm);
            assert!((0.0..=s).contains(&r) && (pm > 0.0 || r == 0.0));
        }
        if s > 0.0 {
            let e = 1e-3 * h;
            assert!(((grain_delay(h, s, e) - h) / e - 1.0).abs() < 2e-3);
            assert!(grain_delay(0.0, s, 1e-3 * s) < 1e-5 * s);
        }
    }
}

/// Driven by feedback or PM, the carrier ceiling takes p̂ = 1: a short
/// grain's carrier gain is capped at 1, a long grain's is unchanged.
#[test]
fn a_driven_carrier_is_capped_at_one() {
    let free = formant_law(A3, 100.0, 10_000.0, Skirt::S0);
    let driven = formant_law_pm(A3, 100.0, 10_000.0, Skirt::S0, 0.0, true);
    assert!(free.g_c > 1.0 && driven.g_c == 1.0 && driven.g_m == free.g_m);
    let free = formant_law(A3, 1000.0, 500.0, Skirt::S1);
    let driven = formant_law_pm(A3, 1000.0, 500.0, Skirt::S1, 0.0, true);
    assert_eq!(free, driven);
}

/// The two poles' output variance at `x = k_n·n` from uniform `n` in
/// [−1, 1): their impulse response's energy × k_n²/3, in f64.
fn two_pole_variance(width: f32) -> f64 {
    let (c, k_n) = noise_coef(width, SR as f32);
    let (c, a) = (f64::from(c), 1.0 - f64::from(c));
    let (mut y1, mut y2, mut e) = (c, 0.0f64, 0.0f64);
    for _ in 0..2_000_000 {
        y2 += c * (y1 - y2);
        e += y2 * y2;
        y1 *= a;
    }
    e * f64::from(k_n).powi(2) / 3.0
}

/// The RMS of the two poles' output over 20 s (after 1 s to settle) of
/// xorshift32 noise at `x = k_n·n`.
fn steady_two_pole_rms(width: f32, seed: u32) -> f32 {
    let (c, k_n) = noise_coef(width, SR as f32);
    let (mut st, mut y1, mut y2, mut acc) = (seed, 0.0f32, 0.0f32, 0.0f64);
    let (settle, n) = (SR as usize, 20 * SR as usize);
    for i in 0..settle + n {
        st ^= st << 13;
        st ^= st >> 17;
        st ^= st << 5;
        let x = k_n * (st as i32) as f32 / 2_147_483_648.0;
        y1 += c * (x - y1);
        y2 += c * (y1 - y2);
        if i >= settle {
            acc += f64::from(y2 * y2);
        }
    }
    (acc / n as f64).sqrt() as f32
}

#[test]
fn noise_band_laws() {
    for w in [20.0, 500.0, 4000.0, 20_000.0] {
        let v = two_pole_variance(w);
        assert!((v - 1.0).abs() < 1e-4, "{w} {v}");
    }
    // Stable at any WIDTH: finite, c in (0, 1].
    for w in [0.0, 1e-6, 0.01, -500.0, 1e9, f32::INFINITY, f32::NAN] {
        let (c, k_n) = noise_coef(w, SR as f32);
        assert!(c > 0.0 && c <= 1.0 && k_n.is_finite(), "{w} {c} {k_n}");
    }
    // 20 s of a 20 Hz band holds too few independent samples for 1 %.
    for w in [500.0, 4000.0] {
        let r = steady_two_pole_rms(w, 0x1234);
        assert!((r - 1.0).abs() < 0.01, "{w} {r}");
    }
    for r in [0, 50, 100] {
        let (a, b) = res_mix(r);
        assert!((a * a + b * b - 1.0).abs() < 1e-6);
    }
    assert!(NOISE_GAIN * (NOISE_CLAMP * NOISE_CLAMP + 1.0).sqrt() <= 0.99);
    assert_eq!(
        [
            noise_edge(15_000.0, 2_000.0),
            noise_edge(17_000.0, 2_000.0),
            noise_edge(19_000.0, 2_000.0)
        ],
        [1.0, 0.5, 0.0]
    );
    let seeds: Vec<u32> = (0..8)
        .flat_map(|v| (0..6).map(move |o| noise_seed(v, o)))
        .collect();
    assert!(seeds.iter().all(|&s| s != 0));
    let mut u = seeds.clone();
    u.sort();
    u.dedup();
    assert_eq!(u.len(), 48);
}

/// `cargo test -p chimera-core --test formant_laws_test -- --ignored
/// print_curves --nocapture`: the constants and the literals for
/// `formant.rs`.
#[test]
#[ignore]
fn print_curves() {
    let all = Skirt::ALL.map(measure);
    for (s, m) in Skirt::ALL.iter().zip(&all) {
        let raise = m
            .peak_grid
            .iter()
            .skip(1)
            .map(|&(f, v)| v / true_lane_peak(*s, f) - 1.0)
            .fold(0.0f32, f32::max);
        println!(
            "{s:?}: K {:.4}  c_q {:.5} (closed {:.5})  m_q {:.3}  sidelobe {:.2} dB  p raise ≤ {:.2} %",
            m.k,
            m.c_q,
            closed_form_c_q(s.q()),
            m.m60 / m.k,
            m.sidelobe_db,
            100.0 * raise
        );
    }
    let rows = |name: &str, n: usize, grid: &dyn Fn(&Measured) -> &Vec<(f32, f32)>| {
        println!("static {name}: [[f32; {n}]; 4] = [");
        for m in &all {
            let v: Vec<String> = grid(m).iter().map(|&(_, r)| format!("{r:.6}")).collect();
            println!("    [{}],", v.join(", "));
        }
        println!("];");
    };
    rows("RHO", RHO_NODES, &|m| &m.rho_grid);
    rows("PEAK", PEAK_NODES, &|m| &m.peak_grid);
}

#[test]
fn fc_law_is_exact() {
    let a = 72.0;
    assert!((fc_hz(a, 50.0, None, 50.0, 72.0) - 1131.4).abs() / 1131.4 < 1e-3);
    for (key, note, want) in [
        (0.0, 48.0, 800.0),
        (100.0, 48.0, 400.0),
        (100.0, 72.0, 1600.0),
        (50.0, 60.0, 800.0),
    ] {
        assert!((fc_hz(a, 50.0, None, key, note) - want).abs() / want < 1e-3);
    }
    for (v, row) in [0.0, 25.0, 50.0, 75.0, 100.0].into_iter().zip(VOWEL_HZ) {
        for (s, hz) in [Formant::F1, Formant::F2, Formant::F3].into_iter().zip(row) {
            let want = 800.0 * hz / VOWEL_HZ[2][s as usize];
            assert!((fc_hz(a, v, Some(s), 0.0, 60.0) - want).abs() / want < 1e-3);
        }
    }
    assert_eq!(fc_hz(0.0, 100.0, Some(Formant::F1), 100.0, 0.0), 20.0);
}

#[test]
fn vowel_table_is_peterson_barney_on_the_u_o_a_e_i_path() {
    for v in 0..5 {
        for s in 0..3 {
            assert!((VOWEL_OCT[v][s] - (VOWEL_HZ[v][s] / VOWEL_HZ[2][s]).log2()).abs() < 1e-6);
        }
    }
    assert_eq!(VOWEL_OCT[2], [0.0; 3]);
    assert!((vowel_delta(Formant::F2, 75.0) - (1840.0f32 / 1090.0).log2()).abs() < 1e-6);
    assert!((vowel_delta(Formant::F2, 62.5) - 0.5 * (1840.0f32 / 1090.0).log2()).abs() < 1e-6);
}

#[test]
fn vowel_slots_resolve() {
    use Formant::{F1, F2, F3};
    use OpKind::{Noise, Table};
    use VowelSlot::{Auto, Off};
    let t = Table(WaveId::W1);
    let fm = OpKind::Formant;
    let k = Kinds::new([t, fm, t, Noise, Noise, t]);
    assert_eq!(
        resolve_slots(&k, [Auto; 6]),
        [None, Some(F1), None, Some(F2), Some(F3), None]
    );
    let k = Kinds::new([fm, fm, Noise, Noise, Noise, t]);
    assert_eq!(resolve_slots(&k, [Auto; 6])[3..5], [None, None]);
    let k = Kinds::new([fm, fm, Noise, Noise, t, t]);
    let f1 = VowelSlot::F1;
    assert_eq!(
        resolve_slots(&k, [f1, f1, Auto, Auto, Auto, Auto])[..4],
        [Some(F1), Some(F1), Some(F1), Some(F2)]
    );
    assert_eq!(
        resolve_slots(&k, [Off, Auto, Auto, Auto, Auto, Auto])[..2],
        [None, Some(F1)]
    );
}
