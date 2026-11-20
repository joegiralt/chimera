//! The delay's tempo sync (ADR 0302): under SYNC, TIME is a note of the
//! tempo, for TAPE's head and GRAIN's repeat alike. The note is folded
//! onto the 500 ms line by halves, never clamped off the grid; a small
//! tempo change glides TAPE's head as tape does, anything else crossfades
//! as a TIME turn always has.
//!
//! With `bank2` this module links into bank 2's S4 (`bank2.x`,
//! `*5delay4sync*`); `tape::run` and `grain::run` reach
//! it. The notes, the tempo and the fold's state and marks are the
//! reverb's (`rev::sync`).

use super::tape::span;
use super::{
    DelayNote, DelayParams, DelaySync, Eased, MAX_DELAY_SAMPLES, TIME_FADE, Tap, TapeDelay,
};
use crate::dsp::clampf;
use crate::dsp::rev::sync::{Division, FoldState, GATE_LENGTH, HYST, Tempo};
use chimera_hal::BLOCK_SIZE;

/// The notes TIME walks: the reverb's list itself. No copy: the linker
/// folds an identical one into this module's rodata, bank 2's, and the
/// reverb's bank-1 code would then read it there.
pub const NOTES: &[Division] = GATE_LENGTH;

/// TIME's range, ms: the line's (ADR 0014).
pub const SHORTEST_MS: f32 = 10.0;
pub const LONGEST_MS: f32 = 500.0;
/// A note the tempo makes exactly the line's length fits, whatever `f32`
/// makes of a triplet's 2/3: the limits give this much, ms.
const GIVE_MS: f32 = 0.01;

/// The most a tempo change bends TAPE's pitch: the head's speed against
/// the tape's, 2 % (34 cents).
pub const GLIDE_SPEED: f32 = 0.02;
/// The longest glide, seconds: a move that would take longer crossfades.
pub const GLIDE_MAX_S: f32 = 0.25;

/// A note of `len` ms halved or doubled onto the line, staying on the
/// grid: it leaves at a limit and comes back only 2 % inside it (`HYST`),
/// so a clock at the boundary never flips it, and it is never held to a
/// limit, which would drift off the grid a little more each repeat.
/// `st` counts the halvings (doublings, negative).
pub fn fit(len: f32, st: &mut FoldState) -> f32 {
    let at = |n: i8| libm::ldexpf(len, -i32::from(n));
    let (lo, hi) = (SHORTEST_MS - GIVE_MS, LONGEST_MS + GIVE_MS);
    let mut n = st.0;
    if len > 0.0 && len.is_finite() {
        // Each pass moves one way only: a dozen settle any tempo.
        for _ in 0..16 {
            let halve = at(n) > hi || n < 0 && at(n + 1) >= lo * HYST;
            let double = at(n) < lo || n > 0 && at(n - 1) <= hi / HYST;
            n = match (halve, double) {
                (true, _) => n + 1,
                (_, true) => n - 1,
                _ => break,
            };
        }
    } else {
        n = 0;
    }
    st.0 = n;
    at(n)
}

/// What a block's TIME was read from, beside the knob and the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aim {
    sync: DelaySync,
    note: DelayNote,
}

impl Aim {
    /// Zero bytes: FREE.
    pub const NEW: Self = Self {
        sync: DelaySync::Free,
        note: DelayNote::new(0),
    };

    pub fn of(p: &DelayParams) -> Self {
        Self {
            sync: p.sync(),
            note: p.note(),
        }
    }
}

/// Why a block's TIME differs from the last's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The tempo alone moved under the same note, folded the same.
    Tempo,
    /// Anything else: the knob, the note, SYNC, a fold.
    Step,
}

/// One block of TAPE's glide: the head's speed off the tape's (samples
/// of delay a sample) ramps from `from` to `to`, and the block ends with
/// the head at `lands`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    from: f32,
    to: f32,
    lands: f32,
}

impl Slide {
    /// No glide: what `tape::span` takes where the head stays.
    pub(super) const STILL: Slide = Slide {
        from: 0.0,
        to: 0.0,
        lands: 0.0,
    };

