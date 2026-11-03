//! USB MIDI 1.0, one cable (spec § 4), its descriptors
//! `chimera_midi::usb_class`'s, the jacks named "Chimera". In-tree, not
//! `usbd-midi`: the same shape in fewer lines, raw packets, no new crate.
//!
//! `door` is bank 2's (`bank2.x` globs it): the allocation, the poll and
//! the descriptors. The class's `UsbClass` impl stays in bank 1, shims
//! only: its `dyn` vtable is an anonymous constant no glob can place, so
//! bank 1 holds it, and it may name only bank 1's code. The descriptors
//! are reached through a pointer the allocation stores in the class, and
//! only bank 2's poll ever hands the class to the device: `Midi` keeps the
//! class to itself, and with the door closed gives bank 1 only `Closed`,
//! MIDI OUT's endpoint to drain.
//!
//! MIDI in: each OUT packet's bytes, cable 0's, through USB's own pipe into
//! its note source and control ring, as DIN's (spec §§ 7, 8). Leaving
//! Configured lets every Part go on a bus reset, and on a suspend only
//! while USB holds a key or a pedal (`midi_in::lets_go`), every pedal it
//! held lifted after; NotesOff and pedal-ups a full queue refused are
//! retried each poll.
//!
//! MIDI out: the UI's echo in USB's out ring, packed into IN packets while
//! the endpoint takes them; the UI pends `OTG_FS` to start, each IN
//! packet's completion carries on. Not Configured, what waits is dropped.

use chimera_core::midi_in::PipeSlot;
use chimera_core::note_queue::NoteProducer;
use chimera_core::out_ring::OutRing;
use chimera_midi::usb::{Edge, InQueue};
use usb_device::bus::UsbBus as _;
use usb_device::class_prelude::{
    DescriptorWriter, EndpointAddress, EndpointIn, EndpointOut, InterfaceNumber, LangID,
    StringIndex, UsbBusAllocator, UsbClass,
};

use crate::usb::{Bus, Counts, Dev, Serial};

/// USB's control ring, as `shared::Ctrl` sizes it.
type UsbCtrl = chimera_core::midi_in::CtrlRing<{ crate::shared::CTRL_USB }>;

/// The jacks' name, and so the port's.
const NAME: &str = "Chimera";

/// `door::descriptors`, as the class carries it.
type Descriptors = fn(&MidiClass, &mut DescriptorWriter) -> usb_device::Result<()>;

pub(crate) struct MidiClass {
    ac: InterfaceNumber,
    ms: InterfaceNumber,
    name: StringIndex,
    out: EndpointOut<'static, Bus>,
    inp: EndpointIn<'static, Bus>,
    /// Bank 2's descriptor writer, stored by `door::alloc`.
    descriptors: Descriptors,
    edge: Edge,
    port: Port,
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

impl Closed {
    /// Every packet waiting read and dropped, counted in `drops`, so none
    /// blocks the shared RX FIFO (spec § 13.1, 13.2).
    #[inline(always)]
    pub(crate) fn drain(self, dev: &Dev, drops: &mut u32) {
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
/// cell. The pipe beside it: its held keys in the cell pushed the
/// counters past bank 1's short offsets.
struct Port {
    pipe: Option<&'static mut PipeSlot>,
    notes: Option<NoteProducer<'static>>,
    ctrl: &'static UsbCtrl,
    queue: InQueue,
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
        (index == self.name).then_some(NAME)
    }
}

/// Bank 2's half.
mod door {
    use chimera_core::midi_in::{self, DinSinks};
    #[cfg(feature = "midi-map")]
    use chimera_core::midi_in::{CTRL_USB_LEN, DinPipe};
    use chimera_core::note_queue::NoteProducer;
    use chimera_core::out_ring::OutRing;
    use chimera_midi::usb::{Edge, InQueue, cable_zero};
    use chimera_midi::usb_class::{Ids, descriptors as write};
    #[cfg(not(feature = "midi-map"))]
    use midi_in::{feed, lost};
    use usb_device::class_prelude::{DescriptorWriter, UsbBusAllocator};

