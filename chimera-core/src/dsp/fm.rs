use chimera_hal::BLOCK_SIZE;

use crate::dsp::{fast_sin, fast_sin_abs};
use crate::dsp::envelope::Envelope;
use crate::params::EnvParams;

use core::f32::consts::PI;

// ── Waveforms (TX81Z W1-W8) ─────────────────────────────────────────

/// TX81Z waveform index (0-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Waveform {
    Sine = 0,
    HalfSine = 1,
    FullSine = 2,
    QuarterSine = 3,
    HalfDouble = 4,
    FullDouble = 5,
    ResPulse1 = 6,
    ResPulse2 = 7,
}

impl Waveform {
    pub fn from_index(i: u8) -> Self {
        match i % 8 {
            0 => Waveform::Sine,
            1 => Waveform::HalfSine,
            2 => Waveform::FullSine,
            3 => Waveform::QuarterSine,
            4 => Waveform::HalfDouble,
            5 => Waveform::FullDouble,
            6 => Waveform::ResPulse1,
            _ => Waveform::ResPulse2,
        }
    }
}

/// Evaluate a TX81Z waveform at phase `theta` (0..2π).
fn waveform(w: Waveform, theta: f32) -> f32 {
    let s = fast_sin(theta);
    match w {
        // W1: sin(θ)
        Waveform::Sine => s,
        // W2: sin(θ) for 0≤θ<π, 0 for π≤θ<2π
        Waveform::HalfSine => {
            if theta < PI {
                s
            } else {
                0.0
            }
        }
        // W3: |sin(θ)| (rectified)
        Waveform::FullSine => fast_sin_abs(theta),
        // W4: sin(θ) for 0≤θ<π/2, 0 otherwise (per half-cycle)
        Waveform::QuarterSine => {
            let t = theta % PI;
            if t < PI * 0.5 { fast_sin(t) } else { 0.0 }
        }
        // W5: sin(2θ) for 0≤θ<π, 0 for π≤θ<2π
        Waveform::HalfDouble => {
            if theta < PI {
                fast_sin(theta * 2.0)
            } else {
                0.0
            }
        }
        // W6: |sin(2θ)| (rectified double-speed)
        Waveform::FullDouble => fast_sin_abs(theta * 2.0),
        // W7: narrow resonance pulse (sin^4 approximation)
        Waveform::ResPulse1 => {
            let s2 = s * s;
            s2 * s2 // sin⁴
        }
        // W8: even narrower (sin^8 approximation)
        Waveform::ResPulse2 => {
            let s2 = s * s;
            let s4 = s2 * s2;
            s4 * s4 // sin⁸
        }
    }
}

// ── Harmonic ratio table (Digitone-style) ───────────────────────────

/// Coarse ratios that snap to harmonic values.
/// Encoder steps through this array — every position is musical.
pub static HARMONIC_RATIOS: &[f32] = &[
    0.125, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 16.0,
];

/// Map a normalized 0..1 value to a harmonic ratio.
pub fn ratio_from_normalized(n: f32) -> f32 {
    let idx = (n * (HARMONIC_RATIOS.len() - 1) as f32 + 0.5) as usize;
    let idx = if idx >= HARMONIC_RATIOS.len() {
        HARMONIC_RATIOS.len() - 1
    } else {
        idx
    };
    HARMONIC_RATIOS[idx]
}

// ── FM Operator ─────────────────────────────────────────────────────

/// A single FM operator: phase accumulator + waveform + envelope.
#[derive(Clone, Debug)]
pub struct FmOperator {
    /// Phase accumulator (0..1, wraps)
    phase: f32,
    /// Phase increment per sample (freq / sample_rate)
    phase_inc: f32,
    /// Per-operator envelope
    pub envelope: Envelope,
    /// Waveform selection
    pub waveform: Waveform,
    /// Output level (0..1)
    pub level: f32,
    /// Coarse frequency ratio (from harmonic table)
    pub ratio: f32,
    /// Fine detune in Hz (additive, Digitone-style)
    pub detune: f32,
    /// Feedback buffer (2-sample history for self-modulation)
    fb_history: [f32; 2],
}

