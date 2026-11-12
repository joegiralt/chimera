//! The configuration descriptor whole, as bytes (#885, design
//! 2026-11-10), for layouts past usb-device's 256-B control buffer: USB
//! audio both ways, the six-channel capture, the Part cables beside them.
//! usb-device writes a configuration into that buffer and stalls one
//! that doesn't fit (the host then gives the port up: 11b82462 stranded
//! the unit). But every control IN goes to the classes first, and a
//! class may answer from a `&'static [u8]` of any length
//! (`ControlIn::accept_with_static`), sent a packet at a time. So the
//! class builds these bytes once, at allocation, and answers
//! GET_DESCRIPTOR(configuration) itself. Not globbed into bank 2: bank
//! 1's, under ADR 0251.
//!
//! The bytes are `usb_class`'s layout, the console's CDC as usbd-serial
//! writes it, and the new interfaces after the capture stream: the host
//! test holds the shipping and bench layouts to what usb-device serves,
//! byte for byte.

use usb_device::bus::UsbBus;
use usb_device::class_prelude::{ControlIn, control};

use crate::{uac, uac_out};

/// The static's room: the largest layout (417 B) and spare.
pub const BUF: usize = 512;

/// The six-channel capture's frames a packet: the nominal and four to
/// spare, so that it and the playback stream fit full speed's periodic
/// budget together (936 + 312 + 3 B, with overheads 1,278 of 1,350).
pub const SIX_FRAMES: usize = uac::NOMINAL + 4;
pub const SIX_CHANNELS: u8 = 6;
pub const SIX_PACKET: u16 = (SIX_FRAMES * SIX_CHANNELS as usize * uac::SUBFRAME) as u16;
const _: () = assert!(SIX_PACKET <= 1023, "full speed's isochronous limit");

/// The endpoint addresses the allocator gave: the console's notification
/// IN, data IN and data OUT, then MIDI's OUT and IN.
#[derive(Clone, Copy, Debug)]
pub struct Eps {
    pub cdc: [u8; 3],
    pub midi_out: u8,
    pub midi_in: u8,
}

/// The console's endpoints as usbd-serial allocates them first on a
/// fresh bus (synopsys-usb-otg 0.4.0 takes the lowest free number past
/// EP0): notification IN 1, data OUT 1, data IN 2.
pub const CDC: [u8; 3] = [0x81, 0x82, 0x01];

/// The shipping layout with the host's playback (ADR 0260): `shipping`'s,
/// and the playback's OUT and feedback IN.
pub const fn duplex(
    name: u8,
    midi_out: u8,
    midi_in: u8,
    capture: u8,
    (play, fb): (u8, u8),
) -> Layout {
    let mut l = shipping(name, midi_out, midi_in, capture);
    l.audio = Some(Audio {
        capture,
        six: false,
        playback: Some((play, fb)),
    });
    l
}

/// The shipping layout (ADR 0256: one cable, the stereo capture), with
/// cable 0's string `name` and the endpoints MIDI and the capture got.
pub const fn shipping(name: u8, midi_out: u8, midi_in: u8, capture: u8) -> Layout {
    Layout {
        cables: 1,
        name,
        eps: Eps {
            cdc: CDC,
            midi_out,
            midi_in,
        },
        audio: Some(Audio {
            capture,
            six: false,
            playback: None,
        }),
    }
}

