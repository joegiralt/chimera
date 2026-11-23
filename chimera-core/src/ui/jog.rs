// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SCRUB's jog (spec § 2 Machines; CHOMPI's jog scrub, ideas only, ADR
//! 0093): the FINGER's encoder turns POSITION at the speed the hand turns
//! it, and once the turns stop it glides to a stop through DRAG's
//! one-pole, as a reel let go. Pure. `pass` is bank 2's, through the UI's
//! door (ADR 0205).

use chimera_hal::Ms;

use crate::dsp::tape_engine::drag_s;

/// The speed window, ms: the turns the speed is read from. A turn's time
/// is its UI pass's, so a longer window averages the pass's grain away.
pub const JOG_WINDOW_MS: u32 = 250;
/// One detent moves 1/128 of the window.
pub const JOG_DETENT: f32 = 1.0 / 128.0;
/// The speed's one-pole toward the hand's rate, s: a new rate eases in,
/// never steps (ADR 0205).
pub const JOG_EASE_S: f32 = 0.050;
/// The shortest span two turns are read over, ms: a hand turns no faster.
const SPAN_MIN_MS: u32 = 8;
/// Turns remembered: more than a hand turns in 250 ms (a UI pass holds
/// one turn).
const TURNS: usize = 16;

/// The hand's rate, detents a second, signed: backward below 0.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct DetentRate(f32);

impl DetentRate {
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The reel's speed it turns: `JOG_DETENT` of the window a detent.
    pub fn reel(self) -> ReelSpeed {
        ReelSpeed(self.0 * JOG_DETENT)
    }
}

/// The reel's speed: POSITION's, windows a second, signed.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct ReelSpeed(f32);

impl ReelSpeed {
    pub const STILL: ReelSpeed = ReelSpeed(0.0);

    pub const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Jog {
    turns: [(Ms, i8); TURNS],
    next: usize,
    /// The reel's speed, and the last pass's time.
    speed: ReelSpeed,
    last_ms: Option<Ms>,
    /// The hand turns: a turn sets it, a let-go clears it, so turns left
    /// in the window after a let-go never turn the reel again.
    turning: bool,
}

impl Default for Jog {
    fn default() -> Self {
        Self::new()
    }
}

impl Jog {
    pub const fn new() -> Self {
        Jog {
            turns: [(Ms(0), 0); TURNS],
            next: 0,
            speed: ReelSpeed::STILL,
            last_ms: None,
            turning: false,
        }
    }

    /// The encoder turned `detents` at `now_ms`.
    /// A turn against the last one starts the reading afresh: a reversal
    /// never averages with the turns before it.
    pub fn turn(&mut self, detents: i8, now_ms: Ms) {
        let last = self.turns[(self.next + TURNS - 1) % TURNS].1;
        if (last < 0) != (detents < 0) {
            self.turns = [(Ms(0), 0); TURNS];
        }
        self.turns[self.next] = (now_ms, detents);
        self.next = (self.next + 1) % TURNS;
        self.turning = true;
    }

    /// The reel's speed now.
    pub fn speed(&self) -> ReelSpeed {
        self.speed
    }

    /// The hand's rate in detents a second, from the turns of the last
    /// 250 ms: their detents after the first over the time from the first
    /// to the last, so a steady hand reads steady whichever pass reads it
    /// (a count of the window's detents steps by one as a detent enters
    /// or leaves it). One turn alone reads as one detent a window. `None`
    /// once the hand has stopped: no turn in the window, or none for half
    /// again its time between turns.
    fn rate(&self, now_ms: Ms) -> Option<DetentRate> {
        let mut n = 0u32;
        let (mut sum, mut first, mut last) = (0i32, (Ms(0), 0i8), Ms(0));
        for &(t, d) in &self.turns {
            let age = now_ms.since(t);
            if d == 0 || age >= JOG_WINDOW_MS {
                continue;
            }
            if n == 0 || age > now_ms.since(first.0) {
                first = (t, d);
            }
            if n == 0 || age < now_ms.since(last) {
                last = t;
            }
            n += 1;
            sum += i32::from(d);
        }
        match n {
            0 => None,
            1 => Some(DetentRate(sum as f32 * 1_000.0 / JOG_WINDOW_MS as f32)),
            _ => {
                let span = last.since(first.0).max(SPAN_MIN_MS);
                let gap = now_ms.since(last);
                (2 * gap * (n - 1) <= 3 * span)
                    .then(|| DetentRate((sum - i32::from(first.1)) as f32 * 1_000.0 / span as f32))
            }
        }
    }

    /// POSITION's move this UI pass, in windows (the caller clamps
    /// POSITION to 0..1): while the hand turns, its rate × 1/128 of the
    /// window, eased in through `JOG_EASE_S`; once it stops, that speed
    /// glides to 0 through a one-pole of `drag_s(drag)`.
    #[inline(never)]
    pub fn pass(&mut self, now_ms: Ms, drag: f32) -> f32 {
        let dt = self.last_ms.map_or(0.0, |t| now_ms.since(t) as f32 * 0.001);
        self.last_ms = Some(now_ms);
        let v = self.speed.0;
        if let Some(r) = self.rate(now_ms).filter(|_| self.turning) {
            let k = 1.0 - libm::expf(-dt / JOG_EASE_S);
            self.speed = ReelSpeed(v + k * (r.reel().0 - v));
            return self.speed.0 * dt;
        }
        self.turning = false;
        // The glide's exact travel over the pass: a stopping tape covers
        // its speed × τ in all.
        let tau = drag_s(drag);
        let fall = libm::expf(-dt / tau);
        self.speed = ReelSpeed(v * fall);
        v * tau * (1.0 - fall)
    }
}
