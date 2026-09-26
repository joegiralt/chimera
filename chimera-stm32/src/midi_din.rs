use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use chimera_core::note_queue::NoteEvent;
use chimera_hal::midi::MidiParser;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use crate::audio::engine::{DIN, NOTES};
use crate::priority::{self, Priority};

const BAUD: u32 = 31_250;

pub static ERRORS: AtomicU32 = AtomicU32::new(0);
static mut PARSER: MidiParser = MidiParser::new();

pub fn init(nvic: &mut NVIC, pclk2_hz: u32) {
    // SAFETY: single-threaded init before USART1's interrupt is unmasked;
    // RCC's APB2ENR is read-modify-written only from `main`, and USART1 is
    // used nowhere else.
    let (rcc, usart) = unsafe { (&*pac::RCC::ptr(), &*pac::USART1::ptr()) };
    rcc.apb2enr.modify(|_, w| w.usart1en().set_bit());
    let _ = rcc.apb2enr.read();
    // FIFOEN can only be written while UE is 0.
    usart.cr1.reset();
    usart.brr.write(|w| w.brr().bits((pclk2_hz / BAUD) as u16));
    usart
        .icr
        .write(|w| w.orecf().clear().fecf().clear().ncf().clear());
    usart
        .cr1
        .write(|w| w.fifoen().set_bit().rxneie().set_bit().re().set_bit());
    usart.cr1.modify(|_, w| w.ue().set_bit());
    priority::set_irq(nvic, pac::Interrupt::USART1, Priority::MIDI);
    // SAFETY: the handler touches only USART1, its own `PARSER` and the DIN
    // queue, whose single producer it is.
    unsafe { NVIC::unmask(pac::Interrupt::USART1) };
}

#[interrupt]
fn USART1() {
    // SAFETY: this handler owns USART1 after `init`, and `PARSER` is touched
    // only here; the handler can't preempt itself.
    let (usart, parser) = unsafe { (&*pac::USART1::ptr(), &mut *addr_of_mut!(PARSER)) };
    loop {
        let isr = usart.isr.read();
        // An uncleared ORE with RXFNEIE set refires this interrupt forever.
        if isr.ore().bit_is_set() || isr.fe().bit_is_set() || isr.nf().bit_is_set() {
            usart
                .icr
                .write(|w| w.orecf().clear().fecf().clear().ncf().clear());
            ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        if isr.rxne().bit_is_clear() {
            break;
        }
        let byte = usart.rdr.read().rdr().bits() as u8;
        if let Some(ev) = parser.feed(byte).and_then(NoteEvent::from_midi) {
            NOTES.source(DIN).push(ev);
        }
    }
}
