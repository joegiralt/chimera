//! What a Part's EQ costs the voice budget (ADR 0296 § Billing): booked on
//! the Part's first voice, as its knobs are (ADR 0112), so admission and
//! `recost` account for the EQ and SMART EQ and a full pool can't overrun
//! them. Cycles a sample, each the ORBIT build's instruction count of its
//! loop at 1.606 cycles an instruction (`tools/loop_count.py`'s factor),
//! rounded up; ADR 0296 has the counts. Bank 1's: admission runs with the
//! door open or closed (closed, the EQ plays flat and these over-bill).

use super::smart::{Bands, StillCut};
use super::smart::{PartKind, Role, SmartEq, SmartState};
use super::{PartEq, PartEqs};
use crate::hw::Cost;
use crate::project::PartId;

/// LO CUT and TILT on a Part's bus (ADR 0284: 23 instructions a sample
/// and the block's coefficients).
pub const TILT: Cost = Cost(50);
/// SMART EQ listening to a Part it cannot cut (FRONT): its four bands
/// decimated (12.7 instructions a sample), and a Part's share of the
/// block's rules, levels and dips (about 80 cycles a sample in all).
pub const LISTEN: Cost = Cost(36);
/// SMART EQ on a Part it can cut: its four bands at the full rate (42
/// instructions a sample), and its share of the block's work.
pub const CUT: Cost = Cost(84);
/// A DRUM Part's side through the bands that cut, at worst all four
/// (36.5 instructions a sample).
pub const SIDE: Cost = Cost(60);
/// SMART EQ listening to the reverb's return (FRONT): its sum's four
/// bands decimated.
pub const WASH_LISTEN: Cost = Cost(36);
/// SMART EQ on a return it can cut: its left and its right through the
/// four bands (36.5 instructions a sample each), the sum listened to.
pub const WASH_CUT: Cost = Cost(160);

/// Whether SMART EQ's bands run this block, so are billed: switched on
/// with an AMOUNT, or still letting its cuts go after (up to some
/// seconds, at the full rate until they are back).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SmartLoad {
    /// Zero bytes: ITCM's boot state.
    Rests = 0,
    Runs = 1,
}

impl SmartLoad {
    pub fn of(smart: SmartEq, st: &SmartState) -> SmartLoad {
        if smart.listens() || st.live() {
            SmartLoad::Runs
        } else {
            SmartLoad::Rests
        }
    }

    /// As `of`, from the EQ's whole state.
    pub fn now(smart: SmartEq, eqs: &PartEqs) -> SmartLoad {
        SmartLoad::of(smart, &eqs.smart)
    }
}

/// What SMART EQ's bill is made from this block: whether it runs, and
/// who still runs cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartBill {
    pub load: SmartLoad,
    pub still: StillCut,
}

impl SmartBill {
    /// At rest: zero bytes, ITCM's boot state.
    pub const RESTS: SmartBill = SmartBill {
        load: SmartLoad::Rests,
        still: StillCut::NONE,
    };

    pub fn now(smart: SmartEq, eqs: &PartEqs) -> SmartBill {
        SmartBill {
            load: SmartLoad::now(smart, eqs),
            still: StillCut::of(&eqs.smart),
        }
    }

    /// Running, nobody held over from another role: for the tests.
    pub const RUNS: SmartBill = SmartBill {
        load: SmartLoad::Runs,
        still: StillCut::NONE,
    };
}

/// What Part `p`'s EQ adds to its first voice's bill: nothing flat with
/// SMART EQ at rest, so every bill is then as before. A DRUM Part pays
/// for its side too. A Part in MID or BACK is billed as cut whether or
/// not anything outranks it: its bands run at the full rate the moment
/// something does. So is a FRONT Part whose cuts have not yet let go.
pub fn first_voice(eq: PartEq, role: Role, kind: PartKind, smart: SmartBill, p: PartId) -> Cost {
    let sides = |c: Cost| match kind {
        PartKind::Drum => c,
        PartKind::Tonal => Cost::ZERO,
    };
    let tilt = if eq.is_flat() {
        Cost::ZERO
    } else {
        TILT + sides(TILT)
    };
    let smart = match (smart.load, role, smart.still.part(p)) {
        (SmartLoad::Rests, _, _) => Cost::ZERO,
        (SmartLoad::Runs, Role::Front, Bands::Listened) => LISTEN,
        (SmartLoad::Runs, Role::Front, Bands::Full)
        | (SmartLoad::Runs, Role::Mid | Role::Back, Bands::Full | Bands::Listened) => {
            CUT + sides(SIDE)
        }
    };
    tilt + smart
}

/// What SMART EQ on the reverb's return reserves from the budget beside
/// the bus (ADR 0296), with the reverb TYPE's own extra: nothing with
/// SMART EQ at rest or the reverb off.
pub fn wash(reverb: &crate::dsp::reverb::ReverbParams, smart: SmartBill) -> Cost {
    match (smart.load, reverb.is_on(), reverb.role, smart.still.wash()) {
        (SmartLoad::Rests, _, _, _) | (_, false, _, _) => Cost::ZERO,
        (SmartLoad::Runs, true, Role::Front, Bands::Listened) => WASH_LISTEN,
        (SmartLoad::Runs, true, Role::Front, Bands::Full)
        | (SmartLoad::Runs, true, Role::Mid | Role::Back, Bands::Full | Bands::Listened) => {
            WASH_CUT
        }
    }
}
