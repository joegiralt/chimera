//! GRAIN, the delay's Beads-style TYPE (ADR 0199): the engine against an
//! f64 reference of the port, its clock at several densities, the pitch
//! shift's accuracy, feedback that never runs away, TYPE and TIME changes
//! that never click, NaN, and its codes.

use chimera_core::block::Block;
use chimera_core::dsp::delay::grain::{
    self, Clock, Ctl, Engine, GRAINS, MAX_D, MIN_D, bell, log_one_minus_x, soft_clip, uniform,
};
use chimera_core::dsp::delay::{DelayParams, DelayType, LINE_SCALE, MAX_DELAY_SAMPLES, TapeDelay};
use chimera_hal::BLOCK_SIZE;

const N: usize = MAX_DELAY_SAMPLES;
const FS: u32 = 48_000;

fn grain_params() -> DelayParams {
    DelayParams {
        time_ms: 250.0,
        feedback: 0.0,
        mix: 1.0,
        tone: 1.0,
        ..DelayParams::DEFAULT
    }
    .with(DelayParams::TYPE, DelayType::Grain.value())
}

/// A line of band-limited noise at about −10 dBFS of its ±2 scale.
fn noisy_line() -> Box<[i16; N]> {
    let mut rng = 0x2468_ace1u32;
    let mut lp = 0.0f32;
    let mut line = Box::new([0i16; N]);
    for s in line.iter_mut() {
        lp += 0.3 * ((uniform(&mut rng) - 0.5) * 2.0 - lp);
        *s = (lp * 0.6 * LINE_SCALE) as i16;
    }
    line
}

// ---------------------------------------------------------------------------
// The f64 reference: the port's maths without its fixed point or its f32.
// The clock is the engine's own (its starts have their own test below), so
// both play the same grains.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct RGrain {
    pos: f64,
    speed: f64,
    phase: f64,
    inc: f64,
    steal: f64,
    dec: f64,
    amp: f64,
    remaining: i64,
}

fn hermite(line: &[i16; N], pos: f64) -> f64 {
    let i = pos.floor() as usize;
    let t = pos - pos.floor();
    let x: [f64; 4] = core::array::from_fn(|k| f64::from(line[(i + N + k - 1) % N]));
    let c = (x[2] - x[0]) * 0.5;
    let v = x[1] - x[2];
    let w = c + v;
    let a = w + v + (x[3] - x[1]) * 0.5;
    let b = w + a;
    ((a * t - b) * t + c) * t + x[1]
}

fn rbell(p: f64) -> f64 {
    let tri = (1.0 - (2.0 * p - 1.0).abs()).max(0.0);
    tri * tri * (3.0 - 2.0 * tri)
}

impl RGrain {
    fn start(&mut self, head: usize, delay: f64, speed: f64, dur: f64, amp: f64, frac: f64) {
        let (min_d, max_d) = (f64::from(MIN_D), f64::from(MAX_D));
        let c = speed - 1.0;
        let delay = delay - frac * c;
        let dur = if c != 0.0 {
            dur.min((max_d - min_d) / c.abs())
        } else {
            dur
        }
        .max(2.0);
        let delay = delay.clamp(min_d + c.max(0.0) * dur, max_d + c.min(0.0) * dur);
        self.pos = (head as f64 - delay).rem_euclid(N as f64);
        self.speed = speed;
        self.inc = 1.0 / dur;
        self.phase = (self.inc * frac).min(1.0);
        (self.steal, self.dec, self.amp) = (1.0, 0.0, amp);
        self.remaining = dur as i64;
    }

    fn fade_out(&mut self, samples: f64) {
        let samples = samples.max(1.0);
        if self.remaining as f64 > samples {
            self.dec = 1.0 / samples;
            self.remaining = (self.steal * samples) as i64;
        }
    }

    fn render(&mut self, line: &[i16; N], out: &mut [f64], from: usize, to: usize) {
        for o in &mut out[from..to] {
            if self.remaining <= 0 {
                return;
            }
            self.steal -= self.dec;
            *o += hermite(line, self.pos) * rbell(self.phase) * self.steal.max(0.0) * self.amp
                / f64::from(LINE_SCALE);
            self.phase = (self.phase + self.inc).min(1.0);
            self.pos = (self.pos + self.speed).rem_euclid(N as f64);
            self.remaining -= 1;
        }
    }
}

