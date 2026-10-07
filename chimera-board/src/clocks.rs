#[cfg(not(feature = "sd-probe"))]
use chimera_core::clock_plan::{Pll3Config, PllRange, VcoRange};
use chimera_core::clock_plan::{SiliconRev, cycles_for_us};
use cortex_m::peripheral::{DCB, DWT};
use stm32h7xx_hal::pac;
use stm32h7xx_hal::prelude::*;
use stm32h7xx_hal::rcc::rec::Spi123ClkSel;
use stm32h7xx_hal::rcc::{Ccdr, PllConfigStrategy};

pub const HSE_HZ: u32 = 8_000_000;

#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    pub cpu_hz: u32,
    pub rev: SiliconRev,
}

pub fn read_rev(dbgmcu: &pac::DBGMCU) -> SiliconRev {
    SiliconRev::from_rev_id(dbgmcu.idc.read().rev_id().bits())
}

pub fn freeze(
    pwr: pac::PWR,
    rcc: pac::RCC,
    syscfg: &pac::SYSCFG,
    rev: SiliconRev,
) -> (Ccdr, Clocks) {
    let pwr = pwr.constrain();
    let pwrcfg = match rev {
        SiliconRev::V => pwr.vos0(syscfg).freeze(),
        SiliconRev::Y | SiliconRev::Unknown(_) => pwr.freeze(),
    };
    let cpu = rev.cpu_hz();
    let pclk = cpu / 4;
    let rcc = rcc
        .constrain()
        .use_hse(HSE_HZ.Hz())
        .sys_ck(cpu.Hz())
        .hclk((cpu / 2).Hz())
        .pclk1(pclk.Hz())
        .pclk2(pclk.Hz())
        .pclk3(pclk.Hz())
        .pclk4(pclk.Hz())
        .pll2_p_ck(100.MHz());
    let rcc = match rev {
        SiliconRev::V => rcc.pll1_strategy(PllConfigStrategy::Iterative),
        SiliconRev::Y | SiliconRev::Unknown(_) => rcc,
    };
    let mut ccdr = rcc.freeze(pwrcfg, syscfg);
    // SPI1/2/3 run from PLL2_P, which no core clock choice can move: the
    // display's 50 MHz is /2 and the SD's 25, 12.5 and 0.39 MHz are /4, /8
    // and /256 of it.
    ccdr.peripheral.kernel_spi123_clk_mux(Spi123ClkSel::Pll2P);
    assert!(ccdr.clocks.pll2_p_ck() == Some(100.MHz()));
    let cpu_hz = ccdr.clocks.c_ck().raw();
    (ccdr, Clocks { cpu_hz, rev })
}

pub fn delay_us(cpu_hz: u32, us: u32) {
    cortex_m::asm::delay(cycles_for_us(cpu_hz, us));
}

/// Starts the DWT cycle counter; false if it will not count.
pub fn enable_cycle_counter(dcb: &mut DCB, dwt: &mut DWT) -> bool {
    dcb.enable_trace();
    dwt.enable_cycle_counter();
    if counting() {
        return true;
    }
    // Without a debugger the M7's DWT can come up software-locked.
    DWT::unlock();
    dwt.enable_cycle_counter();
    counting()
}

fn counting() -> bool {
    let start = DWT::cycle_count();
    cortex_m::asm::delay(1_000);
    DWT::cycle_count() != start
}

#[cfg(not(feature = "sd-probe"))]
pub fn init_pll3(cfg: &Pll3Config) {
    // SAFETY: single-threaded init after the HAL's `freeze` (which leaves
    // PLL3 alone) and before any SAI runs; nothing else touches PLL3.
    let rcc = unsafe { &*pac::RCC::ptr() };
    rcc.cr.modify(|_, w| w.pll3on().off());
    while rcc.cr.read().pll3rdy().is_ready() {}
    rcc.pllckselr.modify(|_, w| w.divm3().bits(cfg.m));
    // SAFETY: DIVN3 = N − 1 with N in 4..=512 and DIVP3 = P − 1 with P in
    // 1..=128 (clock_plan tests); DIVQ3/DIVR3 = 1, their outputs stay off.
    rcc.pll3divr.write(|w| unsafe {
        w.divn3()
            .bits(cfg.n - 1)
            .divp3()
            .bits(cfg.p - 1)
            .divq3()
            .bits(1)
            .divr3()
            .bits(1)
    });
    // FRACN3 is latched when FRACEN goes from 0 to 1.
    rcc.pllcfgr.modify(|_, w| w.pll3fracen().reset());
    rcc.pll3fracr.write(|w| w.fracn3().bits(cfg.fracn));
    rcc.pllcfgr.modify(|_, w| {
        let w = match cfg.vco {
            VcoRange::Wide => w.pll3vcosel().wide_vco(),
            VcoRange::Medium => w.pll3vcosel().medium_vco(),
        };
        let w = match cfg.range {
            PllRange::R1To2 => w.pll3rge().range1(),
            PllRange::R2To4 => w.pll3rge().range2(),
            PllRange::R4To8 => w.pll3rge().range4(),
            PllRange::R8To16 => w.pll3rge().range8(),
        };
        w.pll3fracen().set().divp3en().enabled()
    });
    rcc.cr.modify(|_, w| w.pll3on().on());
    while !rcc.cr.read().pll3rdy().is_ready() {}
    rcc.d2ccip1r
        .modify(|_, w| w.sai1sel().pll3_p().sai23sel().pll3_p());
}
