//! The reverb (FX diet spec § Reverb, § Testing), QUAD at SMEAR 0. SMEAR
//! is `reverb_smear_test`'s; GRIT is retired.

mod common;
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::halfband::HALF;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::arena::ReverbArena;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{REVERB_SPECS, ReverbParams};
use chimera_core::dsp::ring::*;
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::filter_ref::fft;
use common::{SR, rms};

const FS_RING: f32 = 24_000.0;

/// QUAD through the dispatcher, on the ring's controls.
fn run(
    rv: &mut Reverb,
    send: &[f32; BLOCK_SIZE],
    c: &RingControls,
    mix: f32,
    sample_rate: u32,
    out: &mut Stereo,
) {
    let p = ReverbParams {
        time: c.time,
        damping: c.damp,
        size: c.size,
        mix,
        ..ReverbParams::default()
    };
    rv.process(send, &p, mix, Tempo::DEFAULT, sample_rate, out);
}

/// A ring on an arena of its own.
struct Lone {
    ring: Ring,
    arena: ReverbArena,
}

impl Lone {
    fn new() -> Box<Self> {
        Box::new(Self {
            ring: Ring::new(),
            arena: ReverbArena::new(),
        })
    }

    fn process(
        &mut self,
        u: &[f32; HALF],
        blk: &RingBlock,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        self.ring.process(&mut self.arena, u, blk, l, r);
    }
}

impl core::ops::Deref for Lone {
    type Target = Ring;
    fn deref(&self) -> &Ring {
        &self.ring
    }
}

impl core::ops::DerefMut for Lone {
    fn deref_mut(&mut self) -> &mut Ring {
        &mut self.ring
    }
}

#[test]
fn every_size_step_is_distinct_primes_that_fit_their_lines() {
    for (i, row) in SIZE_TABLE.iter().enumerate() {
        for (j, &n) in row.iter().enumerate() {
            assert!(
                n >= 2 && (2..n).all(|d| n % d != 0),
                "step {i} line {j}: {n}"
            );
            assert!(n <= BASE[j], "step {i} line {j}: {n} > {}", BASE[j]);
            assert!(!row[..j].contains(&n), "step {i}: {n} twice");
        }
    }
    assert_eq!(SIZE_TABLE[31], BASE);
    assert_eq!(
        SIZE_TABLE[0],
        [53, 109, 1721, 61, 127, 1801, 73, 139, 1889, 83, 97, 1973]
    );
    let round_trip = |s| (0..STAGES).map(|k| stage_len(s, k) as u32).sum::<u32>();
    assert_eq!(round_trip(31), 23_208);
    assert_eq!(round_trip(0), 8_126);
}

#[test]
fn size_rounds_to_32_steps() {
    assert_eq!(size_step(0.0), 0);
    assert_eq!(size_step(0.5), 16);
    assert_eq!(size_step(1.0), 31);
    assert_eq!(size_step(-1.0), 0);
    assert_eq!(size_step(2.0), 31);
    for i in 0..32u8 {
        assert_eq!(size_step(i as f32 / 31.0), i);
    }
}

#[test]
fn time_floor_rises_only_at_the_largest_sizes() {
    for s in 0..=26 {
        assert_eq!(t_min(s, FS_RING), 0.3, "step {s}");
    }
    assert!((t_min(31, FS_RING) - 0.335).abs() < 1e-3);
    assert!((rt60(0.0, 15, FS_RING) - 0.3).abs() < 1e-6);
    assert!((rt60(1.0, 15, FS_RING) - 12.0).abs() < 1e-3);
    assert_eq!(rt60(0.0, 31, FS_RING), t_min(31, FS_RING));
}

#[test]
fn stage_gains_spread_the_decay_over_the_round_trip() {
    for s in [0u8, 15, 31] {
        let trip: f32 = (0..STAGES).map(|k| stage_len(s, k) as f32).sum();
        let g = stage_gains(trip / FS_RING, s, FS_RING);
        let product: f32 = g.iter().product();
        assert!((product - 1e-3).abs() < 1e-6, "step {s}: {product}");
    }
    let most = (0..32u8)
        .flat_map(|s| stage_gains(rt60(1.0, s, FS_RING), s, FS_RING))
        .fold(0.0f32, f32::max);
    assert!(most < MAX_GAIN && (most - 0.956).abs() < 1e-3, "{most}");
}

