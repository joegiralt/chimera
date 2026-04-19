#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;
use stm32h7xx_hal::pac;

#[entry]
fn main() -> ! {
    let _dp = pac::Peripherals::take().unwrap();

    unsafe {
        // GPIOE clock enable (RCC AHB4ENR bit 4)
        let rcc_ahb4enr = 0x580244E0 as *mut u32;
        core::ptr::write_volatile(rcc_ahb4enr, core::ptr::read_volatile(rcc_ahb4enr) | (1 << 4));
        cortex_m::asm::delay(100);

        // PE1 as output (MODER bits [3:2] = 01)
        let gpioe_moder = 0x58021000 as *mut u32;
        let val = core::ptr::read_volatile(gpioe_moder);
        core::ptr::write_volatile(gpioe_moder, (val & !(3 << 2)) | (1 << 2));

        let gpioe_bsrr = 0x58021018 as *mut u32;

        loop {
            core::ptr::write_volatile(gpioe_bsrr, 1 << 1); // set PE1
            cortex_m::asm::delay(16_000_000);
            core::ptr::write_volatile(gpioe_bsrr, 1 << (1 + 16)); // reset PE1
            cortex_m::asm::delay(16_000_000);
        }
    }
}
