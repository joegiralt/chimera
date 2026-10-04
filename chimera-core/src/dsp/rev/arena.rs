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
        self.split(offset, len).1
    }

    /// The i16 slots below `offset`, the `len` f32s over the 2·`len` slots
    /// from `offset` (even), and the i16 slots above them: all three at
    /// once, for an engine that runs lines and a diffuser together.
    pub fn split(&mut self, offset: usize, len: usize) -> (&mut [i16], &mut [f32], &mut [i16]) {
        assert!(offset.is_multiple_of(2) && offset <= RING_LEN && len <= (RING_LEN - offset) / 2);
        let (lo, rest) = self.lines.split_at_mut(offset);
        let (mid, hi) = rest.split_at_mut(2 * len);
        // SAFETY: alignment: the arena is `align(4)` and `offset` is even,
        // so `mid` starts on a 4-byte boundary. Bounds: `mid` is exactly
        // 2·`len` i16s, the size of `len` f32s. Aliasing: `mid` is borrowed
        // mutably and only this f32 view of it outlives the call; `lo` and
        // `hi` are disjoint from it. Every bit pattern is a valid f32.
        let f = unsafe { core::slice::from_raw_parts_mut(mid.as_mut_ptr().cast::<f32>(), len) };
        (lo, f, hi)
    }

    /// Every slot zero: i16 0, and +0.0 in every f32 region.
    pub fn clear(&mut self) {
        self.lines.fill(0);
    }

    /// Part `i` of `of` zeroed: CLEAR's share of a block.
    pub fn clear_part(&mut self, i: u8, of: u8) {
        let n = RING_LEN.div_ceil(usize::from(of));
        if let Some(c) = self.lines.chunks_mut(n).nth(usize::from(i)) {
            c.fill(0);
        }
    }
}

impl Default for ReverbArena {
    fn default() -> Self {
        Self::new()
    }
}
