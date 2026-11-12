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
    /// CC 73 (`which` 0) or 72 (1) at `v` on Part `p`: `at` (an ALGO
    /// operator's: every operator's) is the Sound's own value shifted by
    /// the offset from 64, clamped, so a patch's shape survives and 64
    /// gives it back exactly, whatever clamped on the way. A continuous
    /// target moves 1/127 of its range a step; ALGO's rates step by
    /// rounding, turned round (a higher rate is a shorter time), AR never
    /// 0. Writes through the edit path; DAMP, a knob, waits for room.
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
        let range = spec.max - spec.min;
        let lo = if algo && at.param == AlgoOpParams::AR {
            1.0
        } else {
            spec.min
        };
        // Offset `o`'s move: ALGO's rounded half away from 0, so +o and
        // -o step alike.
        let delta = |o: i16| {
            if algo {
                let r = range as i16;
                -f32::from((o * r + (o * r).signum() * 63) / 127)
            } else {
                f32::from(o) * range / 127.0
            }
        };
        // The Sound's `b` shifted by `o`: itself at 64.
        let shifted = |b: f32, o: i16| {
            if o == 0 {
                b
            } else {
                crate::dsp::clampf(b + delta(o), lo, spec.max)
            }
        };
        let saved = base::get(i, which);
        let mut keep = 0u32;
        for (k, op) in Op::ALL.into_iter().enumerate() {
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
            // The Sound's own value: the one kept, unless the cell isn't
            // what it shifts to (an edit since), then the cell's unshifted.
            let mine = if algo {
                f32::from((saved >> (5 * k)) as u8 & 31)
            } else {
                f32::from_bits(saved)
            };
            let b = if shifted(mine, from) == was {
                mine
            } else {
                crate::dsp::clampf(was - delta(from), spec.min, spec.max)
            };
            keep |= if algo {
                (b as u32) << (5 * k)
            } else {
                b.to_bits()
            };
            if spec.motion().knob().is_some() {
                if self.cc.room[i] == 0 {
                    return;
                }
                self.cc.room[i] -= 1;
            }
            self.edit_in(p, a, Edit::To(shifted(b, to)));
        }
        base::set(i, which, keep);
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

/// Each Part's Sound's own values under its CC 73 and 72 shifts: a word
/// a shift, ALGO's six 5-bit rates or the cell's `f32`. 48 B of AXI
/// `.bss`, not the Part's (D2 is full); a host's per thread, as each test
/// has its own `UiState`. Kept only while the shift is off 64: at 64 the
/// cell is the Sound, so a save or load (offset 0) needs no reset.
mod base {
    use crate::hw::MAX_PARTS;
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

    type Words = [[AtomicU32; 2]; MAX_PARTS];

    #[cfg(not(feature = "test-support"))]
    fn with<R>(f: impl FnOnce(&Words) -> R) -> R {
        static WORDS: Words = [const { [const { AtomicU32::new(0) }; 2] }; MAX_PARTS];
        f(&WORDS)
    }

    #[cfg(feature = "test-support")]
    fn with<R>(f: impl FnOnce(&Words) -> R) -> R {
        extern crate std;
        std::thread_local! {
            static WORDS: Words = const { [const { [const { AtomicU32::new(0) }; 2] }; MAX_PARTS] };
        }
        WORDS.with(f)
    }

    pub(super) fn get(p: usize, which: usize) -> u32 {
        with(|w| w[p][which & 1].load(Relaxed))
    }

    pub(super) fn set(p: usize, which: usize, v: u32) {
        with(|w| w[p][which & 1].store(v, Relaxed));
    }
}
