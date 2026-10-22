//! WALK's UI work (ADR 0203), bank 2's: `bank2.x` links this module there
//! and bank 1 reaches it only through the UI door (`UiDoor`, ADR 0175),
//! so a closed door writes no machine defaults, dims no TAPE cell with a
//! reason and holds no TAPE choice.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::{Block, ParamId};
use crate::dsp::delay::{DelayParams, DelayType};
use crate::dsp::modulator::law::Law;
use crate::dsp::tape_engine::machine::{Cell, defaults, direction_defaults, env_defaults, machine};
use crate::dsp::tape_engine::{
    self, Choice, Direction, FADE_HOLD, MachineIndex, NeedsRam, TapeEngineParams as T, TapeMachine,
    Way, fade_default, needs_ram,
};
use crate::modulation::VCA;
use crate::preset::Sound;
use crate::ui::fmt::FmtBuf;
use crate::ui::page::ValFmt;
use crate::ui::view::{
    DELAY_GRAIN_ONLY, DELAY_TAPE_ONLY, FADE_ASIDE, FADE_HOLD_ONLY, RETRIG_ONCE_ONLY,
    RETRIG_TRON_OFF, SourceState, WOW_AT_REST, XFADE_CAPPED, XFADE_LOOP_ONLY, XFADE_NO_SEAM,
    tape_routed,
};

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
            // SLICE is on hold (`slice_in_play`): a turn leaves none.
            b.set(T::SLICE, 0.0);
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
        // Each reason by the way it plays, so TRON on never reads as off.
        T::XFADE => match t.way() {
            Way::One => Some(XFADE_LOOP_ONLY),
            Way::Ping => Some(XFADE_NO_SEAM),
            _ => None,
        },
        T::RETRIG => match t.way() {
            Way::Loop | Way::Ping => Some(RETRIG_ONCE_ONLY),
            Way::Tron => Some(RETRIG_TRON_OFF),
            _ => None,
        },
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
        (TapeMachine::Walk, T::DRAG) if way == Way::One => ValFmt::Law(Law::FadeHold),
        (TapeMachine::Walk | TapeMachine::Scrub, T::DRAG) => ValFmt::Law(Law::DragTime),
        (TapeMachine::Walk, T::DRIFT) => ValFmt::Law(Law::Pct),
        (TapeMachine::Scrub, T::SIZE) => ValFmt::Law(Law::ScrubLength),
        (TapeMachine::Stretch, T::MOTION) => ValFmt::Law(Law::StretchSpeed),
        (TapeMachine::Scrub, T::MOTION) => ValFmt::Law(Law::ScrubSpeed),
        _ => return None,
    })
}

/// The text of a TAPE law bank 1 hands here (`fmt::fmt_law`): FADE once
/// through, its top step HOLD, else its time; SPEED at `v` (MOTION's
/// 0..1 display, −1..1 stored) as the engine plays it, `1.00x`, `0.50x`,
/// `-1.00x`: STRETCH by `time_speed` (0 freezes), SCRUB `2 · MOTION`.
pub(crate) fn law_text_in(buf: &mut FmtBuf, law: Law, v: f32) {
    use core::fmt::Write;
    let motion = 2.0 * v - 1.0;
    let x = match law {
        Law::FadeHold if v >= 0.999 => {
            let _ = buf.write_str("HOLD");
            return;
        }
        Law::FadeHold => {
            return crate::ui::fmt::fmt_val(buf, v, ValFmt::Law(Law::DragTime));
        }
        #[cfg(feature = "tape")]
        Law::StretchSpeed => tape_engine::stretch::time_speed(motion),
        _ => 2.0 * motion,
    };
    crate::ui::fmt::fixed(buf, x, 2, "x");
}

