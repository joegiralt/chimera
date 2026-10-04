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
    autocorr_peak_db, check_knobs_ease, check_level_plan, check_no_slap, check_noise_and_loudness,
    check_settles_on_pitch, check_smear_bound, corr, line_db, rt60_t20, settled_peak_hz,
    tail_pitch_hz, thd,
};
use std::cell::RefCell;
use std::f64::consts::TAU;
use std::panic::{AssertUnwindSafe, catch_unwind};

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
fn wander_is_smooth_bounded_and_decorrelated() {
    const RING: f32 = 24_000.0;
    const HALF: usize = 32;
    let seeds = wander_seeds::<4>(0x5EED_0343);
    let blocks = (120.0 * RING) as usize / HALF;
    let traces: Vec<Vec<f32>> = (0..4)
        .map(|k| {
            let mut w = Wander::new(seeds[k], k as f32 / 4.0);
            w.set_rate(1.0, RING);
            (0..blocks).map(|_| w.block()).collect()
        })
        .collect();
    let max_step = TAU as f32 * 1.0 * (HALF as f32 / RING) * 1.1;
    for (k, t) in traces.iter().enumerate() {
        assert!(
            t.iter().all(|v| (-1.0..=1.0).contains(v)),
            "line {k}: out of −1..1"
        );
        let step = t
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step <= max_step, "line {k}: a block step of {step}");
        let line = line_db(t, 1.0);
        assert!(line <= 10.0, "line {k}: a {line:.1} dB spectral line");
    }
    for i in 0..4 {
        for j in i + 1..4 {
            let c = corr(&traces[i], &traces[j]);
            assert!(c.abs() <= 0.2, "lines {i} and {j}: corr {c}");
        }
    }
    // The control: a 1 Hz sine is a line.
    let sine: Vec<f32> = (0..blocks)
        .map(|b| (TAU * (b * HALF) as f64 / RING as f64).sin() as f32)
        .collect();
    assert!(line_db(&sine, 1.0) > 10.0);
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

/// One second of decaying noise, `dark` through two one-poles at 500 Hz.
fn tail(seed: u32, dark: bool) -> Vec<f32> {
    let (mut n, mut a, mut b) = (Noise(seed), 0.0f32, 0.0f32);
    let k = if dark {
        one_pole(500.0, SR as f32)
    } else {
        1.0
    };
    (0..SR as usize)
        .map(|i| {
            a += k * (n.next() - a);
            b += k * (a - b);
            b * (-3.0 * i as f32 / SR as f32).exp()
        })
        .collect()
}

fn with_echo(x: &[f32]) -> Vec<f32> {
    (0..x.len())
        .map(|i| x[i] + if i >= 240 { 0.7 * x[i - 240] } else { 0.0 })
        .collect()
}

#[test]
fn no_slap_passes_plain_and_dark_tails_and_catches_an_echo() {
    for dark in [false, true] {
        let (moved, fixed) = (tail(31, dark), tail(77, dark));
        check_no_slap(&moved, &fixed);
        let caught = catch_unwind(|| check_no_slap(&with_echo(&moved), &fixed));
        assert!(caught.is_err(), "dark {dark}: a 5 ms echo passes");
    }
    // Unwhitened, the dark tail's colour alone reads as a slap.
    assert!(autocorr_peak_db(&tail(31, true), (1.0, 10.0)) > -20.0);
}

/// A test feedback comb at the bus rate: one i16 line, gain 0.9, read
/// `len` back. A new `target` crossfades there in steps of ≤ 240 samples,
/// or with `glide` slides there at 1,200 samples a second. `depth` moves
/// the read shorter by `depth`·(1 + x), x a 2 Hz `Wander`, or with `lfo`
/// a 2 Hz sine (the vibrato control), or with `freeze` still at the
/// centre; the output is two-poled by `dark` (a
/// one-pole's own second difference is white) and
/// scaled by `level`, eased or not.
struct Comb {
    line: Vec<i16>,
    head: usize,
    xf: Xfade,
    target: u16,
    glide: Option<f32>,
    wob: Wander,
    lfo: bool,
    freeze: bool,
    t: f32,
    w: f32,
    depth: f32,
    level: f32,
    gain: Ease,
    ease: bool,
    dark: f32,
    lp: [f32; 2],
}

impl Comb {
    fn new(len: u16) -> Self {
        let (mut xf, mut wob) = (Xfade::default(), Wander::new(wander_seeds::<1>(7)[0], 0.0));
        (xf.from, xf.to) = (len, len);
        wob.set_rate(2.0, 24_000.0);
        let line = vec![0; 6_100];
        Comb {
            line,
            head: 0,
            xf,
            target: len,
            glide: None,
            wob,
            lfo: false,
            freeze: false,
            t: 0.0,
            w: 0.0,
            depth: 0.0,
            level: 1.0,
            gain: Ease::default(),
            ease: true,
            dark: 1.0,
            lp: [0.0; 2],
        }
    }

    fn moving(&self) -> bool {
        self.glide
            .map_or(self.xf.busy() || self.xf.to != self.target, |d| {
                d < self.target as f32
            })
    }