/// What the class answers in usb-device's stead: the configuration, and
/// while a static reply may be stale the device descriptor, strings 0 to
/// 4 and the BOS. Each a whole descriptor, `'static` for
/// `accept_with_static`.
#[derive(Clone, Copy, Debug)]
pub struct Replies {
    pub config: &'static [u8],
    pub device: &'static [u8],
    pub strings: [&'static [u8]; 5],
}

/// The device descriptor `usb_class::device` builds: USB 2.1, composite of
/// IADs (EF/02/01), EP0 of 8 bytes, strings 1 to 3, one configuration.
pub const fn device_descriptor(vid: u16, pid: u16, release: u16) -> [u8; 18] {
    let ([v0, v1], [p0, p1], [r0, r1]) =
        (vid.to_le_bytes(), pid.to_le_bytes(), release.to_le_bytes());
    [
        18, 1, 0x10, 0x02, 0xEF, 0x02, 0x01, EP0 as u8, v0, v1, p0, p1, r0, r1, 1, 2, 3, 1,
    ]
}

/// `s` as a string descriptor (UTF-16LE, ASCII) into `out`; its length.
pub fn string(s: &str, out: &mut [u8]) -> Option<usize> {
    let n = 2 + 2 * s.len();
    let d = out.get_mut(..n)?;
    d[0] = n as u8;
    d[1] = 3;
    for (w, b) in d[2..].chunks_mut(2).zip(s.bytes()) {
        w.copy_from_slice(&[b, 0]);
    }
    Some(n)
}

/// String 0, English (US) alone, as usb-device writes it.
pub static LANGS: [u8; 4] = [4, 3, 0x09, 0x04];
/// "Chimera": strings 1 and 2 (manufacturer, product) and the one cable's
/// jack.
pub static CHIMERA: [u8; 16] = [
    16, 3, b'C', 0, b'h', 0, b'i', 0, b'm', 0, b'e', 0, b'r', 0, b'a', 0,
];
/// The BOS usb-device writes for USB 2.1: its USB 2.0 extension, no LPM.
static BOS: [u8; 12] = [5, 0x0F, 12, 0, 1, 7, 0x10, 2, 0, 0, 0, 0];
/// GET_STATUS's words, by value 0 to 3.
static STATUS: [[u8; 2]; 4] = [[0, 0], [1, 0], [2, 0], [3, 0]];
/// GET_CONFIGURATION's and GET_INTERFACE's byte, by value 0 to 3.
static BYTE: [[u8; 1]; 4] = [[0], [1], [2], [3]];
/// EP0's packet: usb-device's default.
const EP0: usize = 8;

/// The class's side of usb-device 0.3.2's control replies (ADR 0260).
/// A static reply (`accept_with_static`) stays usb-device's source until
/// its data stage ends: a bus reset, a new SETUP or a host that stops
/// reading leaves it set, and usb-device's next buffered reply sends its
/// bytes. So once a static reply longer than a packet starts, `stale`,
/// the class answers every standard IN reply usb-device would buffer
/// (descriptors, GET_STATUS, GET_CONFIGURATION, GET_INTERFACE) itself,
/// statically, with what usb-device would send, from what it saw: the
/// configuration, remote wakeup and halts set and cleared, and bus resets.
/// A reply of one packet ends its data stage at once and clears it.
/// usbd-serial's GET_LINE_CODING, the one class IN reply, is answered
/// before the class sees it: a read cut short right before it is the
/// one case left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Owner {
    stale: bool,
    configured: bool,
    wakeup: bool,
    /// Halted endpoints: bit `n` OUT n, bit `16 + n` IN n.
    halted: u32,
}

impl Owner {
    pub const fn new() -> Owner {
        Owner {
            stale: false,
            configured: false,
            wakeup: false,
            halted: 0,
        }
    }

    /// A bus reset: unconfigured, no remote wakeup, nothing halted (as
    /// usb-device's and the bus's own reset). A stale reply stays stale.
    pub fn reset(&mut self) {
        let stale = self.stale;
        *self = Owner::new();
        self.stale = stale;
    }

