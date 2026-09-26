use chimera_core::clock_plan::{SiliconRev, cycles_for_us};
use stm32h7xx_hal::pac;
use stm32h7xx_hal::prelude::*;
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
        .pll1_q_ck(200.MHz());
    let rcc = match rev {
        SiliconRev::V => rcc.pll1_strategy(PllConfigStrategy::Iterative),
        SiliconRev::Y | SiliconRev::Unknown(_) => rcc,
    };
    let ccdr = rcc.freeze(pwrcfg, syscfg);
    let cpu_hz = ccdr.clocks.c_ck().raw();
    (ccdr, Clocks { cpu_hz, rev })
}

pub fn delay_us(cpu_hz: u32, us: u32) {
    cortex_m::asm::delay(cycles_for_us(cpu_hz, us));
}
