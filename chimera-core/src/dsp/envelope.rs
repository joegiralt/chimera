use crate::params::EnvParams;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Debug)]
pub struct Envelope {
    stage: Stage,
    level: f32,
    velocity: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            velocity: 1.0,
        }
    }

    pub fn note_on(&mut self, velocity: f32) {
        self.stage = Stage::Attack;
        self.velocity = velocity;
    }

    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Current envelope level (pre-VCA snapshot for mod sources).
    pub fn current_level(&self) -> f32 {
        self.level * self.velocity
    }

    /// Process one sample. Returns envelope level 0.0..1.0.
    pub fn process(&mut self, params: &EnvParams, sample_rate: u32) -> f32 {
        let sr = sample_rate as f32;
        match self.stage {
            Stage::Idle => 0.0,
            Stage::Attack => {
                let rate = 1.0 / (params.attack * sr);
                self.level += rate;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
                self.level * self.velocity
            }
            Stage::Decay => {
                let target = params.sustain;
                let rate = 1.0 / (params.decay * sr);
                self.level -= rate;
                if self.level <= target {
                    self.level = target;
                    self.stage = Stage::Sustain;
                }
                self.level * self.velocity
            }
            Stage::Sustain => self.level * self.velocity,
            Stage::Release => {
                let rate = 1.0 / (params.release * sr);
                self.level -= rate;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
                self.level * self.velocity
            }
        }
    }
}
