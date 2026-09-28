use core::ptr::addr_of_mut;
use core::sync::atomic::AtomicU32;

use chimera_core::note_queue::{NoteEvent, NoteProducer};
use chimera_hal::midi::MidiParser;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use crate::priority::{self, Priority};

const BAUD: u32 = 31_250;

pub static ERRORS: AtomicU32 = AtomicU32::new(0);
static mut PARSER: MidiParser = MidiParser::new();
static mut DIN_NOTES: Option<NoteProducer<'static>> = None;

pub fn init(nvic: &mut NVIC, pclk2_hz: u32, notes: NoteProducer<'static>) {
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
    // SAFETY: USART1's interrupt is still masked, so nothing reads
    // `DIN_NOTES` yet; from then on only the handler does.
    unsafe { *addr_of_mut!(DIN_NOTES) = Some(notes) };
    priority::set_irq(nvic, pac::Interrupt::USART1, Priority::MIDI);
    // SAFETY: the handler touches only USART1, its own `PARSER` and
    // `DIN_NOTES`, the DIN queue's producer.
    unsafe { NVIC::unmask(pac::Interrupt::USART1) };
}

#[interrupt]
fn USART1() {
    // SAFETY: this handler owns USART1 after `init`, and `PARSER` and
    // `DIN_NOTES` are touched only here once `init` has unmasked it; the
    // handler can't preempt itself.
    let (usart, parser, notes) = unsafe {
        (
            &*pac::USART1::ptr(),
            &mut *addr_of_mut!(PARSER),
            &mut *addr_of_mut!(DIN_NOTES),
        )
    };
    loop {
        let isr = usart.isr.read();
        // An uncleared ORE with RXFNEIE set refires this interrupt forever.
        if isr.ore().bit_is_set() || isr.fe().bit_is_set() || isr.nf().bit_is_set() {
            usart
                .icr
                .write(|w| w.orecf().clear().fecf().clear().ncf().clear());
            crate::audio::dma::bump(&ERRORS, 1);
        }
        if isr.rxne().bit_is_clear() {
            break;
        }
        let byte = usart.rdr.read().rdr().bits() as u8;
        if let Some(ev) = parser.feed(byte).and_then(NoteEvent::from_midi)
            && let Some(notes) = notes.as_mut()
        {
            notes.push(ev);
        }
    }
}
