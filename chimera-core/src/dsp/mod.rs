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
        let x = t * 2.0 * core::f64::consts::PI;
        // Taylor series with enough terms for <0.001% error
        let x = x - (core::f64::consts::TAU * ((x / core::f64::consts::TAU + 0.5) as i64 as f64));
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
    t -= if t >= 0.0 {
        t as i32 as f32
    } else {
        (t as i32 - 1) as f32
    };
    let idx_f = t * 1024.0;
    let idx = idx_f as usize;
    let frac = idx_f - idx as f32;
    let a = SINE_LUT[idx & 1023];
    let b = SINE_LUT[(idx + 1) & 1023];
    a + (b - a) * frac
}

/// Xorshift32 white noise in [−1, 1]; a zero state stays zero, so seed it.
#[inline]
pub(crate) fn xorshift_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as i32) as f32 / i32::MAX as f32
}

/// sin(2π·phase) for phase in [0, 1): a parabola with one correction step
/// on the folded phase; absolute error below 0.001. `f32` only.
#[inline(always)]
pub fn sin_turns(phase: f32) -> f32 {
    let x = 2.0 * phase - 1.0;
    let y = 4.0 * x * (1.0 - x.abs());
    -(y + 0.224 * (y * y.abs() - y))
}

/// One block per side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stereo {
    pub l: [f32; chimera_hal::BLOCK_SIZE],
    pub r: [f32; chimera_hal::BLOCK_SIZE],
}

impl Stereo {
    pub const SILENT: Self = Self {
        l: [0.0; chimera_hal::BLOCK_SIZE],
        r: [0.0; chimera_hal::BLOCK_SIZE],
    };
}

/// Fast tanh: the Padé approximant x(27 + x²) / (27 + 9x²), clamped to ±1
/// beyond |x| = 3, where it meets ±1. One divide, no libm call. Measured
/// against tanh it is off by up to 0.0236 absolute (near |x| = 1.57) and
/// 2.6 % relative (near |x| = 1.45); within 0.004 for |x| ≤ 0.5.
#[inline(always)]
pub fn fast_tanh(x: f32) -> f32 {
    if x > 3.0 {
        1.0
    } else if x < -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

/// `tan(x)` for filter coefficients: a 5th-order Taylor series. Accurate
/// below about 10 kHz at 48 kHz; above that it reads low (a 20 kHz cutoff
/// comes out near 18.3 kHz, #57). ~5 cycles vs ~400 for `libm::tanf`.
#[inline(always)]
pub fn fast_tan(x: f32) -> f32 {
    // For small x (typical for filter cutoff: 0..π*0.49):
    // tan(x) ≈ x + x³/3 + 2x⁵/15
    let x2 = x * x;
    x * (1.0 + x2 * (1.0 / 3.0 + x2 * (2.0 / 15.0)))
}

/// `2^x`, exact at integers, relative error below 1e-6 (spec § Signal flow).
pub use self::algo::math::exp2 as fast_exp2;
/// `log2(x)` for `x > 0`, absolute error below 3e-5.
pub use self::algo::math::log2 as fast_log2;

pub mod algo;
pub mod chorus;
pub mod comp;
pub mod dc_blocker;
pub mod delay;
pub mod drive;
pub mod ease;
pub mod engines;
pub mod envelope;
pub mod filter;
pub mod fx_bus;
pub mod halfband;
pub mod lfo;
pub mod limiter;
pub mod modal;
pub mod modulator;
pub mod reverb;
pub mod ring;
pub mod tape;
pub mod voice;
pub mod wavefolder;
