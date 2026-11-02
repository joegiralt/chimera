//! USB MIDI 1.0, one cable (spec § 4), its descriptors
//! `chimera_midi::usb_class`'s, the jacks named "Chimera". In-tree, not
//! `usbd-midi`: the same shape in fewer lines, raw packets, no new crate.
//!
//! `door` is bank 2's (`bank2.x` globs it): the allocation, the poll and
//! the descriptors. The class's `UsbClass` impl stays in bank 1, shims
//! only: its `dyn` vtable is an anonymous constant no glob can place, so
//! bank 1 holds it, and it may name only bank 1's code. The descriptors
//! are reached through a pointer the allocation stores in the class, and
//! only bank 2's poll ever hands the class to the device.
//!
//! MIDI in: each OUT packet's bytes, cable 0's, through USB's own pipe into
//! its note source and control ring, as DIN's (spec §§ 7, 8). Leaving
//! Configured lets every Part go on a bus reset, and on a suspend only if
//! USB played a note-on since (`midi_in::lets_go`).
//!
//! MIDI out: the UI's echo in USB's out ring, packed into IN packets while
//! the endpoint takes them; the UI pends `OTG_FS` to start, each IN
//! packet's completion carries on. Not Configured, what waits is dropped.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::midi_in::PipeSlot;
use chimera_core::note_queue::NoteProducer;
use chimera_core::out_ring::OutRing;
use chimera_midi::usb::Packer;

/// USB's control ring, as `shared::Ctrl` sizes it.
type UsbCtrl = chimera_core::midi_in::CtrlRing<{ crate::shared::CTRL_USB }>;

use usb_device::class_prelude::{
    DescriptorWriter, EndpointAddress, EndpointIn, EndpointOut, InterfaceNumber, LangID,
    StringIndex, UsbBusAllocator, UsbClass,
};

use crate::usb::{Bus, UsbCore};

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
}

/// Bank 1's home for the class, filled through the door's `alloc` at
/// `init` only when it is open (`PipeSlot`'s pattern): a filled slot is MIDI up.
pub(crate) struct MidiSlot {
    /// MIDI OUT's endpoint, set once `class` is written.
    out: Option<EndpointAddress>,
    class: MaybeUninit<MidiClass>,
}

impl MidiSlot {
    pub(crate) const fn new() -> Self {
        Self {
            out: None,
            class: MaybeUninit::uninit(),
        }
    }

    /// MIDI OUT's endpoint, if the class was allocated: the closed path
    /// drains it by address.
    #[inline(always)]
    pub(crate) fn out(&self) -> Option<EndpointAddress> {
        self.out
    }

    #[inline(always)]
    fn class(&mut self) -> Option<&mut MidiClass> {
        // SAFETY: `out` is set only by `door::alloc`, after it writes `class`.
        self.out.map(|_| unsafe { self.class.assume_init_mut() })
    }
}

/// USB MIDI in's pipe and sinks, and out's ring, packer and the packets
/// the endpoint hasn't taken yet. The rings are set before `connect`; from
/// then on only the poll touches it, under the cell's exclusion.
struct MidiIn {
    pipe: PipeSlot,
    notes: Option<NoteProducer<'static>>,
    ctrl: Option<&'static UsbCtrl>,
    out: Option<&'static OutRing>,
    packer: Packer,
    held: [[u8; 4]; 16],
    n: usize,
}

static mut IN: MidiIn = MidiIn {
    pipe: PipeSlot::new(),
    notes: None,
    ctrl: None,
    out: None,
    packer: Packer::new(),
    held: [[0; 4]; 16],
    n: 0,
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
    i.out = out;
}