/// The engine, in f64: `Engine::block`'s scheduling and stealing, the
/// starts' sprays from the same draws.
struct Reference {
    grains: [RGrain; GRAINS],
    clock: Clock,
    rng: u32,
    since: i64,
}

impl Reference {
    fn new() -> Self {
        Self {
            grains: [RGrain::default(); GRAINS],
            clock: Clock::NEW,
            rng: 0x9E37_79B9,
            since: 0,
        }
    }

    fn nearest(&self, skip: Option<usize>) -> usize {
        let mut k = usize::from(skip == Some(0));
        for (i, g) in self.grains.iter().enumerate() {
            if Some(i) != skip && g.remaining < self.grains[k].remaining {
                k = i;
            }
        }
        k
    }

    fn block(&mut self, line: &[i16; N], wp: usize, ctl: &Ctl, out: &mut [f64; BLOCK_SIZE]) {
        *out = [0.0; BLOCK_SIZE];
        let mut starts = [(0usize, 0.0f32); 4];
        let n = self
            .clock
            .block(ctl.rate, ctl.texture, &mut self.rng, &mut starts);
        let depth =
            (f64::from(ctl.rate) * f64::from(ctl.dur) * 0.5).clamp(1.0, GRAINS as f64 / 2.0);
        // The port's heads step in Q16.16.
        let speed = (f64::from(ctl.speed) * 65_536.0).floor() / 65_536.0;
        let amp = depth.powf(-(1.0 - 0.5 * f64::from(ctl.texture)));
        let mut from = 0;
        for &(at, frac) in &starts[..n] {
            self.grains
                .iter_mut()
                .for_each(|g| g.render(line, out, from, at));
            from = at;
            let u = f64::from(uniform(&mut self.rng));
            let tex = f64::from(ctl.texture);
            let delay =
                f64::from(ctl.delay) * (1.0 + tex * f64::from(grain::SPRAY) * (2.0 * u - 1.0));
            let k = self.nearest(None);
            self.grains[k].start(
                (wp + at) % N,
                delay,
                speed,
                f64::from(ctl.dur),
                amp,
                f64::from(frac),
            );
            let interval = (self.since + at as i64) as f64;
            let first = self.nearest(None);
            let second = self.nearest(Some(first));
            let (instant, preempt) = (f64::from(ctl.instant), f64::from(ctl.preempt));
            self.grains[first].fade_out(instant);
            self.grains[second].fade_out(interval.max(instant).min(preempt));
            self.since = -(at as i64);
        }
        self.grains
            .iter_mut()
            .for_each(|g| g.render(line, out, from, BLOCK_SIZE));
        self.since += BLOCK_SIZE as i64;
    }
}

/// The engine and the reference over a still line, `blocks` blocks with
/// the write head moving on as if each were written (the line is not, so
/// both read the same samples): the worst error and the output's RMS.
fn against_reference(p: &DelayParams, blocks: usize) -> (f64, f64) {
    let line = noisy_line();
    let ctl = Ctl::new(p, p.time_ms * FS as f32 / 1000.0, FS);
    let (mut e, mut r) = (Engine::NEW, Reference::new());
    let (mut worst, mut sum, mut n) = (0.0f64, 0.0f64, 0usize);
    for b in 0..blocks {
        let wp = (b * BLOCK_SIZE) % N;
        let (mut a, mut x) = ([0.0f32; BLOCK_SIZE], [0.0f64; BLOCK_SIZE]);
        e.block(&line, wp, &ctl, &mut a);
        r.block(&line, wp, &ctl, &mut x);
        for (&a, &x) in a.iter().zip(&x) {
            worst = worst.max((f64::from(a) - x).abs());
            sum += x * x;
            n += 1;
        }
    }
    (worst, (sum / n as f64).sqrt())
}

/// The port against the f64 reference over pitches, sizes, densities and
/// textures: within 5e-4 of signals near 0.1 RMS. An f32 delay of 24,000
/// samples holds 2⁻⁹ of a sample, so a start lands up to 0.002 samples off
/// the reference's.
#[test]
fn the_engine_matches_its_f64_reference() {
    for (size, density, pitch, texture) in [
        (0.6, 0.6, 0.0, 0.0),
        (0.2, 0.9, 12.0, 0.0),
        (0.9, 0.3, -12.0, 0.5),
        (0.5, 1.0, 7.0, 1.0),
        (1.0, 1.0, 24.0, 0.3),
        (0.0, 0.0, -24.0, 1.0),
    ] {
        let p = grain_params()
            .with(DelayParams::SIZE, size)
            .with(DelayParams::DENSITY, density)
            .with(DelayParams::PITCH, pitch)
            .with(DelayParams::TEXTURE, texture);
        let (worst, rms) = against_reference(&p, 1500);
        assert!(rms > 0.002, "{p:?}: silent ({rms})");
        assert!(worst < 5e-4, "{p:?}: {worst} off, RMS {rms}");
    }
}

