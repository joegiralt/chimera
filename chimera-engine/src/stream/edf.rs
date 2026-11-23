// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Earliest deadline first: which voice's chunk the card reads next.

use super::ring::Rings;
use super::units::{AudioBlocks, SlotIdx};
use crate::voice_alloc::VoiceIdx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub voice: VoiceIdx,
    pub slot: SlotIdx,
    pub deadline: AudioBlocks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Fill(Want),
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
/// urgent), ties to the lower voice.
///
/// Owner rule: streams always beat the UI; a slower UI while playing is
/// acceptable. Any want beats a waiting UI: the UI gets the bus only when
/// no stream wants a fill (`Idle`), so it waits at most until every ring
/// is full.
pub fn pick(wants: impl Iterator<Item = Want>, now: AudioBlocks) -> Pick {
    match wants.min_by_key(|w| (now.until(w.deadline), w.voice.index())) {
        Some(w) => Pick::Fill(w),
        None => Pick::Idle,
    }
}
