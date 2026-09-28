//! An ENV slot of the modulator pool (spec § 1): runs its TYPE from
//! `EnvParams` and the matrix's inputs, once per block or per sample.

use chimera_hal::BLOCK_SIZE;

use crate::dsp::modulator::env_a::{ACoefs, EnvA};
use crate::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use crate::dsp::modulator::{EnvType, Func, FuncParams, Glide};
use crate::params::EnvParams;

/// What the matrix feeds an ENV slot, from the previous block (spec
/// § Signal flow 1), so a slot never waits on the matrix it feeds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvMods {
    /// The peak with a route into LEVEL, `clamp(Σ, 0, 1)`; `None` without one (peak 1).
    pub level: Option<f32>,
    /// TIME's Σ.
    pub time: f32,
    /// RISE, FALL and SHAPE's Σ, added to their positions (type B).
    pub slides: Slides,
}

impl EnvMods {
    pub const NONE: Self = Self {
        level: None,
        time: 0.0,
        slides: Slides {
            rise: 0.0,
            fall: 0.0,
            shape: 0.0,
        },
    };
}

/// What an A slot's coefficients were built from: equal inputs reuse them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AKey {
    stages: [f32; 5], // attack, decay, sustain, release, hold
    speed: crate::dsp::modulator::EnvSpeed,
    hold_pos: crate::dsp::modulator::HoldPos,
    time: f32,
    sample_rate: u32,
}

impl AKey {
    fn of(p: &EnvParams, time: f32, sample_rate: u32) -> Self {
        Self {
            stages: [p.attack, p.decay, p.sustain, p.release, p.hold],
            speed: p.speed,
            hold_pos: p.hold_pos,
            time,
            sample_rate,
        }
    }
}

/// What a slot runs (spec § 1): Envelope A, or B with its MODE and FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    A,
    B(Func),
}

impl Kind {
    fn of(p: &EnvParams) -> Self {
        match p.env_type {
            EnvType::A => Kind::A,
            EnvType::B => Kind::B(p.func.func()),
        }
    }
}

/// A block's coefficients: the running TYPE's only.
enum Coefs {
    A(ACoefs),
    B(BCoefs),
}

