//! USB audio toward the host (ADR 0248): the stereo main out (DAC pair 1)
//! as a UAC1 stream in USB MIDI's function (`usb-audio`, in every
//! shipping build: ADR 0256). The audio interrupt pushes each block into
//! `RING`; the USB interrupt pops a packet at a time onto the isochronous
//! IN, sized by `chimera_midi::uac::Pacer`.
//!
//! `door` is bank 2's (`bank2.x` globs it): the stream's allocation, its
//! interrupt events and the pump, all reached through USB MIDI's door
//! poll. Bank 1 keeps the ring, the alt-setting shims (the class's
//! `UsbClass` impl, `usb_midi`) and `closed`. Never a disable or a FIFO
//! flush of the endpoint: the spike's (f16cec8c) left the hub disabling
//! the port. A packet the host never took stays armed until it does.

use chimera_midi::uac::{Counts, PACKET, Pacer, RING as FRAMES};
use chimera_midi::uac_link::{Alt, Ring};
use stm32h7xx_hal::pac;
use usb_device::class_prelude::EndpointIn;

use crate::usb::Bus;

/// The main out's frames, DAC words: the audio interrupt's to push, the
/// USB interrupt's to pop.
pub static RING: Ring<FRAMES> = Ring::new();

/// GINTSTS and GINTMSK: start of frame, incomplete isochronous IN.
const SOF: u32 = 1 << 3;
const IISOIXFR: u32 = 1 << 20;

/// The stream: its interface's alt setting, its endpoint, and the pump's
/// state. In USB MIDI's class, so in the cell, the poll's alone.
pub(crate) struct Stream {
    pub(crate) alt: Alt,
    pub(crate) ep: EndpointIn<'static, Bus>,
    /// SOF and the incomplete interrupt unmasked: the host has it on.
    on: bool,
    pacer: Pacer,
    pub(crate) counts: Counts,
    buf: [u8; PACKET as usize],
}

impl Stream {
    /// `bench`'s line: the counters, the alt setting and the fill.
    pub(crate) fn report(&self, t: &mut impl core::fmt::Write) -> core::fmt::Result {
        let c = self.counts;
        writeln!(
            t,
            "usb_audio on {} sent {} missed {} short {} resyncs {} reaimed {} fill {}",
            u8::from(self.on),
            c.sent,
            c.missed,
            c.short,
            c.resyncs,
            c.reaimed,
            RING.len()
        )
    }
}

/// `bench`'s text: one line, cut short past its room.
pub(crate) struct Line {
    buf: [u8; 128],
    len: usize,
}