#[test]
fn damp_is_a_stable_one_pole_from_11_khz_to_1_5_khz() {
    assert!((damp_coef(0.0, FS_RING) - 0.943_85).abs() < 1e-4);
    assert!((damp_coef(1.0, FS_RING) - 0.324_77).abs() < 1e-4);
    for i in 0..=128 {
        let a = damp_coef(i as f32 / 128.0, FS_RING);
        assert!(a > 0.0 && a <= 1.0, "{i}: {a}");
    }
}

#[test]
fn the_first_reflection_is_the_shortest_tap_at_twice_the_rate() {
    assert_eq!(first_reflection(0), 240);
    assert_eq!(first_reflection(16), 470);
    assert_eq!(first_reflection(31), 686);
}

/// The lines' storage: plain i16 at a fixed 1 LSB, rounding (delays and
/// dampers) or truncating (allpasses), saturating at the range.
#[test]
fn storage_rounds_truncates_and_saturates_on_the_lsb() {
    for v in i16::MIN..=i16::MAX {
        assert_eq!((q_round_fixed(v as f32), q_trunc_fixed(v as f32)), (v, v));
    }
    assert_eq!((q_round_fixed(-100.7), q_round_fixed(100.7)), (-101, 101));
    assert_eq!((q_round_fixed(-0.5), q_round_fixed(0.5)), (-1, 1));
    assert_eq!((q_trunc_fixed(-100.7), q_trunc_fixed(100.7)), (-100, 100));
    assert_eq!((q_trunc_fixed(-0.99), q_trunc_fixed(0.99)), (0, 0));
    for q in [q_round_fixed, q_trunc_fixed] {
        assert_eq!(q(33_000.0), i16::MAX);
        assert_eq!(q(2_100_000.0), i16::MAX);
        assert_eq!(q(-40_000.0), i16::MIN);
        assert_eq!(q(-2_100_000.0), i16::MIN);
        assert_eq!(q(f32::NAN), 0);
    }
}

// ── behaviour ──

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 - 0.5
    }
}

fn blocks(seconds: f32) -> usize {
    (seconds * SR as f32 / BLOCK_SIZE as f32).ceil() as usize
}

/// The return at MIX 1: `input(n)` is send sample n, `ctl(b)` block b's
/// controls.
fn render(
    n_blocks: usize,
    mut input: impl FnMut(usize) -> f32,
    mut ctl: impl FnMut(usize) -> RingControls,
) -> (Vec<f32>, Vec<f32>) {
    let mut rv = Box::new(Reverb::new());
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut out = Stereo::SILENT;
    for b in 0..n_blocks {
        let send: [f32; BLOCK_SIZE] = core::array::from_fn(|i| input(b * BLOCK_SIZE + i));
        run(&mut rv, &send, &ctl(b), 1.0, SR, &mut out);
        l.extend_from_slice(&out.l);
        r.extend_from_slice(&out.r);
    }
    (l, r)
}

fn at(time: f32, damp: f32, size: f32) -> RingControls {
    RingControls { time, damp, size }
}

fn impulse(n: usize) -> f32 {
    if n == 0 { 1.0 } else { 0.0 }
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|&s| s as f64 * s as f64).sum()
}

/// 1 s of noise, 3 s of silence: finite, within ±12·`WET_GAIN`, and the
/// last 0.5 s at least half the target's drop below the first 0.5 s after
/// the input stops (2.5 s apart).
#[test]
fn stable_at_every_setting() {
    for time in [0.0, 0.5, 1.0] {
        for size in [0.0, 0.5, 1.0] {
            for damp in [0.0, 0.5, 1.0] {
                let what = format!("TIME {time} SIZE {size} DAMP {damp}");
                let mut n = Noise(0x1234_5678);
                let c = at(time, damp, size);
                let (l, r) = render(
                    blocks(4.0),
                    |i| if i < SR as usize { n.next() } else { 0.0 },
                    |_| c,
                );
                for &s in l.iter().chain(&r) {
                    assert!(s.is_finite() && s.abs() <= 12.0 * WET_GAIN, "{what}: {s}");
                }
                let (s, w) = (SR as usize, SR as usize / 2);
                let first = energy(&l[s..s + w]) + energy(&r[s..s + w]);
                let last = energy(&l[l.len() - w..]) + energy(&r[r.len() - w..]);
                let predicted = 60.0 * 2.5 / rt60(time, size_step(size), FS_RING);
                let drop = 10.0 * (first / last).log10() as f32;
                assert!(
                    last == 0.0 || drop >= predicted / 2.0,
                    "{what}: fell {drop} dB, want {predicted} / 2"
                );
            }
        }
    }
}

