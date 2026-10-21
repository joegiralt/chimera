//! WALK's UI work (ADR 0203), bank 2's: `bank2.x` links this module there
//! and bank 1 reaches it only through the UI door (`UiDoor`, ADR 0175),
//! so a closed door writes no machine defaults, dims no TAPE cell with a
//! reason and holds no TAPE choice.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::{Block, ParamId};
use crate::dsp::tape_engine::machine::{defaults, direction_defaults, env_defaults};
use crate::dsp::tape_engine::{
    self, Choice, Direction, FADE_HOLD, MachineIndex, NeedsRam, TapeEngineParams as T, TapeMachine,
    Way, fade_default, needs_ram,
};
use crate::modulation::VCA;
use crate::preset::Sound;
use crate::ui::view::{FADE_ASIDE, RETRIG_ONCE_ONLY, WOW_AT_REST, XFADE_LOOP_ONLY, tape_routed};

/// A MODEL edit on TAPE writes the new machine's defaults (spec § 2
/// Machines, ADR 0178), and a DIRECTION edit across once through and a
/// loop WALK's LENGTH and FADE for it (ADR 0203), as a MODEL turn between
/// ONE and LOOP did; a load never does. TRON on turns FADE at HOLD into
/// TRON's 32 ms. Returns the ENV 1 defaults (`env_defaults`) the caller
/// writes.
pub(crate) fn machine_edit_in(
    a: ParamAddr,
    before: f32,
    b: &mut dyn Block,
) -> &'static [(ParamId, f32)] {
    if a.block != BlockRef::TapeEngine || b.get(a.param) == before {
        return &[];
    }
    let dir = |v: f32| Direction::from_value(v).unwrap_or(Direction::Loop);
    let d = dir(b.get(T::MODE));
    let m = TapeMachine::from_index(MachineIndex(b.get(T::MODEL) as u8));
    match a.param {
        T::MODEL => {
            for (id, v) in defaults(m, d) {
                b.set(id, v);
            }
            env_defaults(m, d)
        }
        T::MODE => {
            let writes = direction_defaults(dir(before), d);
            for &(id, v) in writes {
                b.set(id, v);
            }
            if writes.is_empty() {
                &[]
            } else {
                env_defaults(m, d)
            }
        }
        T::TRON if b.get(T::TRON) > 0.5 && b.get(T::DRAG) >= FADE_HOLD => {
            b.set(T::DRAG, fade_default(Direction::Loop));
            &[]
        }
        _ => &[],
    }
}

/// Why a TAPE cell is dimmed though its machine reads it (`view::tape_note`).
pub(crate) fn tape_note_in(addr: ParamAddr, sound: &Sound) -> Option<&'static str> {
    if addr.block != BlockRef::TapeEngine {
        return None;
    }
    let t = &sound.params.tape;
    match addr.param {
        T::WOW if !tape_engine::tape_runs(t.tape, tape_routed(sound)) => Some(WOW_AT_REST),
        T::DRAG
            if matches!(t.way(), Way::Loop | Way::Ping)
                && !t.tron_on()
                && sound.mod_state.routes_into(VCA) != 0 =>
        {
            Some(FADE_ASIDE)
        }
        T::XFADE if t.machine == TapeMachine::Walk && !matches!(t.way(), Way::Loop | Way::Tron) => {
            Some(XFADE_LOOP_ONLY)
        }
        T::RETRIG if t.machine == TapeMachine::Walk && t.way() != Way::One => {
            Some(RETRIG_ONCE_ONLY)
        }
        _ => None,
    }
}

/// What `t` with `param` (MODEL or DIRECTION) at `code` can't play from a
/// streamed source (`held::Held::why`).
pub(crate) fn tape_held_in(t: &T, param: ParamId, code: u8) -> Option<NeedsRam> {
    let mut t = *t;
    t.set(param, f32::from(code));
    needs_ram(&t)
}
