// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! USB MIDI 1.0's class descriptors (spec § 4): an AudioControl and a
//! MIDIStreaming interface under one IAD, cable 0's embedded and external
//! jack each way, an embedded IN jack a Part cable (ADR 0243), each named,
//! and a bulk endpoint each way. With USB audio (ADR 0248), its streaming interface joins the same
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
/// longer than this fails and the host gives the port up (175 B past 128
/// once stranded the unit).
pub const CONTROL_BUF: usize = 256;

/// Room the default configuration keeps under the buffer.
pub const MARGIN: usize = 32;

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
/// embedded OUT (feeds the host's IN endpoint), external OUT: cable 0's.
/// The Parts' embedded IN jacks follow, `PART_IN + n` Part n's.
const IN_EMB: u8 = 1;
const IN_EXT: u8 = 2;
const OUT_EMB: u8 = 3;
const OUT_EXT: u8 = 4;
const PART_IN: u8 = 4;

/// Cables into Chimera (ADR 0243): cable 0 "Chimera", every channel;
/// cable n Part n's, whatever channel it sends on. Out of Chimera, cable 0
/// alone. Each Part cable is an embedded IN jack and a byte of the OUT
/// endpoint's list, 7 B.
pub const CABLES: u8 = 7;

/// The cables a build has: one beside USB audio's stream, whose 74 B
/// leave the buffer no room for the Parts' 42 (ADR 0243, 0248).
pub const fn cable_count(audio: bool) -> u8 {
    if audio { 1 } else { CABLES }
}

/// The class-specific MIDIStreaming descriptors' total (class spec
/// B.4.2) with `cables`: header 7, IN jacks 6 + 6, OUT jacks 9 + 9, each
/// endpoint 9 + 5, and each Part cable's IN jack 6 and list byte 1.
pub const fn ms_total(cables: u8) -> u16 {
    7 + 6 + 6 + 9 + 9 + (9 + 5) * 2 + (cables as u16 - 1) * (6 + 1)
}

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

/// The console's CDC function as usbd-serial writes it, its IAD included.
const CDC_LEN: usize = 66;
/// The stream's interface: alt 0 9, alt 1 9, AS general 7, format 11, its
/// endpoint 9 and 7.
const STREAM_LEN: usize = 9 + 9 + 7 + 11 + 9 + 7;

/// The configuration a build serves: its own 9 B, the console, and MIDI's
/// function (IAD 8, AudioControl 9 and its class-specific block, USB
/// audio's stream if on, MIDIStreaming 9 and `ms_total`). The host test
/// checks each layout against what enumerates.
pub const fn config_len(audio: bool) -> usize {
    let (ac, stream) = if audio {
        (AC_TOTAL as usize, STREAM_LEN)
    } else {
        (9, 0)
    };
    9 + CDC_LEN + 8 + 9 + ac + stream + 9 + ms_total(cable_count(audio)) as usize
}

// Each layout inside usb-device's control buffer, or the build fails: the
// bench builds' (217 B) with `MARGIN` to spare, the shipping builds' with
// USB audio (249 B, ADR 0256) with its one cable (ADR 0248's 7 B of room).
const _: () = assert!(config_len(false) + MARGIN <= CONTROL_BUF);
const _: () = assert!(config_len(true) <= CONTROL_BUF);

/// USB audio's streaming interface and its isochronous IN.
pub struct Stream<'a, B: UsbBus> {
    pub iface: InterfaceNumber,
    pub ep: &'a EndpointIn<'a, B>,
}

/// The interfaces, the jacks' first string (cable n's `name + n`) and the
/// cables.
#[derive(Clone, Copy)]
pub struct Ids {
    pub ac: InterfaceNumber,
    pub ms: InterfaceNumber,
    pub name: StringIndex,
    pub cables: u8,
}

/// A string a cable, in a row: the first.
pub fn names<B: UsbBus>(bus: &UsbBusAllocator<B>, cables: u8) -> StringIndex {
    let first = bus.string();
    for _ in 1..cables {
        let _ = bus.string();
    }
    first
}

/// The Parts' jacks' names; cable 0's "Chimera" is their head. A named
/// static, bytes and all, so bank 2's glob places them (a string
/// literal's anonymous rodata would land in bank 1).
static NAMES: [[u8; 14]; CABLES as usize - 1] = [
    *b"Chimera Part 1",
    *b"Chimera Part 2",
    *b"Chimera Part 3",
    *b"Chimera Part 4",
    *b"Chimera Part 5",
    *b"Chimera Part 6",
];

/// String `index`'s text if one of `ids`' jacks'.
pub fn jack_name(ids: Ids, index: StringIndex) -> Option<&'static str> {
    let n = u8::from(index).checked_sub(u8::from(ids.name))?;
    let name = match n.checked_sub(1) {
        _ if n >= ids.cables => return None,
        None => NAMES.first()?.get(..7)?,
        Some(p) => NAMES.get(usize::from(p))?.as_slice(),
    };
    core::str::from_utf8(name).ok()
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
    let cables = ids.cables.clamp(1, CABLES);
    let [lo, hi] = ms_total(cables).to_le_bytes();
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
    for n in 1..cables {
        w.write(
            CS_INTERFACE,
            &[MIDI_IN_JACK, EMBEDDED, PART_IN + n, name + n],
        )?;
    }
    // Audio's 9-byte endpoints: bRefresh and bSynchAddress 0. Host to
    // Chimera: every cable's IN jack, in cable order.
    w.endpoint_ex(out, zeros)?;
    let mut cs = [IN_EMB; 2 + CABLES as usize];
    cs[0] = MS_GENERAL;
    cs[1] = cables;
    for n in 1..cables {
        cs[2 + usize::from(n)] = PART_IN + n;
    }
    w.write(CS_ENDPOINT, &cs[..2 + usize::from(cables)])?;
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