    use super::{MidiClass, PipeSlot, Port, UsbCtrl};
    use crate::usb::{Bus, Counts, Dev, Serial};

    /// USB's burst-sized ring takes the pipe straight (the door's entries
    /// take DIN's); at DIN's size, the door's.
    #[cfg(feature = "midi-map")]
    #[inline(always)]
    fn feed(pipe: &mut PipeSlot, bytes: &[u8], s: &mut DinSinks<'_, '_, { CTRL_USB_LEN }>) {
        DinPipe::usb_feed(pipe, bytes, s);
    }

    #[cfg(feature = "midi-map")]
    #[inline(always)]
    fn lost(pipe: &mut PipeSlot, s: &mut DinSinks<'_, '_, { CTRL_USB_LEN }>) {
        DinPipe::usb_lost(pipe, s);
    }

    /// Bulk packets at full speed.
    const PACKET: u16 = 64;

    pub(super) fn alloc(
        bus: &'static UsbBusAllocator<Bus>,
        notes: Option<NoteProducer<'static>>,
        ctrl: &'static UsbCtrl,
    ) -> MidiClass {
        MidiClass {
            ac: bus.interface(),
            ms: bus.interface(),
            name: bus.string(),
            out: bus.bulk(PACKET),
            inp: bus.bulk(PACKET),
            descriptors,
            edge: Edge::new(),
            port: Port {
                // `alloc` runs once a boot (`Midi::alloc`, from `init`).
                pipe: cortex_m::singleton!(: PipeSlot = PipeSlot::new()),
                notes,
                ctrl,
                queue: InQueue::new(),
            },
        }
    }

    /// The pipe and its sinks, if the boot gave it a note producer.
    #[inline(always)]
    fn sinks<'a>(
        pipe: &'a mut Option<&'static mut PipeSlot>,
        notes: &'a mut Option<NoteProducer<'static>>,
        ctrl: &'static UsbCtrl,
    ) -> Option<(
        &'a mut PipeSlot,
        DinSinks<'a, 'static, { crate::shared::CTRL_USB }>,
    )> {
        Some((
            pipe.as_deref_mut()?,
            DinSinks {
                notes: notes.as_mut()?,
                ctrl,
            },
        ))
    }

    /// The device polled with both classes; leaving Configured, every Part
    /// let go as `lets_go` rules, NotesOff still owed retried (an empty
    /// feed); then every OUT packet read, cable 0's
    /// bytes fed to USB's pipe here in bank 2 (its ring is not DIN's), the
    /// rest dropped and counted; then the out ring sent (`InQueue::send`).
    pub(super) fn poll(
        dev: &mut Dev,
        serial: &mut Serial,
        m: &mut MidiClass,
        out: Option<&'static OutRing>,
        counts: &mut Counts,
    ) {
        dev.poll(&mut [serial, m]);
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
            for e in cable_zero(&p[..n]) {
                match (e, s.as_mut()) {
                    (Some(b), Some((pipe, s))) => feed(pipe, b.as_slice(), s),
                    _ => counts.in_drops = counts.in_drops.wrapping_add(1),
                }
            }
        }
        if let Some(ring) = out {
            let lost = queue.send(ring, edge.up(), |b| inp.write(b));
            counts.out_drops = counts.out_drops.wrapping_add(lost);
        }
    }

    /// `chimera_midi::usb_class`'s descriptors, its host test's.
    fn descriptors(c: &MidiClass, w: &mut DescriptorWriter) -> usb_device::Result<()> {
        let ids = Ids {
            ac: c.ac,
            ms: c.ms,
            name: c.name,
        };
        write(w, ids, &c.out, &c.inp)
    }
}