    /// A control OUT request, seen as usb-device will take it.
    pub fn control_out(&mut self, r: &control::Request) {
        use control::{Recipient as To, Request as Q};
        if r.request_type != control::RequestType::Standard {
            return;
        }
        let ep = 1u32 << ((r.index & 0x0F) as u32 + if r.index & 0x80 != 0 { 16 } else { 0 });
        match (r.recipient, r.request, r.value) {
            (To::Device, Q::SET_CONFIGURATION, v @ 0..=1) => self.configured = v == 1,
            (To::Device, Q::SET_FEATURE, Q::FEATURE_DEVICE_REMOTE_WAKEUP) => self.wakeup = true,
            (To::Device, Q::CLEAR_FEATURE, Q::FEATURE_DEVICE_REMOTE_WAKEUP) => self.wakeup = false,
            (To::Endpoint, Q::SET_FEATURE, Q::FEATURE_ENDPOINT_HALT) => self.halted |= ep,
            (To::Endpoint, Q::CLEAR_FEATURE, Q::FEATURE_ENDPOINT_HALT) => self.halted &= !ep,
            _ => {}
        }
    }

    /// The class's `control_in`: the configuration always, the rest of
    /// the standard IN replies while a static reply may be stale, each
    /// cut to wLength; anything else left to the next class and
    /// usb-device. `alt` is the alt setting of an interface (0 for one
    /// with none).
    pub fn control_in<B: UsbBus>(
        &mut self,
        xfer: ControlIn<B>,
        a: &Replies,
        alt: impl FnOnce(u8) -> u8,
    ) {
        use control::{Recipient as To, Request as Q};
        let r = *xfer.request();
        if r.request_type != control::RequestType::Standard {
            return;
        }
        let english = r.index == 0x0409;
        let reply: Option<&'static [u8]> = match (r.recipient, r.request) {
            (To::Device, Q::GET_DESCRIPTOR) => match r.descriptor_type_index() {
                (2, 0) => Some(a.config),
                _ if !self.stale => None,
                (1, 0) => Some(a.device),
                (3, 0) => Some(a.strings[0]),
                (3, i @ 1..=3) if english => Some(a.strings[usize::from(i)]),
                // The jack's name, as the class's `get_string`: any language.
                (3, 4) => Some(a.strings[4]),
                (0x0F, 0) => Some(&BOS),
                _ => None,
            },
            _ if !self.stale => None,
            // usb-device's run-time self-powered bit starts clear and
            // nothing sets it (`set_self_powered`), whatever the
            // configuration's bmAttributes say.
            (To::Device, Q::GET_STATUS) => Some(&STATUS[2 * usize::from(self.wakeup)]),
            (To::Interface, Q::GET_STATUS) => Some(&STATUS[0]),
            (To::Endpoint, Q::GET_STATUS) => {
                let ep = (r.index & 0x0F) as u32 + if r.index & 0x80 != 0 { 16 } else { 0 };
                Some(&STATUS[(self.halted >> ep & 1) as usize])
            }
            (To::Device, Q::GET_CONFIGURATION) => Some(&BYTE[usize::from(self.configured)]),
            (To::Interface, Q::GET_INTERFACE) if r.index <= 0xFF => {
                Some(&BYTE[usize::from(alt(r.index as u8).min(3))])
            }
            _ => None,
        };
        if let Some(b) = reply {
            let n = b.len().min(usize::from(r.length));
            // One packet ends its data stage now; a longer one, or a
            // refused one, may be left behind.
            self.stale = xfer.accept_with_static(b).is_err() || n > EP0;
        }
    }
}

/// USB audio: the capture's isochronous IN (alt 1 stereo, alt 2 the three
/// DAC pairs if `six`), and the playback's OUT with its feedback IN.
#[derive(Clone, Copy, Debug)]
pub struct Audio {
    pub capture: u8,
    pub six: bool,
    pub playback: Option<(u8, u8)>,
}

/// A layout: interfaces 0–1 the console, 2 AudioControl, 3 the capture
/// if any, then MIDIStreaming, then the playback if any. `name` is cable
/// 0's string, cable n's `name + n`.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub cables: u8,
    pub name: u8,
    pub eps: Eps,
    pub audio: Option<Audio>,
}

