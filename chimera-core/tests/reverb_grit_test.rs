//! GRIT as the reverb's converters (reverb TYPEs spec § GRIT, § Tests):
//! an ADC on the send and a DAC on the return, outside every engine.

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::ease::{Ease, ease_coeff};
use chimera_core::dsp::halfband::HALF;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::grit::{Converters, IDLE_FADE, IDLE_HOLD, bits, cutoff_hz, step};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::dsp::ring::{size_step, stage_len};
use chimera_hal::BLOCK_SIZE;
use common::rev::{Noise, blocks, burst_products_db, max_d2, sine_floor_dbfs, tone};
use common::sweep::power;
use common::{SR, fnv1a};
use std::f64::consts::TAU;

const FS_RING: f64 = 24_000.0;

/// QUAD at its defaults but `grit`, MIX 1, through the dispatcher.
fn quad(grit: f32) -> (Box<Reverb>, ReverbParams) {
    let p = ReverbParams {
        grit,
        mix: 1.0,
        ..ReverbParams::default()
    };
    (Box::new(Reverb::new()), p)
}

fn tick(
    rv: &mut Reverb,
    p: &ReverbParams,
    send: &[f32; BLOCK_SIZE],
) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    let mut out = Stereo::SILENT;
    rv.process(send, p, p.mix, Tempo::DEFAULT, SR, &mut out);
    (out.l, out.r)
}

// ── the bypass ──

/// `fx_golden_test`'s reverb render (a 110 Hz saw burst, then silence).
fn golden_render(size: f32) -> Vec<f32> {
    let p = ReverbParams {
        grit: 0.0,
        time: 0.7,
        damping: 0.3,
        size,
        mix: 0.5,
    };
    let mut rv = Box::new(Reverb::new());
    let mut out = Vec::new();
    let mut phase = 0.0f32;
    for b in 0..450 {
        let mut block = [0.0f32; BLOCK_SIZE];
        if b < 50 {
            for s in block.iter_mut() {
                *s = phase - 0.5;
                phase = (phase + 110.0 / SR as f32).fract();
            }
        }
        let (l, r) = tick(&mut rv, &p, &block);
        out.extend_from_slice(&l);
        out.extend_from_slice(&r);
    }
    out
}

/// Recorded at the T4 golden reference (a2abe82f), before the converters.
const GRIT_0: &[(&str, u64)] = &[
    ("reverb_ring_grit_0", 0xd17b7769e13816bf),
    ("reverb_ring_max_size_grit_0", 0x83591bf24cab49f6),
];

#[test]
fn grit_0_bypasses_bit_for_bit() {
    let got = [
        ("reverb_ring_grit_0", fnv1a(&golden_render(0.5))),
        ("reverb_ring_max_size_grit_0", fnv1a(&golden_render(1.0))),
    ];
    common::golden::check(GRIT_0, &got);
}

// ── the converter alone ──

/// `x` through `c`'s ADC at `grit`, a block at a time.
fn adc_all(c: &mut Converters, x: &[f32], grit: f32) -> Vec<f32> {
    x.as_chunks::<HALF>()
        .0
        .iter()
        .flat_map(|&b| {
            let mut b = b;
            c.adc(&mut b, grit);
            b
        })
        .collect()
}

/// `x` through `c`'s DAC at `grit` on the left (the right silent).
fn dac_all(c: &mut Converters, x: &[f32], grit: f32) -> Vec<f32> {
    x.as_chunks::<HALF>()
        .0
        .iter()
        .flat_map(|&b| {
            let (mut l, mut r) = (b, [0.0; HALF]);
            c.dac(&mut l, &mut r, grit, 1.0);
            l
        })
        .collect()
}