impl Default for FmOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl FmOperator {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            phase_inc: 0.0,
            envelope: Envelope::new(),
            waveform: Waveform::Sine,
            level: 1.0,
            ratio: 1.0,
            detune: 0.0,
            fb_history: [0.0; 2],
        }
    }

    /// Set base frequency for this operator (called on note_on).
    pub fn set_frequency(&mut self, base_freq: f32, sample_rate: u32) {
        let freq = base_freq * self.ratio + self.detune;
        self.phase_inc = freq / sample_rate as f32;
    }

    /// Compute one sample. `modulation` is added to phase before waveform lookup.
    /// Modulation is pre-scaled by MOD_DEPTH so level 0..1 maps to useful FM indices.
    /// `feedback` is the feedback amount (0..1) — only used for self-modulating op4.
    pub fn tick(
        &mut self,
        modulation: f32,
        feedback: f32,
        env_params: &EnvParams,
        sample_rate: u32,
    ) -> f32 {
        // Feedback: average of last 2 outputs, scaled
        let fb_mod = if feedback > 0.0 {
            feedback * (self.fb_history[0] + self.fb_history[1]) * 0.5
        } else {
            0.0
        };

        // Phase modulation — scale modulator input for useful FM range.
        // Factor of 4.0 means level=1.0 gives mod index of 4*2π ≈ 25,
        // which covers the full range from subtle to extreme.
        let theta = (self.phase + (modulation + fb_mod) * 4.0) * 2.0 * PI;

        // Waveform lookup
        let raw = waveform(self.waveform, theta);

        // Envelope
        let env = self.envelope.process(env_params, sample_rate);

        // Output
        let out = raw * env * self.level;

        // Update feedback history
        self.fb_history[1] = self.fb_history[0];
        self.fb_history[0] = out;

        // Advance phase
        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        out
    }

    pub fn note_on(&mut self, velocity: f32) {
        // Don't reset phase — free-running prevents click on retrigger.
        // Only reset feedback history to avoid artifacts from previous note.
        self.fb_history = [0.0; 2];
        self.envelope.note_on(velocity);
    }

    pub fn note_off(&mut self) {
        self.envelope.note_off();
    }

    pub fn is_active(&self) -> bool {
        self.envelope.is_active()
    }
}

// ── FM Algorithm (TX81Z / YM2414 correct routing) ───────────────────
//
// TX81Z operator numbering: Op1 has feedback, Op4 is typically the carrier.
// Evaluation order: Op1 first (feedback), then forward through the chain.
//
//   Algo 1: [1]→[2]→[3]→[4]*          Serial. One carrier.
//   Algo 2: [1]→[3]→[4]*  [2]→[3]     Two mods into op3. One carrier.
//   Algo 3: [1]→[2]→[4]*  [3]→[4]     Two mods into op4. One carrier.
//   Algo 4: [1]→[2]→[4]*  [3]→[4]     Same as 3 but op1 also→4. One carrier.
//           [1]──────→[4]
//   Algo 5: [1]→[2]*  [3]→[4]*        Two parallel stacks. Two carriers.
//   Algo 6: [1]→[2]*  [1]→[3]*        Op1 fans out. Three carriers.
//                      [1]→[4]*
//   Algo 7: [1]→[2]*  [3]*  [4]*      Op1 mods op2, rest free. Three carriers.
//   Algo 8: [1]*  [2]*  [3]*  [4]*    All carriers. Additive.

/// Carrier masks per algorithm. bit0=op1, bit1=op2, bit2=op3, bit3=op4.
const CARRIER_MASK: [u8; 8] = [
    0b1000, // Algo 1: op4
    0b1000, // Algo 2: op4
    0b1000, // Algo 3: op4
    0b1000, // Algo 4: op4
    0b1010, // Algo 5: op2, op4
    0b1110, // Algo 6: op2, op3, op4
    0b1110, // Algo 7: op2, op3, op4
    0b1111, // Algo 8: all
];

