//! The configuration descriptor the unit serves, enumerated through
//! usb-device on a fake bus: the console's CDC (usbd-serial) alone (bank 2
//! closed at boot), with USB MIDI's function, and with USB audio's stream
//! in that function, as `chimera-board`'s `connect` builds the device. Each
//! must reach the host whole: past usb-device's control buffer,
//! GET_DESCRIPTOR fails and the host gives the port up (11b82462 on the
//! unit). The audio layout is parsed to the last byte: IADs, interfaces
//! and alt settings, the class-specific chain, endpoints.

use std::sync::Mutex;

use chimera_midi::uac;
use chimera_midi::uac_link::Alt;
use chimera_midi::usb_class::{CONTROL_BUF, Ids, MS_TOTAL, Stream, descriptors, device};
use usb_device::bus::PollResult;
use usb_device::class_prelude::*;
use usb_device::endpoint::{IsochronousSynchronizationType, IsochronousUsageType};
use usb_device::{Result, UsbDirection};
use usbd_serial::SerialPort;

#[derive(Default)]
struct State {
    next_in: usize,
    next_out: usize,
    setup: Option<[u8; 8]>,
    sent: Vec<u8>,
    in_done: bool,
    stalled: bool,
}

/// EP0 only: one SETUP in at a time, every IN packet kept.
#[derive(Default)]
struct Fake(Mutex<State>);

impl UsbBus for Fake {
    fn alloc_ep(
        &mut self,
        dir: UsbDirection,
        addr: Option<EndpointAddress>,
        _: EndpointType,
        _: u16,
        _: u8,
    ) -> Result<EndpointAddress> {
        if let Some(a) = addr {
            return Ok(a);
        }
        let s = self.0.get_mut().unwrap();
        let n = match dir {
            UsbDirection::In => &mut s.next_in,
            UsbDirection::Out => &mut s.next_out,
        };
        *n += 1;
        Ok(EndpointAddress::from_parts(*n, dir))
    }
    fn enable(&mut self) {}
    fn reset(&self) {}
    fn set_device_address(&self, _: u8) {}
    fn write(&self, ep: EndpointAddress, buf: &[u8]) -> Result<usize> {
        let mut s = self.0.lock().unwrap();
        assert_eq!(ep.index(), 0, "only EP0 writes here");
        s.sent.extend_from_slice(buf);
        s.in_done = true;
        Ok(buf.len())
    }
    fn read(&self, ep: EndpointAddress, buf: &mut [u8]) -> Result<usize> {
        let mut s = self.0.lock().unwrap();
        match (ep.index(), s.setup.take()) {
            (0, Some(p)) => {
                buf[..8].copy_from_slice(&p);
                Ok(8)
            }
            _ => Err(UsbError::WouldBlock),
        }
    }
    fn set_stalled(&self, _: EndpointAddress, stalled: bool) {
        if stalled {
            self.0.lock().unwrap().stalled = true;
        }
    }
    fn is_stalled(&self, _: EndpointAddress) -> bool {
        false
    }
    fn suspend(&self) {}
    fn resume(&self) {}
    fn poll(&self) -> PollResult {
        let mut s = self.0.lock().unwrap();
        if s.setup.is_some() {
            PollResult::Data {
                ep_out: 1,
                ep_in_complete: 0,
                ep_setup: 1,
            }
        } else if std::mem::take(&mut s.in_done) {
            PollResult::Data {
                ep_out: 0,
                ep_in_complete: 1,
                ep_setup: 0,
            }
        } else {
            PollResult::None
        }
    }
}

/// What the device is built with.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Console,
    Midi,
    Audio,
}

/// The board's class: MIDI, and audio's stream if on, its alt setting as
/// the board's shims keep it (`Alt`).
struct Midi<'a> {
    ids: Ids,
    out: EndpointOut<'a, Fake>,
    inp: EndpointIn<'a, Fake>,
    audio: Option<(EndpointIn<'a, Fake>, Alt)>,
}

impl UsbClass<Fake> for Midi<'_> {
    fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> Result<()> {
        let stream = self.audio.as_ref().map(|(ep, alt)| Stream {
            iface: alt.iface(),
            ep,
        });
        descriptors(w, self.ids, &self.out, &self.inp, stream)
    }

    fn get_string(&self, index: StringIndex, _: LangID) -> Option<&str> {
        (index == self.ids.name).then_some("Chimera")
    }

    fn get_alt_setting(&mut self, iface: InterfaceNumber) -> Option<u8> {
        self.audio.as_ref()?.1.get(iface)
    }

    fn set_alt_setting(&mut self, iface: InterfaceNumber, alt: u8) -> bool {
        self.audio.as_mut().is_some_and(|(_, a)| a.set(iface, alt))
    }

    fn reset(&mut self) {
        if let Some((_, a)) = &mut self.audio {
            a.reset();
        }
    }
}

