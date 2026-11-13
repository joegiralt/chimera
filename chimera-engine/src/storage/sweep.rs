//! The lost-cluster sweep's trigger (ADR 0094): a boot sweeps only when
//! the card's mark owes it (an unclean change, fewer than `DOUBT_HALT`
//! doubting boots in a row, or a sweep asked for), and a sweep that
//! settles clears the mark. Nothing sweeps in the running synth: the
//! bitmap needs the arena's gap as it is before a project loads.

use chimera_hal::store::{
    Doubt, Mark, Store, StoreError, Sweep, SweepRun, SweepStats, Then, Trigger, VolumeId,
};

/// The boot's sweep: the mark read, then the sweep if it owes one or
/// didn't read. A clean boot costs the mount and the mark's few blocks.
#[optimize(size)]
pub fn boot_sweep(s: &mut impl Store, mode: Sweep, marks: &mut [u8]) -> SweepRun {
    let vol = match s.mount() {
        Ok(v) => v,
        Err(e) => return no_card(mode, Trigger::Unread, e),
    };
    match s.mark(vol) {
        Ok(m) if !m.owes_sweep() => SweepRun::Skipped(m),
        Ok(Mark::Asked(_)) => run(s, vol, mode, Trigger::Asked, marks),
        Ok(m) => run(s, vol, mode, Trigger::Owed(m), marks),
        Err(_) => run(s, vol, mode, Trigger::Unread, marks),
    }
}

#[optimize(size)]
fn run(s: &mut impl Store, vol: VolumeId, mode: Sweep, why: Trigger, marks: &mut [u8]) -> SweepRun {
    let seen = s.sweep(vol, mode, marks);
    let then = match &seen {
        Ok(swept) => match s.settle(vol, swept) {
            Ok(true) => Then::Cleared,
            Ok(false) => Then::Kept,
            Err(_) => Then::Failed,
        },
        // A boot's doubt on a volume it read counts toward the halt, an
        // asked sweep's puts back the count it carried; no card, a swap or
        // an unmountable one has no mark to keep it.
        Err((d, _)) if counts(*d) => match s.note_doubt(vol, why != Trigger::Asked) {
            Ok(0) => Then::Kept,
            Ok(n) => Then::Doubted(n),
            Err(_) => Then::Failed,
        },
        Err(_) => Then::Kept,
    };
    SweepRun::Ran {
        mode,
        why,
        seen,
        then,
    }
}

#[optimize(size)]
fn counts(d: Doubt) -> bool {
    !matches!(
        d,
        Doubt::Card(StoreError::NoCard | StoreError::VolumeChanged(_) | StoreError::Unsupported(_))
    )
}

#[optimize(size)]
fn no_card(mode: Sweep, why: Trigger, e: StoreError) -> SweepRun {
    SweepRun::Ran {
        mode,
        why,
        seen: Err((Doubt::Card(e), SweepStats::default())),
        then: Then::Kept,
    }
}
