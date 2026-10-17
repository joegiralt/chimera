//! What a streamed voice's cursor reaches in one block.

use chimera_hal::card_bus::CardBus;

use super::ring::AudioTurn;
use super::stats::StreamStats;
use super::units::{AudioBlocks, Frames};
use crate::voice_alloc::VoiceIdx;

/// Built a block, a voice: it borrows the block's `AudioTurn`, so no
/// `Ready` outlives the block.
pub struct StreamPort<'a, 's> {
    pub turn: &'a mut AudioTurn<'s>,
    pub voice: VoiceIdx,
    /// The streamed file's frames.
    pub file_frames: Frames,
    pub now: AudioBlocks,
    pub bus: &'a CardBus,
    pub stats: &'a StreamStats,
    /// Pends the card IRQ (`DMA2_STR0`), after every new want, whoever
    /// owns the bus.
    pub pend: fn(),
}