impl Layout {
    /// Interfaces in the Audio function: AudioControl, the streams, MIDI.
    const fn audio_ifaces(&self) -> u8 {
        match self.audio {
            None => 2,
            Some(Audio { playback: None, .. }) => 3,
            Some(_) => 4,
        }
    }
}

/// The layout's length: what `write` makes, without making it.
pub const fn len(l: &Layout) -> usize {
    let streams = match l.audio {
        None => 0,
        Some(a) => {
            STEREO_IN
                + if a.six { SIX_ALT } else { 0 }
                + if a.playback.is_some() { PLAYBACK } else { 0 }
        }
    };
    9 + 66 + 8 + 9 + ac_total(l) + streams + 9 + crate::usb_class::ms_total(l.cables) as usize
}

/// AudioControl's class-specific total: its header (8 and a byte a
/// streaming interface), each stream's input terminal 12 and output 9.
const fn ac_total(l: &Layout) -> usize {
    let streams = l.audio_ifaces() as usize - 1;
    8 + streams + (streams - 1) * (12 + 9)
}

/// The capture: alt 0, alt 1, AS general, format, its endpoint 9 and 7.
const STEREO_IN: usize = 9 + 9 + 7 + 11 + 9 + 7;
/// The six-channel alt: interface, AS general, format, endpoint 9 and 7.
const SIX_ALT: usize = 9 + 7 + 11 + 9 + 7;
/// The playback: alt 0, alt 1, AS general, format, its endpoint 9 and 7,
/// the feedback endpoint 9.
const PLAYBACK: usize = 9 + 9 + 7 + 11 + 9 + 7 + 9;

/// The largest layout, every Part cable beside both streams and the six
/// channels, fits the static.
const _: () = assert!(
    len(&Layout {
        cables: crate::usb_class::CABLES,
        name: 0,
        eps: Eps {
            cdc: [0; 3],
            midi_out: 0,
            midi_in: 0
        },
        audio: Some(Audio {
            capture: 0,
            six: true,
            playback: Some((0, 0))
        }),
    }) <= BUF
);

/// Descriptors appended to a buffer, each its length and type first.
struct W<'a> {
    b: &'a mut [u8],
    at: usize,
}

impl W<'_> {
    fn put(&mut self, kind: u8, body: &[u8]) -> Option<()> {
        let n = body.len() + 2;
        let d = self.b.get_mut(self.at..self.at + n)?;
        d[0] = n as u8;
        d[1] = kind;
        d[2..].copy_from_slice(body);
        self.at += n;
        Some(())
    }

    fn iface(&mut self, n: u8, alt: u8, eps: u8, sub: u8) -> Option<()> {
        self.put(INTERFACE, &[n, alt, eps, AUDIO, sub, 0, 0])
    }

    /// Audio's 9-byte isochronous endpoint, every frame.
    fn iso(&mut self, addr: u8, attr: u8, max: u16, refresh: u8, sync: u8) -> Option<()> {
        let [lo, hi] = max.to_le_bytes();
        self.put(ENDPOINT, &[addr, attr, lo, hi, 1, refresh, sync])
    }

    /// Type I PCM, 3-byte subframes, one rate.
    fn format(&mut self, ch: u8) -> Option<()> {
        let [r0, r1, r2, _] = uac::RATE.to_le_bytes();
        let sub = uac::SUBFRAME as u8;
        self.put(
            CS_INTERFACE,
            &[FORMAT_TYPE, 1, ch, sub, 8 * sub, 1, r0, r1, r2],
        )
    }
}

const INTERFACE: u8 = 4;
const ENDPOINT: u8 = 5;
const IAD: u8 = 0x0B;
const AUDIO: u8 = 0x01;
const CS_INTERFACE: u8 = 0x24;
const CS_ENDPOINT: u8 = 0x25;
const FORMAT_TYPE: u8 = 0x02;
/// Isochronous asynchronous data; isochronous feedback (USB 2.0 Table
/// 9-13; Linux takes it as explicit, only 0x20 being implicit).
const ISO_ASYNC: u8 = 0x05;
const ISO_FEEDBACK: u8 = 0x11;