/// Why the delay's cell `id` is dimmed under TYPE `k` (ADR 0202): DLY ›
/// CHAR's MECHANICS and SAT are TAPE's, DLY › GRAIN's four GRAIN's; TIME,
/// FDBK, TONE, REV and MIX are both TYPEs'.
pub(crate) fn delay_note_in(id: ParamId, k: DelayType) -> Option<&'static str> {
    type D = DelayParams;
    match (k, id) {
        (DelayType::Grain, D::WOW_FLUTTER | D::SATURATION) => Some(DELAY_TAPE_ONLY),
        (DelayType::Tape, D::SIZE | D::DENSITY | D::PITCH | D::TEXTURE) => Some(DELAY_GRAIN_ONLY),
        _ => None,
    }
}

/// The focus readout's word for TAPE's `a` at display value `v` (or the
/// held choice `held`) where its cell abbreviates it (full-word labels):
/// MODEL's machine, DIRECTION's way. `None`: the cell's text.
pub(crate) fn readout_word_in(a: ParamAddr, v: f32, held: Option<u8>) -> Option<&'static str> {
    if a.block != BlockRef::TapeEngine {
        return None;
    }
    let names: &[&str] = match a.param {
        T::MODEL => &tape_engine::MACHINE_NAMES,
        T::MODE => &tape_engine::params::DIRECTION_WORDS,
        _ => return None,
    };
    let last = names.len() as u8 - 1;
    let i = match held {
        Some(h) => h,
        None => crate::ui::fmt::discrete(v, last),
    };
    names.get(usize::from(i)).copied()
}

/// The knob writes (`Motion::knob`, glided by the Part's knobs) an edit
/// of TAPE's `param` to `code` on `t` makes besides its own
/// (`machine_edit_in`): MODEL its machine's defaults in `t`'s DIRECTION
/// and their ENV 1; DIRECTION, crossing once through and a loop, LENGTH
/// and FADE and then ENV 1's. A CC's budget (`cc_bin`) and the hand's.
pub(crate) fn tape_writes_in(param: ParamId, code: u8, t: &T) -> u8 {
    let env1 = BlockRef::Env(crate::dsp::modulator::EnvSlot::Env1);
    match param {
        T::MODEL => {
            let m = TapeMachine::from_index(MachineIndex(code));
            // `defaults`' cells, by id alone: no iterator's instance, which
            // would be core's and link into bank 1.
            let k = machine(m);
            cell_knobs(k.deck) + cell_knobs(k.cells) + knobs(env1, env_defaults(m, t.direction))
        }
        T::MODE => {
            let Some(now) = Direction::from_value(f32::from(code)) else {
                return 0;
            };
            let writes = direction_defaults(t.direction, now);
            if writes.is_empty() {
                0
            } else {
                knobs(BlockRef::TapeEngine, writes)
                    + knobs(env1, env_defaults(TapeMachine::Walk, now))
            }
        }
        _ => 0,
    }
}

/// How many of a machine's `cells` a MODEL turn writes (`defaults`: all
/// but the window and DIRECTION) are knobs.
#[inline(always)]
fn cell_knobs(c: &[Cell]) -> u8 {
    let mut n = 0;
    let mut i = 0;
    while i < c.len() {
        let id = c[i].id;
        n += u8::from(!matches!(id, T::START | T::END | T::MODE) && knob(BlockRef::TapeEngine, id));
        i += 1;
    }
    n
}

/// `b`'s param `id` is glided by the Part's knobs (`Motion::knob`).
#[inline(always)]
fn knob(b: BlockRef, id: ParamId) -> bool {
    ParamAddr::new(b, id)
        .spec()
        .is_some_and(|s| s.motion().knob().is_some())
}

/// How many of `writes` on `b` are knobs. Inlined, a plain loop: bank 2's
/// with its caller.
#[inline(always)]
fn knobs(b: BlockRef, writes: &[(ParamId, f32)]) -> u8 {
    let mut n = 0;
    let mut i = 0;
    while i < writes.len() {
        n += u8::from(knob(b, writes[i].0));
        i += 1;
    }
    n
}