    /// The head's speed on the block's first sample's side, and half its
    /// change a sample: sample `n` (from 1) reads `(from + bend · n) · n`
    /// off where the block began.
    #[inline(always)]
    pub(super) fn curve(&self) -> (f32, f32) {
        (self.from, (self.to - self.from) / (2 * BLOCK_SIZE) as f32)
    }
}

/// How fast the head closes on its time: a block covers this share of
/// what is left, so it slows in as a transport does.
const GLIDE_SHARE: f32 = 0.25;
/// Within this of its time, samples, the head's next block ramps onto it
/// exactly and rests.
const GLIDE_NEAR: f32 = 0.05;

/// The block's transport constants with TIME as `p` and `tempo` give it
/// (FREE: the knob's ms, as ever), and why it moved. Keeps the fold
/// while the note stays: a note turned to is fitted from rest, so the
/// same note at the same tempo plays the same however it was reached,
/// and the hysteresis spans tempo moves alone.
#[inline(never)]
pub fn tap(d: &mut TapeDelay, p: &DelayParams, tempo: Tempo, fs: u32) -> (Tap, Cause) {
    let mut tap = Tap::new(p, fs);
    let aim = Aim::of(p);
    let was = (d.aim, d.fold);
    if aim != d.aim {
        d.fold = FoldState(0);
    }
    match aim.sync {
        DelaySync::Free => d.fold = FoldState(0),
        DelaySync::Sync => {
            let ms = fit(aim.note.division().ms(tempo), &mut d.fold);
            tap.base = clampf(ms * fs as f32 / 1000.0, 1.0, (MAX_DELAY_SAMPLES - 2) as f32);
        }
    }
    let cause = if aim.sync == DelaySync::Sync && was == (aim, d.fold) {
        Cause::Tempo
    } else {
        Cause::Step
    };
    d.aim = aim;
    (tap, cause)
}

/// TAPE's block: `tap`, and its head set toward the new TIME. A tempo
/// change the head can make within `GLIDE_MAX_S` at `GLIDE_SPEED` glides
/// (the `Slide` to play this block), its speed ramped a block at a time
/// and slowing in; any other starts the 20 ms crossfade once the running
/// one ends, as a TIME turn always has.
#[inline(never)]
pub fn aim(d: &mut TapeDelay, p: &DelayParams, tempo: Tempo, fs: u32) -> (Tap, Option<Slide>) {
    let (tap, cause) = tap(d, p, tempo, fs);
    if !d.primed {
        (d.time, d.primed) = (tap.base(), true);
    }
    // At rest on its time: nothing to do. A head on its time but still
    // moving goes on to stop.
    if d.fade != 0 || (tap.base() == d.time && d.speed == 0.0) {
        return (tap, None);
    }
    let far = tap.base() - d.time;
    if cause == Cause::Tempo && far.abs() <= GLIDE_SPEED * GLIDE_MAX_S * fs as f32 {
        let n = BLOCK_SIZE as f32;
        // Toward the time at a share of what is left a block, 2 % at most;
        // the speed itself ramps there across the block.
        let from = d.speed;
        let slide = if far.abs() < GLIDE_NEAR {
            // The last block: the ramp that covers what is left.
            d.speed = 0.0;
            Slide {
                from,
                to: 2.0 * far / n - from,
                lands: tap.base(),
            }
        } else {
            let to = clampf(far * (GLIDE_SHARE / n), -GLIDE_SPEED, GLIDE_SPEED);
            d.speed = to;
            Slide {
                from,
                to,
                lands: d.time + 0.5 * (from + to) * n,
            }
        };
        return (tap, Some(slide));
    }
    // A step. A head still gliding slows to rest over this block first,
    // so it stops with no kink; then the new head fades in.
    if d.speed != 0.0 {
        let from = d.speed;
        d.speed = 0.0;
        let lands = d.time + 0.5 * from * BLOCK_SIZE as f32;
        return (
            tap,
            Some(Slide {
                from,
                to: 0.0,
                lands,
            }),
        );
    }
    (d.next, d.fade) = (tap.base(), TIME_FADE);
    (tap, None)
}