/// One answer: the bytes the device sent and whether it stalled.
struct Answer {
    sent: Vec<u8>,
    stalled: bool,
}

/// Each SETUP in turn on one device built as `layout`, as the board's
/// `door::alloc` allocates it: the console, then AudioControl, audio's
/// streaming interface, MIDIStreaming, the jacks' string, MIDI's bulk
/// endpoints and audio's isochronous IN.
fn session(layout: Layout, setups: &[[u8; 8]]) -> Vec<Answer> {
    let alloc = UsbBusAllocator::new(Fake::default());
    let mut serial = SerialPort::new(&alloc);
    let mut class = (layout != Layout::Console).then(|| {
        let ac = alloc.interface();
        let stream = (layout == Layout::Audio).then(|| alloc.interface());
        let ms = alloc.interface();
        let name = alloc.string();
        let out = alloc.bulk(64);
        let inp = alloc.bulk(64);
        let audio = stream.map(|s| {
            let ep = alloc.isochronous(
                IsochronousSynchronizationType::Asynchronous,
                IsochronousUsageType::Data,
                uac::PACKET,
                1,
            );
            (ep, Alt::new(s))
        });
        Midi {
            ids: Ids { ac, ms, name },
            out,
            inp,
            audio,
        }
    });
    // `connect`'s device, the serial a 96-bit UID in hex.
    let release = match layout {
        Layout::Console => None,
        Layout::Midi => Some(0x0020),
        Layout::Audio => Some(0x0030),
    };
    let mut dev = device(
        &alloc,
        (0x0483, 0x5740),
        "0123456789ABCDEF01234567",
        release,
    );
    setups
        .iter()
        .map(|&setup| {
            {
                let mut s = dev.bus().0.lock().unwrap();
                s.sent.clear();
                s.stalled = false;
                s.setup = Some(setup);
            }
            for _ in 0..200 {
                match class.as_mut() {
                    Some(m) => dev.poll(&mut [&mut serial, m]),
                    None => dev.poll(&mut [&mut serial]),
                };
            }
            let s = dev.bus().0.lock().unwrap();
            Answer {
                sent: s.sent.clone(),
                stalled: s.stalled,
            }
        })
        .collect()
}

fn get_descriptor(kind: u8, index: u8) -> [u8; 8] {
    [0x80, 6, index, kind, 0x09, 0x04, 0xFF, 0x03]
}

fn set_interface(iface: u8, alt: u8) -> [u8; 8] {
    [0x01, 11, alt, 0, iface, 0, 0, 0]
}

fn get_interface(iface: u8) -> [u8; 8] {
    [0x81, 10, 0, 0, iface, 0, 1, 0]
}

/// What a GET_DESCRIPTOR of `kind` and `index` brings back, and whether
/// the device stalled it.
fn ask(layout: Layout, kind: u8, index: u8) -> (Vec<u8>, bool) {
    let a = session(layout, &[get_descriptor(kind, index)]).remove(0);
    (a.sent, a.stalled)
}

/// The configuration whole, its wTotalLength the bytes sent, inside the
/// control buffer, and its descriptors chained to the end.
fn configuration(layout: Layout, interfaces: u8) -> Vec<u8> {
    let (c, stalled) = ask(layout, 2, 0);
    assert!(!stalled, "GET_DESCRIPTOR(configuration) stalled");
    assert!(c.len() >= 9, "no configuration: {} B", c.len());
    assert_eq!(usize::from(u16::from_le_bytes([c[2], c[3]])), c.len());
    assert!(
        c.len() <= CONTROL_BUF,
        "{} B past the control buffer",
        c.len()
    );
    assert_eq!(c[4], interfaces);
    let mut at = 0;
    while at < c.len() {
        assert!(c[at] >= 2, "a zero-length descriptor at {at}");
        at += usize::from(c[at]);
    }
    assert_eq!(at, c.len());
    c
}

/// The descriptors of a configuration, each its own bytes.
fn chain(c: &[u8]) -> Vec<&[u8]> {
    let mut v = Vec::new();
    let mut at = 0;
    while at < c.len() {
        let n = usize::from(c[at]);
        v.push(&c[at..at + n]);
        at += n;
    }
    v
}

