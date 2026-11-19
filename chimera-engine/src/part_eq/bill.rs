//! What a Part's EQ costs the voice budget (ADR 0296 § Billing): booked on
//! the Part's first voice, as its knobs are (ADR 0112), so admission and
//! `recost` account for the EQ and SMART EQ and a full pool can't overrun
//! them. Cycles a sample, each the ORBIT build's instruction count of its
//! loop at 1.606 cycles an instruction (`tools/loop_count.py`'s factor),
//! rounded up; ADR 0296 has the counts. Bank 1's: admission runs with the
//! door open or closed (closed, the EQ plays flat and these over-bill).

use super::PartEq;
use super::smart::{PartKind, Role, SmartEq};
use crate::hw::Cost;

/// LO CUT and TILT on a Part's bus (ADR 0284: 23 instructions a sample
/// and the block's coefficients).
pub const TILT: Cost = Cost(50);
/// SMART EQ listening to a Part it cannot cut (FRONT): its four bands
/// decimated, the rule's share.
pub const LISTEN: Cost = Cost(40);
/// SMART EQ on a Part it can cut: its four bands at the full rate, the
/// dips, the rule's share.
pub const CUT: Cost = Cost(115);
/// A DRUM Part's side through the bands that cut, at worst all four.
pub const SIDE: Cost = Cost(85);

/// What a Part's EQ adds to its first voice's bill: nothing flat with
/// SMART EQ off (or at AMOUNT 0), so every bill is then as before. A DRUM
/// Part pays for its side too. A Part in MID or BACK is billed as cut
/// whether or not anything outranks it: its bands run at the full rate
/// the moment something does.
pub fn first_voice(eq: PartEq, role: Role, kind: PartKind, smart: SmartEq) -> Cost {
    let sides = |c: Cost| match kind {
        PartKind::Drum => c,
        PartKind::Tonal => Cost::ZERO,
    };
    let tilt = if eq.is_flat() {
        Cost::ZERO
    } else {
        TILT + sides(TILT)
    };
    let smart = match (smart.listens(), role) {
        (false, _) => Cost::ZERO,
        (true, Role::Front) => LISTEN,
        (true, Role::Mid | Role::Back) => CUT + sides(SIDE),
    };
    tilt + smart
}
