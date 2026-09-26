//! Persistent engine instances with dispatch in one place (spec §3).
//!
//! Engines are never constructed in the audio interrupt: `ModalEngine` is
//! ~40 KB and the stack has no guard. Adding an engine = one field here plus
//! one arm in each exhaustive `match` below; the compiler lists them.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};
use crate::dsp::modal::ModalEngine;
use crate::hw::Cost;
use crate::in_place::{by_value, uninit_at};
use crate::params::{EngineType, ParamSnapshot};
use crate::{MidiNote, Velocity};

pub struct Engines {
    algo: AlgoEngine,
    modal: ModalEngine,
    sample_rate: u32,
}

crate::in_place::field_list!(Engines => Engines { algo, modal, sample_rate });

impl Engines {
    pub fn new(sample_rate: u32) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, sample_rate)) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; both engines are built in
        // place and the sample rate written once, before `assume_init_mut`.
        unsafe {
            AlgoEngine::init_in_place(uninit_at(addr_of_mut!((*p).algo)));
            ModalEngine::init_in_place(uninit_at(addr_of_mut!((*p).modal)));
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            slot.assume_init_mut()
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn note_on(&mut self, kind: EngineType, note: MidiNote, vel: Velocity, p: &ParamSnapshot) {
        match kind {
            EngineType::Algo => self.algo.note_on(note, vel, &p.algo, self.sample_rate),
            EngineType::Modal => {
                self.modal
                    .note_on(note.get(), vel.get(), &p.modal, self.sample_rate)
            }
        }
    }

    pub fn note_off(&mut self, kind: EngineType) {
        match kind {
            EngineType::Algo => self.algo.note_off(),
            EngineType::Modal => self.modal.note_off(),
        }
    }

    /// Render one block of raw engine output from (possibly modulated) params.
    pub fn render(
        &mut self,
        kind: EngineType,
        out: &mut [f32; BLOCK_SIZE],
        p: &ParamSnapshot,
        live: &AlgoLive,
    ) {
        match kind {
            EngineType::Algo => self.algo.render(out, &p.algo, live, self.sample_rate),
            EngineType::Modal => self.modal.render(out, &p.modal, self.sample_rate),
        }
    }

    /// Cycles/sample of one engine instance (ADR 0013).
    pub const fn cost(kind: EngineType) -> Cost {
        match kind {
            EngineType::Algo => AlgoEngine::COST,
            EngineType::Modal => ModalEngine::COST,
        }
    }

    /// VCA choice: does the amp envelope shape this engine's output? Modal's
    /// modes decay naturally and Algo's operators carry their own
    /// envelopes, so both only get the volume.
    pub fn uses_amp_env(kind: EngineType) -> bool {
        match kind {
            EngineType::Algo | EngineType::Modal => false,
        }
    }

    /// Voice lifetime: is this engine still sounding?
    pub fn is_active(&self, kind: EngineType) -> bool {
        match kind {
            EngineType::Algo => self.algo.is_active(),
            EngineType::Modal => self.modal.is_active(),
        }
    }
}