/// What the focus band says in its hint's place about a TAPE cell: why
/// it is dimmed (`tape_note`), or a streamed LOOP's XFADE past the 100 ms
/// its loop head holds (`view::band_note`).
pub(crate) fn band_note_in(
    addr: ParamAddr,
    sound: &Sound,
    src: SourceState,
) -> Option<&'static str> {
    if let Some(n) = tape_note_in(addr, sound) {
        return Some(n);
    }
    // ONE's FADE stays live, its HOLD the gate; below HOLD AMP releases.
    let t = &sound.params.tape;
    if addr == ParamAddr::new(BlockRef::TapeEngine, T::DRAG)
        && t.way() == Way::One
        && t.drag < crate::dsp::tape_engine::FADE_HOLD
        && sound.mod_state.routes_into(VCA) != 0
    {
        return Some(FADE_HOLD_ONLY);
    }
    let SourceState::Held {
        rate,
        frames,
        notes: Some(_),
        ..
    } = src
    else {
        return None;
    };
    if addr != ParamAddr::new(BlockRef::TapeEngine, T::XFADE) || t.way() != Way::Loop {
        return None;
    }
    let file = core::num::NonZeroU32::new(frames).filter(|f| f.get() >= 2)?;
    use crate::dsp::tape_engine::window::{loop_len, window_end, window_start};
    let ws = window_start(t.start, file);
    let we = window_end(ws, t.end, frames, rate);
    let asked = t.xfade * loop_len(t.size, we - ws, rate) as f32;
    let cap = u32::from(rate) * crate::stream::stage::XFADE_MAX_MS / 1_000;
    (asked > cap as f32).then_some(XFADE_CAPPED)
}

/// What `t` with `param` (MODEL or DIRECTION) at `code` can't play from a
/// streamed source (`held::Held::why`).
pub(crate) fn tape_held_in(t: &T, param: ParamId, code: u8) -> Option<NeedsRam> {
    let mut t = *t;
    t.set(param, f32::from(code));
    needs_ram(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A MODEL change's budget is its defaults' knobs and its ENV 1's, on
    /// every machine and DIRECTION.
    #[test]
    fn model_writes_are_the_defaults() {
        let env1 = BlockRef::Env(crate::dsp::modulator::EnvSlot::Env1);
        for m in TapeMachine::ALL {
            for d in Direction::ALL {
                let t = T {
                    direction: d,
                    ..T::default()
                };
                let want = defaults(m, d)
                    .filter(|&(id, _)| knob(BlockRef::TapeEngine, id))
                    .count()
                    + env_defaults(m, d)
                        .iter()
                        .filter(|&&(id, _)| knob(env1, id))
                        .count();
                assert_eq!(usize::from(tape_writes_in(T::MODEL, m.index(), &t)), want);
            }
        }
    }

    /// A DIRECTION change's budget is the knob writes the edit makes, from
    /// every DIRECTION to every other (review of #761, m2).
    #[test]
    fn direction_writes_are_the_edits() {
        let env1 = BlockRef::Env(crate::dsp::modulator::EnvSlot::Env1);
        let knob = |a: ParamAddr| a.spec().and_then(|s| s.motion().knob()).is_some();
        for was in Direction::ALL {
            for now in Direction::ALL {
                let mut s = Sound::init(crate::params::EngineType::Tape);
                s.params.tape.direction = was;
                // Unlike every default, so each write moves its knob.
                (s.params.tape.size, s.params.tape.drag) = (0.123, 0.456);
                let before = s.params.tape;
                let want = tape_writes_in(T::MODE, now as u8, &before);
                s.params.tape.set(T::MODE, now as u8 as f32);
                let addr = ParamAddr::new(BlockRef::TapeEngine, T::MODE);
                let envs = machine_edit_in(addr, f32::from(was as u8), &mut s.params.tape);
                let tape = crate::dsp::tape_engine::TAPE_ENGINE_SPECS
                    .iter()
                    .filter(|sp| sp.id != T::MODE)
                    .filter(|sp| s.params.tape.get(sp.id) != before.get(sp.id))
                    .filter(|sp| knob(ParamAddr::new(BlockRef::TapeEngine, sp.id)))
                    .count();
                let env = envs
                    .iter()
                    .filter(|&&(id, _)| knob(ParamAddr::new(env1, id)))
                    .count();
                assert_eq!(usize::from(want), tape + env, "{was:?} to {now:?}");
            }
        }
    }
}
