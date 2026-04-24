/// FM synthesis lookup tables and math functions.
///
/// Ported from p81z (TX81Z_extra.cpp). Covers:
///   - Frequency ratio tables (coarse + fine)
///   - Level-to-gain conversion (operator output level)
///   - D1L (first decay level) conversion
///   - Feedback factors
///   - KVS (key velocity sensitivity) polynomial coefficients
///   - Rate scaling
///   - Envelope rate factors

/// Coarse frequency ratios (index 0–63), corresponding to the TX81Z ratio table.
pub static FREQ_RATIOS: [f32; 64] = [
    0.50, 0.71, 0.78, 0.87, 1.00, 1.41, 1.57, 1.73,
    2.00, 2.82, 3.00, 3.14, 3.46, 4.00, 4.24, 4.71,
    5.00, 5.19, 5.65, 6.00, 6.28, 6.92, 7.00, 7.07,
    7.85, 8.00, 8.48, 8.65, 9.00, 9.42, 9.89, 10.00,
    10.38, 10.99, 11.00, 11.30, 12.00, 12.11, 12.56, 12.72,
    13.00, 13.84, 14.00, 14.10, 14.13, 15.00, 15.55, 15.57,
    15.70, 16.96, 17.27, 17.30, 18.37, 18.84, 19.03, 19.78,
    20.41, 20.76, 21.20, 21.98, 22.49, 23.55, 24.22, 25.95,
];

/// Upper bound for fine-tuning interpolation per coarse ratio index.
pub static FREQ_RATIOS_MAX: [f32; 64] = [
    0.93, 1.32, 1.37, 1.62, 1.93, 2.73, 3.04, 3.35,
    2.93, 4.14, 3.93, 4.61, 5.08, 4.93, 5.55, 6.18,
    5.93, 6.81, 6.96, 6.93, 7.75, 8.54, 7.93, 8.37,
    9.32, 8.93, 9.78, 10.27, 9.93, 10.89, 11.19, 10.93,
    12.00, 12.46, 11.93, 12.60, 12.93, 13.73, 14.03, 14.01,
    13.93, 15.46, 14.93, 15.42, 15.60, 15.93, 16.83, 17.19,
    17.17, 18.24, 18.74, 18.92, 19.65, 20.31, 20.65, 21.06,
    21.88, 22.38, 22.47, 23.45, 24.11, 25.02, 25.84, 27.57,
];

/// Feedback amount per feedback level (0–7).
pub static FEEDBACK: [f32; 8] = [0.0, 0.008, 0.015, 0.024, 0.07, 0.12, 0.19, 0.26];

/// Global operator scaling factor.
pub const OPERATOR_FACTOR: f32 = 4.0;

// ── KVS polynomial coefficients (degree-4 polynomial in normalised velocity) ──

static KVS_F1: [f32; 8] = [0.0, -34.2, -59.6, -110.5, -145.5, -184.7, -147.4, -98.8];
static KVS_F2: [f32; 8] = [0.0, 83.9, 146.5, 266.3, 351.8, 447.4, 366.2, 259.8];
static KVS_F3: [f32; 8] = [0.0, -76.2, -135.9, -236.0, -313.0, -399.4, -346.2, -274.3];
static KVS_F4: [f32; 8] = [0.0, 36.7, 69.2, 110.7, 147.4, 188.2, 185.8, 178.0];
static KVS_F5: [f32; 8] = [0.0, -15.5, -24.8, -34.2, -43.7, -53.8, -59.7, -64.9];

// ── Rate scaling bounds ──

static RS_LOWER: [f32; 4] = [-0.5, -0.5, 0.0, 0.5];
static RS_UPPER: [f32; 4] = [1.0, 3.0, 7.0, 15.0];

/// MIDI note range for rate-scaling interpolation.
const RS_NOTE_LO: f32 = 28.0;
const RS_NOTE_HI: f32 = 110.0;

