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
//! Configured lets every Part go on a bus reset, and on a suspend only if
//! USB played a note-on since (`midi_in::lets_go`).
//!
//! MIDI out: the UI's echo in USB's out ring, packed into IN packets while
//! the endpoint takes them; the UI pends `OTG_FS` to start, each IN
//! packet's completion carries on. Not Configured, what waits is dropped.

use core::ptr::addr_of_mut;

use chimera_core::midi_in::PipeSlot;
use chimera_core::note_queue::NoteProducer;
use chimera_core::out_ring::OutRing;
use chimera_midi::usb::Packer;
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
    /// Configured, as the last poll saw it.
    configured: bool,
}

/// USB MIDI this boot: the class, if the door was open at `init` to
/// allocate it (a boot's layout never changes under the host). Only this
/// module reaches the class.
pub(crate) struct Midi(Option<MidiClass>);

/// MIDI OUT's endpoint: all a closed door leaves bank 1. Enough to drain
/// it, never the class, whose descriptor shim calls bank 2.
pub(crate) struct Closed(EndpointAddress);

impl Midi {
    /// The class's interfaces and endpoints, through the door: closed,
    /// nothing is allocated and MIDI stays off this boot.
    pub(crate) fn alloc(bus: &'static UsbBusAllocator<Bus>) -> Midi {
        Midi(crate::bank2::board_door(|d| (d.usb.alloc)(bus)))
    }

