//! USB MIDI 1.0 (spec § 4), its descriptors `chimera_midi::usb_class`'s:
//! cable 0 "Chimera" both ways, and into Chimera a cable a Part, "Chimera
//! Part 1" to 6 (ADR 0243). In-tree, not
//! `usbd-midi`: the same shape in fewer lines, raw packets, no new crate.
//!
//! `door` is bank 2's (`bank2.x` globs it): the allocation, the poll and
//! the descriptors. The class's `UsbClass` impl stays in bank 1, shims
//! only: its `dyn` vtable is an anonymous constant no glob can place, so
//! bank 1 holds it, and it may name only bank 1's code. The descriptors
//! and the jacks' names are reached through pointers the allocation
//! stores in the class, and
//! only bank 2's poll ever hands the class to the device: `Midi` keeps the
//! class to itself, and with the door closed gives bank 1 only `Closed`,
//! MIDI OUT's endpoint to drain.
//!
//! MIDI in: each OUT packet's bytes, cable 0's as sent and a Part cable's
//! channel messages on its Part's channel, through USB's own pipe into
//! its note source and control ring, as DIN's (spec §§ 7, 8). Leaving
//! Configured lets every Part go on a bus reset, and on a suspend only
//! while USB holds a key or a pedal (`midi_in::lets_go`), every pedal it
//! held lifted after; NotesOff and pedal-ups a full queue refused are
//! retried each poll.
//!
//! MIDI out: the UI's echo in USB's out ring, packed into IN packets while
//! the endpoint takes them; the UI pends `OTG_FS` to start, each IN
//! packet's completion carries on. Not Configured, what waits is dropped.

use chimera_core::midi_in::UsbSlot;
use chimera_core::note_queue::NoteProducer;
use chimera_core::out_ring::OutRing;
use chimera_midi::usb::{Edge, InQueue, PartCables};
use chimera_midi::usb_class::Ids;
use usb_device::bus::UsbBus as _;
#[cfg(feature = "usb-audio")]
use usb_device::class_prelude::InterfaceNumber;
use usb_device::class_prelude::{
    DescriptorWriter, EndpointAddress, EndpointIn, EndpointOut, LangID, StringIndex,
    UsbBusAllocator, UsbClass,
};

use crate::usb::{Bus, Counts, Dev, Serial};

/// USB's control ring, as `shared::Ctrl` sizes it.
type UsbCtrl = chimera_core::midi_in::CtrlRing<{ crate::shared::CTRL_USB }>;

/// `door::descriptors`, as the class carries it.
type Descriptors = fn(&MidiClass, &mut DescriptorWriter) -> usb_device::Result<()>;

/// `usb_class::jack_name`, as the class carries it.
type JackName = fn(Ids, StringIndex) -> Option<&'static str>;

pub(crate) struct MidiClass {
    ids: Ids,
    out: EndpointOut<'static, Bus>,
    inp: EndpointIn<'static, Bus>,
    /// Bank 2's descriptor writer and jack names, stored by `door::alloc`.
    descriptors: Descriptors,
    jack_name: JackName,
    edge: Edge,
    port: Port,
    /// USB audio's stream (ADR 0248), in MIDI's function.
    #[cfg(feature = "usb-audio")]
    audio: crate::usb_audio::Stream,
}

/// USB MIDI this boot: the class, if the door was open at `init` to
/// allocate it (a boot's layout never changes under the host), and USB's
/// out ring. Only this module reaches the class.
pub(crate) struct Midi {
    class: Option<MidiClass>,
    out: Option<&'static OutRing>,
}

/// What a poll answered.
pub(crate) enum Polled {
    /// Bank 2 polled console and MIDI.
    ByBank2,
    /// The door is closed: bank 1 polls the console, and drains MIDI's OUT
    /// if MIDI is on (`Closed`).
    Console(Option<Closed>),
}

/// MIDI OUT's endpoint: all a closed door leaves bank 1. Enough to drain
/// it, never the class, whose descriptor shim calls bank 2.
pub(crate) struct Closed(EndpointAddress);

