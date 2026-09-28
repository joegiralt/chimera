//! A page slot as it reads now (filter-routing spec § UI).

use crate::addr::{BlockRef, ParamAddr};
use crate::modulation::VCA;
use crate::params::OutParams;
use crate::preset::Sound;

/// A fixed or inapplicable slot draws dimmed, and its encoder is ignored
/// (spec § UI "Dimmed").
pub fn dimmed(addr: ParamAddr, sound: &Sound) -> bool {
    match (addr.block, addr.param) {
        // AMP's VEL under the Algo/Modal pass-through (spec § 5).
        (BlockRef::Out, OutParams::VCA_VEL) => sound.mod_state.routes_into(VCA) == 0,
        _ => false,
    }
}