    fn block(&mut self, send: &[f32; BLOCK_SIZE]) -> [f32; BLOCK_SIZE] {
        let w1 = if self.freeze {
            0.0
        } else if self.lfo {
            self.t += BLOCK_SIZE as f32 / SR as f32;
            (TAU * 2.0 * self.t as f64).sin() as f32
        } else {
            self.wob.block()
        };
        let w0 = core::mem::replace(&mut self.w, w1);
        let (g0, g1) = match self.ease {
            true => self.gain.step(self.level, ease_k(0.02)),
            false => (self.level, self.level),
        };
        core::array::from_fn(|k| {
            let f = k as f32 / BLOCK_SIZE as f32;
            let m = -self.depth * (1.0 + w0 + f * (w1 - w0));
            let rd = |d: f32| pair(&self.line, self.head, d) / 8192.0;
            let fb = if let Some(d) = self.glide {
                self.glide = Some((d + 0.025).min(self.target as f32));
                rd(d + m)
            } else {
                let (wt, from, to) = (self.xf.weight(), self.xf.from as f32, self.xf.to as f32);
                let y = (1.0 - wt) * rd(from + m) + wt * rd(to + m);
                self.xf.tick();
                if !self.xf.busy() && self.xf.to != self.target {
                    let to = self.xf.to;
                    self.xf.start(to + (self.target - to).min(240));
                }
                y
            };
            let y = 0.5 * send[k] + 0.9 * fb;
            self.line[self.head] = store_round(y * 8192.0);
            self.head = (self.head + 1) % self.line.len();
            self.lp[0] += self.dark * (y - self.lp[0]);
            self.lp[1] += self.dark * (self.lp[0] - self.lp[1]);
            self.lp[1] * (g0 + f * (g1 - g0))
        })
    }
}

/// `c` as a `Render`, mono on both sides.
fn render(
    c: &RefCell<Comb>,
) -> impl FnMut(&[f32; BLOCK_SIZE]) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) + '_ {
    |send| {
        let y = c.borrow_mut().block(send);
        (y, y)
    }
}

/// The comb's 100 → 125 ms move, starting as the sine stops.
fn settles(glide: bool) {
    let c = RefCell::new(Comb::new(4_800));
    if glide {
        c.borrow_mut().glide = Some(4_800.0);
    }
    let mut step = |b: usize| {
        if b == common::rev::blocks(0.5) {
            c.borrow_mut().target = 6_000;
        }
        c.borrow().moving()
    };
    check_settles_on_pitch(&mut render(&c), &mut step, 0.125);
}

#[test]
fn a_crossfaded_move_settles_on_pitch_and_a_glide_does_not() {
    settles(false);
    let caught = catch_unwind(|| settles(true));
    assert!(caught.is_err(), "a glide re-pitches the tail and passes");
}

#[test]
fn the_smear_bound_holds_on_the_comb_and_catches_an_offset_and_a_vibrato() {
    // 30 samples of depth centre the read on 4,800, where 1 kHz is a mode.
    let still = RefCell::new(Comb::new(4_830));
    (still.borrow_mut().depth, still.borrow_mut().freeze) = (30.0, true);
    let f_ref = tail_pitch_hz(&mut render(&still));
    let smeared = |lfo: bool| {
        let c = RefCell::new(Comb::new(4_830));
        (c.borrow_mut().depth, c.borrow_mut().lfo) = (30.0, lfo);
        c
    };
    let c = smeared(false);
    check_smear_bound(&mut render(&c), 0.1, f_ref, 2.0);
    let c = smeared(false);
    let sharp = f_ref * 2f32.powf(5.0 / 1_200.0);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        check_smear_bound(&mut render(&c), 0.1, sharp, 2.0)
    }));
    assert!(caught.is_err(), "a 5-cent offset passes");
    let c = smeared(true);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        check_smear_bound(&mut render(&c), 0.1, f_ref, 2.0)
    }));
    assert!(caught.is_err(), "a 2 Hz vibrato passes");
}

#[test]
fn knobs_ease_passes_an_eased_gain_and_catches_a_snapped_one() {
    for ease in [true, false] {
        let c = RefCell::new(Comb::new(480));
        (c.borrow_mut().dark, c.borrow_mut().ease) = (one_pole(100.0, SR as f32), ease);
        let mut set = |_: ParamId, v: f32| c.borrow_mut().level = 0.8 + 0.2 * v;
        let mut r = render(&c);
        let run = catch_unwind(AssertUnwindSafe(|| {
            check_knobs_ease(&mut r, &[ParamId(6)], &mut set)
        }));
        assert_eq!(run.is_ok(), ease, "eased {ease}");
    }
}

#[test]
fn split_holds_lines_and_a_region_at_once() {
    let mut a = Box::new(ReverbArena::new());
    let (lo, f, hi) = a.split(100, 50);
    assert_eq!((lo.len(), f.len(), hi.len()), (100, 50, RING_LEN - 200));
    lo[99] = 5;
    f[0] = 1.5;
    f[49] = -2.5;
    hi[0] = 9;
    assert_eq!(a.f32_region(100, 50)[0], 1.5);
    assert_eq!(a.f32_region(100, 50)[49], -2.5);
    assert_eq!((a.lines()[99], a.lines()[200]), (5, 9));
    let odd = catch_unwind(AssertUnwindSafe(|| {
        a.split(3, 1);
    }));
    assert!(odd.is_err());
}
