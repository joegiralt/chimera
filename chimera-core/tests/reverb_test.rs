//! The reverb (FX diet spec § Reverb, § Testing). GRIT 0 wherever RT60
//! or stereo is measured.

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::halfband::HALF;
use chimera_core::dsp::reverb::{REVERB_SPECS, ReverbParams};
use chimera_core::dsp::ring::*;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const FS_RING: f32 = 24_000.0;

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
fn grit_rounds_onto_its_grid() {
    let g0 = Grid::new(0.0);
    assert_eq!(g0.delta(), 1.0);
    for v in i16::MIN..=i16::MAX {
        assert_eq!(g0.q_round(v as f32), v);
    }
    assert_eq!(g0.q_round(-100.7), -101);
    assert_eq!(g0.q_round(100.7), 101);
    assert_eq!(g0.q_round(-0.5), -1);
    assert_eq!(g0.q_round(0.5), 1);
    let g1 = Grid::new(1.0);
    assert_eq!(g1.delta(), 64.0);
    assert_eq!(g1.q_round(32.0), 64);
    assert_eq!(g1.q_round(-32.0), -64);
    assert_eq!(g1.q_round(31.9), 0);
    assert_eq!(g1.q_round(-31.9), 0);
    assert_eq!(Grid::new(0.5).delta(), 8.0);
    for i in 0..=100 {
        let g = Grid::new(i as f32 / 100.0);
        for x in [-30_000.3f32, -777.7, -1.5, 0.0, 2.5, 999.9, 30_000.1] {
            // The grid point, rounded (never truncated) to an LSB.
            let p = (x / g.delta()).round() * g.delta();
            assert!((g.q_round(x) as f32 - p).abs() <= 0.501, "GRIT {i}: {x}");
            assert!(
                (g.q_round(x) as f32 - x).abs() <= g.delta() / 2.0 + 0.501,
                "GRIT {i}: {x}"
            );
        }
    }
}

#[test]
fn grit_truncates_toward_zero_onto_its_grid() {
    let g0 = Grid::new(0.0);
    for v in i16::MIN..=i16::MAX {
        assert_eq!(g0.q_trunc(v as f32), v);
    }
    assert_eq!(g0.q_trunc(-100.7), -100);
    assert_eq!(g0.q_trunc(100.7), 100);
    assert_eq!(g0.q_trunc(0.99), 0);
    assert_eq!(g0.q_trunc(-0.99), 0);
    let g1 = Grid::new(1.0);
    assert_eq!(g1.q_trunc(64.0), 64);
    assert_eq!(g1.q_trunc(127.9), 64);
    assert_eq!(g1.q_trunc(-127.9), -64);
    assert_eq!(g1.q_trunc(63.9), 0);
    assert_eq!(g1.q_trunc(-63.9), 0);
    for i in 0..=100 {
        let g = Grid::new(i as f32 / 100.0);
        for x in [-30_000.3f32, -777.7, -1.5, 0.0, 2.5, 999.9, 30_000.1] {
            let q = g.q_trunc(x) as f32;
            // Never grows, never flips sign, loses under a step plus an LSB.
            assert!(q.abs() <= x.abs() && q * x >= 0.0, "GRIT {i}: {x} → {q}");
            assert!(x.abs() - q.abs() < g.delta() + 1.0, "GRIT {i}: {x} → {q}");
        }
    }
}

#[test]
fn the_first_reflection_is_the_shortest_tap_at_twice_the_rate() {
    assert_eq!(first_reflection(0), 240);
    assert_eq!(first_reflection(16), 470);
    assert_eq!(first_reflection(31), 686);
}