/// Truncation in the allpasses leaves no limit cycle: silence in reaches
/// exact zeros out, with no gate.
fn falls_silent_within(seconds: f32) {
    let mut rv = Box::new(Reverb::new());
    let mut n = Noise(0xdead_beef);
    let c = at(1.0, 0.0, 1.0);
    let mut out = Stereo::SILENT;
    for _ in 0..blocks(1.0) {
        let send = core::array::from_fn(|_| n.next());
        run(&mut rv, &send, &c, 1.0, SR, &mut out);
    }
    let silence = [0.0; BLOCK_SIZE];
    let mut b = 0;
    while !rv.quad().unwrap().ring().is_silent(rv.arena()) {
        run(&mut rv, &silence, &c, 1.0, SR, &mut out);
        b += 1;
        assert!(b <= blocks(seconds), "still ringing after {seconds} s");
    }
    for _ in 0..blocks(0.1) {
        run(&mut rv, &silence, &c, 1.0, SR, &mut out);
        assert!(out.l.iter().chain(&out.r).all(|&s| s == 0.0));
    }
}

#[test]
fn no_limit_cycle() {
    falls_silent_within(25.0);
}

/// −6 dBFS noise (uniform, 0.5 RMS) for `seconds`, then silence.
fn noise_burst(seed: u32, seconds: f32) -> impl FnMut(usize) -> f32 {
    let mut n = Noise(seed);
    let end = (seconds * SR as f32) as usize;
    move |i| if i < end { 3f32.sqrt() * n.next() } else { 0.0 }
}

/// A tail at a normal level is never zeroed early: every block sounds under
/// input, and for at least half the target RT60 at TIME 0.5 after it.
#[test]
fn a_normal_tail_is_never_zeroed_early() {
    {
        let c = RingControls::default();
        let hold = 0.5 * rt60(0.5, size_step(c.size), FS_RING);
        let (l, r) = render(blocks(0.5 + hold), noise_burst(3, 0.5), |_| c);
        let first = blocks(0.05);
        for (b, (bl, br)) in l.chunks(BLOCK_SIZE).zip(r.chunks(BLOCK_SIZE)).enumerate() {
            if b >= first {
                assert!(bl.iter().chain(br).any(|&s| s != 0.0), "block {b} silent");
            }
        }
    }
}

/// Hann-windowed power per bin, 0..=n/2, of `x` (a power of two long).
fn power(x: &[f32]) -> Vec<f64> {
    let n = x.len();
    let hann = |i: usize| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &s)| s as f64 * hann(i))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (0..=n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}

fn bin(hz: f32, n: usize) -> usize {
    (hz * n as f32 / SR as f32) as usize
}

fn mean(p: &[f64]) -> f64 {
    p.iter().sum::<f64>() / p.len() as f64
}

#[test]
fn the_return_is_band_limited() {
    let mut n = Noise(7);
    let (l, r) = render(blocks(2.0), |_| n.next(), |_| at(0.5, 0.0, 0.5));
    let n = 1 << 16;
    for x in [&l, &r] {
        let p = power(&x[x.len() - n..]);
        let band = mean(&p[bin(500.0, n)..bin(8_000.0, n)]);
        let top = mean(&p[bin(13_500.0, n)..]);
        let db = 10.0 * (band / top).log10();
        assert!(db >= 40.0, "{db} dB");
    }
}

/// `x` band-passed to 500 Hz–4 kHz by FFT.
fn band(x: &[f32]) -> Vec<f64> {
    let n = x.len().next_power_of_two();
    let mut re: Vec<f64> = x.iter().map(|&s| s as f64).collect();
    re.resize(n, 0.0);
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let (lo, hi) = (bin(500.0, n), bin(4_000.0, n));
    for k in 0..n {
        let f = k.min(n - k);
        if f < lo || f > hi {
            (re[k], im[k]) = (0.0, 0.0);
        }
    }
    im.iter_mut().for_each(|v| *v = -*v);
    fft(&mut re, &mut im);
    re.truncate(x.len());
    re
}

