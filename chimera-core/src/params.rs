/// A single parameter value with range clamping
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl Param {
    pub const fn new(min: f32, max: f32, default: f32) -> Self {
        Self {
            value: default,
            min,
            max,
            default,
        }
    }

    pub fn set(&mut self, v: f32) {
        self.value = if v < self.min {
            self.min
        } else if v > self.max {
            self.max
        } else {
            v
        };
    }

    pub fn nudge(&mut self, delta: f32) {
        self.set(self.value + delta);
    }

    /// Normalized 0.0..1.0
    pub fn normalized(&self) -> f32 {
        (self.value - self.min) / (self.max - self.min)
    }
}

/// Parameters for one voice's filter
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub cutoff: Param,
    pub resonance: Param,
    pub drive: Param,
    pub fm_amount: Param,
    pub env_amount: Param,
    pub key_track: Param,
    pub mode: u8,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: Param::new(20.0, 20000.0, 1000.0),
            resonance: Param::new(0.0, 1.0, 0.0),
            drive: Param::new(0.0, 1.0, 0.0),
            fm_amount: Param::new(0.0, 1.0, 0.0),
            env_amount: Param::new(-1.0, 1.0, 0.0),
            key_track: Param::new(0.0, 1.0, 0.0),
            mode: 2, // LP4
        }
    }
}

/// Parameters for one envelope
#[derive(Clone, Copy, Debug)]
pub struct EnvParams {
    pub attack: Param,
    pub decay: Param,
    pub sustain: Param,
    pub release: Param,
    pub level: Param,
    pub vel_sens: Param,
}

impl Default for EnvParams {
    fn default() -> Self {
        Self {
            attack: Param::new(0.001, 10.0, 0.01),
            decay: Param::new(0.001, 10.0, 0.3),
            sustain: Param::new(0.0, 1.0, 0.7),
            release: Param::new(0.001, 10.0, 0.3),
            level: Param::new(0.0, 1.0, 1.0),
            vel_sens: Param::new(0.0, 1.0, 0.5),
        }
    }
}

/// Full snapshot of all parameters for one part
#[derive(Clone, Debug)]
pub struct ParamSnapshot {
    pub filter: FilterParams,
    pub envelopes: [EnvParams; 3],
    pub volume: Param,
    pub pan: Param,
}

impl Default for ParamSnapshot {
    fn default() -> Self {
        Self {
            filter: FilterParams::default(),
            envelopes: [EnvParams::default(); 3],
            volume: Param::new(0.0, 1.0, 0.8),
            pan: Param::new(-1.0, 1.0, 0.0),
        }
    }
}
