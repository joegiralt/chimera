//! The built-in layer's relative CCs and resets (ADR 0263): CC 73 and 72
//! shift the Sound's own envelope times RP-021's way, 64 the Sound as it
//! is; Reset All Controllers brings expression back.
//!
//! Bank 1's (ADR 0251: bank 2 is under its floor), called only from bank
//! 2's `ui::midi`; each entry `inline(never)` so it stays here, and it
//! calls nothing of bank 2's.

use chimera_midi::value::U7;

use super::UiState;
use super::part_page::Edit;
use crate::MidiChannel;
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::dsp::algo::params::AlgoOpParams;
use crate::project::PartId;

impl UiState {
    /// CC 73 (`which` 0) or 72 (1) at `v` on Part `p`: its offset from 64,
    /// less what the Part already applied, moves `at` (an ALGO operator's:
    /// every operator's) from its own value, so a patch's shape survives
    /// and 64 gives it back (bar clamping). A continuous target moves 1/127
    /// of its range a step; ALGO's rates step by rounding, turned round (a
    /// higher rate is a shorter time), AR never 0. Writes through the edit
    /// path; DAMP, a knob, waits for room.
    #[inline(never)]
    pub(crate) fn cc_shift(&mut self, p: PartId, which: u8, at: ParamAddr, v: U7) {
        let which = usize::from(which);
        let to = i16::from(v.get()) - 64;
        let from = i16::from(self.project.part(p).env_offset(which));
        if to == from {
            return;
        }
        let i = p.index();
        let algo = matches!(at.block, BlockRef::AlgoOp(_));
        let Some(spec) = at.spec() else {
            return;
        };
        for op in Op::ALL {
            let a = if algo {
                ParamAddr::new(BlockRef::AlgoOp(op), at.param)
            } else if op == Op::A {
                at
            } else {
                break;
            };
            let Some(was) = self
                .project
                .read_part(p)
                .block(a.block)
                .map(|b| b.get(a.param))
            else {
                continue;
            };
            let range = spec.max - spec.min;
            let new = if algo {
                // Rounded half away from 0, so +o and -o step alike.
                let r = range as i16;
                let steps = |o: i16| f32::from((o * r + (o * r).signum() * 63) / 127);
                let lo = if a.param == AlgoOpParams::AR {
                    1.0
                } else {
                    spec.min
                };
                crate::dsp::clampf(was - (steps(to) - steps(from)), lo, spec.max)
            } else {
                let d = f32::from(to - from) * range / 127.0;
                crate::dsp::clampf(was + d, spec.min, spec.max)
            };
            if spec.motion().knob().is_some() {
                if self.cc.room[i] == 0 {
                    return;
                }
                self.cc.room[i] -= 1;
            }
            self.edit_in(p, a, Edit::To(new));
        }
        self.project.set_env_offset(p, which, to as i8);
    }

    /// Reset All Controllers on `ch`: every Part there plays at full
    /// expression (GM), and its ATTACK and RELEASE shifts go back to 64,
    /// the Sound as saved.
    #[inline(never)]
    pub(crate) fn cc_reset(&mut self, ch: MidiChannel) {
        for p in PartId::ALL {
            let part = self.project.part(p);
            let (on, full, e) = (
                part.mix.channel == ch,
                part.expression() == 127,
                part.sound.params.engine(),
            );
            if !on {
                continue;
            }
            if !full {
                self.project.set_expression(p, 127);
            }
            for which in 0..2 {
                if let Some(at) = crate::midi_map::builtin::shift_target(e, which) {
                    self.cc_shift(p, which, at, U7::new(64).unwrap_or(U7::MAX));
                }
            }
        }
    }
}
