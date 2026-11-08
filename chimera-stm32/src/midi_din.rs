use core::ptr::addr_of_mut;
use core::sync::atomic::{Ordering, compiler_fence};

use chimera_engine::midi_in::{self, CtrlRing, DinSinks, PipeSlot};
use chimera_engine::note_queue::NoteProducer;
#[cfg(feature = "midi-map")]
use chimera_engine::out_ring::OutRing;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use chimera_board::priority::{self, Priority};
use chimera_board::shared::{
    MIDI_CTRL_DROPS, MIDI_CYCLES, MIDI_DROPS, MIDI_ERRORS as ERRORS, MIDI_FAULTS, MIDI_OWES,
    audio_cycles, bump, own_cycles, stamp,
};

const BAUD: u32 = 31_250;
/// Active Sensing's silence: 300 ms in bit times at 31,250 baud (spec § 11).
const SENSING_BITS: u32 = 9_375;

/// The RX FIFO's depth: one ISR entry drains at most this many bytes into
/// one door call.
const RX_FIFO: usize = 16;
/// `TXFTCFG`: the threshold flag once the TX FIFO is half empty.
#[cfg(feature = "midi-map")]
const TXFT_HALF: u8 = 0b010;

/// The DIN pipe's slot; only the handler feeds it (the door builds it).
static mut PIPE: PipeSlot = PipeSlot::new();
static mut DIN_NOTES: Option<NoteProducer<'static>> = None;
/// The DIN control ring; only the handler pushes.
static mut DIN_CTRL: Option<&'static CtrlRing> = None;
/// DIN OUT's ring; only the handler pops (the UI pushes).
#[cfg(feature = "midi-map")]
static mut DIN_OUT: Option<&'static OutRing> = None;

/// DIN OUT's ring, before `init`: the handler pops it into the TX FIFO.
#[cfg(feature = "midi-map")]
pub fn set_out(out: &'static OutRing) {
    // SAFETY: before `init` unmasks USART1's interrupt, so nothing reads
    // `DIN_OUT` yet; from then on only the handler does.
    unsafe { *addr_of_mut!(DIN_OUT) = Some(out) };
}

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
    // The receiver timeout always counts; only an armed pipe takes its
    // interrupt (`RTOIE`, the handler's).
    usart.rtor.write(|w| w.rto().bits(SENSING_BITS));
    usart.cr2.write(|w| w.rtoen().set_bit());
    usart
        .icr
        .write(|w| w.orecf().clear().fecf().clear().ncf().clear());
    // DIN OUT: the TX FIFO's threshold at half empty, so the handler
    // tops it up before the line runs dry.
    #[cfg(feature = "midi-map")]
    // SAFETY: 0b010 is a defined `TXFTCFG` value (RM0433, USART_CR3).
    usart.cr3.write(|w| unsafe { w.txftcfg().bits(TXFT_HALF) });
    usart.cr1.write(|w| {
        w.fifoen()
            .set_bit()
            .rxneie()
            .set_bit()
            .re()
            .set_bit()
            .te()
            .bit(cfg!(feature = "midi-map"))
    });
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
    // `DIN_NOTES`, the DIN queue's producer, `DIN_CTRL`, the DIN
    // control ring it alone pushes to, and `DIN_OUT`, the out ring it
    // alone pops.
    unsafe { NVIC::unmask(pac::Interrupt::USART1) };
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
    // Active Sensing: 300 ms without a byte. A stale flag (counted while
    // disarmed) is cleared here, before this entry's bytes can arm.
    let mut idle = false;
    if usart.isr.read().rtof().bit_is_set() {
        usart.icr.write(|w| w.rtocf().clear());
        idle = usart.cr1.read().rtoie().bit_is_set();
        if idle {
            usart.cr1.modify(|_, w| w.rtoie().clear_bit());
        }
    }
    // Every byte the FIFO holds, then one door call (spec § 7.2).
    let mut buf = [0u8; RX_FIFO];
    let mut n = 0;
    while n < RX_FIFO {
        let isr = usart.isr.read();
        // A framing or noise error flags the byte at the FIFO's head: it is
        // read and dropped, so a glitch can't be FF (Reset).
        let bad = isr.fe().bit_is_set() || isr.nf().bit_is_set();
        // An uncleared ORE with RXFNEIE set refires this interrupt forever.
        if bad || isr.ore().bit_is_set() {
            usart
                .icr
                .write(|w| w.orecf().clear().fecf().clear().ncf().clear());
            bump(&ERRORS, 1);
        }
        if isr.rxne().bit_is_clear() {
            break;
        }
        buf[n] = usart.rdr.read().rdr().bits() as u8;
        n += usize::from(!bad);
    }
    // Owed (a refused wheel, a silence's NotesOff): an empty feed retries
    // it, the main loop pending this handler while `MIDI_OWES` (#884).
    let owes = pipe.owes();
    if (idle || n > 0 || owes)
        && let (Some(notes), Some(ctrl)) = (notes.as_mut(), ctrl)
    {
        let mut sinks = DinSinks {
            notes,
            ctrl,
            sysex: {
                #[cfg(feature = "midi-map")]
                let s = Some(
                    &chimera_core::midi_in::SYSEX_IN[chimera_core::midi_in::Port::Din as usize],
                );
                // No map, nobody drains a SysEx ring: none linked.
                #[cfg(not(feature = "midi-map"))]
                let s = None;
                s
            },
        };
        // The silence first, so this entry's bytes may arm again; its
        // counts too, as no byte may follow (#865).
        if idle {
            midi_in::idle(pipe, &mut sinks);
        }
        // Closed, the bytes are dropped: MIDI in is deaf (spec § 13). A
        // retry skipped here (a silence's entry) or refused again stays
        // owed: `MIDI_OWES` holds, and the next loop pass pends this
        // handler to land it.
        if n > 0 || (owes && !idle) {
            midi_in::feed(pipe, &buf[..n], &mut sinks);
        }
        MIDI_OWES.store(pipe.owes(), Ordering::Relaxed);
        let s = pipe.stats();
        MIDI_FAULTS.store(s.fault_total(), Ordering::Relaxed);
        MIDI_DROPS.store(u32::from(s.refused), Ordering::Relaxed);
        MIDI_CTRL_DROPS.store(u32::from(s.ctrl_refused), Ordering::Relaxed);
        // An FE arms the timeout's interrupt; the next silence fires it.
        if pipe.sensing_armed() {
            usart.cr1.modify(|_, w| w.rtoie().set_bit());
        }
    }
    // DIN OUT: the ring into the TX FIFO while it has room. The threshold
    // interrupt stays on while bytes wait; the main loop pends this handler
    // after the UI's out pass (`din_out`), so an idle line starts. Every
    // CR3 write is this handler's.
    #[cfg(feature = "midi-map")]
    // SAFETY: `DIN_OUT` is written only before `init` unmasks this
    // handler, which alone reads it.
    if let Some(out) = unsafe { *addr_of_mut!(DIN_OUT) } {
        let mut b = [0u8; 1];
        while usart.isr.read().txe().bit_is_set() && out.pop_into(&mut b) == 1 {
            usart.tdr.write(|w| w.tdr().bits(u16::from(b[0])));
        }
        let more = !out.is_empty();
        usart.cr3.modify(|_, w| w.txftie().bit(more));
    }
    let own = own_cycles(enter, stamp(audio_cycles));
    MIDI_CYCLES.fetch_add(own.get(), Ordering::Relaxed);
}
