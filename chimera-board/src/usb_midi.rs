//! USB MIDI 1.0, one cable (spec § 4): an AudioControl and a MIDIStreaming
//! interface, one embedded and one external jack each way, both named
//! "Chimera", and a 64-byte bulk endpoint each way. In-tree, not
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
    use usb_device::class_prelude::{DescriptorWriter, UsbBusAllocator};

    use super::{MidiClass, MidiSlot};
    use crate::usb::{Bus, UsbCore};

    const IAD: u8 = 0x0B;
    const AUDIO: u8 = 0x01;
    const AUDIOCONTROL: u8 = 0x01;
    const MIDISTREAMING: u8 = 0x03;
    const CS_INTERFACE: u8 = 0x24;
    const CS_ENDPOINT: u8 = 0x25;
    const HEADER: u8 = 0x01;
    const MIDI_IN_JACK: u8 = 0x02;
    const MIDI_OUT_JACK: u8 = 0x03;
    const MS_GENERAL: u8 = 0x01;
    const EMBEDDED: u8 = 0x01;
    const EXTERNAL: u8 = 0x02;

    /// Jack IDs: embedded IN (the host's OUT endpoint feeds it), external
    /// IN, embedded OUT (feeds the host's IN endpoint), external OUT.
    const IN_EMB: u8 = 1;
    const IN_EXT: u8 = 2;
    const OUT_EMB: u8 = 3;
    const OUT_EXT: u8 = 4;

    /// The class-specific MIDIStreaming descriptors' total (class spec
    /// B.4.2): header 7, IN jacks 6 + 6, OUT jacks 9 + 9, each endpoint
    /// 9 + 5.
    const MS_TOTAL: u16 = 7 + 6 + 6 + 9 + 9 + (9 + 5) * 2;

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

    fn descriptors(c: &MidiClass, w: &mut DescriptorWriter) -> usb_device::Result<()> {
        // The IAD written raw: `iad` called from here too would leave an
        // outlined copy in bank 1. The device is always composite.
        w.write(IAD, &[c.ac.into(), 2, AUDIO, 0x00, 0x00, 0])?;
        w.interface(c.ac, AUDIO, AUDIOCONTROL, 0)?;
        // bcdADC 1.0, wTotalLength 9, one streaming interface.
        w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, 9, 0, 1, c.ms.into()])?;
        w.interface(c.ms, AUDIO, MIDISTREAMING, 0)?;
        let [lo, hi] = MS_TOTAL.to_le_bytes();
        w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, lo, hi])?;
        let name = c.name.into();
        w.write(CS_INTERFACE, &[MIDI_IN_JACK, EMBEDDED, IN_EMB, name])?;
        w.write(CS_INTERFACE, &[MIDI_IN_JACK, EXTERNAL, IN_EXT, name])?;
        w.write(
            CS_INTERFACE,
            &[MIDI_OUT_JACK, EMBEDDED, OUT_EMB, 1, IN_EXT, 1, name],
        )?;
        w.write(
            CS_INTERFACE,
            &[MIDI_OUT_JACK, EXTERNAL, OUT_EXT, 1, IN_EMB, 1, name],
        )?;
        // Audio's 9-byte endpoints: bRefresh and bSynchAddress 0.
        w.endpoint_ex(&c.out, zeros)?;
        w.write(CS_ENDPOINT, &[MS_GENERAL, 1, IN_EMB])?;
        w.endpoint_ex(&c.inp, zeros)?;
        w.write(CS_ENDPOINT, &[MS_GENERAL, 1, OUT_EMB])
    }

    /// bRefresh and bSynchAddress.
    fn zeros(b: &mut [u8]) -> usb_device::Result<usize> {
        b.get_mut(..2)
            .ok_or(usb_device::UsbError::BufferOverflow)?
            .fill(0);
        Ok(2)
    }
}
