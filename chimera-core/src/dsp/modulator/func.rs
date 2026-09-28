//! Envelope B (spec § Envelope B): the Cascadia's function generator in
//! ENV, LFO and BURST modes; a FUNC LFO runs it in LFO mode.

use core::f32::consts::{FRAC_PI_2, TAU};

use crate::dsp::modulator::law::{
    B_RATE, B_TIME, BURST_LEN, BURST_RATE, block_rate_max, curve, curve_inv, rc_coeff, rc_k,
    shape_w, tilt,
};
use crate::dsp::modulator::{EnvForm, Func, FuncParams, LfoForm};
use crate::dsp::{fast_exp2, fast_sin};

/// The matrix's offsets into RISE, FALL and SHAPE, added to their positions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slides {
    pub rise: f32,
    pub fall: f32,
    pub shape: f32,
}

/// B's constants for one block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BCoefs {
    func: Func,
    /// Per sample: the rise's `x` step (ENV), φ's (LFO), the pulse phase's (BURST).
    rise: f32,
    /// Per sample: the fall's `x` step (ENV); the burst's, a fraction of LENGTH (BURST).
    fall: f32,
    /// A turn's overshoot, converted between the two segments' units.
    fall_per_rise: f32,
    rise_per_fall: f32,
    /// SHAPE's curve weight (ENV).
    w: f32,
    /// SHAPE's position: TILT (LFO, BURST), SLEW (LFV).
    shape: f32,
    /// FALL's position: PHASE (FREE, SYNC), DELTA (LFV).
    fall_pos: f32,
    /// LFV's slew: its per-sample step and `log2` retention (none: 1, −∞).
    slew_c: f32,
    slew_k: f32,
    /// φ's step per sample in a `u32` turn (LFO; BURST's pulse phase): an
    /// integer accumulator wraps exactly, where `phase += step` in f32
    /// drifts past 1e-4 within a second.
    inc: u32,
}

impl BCoefs {
    /// `per_sample`: the slot feeds the VCA, so its rates keep the manual's
    /// full range. Per block, every rate stops at the block rate ÷ 8: LFO
    /// RATE, BURST's pulse RATE, and the repeats of ENV CYCLE
    /// (`1/(RISE + FALL)`, both segments slowed alike) and BURST CYCLE
    /// (`1/LENGTH`). One-shot AD and AHR times are not clamped.
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    pub fn new(p: &FuncParams, s: &Slides, sample_rate: u32, per_sample: bool) -> Self {
        let fs = sample_rate as f32;
        let pos = |v: f32, off: f32| (v + off).max(0.0).min(1.0);
        let (rise_pos, fall_pos, shape) = (
            pos(p.rise, s.rise),
            pos(p.fall, s.fall),
            pos(p.shape, s.shape),
        );
        let max = if per_sample {
            f32::INFINITY
        } else {
            block_rate_max(sample_rate)
        };
        let func = p.func();
        let (rise, fall) = match func {
            Func::Env(form) => {
                let (tr, tf) = (B_TIME.at(rise_pos), B_TIME.at(fall_pos));
                let slow = if form == EnvForm::Cycle {
                    ((tr + tf) * max).recip().max(1.0)
                } else {
                    1.0
                };
                (1.0 / (tr * slow * fs), 1.0 / (tf * slow * fs))
            }
            Func::Lfo(_) => (B_RATE.at(rise_pos).min(max) / fs, 0.0),
            Func::Burst(form) => {
                let len = BURST_LEN.at(fall_pos);
                let len = if form == EnvForm::Cycle {
                    len.max(max.recip())
                } else {
                    len
                };
                (BURST_RATE.at(rise_pos).min(max) / fs, 1.0 / (len * fs))
            }
        };
        let (slew_c, slew_k) = if func == Func::Lfo(LfoForm::Lfv) && shape > 0.0 {
            // τ = SLEW × one cycle.
            let k = rc_k(shape / (rise * fs), fs);
            (rc_coeff(k), k)
        } else {
            (1.0, f32::NEG_INFINITY)
        };
        Self {
            func,
            rise,
            fall,
            fall_per_rise: if rise > 0.0 { fall / rise } else { 0.0 },
            rise_per_fall: if fall > 0.0 { rise / fall } else { 0.0 },
            w: shape_w(shape),
            shape,
            fall_pos,
            slew_c,
            slew_k,
            inc: match func {
                Func::Env(_) => 0,
                Func::Lfo(_) | Func::Burst(_) => (rise * TURN + 0.5) as u32,
            },
        }
    }
}

