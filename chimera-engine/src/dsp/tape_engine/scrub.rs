//! SCRUB's units (ADR 0205): where the finger sits in its room, and the
//! loop behind it as a time. Bank 2's, with the deck (`bank2.x`).

use chimera_hal::MidiNote;

use super::MIN_LOOP_S;
use crate::dsp::algo::math::clampf;
use crate::stream::units::SourceRate;

/// A place in the finger's room (the window less the loop): 0 its start,
/// 1 its end. Never outside 0..1, never NaN.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Place(f32);

impl Place {
    pub const START: Place = Place(0.0);

    /// `x` held to 0..1; NaN is the start.
    pub fn new(x: f32) -> Place {
        Place(if x.is_nan() { 0.0 } else { clampf(x, 0.0, 1.0) })
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    /// `self` moved by `by` (FINGER, a share of the room), at most the
    /// room's end.
    pub fn plus(self, by: f32) -> Place {
        Place::new(self.0 + if by.is_nan() { 0.0 } else { by })
    }
}

/// KEY POSITION's places an octave: C places the finger at the room's
/// start, each semitone up 1/12 further, every octave alike.
pub const KEY_PLACES: u8 = 12;

/// Where KEY POSITION's `note` places the finger: its semitone above C, in
/// twelfths of the room (0 to 11/12).
pub fn key_place(note: MidiNote) -> Place {
    Place(f32::from(note.get() % KEY_PLACES) / f32::from(KEY_PLACES))
}

/// SCRUB's LENGTH at its longest, seconds.
pub const SCRUB_LEN_MAX_S: f32 = 2.0;
/// `ln(SCRUB_LEN_MAX_S / MIN_LOOP_S)`.
const SCRUB_LEN_LN: f32 = 6.907_755;

/// SCRUB's loop as a time, seconds: `2 ms · 1000^LENGTH`, 2 ms to
/// `SCRUB_LEN_MAX_S`, whatever the window.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LoopTime(f32);

impl LoopTime {
    /// LENGTH's knob, 0..1 (NaN the shortest).
    pub fn of_length(length: f32) -> LoopTime {
        let l = if length.is_nan() {
            0.0
        } else {
            clampf(length, 0.0, 1.0)
        };
        LoopTime(MIN_LOOP_S * libm::expf(l * SCRUB_LEN_LN))
    }

    pub const fn seconds(self) -> f32 {
        self.0
    }

    /// The window's law's SIZE (`loop_len`) that makes this loop on a
    /// window of `w` frames at `rate`: at most 1, the whole window.
    pub fn share_of(self, w: u32, rate: SourceRate) -> f32 {
        let min = MIN_LOOP_S * f32::from(rate);
        let span = libm::logf(w as f32 / min);
        if span > 0.0 {
            (libm::logf(self.0 / MIN_LOOP_S) / span).min(1.0)
        } else {
            1.0
        }
    }
}