#[test]
fn adc_lands_on_its_step() {
    for (g, b) in [(0.3f32, 14.2f32), (0.6, 12.4), (1.0, 10.0)] {
        assert!((bits(g) - b).abs() < 1e-5, "bits({g}) = {}", bits(g));
        let d = step(g);
        let want = 2f64.powf(1.0 - b as f64);
        assert!(((d as f64 - want) / want).abs() < 1e-5, "step({g}) = {d}");
        let n = 1 << 16;
        let ramp: Vec<f32> = (0..n).map(|i| -1.0 + 2.0 * i as f32 / n as f32).collect();
        let y = adc_all(&mut Converters::undithered(), &ramp, g);
        for &y in &y {
            let k = (y / d).round();
            assert!(
                ((y as f64 / d as f64) - k as f64).abs() < 1e-6 * (k.abs() as f64).max(1.0),
                "grit {g}: {y} is off the step {d}"
            );
            assert_eq!(y, k * d, "grit {g}: {y} is not {k} steps");
        }
        let mut v = y.clone();
        v.sort_by(f32::total_cmp);
        v.dedup();
        let spacing = v.windows(2).map(|w| w[1] - w[0]).fold(f32::MAX, f32::min);
        assert!(
            (spacing / d - 1.0).abs() < 1e-3,
            "grit {g}: spacing {spacing}, step {d}"
        );
        assert!(
            v.len() as f32 >= 0.9 * 2.0 / d,
            "grit {g}: {} levels",
            v.len()
        );
    }
}

/// The DAC's gain at `hz`, dither off: Hann-windowed DFT of the output
/// over that of the input, after a settle.
fn gain_at(grit: f32, hz: f64) -> f64 {
    let n = 24_000;
    let x: Vec<f32> = (0..n)
        .map(|i| 0.5 * (TAU * hz * i as f64 / FS_RING).sin() as f32)
        .collect();
    let y = dac_all(&mut Converters::undithered(), &x, grit);
    let dft = |s: &[f32]| {
        let m = s.len() as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &v) in s.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU * i as f64 / m).cos();
            re += v as f64 * w * (TAU * hz * i as f64 / FS_RING).cos();
            im += v as f64 * w * (TAU * hz * i as f64 / FS_RING).sin();
        }
        re.hypot(im)
    };
    let s = 4_800..n;
    dft(&y[s.clone()]) / dft(&x[s])
}