/// One turn of φ as a `u32`.
const TURN: f32 = 4_294_967_296.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seg {
    Idle,
    Rise,
    Hold,
    Fall,
}

/// Envelope B's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuncGen {
    func: Func,
    seg: Seg,
    /// ENV: the running segment's linear position, 0..1 (a fall runs 1 → 0).
    /// BURST: time into the burst, a fraction of LENGTH.
    x: f32,
    /// `x`'s anchor: `x = x0 ± k·step`, computed, not accumulated, so a
    /// constant step's f32 rounding can't build up (ADR 0036). Re-anchored
    /// at every turn, wrap and block start (`go`, `set`).
    x0: f32,
    k: u32,
    /// LFO φ; BURST's pulse phase: a `u32` turn.
    phase: u32,
    /// LFV: the cycle's start and end targets, and the slewed output.
    from: f32,
    to: f32,
    slewed: f32,
    rng: u32,
    /// BURST CYCLE: the key went up; the running burst is the last.
    last_burst: bool,
    /// The last block's SHAPE weight, TILT and PHASE, for reads between blocks.
    w: f32,
    tilt: f32,
    phase_off: f32,
}

impl Default for FuncGen {
    fn default() -> Self {
        Self::new()
    }
}

impl FuncGen {
    pub const fn new() -> Self {
        Self {
            func: Func::Env(EnvForm::Ad),
            seg: Seg::Idle,
            x: 0.0,
            x0: 0.0,
            k: 0,
            phase: 0,
            from: 0.0,
            to: 0.0,
            slewed: 0.0,
            rng: 0x2545_f491,
            last_burst: false,
            w: 1.0,
            tilt: 0.5,
            phase_off: 0.0,
        }
    }

    /// Take a block's MODE, FORM and shape; re-anchor, since the step may
    /// change.
    pub fn set(&mut self, c: &BCoefs) {
        self.func = c.func;
        (self.w, self.tilt, self.phase_off) = (c.w, c.shape, c.fall_pos);
        (self.x0, self.k) = (self.x, 0);
    }

    /// Into segment `seg` at `x`, anchored there.
    fn go(&mut self, seg: Seg, x: f32) {
        (self.seg, self.x, self.x0, self.k) = (seg, x, x, 0);
    }

    /// φ as 0..1.
    fn phase_f(&self) -> f32 {
        self.phase as f32 * (1.0 / TURN)
    }

    /// A note-on: AD and AHR rise from the current level, CYCLE restarts its
    /// rise, SYNC resets φ, BURST starts a burst; FREE, LFV and the pulse
    /// phase run on (spec, Defaults chosen 18).
    pub fn note_on(&mut self, f: Func) {
        self.func = f;
        match f {
            Func::Env(EnvForm::Cycle) => self.go(Seg::Rise, 0.0),
            Func::Env(_) => {
                let l = self.env_level();
                self.go(Seg::Rise, curve_inv(l, self.w));
            }
            Func::Lfo(LfoForm::Sync) => self.phase = 0,
            Func::Lfo(_) => {}
            Func::Burst(_) => {
                self.go(Seg::Rise, 0.0);
                self.last_burst = false;
            }
        }
    }

