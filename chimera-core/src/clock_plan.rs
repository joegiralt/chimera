//! STM32H750 PLL3/SAI clock planning (ADR 0013): pure arithmetic, no
//! register access. `chimera-stm32` reads `DBGMCU_IDC.REV_ID` and writes
//! these values; this module only computes them.

use crate::hw::{CPU_HZ_REV_V, CPU_HZ_REV_Y};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiliconRev {
    Y,
    V,
    Unknown(u16),
}

impl SiliconRev {
    pub const REV_ID_Y: u16 = 0x1003;
    pub const REV_ID_V: u16 = 0x2003;

    pub const fn from_rev_id(id: u16) -> Self {
        match id {
            Self::REV_ID_Y => SiliconRev::Y,
            Self::REV_ID_V => SiliconRev::V,
            other => SiliconRev::Unknown(other),
        }
    }

    pub const fn cpu_hz(self) -> u32 {
        match self {
            SiliconRev::V => CPU_HZ_REV_V,
            SiliconRev::Y | SiliconRev::Unknown(_) => CPU_HZ_REV_Y,
        }
    }

    // Rev B and later (REV_ID >= 0x2000, the ST HAL's test) have MCKEN and
    // FS = ker / (MCKDIV x 256); rev Y halves MCLK: ker / (2 x MCKDIV).
    pub const fn new_sai(self) -> bool {
        match self {
            SiliconRev::V => true,
            SiliconRev::Y => false,
            SiliconRev::Unknown(id) => id >= 0x2000,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            SiliconRev::V => "V",
            SiliconRev::Y => "Y",
            SiliconRev::Unknown(_) => "?",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PllRange {
    R1To2,
    R2To4,
    R4To8,
    R8To16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VcoRange {
    Wide,
    Medium,
}

// `n` and `p` are divide ratios; the registers hold ratio - 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pll3Config {
    pub m: u8,
    pub n: u16,
    pub fracn: u16,
    pub p: u8,
    pub range: PllRange,
    pub vco: VcoRange,
    pub mckdiv: u8,
}

pub const SAI_KER_PER_FS: u32 = 1024;
const VCO_TARGET_HZ: u64 = 400_000_000;
const FRACN_ONE: u64 = 8192;

pub const fn pll3_for(hse_hz: u32, fs_hz: u32, rev: SiliconRev) -> Pll3Config {
    let ker = fs_hz as u64 * SAI_KER_PER_FS as u64;
    let m = (hse_hz as u64).div_ceil(16_000_000);
    let ref_hz = hse_hz as u64 / m;
    let p = (VCO_TARGET_HZ + ker / 2) / ker;
    // N + FRACN/8192 = ker x P / ref, rounded to the nearest 1/8192.
    let x = (ker * p * FRACN_ONE * 2 + ref_hz) / (2 * ref_hz);
    let range = if ref_hz >= 8_000_000 {
        PllRange::R8To16
    } else if ref_hz >= 4_000_000 {
        PllRange::R4To8
    } else if ref_hz >= 2_000_000 {
        PllRange::R2To4
    } else {
        PllRange::R1To2
    };
    let mckdiv = if rev.new_sai() {
        SAI_KER_PER_FS / 256
    } else {
        SAI_KER_PER_FS / 512
    };
    Pll3Config {
        m: m as u8,
        n: (x / FRACN_ONE) as u16,
        fracn: (x % FRACN_ONE) as u16,
        p: p as u8,
        range,
        vco: VcoRange::Wide,
        mckdiv: mckdiv as u8,
    }
}

pub fn vco_hz(c: &Pll3Config, hse_hz: u32) -> f64 {
    hse_hz as f64 / c.m as f64 * (c.n as f64 + c.fracn as f64 / FRACN_ONE as f64)
}

pub fn fs_of(c: &Pll3Config, hse_hz: u32, rev: SiliconRev) -> f64 {
    let ker = vco_hz(c, hse_hz) / c.p as f64;
    let div = match (c.mckdiv, rev.new_sai()) {
        (0, _) => 1.0,
        (d, true) => d as f64,
        (d, false) => 2.0 * d as f64,
    };
    ker / (div * 256.0)
}

pub const fn cycles_for_us(cpu_hz: u32, us: u32) -> u32 {
    (cpu_hz as u64 * us as u64 / 1_000_000) as u32
}

pub const fn cycles_for_ns(cpu_hz: u32, ns: u32) -> u32 {
    (cpu_hz as u64 * ns as u64).div_ceil(1_000_000_000) as u32
}

pub const SYSTICK_MAX_RELOAD: u32 = 0x00FF_FFFF;

pub const fn systick_reload(cpu_hz: u32, tick_hz: u32) -> u32 {
    cpu_hz / tick_hz - 1
}
