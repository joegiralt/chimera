//! USB MIDI 1.0's class descriptors, one cable (spec § 4): an AudioControl
//! and a MIDIStreaming interface under one IAD, one embedded and one
//! external jack each way, all named by `name`, and a bulk endpoint each
//! way. With USB audio (ADR 0248), its streaming interface joins the same
//! function, between the two: one AudioControl lists both, as the MIDI
//! class spec's Audio function has it, so the configuration fits the
//! control buffer. The board's class writes these from bank 2; the host
//! test runs them through usb-device's own enumeration.

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

/// usb-device's control buffer (`control-buffer-256`): a GET_DESCRIPTOR
/// longer than this fails and the host gives the port up.
pub const CONTROL_BUF: usize = 256;

const IAD: u8 = 0x0B;
const AUDIO: u8 = 0x01;
const AUDIOCONTROL: u8 = 0x01;
const AUDIOSTREAMING: u8 = 0x02;
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

// UAC1's AudioControl and AudioStreaming subtypes (audio spec A.5, A.6).
const INPUT_TERMINAL: u8 = 0x02;
const OUTPUT_TERMINAL: u8 = 0x03;
const AS_GENERAL: u8 = 0x01;
const FORMAT_TYPE: u8 = 0x02;
const EP_GENERAL: u8 = 0x01;

/// The stream's terminals: the synth's output, as a line connector
/// (0x0603), into USB streaming (0x0101), the host's capture.
const IT_ID: u8 = 1;
const OT_ID: u8 = 2;
/// AudioControl's class-specific total with the stream: header 10, input
/// terminal 12, output terminal 9.
const AC_TOTAL: u16 = 10 + 12 + 9;

// With USB audio the function keeps a single MIDI cable: per-Part cables
// (#840) are the default build's; the stream's 7 B of control-buffer room
// has none for their jacks. #840 adapts this writer when both land.

/// USB audio's streaming interface and its isochronous IN.
pub struct Stream<'a, B: UsbBus> {
    pub iface: InterfaceNumber,
    pub ep: &'a EndpointIn<'a, B>,
}

/// The interfaces and the jacks' string.
#[derive(Clone, Copy)]
pub struct Ids {
    pub ac: InterfaceNumber,
    pub ms: InterfaceNumber,
    pub name: StringIndex,
}

/// The function's descriptors into `w`, with `audio`'s stream if on. The
/// IAD is written raw: the device is always composite, and the board's
/// bank 1 keeps no copy of usb-device's `iad` for it.
pub fn descriptors<B: UsbBus>(
    w: &mut DescriptorWriter,
    ids: Ids,
    out: &EndpointOut<B>,
    inp: &EndpointIn<B>,
    audio: Option<Stream<'_, B>>,
) -> usb_device::Result<()> {
    match audio {
        None => {
            w.write(IAD, &[ids.ac.into(), 2, AUDIO, 0x00, 0x00, 0])?;
            w.interface(ids.ac, AUDIO, AUDIOCONTROL, 0)?;
            // bcdADC 1.0, wTotalLength 9, one streaming interface.
            w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, 9, 0, 1, ids.ms.into()])?;
        }
        Some(s) => stream(w, ids, s)?,
    }
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

/// AudioControl over the stream and MIDI, the terminals, and the stream:
/// alt 0 empty (no bandwidth), alt 1 stereo 48 kHz 24-bit PCM on an
/// asynchronous isochronous IN, polled every frame.
fn stream<B: UsbBus>(
    w: &mut DescriptorWriter,
    ids: Ids,
    s: Stream<'_, B>,
) -> usb_device::Result<()> {
    use crate::uac::{CHANNELS, RATE, SUBFRAME};
    w.write(IAD, &[ids.ac.into(), 3, AUDIO, 0x00, 0x00, 0])?;
    w.interface(ids.ac, AUDIO, AUDIOCONTROL, 0)?;
    let [lo, hi] = AC_TOTAL.to_le_bytes();
    w.write(
        CS_INTERFACE,
        &[HEADER, 0x00, 0x01, lo, hi, 2, s.iface.into(), ids.ms.into()],
    )?;
    // Stereo, left and right front (wChannelConfig 0x0003).
    let ch = CHANNELS as u8;
    w.write(
        CS_INTERFACE,
        &[INPUT_TERMINAL, IT_ID, 0x03, 0x06, 0, ch, 0x03, 0x00, 0, 0],
    )?;
    w.write(
        CS_INTERFACE,
        &[OUTPUT_TERMINAL, OT_ID, 0x01, 0x01, 0, IT_ID, 0],
    )?;
    w.interface(s.iface, AUDIO, AUDIOSTREAMING, 0)?;
    w.interface_alt(s.iface, 1, AUDIO, AUDIOSTREAMING, 0, None)?;
    // Linked to the output terminal, a frame's delay, PCM.
    w.write(CS_INTERFACE, &[AS_GENERAL, OT_ID, 1, 0x01, 0x00])?;
    let [r0, r1, r2, _] = RATE.to_le_bytes();
    let sub = SUBFRAME as u8;
    w.write(
        CS_INTERFACE,
        &[FORMAT_TYPE, 0x01, ch, sub, 8 * sub, 1, r0, r1, r2],
    )?;
    w.endpoint_ex(s.ep, zeros)?;
    // No sampling-frequency or pitch control, no lock delay.
    w.write(CS_ENDPOINT, &[EP_GENERAL, 0x00, 0, 0, 0])
}

/// bRefresh and bSynchAddress.
fn zeros(b: &mut [u8]) -> usb_device::Result<usize> {
    b.get_mut(..2)
        .ok_or(usb_device::UsbError::BufferOverflow)?
        .fill(0);
    Ok(2)
}