impl Line {
    pub(crate) const fn new() -> Line {
        Line {
            buf: [0; 128],
            len: 0,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = &mut self.buf[self.len..];
        let n = s.len().min(room.len());
        room[..n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

/// OTG2's global block.
#[inline(always)]
fn otg() -> &'static pac::otg1_hs_global::RegisterBlock {
    // SAFETY: OTG2's fixed global block. Only the USB interrupt and the
    // UI under `with`'s mask (which never run together) read or write it
    // here, as `usb::rx_park`.
    unsafe { &*pac::OTG2_HS_GLOBAL::ptr() }
}

/// The door closed: SOF and the incomplete interrupt masked for good (a
/// door opens again only at a boot), so their flags, which bank 1's poll
/// never clears, can't hold the handler. GINTMSK's read-modify-write is
/// `rx_park`'s, under the same exclusion.
#[inline(always)]
pub(crate) fn closed() {
    otg()
        .gintmsk
        // SAFETY: clears two mask bits; every other bit as read.
        .modify(|r, w| unsafe { w.bits(r.bits() & !(SOF | IISOIXFR)) });
}

/// Bank 2's half.
pub(crate) mod door {
    use chimera_midi::uac::{self, Counts, FRAME_BYTES, PACKET, Pacer};
    use chimera_midi::uac_link::Alt;
    use usb_device::class_prelude::{InterfaceNumber, UsbBusAllocator};
    use usb_device::endpoint::{IsochronousSynchronizationType, IsochronousUsageType};

    use super::{IISOIXFR, RING, SOF, Stream, otg};
    use crate::usb::Bus;

    /// The stream's endpoint, its interface allocated by the caller
    /// between AudioControl and MIDIStreaming.
    pub(crate) fn alloc(bus: &'static UsbBusAllocator<Bus>, iface: InterfaceNumber) -> Stream {
        Stream {
            alt: Alt::new(iface),
            ep: bus.isochronous(
                IsochronousSynchronizationType::Asynchronous,
                IsochronousUsageType::Data,
                PACKET,
                1,
            ),
            on: false,
            pacer: Pacer::new(),
            counts: Counts::default(),
            buf: [0; PACKET as usize],
        }
    }

    /// DIEPCTL of IN endpoint `n` (RM0433: OTG base + 0x900 + 0x20·n).
    #[inline(always)]
    fn diepctl(n: usize) -> *mut u32 {
        (stm32h7xx_hal::pac::OTG2_HS_GLOBAL::ptr() as usize + 0x900 + 0x20 * n) as *mut u32
    }

    /// The bus frame now (DSTS.FNSOF).
    #[inline(always)]
    fn now() -> u16 {
        // SAFETY: a plain read of OTG2's DSTS, which has no read side effect.
        let d = unsafe { &*stm32h7xx_hal::pac::OTG2_HS_DEVICE::ptr() };
        uac::frame(d.dsts.read().bits())
    }

    /// Before the device's poll: SOF and the incomplete interrupt taken
    /// (write-1-to-clear), and a packet the frame ended without aimed
    /// again at the next.
    #[inline(always)]
    pub(crate) fn events(s: &mut Stream) {
        let g = otg();
        let flags = g.gintsts.read().bits() & (SOF | IISOIXFR);
        if flags == 0 {
            return;
        }
        // SAFETY: GINTSTS's SOF and IISOIXFR are write-1-to-clear; the
        // zeros written elsewhere change nothing.
        g.gintsts.write(|w| unsafe { w.bits(flags) });
        if flags & IISOIXFR != 0 {
            let p = diepctl(s.ep.address().index());
            // SAFETY: the stream's own IN endpoint's DIEPCTL; the bus
            // writes it only in `write` and its reset's configure, both
            // from this poll's context, never concurrently.
            unsafe {
                let v = p.read_volatile();
                if uac::armed(v) {
                    p.write_volatile(uac::aim(v, now()));
                    s.counts.reaimed = s.counts.reaimed.wrapping_add(1);
                }
            }
        }
    }

    /// After the device's poll: the stream on or off as the host set it
    /// (SOF and the incomplete interrupt unmasked only while on, so an idle
    /// stream costs no entries), then the next packet if none is armed,
    /// aimed at the next frame.
    #[inline(always)]
    pub(crate) fn pump(s: &mut Stream, configured: bool) {
        let on = configured && s.alt.streaming();
        if on != s.on {
            s.on = on;
            let g = otg();
            if on {
                RING.skip(RING.len());
                s.pacer.stop();
                // SAFETY: write-1-to-clear, as `events`.
                g.gintsts.write(|w| unsafe { w.bits(SOF | IISOIXFR) });
            }
            // SAFETY: GINTMSK's two bits set or cleared, every other as
            // read: `rx_park`'s read-modify-write, under the same exclusion.
            g.gintmsk.modify(|r, w| unsafe {
                w.bits(if on {
                    r.bits() | SOF | IISOIXFR
                } else {
                    r.bits() & !(SOF | IISOIXFR)
                })
            });
        }
        if !on {
            return;
        }
        let p = diepctl(s.ep.address().index());
        // SAFETY: as `events`'.
        let v = unsafe { p.read_volatile() };
        if uac::armed(v) {
            return;
        }
        let at = now();
        let next = s.pacer.next(RING.len(), at.wrapping_add(1), &mut s.counts);
        RING.skip(next.skip);
        if next.send == 0 {
            return;
        }
        let buf = &mut s.buf;
        let n = RING.pop(next.send, |i, f| {
            uac::pack(f, &mut buf[i * FRAME_BYTES..]);
        });
        // SAFETY: as `events'`; not armed, so the core isn't using it.
        unsafe { p.write_volatile(uac::aim(v, at)) };
        // `WouldBlock` can't be: not armed, and its FIFO holds one packet.
        let _ = s.ep.write(&s.buf[..n * FRAME_BYTES]);
    }
}