fn le16(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

#[test]
fn console_alone_reaches_the_host() {
    configuration(Layout::Console, 2);
}

#[test]
fn console_and_midi_reach_the_host() {
    let c = configuration(Layout::Midi, 4);
    assert_eq!(c.len(), 175);
    // MIDIStreaming's class-specific total: from its MS header to the end.
    let ms = c
        .windows(3)
        .position(|w| w == [7, 0x24, 0x01])
        .expect("an MS header");
    assert_eq!(c.len() - ms, usize::from(MS_TOTAL));
}

/// The audio layout's size, held to the byte: it sits 7 B under the
/// control buffer, so any growth is a decision (ADR 0248), not a drift.
#[test]
fn audio_configuration_fits_the_control_buffer() {
    let c = configuration(Layout::Audio, 5);
    assert_eq!(c.len(), 249);
    assert!(CONTROL_BUF - c.len() >= 7);
    // MIDI's class-specific block is unchanged by the stream before it:
    // from its MS header (AS general shares its first bytes) to the end.
    let ms = c
        .windows(3)
        .rposition(|w| w == [7, 0x24, 0x01])
        .expect("an MS header");
    assert_eq!(c.len() - ms, usize::from(MS_TOTAL));
}

/// Every descriptor of the audio layout in order: the console's function,
/// then one Audio function of AudioControl, the streaming interface (alt
/// 0 empty, alt 1 the stereo 48 kHz 24-bit stream on one asynchronous
/// isochronous IN) and MIDIStreaming.
#[test]
fn audio_configuration_parses_whole() {
    let c = configuration(Layout::Audio, 5);
    let d = chain(&c);
    let kinds: Vec<(u8, u8)> = d.iter().map(|x| (x[0], x[1])).collect();
    assert_eq!(
        kinds[..12],
        [
            (9, 2),    // configuration
            (8, 0x0B), // CDC's IAD
            (9, 4),    // CDC communication
            (5, 0x24), // header
            (4, 0x24), // ACM
            (5, 0x24), // union
            (5, 0x24), // call management
            (7, 5),    // notification IN
            (9, 4),    // CDC data
            (7, 5),    // bulk OUT
            (7, 5),    // bulk IN
            (8, 0x0B), // audio's IAD
        ]
    );
    // The configuration: self-powered, 100 mA.
    assert_eq!((d[0][5], d[0][7], d[0][8]), (1, 0xC0, 50));
    // The IADs: CDC over interfaces 0–1, Audio over 2–4.
    assert_eq!(d[1][2..7], [0, 2, 0x02, 0x02, 0x00]);
    assert_eq!(d[11][2..7], [2, 3, 0x01, 0x00, 0x00]);

    // AudioControl: no endpoint; its header lists the stream and MIDI, and
    // its wTotalLength is the header and the terminals.
    let ac = &d[12..16];
    assert_eq!(ac[0][..], [9, 4, 2, 0, 0, 0x01, 0x01, 0x00, 0]);
    let hdr = ac[1];
    assert_eq!(hdr[..5], [10, 0x24, 0x01, 0x00, 0x01]);
    let total: usize = ac[1..].iter().map(|x| x.len()).sum();
    assert_eq!(usize::from(le16(hdr, 5)), total);
    assert_eq!(hdr[7..], [2, 3, 4]);
    // Input terminal 1, a line connector, stereo L R; output terminal 2,
    // USB streaming, fed by 1.
    let it = ac[2];
    assert_eq!((it[0], it[2], it[3], le16(it, 4)), (12, 0x02, 1, 0x0603));
    assert_eq!((it[7], le16(it, 8)), (2, 0x0003));
    let ot = ac[3];
    assert_eq!((ot[0], ot[2], ot[3], le16(ot, 4)), (9, 0x03, 2, 0x0101));
    assert_eq!(ot[7], 1);

    // The stream: alt 0 with no endpoint, alt 1 with one.
    let st = &d[16..22];
    assert_eq!(st[0][..], [9, 4, 3, 0, 0, 0x01, 0x02, 0x00, 0]);
    assert_eq!(st[1][..], [9, 4, 3, 1, 1, 0x01, 0x02, 0x00, 0]);
    // AS general: linked to the output terminal, PCM.
    assert_eq!(st[2][..], [7, 0x24, 0x01, 2, 1, 0x01, 0x00]);
    // Type I: 2 channels, 3-byte subframes, 24 bits, one rate.
    let f = st[3];
    assert_eq!(f[..8], [11, 0x24, 0x02, 0x01, 2, 3, 24, 1]);
    assert_eq!(u32::from_le_bytes([f[8], f[9], f[10], 0]), uac::RATE);
    // The endpoint: UAC1's 9 bytes, IN, isochronous asynchronous data,
    // room for the frames a late frame owes, every frame, no sync.
    let ep = st[4];
    assert_eq!((ep[0], ep[1]), (9, 5));
    assert_eq!(ep[2] & 0x80, 0x80, "an IN endpoint");
    assert_eq!(ep[3], 0x05);
    let max = usize::from(le16(ep, 4));
    assert_eq!(max, usize::from(uac::PACKET));
    assert!(((uac::NOMINAL + 1) * uac::FRAME_BYTES..=1023).contains(&max));
    assert_eq!(max % uac::FRAME_BYTES, 0);
    assert_eq!(ep[6..], [1, 0, 0]);
    // Its class-specific endpoint: no controls, no lock delay.
    assert_eq!(st[5][..], [7, 0x25, 0x01, 0x00, 0, 0, 0]);

    // MIDIStreaming, then its jacks and two bulk endpoints of 64.
    assert_eq!(d[22][..], [9, 4, 4, 0, 2, 0x01, 0x03, 0x00, 0]);
    let eps: Vec<&&[u8]> = d[23..].iter().filter(|x| x[1] == 5).collect();
    assert_eq!(eps.len(), 2);
    for e in eps {
        assert_eq!((e[0], e[3], le16(e, 4)), (9, 0x02, 64));
    }

    // Each interface's bNumEndpoints is the endpoints that follow it.
    let mut ifaces = Vec::new();
    for x in &d {
        match x[1] {
            4 => ifaces.push((x[2], x[3], x[4], 0u8)),
            5 => ifaces.last_mut().unwrap().3 += 1,
            _ => {}
        }
    }
    for (n, alt, want, got) in &ifaces {
        assert_eq!(want, got, "interface {n} alt {alt}");
    }
    // Every endpoint address once.
    let mut addrs: Vec<u8> = d.iter().filter(|x| x[1] == 5).map(|x| x[2]).collect();
    let all = addrs.len();
    addrs.sort();
    addrs.dedup();
    assert_eq!(addrs.len(), all);
}

/// The host starts the stream with SET_INTERFACE: alt 1 is taken and read
/// back, alt 2 stalls, alt 0 stops it; MIDI's and AudioControl's only
/// setting is still taken.
#[test]
fn audio_alt_settings_reach_the_class() {
    let a = session(
        Layout::Audio,
        &[
            get_interface(3),
            set_interface(3, 1),
            get_interface(3),
            set_interface(3, 2),
            get_interface(3),
            set_interface(3, 0),
            get_interface(3),
            set_interface(4, 0),
            set_interface(2, 0),
        ],
    );
    let read = |i: usize| (a[i].sent.clone(), a[i].stalled);
    assert_eq!(read(0), (vec![0], false));
    assert!(!a[1].stalled);
    assert_eq!(read(2), (vec![1], false));
    assert!(a[3].stalled, "alt 2 is no setting");
    assert_eq!(read(4), (vec![1], false));
    assert!(!a[5].stalled);
    assert_eq!(read(6), (vec![0], false));
    assert!(!a[7].stalled && !a[8].stalled);
}

#[test]
fn device_strings_and_bos_reach_the_host() {
    for layout in [Layout::Midi, Layout::Audio] {
        let whole = |kind, index| {
            let (d, stalled) = ask(layout, kind, index);
            assert!(!stalled && !d.is_empty(), "descriptor {kind}:{index}");
            d
        };
        assert_eq!(whole(1, 0).len(), 18);
        // Languages, manufacturer, product, serial, the jacks' name.
        for i in 0..=4 {
            let d = whole(3, i);
            assert_eq!(usize::from(d[0]), d.len(), "string {i}");
        }
        assert_eq!(whole(3, 4), b"\x10\x03C\0h\0i\0m\0e\0r\0a\0");
        let b = whole(0x0F, 0);
        assert_eq!(usize::from(u16::from_le_bytes([b[2], b[3]])), b.len());
    }
}

/// The console alone is a CDC device; with MIDI, a composite of IADs, its
/// bcdDevice the layout's.
#[test]
fn device_class_follows_the_layout() {
    for (layout, class, release) in [
        (Layout::Console, 0x02, None),
        (Layout::Midi, 0xEF, Some(0x0020)),
        (Layout::Audio, 0xEF, Some(0x0030)),
    ] {
        let (d, stalled) = ask(layout, 1, 0);
        assert!(!stalled);
        assert_eq!(d[4], class);
        if let Some(r) = release {
            assert_eq!(le16(&d, 12), r);
        }
    }
}