#[test]
fn grit_saturates_past_the_i16_range() {
    for grit in [0.0, 0.5, 1.0] {
        let g = Grid::new(grit);
        for q in [Grid::q_round, Grid::q_trunc] {
            assert_eq!(q(g, 33_000.0), i16::MAX, "grit {grit}");
            assert_eq!(q(g, 2_100_000.0), i16::MAX, "grit {grit}");
            assert_eq!(q(g, -40_000.0), i16::MIN, "grit {grit}");
            assert_eq!(q(g, -2_100_000.0), i16::MIN, "grit {grit}");
        }
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
    let mut rv = Box::new(RingReverb::new());
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut out = Stereo::SILENT;
    for b in 0..n_blocks {
        let send: [f32; BLOCK_SIZE] = core::array::from_fn(|i| input(b * BLOCK_SIZE + i));
        rv.process(&send, &ctl(b), 1.0, SR, &mut out);
        l.extend_from_slice(&out.l);
        r.extend_from_slice(&out.r);
    }
    (l, r)
}

fn at(grit: f32, time: f32, damp: f32, size: f32) -> RingControls {
    RingControls {
        grit,
        time,
        damp,
        size,
    }
}

fn impulse(n: usize) -> f32 {
    if n == 0 { 1.0 } else { 0.0 }
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|&s| s as f64 * s as f64).sum()
}

fn rms(x: &[f32]) -> f32 {
    (energy(x) / x.len() as f64).sqrt() as f32
}

