use cortex_m::peripheral::{CPUID, MPU, SCB};
use stm32h7xx_hal::pac;

const DMA_REGION_BASE: u32 = 0x3000_0000;
pub(crate) const RBAR_VALID: u32 = 1 << 4;
// XN | AP = full access | TEX 001, C 0, B 0 (Normal, non-cacheable) | S 0 | SIZE 11 (4 KB) | ENABLE
const DMA_REGION_RASR: u32 = (1 << 28) | (0b011 << 24) | (0b001 << 19) | (11 << 1) | 1;
const _: () = assert!(DMA_REGION_RASR == 0x1308_0017);
// ENABLE | PRIVDEFENA: the default memory map everywhere else.
const MPU_CTRL: u32 = 0b101;

// Bank 2 (ADR 0137). Region 1 is all of it, 1 MB: no access, XN,
// strongly-ordered, so nothing reads or speculates into the stock
// bootloader's slots at 0x08180000+, ever. Region 2, numbered higher so it
// wins, is ours, S0-S3, in one of `bank2::Mpu2`'s states.
pub(crate) const BANK2_BASE: u32 = 0x0810_0000;
const XN: u32 = 1 << 28;
const AP_NONE: u32 = 0b000 << 24;
const AP_RO: u32 = 0b110 << 24;
const AP_RW: u32 = 0b011 << 24;
/// TEX 000 C 1 B 0: Normal, write-through, no write-allocate.
const NORMAL_WT: u32 = 1 << 17;
/// TEX 001 C 0 B 0: Normal, non-cacheable.
const NORMAL_NC: u32 = 0b001 << 19;
/// TEX 000 C 0 B 0.
const STRONGLY_ORDERED: u32 = 0;
const SIZE_1M: u32 = 19 << 1;
const SIZE_512K: u32 = 18 << 1;
const ENABLE: u32 = 1;
const BANK2_ALL_RASR: u32 = XN | AP_NONE | STRONGLY_ORDERED | SIZE_1M | ENABLE;
const _: () = assert!(BANK2_ALL_RASR == 0x1000_0027);
/// Read-only, never executed, never cached: the judge's reads and the
/// console's `flash` see the flash itself, with no cache maintenance.
pub(crate) const BANK2_READ_RASR: u32 = XN | AP_RO | NORMAL_NC | SIZE_512K | ENABLE;
const _: () = assert!(BANK2_READ_RASR == 0x1608_0025);
/// Read-write for the flash interface, never cached nor speculated.
pub(crate) const BANK2_WRITE_RASR: u32 = XN | AP_RW | STRONGLY_ORDERED | SIZE_512K | ENABLE;
const _: () = assert!(BANK2_WRITE_RASR == 0x1300_0025);
/// Read-only and executable: the door open.
pub(crate) const BANK2_OPEN_RASR: u32 = AP_RO | NORMAL_WT | SIZE_512K | ENABLE;
const _: () = assert!(BANK2_OPEN_RASR == 0x0602_0025);
pub(crate) const BANK2_REGION: u32 = 2;

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
    // running; regions 3-15 are disabled in case a bootloader left any,
    // region 0 covers exactly the linker-asserted 4 KB DMA block, and
    // regions 1 and 2 bank 2, where nothing bank 1 links lives (memory.x
    // ends FLASH at 0x08100000), so no code or data in use loses access.
    unsafe {
        mpu.ctrl.write(0);
        for region in 3..16 {
            mpu.rnr.write(region);
            mpu.rasr.write(0);
        }
        mpu.rnr.write(0);
        mpu.rbar.write(DMA_REGION_BASE | RBAR_VALID);
        mpu.rasr.write(DMA_REGION_RASR);
        mpu.rbar.write(BANK2_BASE | RBAR_VALID | 1);
        mpu.rasr.write(BANK2_ALL_RASR);
        mpu.rbar.write(BANK2_BASE | RBAR_VALID | BANK2_REGION);
        mpu.rasr.write(BANK2_READ_RASR);
        mpu.ctrl.write(MPU_CTRL);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    scb.enable_icache();
    scb.enable_dcache(cpuid);
}
