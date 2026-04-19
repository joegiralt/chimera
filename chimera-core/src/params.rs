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

    /// Set from normalized 0.0..1.0
    pub fn set_normalized(&mut self, n: f32) {
        self.set(self.min + n * (self.max - self.min));
    }

    /// Snap to next coarse point in the given direction.
    /// `snap_points` are in normalized space (0..1), must be sorted ascending.
    pub fn snap_to(&mut self, delta: i8, snap_points: &[f32]) {
        let n = self.normalized();
        if delta > 0 {
            // Find first snap point above current (with small epsilon)
            for &sp in snap_points {
                if sp > n + 0.005 {
                    self.set_normalized(sp);
                    return;
                }
            }
            // Already at or above max snap point
            self.set_normalized(1.0);
        } else {
            // Find last snap point below current
            for &sp in snap_points.iter().rev() {
                if sp < n - 0.005 {
                    self.set_normalized(sp);
                    return;
                }
            }
            // Already at or below min snap point
            self.set_normalized(0.0);
        }
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

/// Parameters for pre-filter drive stage
#[derive(Clone, Copy, Debug)]
pub struct DriveParams {
    pub drive: Param,
    pub tone: Param,
    pub mix: Param,
}

impl Default for DriveParams {
    fn default() -> Self {
        Self {
            drive: Param::new(0.0, 1.0, 0.0),
            tone: Param::new(0.0, 1.0, 0.5),
            mix: Param::new(0.0, 1.0, 1.0),
        }
    }
}

/// Parameters for post-filter wavefolder
#[derive(Clone, Copy, Debug)]
pub struct FolderParams {
    pub fold: Param,
    pub symmetry: Param,
    pub mix: Param,
}

impl Default for FolderParams {
    fn default() -> Self {
        Self {
            fold: Param::new(0.0, 1.0, 0.0),
            symmetry: Param::new(0.0, 1.0, 0.5),
            mix: Param::new(0.0, 1.0, 0.5),
        }
    }
}

/// Which synthesis engine is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EngineType {
    #[default]
    Fm = 0,
    Modal = 1,
    Va = 2,
}

#[derive(Clone, Debug)]
pub struct ParamSnapshot {
    pub engine: EngineType,
    pub filter: FilterParams,
    pub drive: DriveParams,
    pub folder: FolderParams,
    pub envelopes: [EnvParams; 3],
    pub fm: crate::dsp::fm::FmParams,
    pub modal: crate::dsp::modal::ModalParams,
    pub reverb: crate::dsp::reverb::ReverbParams,
    pub delay: crate::dsp::delay::DelayParams,
    pub chorus: crate::dsp::chorus::ChorusParams,
    pub volume: Param,
    pub pan: Param,
}

impl Default for ParamSnapshot {
    fn default() -> Self {
        Self {
            engine: EngineType::default(),
            filter: {
                let mut f = FilterParams::default();
                f.cutoff = Param::new(20.0, 20000.0, 20000.0); // fully open
                f
            },
            drive: DriveParams::default(),
            folder: FolderParams::default(),
            envelopes: [EnvParams::default(); 3],
            fm: crate::dsp::fm::FmParams::default(),
            modal: crate::dsp::modal::ModalParams::default(),
            delay: crate::dsp::delay::DelayParams::default(),
            chorus: crate::dsp::chorus::ChorusParams::default(),
            reverb: crate::dsp::reverb::ReverbParams {
                reverb_type: 0,
                time: 0.5,
                damping: 0.3,
                size: 0.5,
                mix: 0.0, // effects off by default
            },
            volume: Param::new(0.0, 1.0, 0.8),
            pan: Param::new(-1.0, 1.0, 0.0),
        }
    }
}
