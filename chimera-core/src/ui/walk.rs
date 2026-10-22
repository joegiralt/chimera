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
use crate::dsp::modulator::law::Law;
use crate::ui::fmt::FmtBuf;
use crate::ui::page::ValFmt;
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
        // DIRECTION is WALK's: off it, LENGTH and FADE are another machine's.
        T::MODE if m == TapeMachine::Walk => {
            let writes = direction_defaults(dir(before), d);
            for &(id, v) in writes {
                b.set(id, v);
            }
            tron_fade(b);
            if writes.is_empty() {
                &[]
            } else {
                env_defaults(m, d)
            }
        }
        T::TRON => {
            tron_fade(b);
            &[]
        }
        _ => &[],
    }
}

/// TRON on: FADE at HOLD, which means nothing there, turns to TRON's 32 ms.
/// Inlined: a function of its own would link into bank 1.
#[inline(always)]
fn tron_fade(b: &mut dyn Block) {
    if b.get(T::TRON) > 0.5 && b.get(T::DRAG) >= FADE_HOLD {
        b.set(T::DRAG, fade_default(Direction::Loop));
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
        id if t.machine == TapeMachine::Stretch => {
            tape_engine::TapeEngine::stretch_only(t.kind, id)
        }
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

/// How TAPE's cell `id` reads on `m` in `way` where the engine has a law
/// the spec's format doesn't say (`view::fmt_of`): WALK's LENGTH under
/// TRON once through is the tape, 0.25 to 8 s, STRETCH's LENGTH law (ADR
/// 0203); SCRUB's LENGTH its loop, 2 ms to 2 s (ADR 0205); FADE and DRAG
/// 10 to 500 ms, FADE's top HOLD once through; DRIFT a share of the
/// window; SPEED a factor of the source's own. `None`: the spec's.
pub(crate) fn tape_fmt_in(id: ParamId, m: TapeMachine, way: Way) -> Option<ValFmt> {
    Some(match (m, id) {
        (TapeMachine::Walk, T::SIZE) if way == Way::Tron => ValFmt::Law(Law::StretchLength),
        (TapeMachine::Walk, T::DRAG) if way == Way::One => ValFmt::Hold,
        (TapeMachine::Walk | TapeMachine::Scrub, T::DRAG) => ValFmt::Law(Law::DragTime),
        (TapeMachine::Walk, T::DRIFT) => ValFmt::Law(Law::Pct),
        (TapeMachine::Scrub, T::SIZE) => ValFmt::Law(Law::ScrubLength),
        (TapeMachine::Stretch, T::MOTION) => ValFmt::Law(Law::StretchSpeed),
        (TapeMachine::Scrub, T::MOTION) => ValFmt::Law(Law::ScrubSpeed),
        _ => return None,
    })
}

/// SPEED at `v` (MOTION's 0..1 display, −1..1 stored) as the engine
/// plays it: `1.00x`, `0.50x`, `-1.00x`; STRETCH by `time_speed`
/// (0 freezes), SCRUB `2 · MOTION`.
pub(crate) fn speed_text_in(buf: &mut FmtBuf, law: Law, v: f32) {
    let motion = 2.0 * v - 1.0;
    let x = match law {
        #[cfg(feature = "tape")]
        Law::StretchSpeed => tape_engine::stretch::time_speed(motion),
        _ => 2.0 * motion,
    };
    crate::ui::fmt::fixed(buf, x, 2, "x");
}

/// What `t` with `param` (MODEL or DIRECTION) at `code` can't play from a
/// streamed source (`held::Held::why`).
pub(crate) fn tape_held_in(t: &T, param: ParamId, code: u8) -> Option<NeedsRam> {
    let mut t = *t;
    t.set(param, f32::from(code));
    needs_ram(&t)
}
