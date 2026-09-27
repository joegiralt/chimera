//! The 35-tap half-band FIR (FX diet spec § Rate): 2:1 into the reverb
//! ring, 1:2 back out.

use chimera_hal::BLOCK_SIZE;

/// Ring samples per block.
pub const HALF: usize = BLOCK_SIZE / 2;

/// h[±1], h[±3] … h[±17]. The centre tap is 0.5; every other even tap is 0.
pub const ODD_TAPS: [f32; 9] = [
    0.316_963, -0.102_118, 0.057_184, -0.036_748, 0.024_702, -0.016_701, 0.011_078, -0.007_044,
    0.006_066,
];

const DEC_HIST: usize = 34;
const INT_HIST: usize = 17;

pub struct Decimator {
    hist: [f32; DEC_HIST],
}

impl Decimator {
    pub const fn new() -> Self {
        Self {
            hist: [0.0; DEC_HIST],
        }
    }

    pub fn process(&mut self, x: &[f32; BLOCK_SIZE], y: &mut [f32; HALF]) {
        let mut buf = [0.0f32; DEC_HIST + BLOCK_SIZE];
        buf[..DEC_HIST].copy_from_slice(&self.hist);
        buf[DEC_HIST..].copy_from_slice(x);
        for (m, out) in y.iter_mut().enumerate() {
            let c = 2 * m + 18;
            let mut acc = 0.5 * buf[c];
            for (j, &h) in ODD_TAPS.iter().enumerate() {
                acc += h * (buf[c - 2 * j - 1] + buf[c + 2 * j + 1]);
            }
            *out = acc;
        }
        self.hist.copy_from_slice(&buf[BLOCK_SIZE..]);
    }
}

impl Default for Decimator {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Interpolator {
    hist: [f32; INT_HIST],
}

impl Interpolator {
    pub const fn new() -> Self {
        Self {
            hist: [0.0; INT_HIST],
        }
    }

    /// Zero-stuffed with gain 2: even outputs are the centre tap's copy, odd
    /// outputs the odd taps' sum.
    pub fn process(&mut self, z: &[f32; HALF], y: &mut [f32; BLOCK_SIZE]) {
        let mut buf = [0.0f32; INT_HIST + HALF];
        buf[..INT_HIST].copy_from_slice(&self.hist);
        buf[INT_HIST..].copy_from_slice(z);
        for p in 0..HALF {
            let c = p + 8;
            let mut acc = 0.0;
            for (j, &h) in ODD_TAPS.iter().enumerate() {
                acc += h * (buf[c - j] + buf[c + 1 + j]);
            }
            y[2 * p] = buf[c];
            y[2 * p + 1] = 2.0 * acc;
        }
        self.hist.copy_from_slice(&buf[HALF..]);
    }
}

impl Default for Interpolator {
    fn default() -> Self {
        Self::new()
    }
}

crate::in_place::field_list!(Decimator => Decimator { hist });
crate::in_place::field_list!(Interpolator => Interpolator { hist });
