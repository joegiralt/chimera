//! The one i16 arena every reverb TYPE lays its lines in (reverb TYPEs
//! spec § Arena), with f32 regions for R3's diffusers and R4's
//! decorrelators.

use crate::dsp::ring::RING_LEN;

#[repr(C, align(4))]
pub struct ReverbArena {
    lines: [i16; RING_LEN],
}

const _: () = assert!(RING_LEN.is_multiple_of(2));

impl ReverbArena {
    pub const fn new() -> Self {
        Self {
            lines: [0; RING_LEN],
        }
    }

    pub fn lines(&self) -> &[i16; RING_LEN] {
        &self.lines
    }

    pub fn lines_mut(&mut self) -> &mut [i16; RING_LEN] {
        &mut self.lines
    }

    /// `len` f32s over the 2·`len` slots from `offset`, which is even.
    pub fn f32_region(&mut self, offset: usize, len: usize) -> &mut [f32] {
        assert!(offset.is_multiple_of(2) && offset <= RING_LEN && len <= (RING_LEN - offset) / 2);
        // SAFETY: alignment: the arena is `align(4)` and `offset` is even,
        // so the region starts on a 4-byte boundary. Bounds: `offset +
        // 2·len ≤ RING_LEN`, asserted above, so the f32s lie inside
        // `lines`. Aliasing: `&mut self` is held for the slice's life, so
        // no i16 view of it is alive. Every bit pattern is a valid f32.
        unsafe {
            core::slice::from_raw_parts_mut(self.lines.as_mut_ptr().add(offset).cast::<f32>(), len)
        }
    }

    /// Every slot zero: i16 0, and +0.0 in every f32 region.
    pub fn clear(&mut self) {
        self.lines.fill(0);
    }
}

impl Default for ReverbArena {
    fn default() -> Self {
        Self::new()
    }
}