/// Evaluate the 4 operators for one sample.
/// Feedback is always on Op1 (TX81Z convention).
pub fn fm_algorithm_tick(
    ops: &mut [FmOperator; 4],
    algo: u8,
    feedback: f32,
    env_params: &[EnvParams; 4],
    sample_rate: u32,
) -> f32 {
    // Op1 always evaluates first — it has the feedback loop.
    let op1 = ops[0].tick(0.0, feedback, &env_params[0], sample_rate);

    let op2;
    let op3;
    let op4;

    match algo {
        0 => {
            // Algo 1: 1→2→3→4*  (serial chain)
            op2 = ops[1].tick(op1, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(op2, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(op3, 0.0, &env_params[3], sample_rate);
        }
        1 => {
            // Algo 2: 1→3, 2→3, 3→4*  (two mods into op3)
            op2 = ops[1].tick(0.0, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(op1 + op2, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(op3, 0.0, &env_params[3], sample_rate);
        }
        2 => {
            // Algo 3: 1→2→4, 3→4*  (two paths into carrier)
            op2 = ops[1].tick(op1, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(0.0, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(op2 + op3, 0.0, &env_params[3], sample_rate);
        }
        3 => {
            // Algo 4: 1→2→4, 3→4, 1→4*  (three mods into carrier)
            op2 = ops[1].tick(op1, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(0.0, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(op1 + op2 + op3, 0.0, &env_params[3], sample_rate);
        }
        4 => {
            // Algo 5: 1→2*, 3→4*  (two parallel stacks)
            op2 = ops[1].tick(op1, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(0.0, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(op3, 0.0, &env_params[3], sample_rate);
        }
        5 => {
            // Algo 6: 1→2*, 1→3*, 1→4*  (one mod fans to three carriers)
            op2 = ops[1].tick(op1, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(op1, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(op1, 0.0, &env_params[3], sample_rate);
        }
        6 => {
            // Algo 7: 1→2*, 3*, 4*  (op1 mods op2, rest independent)
            op2 = ops[1].tick(op1, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(0.0, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(0.0, 0.0, &env_params[3], sample_rate);
        }
        _ => {
            // Algo 8: 1*, 2*, 3*, 4*  (all carriers, additive)
            op2 = ops[1].tick(0.0, 0.0, &env_params[1], sample_rate);
            op3 = ops[2].tick(0.0, 0.0, &env_params[2], sample_rate);
            op4 = ops[3].tick(0.0, 0.0, &env_params[3], sample_rate);
        }
    }

    // Sum carriers
    let mask = CARRIER_MASK[algo.min(7) as usize];
    let mut out = 0.0;
    if mask & 0b0001 != 0 {
        out += op1;
    }
    if mask & 0b0010 != 0 {
        out += op2;
    }
    if mask & 0b0100 != 0 {
        out += op3;
    }
    if mask & 0b1000 != 0 {
        out += op4;
    }

    let carrier_count = mask.count_ones() as f32;
    out / carrier_count
}

// ── FM Engine ───────────────────────────────────────────────────────

/// Complete 4-operator FM engine. Renders block-based audio.
#[derive(Clone, Debug)]
pub struct FmEngine {
    pub ops: [FmOperator; 4],
    /// Algorithm index (0-7)
    pub algorithm: u8,
    /// Feedback amount for op4 (0..1, maps to 0..4π modulation index)
    pub feedback: f32,
    /// Base frequency from last note_on
    base_freq: f32,
    /// Whether any operator is still sounding
    active: bool,
}

impl Default for FmEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FmEngine {
    pub fn new() -> Self {
        Self {
            ops: [
                FmOperator::new(),
                FmOperator::new(),
                FmOperator::new(),
                FmOperator::new(),
            ],
            algorithm: 0,
            feedback: 0.0,
            base_freq: 0.0,
            active: false,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, sample_rate: u32) {
        self.base_freq = note_to_freq(note);
        let vel = velocity as f32 / 127.0;
        for op in &mut self.ops {
            op.set_frequency(self.base_freq, sample_rate);
            op.note_on(vel);
        }
        self.active = true;
    }

    pub fn note_off(&mut self) {
        for op in &mut self.ops {
            op.note_off();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Render a block of audio samples.
    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        env_params: &[EnvParams; 4],
        sample_rate: u32,
    ) {
        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        // Scale feedback: 0..1 maps to 0..4π modulation index
        let fb = self.feedback * 4.0 * PI;

        for s in output.iter_mut() {
            *s = fm_algorithm_tick(&mut self.ops, self.algorithm, fb, env_params, sample_rate);
        }

        // Check if all operators have finished their envelopes
        self.active = self.ops.iter().any(|op| op.is_active());
    }

    /// Update operator parameters (call from UI thread before render).
    pub fn update_params(&mut self, params: &FmParams, sample_rate: u32) {
        self.algorithm = params.algorithm;
        self.feedback = params.feedback;
        for (i, op) in self.ops.iter_mut().enumerate() {
            op.ratio = ratio_from_normalized(params.op_ratio[i]);
            op.detune = (params.op_detune[i] - 0.5) * 20.0; // ±10 Hz
            op.waveform = Waveform::from_index(params.op_waveform[i]);
            op.level = params.op_level[i];
            if self.active {
                op.set_frequency(self.base_freq, sample_rate);
            }
        }
    }
}

use super::note_to_freq;

// ── FM Parameters ───────────────────────────────────────────────────

/// Parameters for the FM engine, stored in ParamSnapshot.
#[derive(Clone, Copy, Debug)]
pub struct FmParams {
    /// Algorithm index (0-7)
    pub algorithm: u8,
    /// Feedback amount (normalized 0..1)
    pub feedback: f32,
    /// Per-operator coarse ratio (normalized, maps to harmonic table)
    pub op_ratio: [f32; 4],
    /// Per-operator fine detune (normalized 0..1, center=0.5=no detune)
    pub op_detune: [f32; 4],
    /// Per-operator waveform index (0-7)
    pub op_waveform: [u8; 4],
    /// Per-operator output level (0..1)
    pub op_level: [f32; 4],
    /// Per-operator envelope parameters
    pub op_env: [EnvParams; 4],
}

impl Default for FmParams {
    fn default() -> Self {
        Self {
            algorithm: 0,
            feedback: 0.0,
            op_ratio: [
                0.2, // op1: 1.0x (modulator with feedback)
                0.2, // op2: 1.0x
                0.2, // op3: 1.0x
                0.2, // op4: 1.0x (carrier)
            ],
            op_detune: [0.5; 4],
            op_waveform: [0; 4],
            op_level: [0.0, 0.0, 0.0, 1.0], // only op4 (carrier) audible by default
            op_env: [EnvParams::default(); 4],
        }
    }
}