// ─────────────────────────────────────────────────────────────────────────────
// Public functions
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the frequency ratio for a given coarse (0–63) and fine (0–15) index.
///
/// For coarse < 4 the fine parameter is clamped to 0–7 (only 8 steps).
/// Result is linearly interpolated between `FREQ_RATIOS[coarse]` and
/// `FREQ_RATIOS_MAX[coarse]`, then clamped to the max.
#[inline]
pub fn compute_ratio(coarse: usize, fine: usize) -> f32 {
    let fine = if coarse < 4 { fine.min(7) } else { fine };
    let min = FREQ_RATIOS[coarse];
    let max = FREQ_RATIOS_MAX[coarse];
    let steps = if coarse < 4 { 7.0_f32 } else { 15.0_f32 };
    (min + (max - min) / steps * fine as f32).min(max)
}

/// Convert dB to a linear gain value.
///
/// Uses the half-dB convention from p81z: `10^(db/20)`.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    libm::powf(10.0_f32, db * 0.05)
}

/// Convert operator output level (0–99) to a linear gain.
///
/// Maps level 0 to a very small (but non-zero) gain and level 99 to near unity.
#[inline]
pub fn level_to_gain(level: u8) -> f32 {
    db_to_gain(0.74 * (level as f32 + 1.0) - 73.26)
}

/// Convert first-decay level (D1L, 0–15) to a linear level.
///
/// D1L=0 means the decay goes all the way to zero. D1L=15 means sustain at
/// the peak level. Intermediate values follow a -3 dB per step curve.
#[inline]
pub fn d1l_to_level(d1l: u8) -> f32 {
    if d1l == 0 {
        0.0
    } else {
        db_to_gain(-3.0 * (15 - d1l) as f32)
    }
}

/// Compute per-note velocity-sensitivity gain factor.
///
/// `kvs` is 0–7 (TX81Z key velocity sensitivity setting).
/// `velocity` is normalised 0.0–1.0.
/// Returns a linear amplitude factor. When `kvs == 0`, always returns 1.0.
#[inline]
pub fn compute_velocity_factor(kvs: usize, velocity: f32) -> f32 {
    let v = velocity;
    let db = KVS_F1[kvs] * v * v * v * v
        + KVS_F2[kvs] * v * v * v
        + KVS_F3[kvs] * v * v
        + KVS_F4[kvs] * v
        + KVS_F5[kvs];
    db_to_gain(db)
}

/// Compute rate-scaling offset for a given `rs` setting (0–3) and MIDI `note`.
///
/// Returns a semitone-like offset that is added to the envelope rate before
/// computing attack/decay factors.
#[inline]
pub fn compute_rate_scaling(rs: usize, note: u8) -> f32 {
    let t = ((note as f32 - RS_NOTE_LO) / (RS_NOTE_HI - RS_NOTE_LO)).clamp(0.0, 1.0);
    RS_LOWER[rs] + (RS_UPPER[rs] - RS_LOWER[rs]) * t
}

/// Compute the per-sample attack increment for an envelope stage.
///
/// The envelope level rises from 0 to 1 in `1/attack_increment` samples.
/// `rate` is the raw envelope rate (0–31 in TX81Z terms, or similar).
/// `rs_offset` comes from `compute_rate_scaling`.
/// `sample_rate` is in Hz (e.g. 48000.0).
#[inline]
pub fn attack_increment(rate: f32, rs_offset: f32, sample_rate: f32) -> f32 {
    1.0 / (sample_rate * libm::powf(2.0_f32, 3.5 - 0.5 * (rate + rs_offset)))
}

/// Compute the per-sample multiplicative decay factor for an envelope stage.
///
/// Multiply the current envelope level by this factor every sample to get an
/// exponential decay that reaches 1/16 of its initial value in the desired time.
/// `rate` is the raw decay rate (0–31). `rs_offset` comes from `compute_rate_scaling`.
#[inline]
pub fn decay_factor(rate: f32, rs_offset: f32, sample_rate: f32) -> f32 {
    let ln16 = libm::logf(16.0_f32);
    libm::expf(-ln16 / (sample_rate * libm::powf(2.0_f32, 5.5 - 0.5 * (rate + rs_offset))))
}
