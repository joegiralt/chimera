//! The VCA's low-pass gate (ADR 0195): LPG, DECAY or STRIKE above 0 hand
//! the VCA's block to Plaits' gate (`dsp::lpg`). Every loop is bank 2's
//! (`kern`, `bank2.x` links `*5voice4gate4kern*`); `GateDoor` is the entry.
//! With the door closed the VCA plays as it always has.

use super::Voice;
use crate::dsp::lpg::{Drive, Key, Knobs, Level};
use crate::dsp::va::osc::Unit;
use crate::hw::{BLOCK_SIZE, Cost};
use crate::modulation::ModState;
use crate::params::ParamSnapshot;

type Buf = [f32; BLOCK_SIZE];

/// What the gate leaves the voice: its last gain (the lifetime check's
/// fade) and whether it holds the voice.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub last: Unit,
    pub holds: bool,
}

/// The kernel: the voice, its block after the DC blocker, the VCA routes'
/// gain and OUT LEVEL's ramp `(from, step)`; `None`, the block untouched,
/// while LPG, DECAY and STRIKE are all 0.
pub type GateRun = fn(&mut Voice, &mut Buf, Option<&Buf>, (f32, f32)) -> Option<Gate>;
/// The gate's bill on a Sound: `LPG_COST` while it runs.
pub type GateCost = fn(&ParamSnapshot, &ModState) -> Cost;

#[repr(C)]
pub struct GateDoor {
    pub run: GateRun,
    pub cost: GateCost,
}

impl GateDoor {
    pub const DOOR: GateDoor = GateDoor {
        run: kern::run,
        cost: kern::cost,
    };
}

/// `f` on the kernel: through the door with `bank2` (`None` while it is
/// closed), else bank 1's own on a host. A firmware built without bank 2
/// has no room for it in bank 1: there the gate stays a plain VCA.
#[inline(always)]
pub(super) fn through<R>(f: impl FnOnce(&GateDoor) -> R) -> Option<R> {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL.with(|d| f(&d.gate))
    }
    #[cfg(all(not(feature = "bank2"), not(target_os = "none")))]
    {
        Some(f(&GateDoor::DOOR))
    }
    #[cfg(all(not(feature = "bank2"), target_os = "none"))]
    {
        let _ = f;
        None
    }
}

pub(super) mod kern {
    use super::{Buf, Cost, Drive, Gate, Key, Knobs, Level, ModState, ParamSnapshot, Unit, Voice};
    use crate::addr::{BlockRef, ParamAddr};
    use crate::dsp::ease::at;
    use crate::dsp::lpg::LPG_COST;
    use crate::params::OutParams;

    /// Billed while LPG, DECAY or STRIKE is stored above 0 or routed.
    #[inline(never)]
    pub(super) fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let routed = [OutParams::LPG, OutParams::DECAY, OutParams::STRIKE]
            .iter()
            .any(|&id| mods.moves_addr(ParamAddr::new(BlockRef::Out, id)));
        if p.out.gated() || routed {
            LPG_COST
        } else {
            Cost::ZERO
        }
    }

    /// The gate in the VCA's place: AMP's VEL on the routes and the
    /// strike, the key as the level of a key-gated note with no route, then
    /// OUT LEVEL. The voice is held while the gate is open, or as the VCA
    /// holds it unless STRIKE alone drives the gate (no route, STRIKE 1).
    #[inline(never)]
    pub(super) fn run(
        v: &mut Voice,
        out: &mut Buf,
        gain: Option<&Buf>,
        vol: (f32, f32),
    ) -> Option<Gate> {
        let o = v.played.out;
        if !o.gated() {
            v.lpg.idle();
            return None;
        }
        let vel = Unit::new(1.0 - o.vca_vel + o.vca_vel * v.last_velocity.unit());
        v.vca_velocity = v.last_velocity;
        let key_gated = v.key_gates();
        let knobs = Knobs::of(o.gate);
        let level = match gain {
            Some(g) => Level::Routes(g, vel),
            None if key_gated => Level::Key(if v.held { Key::Held } else { Key::Up }),
            None => Level::Open,
        };
        let d = Drive {
            level,
            knobs,
            hit: v.last_velocity,
            note: v.last_note,
            fs: v.sample_rate as f32,
        };
        v.lpg.block(out, &d);
        for (i, x) in out.iter_mut().enumerate() {
            *x *= at(vol.0, vol.1, i);
        }
        // With no route the engine (ALGO, MODAL) or the key (in the level)
        // ends the note.
        let today = if gain.is_some() {
            v.vca_holds()
        } else {
            !key_gated
        };
        Some(Gate {
            last: v.lpg.level(),
            holds: v.lpg.open() || (!knobs.strike_alone(level.law()) && today),
        })
    }
}
