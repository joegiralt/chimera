use cortex_m::peripheral::scb::SystemHandler;
use cortex_m::peripheral::{NVIC, SCB};
use stm32h7xx_hal::pac;

// The H7 keeps the upper 4 bits of each priority byte: level n is n << 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priority(u8);

impl Priority {
    pub const AUDIO: Priority = Priority::level(0);
    #[cfg(feature = "midi-din")]
    pub const MIDI: Priority = Priority::level(4);
    pub const SYSTICK: Priority = Priority::level(15);

    const fn level(level: u8) -> Self {
        assert!(level < 16);
        Priority(level << 4)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

const _: () = assert!(Priority::AUDIO.bits() == 0x00 && Priority::SYSTICK.bits() == 0xF0);
#[cfg(feature = "midi-din")]
const _: () = assert!(Priority::MIDI.bits() == 0x40);

pub fn set_irq(nvic: &mut NVIC, irq: pac::Interrupt, p: Priority) {
    // SAFETY: priorities can break priority-based critical sections; this
    // firmware has none (it shares state through atomics and lock-free
    // buffers), and each interrupt is set before it is unmasked.
    unsafe { nvic.set_priority(irq, p.bits()) };
    debug_assert_eq!(NVIC::get_priority(irq), p.bits());
}

pub fn set_systick(scb: &mut SCB, p: Priority) {
    // SAFETY: as in `set_irq`.
    unsafe { scb.set_priority(SystemHandler::SysTick, p.bits()) };
    debug_assert_eq!(SCB::get_priority(SystemHandler::SysTick), p.bits());
}