    /// The key is up this block: AHR falls, a held burst goes on, a
    /// cycling burst finishes.
    pub fn key_up(&mut self) {
        match (self.func, self.seg) {
            (Func::Env(EnvForm::Ahr), Seg::Rise) => {
                let l = self.env_level();
                self.fall_from(l);
            }
            (Func::Env(EnvForm::Ahr), Seg::Hold) => self.go(Seg::Fall, 1.0),
            (Func::Burst(EnvForm::Ahr), Seg::Hold) => self.seg = Seg::Rise,
            (Func::Burst(EnvForm::Cycle), Seg::Rise) => self.last_burst = true,
            _ => {}
        }
    }

    fn fall_from(&mut self, l: f32) {
        self.go(Seg::Fall, 1.0 - curve_inv(1.0 - l, self.w));
    }

    /// ENV mode's level: `f(x)` rising, `1 − f(1 − x)` falling.
    fn env_level(&self) -> f32 {
        match self.seg {
            Seg::Idle => 0.0,
            Seg::Rise => curve(self.x, self.w),
            Seg::Hold => 1.0,
            Seg::Fall => 1.0 - curve(1.0 - self.x, self.w),
        }
    }

    fn lfo_level(&self, form: LfoForm) -> f32 {
        if form == LfoForm::Lfv {
            return self.slewed;
        }
        let p = self.phase_f() + self.phase_off;
        2.0 * tilt(p - (p as u32) as f32, self.tilt) - 1.0
    }

    fn burst_level(&self, form: EnvForm) -> f32 {
        if self.seg == Seg::Idle {
            return 0.0;
        }
        let env = tilt(self.x.min(1.0), self.tilt);
        let ph = self.phase_f();
        let pulse = if form == EnvForm::Cycle {
            tilt(ph, self.tilt)
        } else {
            let m = 1.0 - (2.0 * self.tilt - 1.0).abs();
            let square = if ph < 0.5 { 1.0 } else { 0.0 };
            let sine = 0.5 - 0.5 * fast_sin(TAU * ph + FRAC_PI_2);
            (1.0 - m) * square + m * sine
        };
        env * pulse
    }

    /// The output now: 0..1 (ENV, BURST) or −1..1 (LFO).
    pub fn output(&self) -> f32 {
        match self.func {
            Func::Env(_) => self.env_level(),
            Func::Lfo(form) => self.lfo_level(form),
            Func::Burst(form) => self.burst_level(form),
        }
    }

    /// One sample; returns the output after it.
    pub fn tick(&mut self, c: &BCoefs, key: bool) -> f32 {
        self.step(c, key, 1, true);
        self.output()
    }

    /// A block of per-sample outputs, as `tick` would give them, for the
    /// VCA: the MODE matched once, one output evaluation (one curve
    /// divide) per sample.
    pub fn fill(&mut self, c: &BCoefs, key: bool, out: &mut [f32]) {
        match c.func {
            Func::Env(form) => {
                for o in out.iter_mut() {
                    self.env_step(c, form, key, 1);
                    *o = self.env_level();
                }
            }
            Func::Lfo(form) => {
                for o in out.iter_mut() {
                    self.lfo_step(c, form, 1, true);
                    *o = self.lfo_level(form);
                }
            }
            Func::Burst(form) => {
                for o in out.iter_mut() {
                    self.burst_step(c, form, key, 1);
                    *o = self.burst_level(form);
                }
            }
        }
    }

    /// `n` samples at once: linear in `x` and φ, so within ±1 sample of `n`
    /// ticks (ADR 0036).
    pub fn advance(&mut self, c: &BCoefs, key: bool, n: u32) {
        self.step(c, key, n, false);
    }

    fn step(&mut self, c: &BCoefs, key: bool, n: u32, tick: bool) {
        match c.func {
            Func::Env(form) => self.env_step(c, form, key, n),
            Func::Lfo(form) => self.lfo_step(c, form, n, tick),
            Func::Burst(form) => self.burst_step(c, form, key, n),
        }
    }

