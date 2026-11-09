//! LFO DEPTH's routes (ADR 0250): each LFO's DEPTH column summed over the
//! last block's source values, onto the played copy before the LFOs run.
//! The pass is bank 2's (`kern`, `bank2.x` links `*5voice5depth4kern*`);
//! `DepthDoor` is the entry. With the door closed DEPTH takes no routes.

use crate::dsp::lfo::LfoParams;
use crate::modulation::{MAX_MOD_SOURCES, ModState};

/// The pass: the routes, the sources' values, the played LFOs.
pub type DepthRun = fn(&ModState, &[f32; MAX_MOD_SOURCES], &mut [LfoParams; 3]);

#[repr(C)]
pub struct DepthDoor {
    pub run: DepthRun,
}

impl DepthDoor {
    pub const DOOR: DepthDoor = DepthDoor { run: kern::run };
}

/// `f` on the pass: through the door with `bank2` (`None` while it is
/// closed), else bank 1's own on a host; a firmware without bank 2 has none.
#[inline(always)]
pub(super) fn through<R>(f: impl FnOnce(&DepthDoor) -> R) -> Option<R> {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL.with(|d| f(&d.depth))
    }
    #[cfg(all(not(feature = "bank2"), not(target_os = "none")))]
    {
        Some(f(&DepthDoor::DOOR))
    }
    #[cfg(all(not(feature = "bank2"), target_os = "none"))]
    {
        let _ = f;
        None
    }
}

pub(super) mod kern {
    use super::{LfoParams, MAX_MOD_SOURCES, ModState};
    use crate::addr::BlockRef;

    /// DEPTH is an LFO's one destination: its sum on its Linear law, 0..1.
    /// A zero sum leaves DEPTH as stored.
    #[inline(never)]
    pub(super) fn run(m: &ModState, v: &[f32; MAX_MOD_SOURCES], lfos: &mut [LfoParams; 3]) {
        for d in 0..m.num_dests() {
            if let BlockRef::Lfo(s) = m.dest(d).block {
                let off = m.sum_for(d, v);
                if off != 0.0 {
                    let l = &mut lfos[s.index()];
                    l.depth = crate::dsp::clampf(l.depth + off, 0.0, 1.0);
                }
            }
        }
    }
}
