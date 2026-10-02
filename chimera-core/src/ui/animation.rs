use chimera_hal::Ms;

/// Linear interpolation between a and b
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Ease-out cubic: decelerating to zero velocity
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = 1.0 - t;
    1.0 - t * t * t
}

#[inline(always)]
fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

/// A value that smoothly interpolates toward a target.
/// If the frame rate drops, snaps to target instead of lagging behind.
#[derive(Clone, Copy, Debug)]
pub struct AnimatedValue {
    current: f32,
    target: f32,
    /// Fraction of the gap closed per update (0.0..1.0).
    /// Higher = snappier. 0.3 gives responsive feel even at reduced frame rates.
    speed: f32,
}

const SNAP_THRESHOLD: f32 = 0.003;

impl AnimatedValue {
    pub const fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            speed: 0.3,
        }
    }

    pub const fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Snap both current and target to a value (no animation).
    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    /// Advance animation by one frame.
    /// Uses aggressive convergence — at low frame rates, jumps to target
    /// rather than dragging out the animation.
    pub fn update(&mut self) {
        let gap = abs(self.target - self.current);
        if gap < SNAP_THRESHOLD {
            self.current = self.target;
        } else {
            // Lerp with speed, but snap if we'd need more than ~3 frames to settle
            self.current = lerp(self.current, self.target, self.speed);
        }
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_settled(&self) -> bool {
        abs(self.target - self.current) < SNAP_THRESHOLD
    }
}

/// UI frames since boot, ticked once per `UiState::update`: the phase an
/// animated glyph reads. UI side only; wraps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiClock(u32);

impl UiClock {
    pub const fn new() -> Self {
        Self(0)
    }

    pub fn tick(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }

    pub const fn frame(self) -> u32 {
        self.0
    }
}

/// UI frames a second: what `update`'s easing and clock are tuned for.
pub const UI_FPS: u32 = 20;
const FRAME_MS: u32 = 1000 / UI_FPS;
/// Frames still owed after a stall: the rest are dropped.
const MAX_OWED: u32 = 2;

/// Leave to run one UI frame. Only `Pacer` makes one, so `update` runs at
/// `UI_FPS` however fast the loop spins:
/// ```compile_fail,E0603
/// let _ = chimera_core::ui::animation::UiTick(());
/// ```
#[derive(Debug)]
pub struct UiTick(());

impl UiTick {
    /// A frame for tests and harnesses that step time themselves.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn for_test() -> UiTick {
        UiTick(())
    }
}

/// Hands out `UiTick`s at `UI_FPS` from a wrapping ms clock.
#[derive(Clone, Copy, Debug)]
pub struct Pacer {
    last: Ms,
}

impl Pacer {
    /// A pacer from `now`, and the first frame, due at once.
    pub const fn start(now: Ms) -> (Pacer, UiTick) {
        (Pacer { last: now }, UiTick(()))
    }

    /// One frame if one is due: never a burst, and after a stall at most
    /// `MAX_OWED` more on the calls that follow.
    pub fn due(&mut self, now: Ms) -> Option<UiTick> {
        if now.since(self.last) < FRAME_MS {
            return None;
        }
        self.last = Ms(self.last.0.wrapping_add(FRAME_MS));
        if now.since(self.last) > MAX_OWED * FRAME_MS {
            self.last = Ms(now.0.wrapping_sub(MAX_OWED * FRAME_MS));
        }
        Some(UiTick(()))
    }
}
