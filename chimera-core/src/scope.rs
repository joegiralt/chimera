//! Oscilloscope buffer: `ScopeWriter` accumulates audio-thread blocks into a
//! back buffer twice the display width, finds a rising zero-crossing trigger
//! point once it fills, and publishes the triggered window through a
//! `TripleBuffer`. The UI thread reads the latest published frame.

use crate::triple::{TripleBuffer, Writer};

/// Display width in samples.
pub const SCOPE_LEN: usize = 240;
/// Back buffer is larger to allow trigger search.
const BACK_LEN: usize = SCOPE_LEN * 2;

pub type ScopeFrame = [f32; SCOPE_LEN];

pub const fn scope_buffer() -> TripleBuffer<ScopeFrame> {
    TripleBuffer::new([0.0; SCOPE_LEN], [0.0; SCOPE_LEN], [0.0; SCOPE_LEN])
}

pub struct ScopeWriter {
    back: [f32; BACK_LEN],
    pos: usize,
    out: Writer<ScopeFrame>,
}

impl ScopeWriter {
    pub fn new(out: Writer<ScopeFrame>) -> Self {
        Self {
            back: [0.0; BACK_LEN],
            pos: 0,
            out,
        }
    }

    /// Called by the audio thread after each block render.
    pub fn write(&mut self, samples: &[f32]) {
        for &s in samples {
            if self.pos < BACK_LEN {
                self.back[self.pos] = s;
                self.pos += 1;
            }
        }
        if self.pos >= BACK_LEN {
            let trigger = (1..BACK_LEN - SCOPE_LEN)
                .find(|&i| self.back[i - 1] <= 0.0 && self.back[i] > 0.0)
                .unwrap_or(0);
            let back = &self.back;
            self.out
                .publish(|f| f.copy_from_slice(&back[trigger..trigger + SCOPE_LEN]));
            self.pos = 0;
        }
    }
}

/// Largest |sample| in a scope buffer (or a slice of one).
pub fn peak(buf: &[f32]) -> f32 {
    buf.iter()
        .fold(0.0f32, |m, &s| m.max(if s < 0.0 { -s } else { s }))
}

/// Below this peak the output counts as silent (the header dot is off).
pub const SOUNDING_PEAK: f32 = 1.0e-3;
