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

/// The host's playback (ADR 0260): the USB interrupt pushes each packet,
/// the audio interrupt pops a block into the main out after the capture's
/// tap. Bank 1's (bank 2 is full, ADR 0251), as is everything below.
#[cfg(not(feature = "usb-config-writer"))]
pub static PLAY: Ring<{ chimera_midi::uac_out::RING }> = Ring::new();
/// The host has the playback on: the audio side drains while it isn't.
#[cfg(not(feature = "usb-config-writer"))]
pub static PLAY_ON: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// USB IN's step, the UI's each lap (`MidiSettings::usb_in`).
#[cfg(not(feature = "usb-config-writer"))]
pub static PLAY_LEVEL: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(chimera_core::ui::midi_settings::USB_IN_TOP);
/// Blocks the playback's ring was dry for: the audio side's `short`.
#[cfg(not(feature = "usb-config-writer"))]
pub static PLAY_SHORT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// The playback's interface, its OUT and feedback IN, and the servo. In
/// USB MIDI's class, so in the cell, the poll's alone.
#[cfg(not(feature = "usb-config-writer"))]
pub(crate) struct Play {
    pub(crate) alt: Alt,
    pub(crate) out: usb_device::class_prelude::EndpointOut<'static, Bus>,
    pub(crate) fb: EndpointIn<'static, Bus>,
    on: bool,
    servo: chimera_midi::uac_out::Feedback,
    /// The 10.14 value the feedback carries.
    value: u32,
    /// The bus frame the servo last ran in.
    last: u16,
    counts: chimera_midi::uac_out::Counts,
}

#[cfg(not(feature = "usb-config-writer"))]
impl Play {
    /// The playback's interface (after MIDIStreaming's) and endpoints:
    /// an asynchronous OUT and its 3-byte feedback IN, every frame.
    pub(crate) fn alloc(bus: &'static usb_device::class_prelude::UsbBusAllocator<Bus>) -> Play {
        use chimera_midi::uac_out::{self, Feedback, NOMINAL_FB};
        use usb_device::endpoint::{
            IsochronousSynchronizationType as Sync, IsochronousUsageType as Usage,
        };
        let iface = bus.interface();
        Play {
            alt: Alt::new(iface),
            out: bus.isochronous(Sync::Asynchronous, Usage::Data, uac_out::PACKET, 1),
            fb: bus.isochronous(Sync::NoSynchronization, Usage::Feedback, 3, 1),
            on: false,
            servo: Feedback::new(),
            value: NOMINAL_FB,
            last: 0,
            counts: uac_out::Counts::default(),
        }
    }

    /// After the device's poll, each entry: the stream on or off as the
    /// host set it (SOF unmasked while on: the capture's pump masks it as
    /// its own stream stops, so it is set again here); once a bus frame
    /// the servo runs and both endpoints are aimed at the frame now (an
    /// isochronous endpoint moves only in a frame of its parity); a packet
    /// in goes to the ring and its endpoint is aimed at the next frame; the
    /// feedback, once sent, is armed again. Never a disable or a flush.
    pub(crate) fn pump(&mut self, configured: bool, capture: bool) {
        use chimera_midi::uac;
        use chimera_midi::uac_out::{self, Feedback, NOMINAL_FB};
        use core::sync::atomic::Ordering;
        let on = configured && self.alt.streaming();
        let g = otg();
        if on != self.on {
            self.on = on;
            PLAY_ON.store(on, Ordering::Release);
            if on {
                self.servo = Feedback::new();
                self.value = NOMINAL_FB;
            } else if !capture {
                // SAFETY: SOF's mask bit cleared, every other as read:
                // `rx_park`'s read-modify-write, under the same exclusion.
                g.gintmsk.modify(|r, w| unsafe { w.bits(r.bits() & !SOF) });
            }
        }
        if !on {
            return;
        }
        if g.gintmsk.read().bits() & SOF == 0 {
            // SAFETY: as above, SOF's bit set.
            g.gintmsk.modify(|r, w| unsafe { w.bits(r.bits() | SOF) });
        }
        let now = door::now();
        let (out, fb) = (
            doepctl(self.out.address().index()),
            door::diepctl(self.fb.address().index()),
        );
        if now != self.last {
            self.last = now;
            self.value = self.servo.tick(PLAY.len());
            for p in [out, fb] {
                // SAFETY: the playback's own endpoints' control registers;
                // the bus writes them only from this poll's context.
                unsafe {
                    let v = p.read_volatile();
                    if uac::armed(v) {
                        p.write_volatile(uac::aim(v, now.wrapping_sub(1)));
                    }
                }
            }
        }
        // On the stack (DTCM), not in the cell: AXI is short.
        let mut buf = [0u8; uac_out::PACKET as usize];
        if let Ok(n) = self.out.read(&mut buf) {
            uac_out::receive(&buf[..n], &PLAY, &mut self.counts);
            // SAFETY: as above; the next packet comes in the next frame.
            unsafe {
                let v = out.read_volatile();
                out.write_volatile(uac::aim(v, now));
            }
        }
        // SAFETY: as above.
        let v = unsafe { fb.read_volatile() };
        if !uac::armed(v) {
            // SAFETY: as above; not armed, so the core isn't using it.
            unsafe { fb.write_volatile(uac::aim(v, now.wrapping_sub(1))) };
            let _ = self.fb.write(&uac_out::bytes(self.value));
        }
    }

    /// `bench`'s line.
    pub(crate) fn report(&self, t: &mut impl core::fmt::Write) -> core::fmt::Result {
        let c = self.counts;
        writeln!(
            t,
            "usb_play on {} received {} short {} overruns {} fb {:#x} fill {}",
            u8::from(self.on),
            c.received,
            PLAY_SHORT.load(core::sync::atomic::Ordering::Relaxed),
            c.overruns,
            self.value,
            PLAY.len()
        )
    }
}

/// DOEPCTL of OUT endpoint `n` (RM0433: OTG base + 0xB00 + 0x20·n).
#[cfg(not(feature = "usb-config-writer"))]
#[inline(always)]
fn doepctl(n: usize) -> *mut u32 {
    (pac::OTG2_HS_GLOBAL::ptr() as usize + 0xB00 + 0x20 * n) as *mut u32
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
    pub(super) fn diepctl(n: usize) -> *mut u32 {
        (stm32h7xx_hal::pac::OTG2_HS_GLOBAL::ptr() as usize + 0x900 + 0x20 * n) as *mut u32
    }

    /// The bus frame now (DSTS.FNSOF).
    #[inline(always)]
    pub(super) fn now() -> u16 {
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
            // Only a capture packet past its own frame: in duplex the
            // feedback endpoint's misses raise the flag too (#973).
            unsafe {
                let v = p.read_volatile();
                let at = now();
                if uac::armed(v) && s.pacer.late(at) {
                    p.write_volatile(uac::aim(v, at));
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