/// One gliding block of TAPE: `tape::span`, its head moving.
#[inline(never)]
pub(super) fn glide(
    d: &mut TapeDelay,
    buf: &mut [f32; BLOCK_SIZE],
    tap: &Tap,
    e: Eased,
    s: Slide,
    insert: bool,
) {
    span::<false, true, true>(d, buf, tap, e, s, insert);
    d.time = s.lands;
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;

    /// A delay with a second of noise through it, feedback and MECHANICS
    /// running, SYNC on 1/8D at 120 BPM.
    fn warm() -> (Box<TapeDelay>, DelayParams) {
        let p = DelayParams {
            feedback: 0.7,
            wow_flutter: 0.4,
            ..DelayParams::DEFAULT
        }
        .with(DelayParams::SYNC, DelaySync::Sync.value());
        let mut d = Box::new(TapeDelay::new());
        let mut rng = 0x1234_5678u32;
        for _ in 0..750 {
            let mut buf = [0.0; BLOCK_SIZE];
            for s in &mut buf {
                *s = crate::dsp::xorshift_noise(&mut rng) * 0.5;
            }
            d.play(&mut buf, &p, Tempo(120.0), 48_000);
        }
        (d, p)
    }

    /// A gliding block whose head doesn't move is the plain block, bit
    /// for bit: the line, the return and every state.
    #[test]
    fn a_still_glide_is_the_plain_block() {
        let (mut a, p) = warm();
        let (mut b, _) = warm();
        let tap = Tap::new(&p, 48_000);
        let input: [f32; BLOCK_SIZE] = core::array::from_fn(|i| (i as f32 * 0.37).sin() * 0.4);
        let (mut x, mut y) = (input, input);
        let e = a.eased(&p, 48_000);
        span::<false, true, false>(&mut a, &mut x, &tap, e, Slide::STILL, false);
        let e = b.eased(&p, 48_000);
        let still = Slide {
            lands: b.time,
            ..Slide::STILL
        };
        glide(&mut b, &mut y, &tap, e, still, false);
        assert_eq!(x.map(f32::to_bits), y.map(f32::to_bits));
        assert!(a.buffer == b.buffer);
        assert_eq!(a.time.to_bits(), b.time.to_bits());
        assert_eq!(a.lp_state.to_bits(), b.lp_state.to_bits());
        assert_eq!(
            (a.write_pos, a.zeros, a.transport),
            (b.write_pos, b.zeros, b.transport)
        );
    }

    /// A head on its time with a glide's speed left over doesn't rest
    /// there: it takes one block to stop, back on its time, and then rests.
    #[test]
    fn a_stale_speed_is_stopped() {
        let (mut d, p) = warm();
        assert_eq!((d.time, d.speed), (18_000.0, 0.0));
        d.speed = 0.01;
        let (tap, slide) = aim(&mut d, &p, Tempo(120.0), 48_000);
        let s = slide.expect("a block to stop in");
        assert_eq!((s.from, s.lands), (0.01, 18_000.0));
        assert_eq!(d.speed, 0.0);
        let e = d.eased(&p, 48_000);
        glide(&mut d, &mut [0.0; BLOCK_SIZE], &tap, e, s, false);
        assert_eq!(aim(&mut d, &p, Tempo(120.0), 48_000).1, None);
        assert_eq!((d.time, d.speed, d.fade), (18_000.0, 0.0, 0));
    }

    fn note(den: u8, kind: crate::dsp::rev::sync::DivKind) -> DelayNote {
        let i = NOTES
            .iter()
            .position(|d| d.den == den && d.kind == kind)
            .unwrap();
        DelayNote::new(i)
    }

    #[test]
    fn the_notes_are_the_reverbs_and_the_default_the_dotted_eighth() {
        assert_eq!(NOTES, GATE_LENGTH);
        assert_eq!(
            note(8, crate::dsp::rev::sync::DivKind::Dotted),
            DelayNote::DEFAULT
        );
        assert_eq!(DelayParams::DEFAULT.note(), DelayNote::DEFAULT);
        assert_eq!(DelayParams::DEFAULT.sync(), DelaySync::Free);
        // 375 ms at 120 BPM: TIME's own default.
        assert_eq!(
            NOTES[DelayNote::DEFAULT.index()].ms(Tempo(120.0)),
            DelayParams::DEFAULT.time_ms
        );
    }

    /// Every stored place reads back, and the ends hold.
    #[test]
    fn a_note_stores_as_its_place() {
        for i in 0..DelayNote::COUNT {
            assert_eq!(DelayNote::at(DelayNote::new(i).value()).index(), i);
        }
        assert_eq!(DelayNote::at(f32::NAN).index(), 0);
        assert_eq!(DelayNote::at(7.0).index(), DelayNote::COUNT - 1);
        assert_eq!(DelayNote::new(99).index(), DelayNote::COUNT - 1);
    }

    /// Every note at every tempo of ORBIT's clock plays inside the line
    /// and on its grid: the note's own length times a power of two.
    #[test]
    fn a_fitted_note_is_in_the_line_and_on_the_grid() {
        for bpm4 in 30 * 4..=1_200 * 4 {
            let t = Tempo(bpm4 as f32 / 4.0);
            for &d in NOTES {
                let mut st = FoldState(0);
                let len = d.ms(t);
                let ms = fit(len, &mut st);
                assert!(
                    (SHORTEST_MS - GIVE_MS..=LONGEST_MS + GIVE_MS).contains(&ms),
                    "{d:?} at {t:?}: {ms}"
                );
                assert_eq!(ms, libm::ldexpf(len, -i32::from(st.0)), "{d:?} at {t:?}");
            }
        }
    }

    /// 1/4 leaves the line under 120 BPM and comes back whole only at
    /// 122.4: a sweep down and up flips it once each way, and a clock
    /// wandering at the boundary never does.
    #[test]
    fn the_fold_flips_once_each_way() {
        let quarter = NOTES[DelayNote::COUNT - 1];
        let mut st = FoldState(0);
        let mut flips = 0;
        let sweep = (118 * 4..=123 * 4).rev().chain(118 * 4..=123 * 4);
        for bpm4 in sweep {
            let was = st;
            fit(quarter.ms(Tempo(bpm4 as f32 / 4.0)), &mut st);
            flips += u32::from(st != was);
        }
        assert_eq!(flips, 2);
        assert_eq!(st, FoldState(0));
        // Exactly the line at 120 BPM: whole from rest.
        let mut st = FoldState(0);
        assert_eq!(fit(quarter.ms(Tempo(120.0)), &mut st), 500.0);
        assert_eq!(st, FoldState(0));
        // Halved, a clock wandering 119.5..121 stays halved.
        fit(quarter.ms(Tempo(119.75)), &mut st);
        assert_eq!(st, FoldState(1));
        for bpm4 in [478, 484, 480, 483, 479, 484] {
            fit(quarter.ms(Tempo(bpm4 as f32 / 4.0)), &mut st);
            assert_eq!(st, FoldState(1), "{bpm4}");
        }
        fit(quarter.ms(Tempo(122.5)), &mut st);
        assert_eq!(st, FoldState(0));
    }

    /// A triplet the tempo makes exactly 500 ms fits: 1/4T at 80 BPM.
    #[test]
    fn a_note_the_length_of_the_line_fits() {
        let d = NOTES[8];
        assert_eq!(
            (d.den, d.kind),
            (4, crate::dsp::rev::sync::DivKind::Triplet)
        );
        let mut st = FoldState(0);
        let ms = fit(d.ms(Tempo(80.0)), &mut st);
        assert_eq!(st, FoldState(0));
        assert!((ms - 500.0).abs() < 1.0e-3);
    }

    /// Faster than the line's 10 ms a note doubles, on the grid still.
    #[test]
    fn a_note_too_short_doubles() {
        let mut st = FoldState(0);
        let len = NOTES[0].ms(Tempo(1_200.0));
        assert_eq!(fit(len, &mut st), 4.0 * len);
        assert_eq!(st, FoldState(-2));
        // No length, no fold.
        fit(f32::NAN, &mut st);
        assert_eq!(st, FoldState(0));
    }
}
