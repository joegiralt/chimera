//! The card interrupt's decisions (slice 1 § 7.2), pure so the board only
//! runs them: the earliest Wanted slot it can read, claimed, with its
//! file's registry row held for the read (#597), else the bus goes back;
//! and what a failed read does next.

use core::sync::atomic::Ordering;

use chimera_hal::card_bus::{CardBus, CardFault, ProtoError};
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
/// Wanted (requeued) for a later wake and counted (`unreadable`). Never a
/// fault (`on_error`): the file's, not the card's, so a re-init can't mend
/// it, and its voice alone starves, fading out.
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

/// Why a fill failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The card's: CMD18's R1, a data token or CRC16, a timeout.
    Proto(ProtoError),
    /// CRC-good blocks whose records don't de-frame (`layout::deframe`).
    Frame,
}

/// A failed fill's next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorStep {
    /// The transfer once more, its claim kept.
    Retry,
    /// `CardFault::Read`: the claim requeued, nothing more starts until
    /// the UI's re-init and mount clear it.
    Fault,
    /// The fault, with no retry: the same bytes would come back. Counted
    /// in `frame_errors`.
    FrameError,
}

/// A CRC16 mismatch or an error token is read again, once; a second
/// failure, a token timeout or a bad R1 faults (C1).
pub fn on_error(err: ReadError, retried: bool) -> ErrorStep {
    match err {
        ReadError::Proto(ProtoError::Crc | ProtoError::ErrorToken(_)) if !retried => {
            ErrorStep::Retry
        }
        ReadError::Proto(_) => ErrorStep::Fault,
        ReadError::Frame => ErrorStep::FrameError,
    }
}

/// `on_error`'s step, taken: a fault set on `bus`, a frame error counted.
pub fn read_failed(err: ReadError, retried: bool, bus: &CardBus, stats: &StreamStats) -> ErrorStep {
    let step = on_error(err, retried);
    if step == ErrorStep::FrameError {
        stats.frame_errors.fetch_add(1, Ordering::Relaxed);
    }
    if step != ErrorStep::Retry {
        bus.set_fault(CardFault::Read);
    }
    step
}
