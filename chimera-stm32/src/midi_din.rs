use core::ptr::addr_of_mut;
use core::sync::atomic::{Ordering, compiler_fence};

use chimera_engine::midi_in::{CcRing, Routed, route};
use chimera_engine::note_queue::NoteProducer;
use chimera_hal::midi::MidiParser;
use cortex_m::peripheral::{DWT, NVIC};
use stm32h7xx_hal::pac::{self, interrupt};

use chimera_board::priority::{self, Priority};
use chimera_board::shared::{AUDIO_CYCLES, MIDI_CYCLES, MIDI_ERRORS as ERRORS, bump};

const BAUD: u32 = 31_250;

static mut PARSER: MidiParser = MidiParser::new();
static mut DIN_NOTES: Option<NoteProducer<'static>> = None;
/// The DIN CC ring; only the handler pushes.
static mut DIN_CCS: Option<&'static CcRing> = None;

pub fn init(nvic: &mut NVIC, pclk2_hz: u32, notes: NoteProducer<'static>, ccs: &'static CcRing) {
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
    // `DIN_NOTES` or `DIN_CCS` yet; from then on only the handler does.
    unsafe {
        *addr_of_mut!(DIN_NOTES) = Some(notes);
        *addr_of_mut!(DIN_CCS) = Some(ccs);
    }
    // The plain store above must not sink past the unmask's volatile write.
    compiler_fence(Ordering::SeqCst);
    priority::set_irq(nvic, pac::Interrupt::USART1, Priority::MIDI);
    // SAFETY: the handler touches only USART1, its own `PARSER`,
    // `DIN_NOTES`, the DIN queue's producer, and `DIN_CCS`, the DIN CC
    // ring it alone pushes to.
    unsafe { NVIC::unmask(pac::Interrupt::USART1) };
}

#[interrupt]
fn USART1() {
    // Its own time, for the card IRQ's (`card_dma::clocks`).
    let (t0, a0) = (DWT::cycle_count(), AUDIO_CYCLES.load(Ordering::Relaxed));
    // SAFETY: this handler owns USART1 after `init`, and `PARSER`,
    // `DIN_NOTES` and `DIN_CCS` are touched only here once `init` has
    // unmasked it; the handler can't preempt itself.
    let (usart, parser, notes, ccs) = unsafe {
        (
            &*pac::USART1::ptr(),
            &mut *addr_of_mut!(PARSER),
            &mut *addr_of_mut!(DIN_NOTES),
            *addr_of_mut!(DIN_CCS),
        )
    };
    loop {
        let isr = usart.isr.read();
        // An uncleared ORE with RXFNEIE set refires this interrupt forever.
        if isr.ore().bit_is_set() || isr.fe().bit_is_set() || isr.nf().bit_is_set() {
            usart
                .icr
                .write(|w| w.orecf().clear().fecf().clear().ncf().clear());
            bump(&ERRORS, 1);
        }
        if isr.rxne().bit_is_clear() {
            break;
        }
        let byte = usart.rdr.read().rdr().bits() as u8;
        match parser.feed(byte).map(route) {
            Some(Routed::Queue(ev)) => {
                if let Some(notes) = notes.as_mut() {
                    notes.push(ev);
                }
            }
            Some(Routed::Cc(ev)) => {
                if let Some(ccs) = ccs {
                    ccs.push(ev);
                }
            }
            _ => {}
        }
    }
    let audio = AUDIO_CYCLES.load(Ordering::Relaxed).wrapping_sub(a0);
    let own = DWT::cycle_count().wrapping_sub(t0).saturating_sub(audio);
    MIDI_CYCLES.fetch_add(own, Ordering::Relaxed);
}