/// 1 s of noise, 3 s of silence: finite, within ±12·`WET_GAIN`, and the
/// last 0.5 s at least half the target's drop below the first 0.5 s after
/// the input stops (2.5 s apart).
fn stable(grit: f32) {
    for time in [0.0, 0.5, 1.0] {
        for size in [0.0, 0.5, 1.0] {
            for damp in [0.0, 0.5, 1.0] {
                let what = format!("GRIT {grit} TIME {time} SIZE {size} DAMP {damp}");
                let mut n = Noise(0x1234_5678);
                let c = at(grit, time, damp, size);
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

#[test]
fn stable_at_grit_0() {
    stable(0.0);
}

#[test]
fn stable_at_grit_1() {
    stable(1.0);
}

/// Truncation in the allpasses leaves no limit cycle: silence in reaches
/// exact zeros out, with no gate.
fn falls_silent_within(grit: f32, seconds: f32) {
    let mut rv = Box::new(RingReverb::new());
    let mut n = Noise(0xdead_beef);
    let c = at(grit, 1.0, 0.0, 1.0);
    let mut out = Stereo::SILENT;
    for _ in 0..blocks(1.0) {
        let send = core::array::from_fn(|_| n.next());
        rv.process(&send, &c, 1.0, SR, &mut out);
    }
    let silence = [0.0; BLOCK_SIZE];
    let mut b = 0;
    while !rv.ring().is_silent() {
        rv.process(&silence, &c, 1.0, SR, &mut out);
        b += 1;
        assert!(
            b <= blocks(seconds),
            "GRIT {grit}: still ringing after {seconds} s"
        );
    }
    for _ in 0..blocks(0.1) {
        rv.process(&silence, &c, 1.0, SR, &mut out);
        assert!(out.l.iter().chain(&out.r).all(|&s| s == 0.0));
    }
}

#[test]
fn no_limit_cycle_at_grit_1() {
    falls_silent_within(1.0, 15.0);
}

#[test]
fn no_limit_cycle_at_grit_0() {
    falls_silent_within(0.0, 25.0);
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
    for grit in [0.0, 0.3, 1.0] {
        let c = RingControls {
            grit,
            ..RingControls::default()
        };
        let hold = 0.5 * rt60(0.5, size_step(c.size), FS_RING);
        let (l, r) = render(blocks(0.5 + hold), noise_burst(3, 0.5), |_| c);
        let first = blocks(0.05);
        for (b, (bl, br)) in l.chunks(BLOCK_SIZE).zip(r.chunks(BLOCK_SIZE)).enumerate() {
            if b >= first {
                assert!(
                    bl.iter().chain(br).any(|&s| s != 0.0),
                    "GRIT {grit}: block {b} silent"
                );
            }
        }
    }
}

/// In-place iterative radix-2 FFT; `re.len()` is a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
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
    let (l, r) = render(blocks(2.0), |_| n.next(), |_| at(0.0, 0.5, 0.0, 0.5));
    let n = 1 << 16;
    for x in [&l, &r] {
        let p = power(&x[x.len() - n..]);
        let band = mean(&p[bin(500.0, n)..bin(8_000.0, n)]);
        let top = mean(&p[bin(13_500.0, n)..]);
        let db = 10.0 * (band / top).log10();
        assert!(db >= 40.0, "{db} dB");
    }
}

#[test]
fn grit_1_stores_only_multiples_of_64() {
    let mut ring = Box::new(Ring::new());
    ring.snap(31);
    let blk = RingBlock {
        gains_from: [0.9; STAGES],
        gains_to: [0.9; STAGES],
        damp_from: 0.5,
        damp_to: 0.5,
        grid: Grid::new(1.0),
    };
    let mut n = Noise(99);
    let (mut l, mut r) = ([0.0; HALF], [0.0; HALF]);
    for _ in 0..2_000 {
        let u = core::array::from_fn(|_| n.next());
        ring.process(&u, &blk, &mut l, &mut r);
    }
    assert!(ring.lines().iter().any(|&s| s != 0));
    assert!(ring.lines().iter().all(|&s| s % 64 == 0));
    assert!(ring.damp_state().iter().all(|&s| s % 64.0 == 0.0));
}

/// Energy from 20 Hz to 10.5 kHz outside 1 kHz ± 50 Hz, 2 s into a steady
/// −12 dB 1 kHz sine, in dB.
fn floor_db(grit: f32) -> f64 {
    let sine =
        |i: usize| 0.251 * (2.0 * core::f32::consts::PI * 1_000.0 * i as f32 / SR as f32).sin();
    let c = RingControls {
        grit,
        ..RingControls::default()
    };
    let (l, _) = render(blocks(3.4), sine, |_| c);
    let n = 1 << 16;
    let p = power(&l[2 * SR as usize..][..n]);
    let (lo, hi) = (bin(950.0, n), bin(1_050.0, n));
    let floor: f64 = p[bin(20.0, n)..lo]
        .iter()
        .chain(&p[hi..bin(10_500.0, n)])
        .sum();
    10.0 * floor.log10()
}

#[test]
fn grit_raises_the_noise_floor() {
    let floors: Vec<f64> = (0..=10).map(|i| floor_db(i as f32 / 10.0)).collect();
    for w in floors.windows(2) {
        assert!(w[1] >= w[0] - 0.5, "{floors:?}");
    }
    assert!(floors[10] - floors[0] >= 24.0, "{floors:?}");
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
            let c = at(0.0, time, 0.0, step as f32 / 31.0);
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
    let c = RingControls {
        grit: 0.0,
        ..RingControls::default()
    };
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
    let sets: [(&str, Set, f32); 4] = [
        ("TIME", |c, v| c.time = v, 1.0 / 128.0),
        ("DAMP", |c, v| c.damp = v, 1.0 / 128.0),
        ("GRIT", |c, v| c.grit = v, 1.0 / 128.0),
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
        grid: Grid::new(0.0),
    }
}

/// A ring at SIZE step 20 after 1 s of loud noise: twins built by this are
/// in the same state.
fn filled() -> Box<Ring> {
    let mut ring = Box::new(Ring::new());
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
fn tick(ring: &mut Ring) -> ([f32; HALF], [f32; HALF]) {
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
    let c = at(0.0, 0.5, damp, 0.5);
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
    let mut rv = Box::new(RingReverb::new());
    let mut n = Noise(5);
    let mut out = Stereo::SILENT;
    for b in 0..200usize {
        let c = RingControls {
            size: (b % 32) as f32 / 31.0,
            ..RingControls::default()
        };
        let send = core::array::from_fn(|_| n.next());
        rv.process(&send, &c, 1.0, SR, &mut out);
        assert!(out.l.iter().chain(&out.r).all(|s| s.is_finite()));
    }
    let last = RingControls {
        size: 7.0 / 31.0,
        ..RingControls::default()
    };
    for _ in 0..2 * (XFADE as usize / HALF + 1) {
        rv.process(&[0.0; BLOCK_SIZE], &last, 1.0, SR, &mut out);
    }
    assert_eq!(rv.ring().step(), 7);
    assert!(!rv.ring().crossfading());
}

#[test]
fn six_full_scale_parts_saturate_without_wrapping() {
    let mut n = Noise(17);
    let c = at(0.0, 1.0, 0.0, 1.0);
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
        at(f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        at(-3.0, 9.0, -1.0, 7.0),
        at(
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ),
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
    let mut rv = Box::new(RingReverb::new());
    let mut n = Noise(13);
    let c = at(0.0, 0.5, 0.3, 1.0);
    let mut out = Stereo::SILENT;
    let (mut first, mut last) = (0.0, 0.0);
    let second = 44_100 / BLOCK_SIZE;
    for b in 0..4 * second {
        let send = core::array::from_fn(|_| if b < second { n.next() } else { 0.0 });
        rv.process(&send, &c, 1.0, 44_100, &mut out);
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
        assert_ne!(s.id, ParamId(0), "TYPE's id is retired (ADR 0009)");
    }
    assert_eq!(p.mix, 0.0);
    let size = REVERB_SPECS
        .iter()
        .find(|s| s.id == ReverbParams::SIZE)
        .unwrap();
    assert_eq!(size.step, 1.0 / 31.0);
    assert_eq!(p.controls(), RingControls::default());
}

#[test]
fn grit_is_param_5_and_reaches_the_ring() {
    let mut p = ReverbParams::default();
    assert_eq!(ReverbParams::GRIT, ParamId(5));
    assert_eq!(p.get(ReverbParams::GRIT), 0.3);
    p.write(ReverbParams::GRIT, 0.8);
    assert_eq!(p.grit, 0.8);
    assert_eq!(p.controls().grit, 0.8);
    let ids: Vec<ParamId> = REVERB_SPECS.iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        [
            ReverbParams::GRIT,
            ReverbParams::TIME,
            ReverbParams::DAMPING,
            ReverbParams::SIZE,
            ReverbParams::MIX
        ]
    );
    let grit = &REVERB_SPECS[0];
    assert_eq!(
        (grit.label, grit.min, grit.max, grit.step),
        ("GRIT", 0.0, 1.0, 1.0 / 128.0)
    );
}

#[test]
fn a_voice_through_the_reverb_leaves_a_tail() {
    use chimera_core::dsp::algo::params::AlgoParams;
    use chimera_core::dsp::algo::waves::WaveId;
    use chimera_core::dsp::voice::Voice;
    use chimera_core::modulation::ModState;
    use chimera_core::params::{EngineType, ParamSnapshot};
    use chimera_core::{MidiNote, Velocity};
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::TRI);
    let m = ModState::new();
    let mut voice = Voice::new(SR);
    let mut rv = Box::new(RingReverb::new());
    let c = RingControls {
        time: 0.7,
        ..RingControls::default()
    };
    let (mut block, mut out) = ([0.0f32; BLOCK_SIZE], Stereo::SILENT);
    voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
    for _ in 0..8 {
        voice.render(&mut block, &p, &m);
        rv.process(&block, &c, 0.5, SR, &mut out);
    }
    voice.note_off();
    let mut tail = 0.0;
    for _ in 0..64 {
        voice.render(&mut block, &p, &m);
        rv.process(&block, &c, 0.5, SR, &mut out);
        tail += energy(&out.l) + energy(&out.r);
    }
    assert!(tail > 0.01, "{tail}");
}
