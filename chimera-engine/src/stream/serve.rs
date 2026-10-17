//! The card interrupt's decision (slice 1 § 7.2), pure so the board only
//! runs it: the earliest Wanted slot it can read, claimed, with its file's
//! registry row held for the read (#597); else the bus goes back.

use core::sync::atomic::Ordering;

use chimera_hal::units::UiWait;

use super::edf::{Pick, Want, pick, wants};
use super::registry::{RowHold, StreamRegistry};
use super::ring::{CardSide, Claim, Rings, SLOTS};
use super::stats::StreamStats;
use super::units::AudioBlocks;
use crate::hw::MAX_VOICES;

pub enum ServeStep<'r> {
    /// Read `claimed`'s chunk from `row`'s file (`StreamFile::chunk_read`
    /// holds for it); the row stays held until `row` drops.
    Read { claimed: Claim, row: RowHold<'r> },
    /// Every ring full and the UI waits: the bus goes to it.
    Yield,
    /// Nothing to read.
    Idle,
}

/// The Wanted slots whose rings name a registry row.
pub fn servable<'a>(rings: &'a Rings, reg: &'a StreamRegistry) -> impl Iterator<Item = Want> + 'a {
    wants(rings).filter(|w| {
        rings
            .source(w.voice)
            .is_some_and(|id| reg.get(id).is_some())
    })
}

/// The next read: EDF over the servable wants (owner rule: any want
/// before a waiting UI, `edf::pick`). A want cancelled since the pick, or
/// whose chunk its file can't read, is passed over, the latter left
/// Wanted (requeued) for a later wake and counted (`unreadable`).
pub fn serve_step<'r>(
    card: &mut CardSide<'_>,
    reg: &'r StreamRegistry,
    now: AudioBlocks,
    ui: UiWait,
    stats: &StreamStats,
) -> ServeStep<'r> {
    let rings = card.rings();
    let mut passed = [[false; SLOTS]; MAX_VOICES];
    loop {
        let open = servable(rings, reg).filter(|w| !passed[w.voice.index()][w.slot.index()]);
        let Pick::Fill(w) = pick(open, now) else {
            return match ui {
                UiWait::Waiting => ServeStep::Yield,
                UiWait::Idle => ServeStep::Idle,
            };
        };
        passed[w.voice.index()][w.slot.index()] = true;
        let Some(claimed) = card.claim(w.voice, w.slot) else {
            continue;
        };
        // After the claim: the source the want's note set (`Rings::source`).
        match rings.source(w.voice).and_then(|id| reg.hold(id)) {
            Some(row) if row.file().chunk_read(claimed.chunk()).is_some() => {
                return ServeStep::Read { claimed, row };
            }
            _ => {
                card.requeue(claimed);
                stats.unreadable.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