/// The board door's USB entries.
#[repr(C)]
pub(crate) struct UsbDoor {
    alloc: fn(&'static UsbBusAllocator<Bus>, &mut MidiSlot),
    poll: fn(&mut UsbCore),
}

impl UsbDoor {
    pub(crate) const DOOR: UsbDoor = UsbDoor {
        alloc: door::alloc,
        poll: door::poll,
    };
}

/// The class's interfaces and endpoints into `slot`, through the door:
/// closed, nothing is allocated and MIDI stays down this boot.
pub(crate) fn alloc(bus: &'static UsbBusAllocator<Bus>, slot: &mut MidiSlot) {
    crate::bank2::board_door(|d| (d.usb.alloc)(bus, slot));
}

/// Console and MIDI polled through the door; `None` closed.
#[inline(always)]
pub(crate) fn poll(core: &mut UsbCore) -> Option<()> {
    crate::bank2::board_door(|d| (d.usb.poll)(core))
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
    use chimera_core::out_ring::OutRing;
    use chimera_midi::usb::{Cable, Packer, unpack};
    use chimera_midi::usb_class::{Ids, descriptors as write};
    #[cfg(not(feature = "midi-map"))]
    use midi_in::{feed, lost};
    use usb_device::UsbError;
    use usb_device::class_prelude::{DescriptorWriter, UsbBusAllocator};
    use usb_device::device::UsbDeviceState;

    use super::{IN, MidiClass, MidiIn, MidiSlot};
    use crate::usb::{Bus, IrqCounts, UsbCore};

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

    pub(super) fn alloc(bus: &'static UsbBusAllocator<Bus>, slot: &mut MidiSlot) {
        let c = MidiClass {
            ac: bus.interface(),
            ms: bus.interface(),
            name: bus.string(),
            out: bus.bulk(PACKET),
            inp: bus.bulk(PACKET),
            descriptors,
        };
        let out = c.out.address();
        slot.class.write(c);
        slot.out = Some(out);
    }

    /// The device polled with both classes; leaving Configured, every
    /// Part let go as `lets_go` rules; then every OUT packet read, cable 0's bytes fed to
    /// USB's pipe here in bank 2 (its ring is not DIN's), the rest dropped and counted; then
    /// the out ring sent (`send`).
    pub(super) fn poll(core: &mut UsbCore) {
        let UsbCore {
            dev,
            serial,
            midi,
            irq,
        } = core;
        let Some(m) = midi.class() else {
            dev.poll(&mut [serial]);
            return;
        };
        dev.poll(&mut [serial, m]);
        // SAFETY: only the poll touches `IN` once `connect` has run, and
        // the poll runs in the handler or under `with`, which masks it.
        let i = unsafe { &mut *addr_of_mut!(IN) };
        let MidiIn {
            pipe, notes, ctrl, ..
        } = &mut *i;
        let mut sinks = notes
            .as_mut()
            .zip(*ctrl)
            .map(|(notes, ctrl)| DinSinks { notes, ctrl });
        let up = dev.state() == UsbDeviceState::Configured;
        if irq.configured != up {
            irq.configured = up;
            let left = match dev.state() {
                UsbDeviceState::Suspend => Left::Suspend,
                _ => Left::Reset,
            };
            if let (false, Some(s)) = (up, sinks.as_mut())
                && midi_in::lets_go(left, pipe.played())
            {
                lost(pipe, s);
            }
        }
        let mut p = [0u8; PACKET as usize];
        while let Ok(n) = m.out.read(&mut p) {
            for &e in p[..n].as_chunks::<4>().0 {
                match (unpack(e), sinks.as_mut()) {
                    (Some((Cable::ZERO, b)), Some(s)) => feed(pipe, b.as_slice(), s),
                    _ => irq.midi_drops = irq.midi_drops.wrapping_add(1),
                }
            }
        }
        if let Some(ring) = i.out {
            send(m, ring, i, irq, up);
        }
    }

    /// The UI's lost echo counted; then, Configured, `ring`'s bytes packed
    /// into IN packets while the endpoint takes them; busy, held for the
    /// next poll; any other error, dropped and counted. Not Configured, all
    /// dropped: a host that comes back hears nothing stale.
    fn send(m: &mut MidiClass, ring: &OutRing, i: &mut MidiIn, irq: &mut IrqCounts, up: bool) {
        irq.midi_drops = irq.midi_drops.wrapping_add(ring.take_dropped());
        if !up {
            while ring.pop().is_some() {}
            (i.packer, i.n) = (Packer::new(), 0);
            return;
        }
        loop {
            while i.n < i.held.len()
                && let Some(x) = ring.pop()
            {
                if let Some(p) = i.packer.push(x) {
                    i.held[i.n] = p;
                    i.n += 1;
                }
            }
            let (held, _) = i.held.as_flattened().split_at(i.n * 4);
            if held.is_empty() {
                return;
            }
            match m.inp.write(held) {
                Err(UsbError::WouldBlock) => return,
                Err(_) => irq.midi_drops = irq.midi_drops.wrapping_add(i.n as u32),
                Ok(_) => {}
            }
            i.n = 0;
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
