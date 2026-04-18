use libm::fabsf;

/// Linear interpolation between a and b
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Ease-out cubic: decelerating to zero velocity
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = 1.0 - t;
    1.0 - t * t * t
}

/// A value that smoothly interpolates toward a target.
/// Used for display-side parameter animation ("never snap, always glide").
#[derive(Clone, Copy, Debug)]
pub struct AnimatedValue {
    current: f32,
    target: f32,
    /// Fraction of the gap closed per update (0.0..1.0).
    /// Higher = snappier. 0.15 is a good default for 30fps UI.
    speed: f32,
}

const SNAP_THRESHOLD: f32 = 0.001;

impl AnimatedValue {
    pub const fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            speed: 0.15,
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

    /// Advance animation by one frame. Call at 30fps.
    pub fn update(&mut self) {
        if fabsf(self.target - self.current) < SNAP_THRESHOLD {
            self.current = self.target;
        } else {
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
        fabsf(self.target - self.current) < SNAP_THRESHOLD
    }
}