impl Midi {
    /// The class's interfaces and endpoints, through the door: closed,
    /// nothing is allocated and MIDI stays off this boot.
    pub(crate) fn alloc(bus: &'static UsbBusAllocator<Bus>, sinks: Sinks) -> Midi {
        let Sinks { notes, ctrl, out } = sinks;
        Midi {
            class: crate::bank2::board_door(|d| (d.usb.alloc)(bus, notes, ctrl)),
            out,
        }
    }

    /// Console and MIDI polled by bank 2 if the door is open now.
    #[inline(always)]
    pub(crate) fn poll(
        &mut self,
        dev: &mut Dev,
        serial: &mut Serial,
        counts: &mut Counts,
    ) -> Polled {
        let Midi { class, out } = self;
        let polled = match class {
            Some(m) => crate::bank2::board_door(|d| (d.usb.poll)(dev, serial, m, *out, counts))
                .map_or_else(
                    || Polled::Console(Some(Closed(m.out.address()))),
                    |()| Polled::ByBank2,
                ),
            None => Polled::Console(None),
        };
        if let Polled::Console(_) = polled
            && let Some(r) = out
        {
            // Nothing sends USB's out ring, and the UI copies to it all the
            // same (a door opened after a closed boot runs the echo): it is
            // dropped, and its losses are no drops.
            r.discard();
        }
        polled
    }
}

impl Midi {
    /// USB audio's `bench` line, if the class is up.
    #[cfg(feature = "usb-audio")]
    pub(crate) fn audio_report(&self, t: &mut impl core::fmt::Write) -> core::fmt::Result {
        self.class.as_ref().map_or(Ok(()), |m| m.audio.report(t))
    }
}

impl Closed {
    /// The packets the bus holds now read and dropped, counted in `drops`,
    /// so none blocks the shared RX FIFO (spec § 13.1, 13.2); later ones
    /// at a later poll.
    #[inline(always)]
    pub(crate) fn drain(self, dev: &Dev, drops: &mut u32) {
        #[cfg(feature = "usb-audio")]
        crate::usb_audio::closed();
        let mut p = [0u8; 64];
        while let Ok(n) = dev.bus().read(self.0, &mut p) {
            *drops = drops.wrapping_add((n / 4) as u32);
        }
    }
}

/// What USB MIDI feeds and empties: in's note producer and control ring,
/// out's ring. Handed to `init`.
pub struct Sinks {
    pub(crate) notes: Option<NoteProducer<'static>>,
    pub(crate) ctrl: &'static UsbCtrl,
    pub(crate) out: Option<&'static OutRing>,
}

/// USB MIDI in's pipe and sinks, and out's queue: the poll's alone, in the
/// cell. The pipe and the Part cables' keys beside it: in the cell they
/// pushed the counters past bank 1's short offsets.
struct Port {
    pipe: Option<&'static mut UsbSlot>,
    notes: Option<NoteProducer<'static>>,
    ctrl: &'static UsbCtrl,
    queue: InQueue,
    /// Where the Part cables' held keys and pedals went down.
    parts: Option<&'static mut PartCables>,
}

/// The board door's USB entries.
#[repr(C)]
pub(crate) struct UsbDoor {
    alloc: fn(
        &'static UsbBusAllocator<Bus>,
        Option<NoteProducer<'static>>,
        &'static UsbCtrl,
    ) -> MidiClass,
    poll: fn(&mut Dev, &mut Serial, &mut MidiClass, Option<&'static OutRing>, &mut Counts),
}

impl UsbDoor {
    pub(crate) const DOOR: UsbDoor = UsbDoor {
        alloc: door::alloc,
        poll: door::poll,
    };
}

/// Shims: only `door::poll` lists the class, so these run with the door
/// open.
impl UsbClass<Bus> for MidiClass {
    fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> usb_device::Result<()> {
        (self.descriptors)(self, w)
    }

    fn get_string(&self, index: StringIndex, _: LangID) -> Option<&str> {
        (self.jack_name)(self.ids, index)
    }

    // USB audio's alt setting, the stream on or off: kept here, read by
    // bank 2's pump.
    #[cfg(feature = "usb-audio")]
    fn get_alt_setting(&mut self, iface: InterfaceNumber) -> Option<u8> {
        self.audio.alt.get(iface)
    }