/// RT60 of the tail from `start`: each side band-passed, the Schroeder EDC
/// of L² + R², a least-squares line from −5 to −35 dB.
fn rt60_of(l: &[f32], r: &[f32], start: usize) -> f32 {
    let (l, r) = (band(l), band(r));
    let mut edc: Vec<f64> = (start..l.len())
        .map(|i| l[i] * l[i] + r[i] * r[i])
        .collect();
    for i in (0..edc.len() - 1).rev() {
        edc[i] += edc[i + 1];
    }
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &e) in edc.iter().enumerate() {
        let db = 10.0 * (e / edc[0]).log10();
        if (-35.0..=-5.0).contains(&db) {
            let x = i as f64 / SR as f64;
            (n, sx, sy, sxx, sxy) = (n + 1.0, sx + x, sy + db, sxx + x * x, sxy + x * db);
        }
    }
    (-60.0 * (n * sxx - sx * sx) / (n * sxy - sx * sy)) as f32
}

/// 0.5 s of −6 dBFS noise; an impulse leaves the tail at the grid's floor.
#[test]
fn rt60_follows_time() {
    for step in [0u8, 15, 31] {
        let mut last = 0.0;
        for time in [0.25, 0.5, 0.75, 1.0] {
            let target = rt60(time, step, FS_RING);
            let c = at(time, 0.0, step as f32 / 31.0);
            let (l, r) = render(
                blocks(0.5 + (1.3 * target).max(1.5)),
                noise_burst(0x5eed, 0.5),
                |_| c,
            );
            let t = rt60_of(&l, &r, SR as usize / 2);
            println!("step {step} TIME {time}: RT60 {t:.3} s, target {target:.3} s");
            assert!(
                (t / target - 1.0).abs() <= 0.35,
                "step {step} TIME {time}: RT60 {t} s, target {target} s"
            );
            assert!(t > last, "step {step}: RT60 not rising at TIME {time}");
            last = t;
        }
    }
}

#[test]
fn each_side_arrives_early_and_the_sides_decorrelate() {
    let c = RingControls::default();
    let (l, r) = render(blocks(1.0), impulse, |_| c);
    let w = SR as usize / 100;
    for (side, x) in [("L", &l), ("R", &r)] {
        let windows: Vec<f32> = x.chunks(w).map(rms).collect();
        let peak = windows.iter().copied().fold(0.0, f32::max);
        let early = windows[..5].iter().copied().fold(0.0, f32::max);
        assert!(early >= 0.1 * peak, "{side}: {early} < {peak} / 10");
    }
    let s = SR as usize / 2;
    let (a, b) = (&l[s..][..SR as usize / 10], &r[s..][..SR as usize / 10]);
    let xy: f64 = a.iter().zip(b).map(|(&x, &y)| x as f64 * y as f64).sum();
    let corr = xy / (energy(a) * energy(b)).sqrt();
    assert!(corr.abs() < 0.5, "{corr}");
}

