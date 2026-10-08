//! The lost-cluster sweep's trigger (ADR 0094): a boot sweeps only when
//! the card's mark owes it, the console's `sweep` whenever asked, and a
//! sweep that settles clears the mark.

use chimera_hal::store::{Doubt, Store, Sweep, SweepRun, SweepStats, Trigger, VolumeId};

/// The boot's sweep: the mark read, then the sweep if it owes one or
/// didn't read. A clean boot costs the mount and the mark's few blocks.
pub fn boot_sweep(s: &mut impl Store, mode: Sweep, marks: &mut [u8]) -> SweepRun {
    let vol = match s.mount() {
        Ok(v) => v,
        Err(e) => return no_card(mode, Trigger::Unread, e),
    };
    match s.mark(vol) {
        Ok(m) if !m.owes_sweep() => SweepRun::Skipped(m),
        Ok(m) => run(s, vol, mode, Trigger::Owed(m), marks),
        Err(_) => run(s, vol, mode, Trigger::Unread, marks),
    }
}

/// The console's `sweep`: run now, whatever the mark says.
pub fn asked_sweep(s: &mut impl Store, mode: Sweep, marks: &mut [u8]) -> SweepRun {
    match s.mount() {
        Ok(vol) => run(s, vol, mode, Trigger::Asked, marks),
        Err(e) => no_card(mode, Trigger::Asked, e),
    }
}

fn run(s: &mut impl Store, vol: VolumeId, mode: Sweep, why: Trigger, marks: &mut [u8]) -> SweepRun {
    let seen = s.sweep(vol, mode, marks);
    let cleared = match &seen {
        Ok(swept) => s.settle(vol, swept),
        Err(_) => Ok(false),
    };
    SweepRun::Ran {
        mode,
        why,
        seen,
        cleared,
    }
}

fn no_card(mode: Sweep, why: Trigger, e: chimera_hal::store::StoreError) -> SweepRun {
    SweepRun::Ran {
        mode,
        why,
        seen: Err((Doubt::Card(e), SweepStats::default())),
        cleared: Ok(false),
    }
}
