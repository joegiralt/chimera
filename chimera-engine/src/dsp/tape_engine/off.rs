//! TAPE without the `tape` feature (ADR 0132): a silent engine with the
//! real one's surface, for the bench builds without bank 2. A project's
//! TAPE Part loads as TAPE (ADR 0141) and plays nothing here: a note on it
//! is refused, never allocated.

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::hw::Cost;
use crate::modulation::ModState;
use crate::project::SourceId;
use crate::source_table::GateView;
use crate::stream::port::StreamPort;

use super::{Heads, PlayMode, TapeEngineParams, TapeModel};

#[derive(Clone, Copy)]
pub struct TapeEngine;

/// No strip of tape to keep.
#[derive(Clone, Copy)]
pub(crate) struct Strip;

impl TapeEngine {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(TapeEngine)
    }

    pub fn seed(&mut self, _: u32) {}

    pub(crate) fn strip(&self) -> Strip {
        Strip
    }

    pub(crate) fn restore(&mut self, _: Strip) {}

    pub fn latch(&mut self, _: SourceId) {}

    pub fn unlatch(&mut self) {}

    pub fn source(&self) -> Option<SourceId> {
        None
    }

    pub fn start(
        &mut self,
        _: u8,
        _: &TapeEngineParams,
        _: &GateView<'_>,
        _: u32,
        _: Option<&mut StreamPort<'_, '_>>,
    ) {
    }

    /// Every note: nothing here plays.
    pub fn refuses(_: &TapeEngineParams, _: Option<SourceId>, _: &GateView<'_>) -> bool {
        true
    }

    pub fn play(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        _: &TapeEngineParams,
        _: &GateView<'_>,
        _: u32,
        _: Option<&mut StreamPort<'_, '_>>,
    ) {
        out.fill(0.0);
    }

    pub fn cost(_: &TapeEngineParams, _: &ModState) -> Cost {
        Cost(0)
    }

    pub const fn model_cost(_: TapeModel, _: PlayMode) -> Cost {
        Cost(0)
    }

    pub fn playing_cost(&self) -> Option<Cost> {
        None
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        1.0
    }

    pub fn set_pitch(&mut self, _: f32) {}

    pub fn glide_to(&mut self, _: u8, _: (u32, f32)) {}

    pub fn note_off(&mut self) {}

    pub fn sustains(&self) -> bool {
        false
    }

    pub fn hold(&mut self) {}

    pub fn is_active(&self) -> bool {
        false
    }

    pub fn heads(&self) -> Heads {
        Heads::NONE
    }
}
