//! WALK's DRIFT (ADR 0203): at each note-on POSITION steps by a random
//! amount of up to DRIFT windows either way, from where the last note's
//! step left it, so repeats wander through the source (Octatrack's random
//! start, carried from note to note). Each voice draws from its own seeded
//! xorshift, so a render repeats. The place wraps at the window's ends.
//! DRIFT 0 rests at POSITION and draws nothing.

use super::heads::Window;
use crate::dsp::algo::math::clampf;

/// A voice's draws before its seed.
pub const SEED: u32 = 0x2545_F491;

/// Voice `s`'s draws: distinct from its wow's (`TapeDeck::seed`).
pub const fn seed(s: u32) -> u32 {
    (s.wrapping_mul(0x85EB_CA6B) ^ SEED) | 1
}

/// A uniform draw in `[−1, 1)`.
fn draw(rng: &mut u32) -> f32 {
    let mut x = *rng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *rng = x;
    x as i32 as f32 * (1.0 / 2_147_483_648.0)
}

/// The walk's place after a note-on at `walk` from `at`, in windows
/// `[0, 1)`: `at` plus up to ±`walk`, wrapped. 0 at DRIFT 0 (or a NaN).
pub fn step(at: f32, walk: f32, rng: &mut u32) -> f32 {
    let walk = clampf(walk, 0.0, 1.0);
    if !(walk > 0.0) {
        return 0.0;
    }
    wrap(at + walk * draw(rng))
}

/// `x` into `[0, 1)`; a NaN is 0.
pub fn wrap(x: f32) -> f32 {
    let w = x - libm::floorf(x);
    if w >= 0.0 && w < 1.0 { w } else { 0.0 }
}

/// POSITION moved by the walk, `[0, 1)` wrapped; POSITION alone (its 1
/// kept) while the walk rests, so a still walk changes nothing.
pub fn place(position: f32, walk: f32) -> f32 {
    if walk == 0.0 {
        position
    } else {
        wrap(position + walk)
    }
}

/// The frame a once-through note starts at, `place` of `w` counted the way
/// it plays, leaving at least `floor` frames (or the window) to play.
pub fn from(w: Window, place: f32, floor: u32) -> u32 {
    let n = w.frames();
    let room = n.saturating_sub(floor.min(n));
    w.lo + ((clampf(place, 0.0, 1.0) * n as f32) as u32).min(room)
}
