//! The VCA's low-pass gate (ADR 0195): LPG, DECAY or STRIKE above 0 hand
//! the VCA's block to Plaits' gate (`dsp::lpg`). Every loop is bank 2's
//! (`kern`, `bank2.x` links `*5voice4gate4kern*`); `GateDoor` is the entry.
//! With the door closed the VCA plays as it always has.

use super::Voice;
use crate::dsp::lpg::Drive;
use crate::hw::BLOCK_SIZE;

type Buf = [f32; BLOCK_SIZE];

/// What the gate leaves the voice: its last gain (the lifetime check's
/// fade) and whether it holds the voice.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub last: f32,
    pub holds: bool,
}

/// The kernel: the voice, its block after the DC blocker, the VCA routes'
/// gain and OUT LEVEL's ramp `(from, step)`.
pub type GateRun = fn(&mut Voice, &mut Buf, Option<&Buf>, (f32, f32)) -> Gate;

#[repr(C)]
pub struct GateDoor {
    pub run: GateRun,
}

impl GateDoor {
    pub const DOOR: GateDoor = GateDoor { run: kern::run };
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
    use super::{Buf, Drive, Gate, Voice};
    use crate::dsp::ease::at;

    /// The gate in the VCA's place: AMP's VEL on the routes and the
    /// strike, the key as the level of a key-gated note with no route, then
    /// OUT LEVEL. The voice is held while the gate is open, or as the VCA
    /// holds it unless STRIKE alone drives the gate.
    #[inline(never)]
    pub(super) fn run(v: &mut Voice, out: &mut Buf, gain: Option<&Buf>, vol: (f32, f32)) -> Gate {
        let o = v.played.out;
        let vel = 1.0 - o.vca_vel + o.vca_vel * v.last_velocity.unit();
        v.vca_velocity = v.last_velocity;
        let key_gated = v.key_gates();
        let d = Drive {
            level: gain,
            vel,
            key: key_gated.then_some(v.held),
            lpg: o.lpg,
            decay: o.decay,
            strike: o.strike,
            note: f32::from(v.last_note.get()),
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
        Gate {
            last: v.lpg.level(),
            holds: v.lpg.open() || (o.strike < 1.0 && today),
        }
    }
}
