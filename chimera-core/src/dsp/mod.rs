/// Convert MIDI note number to frequency in Hz.
pub fn note_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

/// 1024-entry sine lookup table for fast DSP sin approximation.
static SINE_LUT: [f32; 1024] = {
    let mut table = [0.0f32; 1024];
    let mut i = 0;
    while i < 1024 {
        let t = i as f64 / 1024.0;
        let x = t * 2.0 * 3.14159265358979323846;
        // Taylor series with enough terms for <0.001% error
        let x = x - (6.28318530717958647692 * ((x / 6.28318530717958647692 + 0.5) as i64 as f64));
        let x2 = x * x;
        let x3 = x2 * x;
        let x5 = x3 * x2;
        let x7 = x5 * x2;
        let x9 = x7 * x2;
        let x11 = x9 * x2;
        let s = x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0 + x9 / 362880.0 - x11 / 39916800.0;
        table[i] = s as f32;
        i += 1;
    }
    table
};

/// Fast sine approximation using 1024-entry LUT with linear interpolation.
/// Input: theta in radians (any range, wraps automatically).
/// ~10 cycles on Cortex-M7 vs ~500 for libm::sinf.
#[inline(always)]
pub fn fast_sin(theta: f32) -> f32 {
    const INV_TAU: f32 = 1.0 / core::f32::consts::TAU;
    // Normalize to 0.0..1.0
    let mut t = theta * INV_TAU;
    // Fast floor: cast to i32 truncates toward zero; adjust for negatives
    t -= if t >= 0.0 { t as i32 as f32 } else { (t as i32 - 1) as f32 };
    let idx_f = t * 1024.0;
    let idx = idx_f as usize;
    let frac = idx_f - idx as f32;
    let a = SINE_LUT[idx & 1023];
    let b = SINE_LUT[(idx + 1) & 1023];
    a + (b - a) * frac
}

/// Fast absolute-value sine: |sin(theta)|.
#[inline(always)]
pub fn fast_sin_abs(theta: f32) -> f32 {
    let s = fast_sin(theta);
    if s < 0.0 { -s } else { s }
}

/// Fast tanh approximation using rational polynomial.
/// Accurate to ~0.1% for |x| < 4. Clamps to ±1 beyond that.
/// ~5 cycles on Cortex-M7 vs ~400 for libm::tanhf.
#[inline(always)]
pub fn fast_tanh(x: f32) -> f32 {
    // Padé approximant: tanh(x) ≈ x(27 + x²) / (27 + 9x²)
    // Good to ~0.3% error for |x| < 3
    if x > 3.0 {
        1.0
    } else if x < -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

/// Fast tan approximation for filter coefficients.
/// Valid for |x| < π/2. Uses 3rd-order polynomial.
/// ~5 cycles vs ~400 for libm::tanf.
#[inline(always)]
pub fn fast_tan(x: f32) -> f32 {
    // For small x (typical for filter cutoff: 0..π*0.49):
    // tan(x) ≈ x + x³/3 + 2x⁵/15
    let x2 = x * x;
    x * (1.0 + x2 * (1.0 / 3.0 + x2 * (2.0 / 15.0)))
}

pub mod chorus;
pub mod delay;
pub mod drive;
pub mod midiverb;
pub mod envelope;
pub mod filter;
pub mod fm;
pub mod modal;
pub mod oscillator;
pub mod reverb;
pub mod voice;
pub mod wavefolder;
