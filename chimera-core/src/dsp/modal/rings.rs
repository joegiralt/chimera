// The SVF bandpasses, fast tangent, cosine oscillator and stiffness table follow
// Mutable Instruments Rings and stmlib (ADR 0032):
//
// Copyright 2014-2015 Emilie Gillet.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

//! The modal bank's parts that follow Mutable Instruments Rings and stmlib
//! (ADR 0032): the SVF bandpass, fast tangent, cosine oscillator and
//! stiffness table.

// ── SVF Bandpass (ZDF topology, matching Rings/stmlib) ──────────────

#[derive(Clone, Copy, Debug)]
pub(super) struct Svf {
    state_1: f32,
    state_2: f32,
    g: f32, // tan(pi * f)
    r: f32, // 1/Q
    h: f32, // 1 / (1 + r*g + g*g)
}

impl Svf {
    pub(super) fn new() -> Self {
        Self {
            state_1: 0.0,
            state_2: 0.0,
            g: 0.0,
            r: 1.0,
            h: 1.0,
        }
    }

    /// Configure filter. `freq` = normalized frequency (Hz/sr), `resonance` = Q.
    pub(super) fn set(&mut self, freq: f32, resonance: f32) {
        self.g = tan_approx(freq);
        self.r = 1.0 / resonance.max(0.5);
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    /// Silent, its tuning kept.
    pub(super) fn reset(&mut self) {
        self.state_1 = 0.0;
        self.state_2 = 0.0;
    }

    /// `tan(π·f)` as set: for the tests.
    #[cfg(test)]
    pub(super) fn g(&self) -> f32 {
        self.g
    }

    /// Process one sample, return bandpass output.
    #[inline]
    pub(super) fn process_bp(&mut self, input: f32) -> f32 {
        let hp = (input - self.r * self.state_1 - self.g * self.state_1 - self.state_2) * self.h;
        let bp = self.g * hp + self.state_1;
        self.state_1 = self.g * hp + bp;
        let lp = self.g * bp + self.state_2;
        self.state_2 = self.g * bp + lp;
        bp
    }

    /// `process_bp` at unity gain at its peak (Rings'
    /// `FILTER_MODE_BAND_PASS_NORMALIZED`).
    #[inline]
    pub(super) fn process_bp_normalized(&mut self, input: f32) -> f32 {
        self.r * self.process_bp(input)
    }
}

/// Fast tangent approximation (matches Rings' FREQUENCY_FAST).
pub(super) fn tan_approx(f: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let f2 = f * f;
    f * (pi + f2 * (0.326 * pi * pi * pi + 0.1823 * pi * pi * pi * pi * pi * f2))
}

// ── Cosine Oscillator (position weighting, matching Rings) ──────────

pub(super) struct CosineOsc {
    y0: f32,
    y1: f32,
    iir_coefficient: f32,
    initial_amplitude: f32,
}

impl CosineOsc {
    pub(super) fn new() -> Self {
        Self {
            y0: 0.0,
            y1: 0.0,
            iir_coefficient: 0.0,
            initial_amplitude: 0.0,
        }
    }

    /// Initialize with position (0..1).
    pub(super) fn init(&mut self, position: f32) {
        let mut sign = 16.0_f32;
        let mut freq = position - 0.25;
        if freq < 0.0 {
            freq = -freq;
        } else if freq > 0.5 {
            freq -= 0.5;
        } else {
            sign = -16.0;
        }
        self.iir_coefficient = sign * freq * (1.0 - 2.0 * freq);
        self.initial_amplitude = self.iir_coefficient * 0.25;
    }

    /// Reset to start of sequence.
    pub(super) fn start(&mut self) {
        self.y1 = self.initial_amplitude;
        self.y0 = 0.5;
    }

    /// Get next amplitude weight (call once per mode).
    #[inline]
    pub(super) fn next(&mut self) -> f32 {
        let temp = self.y0;
        self.y0 = self.iir_coefficient * self.y0 - self.y1;
        self.y1 = temp;
        temp + 0.5 // shift to [0, 1]
    }
}

// ── Stiffness lookup table ──────────────────────────────────────────
// Maps structure (0..1) to stiffness coefficient.
// 0.0 → -0.0625 (compressed, tube-like)
// ~0.25 → 0.0 (perfect harmonics)
// 1.0 → 2.0 (very stretched, metallic)

/// Approximate lut_stiffness: structure (0..1) → stiffness coefficient.
/// Rings: ranges from -0.0625 to +2.0 with a zero plateau at ~0.25.
pub(super) fn stiffness_from_structure(structure: f32) -> f32 {
    if structure < 0.24 {
        // Negative stiffness (compressed partials, tube-like)
        -0.0625 * (1.0 - structure / 0.24)
    } else if structure < 0.3 {
        // Harmonic plateau (perfect string)
        0.0
    } else {
        // Stretch ramp: 0 to 0.5 (with stiffness decay in the loop,
        // this produces moderate to heavy inharmonicity without
        // pushing all modes past Nyquist)
        let t = (structure - 0.3) / 0.7;
        t * t * 0.5
    }
}