    fn env_step(&mut self, c: &BCoefs, form: EnvForm, key: bool, n: u32) {
        match self.seg {
            Seg::Rise => {
                self.k += n;
                self.x = self.x0 + self.k as f32 * c.rise;
            }
            Seg::Fall => {
                self.k += n;
                self.x = self.x0 - self.k as f32 * c.fall;
            }
            Seg::Idle | Seg::Hold => return,
        }
        // Turns carry the overshoot into the next segment, anchored there.
        for _ in 0..4 {
            match self.seg {
                Seg::Rise if self.x >= 1.0 => {
                    if form == EnvForm::Ahr && key {
                        self.go(Seg::Hold, 1.0);
                        return;
                    }
                    self.go(Seg::Fall, 1.0 - (self.x - 1.0) * c.fall_per_rise);
                }
                Seg::Fall if self.x <= 0.0 => {
                    if form != EnvForm::Cycle {
                        self.go(Seg::Idle, 0.0);
                        return;
                    }
                    self.go(Seg::Rise, -self.x * c.rise_per_fall);
                }
                _ => return,
            }
        }
    }

    fn lfo_step(&mut self, c: &BCoefs, form: LfoForm, n: u32, tick: bool) {
        // Exact in integers; per block the clamp allows at most one wrap.
        let t = self.phase as u64 + c.inc as u64 * n as u64;
        self.phase = t as u32;
        if form == LfoForm::Lfv {
            if t >> 32 != 0 {
                self.next_target(c.fall_pos);
            }
            let lin = self.from + (self.to - self.from) * self.phase_f();
            self.slewed = if tick {
                self.slewed + c.slew_c * (lin - self.slewed)
            } else {
                lin + (self.slewed - lin) * fast_exp2(c.slew_k * n as f32)
            };
        }
    }

    /// LFV's next target, `clamp(t + DELTA·r, −1, 1)` with `r` uniform in −1..1.
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    fn next_target(&mut self, delta: f32) {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        let r = x as f32 / u32::MAX as f32 * 2.0 - 1.0;
        self.from = self.to;
        self.to = (self.to + delta * r).max(-1.0).min(1.0);
    }

    fn burst_step(&mut self, c: &BCoefs, form: EnvForm, key: bool, n: u32) {
        self.phase = self.phase.wrapping_add(c.inc.wrapping_mul(n));
        if matches!(self.seg, Seg::Idle | Seg::Hold) {
            return;
        }
        let before = self.x;
        self.k += n;
        self.x = self.x0 + self.k as f32 * c.fall;
        if form == EnvForm::Ahr && key && before <= c.shape && self.x >= c.shape {
            self.go(Seg::Hold, c.shape);
        } else if self.x >= 1.0 {
            if form == EnvForm::Cycle && !self.last_burst {
                self.go(Seg::Rise, self.x - 1.0);
            } else {
                self.go(Seg::Idle, 0.0);
            }
        }
    }

    /// Holds a voice through the VCA (spec § 4): AD and AHR until idle;
    /// ENV CYCLE and LFO while the key is held; BURST CYCLE until its last
    /// burst ends.
    pub fn holds(&self, key: bool) -> bool {
        match self.func {
            Func::Lfo(_) | Func::Env(EnvForm::Cycle) => key,
            _ => self.seg != Seg::Idle,
        }
    }

    /// Done: ENV CYCLE and LFO never are.
    pub fn is_idle(&self) -> bool {
        !matches!(self.func, Func::Lfo(_) | Func::Env(EnvForm::Cycle)) && self.seg == Seg::Idle
    }

    /// On a rising segment, for a TYPE change into A or B ENV.
    pub fn rising(&self) -> bool {
        match self.func {
            Func::Env(_) => self.seg == Seg::Rise,
            Func::Lfo(_) => {
                let p = self.phase_f() + self.phase_off;
                p - ((p as u32) as f32) < self.tilt
            }
            Func::Burst(_) => self.seg != Seg::Idle && self.x < self.tilt,
        }
    }

