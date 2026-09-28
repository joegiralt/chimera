//! An ENV slot of the modulator pool (spec § 1): runs its TYPE from
//! `EnvParams` and the matrix's inputs, once per block or per sample.

use chimera_hal::BLOCK_SIZE;

use crate::dsp::modulator::env_a::{ACoefs, EnvA};
use crate::params::EnvParams;

/// What the matrix feeds an ENV slot, from the previous block (spec
/// § Signal flow 1), so a slot never waits on the matrix it feeds. Task 6
/// adds RISE, FALL and SHAPE as `slides`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvMods {
    /// The peak with a route into LEVEL, `clamp(Σ, 0, 1)`; `None` without one (peak 1).
    pub level: Option<f32>,
    /// TIME's Σ.
    pub time: f32,
}

impl EnvMods {
    pub const NONE: Self = Self {
        level: None,
        time: 0.0,
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

/// `gain[n] += amount · level[n] · peak[n]`, the peak ramped from `from` to
/// `to` across the block, so a route into LEVEL doesn't zipper the VCA.
fn add_ramped(
    gain: &mut [f32; BLOCK_SIZE],
    level: &[f32; BLOCK_SIZE],
    amount: f32,
    from: f32,
    to: f32,
) {
    if from == to {
        let a = amount * to;
        for (g, l) in gain.iter_mut().zip(level) {
            *g += a * l;
        }
    } else {
        let step = (to - from) / BLOCK_SIZE as f32;
        for (n, (g, l)) in gain.iter_mut().zip(level).enumerate() {
            *g += amount * l * (from + step * (n + 1) as f32);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    a: EnvA,
    /// The last A coefficients and their inputs.
    ac: Option<(AKey, ACoefs)>,
    /// This block's peak (LEVEL) and the last block's.
    peak: f32,
    prev_peak: f32,
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
            ac: None,
            peak: 1.0,
            prev_peak: 1.0,
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

    pub fn note_on(&mut self, _p: &EnvParams) {
        self.a.note_on();
    }

    /// The raw contour, 0..1: no velocity (spec § 1).
    pub fn output(&self) -> f32 {
        self.a.level() * self.peak
    }

    pub fn is_idle(&self) -> bool {
        self.a.is_idle()
    }

    /// One block. Returns the output at the block's start. With `vca`, the
    /// slot fills a block of levels and adds `amount · level · peak` into
    /// the buffer (the VCA's sum), the peak ramped per sample; otherwise it
    /// advances in closed form.
    pub fn run_block(
        &mut self,
        p: &EnvParams,
        m: &EnvMods,
        key: bool,
        sample_rate: u32,
        vca: Option<(&mut [f32; BLOCK_SIZE], f32)>,
    ) -> f32 {
        let c = self.a_coefs(p, m.time, sample_rate);
        self.prev_peak = self.peak;
        self.peak = m.level.unwrap_or(1.0);
        let start = self.output();
        match vca {
            Some((gain, amount)) => {
                let mut level = [0.0f32; BLOCK_SIZE];
                self.a.fill(&c, key, &mut level);
                add_ramped(gain, &level, amount, self.prev_peak, self.peak);
            }
            None => self.a.advance(&c, key, BLOCK_SIZE as u32),
        }
        start
    }
}