fn max_d2(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

/// A move at 0.7 s, mid-tail after 0.5 s of noise: over the next 50 ms the
/// largest second difference is at most 1.5× that of a held render at
/// either value.
fn no_click(what: &str, from: RingControls, to: RingControls) {
    let at_move = blocks(0.7);
    let run = |ctl: &dyn Fn(usize) -> RingControls| {
        let mut n = Noise(4242);
        let (l, r) = render(
            at_move + blocks(0.05),
            |i| if i < SR as usize / 2 { n.next() } else { 0.0 },
            ctl,
        );
        let s = at_move * BLOCK_SIZE - 2;
        max_d2(&l[s..]).max(max_d2(&r[s..]))
    };
    let (old, new) = (run(&|_| from), run(&|_| to));
    let moved = run(&|b| if b < at_move { from } else { to });
    assert!(
        moved <= 1.5 * old.max(new),
        "{what}: {moved} vs held {old} / {new}"
    );
}

#[test]
fn moving_a_control_mid_tail_does_not_click() {
    type Set = fn(&mut RingControls, f32);
    let sets: [(&str, Set, f32); 3] = [
        ("TIME", |c, v| c.time = v, 1.0 / 128.0),
        ("DAMP", |c, v| c.damp = v, 1.0 / 128.0),
        ("SIZE", |c, v| c.size = v, 1.0 / 31.0),
    ];
    for (name, set, step) in sets {
        let (mut a, mut b) = (RingControls::default(), RingControls::default());
        set(&mut a, 16.0 / 31.0);
        set(&mut b, 16.0 / 31.0 + step);
        no_click(&format!("{name} one step"), a, b);
        set(&mut a, 0.0);
        set(&mut b, 1.0);
        no_click(&format!("{name} full sweep"), a, b);
    }
}

#[test]
fn nothing_returns_before_the_first_reflection() {
    for step in [0u8, 16, 31] {
        let c = RingControls {
            size: step as f32 / 31.0,
            ..RingControls::default()
        };
        let (l, r) = render(blocks(0.2), impulse, |_| c);
        let first = first_reflection(step);
        let heard = l
            .iter()
            .zip(&r)
            .position(|(a, b)| *a != 0.0 || *b != 0.0)
            .unwrap();
        assert!(
            heard >= first && heard < first + 200,
            "step {step}: {heard} vs {first}"
        );
    }
}

// ── SIZE crossfade, DAMP, signs ──

fn held_block() -> RingBlock {
    RingBlock {
        gains_from: [0.9; STAGES],
        gains_to: [0.9; STAGES],
        damp_from: 0.5,
        damp_to: 0.5,
        smear: None,
    }
}

/// A ring at SIZE step 20 after 1 s of loud noise: twins built by this are
/// in the same state.
fn filled() -> Box<Lone> {
    let mut ring = Lone::new();
    ring.snap(20);
    let mut n = Noise(0xface);
    let (mut l, mut r) = ([0.0; HALF], [0.0; HALF]);
    for _ in 0..(FS_RING as usize / HALF) {
        let u = core::array::from_fn(|_| 2.0 * n.next());
        ring.process(&u, &held_block(), &mut l, &mut r);
    }
    ring
}

/// One silent block: (L, R).
fn tick(ring: &mut Lone) -> ([f32; HALF], [f32; HALF]) {
    let (mut l, mut r) = ([0.0; HALF], [0.0; HALF]);
    ring.process(&[0.0; HALF], &held_block(), &mut l, &mut r);
    (l, r)
}

#[test]
fn a_size_change_fades_linearly_from_the_old_length_over_xfade() {
    let (mut old, mut new, mut fade) = (filled(), filled(), filled());
    new.snap(25);
    fade.request(25);
    assert!(fade.crossfading() && fade.step() == 25);

    // The first 64 samples' taps read only what was written before the
    // request (the shortest tap at step 20 is 264 samples), so the fade's
    // output is old + w·(new − old), with w = i / XFADE.
    let blocks_read = 2;
    for b in 0..blocks_read {
        let ((ol, or), (nl, nr), (fl, fr)) = (tick(&mut old), tick(&mut new), tick(&mut fade));
        for i in 0..HALF {
            let n = b * HALF + i;
            if n == 0 {
                assert_eq!(
                    (fl[0], fr[0]),
                    (ol[0], or[0]),
                    "the first sample is the old read"
                );
            }
            for (o, nw, f) in [(ol[i], nl[i], fl[i]), (or[i], nr[i], fr[i])] {
                if (nw - o).abs() > 100.0 {
                    let w = (f - o) / (nw - o);
                    let want = n as f32 / XFADE as f32;
                    assert!((w - want).abs() < 1e-4, "sample {n}: w {w}, want {want}");
                }
            }
        }
    }

    // Exactly XFADE samples: still fading at the last whole block before
    // 720, done by the block that holds sample 720.
    let before = XFADE as usize / HALF;
    for _ in blocks_read..before {
        tick(&mut fade);
    }
    assert!(fade.crossfading());
    tick(&mut fade);
    assert!(!fade.crossfading());
}

#[test]
fn a_request_mid_fade_neither_restarts_nor_reverses_it() {
    let (mut a, mut b) = (filled(), filled());
    a.request(25);
    b.request(25);
    for n in 0..2 * (XFADE as usize / HALF + 1) {
        if n == 5 {
            b.request(3);
            assert_eq!(b.step(), 25);
        }
        assert_eq!(tick(&mut a), tick(&mut b), "block {n}");
    }
    assert!(!b.crossfading());
    b.request(3);
    assert!(b.crossfading() && b.step() == 3);
}

/// The share of 4–10 kHz in the tail's 20 Hz–10 kHz energy, in dB, 0.2 s
/// after 0.5 s of noise stops.
fn high_share_db(damp: f32) -> f64 {
    let c = at(0.5, damp, 0.5);
    let (l, _) = render(blocks(0.5 + 0.2 + 0.35), noise_burst(21, 0.5), |_| c);
    let n = 1 << 14;
    let p = power(&l[(0.7 * SR as f32) as usize..][..n]);
    let high: f64 = p[bin(4_000.0, n)..bin(10_000.0, n)].iter().sum();
    let all: f64 = p[bin(20.0, n)..bin(10_000.0, n)].iter().sum();
    10.0 * (high / all).log10()
}

#[test]
fn damp_darkens_the_tail() {
    let (open, dark) = (high_share_db(0.0), high_share_db(1.0));
    println!("4–10 kHz share: DAMP 0 {open:.1} dB, DAMP 1 {dark:.1} dB");
    assert!(dark <= open - 10.0, "DAMP 0 {open} dB, DAMP 1 {dark} dB");
}

#[test]
fn the_signs_match_the_spec() {
    // The shorter allpass of each stage is +c, the longer −c.
    for k in 0..STAGES {
        let [c1, c2] = AP_COEF[k];
        let (short, long) = if BASE[3 * k] < BASE[3 * k + 1] {
            (c1, c2)
        } else {
            (c2, c1)
        };
        assert!(short > 0.0 && long == -short, "stage {k}: {:?}", AP_COEF[k]);
    }
    let signs = |s: usize| TAPS[s].map(|(_, _, sign)| sign);
    assert_eq!(signs(0), [1.0, -1.0, 1.0]);
    assert_eq!(signs(1), [1.0, -1.0, 1.0]);
    assert_eq!(INJECT, [0.5, 0.0, -0.5, 0.0]);
}

// ── WET_GAIN (spec § Topology) ──

/// The plate's return RMS for `calibration_send` at TIME 0.5, DAMP 0.3,
/// SIZE 0.5, MIX 1, recorded before the plate was deleted.
const PLATE_RMS: f32 = 0.340_919_05;

/// 0.5 s of noise, then silence.
fn calibration_send(n: &mut Noise, i: usize) -> f32 {
    if i < 24_000 { n.next() } else { 0.0 }
}

#[test]
fn wet_gain_matches_the_plate_within_1_db() {
    let mut n = Noise(0x5eed);
    let (l, r) = render(
        blocks(2.0),
        |i| calibration_send(&mut n, i),
        |_| RingControls::default(),
    );
    let ring = ((energy(&l) + energy(&r)) / (2.0 * l.len() as f64)).sqrt();
    let db = 20.0 * (ring / PLATE_RMS as f64).log10();
    assert!(db.abs() <= 1.0, "{db} dB");
}

// ── Review Focus ──

#[test]
fn a_size_spin_settles_on_the_last_step() {
    let mut rv = Box::new(Reverb::new());
    let mut n = Noise(5);
    let mut out = Stereo::SILENT;
    for b in 0..200usize {
        let c = RingControls {
            size: (b % 32) as f32 / 31.0,
            ..RingControls::default()
        };
        let send = core::array::from_fn(|_| n.next());
        run(&mut rv, &send, &c, 1.0, SR, &mut out);
        assert!(out.l.iter().chain(&out.r).all(|s| s.is_finite()));
    }
    let last = RingControls {
        size: 7.0 / 31.0,
        ..RingControls::default()
    };
    for _ in 0..2 * (XFADE as usize / HALF + 1) {
        run(&mut rv, &[0.0; BLOCK_SIZE], &last, 1.0, SR, &mut out);
    }
    assert_eq!(rv.quad().unwrap().ring().step(), 7);
    assert!(!rv.quad().unwrap().ring().crossfading());
}

#[test]
fn six_full_scale_parts_saturate_without_wrapping() {
    let mut n = Noise(17);
    let c = at(1.0, 0.0, 1.0);
    let (l, r) = render(
        blocks(3.0),
        |i| {
            if i < SR as usize {
                6.0 * n.next().signum()
            } else {
                0.0
            }
        },
        |_| c,
    );
    assert!(
        l.iter()
            .chain(&r)
            .all(|s| s.is_finite() && s.abs() <= 12.0 * WET_GAIN)
    );
    let w = SR as usize / 2;
    let (first, last) = (energy(&l[SR as usize..][..w]), energy(&l[l.len() - w..]));
    assert!(last < first, "{first} → {last}");
}

#[test]
fn nan_and_out_of_range_controls_render_finite() {
    let wild = [
        at(f32::NAN, f32::NAN, f32::NAN),
        at(9.0, -1.0, 7.0),
        at(f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY),
    ];
    for c in wild {
        let mut n = Noise(11);
        let (l, r) = render(
            blocks(1.0),
            |_| n.next(),
            |b| {
                if b % 50 < 25 {
                    c
                } else {
                    RingControls::default()
                }
            },
        );
        assert!(
            l.iter()
                .chain(&r)
                .all(|s| s.is_finite() && s.abs() <= 12.0 * WET_GAIN),
            "{c:?}"
        );
        assert!(
            energy(&l[l.len() - 1_000..]) > 0.0,
            "{c:?}: the reverb went dead"
        );
    }
}

#[test]
fn runs_at_44_1_khz() {
    let mut rv = Box::new(Reverb::new());
    let mut n = Noise(13);
    let c = at(0.5, 0.3, 1.0);
    let mut out = Stereo::SILENT;
    let (mut first, mut last) = (0.0, 0.0);
    let second = 44_100 / BLOCK_SIZE;
    for b in 0..4 * second {
        let send = core::array::from_fn(|_| if b < second { n.next() } else { 0.0 });
        run(&mut rv, &send, &c, 1.0, 44_100, &mut out);
        assert!(out.l.iter().chain(&out.r).all(|s| s.is_finite()));
        let e = energy(&out.l) + energy(&out.r);
        if b == second + 5 {
            first = e;
        }
        last = e;
    }
    assert!(last < first * 1e-3, "{first} → {last}");
}

// ── settings ──

#[test]
fn reverb_params_default_to_the_spec_and_retire_type() {
    let p = ReverbParams::default();
    for s in REVERB_SPECS.iter() {
        assert_eq!(p.get(s.id), s.default, "{}", s.label);
        assert_ne!(s.id, ParamId(0), "the old TYPE's id is retired (ADR 0009)");
    }
    assert_eq!(p.mix, 0.0);
    // Slot 3 ticks 1/128 (GATE's LENGTH); QUAD's panel keeps its 1/31
    // (`reverb_panel_test`).
    let size = REVERB_SPECS
        .iter()
        .find(|s| s.id == ReverbParams::SIZE)
        .unwrap();
    assert_eq!(size.step, 1.0 / 128.0);
    assert_eq!(p.controls(), RingControls::default());
}

/// GRIT (id 5) is retired; SMEAR (11) and RATE (12) are QUAD's new knobs
/// (reverb TYPEs spec § Parameters).
#[test]
fn smear_and_rate_are_params_11_and_12() {
    let mut p = ReverbParams::default();
    assert_eq!(
        (ReverbParams::SMEAR, ReverbParams::RATE),
        (ParamId(11), ParamId(12))
    );
    assert_eq!(
        (p.get(ReverbParams::SMEAR), p.get(ReverbParams::RATE)),
        (0.0, 0.75)
    );
    p.write(ReverbParams::SMEAR, 0.8);
    p.write(ReverbParams::RATE, 0.2);
    assert_eq!((p.smear, p.rate), (0.8, 0.2));
    assert_eq!(p.get(ParamId(5)), 0.0);
    let ids: Vec<ParamId> = REVERB_SPECS.iter().map(|s| s.id).collect();
    assert!(!ids.contains(&ParamId(5)));
    let at = |id| REVERB_SPECS.iter().position(|s| s.id == id).unwrap();
    for (s, label) in [
        &REVERB_SPECS[at(ReverbParams::SMEAR)],
        &REVERB_SPECS[at(ReverbParams::RATE)],
    ]
    .into_iter()
    .zip(["SMEAR", "RATE"])
    {
        assert_eq!(
            (s.label, s.min, s.max, s.step),
            (label, 0.0, 1.0, 1.0 / 128.0)
        );
    }
}

#[test]
fn a_voice_through_the_reverb_leaves_a_tail() {
    use chimera_core::dsp::algo::params::AlgoParams;
    use chimera_core::dsp::algo::waves::WaveId;
    use chimera_core::modulation::ModState;
    use chimera_core::params::{EngineType, ParamSnapshot};
    use chimera_core::{MidiNote, Velocity};
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::TRI);
    let m = ModState::new();
    let mut voice = Rig::new(SR);
    let mut rv = Box::new(Reverb::new());
    let c = RingControls {
        time: 0.7,
        ..RingControls::default()
    };
    let (mut block, mut out) = ([0.0f32; BLOCK_SIZE], Stereo::SILENT);
    voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
    for _ in 0..8 {
        voice.render(&mut block, &p, &m);
        run(&mut rv, &block, &c, 0.5, SR, &mut out);
    }
    voice.note_off();
    let mut tail = 0.0;
    for _ in 0..64 {
        voice.render(&mut block, &p, &m);
        run(&mut rv, &block, &c, 0.5, SR, &mut out);
        tail += energy(&out.l) + energy(&out.r);
    }
    assert!(tail > 0.01, "{tail}");
}

