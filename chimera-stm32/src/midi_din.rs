use core::ptr::addr_of_mut;
use core::sync::atomic::{Ordering, compiler_fence};

use chimera_engine::midi_in::{self, CtrlRing, DinSinks, PipeSlot};
use chimera_engine::note_queue::NoteProducer;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use chimera_board::priority::{self, Priority};
use chimera_board::shared::{
    AUDIO_CYCLES, MIDI_CYCLES, MIDI_DROPS, MIDI_ERRORS as ERRORS, MIDI_FAULTS, bump, stamp,
};

const BAUD: u32 = 31_250;

/// The RX FIFO's depth: one ISR entry drains at most this many bytes into
/// one door call.
const RX_FIFO: usize = 16;

/// The DIN pipe's slot; only the handler feeds it (the door builds it).
static mut PIPE: PipeSlot = PipeSlot::new();
static mut DIN_NOTES: Option<NoteProducer<'static>> = None;
/// The DIN control ring; only the handler pushes.
static mut DIN_CTRL: Option<&'static CtrlRing> = None;

pub fn init(nvic: &mut NVIC, pclk2_hz: u32, notes: NoteProducer<'static>, ctrl: &'static CtrlRing) {
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
    // `DIN_NOTES` or `DIN_CTRL` yet; from then on only the handler does.
    unsafe {
        *addr_of_mut!(DIN_NOTES) = Some(notes);
        *addr_of_mut!(DIN_CTRL) = Some(ctrl);
    }
    // The plain store above must not sink past the unmask's volatile write.
    compiler_fence(Ordering::SeqCst);
    priority::set_irq(nvic, pac::Interrupt::USART1, Priority::MIDI);
    // SAFETY: the handler touches only USART1, its own `PIPE`,
    // `DIN_NOTES`, the DIN queue's producer, and `DIN_CTRL`, the DIN
    // control ring it alone pushes to.
    unsafe { NVIC::unmask(pac::Interrupt::USART1) };
}

fn audio_cycles() -> u32 {
    AUDIO_CYCLES.load(Ordering::Relaxed)
}

#[interrupt]
fn USART1() {
    // Its own time, for the card IRQ's (`card_dma::clocks`).
    let enter = stamp(audio_cycles);
    // SAFETY: this handler owns USART1 after `init`, and `PIPE`,
    // `DIN_NOTES` and `DIN_CTRL` are touched only here once `init` has
    // unmasked it; the handler can't preempt itself.
    let (usart, pipe, notes, ctrl) = unsafe {
        (
            &*pac::USART1::ptr(),
            &mut *addr_of_mut!(PIPE),
            &mut *addr_of_mut!(DIN_NOTES),
            *addr_of_mut!(DIN_CTRL),
        )
    };
    // Every byte the FIFO holds, then one door call (spec § 7.2).
    let mut buf = [0u8; RX_FIFO];
    let mut n = 0;
    while n < RX_FIFO {
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
        buf[n] = usart.rdr.read().rdr().bits() as u8;
        n += 1;
    }
    if n > 0
        && let (Some(notes), Some(ctrl)) = (notes.as_mut(), ctrl)
    {
        // Closed, the bytes are dropped: MIDI in is deaf (spec § 13).
        midi_in::feed(pipe, &buf[..n], &mut DinSinks { notes, ctrl });
        let s = pipe.stats();
        MIDI_FAULTS.store(s.fault_total(), Ordering::Relaxed);
        MIDI_DROPS.store(u32::from(s.refused), Ordering::Relaxed);
    }
    let leave = stamp(audio_cycles);
    let audio = leave.over.since(enter.over);
    let own = leave.at.since(enter.at).get().saturating_sub(audio.get());
    MIDI_CYCLES.fetch_add(own, Ordering::Relaxed);
}