// The terminals: 1 the synth's outs (a line connector) into 2, USB
// streaming, the host's capture; 3 USB streaming, the host's playback,
// into 4, a line connector, the synth's main out.
const IT_SYNTH: u8 = 1;
const OT_CAPTURE: u8 = 2;
const IT_PLAYBACK: u8 = 3;
const OT_MAIN: u8 = 4;

/// The layout into `buf`; its length, or `None` past the buffer.
pub fn write(buf: &mut [u8], l: &Layout) -> Option<usize> {
    let mut w = W { b: buf, at: 0 };
    let n_ifaces = 2 + l.audio_ifaces();
    // wTotalLength is set at the end. Self-powered, 100 mA.
    w.put(2, &[0, 0, n_ifaces, 1, 0, 0xC0, 50])?;
    // The console, as usbd-serial 0.2 writes it.
    let [notify, data_in, data_out] = l.eps.cdc;
    w.put(IAD, &[0, 2, 0x02, 0x02, 0x00, 0])?;
    w.put(INTERFACE, &[0, 0, 1, 0x02, 0x02, 0x00, 0])?;
    w.put(CS_INTERFACE, &[0x00, 0x10, 0x01])?;
    w.put(CS_INTERFACE, &[0x02, 0x00])?;
    w.put(CS_INTERFACE, &[0x06, 0, 1])?;
    w.put(CS_INTERFACE, &[0x01, 0x00, 1])?;
    w.put(ENDPOINT, &[notify, 0x03, 8, 0, 255])?;
    w.put(INTERFACE, &[1, 0, 2, 0x0A, 0x00, 0x00, 0])?;
    w.put(ENDPOINT, &[data_in, 0x02, 64, 0, 0])?;
    w.put(ENDPOINT, &[data_out, 0x02, 64, 0, 0])?;

    // The Audio function: AudioControl, the capture, MIDIStreaming, the
    // playback.
    let ifaces = l.audio_ifaces();
    // MIDIStreaming follows AudioControl and the capture; the playback
    // follows it, so MIDI's interface is the same with or without it.
    let ms = if l.audio.is_some() { 4 } else { 3 };
    w.put(IAD, &[2, ifaces, AUDIO, 0x00, 0x00, 0])?;
    w.iface(2, 0, 0, 0x01)?;
    match l.audio {
        // bcdADC 1.0, wTotalLength 9, MIDI alone.
        None => w.put(CS_INTERFACE, &[0x01, 0x00, 0x01, 9, 0, 1, ms])?,
        Some(a) => {
            let [lo, hi] = (ac_total(l) as u16).to_le_bytes();
            let mut hdr = [0x01, 0x00, 0x01, lo, hi, ifaces - 1, 3, 4, 5];
            hdr[6..6 + usize::from(ifaces - 1)]
                .iter_mut()
                .zip(3..)
                .for_each(|(b, i)| *b = i);
            w.put(CS_INTERFACE, &hdr[..6 + usize::from(ifaces - 1)])?;
            // The synth's outs: L R of the main pair first (predefined),
            // pairs 2 and 3 after (non-predefined, UAC1 § 3.7.2.3) when
            // the six-channel alt carries them.
            let ch = if a.six { SIX_CHANNELS } else { 2 };
            w.put(
                CS_INTERFACE,
                &[0x02, IT_SYNTH, 0x03, 0x06, 0, ch, 0x03, 0x00, 0, 0],
            )?;
            w.put(
                CS_INTERFACE,
                &[0x03, OT_CAPTURE, 0x01, 0x01, 0, IT_SYNTH, 0],
            )?;
            if a.playback.is_some() {
                w.put(
                    CS_INTERFACE,
                    &[0x02, IT_PLAYBACK, 0x01, 0x01, 0, 2, 0x03, 0x00, 0, 0],
                )?;
                w.put(
                    CS_INTERFACE,
                    &[0x03, OT_MAIN, 0x03, 0x06, 0, IT_PLAYBACK, 0],
                )?;
            }
            // The capture: alt 0 empty, alt 1 stereo, alt 2 six channels.
            w.iface(3, 0, 0, 0x02)?;
            w.iface(3, 1, 1, 0x02)?;
            w.put(CS_INTERFACE, &[0x01, OT_CAPTURE, 1, 0x01, 0x00])?;
            w.format(2)?;
            w.iso(a.capture, ISO_ASYNC, uac::PACKET, 0, 0)?;
            w.put(CS_ENDPOINT, &[0x01, 0x00, 0, 0, 0])?;
            if a.six {
                w.iface(3, 2, 1, 0x02)?;
                w.put(CS_INTERFACE, &[0x01, OT_CAPTURE, 1, 0x01, 0x00])?;
                w.format(SIX_CHANNELS)?;
                w.iso(a.capture, ISO_ASYNC, SIX_PACKET, 0, 0)?;
                w.put(CS_ENDPOINT, &[0x01, 0x00, 0, 0, 0])?;
            }
        }
    }
    // MIDIStreaming, as `usb_class::descriptors` writes it.
    w.iface(ms, 0, 2, 0x03)?;
    let cables = l.cables.clamp(1, crate::usb_class::CABLES);
    let [lo, hi] = crate::usb_class::ms_total(cables).to_le_bytes();
    w.put(CS_INTERFACE, &[0x01, 0x00, 0x01, lo, hi])?;
    let name = l.name;
    w.put(CS_INTERFACE, &[0x02, 0x01, 1, name])?;
    w.put(CS_INTERFACE, &[0x02, 0x02, 2, name])?;
    w.put(CS_INTERFACE, &[0x03, 0x01, 3, 1, 2, 1, name])?;
    w.put(CS_INTERFACE, &[0x03, 0x02, 4, 1, 1, 1, name])?;
    for n in 1..cables {
        w.put(CS_INTERFACE, &[0x02, 0x01, 4 + n, name + n])?;
    }
    w.put(ENDPOINT, &[l.eps.midi_out, 0x02, 64, 0, 0, 0, 0])?;
    let mut cs = [1u8; 2 + crate::usb_class::CABLES as usize];
    cs[0] = 0x01;
    cs[1] = cables;
    for n in 1..cables {
        cs[2 + usize::from(n)] = 4 + n;
    }
    w.put(CS_ENDPOINT, &cs[..2 + usize::from(cables)])?;
    w.put(ENDPOINT, &[l.eps.midi_in, 0x02, 64, 0, 0, 0, 0])?;
    w.put(CS_ENDPOINT, &[0x01, 1, 3])?;
    // The playback: alt 0 empty, alt 1 stereo on an asynchronous OUT
    // synced by its feedback IN (10.14, every 2^REFRESH frames; its own
    // bSynchAddress 0, as Linux asks).
    if let Some(Audio {
        playback: Some((data, fb)),
        ..
    }) = l.audio
    {
        let p = ms + 1;
        w.iface(p, 0, 0, 0x02)?;
        w.iface(p, 1, 2, 0x02)?;
        w.put(CS_INTERFACE, &[0x01, IT_PLAYBACK, 1, 0x01, 0x00])?;
        w.format(2)?;
        w.iso(data, ISO_ASYNC, uac_out::PACKET, 0, fb)?;
        w.put(CS_ENDPOINT, &[0x01, 0x00, 0, 0, 0])?;
        w.iso(fb, ISO_FEEDBACK, 3, uac_out::REFRESH, 0)?;
    }
    let n = w.at;
    let [lo, hi] = (n as u16).to_le_bytes();
    buf[2] = lo;
    buf[3] = hi;
    Some(n)
}
