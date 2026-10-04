//! The master reverb TYPEs (reverb TYPEs spec, ADR 0085): `Reverb` runs
//! the half-band rate change, SMEAR's source and depth, and MIX, and
//! dispatches the block to the TYPE's engine over the one shared arena.

pub mod arena;
pub mod parts;
pub mod sync;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::Stereo;
use crate::dsp::clampf;
use crate::dsp::ease::{Ease, Gate, at, ease_coeff, step_of};
use crate::dsp::halfband::{Decimator, HALF, Interpolator};
use crate::dsp::reverb::ReverbParams;
use crate::dsp::ring::{FULL_SCALE, STAGES, WET_GAIN, smear_depth, smear_hz};
use arena::ReverbArena;
use parts::{Wander, wander_seeds};
use sync::Tempo;

pub use crate::dsp::ring::QuadState;

/// The running TYPE's state. Zero bytes are `Quad(QuadState::default())`:
/// tag 0 is `Quad`, and every field of every variant is an integer, a
/// float or a `bool`.
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineState {
    Quad(QuadState),
}

const _: () = assert!(core::mem::size_of::<EngineState>() <= 160);

pub struct Reverb {
    arena: ReverbArena,
    state: EngineState,
    dec: Decimator,
    up_l: Interpolator,
    up_r: Interpolator,
    /// SMEAR's knob, eased (never snap).
    smear: Ease,
    /// SMEAR's depth on or resting: it rests, linearly over 20 ms, while
    /// a SIZE move waits or crossfades.
    smear_on: Gate,
    /// One wander per smeared line.
    wander: [Wander; STAGES],
    /// !RATE's bits the wanders were set for (zero bytes match none).
    rate_key: u32,
    /// The wanders held at their centre (tests only).
    frozen: bool,
    /// Last block's (SIZE crossfade, SMEAR) paths, and its end's depth.
    paths: (bool, bool),
    depth: f32,
    /// MIX, eased (never snap).
    mix: Ease,
}

crate::in_place::field_list!(Reverb => Reverb {
    arena, state, dec, up_l, up_r, smear, smear_on, wander, rate_key, frozen, paths, depth, mix
});

/// QUAD's SMEAR seed: its four lines' wanders hash it.
const QUAD_SEED: u32 = 0x5EED_0343;

/// QUAD's wanders, line k starting k/4 of a period in.
fn quad_wanders() -> [Wander; STAGES] {
    let seeds = wander_seeds::<STAGES>(QUAD_SEED);
    core::array::from_fn(|k| Wander::new(seeds[k], k as f32 / STAGES as f32))
}

impl Reverb {
    pub fn new() -> Self {
        Self {
            arena: ReverbArena::new(),
            state: EngineState::Quad(QuadState::default()),
            dec: Decimator::new(),
            up_l: Interpolator::new(),
            up_r: Interpolator::new(),
            smear: Ease::default(),
            smear_on: Gate::default(),
            wander: quad_wanders(),
            rate_key: 0,
            frozen: false,
            paths: (false, false),
            depth: 0.0,
            mix: Ease::default(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the arena and half-bands are integer and float arrays and
        // integers; `state` is `EngineState`, whose variant 0 is `Quad` and
        // every field of every variant an integer, a float or a `bool`
        // (`false`); `smear` and `mix` are `Ease`s of a float and a
        // `bool`, `smear_on` a `Gate` of a `u8` and a `bool`, `wander`
        // integers and floats, the rest integers and `bool`s. All are valid
        // as zero bytes, and zero is `new()`'s state but for the wanders'
        // seeds and phases, set below.
        let r = unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        };
        r.wander = quad_wanders();
        r
    }

    /// Hold every wander at its centre from now on: SMEAR's reads sit `D`
    /// short, still. The pitch reference for `check_smear_bound`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn freeze_smear(&mut self) {
        self.frozen = true;
    }

    /// Last block's paths: (a SIZE crossfade ran, SMEAR moved the reads).
    pub fn debug_paths(&self) -> (bool, bool) {
        self.paths
    }

    /// SMEAR's depth D at last block's end, in ring samples.
    pub fn debug_depth(&self) -> f32 {
        self.depth
    }

    pub fn arena(&self) -> &ReverbArena {
        &self.arena
    }

    /// QUAD's state, while QUAD runs.
    pub fn quad(&self) -> Option<&QuadState> {
        match &self.state {
            EngineState::Quad(q) => Some(q),
        }
    }

    /// Wet only: the engine's return × `mix`, into `out`. QUAD runs free
    /// of `tempo`.
    pub fn process(
        &mut self,
        send: &[f32; BLOCK_SIZE],
        p: &ReverbParams,
        mix: f32,
        _tempo: Tempo,
        sample_rate: u32,
        out: &mut Stereo,
    ) {
        let mut u = [0.0f32; HALF];
        self.dec.process(send, &mut u);
        let mut lr = [[0.0f32; HALF]; 2];
        let fs = sample_rate as f32 * 0.5;
        let rate = if p.rate.is_nan() { 0.5 } else { p.rate };
        if !rate.to_bits() != self.rate_key {
            self.rate_key = !rate.to_bits();
            for w in &mut self.wander {
                w.set_rate(smear_hz(rate), fs);
            }
        }
        let x0 = self.wander.map(|w| w.value());
        let x1 = if self.frozen {
            x0
        } else {
            core::array::from_fn(|k| self.wander[k].block())
        };
        let knob = if p.smear.is_nan() {
            0.0
        } else {
            clampf(p.smear, 0.0, 1.0)
        };
        let (s0, s1) = self.smear.step(knob, ease_coeff(sample_rate));
        match &mut self.state {
            EngineState::Quad(q) => {
                let (g0, g1) = self.smear_on.step(!q.wants_size(p.size));
                let step = q.ring().step();
                let (d0, d1) = (
                    smear_depth(s0, step, fs) * g0,
                    smear_depth(s1, step, fs) * g1,
                );
                let smear = (d0 != 0.0 || d1 != 0.0)
                    .then(|| core::array::from_fn(|k| (d0 * (1.0 + x0[k]), d1 * (1.0 + x1[k]))));
                self.paths = (q.ring().crossfading(), smear.is_some());
                self.depth = d1;
                q.process(
                    &mut self.arena,
                    &u,
                    &p.controls(),
                    smear,
                    sample_rate,
                    &mut lr,
                );
                if q.ring().crossfading() {
                    self.paths.0 = true;
                }
            }
        }
        self.up_l.process(&lr[0], &mut out.l);
        self.up_r.process(&lr[1], &mut out.r);
        let (from, to) = self.mix.step(mix, ease_coeff(sample_rate));
        let gain = WET_GAIN * to / FULL_SCALE;
        if from == to {
            for s in out.l.iter_mut().chain(out.r.iter_mut()) {
                *s *= gain;
            }
        } else {
            let (g0, step) = (
                WET_GAIN * from / FULL_SCALE,
                step_of((from, to), BLOCK_SIZE) * WET_GAIN / FULL_SCALE,
            );
            for side in [&mut out.l, &mut out.r] {
                for (i, s) in side.iter_mut().enumerate() {
                    *s *= at(g0, step, i);
                }
            }
        }
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}