    #[cfg(feature = "usb-audio")]
    fn set_alt_setting(&mut self, iface: InterfaceNumber, alt: u8) -> bool {
        self.audio.alt.set(iface, alt)
    }

    #[cfg(feature = "usb-audio")]
    fn reset(&mut self) {
        self.audio.alt.reset();
    }
}

/// Bank 2's half.
mod door {
    use chimera_core::midi_in::{self, DinPipe, DinSinks, PART_CHANNELS};
    use chimera_core::note_queue::NoteProducer;
    use chimera_core::out_ring::OutRing;
    use chimera_midi::usb::{Cable, Edge, InQueue, PartCables, cables};
    use chimera_midi::usb_class::{Ids, cable_count, descriptors as write, jack_name, names};

    /// This build's cables: one beside USB audio's stream (ADR 0243).
    const CABLES: u8 = cable_count(cfg!(feature = "usb-audio"));
    use usb_device::class_prelude::{DescriptorWriter, UsbBusAllocator};

    use super::{MidiClass, Port, UsbCtrl, UsbSlot};
    use crate::usb::{Bus, Counts, Dev, Serial};

    /// USB's pipe, straight: the poll runs in bank 2. Its ring is
    /// burst-sized with the map, DIN's size without.
    #[inline(always)]
    fn feed(
        pipe: &mut UsbSlot,
        bytes: &[u8],
        s: &mut DinSinks<'_, '_, { crate::shared::CTRL_USB }>,
    ) {
        #[cfg(feature = "midi-map")]
        DinPipe::usb_feed(pipe, bytes, s);
        #[cfg(not(feature = "midi-map"))]
        DinPipe::usb_feed_short(pipe, bytes, s);
    }

    /// A Part cable's message, past the pipe's parser (ADR 0243).
    #[inline(always)]
    fn feed_msg(
        pipe: &mut UsbSlot,
        msg: &[u8],
        s: &mut DinSinks<'_, '_, { crate::shared::CTRL_USB }>,
    ) {
        #[cfg(feature = "midi-map")]
        DinPipe::usb_feed_msg(pipe, msg, s);
        #[cfg(not(feature = "midi-map"))]
        DinPipe::usb_feed_msg_short(pipe, msg, s);
    }

    #[inline(always)]
    fn lost(pipe: &mut UsbSlot, s: &mut DinSinks<'_, '_, { crate::shared::CTRL_USB }>) {
        #[cfg(feature = "midi-map")]
        DinPipe::usb_lost(pipe, s);
        #[cfg(not(feature = "midi-map"))]
        DinPipe::usb_lost_short(pipe, s);
    }

    /// Bulk packets at full speed.
    const PACKET: u16 = 64;

    pub(super) fn alloc(
        bus: &'static UsbBusAllocator<Bus>,
        notes: Option<NoteProducer<'static>>,
        ctrl: &'static UsbCtrl,
    ) -> MidiClass {
        // AudioControl, USB audio's stream, MIDIStreaming: one function's
        // interfaces, numbered in a row under its IAD.
        let ac = bus.interface();
        #[cfg(feature = "usb-audio")]
        let stream = bus.interface();
        let ms = bus.interface();
        MidiClass {
            ids: Ids {
                ac,
                ms,
                name: names(bus, CABLES),
                cables: CABLES,
            },
            out: bus.bulk(PACKET),
            inp: bus.bulk(PACKET),
            #[cfg(feature = "usb-audio")]
            audio: crate::usb_audio::door::alloc(bus, stream),
            descriptors,
            jack_name,
            edge: Edge::new(),
            port: Port {
                // `alloc` runs once a boot (`Midi::alloc`, from `init`).
                pipe: cortex_m::singleton!(: UsbSlot = UsbSlot::new()),
                notes,
                ctrl,
                queue: InQueue::new(),
                parts: cortex_m::singleton!(: PartCables = PartCables::new()),
            },
        }
    }