/// The pure pieces as Beads has them: the bell, the soft clip, the log's
/// series.
#[test]
fn the_ported_pieces_match_beads() {
    assert_eq!(bell(0.0), 0.0);
    assert_eq!(bell(1.0), 0.0);
    assert_eq!(bell(0.5), 1.0);
    assert!((bell(0.25) - 0.5).abs() < 1e-6, "smoothstep(½)");
    for x in [-5.0f32, -3.0, -1.0, 0.0, 0.5, 2.0, 3.0, 9.0] {
        let want = if x.abs() > 3.0 {
            x.signum()
        } else {
            x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
        };
        assert_eq!(soft_clip(x), want, "{x}");
    }
    assert_eq!(soft_clip(3.0), 1.0, "continuous at the knee");
    for x in [0.1f32, 0.5, 0.9] {
        let series = -x * (1.0 + x * (0.5 + x * (0.3333 + x * 0.25)));
        assert_eq!(log_one_minus_x(x), series);
    }
}

// ---------------------------------------------------------------------------
// The clock
// ---------------------------------------------------------------------------

fn starts_over(density: f32, texture: f32, secs: f32) -> (usize, Vec<usize>) {
    let p = grain_params()
        .with(DelayParams::DENSITY, density)
        .with(DelayParams::TEXTURE, texture);
    let ctl = Ctl::new(&p, 4800.0, FS);
    let (mut c, mut rng) = (Clock::NEW, 0x9E37_79B9u32);
    let blocks = (secs * FS as f32) as usize / BLOCK_SIZE;
    let (mut n, mut at) = (0, Vec::new());
    for b in 0..blocks {
        let mut out = [(0usize, 0.0f32); 4];
        let k = c.block(ctl.rate, texture, &mut rng, &mut out);
        for &(i, frac) in &out[..k] {
            assert!(i < BLOCK_SIZE && (0.0..1.0).contains(&frac));
            at.push(b * BLOCK_SIZE + i);
        }
        n += k;
    }
    (n, at)
}

/// At TEXTURE 0 the train is regular, `RATE_MIN · 2^(7·DENSITY)` a second:
/// 1, 11.3 and 128 Hz at 0, ½ and 1, each interval within a sample.
#[test]
fn grains_start_at_density_and_regularly() {
    for (density, hz) in [(0.0f32, 1.0f32), (0.5, 11.313_708), (1.0, 128.0)] {
        let (n, at) = starts_over(density, 0.0, 20.0);
        let want = 20.0 * hz;
        assert!(
            (n as f32 - want).abs() <= 1.0,
            "DENSITY {density}: {n} vs {want}"
        );
        let period = FS as f32 / hz;
        for w in at.windows(2) {
            let d = (w[1] - w[0]) as f32;
            assert!(
                (d - period).abs() <= 1.0,
                "DENSITY {density}: interval {d} vs {period}"
            );
        }
    }
}

/// TEXTURE makes the intervals Beads' random ones: no longer regular, the
/// rate within a factor of three of the regular one, never more than two
/// starts a block.
#[test]
fn texture_makes_the_intervals_irregular() {
    for density in [0.0f32, 0.5, 1.0] {
        let (regular, _) = starts_over(density, 0.0, 30.0);
        let (n, at) = starts_over(density, 1.0, 30.0);
        let ratio = n as f32 / regular as f32;
        assert!(
            (1.0 / 3.0..3.0).contains(&ratio),
            "DENSITY {density}: {n} vs {regular}"
        );
        let d: Vec<_> = at.windows(2).map(|w| w[1] - w[0]).collect();
        let (lo, hi) = (d.iter().min().unwrap(), d.iter().max().unwrap());
        assert!(
            *hi > 2 * *lo,
            "DENSITY {density}: intervals {lo}..{hi} stayed regular"
        );
        let per_block = at.chunk_by(|a, b| a / BLOCK_SIZE == b / BLOCK_SIZE);
        assert!(per_block.map(<[usize]>::len).max().unwrap() <= 2);
    }
}