/// What a B slot's coefficients were built from.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BKey {
    func: FuncParams,
    slides: Slides,
    sample_rate: u32,
    per_sample: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    a: EnvA,
    b: FuncGen,
    /// What ran last block; `None` before the first.
    kind: Option<Kind>,
    /// A note-on the next block runs, after any take-over, with that
    /// block's coefficients.
    note_on: bool,
    /// The running TYPE's coefficients and their inputs, reused while
    /// the inputs hold.
    ac: Option<(AKey, ACoefs)>,
    bc: Option<(BKey, BCoefs)>,
    /// This block's peak (LEVEL; 1 under B) and the last block's.
    peak: f32,
    prev_peak: f32,
    /// A change's leftover, gliding out.
    glide: Glide,
    /// The glided output's floor: the old and new kinds' ranges joined
    /// (−1 if either is an LFO, else 0, or `old` if lower); the ceiling is 1.
    glide_lo: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    pub const fn new() -> Self {
        Self {
            a: EnvA::new(),
            b: FuncGen::new(),
            kind: None,
            note_on: false,
            ac: None,
            bc: None,
            peak: 1.0,
            prev_peak: 1.0,
            glide: Glide::NONE,
            glide_lo: 0.0,
        }
    }

    /// This block's A coefficients, rebuilt only when an input changed.
    fn a_coefs(&mut self, p: &EnvParams, time: f32, sample_rate: u32) -> ACoefs {
        let key = AKey::of(p, time, sample_rate);
        match self.ac {
            Some((k, c)) if k == key => c,
            _ => {
                let c = ACoefs::new(p, time, sample_rate);
                self.ac = Some((key, c));
                c
            }
        }
    }

    /// This block's B coefficients, rebuilt only when an input changed.
    fn b_coefs(
        &mut self,
        p: &EnvParams,
        slides: &Slides,
        sample_rate: u32,
        per_sample: bool,
    ) -> BCoefs {
        let key = BKey {
            func: p.func,
            slides: *slides,
            sample_rate,
            per_sample,
        };
        match self.bc {
            Some((k, c)) if k == key => c,
            _ => {
                let c = BCoefs::new(&p.func, slides, sample_rate, per_sample);
                self.bc = Some((key, c));
                c
            }
        }
    }

    /// `raw` plus the leftover `n` samples on, within the glide's range.
    fn glided(&self, raw: f32, n: usize) -> f32 {
        (raw + self.glide.at(n)).clamp(self.glide_lo, 1.0)
    }

    /// `gain[n] += amount · level[n] · peak[n]`, the peak ramped from `from`
    /// to `to` across the block, so a route into LEVEL doesn't zipper the
    /// VCA; a gliding change's leftover in the same pass.
    fn add_ramped(
        &self,
        gain: &mut [f32; BLOCK_SIZE],
        level: &[f32; BLOCK_SIZE],
        amount: f32,
        from: f32,
        to: f32,
    ) {
        let step = (to - from) / BLOCK_SIZE as f32;
        if self.glide.active() {
            for (n, (g, l)) in gain.iter_mut().zip(level).enumerate() {
                let raw = l * (from + step * (n + 1) as f32);
                *g += amount * self.glided(raw, n + 1);
            }
        } else if from == to {
            let a = amount * to;
            for (g, l) in gain.iter_mut().zip(level) {
                *g += a * l;
            }
        } else {
            for (n, (g, l)) in gain.iter_mut().zip(level).enumerate() {
                *g += amount * l * (from + step * (n + 1) as f32);
            }
        }
    }

    fn is_b(&self) -> bool {
        matches!(self.kind, Some(Kind::B(_)))
    }

    /// A note-on, run by the next block: a TYPE, MODE or FORM change not
    /// yet seen takes over first, with the new kind's coefficients, then
    /// the new kind's note-on runs.
    pub fn note_on(&mut self) {
        self.note_on = true;
    }

    fn raw(&self) -> f32 {
        if self.is_b() {
            self.b.output()
        } else {
            self.a.level() * self.peak
        }
    }

    /// The output now: 0..1, or −1..1 for B in LFO mode. No velocity.
    pub fn output(&self) -> f32 {
        if self.glide.active() {
            self.glided(self.raw(), 0)
        } else {
            self.raw()
        }
    }

    pub fn is_idle(&self) -> bool {
        if self.note_on {
            false
        } else if self.is_b() {
            self.b.is_idle()
        } else {
            self.a.is_idle()
        }
    }

    /// Whether this slot, routed to the VCA, still holds the voice (spec § 4).
    pub fn holds(&self, key: bool) -> bool {
        if self.note_on {
            true
        } else if self.is_b() {
            self.b.holds(key)
        } else {
            !self.a.is_idle()
        }
    }

    /// A change's leftover is still gliding out.
    pub fn gliding(&self) -> bool {
        self.glide.active()
    }

    fn rising(&self) -> bool {
        if self.is_b() {
            self.b.rising()
        } else {
            self.a.rising()
        }
    }

    /// The new kind takes over at the old output `old` (spec § 1): A and B
    /// ENV enter at that level; the rest start where they would. The caller
    /// glides out the difference.
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    fn take_over(&mut self, k: Kind, old: f32, rising: bool, key: bool, sus: f32) {
        let level = old.max(0.0).min(1.0);
        match k {
            Kind::A => self.a.enter(level, rising, key, sus),
            Kind::B(Func::Env(_)) => self.b.enter_env(level, rising),
            Kind::B(Func::Burst(_)) => self.b.enter_burst(key),
            Kind::B(Func::Lfo(_)) => {}
        }
    }

    /// One block. Returns the output at the block's start. With `vca`, the
    /// slot fills a block of outputs and adds `amount ·` each into the
    /// buffer (the peak ramped, any glide in the same pass); otherwise it
    /// advances in closed form. Only the running TYPE's coefficients are
    /// computed.
    pub fn run_block(
        &mut self,
        p: &EnvParams,
        m: &EnvMods,
        key: bool,
        sample_rate: u32,
        vca: Option<(&mut [f32; BLOCK_SIZE], f32)>,
    ) -> f32 {
        let kind = Kind::of(p);
        let (old, rising) = (self.output(), self.rising());
        let prev = self.kind.replace(kind);
        self.prev_peak = self.peak;
        self.peak = if kind == Kind::A {
            m.level.unwrap_or(1.0)
        } else {
            1.0
        };
        let c = match kind {
            Kind::A => Coefs::A(self.a_coefs(p, m.time, sample_rate)),
            Kind::B(_) => Coefs::B(self.b_coefs(p, &m.slides, sample_rate, vca.is_some())),
        };
        if let Coefs::B(c) = &c {
            self.b.set(c);
        }
        let changed = prev.is_some_and(|k| k != kind);
        if changed {
            self.take_over(kind, old, rising, key, p.sustain);
        }
        if core::mem::take(&mut self.note_on) {
            match kind {
                Kind::A => self.a.note_on(),
                Kind::B(f) => self.b.note_on(f),
            }
        }
        if changed {
            // After the note-on, so the glide ends where the new kind starts.
            self.glide.start(old - self.raw());
            let lfo = |k: Option<Kind>| matches!(k, Some(Kind::B(Func::Lfo(_))));
            // `old` may still be gliding up from an LFO: below 0.
            self.glide_lo = if lfo(prev) || lfo(Some(kind)) {
                -1.0
            } else {
                old.min(0.0)
            };
        }
        if let Coefs::B(_) = c
            && !key
        {
            self.b.key_up();
        }
        let start = self.output();
        match (vca, &c) {
            (Some((gain, amount)), c) => {
                let mut level = [0.0f32; BLOCK_SIZE];
                let (from, to) = match c {
                    Coefs::A(c) => {
                        self.a.fill(c, key, &mut level);
                        (self.prev_peak, self.peak)
                    }
                    Coefs::B(c) => {
                        self.b.fill(c, key, &mut level);
                        (1.0, 1.0)
                    }
                };
                self.add_ramped(gain, &level, amount, from, to);
            }
            (None, Coefs::A(c)) => self.a.advance(c, key, BLOCK_SIZE as u32),
            (None, Coefs::B(c)) => self.b.advance(c, key, BLOCK_SIZE as u32),
        }
        self.glide.advance(BLOCK_SIZE as u16);
        start
    }
}
