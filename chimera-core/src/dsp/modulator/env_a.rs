//! Envelope A (spec § Envelope A): an AHDSR with the Cascadia's HOLD
//! POSITION and SPEED ranges and one fixed RC shape: each stage approaches
//! a target past its end, as a capacitor charging toward a rail.

use core::f32::consts::LN_2;

use crate::dsp::modulator::HoldPos;
use crate::dsp::modulator::law::{rc_coeff, rc_k, speed_ranges};
use crate::dsp::{fast_exp2, fast_log2};
use crate::params::EnvParams;

/// Attack aims at 1.3 and stops at 1: τ = A / ln(1.3 / 0.3).
const ATTACK_TARGET: f32 = 1.3;
const ATTACK_LN: f32 = 1.466_337;
/// Decay and release aim 0.01 past their end: τ = time / ln(1.01 / 0.01).
const OVERSHOOT: f32 = 0.01;
const DEC_REL_LN: f32 = 4.615_12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// One RC stage for a block: `k`, the `log2` of the per-sample retention,
/// and `c = 1 − 2^k`, the per-sample step.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rc {
    k: f32,
    c: f32,
}

impl Rc {
    fn new(secs: f32, ln: f32, fs: f32) -> Self {
        let k = rc_k(secs / ln, fs);
        Self { k, c: rc_coeff(k) }
    }
}

/// A's constants for one block: SPEED, the slider laws and TIME resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ACoefs {
    attack: Rc,
    decay: Rc,
    release: Rc,
    sus: f32,
    /// H in samples: AHDSR's hold stage, GATE EXT's gate.
    hold: u32,
    hold_pos: HoldPos,
}

impl ACoefs {
    /// `time`: TIME's route sum. Every stage × 2^(−5·Σ), Σ clamped to ±1
    /// (NaN reads as −1).
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    pub fn new(p: &EnvParams, time: f32, sample_rate: u32) -> Self {
        let fs = sample_rate as f32;
        let r = speed_ranges(p.speed);
        let scale = fast_exp2(-5.0 * time.max(-1.0).min(1.0));
        Self {
            attack: Rc::new(r.attack.at(p.attack) * scale, ATTACK_LN, fs),
            decay: Rc::new(r.dec_rel.at(p.decay) * scale, DEC_REL_LN, fs),
            release: Rc::new(r.dec_rel.at(p.release) * scale, DEC_REL_LN, fs),
            sus: p.sustain.max(0.0).min(1.0),
            hold: (r.hold.at(p.hold) * scale * fs) as u32,
            hold_pos: p.hold_pos,
        }
    }
}

/// Envelope A's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvA {
    stage: Stage,
    level: f32,
    /// Samples left in Hold.
    hold_left: u32,
    /// Samples since the note-on, saturating (GATE EXT's gate).
    since_on: u32,
}

impl Default for EnvA {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvA {
    pub const fn new() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            hold_left: 0,
            since_on: u32::MAX,
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// 0..1.
    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn rising(&self) -> bool {
        self.stage == Stage::Attack
    }

    /// Attack from the current level (the Cascadia's RETRIG).
    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.since_on = 0;
    }

    /// Take over at `level` after a TYPE change (spec § 1): the level stays.
    /// `sus` is S, 0..1.
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    pub fn enter(&mut self, level: f32, rising: bool, key: bool, sus: f32) {
        self.level = level.max(0.0).min(1.0);
        self.since_on = u32::MAX;
        self.stage = match (key, rising) {
            (false, _) if self.level > 0.0 => Stage::Release,
            (false, _) => Stage::Idle,
            (true, true) => Stage::Attack,
            (true, false) if self.level > sus => Stage::Decay,
            (true, false) => Stage::Sustain,
        };
    }

    fn gate(&self, c: &ACoefs, key: bool) -> bool {
        key || (c.hold_pos == HoldPos::GateExt && self.since_on < c.hold)
    }

    /// The stage changes that take no time: a fallen gate, a finished
    /// hold, a sustain above a lowered S, a decay under a raised S.
    fn settle(&mut self, c: &ACoefs, gate: bool) {
        let running = matches!(
            self.stage,
            Stage::Attack | Stage::Hold | Stage::Decay | Stage::Sustain
        );
        if running && !gate {
            self.stage = Stage::Release;
        } else if (self.stage == Stage::Hold && self.hold_left == 0)
            || (self.stage == Stage::Sustain && self.level > c.sus)
        {
            self.stage = Stage::Decay;
        } else if self.stage == Stage::Decay && self.level <= c.sus {
            // A raised S doesn't lift a decaying level: sustain where it is.
            self.stage = Stage::Sustain;
        }
    }

    /// The running RC stage: its constants, target, end and direction.
    fn rc(&self, c: &ACoefs) -> Option<(Rc, f32, f32, bool)> {
        match self.stage {
            Stage::Attack => Some((c.attack, ATTACK_TARGET, 1.0, true)),
            Stage::Decay => Some((c.decay, c.sus - OVERSHOOT, c.sus, false)),
            Stage::Release => Some((c.release, -OVERSHOOT, 0.0, false)),
            _ => None,
        }
    }

