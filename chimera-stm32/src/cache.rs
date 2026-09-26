use cortex_m::peripheral::{CPUID, MPU, SCB};
use stm32h7xx_hal::pac;

const DMA_REGION_BASE: u32 = 0x3000_0000;
const RBAR_VALID: u32 = 1 << 4;
// XN | AP = full access | TEX 001, C 0, B 0 (Normal, non-cacheable) | S 0 | SIZE 11 (4 KB) | ENABLE
const DMA_REGION_RASR: u32 = (1 << 28) | (0b011 << 24) | (0b001 << 19) | (11 << 1) | 1;
const _: () = assert!(DMA_REGION_RASR == 0x1308_0017);
// ENABLE | PRIVDEFENA: the default memory map everywhere else.
const MPU_CTRL: u32 = 0b101;

pub fn enable_d2_sram() {
    // SAFETY: read-modify-write of RCC_AHB2ENR's SRAM1/2/3EN before anything
    // touches D2 (ADR 0014); single-threaded, before the HAL owns RCC.
    let rcc = unsafe { &*pac::RCC::ptr() };
    rcc.ahb2enr.modify(|_, w| {
        w.sram1en()
            .enabled()
            .sram2en()
            .enabled()
            .sram3en()
            .enabled()
    });
    let _ = rcc.ahb2enr.read();
    cortex_m::asm::dsb();
}

pub fn init(mpu: &mut MPU, scb: &mut SCB, cpuid: &mut CPUID) {
    cortex_m::asm::dmb();
    // SAFETY: the MPU is reprogrammed with both caches still off and no DMA
    // running; region 0 covers exactly the linker-asserted 4 KB DMA block.
    unsafe {
        mpu.ctrl.write(0);
        mpu.rnr.write(0);
        mpu.rbar.write(DMA_REGION_BASE | RBAR_VALID);
        mpu.rasr.write(DMA_REGION_RASR);
        mpu.ctrl.write(MPU_CTRL);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    scb.enable_icache();
    scb.enable_dcache(cpuid);
}