/// The grains a block plays never exceed the pool, and at a dense, long
/// setting the pool is in use but a start always finds a grain.
#[test]
fn the_pool_holds_the_cloud() {
    let line = noisy_line();
    let p = DelayParams { ..grain_params() }
        .with(DelayParams::SIZE, 1.0)
        .with(DelayParams::DENSITY, 1.0);
    let ctl = Ctl::new(&p, 12_000.0, FS);
    let mut e = Engine::NEW;
    let mut most = 0;
    for b in 0..2000 {
        let mut out = [0.0; BLOCK_SIZE];
        e.block(&line, (b * BLOCK_SIZE) % N, &ctl, &mut out);
        most = most.max(e.active());
    }
    assert!(most >= GRAINS / 2, "{most} grains at once");
    assert!(most <= GRAINS);
}

// ---------------------------------------------------------------------------
// Through the delay
// ---------------------------------------------------------------------------

/// `blocks` of `p` over `input`'s blocks, the send's return.
fn play(
    d: &mut TapeDelay,
    p: &DelayParams,
    blocks: usize,
    input: impl Fn(usize) -> [f32; BLOCK_SIZE],
) -> Vec<f32> {
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    for b in 0..blocks {
        let mut buf = input(b);
        d.process_wet(&mut buf, p, FS);
        out.extend_from_slice(&buf);
    }
    out
}

fn sine(hz: f32, amp: f32) -> impl Fn(usize) -> [f32; BLOCK_SIZE] {
    move |b| {
        core::array::from_fn(|i| {
            let n = (b * BLOCK_SIZE + i) as f32;
            amp * (core::f32::consts::TAU * hz * n / FS as f32).sin()
        })
    }
}

/// The mean frequency of `x` from its rising zero crossings, each placed
/// between its two samples.
fn crossing_hz(x: &[f32]) -> f64 {
    let mut at = Vec::new();
    for (i, w) in x.windows(2).enumerate() {
        if w[0] < 0.0 && w[1] >= 0.0 {
            at.push(i as f64 + f64::from(-w[0] / (w[1] - w[0])));
        }
    }
    let span = at.last().unwrap() - at[0];
    f64::from(FS) * (at.len() - 1) as f64 / span
}

/// PITCH shifts each grain's read by its semitones: in one grain alone
/// (DENSITY 0, a grain a second; SIZE 0.9) a 440 Hz sine reads 440 ·
/// 2^(PITCH/12) within a cent, at 0, +7, +12, −12, +24 and −24. Grains
/// started on a regular train add up to lines a grain rate apart, as any
/// grain shifter's do: TEXTURE spreads them (ADR 0199).
#[test]
fn pitch_shifts_each_grain_within_a_cent() {
    for semis in [0.0f32, 7.0, 12.0, -12.0, 24.0, -24.0] {
        let p = DelayParams { ..grain_params() }
            .with(DelayParams::PITCH, semis)
            .with(DelayParams::TEXTURE, 0.0)
            .with(DelayParams::SIZE, 0.9)
            .with(DelayParams::DENSITY, 0.0);
        let mut d = Box::new(TapeDelay::new());
        let out = play(&mut d, &p, 2000, sine(440.0, 0.5));
        // The second grain, at 2 s: the first may start before the line
        // is full.
        let loud: Vec<usize> = (0..out.len()).filter(|&i| out[i].abs() > 0.02).collect();
        let second = loud.iter().position(|&i| i >= 95_000).unwrap();
        let start = loud[second];
        let end = loud[second..]
            .windows(2)
            .find(|w| w[1] - w[0] > 2000)
            .map_or(*loud.last().unwrap(), |w| w[0]);
        let len = end - start;
        assert!(len > 4000, "PITCH {semis}: a grain of {len}");
        let mid = &out[start + len / 4..end - len / 4];
        let want = 440.0 * 2f64.powf(f64::from(semis) / 12.0);
        let cents = 1200.0 * (crossing_hz(mid) / want).log2();
        assert!(cents.abs() < 1.0, "PITCH {semis}: {cents} cents");
    }
}