    /// An RC stage reached its end.
    fn finish(&mut self, c: &ACoefs, end: f32) {
        self.level = end;
        self.stage = match self.stage {
            Stage::Attack if c.hold_pos == HoldPos::Ahdsr && c.hold > 0 => {
                self.hold_left = c.hold;
                Stage::Hold
            }
            Stage::Attack => Stage::Decay,
            Stage::Decay => Stage::Sustain,
            _ => Stage::Idle,
        };
    }

    /// One sample: one multiply-add and a compare, no divide. Returns the
    /// level after it.
    pub fn tick(&mut self, c: &ACoefs, key: bool) -> f32 {
        let gate = self.gate(c, key);
        self.since_on = self.since_on.saturating_add(1);
        self.settle(c, gate);
        if self.stage == Stage::Hold {
            self.hold_left -= 1;
        } else if let Some((rc, t, end, rising)) = self.rc(c) {
            let l = self.level + rc.c * (t - self.level);
            if (rising && l >= end) || (!rising && l <= end) {
                self.finish(c, end);
            } else {
                self.level = l;
            }
        }
        self.level
    }

    /// A block of per-sample levels, as `tick` would give them (the same
    /// arithmetic), for the VCA: one tight loop per stretch of a stage, the
    /// gate and settling checked once per stretch, not per sample.
    pub fn fill(&mut self, c: &ACoefs, key: bool, out: &mut [f32]) {
        let mut i = 0;
        while i < out.len() {
            let gate = self.gate(c, key);
            self.settle(c, gate);
            let left = (out.len() - i) as u32;
            // GATE EXT's own gate falls inside this stretch: stop there.
            let span = (if gate && !key {
                left.min(c.hold - self.since_on)
            } else {
                left
            }) as usize;
            let used = match self.rc(c) {
                None => {
                    let m = if self.stage == Stage::Hold {
                        let m = span.min(self.hold_left as usize);
                        self.hold_left -= m as u32;
                        m
                    } else {
                        span
                    };
                    out[i..i + m].fill(self.level);
                    m
                }
                Some((rc, t, end, rising)) => {
                    let mut l = self.level;
                    let mut m = 0;
                    let mut ended = false;
                    while m < span {
                        l += rc.c * (t - l);
                        m += 1;
                        if (rising && l >= end) || (!rising && l <= end) {
                            ended = true;
                            break;
                        }
                        out[i + m - 1] = l;
                    }
                    if ended {
                        self.finish(c, end);
                        out[i + m - 1] = end;
                    } else {
                        self.level = l;
                    }
                    m
                }
            };
            self.since_on = self.since_on.saturating_add(used as u32);
            i += used;
        }
    }

    /// `n` samples in closed form, `L_n = T + (L − T)·2^(k·n)`, stage ends
    /// placed inside the block (within ±1 sample of the ticks, ADR 0036).
    pub fn advance(&mut self, c: &ACoefs, key: bool, mut n: u32) {
        while n > 0 {
            let gate = self.gate(c, key);
            self.settle(c, gate);
            // GATE EXT's own gate falls inside this stretch: stop there.
            let span = if gate && !key {
                n.min(c.hold - self.since_on)
            } else {
                n
            };
            let used = self.run(c, span);
            self.since_on = self.since_on.saturating_add(used);
            n -= used;
        }
    }

    /// Up to `span` samples of the running stage; returns how many it took.
    fn run(&mut self, c: &ACoefs, span: u32) -> u32 {
        match self.stage {
            Stage::Idle | Stage::Sustain => span,
            Stage::Hold => {
                let m = span.min(self.hold_left);
                self.hold_left -= m;
                m
            }
            _ => {
                let Some((rc, t, end, _)) = self.rc(c) else {
                    return span;
                };
                let m = steps_to(self.level, t, end, rc.k);
                if m <= span {
                    self.finish(c, end);
                    m
                } else {
                    self.level = t + (self.level - t) * fast_exp2(rc.k * span as f32);
                    span
                }
            }
        }
    }

    /// Samples until the running RC stage ends (`u32::MAX` in Idle, Hold
    /// and Sustain).
    pub fn stage_samples(&self, c: &ACoefs) -> u32 {
        self.rc(c).map_or(u32::MAX, |(rc, t, end, _)| {
            steps_to(self.level, t, end, rc.k)
        })
    }
}

/// Ticks until an RC stage from `l` toward `t` reaches `end` (at least 1;
/// `u32::MAX` if it never does): `fast_log2` places it and one Newton step
/// on `2^(k·m)` sharpens it to `exp2`'s precision.
fn steps_to(l: f32, t: f32, end: f32, k: f32) -> u32 {
    let ratio = (end - t) / (l - t);
    if ratio >= 1.0 {
        return 1;
    }
    if ratio.is_nan() || ratio <= 0.0 || k >= 0.0 {
        return u32::MAX;
    }
    let m = fast_log2(ratio) / k;
    let e = fast_exp2(k * m);
    let m = m - (e - ratio) / (e * k * LN_2);
    if m >= u32::MAX as f32 {
        return u32::MAX;
    }
    let i = m as u32;
    if (i as f32) < m { i + 1 } else { i.max(1) }
}
