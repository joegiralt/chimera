/// TX81Z-style 5-stage FM envelope.
///
/// Stages: Idle → Attack → Decay1 → Decay2 → (Release) → Idle
///
/// Ported from the p81z C++ implementation with one intentional deviation:
/// noteOff() enters a proper Release stage rather than going directly to Idle.

use crate::dsp::fm_tables;

const ENVELOPE_LIMIT: f64 = 0.0001;

#[derive(Clone, Copy, Debug, PartialEq)]
enum EnvState {
    Idle,
    Attack,
    Decay1,
    Decay2,
    Release,
}

#[derive(Clone, Debug)]
pub struct FmEnvelope {
    value: f64,
    state: EnvState,
    attack_factor: f64,
    decay1_target: f64,
    decay1_factor: f64,
    decay2_factor: f64,
    release_factor: f64,
    key_scaling: f32,
}

impl Default for FmEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl FmEnvelope {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            state: EnvState::Idle,
            attack_factor: 0.0,
            decay1_target: 0.0,
            decay1_factor: 1.0,
            decay2_factor: 1.0,
            release_factor: 1.0,
            key_scaling: 1.0,
        }
    }

    /// Arm the envelope and compute all rate factors.
    ///
    /// Parameters follow TX81Z naming:
    /// - `ar`  : attack rate (0–31)
    /// - `d1r` : decay-1 rate (0–31)
    /// - `d1l` : decay-1 level / first-decay target (0–15)
    /// - `d2r` : decay-2 rate (0–31)
    /// - `rr`  : release rate (0–15; internally mapped to 1 + rr*2)
    /// - `kvs` : key velocity sensitivity (0–7)
    /// - `rs`  : rate scaling (0–3)
    /// - `sample_rate` : audio sample rate in Hz
    /// - `note` : MIDI note number (0–127)
    #[allow(clippy::too_many_arguments)]
    pub fn note_on(
        &mut self,
        ar: u8,
        d1r: u8,
        d1l: u8,
        d2r: u8,
        rr: u8,
        kvs: usize,
        rs: usize,
        sample_rate: f32,
        note: u8,
    ) {
        self.state = EnvState::Attack;

        // Velocity = 1.0 (full) when called without a velocity argument.
        // Callers that have velocity should set key_scaling themselves via
        // fm_tables::compute_velocity_factor before calling note_on, or we
        // accept it always at 1.0 normalised velocity here.
        self.key_scaling = fm_tables::compute_velocity_factor(kvs, 1.0);

        let rs_offset = fm_tables::compute_rate_scaling(rs, note);

        self.attack_factor =
            fm_tables::attack_increment(ar as f32, rs_offset, sample_rate) as f64;

        self.decay1_target = fm_tables::d1l_to_level(d1l) as f64;

        self.decay1_factor =
            fm_tables::decay_factor(d1r as f32, rs_offset, sample_rate) as f64;

        self.decay2_factor =
            fm_tables::decay_factor(d2r as f32, rs_offset, sample_rate) as f64;

        // p81z maps release rate as: 1 + rr * 2
        let release_rate = 1.0 + rr as f32 * 2.0;
        self.release_factor =
            fm_tables::decay_factor(release_rate, rs_offset, sample_rate) as f64;

        // Start from silence each note-on (retrigger behaviour).
        self.value = 0.0;
    }

    /// Begin release stage (transitions from any active state).
    pub fn note_off(&mut self) {
        if self.state != EnvState::Idle {
            self.state = EnvState::Release;
        }
    }

    /// Fill `buf` with envelope amplitude values (linear, 0.0–1.0 scaled by
    /// key_scaling).  The caller multiplies the operator output by these values.
    pub fn run(&mut self, buf: &mut [f32]) {
        for sample in buf.iter_mut() {
            *sample = self.process();
        }
    }

    /// Process a single sample, advancing the state machine.
    pub fn process(&mut self) -> f32 {
        match self.state {
            EnvState::Idle => 0.0,

            EnvState::Attack => {
                self.value += self.attack_factor;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.state = EnvState::Decay1;
                }
                (self.key_scaling as f64 * self.value) as f32
            }

            EnvState::Decay1 => {
                self.value *= self.decay1_factor;
                if self.value <= self.decay1_target {
                    self.value = self.decay1_target;
                    self.state = EnvState::Decay2;
                }
                (self.key_scaling as f64 * self.value) as f32
            }

            EnvState::Decay2 => {
                self.value *= self.decay2_factor;
                if self.value < ENVELOPE_LIMIT {
                    self.value = 0.0;
                    self.state = EnvState::Idle;
                }
                (self.key_scaling as f64 * self.value) as f32
            }

            EnvState::Release => {
                self.value *= self.release_factor;
                if self.value < ENVELOPE_LIMIT {
                    self.value = 0.0;
                    self.state = EnvState::Idle;
                }
                (self.key_scaling as f64 * self.value) as f32
            }
        }
    }

    /// Returns `true` when the envelope is silent and will produce no output.
    #[inline]
    pub fn is_idle(&self) -> bool {
        self.state == EnvState::Idle
    }

    /// Instantaneous envelope level (includes key_scaling).
    #[inline]
    pub fn current_level(&self) -> f32 {
        (self.key_scaling as f64 * self.value) as f32
    }
}
