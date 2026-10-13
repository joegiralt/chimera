//! The pure CC core (spec §§ 5-6). A continuous target ramps over the gap
//! between its CCs so a slow sweep is never a staircase; a stepped one bins
//! with hysteresis; the MOVES budget picks which ramps write a pass.

use chimera_hal::Ms;

/// The ramp's shortest and longest crossing, ms.
pub const RAMP_MIN_MS: u32 = 15;
pub const RAMP_MAX_MS: u32 = 120;
/// A ±1 reversal this soon after the last change is jitter.
pub const DEAD_MS: u32 = 50;
/// Counts past a bin's boundary a stepped target needs to move.
pub const HYST: u8 = 2;

/// No CC heard yet. CC values are 7-bit, so it never collides.
const NONE: u8 = u8::MAX;

/// One mapped entry's ramp: 16 bytes. The rate is implied by `left`, the ms
/// still to go, so the last step lands exactly on `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    at: f32,
    to: f32,
    last: Ms,
    left: u8,
    last_v: u8,
    prev_v: u8,
}

impl Ramp {
    pub const fn rest(place: f32) -> Ramp {
        Ramp {
            at: place,
            to: place,
            last: Ms(0),
            left: 0,
            last_v: NONE,
            prev_v: NONE,
        }
    }

    /// Heads for `place`, crossing in the gap since the last CC. `false`
    /// when `v` repeats the last value or is a ±1 reversal inside `DEAD_MS`.
    pub fn hear(&mut self, v: u8, place: f32, now: Ms) -> bool {
        let v = v.min(127);
        let dt = now.since(self.last);
        if v == self.last_v || (v == self.prev_v && v.abs_diff(self.last_v) == 1 && dt < DEAD_MS) {
            return false;
        }
        let t = if self.last_v == NONE || dt > RAMP_MAX_MS {
            RAMP_MIN_MS
        } else {
            dt.max(RAMP_MIN_MS)
        };
        (self.prev_v, self.last_v, self.last) = (self.last_v, v, now);
        self.to = place;
        self.left = if place == self.at { 0 } else { t as u8 };
        true
    }

    /// The new place after `elapsed_ms`, `None` at rest. Linear, never past
    /// `to`, and exactly `to` on arrival.
    pub fn step(&mut self, elapsed_ms: u32) -> Option<f32> {
        if self.left == 0 {
            return None;
        }
        let left = u32::from(self.left);
        if elapsed_ms >= left {
            (self.at, self.left) = (self.to, 0);
        } else {
            let next = self.at + (self.to - self.at) * (elapsed_ms as f32 / left as f32);
            self.at = if self.to >= self.at {
                next.min(self.to)
            } else {
                next.max(self.to)
            };
            self.left = (left - elapsed_ms) as u8;
        }
        Some(self.at)
    }

    /// The hand wins: rest at `place`.
    pub fn cancel(&mut self, place: f32) {
        (self.at, self.to, self.left) = (place, place, 0);
    }
}

/// The step `v` (0..=127) falls in when `min..=max` (places 0..=255,
/// `min > max` inverts) is cut into `steps` equal bins.
fn raw_bin(v: u8, steps: u16, min: u8, max: u8) -> u16 {
    let (lo, hi) = (i32::from(min), i32::from(max));
    let p = lo + (hi - lo) * i32::from(v.min(127)) / 127;
    ((p * i32::from(steps) / 256) as u16).min(steps.saturating_sub(1))
}

/// The step index for `v`, moving off `cur` only once `v` is `HYST` counts
/// past the boundary. The ends 0 and 127 always reach their bin.
pub fn bin(v: u8, steps: u16, min: u8, max: u8, cur: u16) -> u16 {
    let raw = raw_bin(v, steps, min, max);
    if raw == cur || v == 0 || v >= 127 {
        return raw;
    }
    let a = raw_bin(v.saturating_sub(HYST), steps, min, max);
    let b = raw_bin(v + HYST, steps, min, max);
    if raw > cur {
        let n = a.min(b);
        if n > cur { n } else { cur }
    } else {
        let n = a.max(b);
        if n < cur { n } else { cur }
    }
}

/// Picks at most `cap` due entries, round-robin from `cursor`, and moves the
/// cursor past the last picked so the ones left over go first next pass.
pub fn budget<const N: usize>(due: &[bool; N], cursor: &mut usize, cap: usize) -> [bool; N] {
    let mut out = [false; N];
    let (start, mut n) = (*cursor, 0);
    for k in 0..N {
        if n == cap {
            break;
        }
        let i = (start + k) % N;
        if due[i] {
            out[i] = true;
            n += 1;
            *cursor = (i + 1) % N;
        }
    }
    out
}