/// Shimmer at its worst: FDBK 1, PITCH +12 (and +24), TONE open, every
/// TEXTURE. A burst, then 12 s of silence: always finite, never past the
/// line's ±2 scale by more than the grains' sum allows, and no louder at
/// the end than after the burst.
#[test]
fn full_feedback_with_pitch_up_never_runs_away() {
    for (pitch, texture) in [(12.0f32, 0.0f32), (12.0, 1.0), (24.0, 0.5), (7.0, 0.0)] {
        let p = DelayParams {
            feedback: 1.0,
            tone: 1.0,
            ..grain_params()
        }
        .with(DelayParams::PITCH, pitch)
        .with(DelayParams::TEXTURE, texture)
        .with(DelayParams::DENSITY, 0.8)
        .with(DelayParams::SIZE, 0.7);
        let mut d = Box::new(TapeDelay::new());
        let burst = |b: usize| -> [f32; BLOCK_SIZE] {
            let mut rng = (b as u32).wrapping_mul(2_654_435_761) | 1;
            core::array::from_fn(|_| {
                if b < 375 {
                    2.0 * uniform(&mut rng) - 1.0
                } else {
                    0.0
                }
            })
        };
        let out = play(&mut d, &p, 375 + 9000, burst);
        assert!(out.iter().all(|v| v.is_finite()));
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let all = peak(&out);
        assert!(all < 4.0, "PITCH {pitch} TEXTURE {texture}: peak {all}");
        let early = peak(&out[24_000..72_000]);
        let late = peak(&out[out.len() - 48_000..]);
        assert!(
            late <= early.max(0.5) * 1.5,
            "PITCH {pitch}: grew {early} to {late}"
        );
    }
}

/// Below full feedback the shimmer dies away: FDBK 0.8, PITCH +12, the
/// last second 20 dB under the first after the burst.
#[test]
fn shimmer_decays_below_full_feedback() {
    let p = DelayParams {
        feedback: 0.8,
        tone: 0.8,
        ..grain_params()
    }
    .with(DelayParams::PITCH, 12.0);
    let mut d = Box::new(TapeDelay::new());
    let out = play(&mut d, &p, 375 * 10, |b| {
        if b < 75 {
            sine(330.0, 0.8)(b)
        } else {
            [0.0; BLOCK_SIZE]
        }
    });
    let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
    let (first, last) = (rms(&out[4_800..52_800]), rms(&out[out.len() - 48_000..]));
    assert!(first > 0.01);
    assert!(last < first * 0.1, "{first} to {last}");
}

/// The largest step between neighbouring samples of `x`.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0, |m, w| m.max((w[1] - w[0]).abs()))
}

/// TYPE TAPE → GRAIN → TAPE under a held sine: each switch crossfades the
/// returns in 20 ms, so no step is larger than the steady return's own
/// largest; where the switch to GRAIN would snap, it would be.
#[test]
fn type_switches_never_click() {
    let tape = DelayParams {
        wow_flutter: 0.0,
        ..grain_params()
    }
    .with(DelayParams::TYPE, DelayType::Tape.value())
    .with(DelayParams::SIZE, 0.9)
    .with(DelayParams::DENSITY, 0.8)
    .with(DelayParams::TEXTURE, 0.0);
    let grain = DelayParams { ..tape }.with(DelayParams::TYPE, DelayType::Grain.value());
    let mut d = Box::new(TapeDelay::new());
    let src = sine(220.0, 0.5);
    let mut out = Vec::new();
    for b in 0..1200 {
        let p = if (400..800).contains(&b) {
            &grain
        } else {
            &tape
        };
        let mut buf = src(b);
        d.process_wet(&mut buf, p, FS);
        out.extend_from_slice(&buf);
    }
    let steady = max_step(&out[300 * BLOCK_SIZE..399 * BLOCK_SIZE]);
    for at in [400, 800] {
        let around = max_step(&out[(at - 2) * BLOCK_SIZE..(at + 20) * BLOCK_SIZE]);
        assert!(
            around <= steady * 1.25,
            "block {at}: step {around} against {steady}"
        );
    }
    let grain_steady = max_step(&out[600 * BLOCK_SIZE..799 * BLOCK_SIZE]);
    assert!(
        grain_steady <= steady * 1.25,
        "GRAIN's own steps {grain_steady}"
    );
}