    /// Console and MIDI polled by bank 2, the door open now (`Ok`). Else
    /// what bank 1 polls: the console, and `Closed` if MIDI is on.
    #[inline(always)]
    pub(crate) fn poll(
        &mut self,
        dev: &mut Dev,
        serial: &mut Serial,
        counts: &mut Counts,
    ) -> Result<(), Option<Closed>> {
        let Some(m) = &mut self.0 else {
            return Err(None);
        };
        crate::bank2::board_door(|d| (d.usb.poll)(dev, serial, m, counts))
            .ok_or_else(|| Some(Closed(m.out.address())))
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

/// USB MIDI in's pipe and sinks, and out's (`Tx`). The rings are set before
/// `connect`; from then on only the poll touches it, under the cell's
/// exclusion.
struct MidiIn {
    pipe: PipeSlot,
    notes: Option<NoteProducer<'static>>,
    ctrl: Option<&'static UsbCtrl>,
    tx: Tx,
}

/// USB's out ring, its packer and the packets the endpoint hasn't taken.
struct Tx {
    ring: Option<&'static OutRing>,
    packer: Packer,
    held: [[u8; 4]; 16],
    n: usize,
}

static mut IN: MidiIn = MidiIn {
    pipe: PipeSlot::new(),
    notes: None,
    ctrl: None,
    tx: Tx {
        ring: None,
        packer: Packer::new(),
        held: [[0; 4]; 16],
        n: 0,
    },
};

/// USB's note producer, control ring and out ring, before `connect`
/// installs the device: bank 2's poll feeds and empties them.
pub(crate) fn set_in(
    notes: Option<NoteProducer<'static>>,
    ctrl: &'static UsbCtrl,
    out: Option<&'static OutRing>,
) {
    // SAFETY: before `connect` unmasks `OTG_FS` or makes the `Link` that
    // `with` needs, so no poll reads `IN` yet.
    let i = unsafe { &mut *addr_of_mut!(IN) };
    i.notes = notes;
    i.ctrl = Some(ctrl);
    i.tx.ring = out;
}

/// The board door's USB entries.
#[repr(C)]
pub(crate) struct UsbDoor {
    alloc: fn(&'static UsbBusAllocator<Bus>) -> MidiClass,
    poll: fn(&mut Dev, &mut Serial, &mut MidiClass, &mut Counts),
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
    use core::ptr::addr_of_mut;

    use chimera_core::midi_in::{self, DinSinks, Left};
    #[cfg(feature = "midi-map")]
    use chimera_core::midi_in::{CTRL_USB_LEN, DinPipe, PipeSlot};
    use chimera_core::note_queue::NoteProducer;
    use chimera_midi::usb::{Cable, Packer, unpack};
    use chimera_midi::usb_class::{Ids, descriptors as write};
    #[cfg(not(feature = "midi-map"))]
    use midi_in::{feed, lost};
    use usb_device::UsbError;
    use usb_device::class_prelude::{DescriptorWriter, UsbBusAllocator};
    use usb_device::device::UsbDeviceState;

    use super::{IN, MidiClass, MidiIn, Tx, UsbCtrl};
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

    pub(super) fn alloc(bus: &'static UsbBusAllocator<Bus>) -> MidiClass {
        MidiClass {
            ac: bus.interface(),
            ms: bus.interface(),
            name: bus.string(),
            out: bus.bulk(PACKET),
            inp: bus.bulk(PACKET),
            descriptors,
            configured: false,
        }
    }

    /// The pipe's sinks, if `set_in` gave it both.
    #[inline(always)]
    fn sinks<'a>(
        notes: &'a mut Option<NoteProducer<'static>>,
        ctrl: Option<&'static UsbCtrl>,
    ) -> Option<DinSinks<'a, 'static, { crate::shared::CTRL_USB }>> {
        notes
            .as_mut()
            .zip(ctrl)
            .map(|(notes, ctrl)| DinSinks { notes, ctrl })
    }

    /// The device polled with both classes; leaving Configured, every Part
    /// let go as `lets_go` rules; then every OUT packet read, cable 0's
    /// bytes fed to USB's pipe here in bank 2 (its ring is not DIN's), the
    /// rest dropped and counted; then the out ring sent (`send`).
    pub(super) fn poll(dev: &mut Dev, serial: &mut Serial, m: &mut MidiClass, counts: &mut Counts) {
        dev.poll(&mut [serial, m]);
        // SAFETY: only the poll touches `IN` once `connect` has run, and
        // the poll runs in the handler or under `with`, which masks it.
        let MidiIn {
            pipe,
            notes,
            ctrl,
            tx,
        } = unsafe { &mut *addr_of_mut!(IN) };
        let state = dev.state();
        let up = state == UsbDeviceState::Configured;
        if m.configured != up {
            m.configured = up;
            let left = match state {
                UsbDeviceState::Suspend => Left::Suspend,
                _ => Left::Reset,
            };
            if !up
                && midi_in::lets_go(left, pipe.played())
                && let Some(s) = &mut sinks(notes, *ctrl)
            {
                lost(pipe, s);
            }
        }
        let mut p = [0u8; PACKET as usize];
        while let Ok(n) = m.out.read(&mut p) {
            let mut s = sinks(notes, *ctrl);
            for &e in p[..n].as_chunks::<4>().0 {
                match (unpack(e), s.as_mut()) {
                    (Some((Cable::ZERO, b)), Some(s)) => feed(pipe, b.as_slice(), s),
                    _ => counts.midi_drops = counts.midi_drops.wrapping_add(1),
                }
            }
        }
        send(m, tx, counts, up);
    }

    /// Configured, the UI's lost echo counted, then the ring's bytes packed
    /// into IN packets while the endpoint takes them; busy, held for the
    /// next poll; any other error, dropped and counted. Not Configured, all
    /// dropped and the losses uncounted (the UI copies whatever the host
    /// does): a host that comes back hears nothing stale.
    fn send(m: &mut MidiClass, tx: &mut Tx, counts: &mut Counts, up: bool) {
        let Some(ring) = tx.ring else { return };
        if tx.n == 0 && ring.is_empty() {
            return;
        }
        let lost = ring.take_dropped();
        if !up {
            ring.clear();
            (tx.packer, tx.n) = (Packer::new(), 0);
            return;
        }
        counts.midi_drops = counts.midi_drops.wrapping_add(lost);
        loop {
            while tx.n < tx.held.len()
                && let Some(x) = ring.pop()
            {
                if let Some(p) = tx.packer.push(x) {
                    tx.held[tx.n] = p;
                    tx.n += 1;
                }
            }
            let (held, _) = tx.held.as_flattened().split_at(tx.n * 4);
            if held.is_empty() {
                return;
            }
            match m.inp.write(held) {
                Err(UsbError::WouldBlock) => return,
                Err(_) => counts.midi_drops = counts.midi_drops.wrapping_add(tx.n as u32),
                Ok(_) => {}
            }
            tx.n = 0;
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
