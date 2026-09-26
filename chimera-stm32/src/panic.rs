use core::panic::PanicInfo;

use cortex_m_rt::{ExceptionFrame, exception};
use stm32h7xx_hal::pac;

const GPIOE_BSRR: *mut u32 = 0x5802_1018 as *mut u32;
const LED_ON: u32 = 1 << 1;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    silence_and_halt()
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    silence_and_halt()
}

// Circular DMA keeps looping the last buffer after the CPU stops; with the
// SAI blocks disabled the DACs lose their clocks and go quiet instead.
fn silence_and_halt() -> ! {
    cortex_m::interrupt::disable();
    // SAFETY: interrupts are off and nothing runs after this; SAIEN is only
    // cleared on blocks whose APB2 clock is on, so no access faults, and
    // GPIOE_BSRR is PE's set/reset register (PE1 = LED).
    unsafe {
        let rcc = &*pac::RCC::ptr();
        let enabled = rcc.apb2enr.read();
        if enabled.sai1en().bit_is_set() {
            let sai1 = &*pac::SAI1::ptr();
            sai1.cha().cr1.modify(|_, w| w.saien().clear_bit());
            sai1.chb().cr1.modify(|_, w| w.saien().clear_bit());
        }
        if enabled.sai2en().bit_is_set() {
            (*pac::SAI2::ptr())
                .cha()
                .cr1
                .modify(|_, w| w.saien().clear_bit());
        }
        core::ptr::write_volatile(GPIOE_BSRR, LED_ON);
    }
    loop {
        cortex_m::asm::nop();
    }
}