/// `rnd`'s portable path against std's round, ties away from zero, with
/// `as` saturating: near every half and whole number to ±300 and the
/// 2^22..2^24 and 2^31 edges, and a stride through every bit pattern.
fn rounding_matches(x: f32) {
    let want = x.round();
    let got = rint_portable(x);
    assert!(
        got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan()),
        "rint({x:e}) = {got:e}, want {want:e}"
    );
    assert_eq!(rnd_portable(x), want as i32, "rnd({x:e})");
    assert_eq!(rnd(x), want as i32, "rnd({x:e})");
    assert_eq!(rint(x).to_bits(), got.to_bits(), "rint({x:e})");
}

fn around(x: f32, ulps: i32) -> impl Iterator<Item = f32> {
    (-ulps..=ulps).map(move |d| f32::from_bits((x.to_bits() as i32 + d) as u32))
}

#[test]
fn rounding_is_ties_away_from_zero() {
    for k in -600..=600 {
        let x = k as f32 * 0.5;
        around(x, 64).for_each(rounding_matches);
    }
    for e in [22, 23, 24, 31] {
        let x = (1u64 << e) as f32;
        around(x, 64)
            .chain(around(-x, 64))
            .for_each(rounding_matches);
    }
    for x in [
        0.0,
        -0.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MAX,
        f32::MIN,
    ] {
        rounding_matches(x);
    }
    (0..=u32::MAX)
        .step_by(4099)
        .map(f32::from_bits)
        .for_each(rounding_matches);
}

