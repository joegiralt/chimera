//! The reverb TYPEs' shared parts (reverb TYPEs spec § Shared machinery,
//! § Level plan, § Arena) and the shared test helpers' own measures.

mod common;
use chimera_core::block::ParamId;
use chimera_core::dsp::clampf;
use chimera_core::dsp::ease::Ease;
use chimera_core::dsp::rev::arena::ReverbArena;
use chimera_core::dsp::rev::parts::*;
use chimera_core::dsp::ring::RING_LEN;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{
    check_knobs_ease, check_level_plan, check_no_slap, check_noise_and_loudness, corr, rt60_t20,
    settled_peak_hz, thd,
};
use std::f64::consts::TAU;

struct Noise(u32);

impl Noise {
    /// Uniform in ±1.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        2.0 * (self.0 as f32 / u32::MAX as f32) - 1.0
    }
}

fn sine(hz: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (TAU * hz as f64 * i as f64 / SR as f64).sin() as f32)
        .collect()
}

#[test]
fn soft_clip_is_identity_to_the_knee_and_reaches_two_flat() {
    for x in [-1.0f32, -0.5, 0.0, 0.3, 1.0] {
        assert_eq!(soft_clip(x), x);
    }
    assert!((soft_clip(3.0) - 2.0).abs() < 1e-6);
    assert_eq!((soft_clip(10.0), soft_clip(-10.0)), (2.0, -2.0));
    assert!(((soft_clip(3.0) - soft_clip(2.999)) / 0.001).abs() < 1e-2);
    assert!((1..400).all(|i| {
        let x = i as f32 * 0.01;
        soft_clip(x) >= soft_clip(x - 0.01)
    }));
    assert!(soft_clip(f32::NAN).is_finite());
}

/// The plain allpass over an `M`-sample state line, and the normalised one
/// beside it.
struct Pair<const M: usize> {
    v: [f32; M],
    w: [f32; M],
    i: usize,
}

impl<const M: usize> Pair<M> {
    /// `(plain y, normalised y, w, v from w)`.
    fn tick(&mut self, x: f32, g: f32) -> (f32, f32, f32, f32) {
        let (y0, v) = ap_f32(x, self.v[self.i], g);
        let inv = 1.0 / (1.0 - g);
        let (y1, w) = ap_norm(x, self.w[self.i], g, inv);
        (self.v[self.i], self.w[self.i]) = (v, w);
        self.i = (self.i + 1) % M;
        (y0, y1, w, w * inv)
    }
}

#[test]
fn ap_norm_matches_the_plain_allpass_and_bounds_its_state() {
    let g = 0.75;
    // The plain form: v = x − g·z, y = z + g·v.
    let (z, x) = (0.4f32, 0.3f32);
    assert_eq!(ap_f32(x, z, g), (z + g * (x - g * z), x - g * z));
    let mut ap = Pair::<37> {
        v: [0.0; 37],
        w: [0.0; 37],
        i: 0,
    };
    let mut n = Noise(7);
    for _ in 0..10_000 {
        let (y0, y1, _, _) = ap.tick(n.next(), g);
        assert!((y0 - y1).abs() <= 1e-6, "{y0} vs {y1}");
    }
    // z^-37 = −1 at fs / 74: a sine there builds v to 1 / (1 − g).
    let mut ap = Pair::<37> {
        v: [0.0; 37],
        w: [0.0; 37],
        i: 0,
    };
    let (mut w_max, mut v_max) = (0.0f32, 0.0f32);
    for i in 0..20_000 {
        let x = (TAU * i as f64 / 74.0).sin() as f32;
        let (_, _, w, v) = ap.tick(x, g);
        (w_max, v_max) = (w_max.max(w.abs()), v_max.max(v.abs()));
    }
    assert!(w_max <= 1.0 + 1e-6, "{w_max}");
    assert!(v_max > 3.9 && v_max <= 4.0 + 1e-5, "{v_max}");
}

#[test]
fn pair_reads_fixed_at_fraction_0_and_the_mean_at_half_across_the_wrap() {
    const N: usize = 50;
    let lines: Vec<i16> = (0..N as i16).map(|i| i * 100).collect();
    assert_eq!(pair(&lines, 3, 2.0), lines[1] as f32);
    assert_eq!(
        pair(&lines, 3, 2.5),
        (lines[1] as f32 + lines[0] as f32) / 2.0
    );
    assert_eq!(
        pair(&lines, 0, 2.5),
        (lines[N - 2] as f32 + lines[N - 3] as f32) / 2.0
    );
    assert_eq!(pair(&lines, 1, 2.0), lines[N - 1] as f32);
}

