//! Persistent engine instances with dispatch in one place (spec §3).
//!
//! Engines are never constructed in the audio interrupt: `ModalEngine` is
//! ~40 KB and the stack has no guard. Adding an engine = one field here plus
//! one arm in each exhaustive `match` below; the compiler lists them.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::engine_fm::FmEngine;
use crate::dsp::envelope::Envelope;
use crate::dsp::modal::ModalEngine;
use crate::dsp::pizza::PizzaOsc;
use crate::hw::Cost;
use crate::in_place::{by_value, uninit_at};
use crate::params::{EngineType, ParamSnapshot};
use crate::{MidiNote, Velocity};

/// Design doc § CPU Budget: VA Polymod (2 osc + sync + PWM) ~300. The VA
/// engine is a silent placeholder; its budget is reserved now.
const VA_COST: Cost = Cost(300); // estimate

pub struct Engines {
    pizza: PizzaOsc,
    fm: FmEngine,
    modal: ModalEngine,
    sample_rate: u32,
}

crate::in_place::field_list!(Engines => Engines { pizza, fm, modal, sample_rate });

impl Engines {
    pub fn new(sample_rate: u32) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, sample_rate)) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; Modal (40 KB) is built in place,
        // Pizza and FM (about 1.1 KB) by value, each field once.
        unsafe {
            addr_of_mut!((*p).pizza).write(PizzaOsc::new());
            addr_of_mut!((*p).fm).write(FmEngine::new());
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
            EngineType::Pizza => self
                .pizza
                .note_on(crate::dsp::note_to_freq(note.get()), self.sample_rate),
            EngineType::Fm => {
                self.fm
                    .note_on_params(note.get(), vel.unit(), &p.fm, self.sample_rate as f32)
            }
            EngineType::Modal => {
                self.modal
                    .note_on(note.get(), vel.get(), &p.modal, self.sample_rate)
            }
            EngineType::Va => {} // placeholder: silent
        }
    }

    pub fn note_off(&mut self, kind: EngineType) {
        match kind {
            EngineType::Pizza => self.pizza.note_off(),
            EngineType::Fm => self.fm.note_off(),
            EngineType::Modal => self.modal.note_off(),
            EngineType::Va => {}
        }
    }

    /// Render one block of raw engine output from (possibly modulated) params.
    pub fn render(&mut self, kind: EngineType, out: &mut [f32; BLOCK_SIZE], p: &ParamSnapshot) {
        match kind {
            EngineType::Pizza => self.pizza.render(out, &p.pizza, self.sample_rate),
            EngineType::Fm => self.fm.render_params(out, &p.fm),
            EngineType::Modal => self.modal.render(out, &p.modal, self.sample_rate),
            EngineType::Va => out.fill(0.0),
        }
    }

    /// Cycles/sample of one engine instance (ADR 0013).
    pub const fn cost(kind: EngineType) -> Cost {
        match kind {
            EngineType::Pizza => PizzaOsc::COST,
            EngineType::Fm => FmEngine::COST,
            EngineType::Modal => ModalEngine::COST,
            EngineType::Va => VA_COST,
        }
    }

    /// VCA choice: does the amp envelope shape this engine's output?
    /// Modal's modes decay naturally, so it only gets the volume.
    pub fn uses_amp_env(kind: EngineType) -> bool {
        match kind {
            EngineType::Pizza | EngineType::Fm | EngineType::Va => true,
            EngineType::Modal => false,
        }
    }

    /// Voice lifetime: is this engine still sounding?
    pub fn is_active(&self, kind: EngineType, amp_env: &Envelope) -> bool {
        match kind {
            EngineType::Pizza => amp_env.is_active(),
            EngineType::Fm => !self.fm.is_idle(),
            EngineType::Modal => self.modal.is_active(),
            EngineType::Va => false,
        }
    }
}
