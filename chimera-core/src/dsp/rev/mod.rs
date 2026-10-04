//! The master reverb TYPEs (reverb TYPEs spec, ADR 0085): `Reverb` runs
//! the half-band rate change and MIX, and dispatches the block to the
//! TYPE's engine over the one shared arena.

pub mod arena;
pub mod parts;
pub mod sync;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::Stereo;
use crate::dsp::ease::{Ease, at, ease_coeff, step_of};
use crate::dsp::halfband::{Decimator, HALF, Interpolator};
use crate::dsp::reverb::ReverbParams;
use crate::dsp::ring::{FULL_SCALE, WET_GAIN};
use arena::ReverbArena;
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
    /// MIX, eased (never snap).
    mix: Ease,
}

crate::in_place::field_list!(Reverb => Reverb { arena, state, dec, up_l, up_r, mix });

impl Reverb {
    pub fn new() -> Self {
        Self {
            arena: ReverbArena::new(),
            state: EngineState::Quad(QuadState::default()),
            dec: Decimator::new(),
            up_l: Interpolator::new(),
            up_r: Interpolator::new(),
            mix: Ease::default(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the arena and half-bands are integer and float arrays and
        // integers; `state` is `EngineState`, whose variant 0 is `Quad` and
        // every field of every variant an integer, a float or a `bool`
        // (`false`); `mix` an `Ease` of a float and a `bool`. All are valid
        // as zero bytes, and zero is exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
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
        let (mut l, mut r) = ([0.0f32; HALF], [0.0f32; HALF]);
        match &mut self.state {
            EngineState::Quad(q) => q.process(
                &mut self.arena,
                &u,
                &p.controls(),
                sample_rate,
                &mut l,
                &mut r,
            ),
        }
        self.up_l.process(&l, &mut out.l);
        self.up_r.process(&r, &mut out.r);
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