/// Where the DAC's gain crosses −3 dB, by bisection.
fn corner_hz(grit: f32) -> f64 {
    let (mut lo, mut hi) = (1_000.0, 11_990.0);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if gain_at(grit, mid) > 0.5f64.sqrt() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[test]
fn cutoff_tracks_grit() {
    assert!((cutoff_hz(1.0) - 8_000.0).abs() < 1.0, "{}", cutoff_hz(1.0));
    assert!(
        (cutoff_hz(0.0) - 11_000.0).abs() < 1.0,
        "{}",
        cutoff_hz(0.0)
    );
    for (g, want) in [(0.01f32, 11_000.0), (1.0, 8_000.0)] {
        let f = corner_hz(g);
        assert!(
            (f / want - 1.0).abs() <= 0.03,
            "grit {g}: −3 dB at {f:.0} Hz"
        );
    }
}

#[test]
fn dither_keeps_a_minus_80_dbfs_sine_as_a_sine() {
    let n = 24_000;
    let amp = 1e-4; // −80 dBFS
    let x: Vec<f32> = (0..n)
        .map(|i| amp * (TAU * 997.0 * i as f64 / FS_RING).sin() as f32)
        .collect();
    let y = adc_all(&mut Converters::new(), &x, 1.0);
    let bin = |hz: f64, len: usize| (hz * len as f64 / FS_RING).round() as usize;
    let median = |p: &[f64], len: usize| {
        let mut v = p[bin(20.0, len)..bin(10_000.0, len)].to_vec();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    // The sine: one long frame.
    let long = 1 << 14;
    let p = power(&y, long);
    let k = bin(997.0, long);
    let sine = p[k - 1..=k + 1].iter().copied().fold(0.0, f64::max);
    let db = 10.0 * (sine / median(&p, long)).log10();
    assert!(
        db >= 10.0,
        "the sine stands {db:.1} dB over the noise median"
    );
    // Its harmonics: short frames, averaged.
    let short = 1 << 10;
    let p = power(&y, short);
    let m = median(&p, short);
    for h in 2..=10 {
        let k = bin(997.0 * h as f64, short);
        let db = 10.0 * (p[k] / m).log10();
        assert!(db <= 3.0, "harmonic {h}: {db:.1} dB over the median");
    }
}

// ── through QUAD ──

/// QUAD's stage-1 round trip at its default SIZE, in seconds.
fn stage_1_s() -> f64 {
    stage_len(size_step(0.5), 0) as f64 / FS_RING
}

#[test]
fn a_grit_1_tail_has_no_correlated_products() {
    let (mut rv, p) = quad(1.0);
    let spacing = 1.0 / stage_1_s();
    // 1 kHz ± `spacing` lies inside the tail's own main lobe at a 100 ms
    // window (10 Hz bins), so only the harmonics can be told apart.
    println!("stage-1 mode spacing {spacing:.1} Hz: not resolvable at 100 ms");
    let (summed, worst) = burst_products_db(&mut |s| tick(&mut rv, &p, s));
    println!("products: {summed:.1} dB summed, {worst:.1} dB worst window");
    assert!(summed <= 3.0, "products {summed:.1} dB over the median");
}

#[test]
fn idle_mute_lets_the_tail_reach_zero() {
    // 100 ms of hold and 50 ms of fade at the ring's 24 kHz.
    assert_eq!((IDLE_HOLD, IDLE_FADE), (2_400, 1_200));
    let (mut rv, p) = quad(1.0);
    let mut n = Noise(0xdead_beef);
    for _ in 0..blocks(1.0) {
        tick(&mut rv, &p, &core::array::from_fn(|_| n.next()));
    }
    assert_eq!(rv.converters().gains().0, 1.0);
    let silence = [0.0; BLOCK_SIZE];
    // One block more for the half-band's tail into the ADC.
    for _ in 0..blocks(0.15) + 1 {
        tick(&mut rv, &p, &silence);
    }
    assert_eq!(rv.converters().gains().0, 0.0, "the ADC's dither 150 ms on");
    let mut b = 0;
    while !rv.quad().unwrap().ring().is_silent(rv.arena()) {
        tick(&mut rv, &p, &silence);
        b += 1;
        assert!(b <= blocks(20.0), "still ringing after 20 s");
    }
    for _ in 0..blocks(0.15) {
        tick(&mut rv, &p, &silence);
    }
    assert_eq!(rv.converters().gains(), (0.0, 0.0));
    for _ in 0..blocks(0.1) {
        let (l, r) = tick(&mut rv, &p, &silence);
        assert!(l.iter().chain(&r).all(|&s| s == 0.0));
    }
}

/// Review Focus 4. Noise for 0.5 s, then silence until the return sits
/// within one converter step at GRIT 1: what the DAC returns from then on
/// is its dither over the tail's last LSBs, through its hold and its
/// fade. GRIT moves to 0.3 for 50 ms, then back to 1 into the fade. The
/// return's largest second difference stays within what ±1 step of
/// dither makes at GRIT 1 (4 steps), and it ends on exact zeros.
#[test]
fn dither_mute_fades_without_a_step() {
    let (mut rv, mut p) = quad(1.0);
    let mut n = Noise(4242);
    for _ in 0..blocks(0.5) {
        tick(&mut rv, &p, &core::array::from_fn(|_| n.next()));
    }
    let silence = [0.0; BLOCK_SIZE];
    let quiet = |x: &[f32]| x.iter().all(|s| s.abs() <= step(1.0));
    loop {
        let (l, r) = tick(&mut rv, &p, &silence);
        if quiet(&l) && quiet(&r) {
            break;
        }
    }
    assert_eq!(rv.converters().gains().1, 1.0, "the DAC is awake");
    let move_end = blocks(0.05);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut b = 0;
    while rv.converters().gains().1 > 0.0 || b < move_end {
        p.grit = if b < move_end { 0.3 } else { 1.0 };
        let (bl, br) = tick(&mut rv, &p, &silence);
        l.extend_from_slice(&bl);
        r.extend_from_slice(&br);
        b += 1;
        assert!(b <= blocks(20.0), "the DAC never mutes");
    }
    let d2 = max_d2(&l).max(max_d2(&r));
    let bound = 4.0 * step(1.0);
    println!("d2 {d2:e}, bound {bound:e}, {b} blocks");
    assert!(d2 > 0.0, "no dither: the test measures nothing");
    assert!(d2 <= bound, "d2 {d2} over {bound}");
    let (l, r) = tick(&mut rv, &p, &silence);
    assert!(l.iter().chain(&r).all(|&s| s == 0.0));
}

/// A held 1 kHz sine at −12 dBFS, GRIT `grit(block)`: the return from
/// `from` blocks on.
fn held_tone(grit: &dyn Fn(usize) -> f32, from: usize, to: usize) -> Vec<f32> {
    let (mut rv, mut p) = quad(0.3);
    let sine = tone(1_000.0, 0.251);
    let mut out = Vec::new();
    for b in 0..to {
        p.grit = grit(b);
        let (l, r) = tick(
            &mut rv,
            &p,
            &core::array::from_fn(|i| sine(b * BLOCK_SIZE + i)),
        );
        if b >= from {
            out.extend(l.iter().zip(&r).flat_map(|(&l, &r)| [l, r]));
        }
    }
    out
}

/// GRIT eased from 0.3 to 0 on a settled tone: the block it lands on 0
/// fades from the converters to the raw return, so the bypass adds no
/// step. The tone (1 kHz) and the half-band's image (23 kHz) both repeat
/// every 48 samples, so `r[n] = x[n] − x[n − 48]` leaves only what
/// changed, and a step shows as a kink in it. Over the landing, ±3
/// blocks, r's largest second difference stays under 1.5e−3: the 16-bit
/// dither alone reads about 5e−4 before it, the fade 1.0e−3, and a hard
/// switch 3.7e−3.
#[test]
fn landing_on_grit_0_does_not_step() {
    let settle = blocks(3.0);
    let k = ease_coeff(SR);
    let mut e = Ease::default();
    e.step(0.3, k);
    let mut land = 1;
    while e.step(0.0, k).1 != 0.0 {
        land += 1;
    }
    let land = settle + land;
    let out = held_tone(&|b| if b < settle { 0.3 } else { 0.0 }, land - 3, land + 3);
    for s in 0..2 {
        let x: Vec<f32> = out.iter().skip(s).step_by(2).copied().collect();
        let r: Vec<f32> = x.windows(49).map(|w| w[48] - w[0]).collect();
        let step = max_d2(&r);
        println!("side {s}: step {step:e}");
        assert!(step <= 1.5e-3, "side {s}: {step} over the landing");
    }
}

fn floor_dbfs(grit: f32) -> f32 {
    let (mut rv, p) = quad(grit);
    sine_floor_dbfs(&mut |s| tick(&mut rv, &p, s))
}

/// § GRIT's table, measured. The table's "QUAD total" holds at 0.3 (the
/// arena dominates); from 0.6 up it assumed the ADC's noise reaches the
/// return at unity, but QUAD's steady-state gain to white noise at its
/// defaults is about +8.5 dB, so the measured floors sit above it: −62
/// against −64, and −48 against −54. A silent send measures the same at
/// 0.6 and 1 (−64.4 and −48.7) and nothing at 0.3, whose ADC step
/// injects under one LSB and truncates away in the ring.
#[test]
fn noise_floor_matches_the_table() {
    for (g, table, measured) in [
        (0.3f32, -66.0f32, -68.0f32),
        (0.6, -64.0, -62.0),
        (1.0, -54.0, -48.0),
    ] {
        let got = floor_dbfs(g);
        println!("GRIT {g}: {got:.1} dBFS (table {table})");
        assert!(
            (got - measured).abs() <= 2.0,
            "GRIT {g}: {got:.1} dBFS, measured {measured}"
        );
    }
    assert!(floor_dbfs(0.0) <= -66.0);
}