/// Every f32: run by hand, in release.
#[test]
#[ignore]
fn rounding_is_ties_away_from_zero_everywhere() {
    (0..=u32::MAX)
        .map(f32::from_bits)
        .for_each(rounding_matches);
}

/// The voice budget's reserve for the reverb (`RevBill`): a TYPE's extra
/// over QUAD at rest, both TYPEs' and the switch's while one runs.
#[test]
fn the_reverb_bill_covers_a_switch_at_both_types() {
    use chimera_core::dsp::rev::{RevBill, SWITCH_BILLED, SWITCH_COST, extra_cost};
    use chimera_core::dsp::reverb::ReverbType as T;
    assert_eq!(extra_cost(T::Quad).0, 0);
    for t in [T::Gate, T::Reverse, T::Room, T::Hall, T::Bloom] {
        assert!(extra_cost(t) > extra_cost(T::Quad));
    }
    // The bench's measured REV rows less QUAD's 73 (2026-10-09 bench extras).
    assert_eq!(
        [T::Gate, T::Reverse, T::Room, T::Hall, T::Bloom].map(|t| extra_cost(t).0),
        [96, 112, 193, 201, 206]
    );
    let mut b = RevBill::new();
    assert_eq!(b.step(T::Quad), extra_cost(T::Quad));
    assert_eq!(
        b.step(T::Room),
        extra_cost(T::Quad) + extra_cost(T::Room) + SWITCH_COST
    );
    for _ in 1..SWITCH_BILLED {
        assert_eq!(b.step(T::Room).0, 193 + SWITCH_COST.0);
    }
    assert_eq!(b.step(T::Room), extra_cost(T::Room), "settled on ROOM");
    // HALL to BLOOM, the dearest pair, is the bench's worst switch block.
    b.step(T::Hall);
    for _ in 0..SWITCH_BILLED {
        b.step(T::Hall);
    }
    assert_eq!(b.step(T::Bloom).0, 201 + 206 + SWITCH_COST.0);
    // A switch on a switch keeps the costlier engine still fading.
    assert_eq!(b.step(T::Gate).0, 206 + 96 + SWITCH_COST.0);
}
