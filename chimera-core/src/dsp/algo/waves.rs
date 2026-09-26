//! The generated wave tables (ADR 0023).

include!(concat!(env!("OUT_DIR"), "/waves.rs"));

pub const WAVE_LEN: usize = 256;
pub const MIPS: usize = 8;
pub const WAVE_COUNT: usize = 16;
pub type Table = [i16; WAVE_LEN];

pub const WAVE_FLASH_BUDGET: usize = 64 * 1024;
const _: () = assert!(core::mem::size_of::<[[Table; MIPS]; WAVE_COUNT]>() <= WAVE_FLASH_BUDGET);

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
        &WAVES[self.0 as usize][mip.min(MIPS - 1)]
    }
}
