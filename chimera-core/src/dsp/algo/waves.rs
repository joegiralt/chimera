//! The generated wave tables (ADR 0023).

use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::dsp::algo::math::log2;
use crate::hw::SAMPLE_RATE;

include!(concat!(env!("OUT_DIR"), "/waves.rs"));

pub const WAVE_LEN: usize = 256;
pub const MIPS: usize = 8;
pub const WAVE_COUNT: usize = 16;
/// One period and a guard copy of its first sample.
pub type Table = [i16; WAVE_LEN + 1];

pub const WAVE_FLASH_BUDGET: usize = 64 * 1024 + WAVE_COUNT * MIPS * 2;
pub type Waves = [[Table; MIPS]; WAVE_COUNT];
const _: () = assert!(core::mem::size_of::<Waves>() <= WAVE_FLASH_BUDGET);

/// The tables `WaveId::table` reads: `WAVES` itself, or a RAM copy.
static ACTIVE: AtomicPtr<Waves> = AtomicPtr::new(ptr::addr_of!(WAVES).cast_mut());

/// Copies the tables into `ram` and reads them from there from then on, for
/// a target whose RAM is faster than its flash. Call before any audio runs.
pub fn copy_into(ram: &'static mut MaybeUninit<Waves>) {
    // SAFETY: both are one `Waves`, so the copy is in bounds and initialises
    // all of `ram`; `ram` is a unique `&mut`, so it cannot overlap the
    // immutable `WAVES`.
    unsafe { ptr::copy_nonoverlapping(ptr::addr_of!(WAVES), ram.as_mut_ptr(), 1) };
    let copy: &'static MaybeUninit<Waves> = ram;
    ACTIVE.store(copy.as_ptr().cast_mut(), Ordering::Release);
}

/// Mip 0's 127 harmonics stay under Nyquist up to this fundamental; each
/// mip doubles it.
pub const MIP0_TOP_HZ: f32 = SAMPLE_RATE as f32 / 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveId(u8);

impl WaveId {
    pub const W1: WaveId = WaveId(0);
    pub const W2: WaveId = WaveId(1);
    pub const W3: WaveId = WaveId(2);
    pub const W4: WaveId = WaveId(3);
    pub const W5: WaveId = WaveId(4);
    pub const W6: WaveId = WaveId(5);
    pub const W7: WaveId = WaveId(6);
    pub const W8: WaveId = WaveId(7);
    pub const TRI: WaveId = WaveId(8);
    pub const SAW: WaveId = WaveId(9);
    pub const SQR: WaveId = WaveId(10);
    pub const P25: WaveId = WaveId(11);
    pub const P12: WaveId = WaveId(12);
    pub const TSAW: WaveId = WaveId(13);
    pub const RSQR: WaveId = WaveId(14);
    pub const SSAW: WaveId = WaveId(15);

    pub const fn clamped(v: u8) -> Self {
        if (v as usize) < WAVE_COUNT {
            WaveId(v)
        } else {
            WaveId(WAVE_COUNT as u8 - 1)
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn name(self) -> &'static str {
        WAVE_NAMES[self.0 as usize]
    }

    pub fn table(self, mip: usize) -> &'static Table {
        // SAFETY: `ACTIVE` points at `WAVES` or at a finished copy, whose
        // only `&mut` was given up to `copy_into`; neither is written again.
        let waves = unsafe { &*ACTIVE.load(Ordering::Acquire) };
        &waves[self.0 as usize][mip.min(MIPS - 1)]
    }
}

/// Where `bandwidth_hz` falls among the mips, `0.0..=7.0`: mip `floor` and
/// the next both keep it under Nyquist (the ceiling of its octave, so
/// nothing folds down), crossfaded by the fraction.
pub fn mip_position(bandwidth_hz: f32) -> f32 {
    let half = 0.5 * MIP0_TOP_HZ;
    if bandwidth_hz > half {
        log2(bandwidth_hz / half).min((MIPS - 1) as f32)
    } else {
        0.0
    }
}

/// One block's move of a mip position from `from` toward `to`: the lower
/// mip of the pair holding `from`, and the crossfade's ends within it. The
/// position moves at most one mip per block, so it never jumps; a rising
/// bandwidth fades out the lower mip within the block.
pub fn mip_step(from: f32, to: f32) -> (usize, f32, f32) {
    let floor = from as usize;
    let lo = if to >= from || floor as f32 != from {
        floor
    } else {
        floor.saturating_sub(1)
    };
    let to = to.clamp(lo as f32, lo as f32 + 1.0);
    (lo, from - lo as f32, to - lo as f32)
}