    /// Into ENV mode at `level` (spec § 1): rising → rise from it, else fall
    /// from it. At 0 the fall is already over: CYCLE turns and rises (its own
    /// rule), AD and AHR are done. Call after `set`, which names the FORM.
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    pub fn enter_env(&mut self, level: f32, rising: bool) {
        let l = level.max(0.0).min(1.0);
        if rising {
            self.go(Seg::Rise, curve_inv(l, self.w));
        } else if l > 0.0 {
            self.fall_from(l);
        } else if self.func == Func::Env(EnvForm::Cycle) {
            self.go(Seg::Rise, 0.0);
        } else {
            self.go(Seg::Idle, 0.0);
        }
    }

    /// Into BURST: a burst starts now if the key is held.
    pub fn enter_burst(&mut self, key: bool) {
        self.go(if key { Seg::Rise } else { Seg::Idle }, 0.0);
        self.last_burst = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per block, the cycling forms repeat no faster than the block rate ÷ 8
    /// (spec § Rates, gap 8's ruling); per sample, and in one-shot forms,
    /// nothing is clamped.
    #[test]
    fn cycles_repeat_no_faster_than_the_clamp() {
        let sr = 48_000;
        let max = block_rate_max(sr) / sr as f32; // cycles per sample
        let fastest = |f| {
            let mut p = FuncParams {
                rise: 0.0,
                fall: 0.0,
                ..FuncParams::ENV
            };
            p.set_func(f);
            p
        };
        let coefs = |f, per_sample| BCoefs::new(&fastest(f), &Slides::default(), sr, per_sample);
        // ENV CYCLE at 2 ms + 2 ms (250 Hz): one period is 1/rise + 1/fall samples.
        let env = coefs(Func::Env(EnvForm::Cycle), false);
        assert!(1.0 / (1.0 / env.rise + 1.0 / env.fall) <= max * 1.0001);
        // BURST CYCLE at 10 ms (100 Hz) with 50 Hz pulses.
        let burst = coefs(Func::Burst(EnvForm::Cycle), false);
        assert!(burst.fall <= max * 1.0001);
        assert!(coefs(Func::Env(EnvForm::Cycle), true).rise > max);
        assert!(coefs(Func::Env(EnvForm::Ad), false).rise > max);
    }

    /// Idle and Hold don't count samples, so a long-idle slot can't overflow.
    #[test]
    fn idle_and_hold_do_not_count_samples() {
        for (f, key) in [
            (Func::Env(EnvForm::Ad), false),
            (Func::Env(EnvForm::Ahr), true),
        ] {
            let mut p = FuncParams {
                rise: 0.0,
                ..FuncParams::ENV
            };
            p.set_func(f);
            let c = BCoefs::new(&p, &Slides::default(), 48_000, false);
            let mut g = FuncGen::new();
            g.set(&c);
            if key {
                g.note_on(f);
                g.advance(&c, true, 1_000); // AHR holds at 1
            }
            for _ in 0..3 {
                g.advance(&c, key, u32::MAX / 2); // overflows a counting `k`
            }
            assert_eq!(g.output(), if key { 1.0 } else { 0.0 }, "{f:?}");
        }
    }

    /// CYCLE entered at level 0 (from a finished AD, or an idle A) rises at
    /// once instead of parking; AD entered at 0 stays done.
    #[test]
    fn cycle_entered_at_zero_rises() {
        for (f, moves) in [
            (Func::Env(EnvForm::Cycle), true),
            (Func::Env(EnvForm::Ad), false),
        ] {
            let mut p = FuncParams::ENV;
            p.set_func(f);
            let c = BCoefs::new(&p, &Slides::default(), 48_000, false);
            let mut g = FuncGen::new();
            g.set(&c);
            g.enter_env(0.0, false);
            g.advance(&c, true, 64);
            assert_eq!(g.output() > 0.0, moves, "{f:?}");
        }
    }
}