/// TIME jumps under GRAIN: the grains in flight finish where they were,
/// the new ones start at the new TIME, so no step is larger than the
/// steady return's.
#[test]
fn time_changes_never_click_under_grain() {
    let a = DelayParams { ..grain_params() }
        .with(DelayParams::SIZE, 0.8)
        .with(DelayParams::DENSITY, 0.8)
        .with(DelayParams::TEXTURE, 0.0);
    let b = DelayParams { time_ms: 61.0, ..a };
    let mut d = Box::new(TapeDelay::new());
    let src = sine(220.0, 0.5);
    let mut out = Vec::new();
    for k in 0..900 {
        let p = if (400..600).contains(&k) { &b } else { &a };
        let mut buf = src(k);
        d.process_wet(&mut buf, p, FS);
        out.extend_from_slice(&buf);
    }
    let steady = max_step(&out[300 * BLOCK_SIZE..399 * BLOCK_SIZE]);
    let around = max_step(&out[399 * BLOCK_SIZE..900 * BLOCK_SIZE]);
    assert!(around <= steady * 1.25, "step {around} against {steady}");
}

/// NaN in GRAIN's knobs reads as each one's floor (PITCH as 0) and NaN in
/// the send writes silence into the line: the return stays finite.
#[test]
fn nan_never_reaches_the_return() {
    let p = DelayParams {
        feedback: 0.7,
        ..grain_params()
    }
    .with(DelayParams::SIZE, f32::NAN)
    .with(DelayParams::DENSITY, f32::NAN)
    .with(DelayParams::PITCH, f32::NAN)
    .with(DelayParams::TEXTURE, f32::NAN);
    let ctl = Ctl::new(&p, 4800.0, FS);
    for v in [ctl.dur, ctl.rate, ctl.speed, ctl.texture, ctl.amp()] {
        assert!(v.is_finite(), "{ctl:?}");
    }
    assert_eq!(ctl.speed, 1.0);
    let mut d = Box::new(TapeDelay::new());
    let out = play(&mut d, &p, 2000, |b| {
        let mut x = sine(300.0, 0.5)(b);
        if b % 7 == 3 {
            x[5] = f32::NAN;
            x[9] = f32::INFINITY;
        }
        x
    });
    assert!(out.iter().all(|v| v.is_finite()));
    assert!(out.iter().any(|v| *v != 0.0), "NaN silenced it");
    let kind = DelayParams { ..p }.with(DelayParams::TYPE, f32::NAN);
    assert_eq!(kind.kind(), DelayType::Tape);
}

/// TYPE is delay param 7 (0–6 are in use, ADR 0009), TAPE by default and
/// as a project saved before it loads; stored as its index (TAPE 0, GRAIN
/// 1), a real the codec clamps and rounds, so an unknown one lands on a
/// TYPE.
#[test]
fn type_is_param_7_and_tape_by_default() {
    let mut p = DelayParams::default();
    assert_eq!(p.kind(), DelayType::Tape);
    assert_eq!(DelayParams::TYPE.0, 7);
    assert_eq!(p.get(DelayParams::TYPE), 0.0);
    p.set(DelayParams::TYPE, 1.0);
    assert_eq!(p.kind(), DelayType::Grain);
    p.set(DelayParams::TYPE, 7.0);
    assert_eq!(p.get(DelayParams::TYPE), 1.0, "held to GRAIN");
    p.set(DelayParams::TYPE, -3.0);
    assert_eq!(p.kind(), DelayType::Tape);
    for t in [DelayType::Tape, DelayType::Grain] {
        assert_eq!(DelayType::at(t.value()), t);
    }
}

/// GRAIN's return, a regression pin of the port: TIME 250 ms, FDBK 0.6,
/// PITCH +12, TEXTURE 0.4 over plucks.
#[test]
fn grain_render_is_pinned() {
    let p = DelayParams {
        feedback: 0.6,
        ..grain_params()
    }
    .with(DelayParams::PITCH, 12.0)
    .with(DelayParams::TEXTURE, 0.4);
    let mut d = Box::new(TapeDelay::new());
    let out = play(&mut d, &p, 1200, |b| {
        core::array::from_fn(|i| {
            let t = ((b * BLOCK_SIZE + i) % 14_400) as f32;
            0.8 * (t * 0.031).sin() * (-t / 2400.0).exp()
        })
    });
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in &out {
        for byte in v.to_bits().to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    assert_eq!(h, 0x3e78_9b3a_0bbe_62b4, "re-pin only after listening");
}
