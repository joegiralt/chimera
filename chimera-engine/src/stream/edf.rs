//! Earliest deadline first: which voice's chunk the card reads next.

use chimera_hal::units::{Millis, UiWait};

use super::ring::Rings;
use super::units::{AudioBlocks, SlotIdx};
use crate::voice_alloc::VoiceIdx;

/// A UI wait yields the bus unless a deadline is this close.
pub const URGENT_MS: Millis = Millis::new(15);
pub const URGENT_BLOCKS: u32 = AudioBlocks::blocks_in(URGENT_MS);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub voice: VoiceIdx,
    pub slot: SlotIdx,
    pub deadline: AudioBlocks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Fill(Want),
    YieldToUi,
    Idle,
}

/// The Wanted slots of every ring.
pub fn wants(rings: &Rings) -> impl Iterator<Item = Want> + '_ {
    rings.wants().map(|(voice, slot, deadline)| Want {
        voice,
        slot,
        deadline,
    })
}

/// The least `now.until(deadline)` wins (overdue is negative, so most
/// urgent), ties to the lower voice. A waiting UI gets the bus unless that
/// is within `URGENT_BLOCKS`.
///
/// Owner rule: streams always beat the UI; a slower UI while playing is
/// acceptable. A want within `URGENT_BLOCKS` is never held for a waiting
/// UI, and the UI only ever gets the bus between transfers.
pub fn pick(wants: impl Iterator<Item = Want>, now: AudioBlocks, ui: UiWait) -> Pick {
    let Some(w) = wants.min_by_key(|w| (now.until(w.deadline), w.voice.index())) else {
        return Pick::Idle;
    };
    let urgent = now.until(w.deadline) <= URGENT_BLOCKS.cast_signed();
    match ui {
        UiWait::Waiting if !urgent => Pick::YieldToUi,
        _ => Pick::Fill(w),
    }
}
