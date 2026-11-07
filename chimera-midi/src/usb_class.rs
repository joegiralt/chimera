//! USB MIDI 1.0's class descriptors, one cable (spec § 4): an AudioControl
//! and a MIDIStreaming interface under one IAD, one embedded and one
//! external jack each way, all named by `name`, and a bulk endpoint each
//! way. The board's class writes these from bank 2; the host test runs
//! them through usb-device's own enumeration.

use usb_device::class_prelude::{
    DescriptorWriter, EndpointIn, EndpointOut, InterfaceNumber, StringIndex, UsbBus,
    UsbBusAllocator,
};
use usb_device::device::{StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbVidPid};

/// The console's class, as usbd-serial names it.
const CDC: u8 = 0x02;

/// Chimera's device on `bus`: the strings, `serial` its serial number,
/// self-powered at 100 mA; the console alone a CDC device, with USB MIDI
/// (`midi`, its device release) a composite of IADs. The board's `connect`
/// and the host test build it here alike.
#[inline(always)]
pub fn device<'a, B: UsbBus>(
    bus: &'a UsbBusAllocator<B>,
    (vid, pid): (u16, u16),
    serial: &'a str,
    midi: Option<u16>,
) -> UsbDevice<'a, B> {
    let strings = StringDescriptors::default()
        .manufacturer("Chimera")
        .product("Chimera")
        .serial_number(serial);
    let dev = UsbDeviceBuilder::new(bus, UsbVidPid(vid, pid))
        .strings(&[strings])
        .expect("one language")
        .self_powered(true)
        .max_power(100)
        .expect("100 mA is within 500");
    match midi {
        Some(release) => dev.composite_with_iads().device_release(release),
        None => dev.device_class(CDC),
    }
    .build()
}

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

/// Jack IDs: embedded IN (the host's OUT endpoint feeds it), external IN,
/// embedded OUT (feeds the host's IN endpoint), external OUT.
const IN_EMB: u8 = 1;
const IN_EXT: u8 = 2;
const OUT_EMB: u8 = 3;
const OUT_EXT: u8 = 4;

/// The class-specific MIDIStreaming descriptors' total (class spec
/// B.4.2): header 7, IN jacks 6 + 6, OUT jacks 9 + 9, each endpoint 9 + 5.
pub const MS_TOTAL: u16 = 7 + 6 + 6 + 9 + 9 + (9 + 5) * 2;

/// The interfaces and the jacks' string.
#[derive(Clone, Copy)]
pub struct Ids {
    pub ac: InterfaceNumber,
    pub ms: InterfaceNumber,
    pub name: StringIndex,
}

/// The function's descriptors into `w`. The IAD is written raw: the
/// device is always composite, and the board's bank 1 keeps no copy of
/// usb-device's `iad` for it.
pub fn descriptors<B: UsbBus>(
    w: &mut DescriptorWriter,
    ids: Ids,
    out: &EndpointOut<B>,
    inp: &EndpointIn<B>,
) -> usb_device::Result<()> {
    w.write(IAD, &[ids.ac.into(), 2, AUDIO, 0x00, 0x00, 0])?;
    w.interface(ids.ac, AUDIO, AUDIOCONTROL, 0)?;
    // bcdADC 1.0, wTotalLength 9, one streaming interface.
    w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, 9, 0, 1, ids.ms.into()])?;
    w.interface(ids.ms, AUDIO, MIDISTREAMING, 0)?;
    let [lo, hi] = MS_TOTAL.to_le_bytes();
    w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, lo, hi])?;
    let name = ids.name.into();
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
    w.endpoint_ex(out, zeros)?;
    w.write(CS_ENDPOINT, &[MS_GENERAL, 1, IN_EMB])?;
    w.endpoint_ex(inp, zeros)?;
    w.write(CS_ENDPOINT, &[MS_GENERAL, 1, OUT_EMB])
}

/// bRefresh and bSynchAddress.
fn zeros(b: &mut [u8]) -> usb_device::Result<usize> {
    b.get_mut(..2)
        .ok_or(usb_device::UsbError::BufferOverflow)?
        .fill(0);
    Ok(2)
}
