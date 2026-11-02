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

use core::mem::MaybeUninit;

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
    #[allow(dead_code)] // USB MIDI out writes it.
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
    use chimera_midi::usb_class::{Ids, descriptors as write};
    use usb_device::class_prelude::{DescriptorWriter, UsbBusAllocator};

    use super::{MidiClass, MidiSlot};
    use crate::usb::{Bus, UsbCore};

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

    /// The device polled with both classes, then every OUT packet read:
    /// dropped and counted until USB MIDI in feeds them to its pipe.
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
        let mut p = [0u8; PACKET as usize];
        while let Ok(n) = m.out.read(&mut p) {
            irq.midi_drops = irq.midi_drops.wrapping_add((n / 4) as u32);
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
