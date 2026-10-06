//! SCRUB's jog (spec § 2 Machines; CHOMPI's jog scrub, ideas only, ADR
//! 0093): the FINGER's encoder turns POSITION at the speed it was turned
//! over the last 1/8 s, and once the turns stop it glides to a stop
//! through DRAG's one-pole, as a reel let go. Pure; Task 15 wires it.

use crate::dsp::tape_engine::drag_s;

/// The speed window, ms.
pub const JOG_WINDOW_MS: u32 = 125;
/// One detent moves 1/128 of the window.
pub const JOG_DETENT: f32 = 1.0 / 128.0;
/// Turns remembered: more than a hand turns in 125 ms.
const TURNS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Jog {
    turns: [(u32, i8); TURNS],
    next: usize,
    /// A turn came since the last pass.
    fresh: bool,
    /// POSITION's speed, windows a second, and the last pass's time.
    speed: f32,
    last_ms: Option<u32>,
}

impl Default for Jog {
    fn default() -> Self {
        Self::new()
    }
}

impl Jog {
    pub const fn new() -> Self {
        Jog {
            turns: [(0, 0); TURNS],
            next: 0,
            fresh: false,
            speed: 0.0,
            last_ms: None,
        }
    }

    /// The encoder turned `detents` at `now_ms`.
    pub fn turn(&mut self, detents: i8, now_ms: u32) {
        self.turns[self.next] = (now_ms, detents);
        self.next = (self.next + 1) % TURNS;
        self.fresh = true;
    }

    /// The speed now, windows a second.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// POSITION's move this UI pass, in windows (the caller clamps
    /// POSITION to 0..1): while turns come, the detents of the last
    /// 125 ms × 1/128 of the window ÷ 0.125 s; once they stop, that speed
    /// glides to 0 through a one-pole of `drag_s(drag)`.
    pub fn pass(&mut self, now_ms: u32, drag: f32) -> f32 {
        let dt = self
            .last_ms
            .map_or(0.0, |t| now_ms.wrapping_sub(t) as f32 * 0.001);
        self.last_ms = Some(now_ms);
        if core::mem::take(&mut self.fresh) {
            let n: i32 = self
                .turns
                .iter()
                .filter(|(t, _)| now_ms.wrapping_sub(*t) < JOG_WINDOW_MS)
                .map(|&(_, d)| i32::from(d))
                .sum();
            self.speed = n as f32 * JOG_DETENT / (JOG_WINDOW_MS as f32 * 0.001);
            return self.speed * dt;
        }
        // The glide's exact travel over the pass: a stopping tape covers
        // its speed × τ in all.
        let tau = drag_s(drag);
        let fall = libm::expf(-dt / tau);
        let mv = self.speed * tau * (1.0 - fall);
        self.speed *= fall;
        mv
    }
}
