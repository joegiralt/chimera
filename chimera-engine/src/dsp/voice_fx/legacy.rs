//! Old Sounds in the slot (spec § Loading old Sounds): DRV's block 9 loads
//! as SOFT PRE and the folder's block 11 as FOLD POST. A Sound that ran
//! both keeps DRV; it needn't sound the same (no legacy compat).

use super::params::{FxType, Place, VoiceFxParams};

/// DRV's raw values by old id (DRIVE, TONE, MIX) and the folder's (FOLD,
/// SYM, MIX); `None`: the file left the neutral value out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Legacy {
    pub drive: [Option<f32>; 3],
    pub fold: [Option<f32>; 3],
}

/// The old neutral values a file's missing entries stand for.
const DRIVE_DEFAULT: [f32; 3] = [0.0, 0.5, 1.0];
const FOLD_DEFAULT: [f32; 3] = [0.0, 0.5, 0.5];

/// The old stage ran: its amount at 0.001 or more (`Voice::stage_runs`),
/// or `moved` (a route or tap on it may raise it).
fn ran(amount: f32, moved: bool) -> bool {
    amount >= 0.001 || moved
}

fn vals(raw: &[Option<f32>; 3], d: [f32; 3]) -> [f32; 3] {
    core::array::from_fn(|i| raw[i].unwrap_or(d[i]).clamp(0.0, 1.0))
}

/// The slot an old Sound's stages become, or `None` when neither ran: the
/// Sound keeps OFF. `drive_routed` and `fold_moved` come from the file's
/// routes (and the fold's tap) with a nonzero amount.
pub fn legacy_fx(l: &Legacy, drive_routed: bool, fold_moved: bool) -> Option<VoiceFxParams> {
    let [drive, tone, dmix] = vals(&l.drive, DRIVE_DEFAULT);
    let [fold, sym, fmix] = vals(&l.fold, FOLD_DEFAULT);
    let (ty, place, p1, p2, mix) = if ran(drive, drive_routed) {
        (FxType::Soft, Place::Pre, drive, tone, dmix)
    } else if ran(fold, fold_moved) {
        (FxType::Fold, Place::Post, fold, sym, fmix)
    } else {
        return None;
    };
    let mut p = VoiceFxParams::default();
    p.set_type(ty);
    (p.place, p.pots, p.mix) = (place, [p1, p2, 0.5], mix);
    Some(p)
}