    /// The pipe and its sinks, if the boot gave it a note producer.
    #[inline(always)]
    fn sinks<'a>(
        pipe: &'a mut Option<&'static mut UsbSlot>,
        notes: &'a mut Option<NoteProducer<'static>>,
        ctrl: &'static UsbCtrl,
    ) -> Option<(
        &'a mut UsbSlot,
        DinSinks<'a, 'static, { crate::shared::CTRL_USB }>,
    )> {
        Some((
            pipe.as_deref_mut()?,
            DinSinks {
                notes: notes.as_mut()?,
                ctrl,
                sysex: {
                    #[cfg(feature = "midi-map")]
                    let s = Some(
                        &chimera_core::midi_in::SYSEX_IN[chimera_core::midi_in::Port::Usb as usize],
                    );
                    // No map, nobody drains a SysEx ring: none linked.
                    #[cfg(not(feature = "midi-map"))]
                    let s = None;
                    s
                },
            },
        ))
    }

    /// The device polled with both classes; leaving Configured, every Part
    /// let go as `lets_go` rules, NotesOff still owed retried (an empty
    /// feed); then the OUT packets the bus holds now read, cable 0's
    /// bytes fed to USB's pipe here in bank 2 (its ring is not DIN's), the
    /// rest dropped and counted; then the out ring sent (`InQueue::send`).
    pub(super) fn poll(
        dev: &mut Dev,
        serial: &mut Serial,
        m: &mut MidiClass,
        out: Option<&'static OutRing>,
        counts: &mut Counts,
    ) {
        #[cfg(feature = "usb-audio")]
        crate::usb_audio::door::events(&mut m.audio);
        dev.poll(&mut [serial, m]);
        #[cfg(feature = "usb-audio")]
        crate::usb_audio::door::pump(
            &mut m.audio,
            dev.state() == usb_device::device::UsbDeviceState::Configured,
        );
        let MidiClass {
            out: from_host,
            inp,
            edge,
            port:
                Port {
                    pipe,
                    notes,
                    ctrl,
                    queue,
                    parts,
                },
            ..
        } = m;
        let left = edge.poll(dev.state());
        if let Some((pipe, s)) = &mut sinks(pipe, notes, ctrl) {
            if left.is_some_and(|l| midi_in::lets_go(l, pipe.holds())) {
                lost(pipe, s);
            } else if pipe.owes() {
                feed(pipe, &[], s);
            }
        }
        let mut p = [0u8; PACKET as usize];
        while let Ok(n) = from_host.read(&mut p) {
            let mut s = sinks(pipe, notes, ctrl);
            for e in cables(&p[..n]) {
                // Cable 0 as sent; a Part cable's channel message on its
                // Part's channel (a held key's and the pedal's where they
                // went down), past the parser; anything else on it dropped
                // (ADR 0243).
                match (e, s.as_mut()) {
                    (Some((Cable::ZERO, b)), Some((pipe, s))) => feed(pipe, b.as_slice(), s),
                    (Some((c, b)), Some((pipe, s))) => {
                        match PART_CHANNELS
                            .of_cable(c)
                            .and_then(|ch| parts.as_deref_mut()?.route(c, b, ch))
                        {
                            Some(m) => feed_msg(pipe, m.as_slice(), s),
                            None => counts.in_drops = counts.in_drops.wrapping_add(1),
                        }
                    }
                    _ => counts.in_drops = counts.in_drops.wrapping_add(1),
                }
            }
        }
        // What the queue refused, a lost's too: `status`'s (#865).
        if let Some(p) = pipe {
            counts.pipe_drops = u32::from(p.stats().refused);
        }
        if let Some(ring) = out {
            let lost = queue.send(ring, edge.up(), |b| inp.write(b));
            counts.out_drops = counts.out_drops.wrapping_add(lost);
        }
    }

    /// `chimera_midi::usb_class`'s descriptors, its host test's.
    fn descriptors(c: &MidiClass, w: &mut DescriptorWriter) -> usb_device::Result<()> {
        #[cfg(feature = "usb-audio")]
        let audio = Some(chimera_midi::usb_class::Stream {
            iface: c.audio.alt.iface(),
            ep: &c.audio.ep,
        });
        #[cfg(not(feature = "usb-audio"))]
        let audio = None;
        write(w, c.ids, &c.out, &c.inp, audio)
    }
}