#[test]
fn xfade_blends_linearly_over_720_and_lands() {
    assert_eq!(XFADE, 720);
    let mut x = Xfade::default();
    assert!(!x.busy());
    x.start(10);
    assert_eq!((x.from, x.to), (0, 10));
    for k in 0..XFADE {
        assert!(x.busy(), "tick {k}");
        assert!((x.weight() - k as f32 / 720.0).abs() <= 1e-6, "tick {k}");
        // A request during a crossfade waits.
        x.start(20);
        assert_eq!(x.to, 10);
        x.tick();
    }
    assert!(!x.busy());
    assert_eq!(x.weight(), 1.0);
    assert_eq!((x.from, x.to), (10, 10));
    x.tick();
    assert_eq!((x.from, x.to, x.weight()), (10, 10, 1.0));
}

#[test]
fn wobble_is_quadrature_and_its_block_lerp_errs_under_1e4_at_2_hz() {
    const RING: f64 = 24_000.0;
    const HALF: usize = 32;
    let mut w = Wobble::default();
    w.set_rate(2.0, RING as f32);
    let ends: Vec<[f32; 2]> = (0..=750).map(|_| w.block()).collect();
    for side in 0..2 {
        let m = ends.iter().map(|e| e[side].abs()).fold(0.0, f32::max);
        assert!((m - 1.0).abs() <= 1e-3, "side {side}: {m}");
    }
    // Each side's phase at 2 Hz over the second's two whole cycles.
    let phase = |side: usize| {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (b, e) in ends[..750].iter().enumerate() {
            let t = (b * HALF) as f64 / RING;
            re += e[side] as f64 * (TAU * 2.0 * t).cos();
            im += e[side] as f64 * (TAU * 2.0 * t).sin();
        }
        // sin(ωt + φ) against cos and sin: (sin φ, cos φ), halved.
        re.atan2(im).to_degrees()
    };
    let lead = (phase(0) - phase(1)).rem_euclid(360.0);
    assert!((lead - 90.0).abs() <= 1.0, "L leads R by {lead}°");
    let mut worst = 0.0f64;
    for (b, e) in ends.windows(2).enumerate() {
        for k in 0..HALF {
            let t = (b * HALF + k) as f64 / RING;
            let f = k as f32 / HALF as f32;
            let lerp = e[0][0] + f * (e[1][0] - e[0][0]);
            worst = worst.max((lerp as f64 - (TAU * 2.0 * t).sin()).abs());
        }
    }
    assert!(worst < 1e-4, "{worst}");
}

#[test]
fn ease_lands_and_never_snaps() {
    let k = ease_k(0.02);
    let most = 1.0 - (-64.0f32 / (0.02 * 48_000.0)).exp() + 1e-6;
    let mut e = Ease::default();
    assert_eq!(e.step(0.0, k), (0.0, 0.0));
    let (from, mut v) = e.step(1.0, k);
    assert_eq!(from, 0.0);
    assert!(v - from <= most, "{}", v - from);
    let blocks = (0.25 * SR as f32 / 64.0).ceil() as usize;
    for _ in 1..blocks {
        let (from, to) = e.step(1.0, k);
        assert_eq!(from, v);
        assert!(to - from <= most && to >= from, "{from} → {to}");
        v = to;
    }
    assert!((v - 1.0).abs() <= 1e-5, "{v}");
}

#[test]
fn one_pole_is_the_tone_filters_coefficient() {
    let a = one_pole(1_000.0, 24_000.0);
    let want = 1.0 - (-TAU * 1_000.0 / 24_000.0).exp();
    assert!((a as f64 - want).abs() < 1e-5, "{a} vs {want}");
    assert!(one_pole(0.0, 24_000.0).abs() < 1e-7);
}

#[test]
fn stores_round_truncate_saturate_and_count_their_clamps() {
    reset_clamp_counts();
    assert_eq!(store_round(2.5), 3);
    assert_eq!(store_round(-2.5), -3);
    assert_eq!(store_trunc(2.9), 2);
    assert_eq!(store_trunc(-2.9), -2);
    assert_eq!(clamp_counts(), (0, 4));
    assert_eq!(store_round(40_000.0), i16::MAX);
    assert_eq!(store_trunc(-40_000.0), i16::MIN);
    assert_eq!(store_round(32_767.4), i16::MAX);
    assert_eq!(store_trunc(-32_768.9), i16::MIN);
    assert_eq!(clamp_counts(), (2, 8));
    reset_clamp_counts();
    assert_eq!(clamp_counts(), (0, 0));
}

#[test]
fn f32_region_round_trips_and_clear_zeroes_both_views() {
    let mut a = Box::new(ReverbArena::new());
    a.lines_mut()[..100].fill(7);
    for (i, s) in a.f32_region(100, 50).iter_mut().enumerate() {
        *s = i as f32 + 0.5;
    }
    for (i, &s) in a.f32_region(100, 50).iter().enumerate() {
        assert_eq!(s, i as f32 + 0.5);
    }
    assert_eq!(a.lines()[99], 7);
    assert_eq!(a.lines()[200], 0);
    // The last whole f32 fits.
    a.f32_region(RING_LEN - 2, 1)[0] = 1.0;
    a.clear();
    assert!(a.lines().iter().all(|&s| s == 0));
    assert!(
        a.f32_region(0, RING_LEN / 2)
            .iter()
            .all(|&s| s.to_bits() == 0)
    );
    let odd = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.f32_region(101, 1);
    }));
    assert!(odd.is_err(), "odd offset");
    let over = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.f32_region(RING_LEN - 2, 2);
    }));
    assert!(over.is_err(), "past the end");
}

#[test]
fn helpers_measure_known_signals() {
    let mut n = Noise(0x5eed);
    let decay: Vec<f32> = (0..3 * SR as usize)
        .map(|i| n.next() * (-6.91 * i as f32 / SR as f32 / 1.5).exp())
        .collect();
    let t = rt60_t20(&decay, &decay);
    assert!((t / 1.5 - 1.0).abs() <= 0.05, "{t}");
    let clean = sine(1_000.0, SR as usize);
    let clipped: Vec<f32> = clean.iter().map(|&x| clampf(x, -0.9, 0.9)).collect();
    let (c, d) = (thd(&clean, 1_000.0), thd(&clipped, 1_000.0));
    assert!(c < 1e-4, "clean {c}");
    assert!(d > 1e-2, "clipped {d}");
    let f = settled_peak_hz(&clean);
    assert!((f - 1_000.0).abs() <= 1.0, "{f}");
    let neg: Vec<f32> = decay.iter().map(|&x| -x).collect();
    assert!((corr(&decay, &decay) - 1.0).abs() < 1e-6);
    assert!((corr(&decay, &neg) + 1.0).abs() < 1e-6);
}

/// A 10 ms delay through i16 storage (or f32): the plumbing's stand-in for
/// an engine.
fn delay(
    f32_line: bool,
) -> impl FnMut(&[f32; BLOCK_SIZE]) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    let (mut line, mut fl, mut i) = (vec![0i16; 480], vec![0.0f32; 480], 0);
    move |send| {
        let out = core::array::from_fn(|k| {
            let y = if f32_line {
                fl[i]
            } else {
                line[i] as f32 / 16_384.0
            };
            (line[i], fl[i]) = (store_round(send[k] * 16_384.0), send[k]);
            i = (i + 1) % 480;
            y
        });
        (out, out)
    }
}

#[test]
fn the_shared_checks_pass_a_plain_delay() {
    let (mut a, mut b, mut c) = (delay(false), delay(false), delay(true));
    check_level_plan(&mut a, &mut || {}, &mut b, &mut c, 100.0);
    let (mut a, mut q) = (delay(false), delay(false));
    check_noise_and_loudness(&mut a, &mut q, 0.0);
    check_knobs_ease(&mut a, &[ParamId(6)], &mut |_, _| {});
}

#[test]
fn no_slap_passes_a_plain_tail_and_catches_an_echo() {
    let mut n = Noise(31);
    let tail: Vec<f32> = (0..SR as usize)
        .map(|i| n.next() * (-3.0 * i as f32 / SR as f32).exp())
        .collect();
    check_no_slap(&tail, &tail);
    let echo: Vec<f32> = (0..tail.len())
        .map(|i| tail[i] + if i >= 240 { 0.7 * tail[i - 240] } else { 0.0 })
        .collect();
    let caught = std::panic::catch_unwind(|| check_no_slap(&echo, &tail));
    assert!(caught.is_err(), "a 5 ms echo passes");
}
