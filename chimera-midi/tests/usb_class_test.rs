//! The configuration descriptor the unit serves, enumerated through
//! usb-device on a fake bus: the console's CDC (usbd-serial) alone (bank 2
//! closed at boot), with USB MIDI's function (the bench builds'), and with
//! USB audio's stream in that function (every shipping build's, ADR 0256),
//! as `chimera-board`'s `connect` builds the device. Each
//! must reach the host whole: past usb-device's control buffer,
//! GET_DESCRIPTOR fails and the host gives the port up (11b82462 on the
//! unit). The audio layout is parsed to the last byte: IADs, interfaces
//! and alt settings, the class-specific chain, endpoints.

use std::sync::Mutex;

use chimera_midi::uac;
use chimera_midi::uac_link::Alt;
use chimera_midi::uac_out;
use chimera_midi::usb_class::{
    CABLES, CONTROL_BUF, Ids, MARGIN, Stream, cable_count, config_len, descriptors, device,
    jack_name, ms_total, names,
};
use chimera_midi::usb_config::{
    self, Audio as StaticAudio, Eps, Layout as StaticLayout, Owner, Replies,
};
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
    /// IN packets the host still reads this step; `None` all of them.
    cut: Option<usize>,
    /// A bus reset at the next poll.
    reset: bool,
    /// Endpoints halted, by address.
    halted: Vec<u8>,
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
    fn reset(&self) {
        self.0.lock().unwrap().halted.clear();
    }
    fn set_device_address(&self, _: u8) {}
    fn write(&self, ep: EndpointAddress, buf: &[u8]) -> Result<usize> {
        let mut s = self.0.lock().unwrap();
        assert_eq!(ep.index(), 0, "only EP0 writes here");
        s.sent.extend_from_slice(buf);
        // A host that stops reading leaves the packet unacknowledged.
        match &mut s.cut {
            Some(0) => {}
            Some(n) => {
                *n -= 1;
                s.in_done = true;
            }
            None => s.in_done = true,
        }
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
    fn set_stalled(&self, ep: EndpointAddress, stalled: bool) {
        let mut s = self.0.lock().unwrap();
        let a = u8::from(ep);
        s.halted.retain(|&h| h != a);
        if stalled {
            s.stalled = true;
            s.halted.push(a);
        }
    }
    fn is_stalled(&self, ep: EndpointAddress) -> bool {
        self.0.lock().unwrap().halted.contains(&u8::from(ep))
    }
    fn suspend(&self) {}
    fn resume(&self) {}
    fn poll(&self) -> PollResult {
        let mut s = self.0.lock().unwrap();
        if std::mem::take(&mut s.reset) {
            PollResult::Reset
        } else if s.setup.is_some() {
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

/// What the device is built with: the console alone (bank 2 closed), MIDI
/// without the stream (the bench builds, a cable a Part), MIDI and the
/// stream (`default`, `orbit`, `bank2-gate`: one cable).
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
        jack_name(self.ids, index)
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
        let n = cable_count(layout == Layout::Audio);
        let name = names(&alloc, n);
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
            ids: Ids {
                ac,
                ms,
                name,
                cables: n,
            },
            out,
            inp,
            audio,
        }
    });
    // `connect`'s device, the serial a 96-bit UID in hex.
    let release = match layout {
        Layout::Console => None,
        Layout::Midi => Some(0x0021),
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
    assert_eq!(c.len(), 217);
    assert_eq!(c.len(), config_len(false));
    assert!(c.len() + MARGIN <= CONTROL_BUF, "{} B", c.len());
    // MIDIStreaming's class-specific total: from its MS header to the end.
    let ms = c
        .windows(3)
        .position(|w| w == [7, 0x24, 0x01])
        .expect("an MS header");
    assert_eq!(c.len() - ms, usize::from(ms_total(CABLES)));
}

/// The audio layout's size, held to the byte: it sits 7 B under the
/// control buffer, so any growth is a decision (ADR 0248), not a drift.
#[test]
fn audio_configuration_fits_the_control_buffer() {
    let c = configuration(Layout::Audio, 5);
    assert_eq!(c.len(), 249);
    assert_eq!(c.len(), config_len(true));
    assert!(CONTROL_BUF - c.len() >= 7);
    // MIDI's class-specific block is unchanged by the stream before it:
    // from its MS header (AS general shares its first bytes) to the end.
    let ms = c
        .windows(3)
        .rposition(|w| w == [7, 0x24, 0x01])
        .expect("an MS header");
    assert_eq!(c.len() - ms, usize::from(ms_total(1)));
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
    // room for half again the nominal (Linux's), every frame, no sync.
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
        // Languages, manufacturer, product, serial, the jacks' names.
        for i in 0..4 + cable_count(layout == Layout::Audio) {
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
        (Layout::Midi, 0xEF, Some(0x0021)),
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

/// The class-specific MIDIStreaming descriptors, each its bytes.
fn ms_descriptors(c: &[u8]) -> Vec<&[u8]> {
    let ms = c
        .windows(3)
        .rposition(|w| w == [7, 0x24, 0x01])
        .expect("an MS header");
    chain(&c[ms..])
}

/// Each layout's cables (ADR 0243): an embedded IN jack a cable, the bulk
/// OUT endpoint naming them in cable order, each its own name; the IN
/// endpoint cable 0's alone. The bench builds' seven, the shipping builds' one.
#[test]
fn every_cable_has_its_jack_and_name() {
    for (layout, interfaces) in [(Layout::Midi, 4), (Layout::Audio, 5)] {
        let c = configuration(layout, interfaces);
        let d = ms_descriptors(&c);
        let jacks = |kind: u8| -> Vec<(u8, u8)> {
            d.iter()
                .filter(|x| x[1] == 0x24 && x[2] == kind && x[3] == 0x01)
                .map(|x| (x[4], *x.last().unwrap()))
                .collect()
        };
        let (ins, outs) = (jacks(0x02), jacks(0x03));
        let n = cable_count(layout == Layout::Audio);
        assert_eq!(ins.len(), usize::from(n));
        assert_eq!(outs.len(), 1);
        let cs: Vec<&[u8]> = d.iter().filter(|x| x[1] == 0x25).copied().collect();
        assert_eq!(cs.len(), 2);
        let (from_host, to_host) = (cs[0], cs[1]);
        assert_eq!(usize::from(from_host[3]), ins.len());
        assert_eq!(
            from_host[4..],
            ins.iter().map(|j| j.0).collect::<Vec<_>>()[..]
        );
        assert_eq!(&to_host[3..], &[1, outs[0].0]);
        let answers = session(
            layout,
            &ins.iter()
                .map(|&(_, i)| get_descriptor(3, i))
                .collect::<Vec<_>>(),
        );
        let names: Vec<String> = answers
            .iter()
            .map(|a| {
                assert!(!a.stalled);
                let u: Vec<u16> = a.sent[2..]
                    .chunks(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                String::from_utf16(&u).unwrap()
            })
            .collect();
        let want: Vec<String> = std::iter::once("Chimera".to_string())
            .chain((1..n).map(|p| format!("Chimera Part {p}")))
            .collect();
        assert_eq!(names, want, "{}", n);
    }
}

// The configuration as one static (#885): layouts past the control
// buffer, answered by the class from `usb_config`'s bytes.

/// The fake allocator's addresses, in `session`'s order: the console's
/// notification IN 1, data OUT 1, data IN 2; MIDI's OUT 2 and IN 3; the
/// capture's IN 4, the playback's OUT 3 and its feedback IN 5.
const EPS: Eps = Eps {
    cdc: [0x81, 0x82, 0x01],
    midi_out: 0x02,
    midi_in: 0x83,
};

/// The jacks' first string: the device's three, then usb-device's next.
const NAME: u8 = 4;

fn bytes(l: &StaticLayout) -> Vec<u8> {
    let mut b = vec![0u8; usb_config::BUF];
    let n = usb_config::write(&mut b, l).expect("fits the static");
    assert_eq!(n, usb_config::len(l));
    b.truncate(n);
    b
}

/// The shipping and bench layouts as bytes are what usb-device serves
/// today, byte for byte: the static path changes nothing it replaces.
#[test]
fn static_bytes_match_what_usb_device_serves() {
    let bench = StaticLayout {
        cables: CABLES,
        name: NAME,
        eps: EPS,
        audio: None,
    };
    assert_eq!(bytes(&bench), configuration(Layout::Midi, 4));
    let shipping = usb_config::shipping(NAME, EPS.midi_out, EPS.midi_in, 0x84);
    assert_eq!(bytes(&shipping), configuration(Layout::Audio, 5));
    assert_eq!(EPS.cdc, usb_config::CDC);
}

/// The shipping layout served from the static (`serve`), as the firmware
/// does: Linux reads 9 bytes first, then wTotalLength; Windows asks 255
/// first; any wLength past it gets the 249 B whole. Each is the same
/// bytes usb-device serves itself.
#[test]
fn shipping_layout_served_from_the_static_cuts_to_wlength() {
    let want = configuration(Layout::Audio, 5);
    let l = usb_config::shipping(NAME, EPS.midi_out, EPS.midi_in, 0x84);
    let lens = [9u16, 64, 248, 249, 255, 512, 0x03FF];
    let setups: Vec<[u8; 8]> = lens.iter().map(|&n| get_config(n)).collect();
    let a = static_session(bytes(&l), &setups);
    for (n, a) in lens.iter().zip(&a) {
        assert!(!a.stalled, "wLength {n}");
        assert_eq!(
            a.sent,
            want[..usize::from(*n).min(want.len())],
            "wLength {n}"
        );
    }
    // Any other descriptor still falls to usb-device.
    let r = static_session(bytes(&l), &[get_descriptor(1, 0)]);
    assert_eq!(r[0].sent.len(), 18);
}

/// The shipping layout with the host's playback (ADR 0260): one cable,
/// the stereo capture, the playback's OUT 3 and feedback IN 5.
const DUPLEX: StaticLayout = usb_config::duplex(NAME, 0x02, 0x83, 0x84, (0x03, 0x85));

/// Both streams, the six-channel capture and one cable (#885).
const FULL: StaticLayout = StaticLayout {
    cables: 1,
    name: NAME,
    eps: EPS,
    audio: Some(StaticAudio {
        capture: 0x84,
        six: true,
        playback: Some((0x03, 0x85)),
    }),
};

/// A class answering GET_DESCRIPTOR(configuration) from its static, and
/// the two streams' alt settings, as the board's: the capture's and the
/// playback's `Alt`, 0 or 1 each.
struct Whole {
    replies: Replies,
    owner: Owner,
    alts: [Alt; 2],
}

impl Whole {
    /// The board's replies: the configuration `bytes`, the device's
    /// descriptor, and strings 0 to 4 with `SERIAL` the serial number.
    fn new(config: &'static [u8], alts: [Alt; 2]) -> Whole {
        let serial = Vec::leak(vec![0u8; 2 + 2 * SERIAL.len()]);
        usb_config::string(SERIAL, serial).unwrap();
        let device = Vec::leak(usb_config::device_descriptor(0x0483, 0x5740, 0x0040).to_vec());
        Whole {
            replies: Replies {
                config,
                device,
                strings: [
                    &usb_config::LANGS,
                    &usb_config::CHIMERA,
                    &usb_config::CHIMERA,
                    serial,
                    &usb_config::CHIMERA,
                ],
            },
            owner: Owner::new(),
            alts,
        }
    }
}

impl UsbClass<Fake> for Whole {
    fn control_in(&mut self, xfer: ControlIn<Fake>) {
        let alts = self.alts;
        self.owner.control_in(xfer, &self.replies, |i| {
            alts.iter()
                .find(|a| u8::from(a.iface()) == i)
                .and_then(|a| a.get(a.iface()))
                .unwrap_or(0)
        });
    }

    fn control_out(&mut self, xfer: ControlOut<Fake>) {
        self.owner.control_out(xfer.request());
    }

    /// The one cable's jack, as the board's `jack_name`.
    fn get_string(&self, index: StringIndex, _: LangID) -> Option<&str> {
        (u8::from(index) == NAME).then_some("Chimera")
    }

    fn get_alt_setting(&mut self, iface: InterfaceNumber) -> Option<u8> {
        self.alts.iter().find_map(|a| a.get(iface))
    }

    fn set_alt_setting(&mut self, iface: InterfaceNumber, alt: u8) -> bool {
        self.alts.iter_mut().any(|a| a.set(iface, alt))
    }

    fn reset(&mut self) {
        self.alts.iter_mut().for_each(Alt::reset);
        self.owner.reset();
    }
}

/// Each SETUP in turn on a device whose class serves `bytes`, the
/// interfaces allocated as the board allocates them: the console, then
/// AudioControl 2, the capture 3, MIDIStreaming 4, the playback 5.
fn static_session(bytes: Vec<u8>, setups: &[[u8; 8]]) -> Vec<Answer> {
    let steps: Vec<Step> = setups.iter().map(|&s| Step::Setup(s)).collect();
    static_steps(bytes, &steps)
}

/// What the host does next: a SETUP read whole, a SETUP whose data stage
/// it stops reading after `n` packets (a timeout, a cancel), or a bus
/// reset.
#[derive(Clone, Copy)]
enum Step {
    Setup([u8; 8]),
    Cut([u8; 8], usize),
    Reset,
}

fn static_steps(bytes: Vec<u8>, steps: &[Step]) -> Vec<Answer> {
    let alloc = UsbBusAllocator::new(Fake::default());
    let mut serial = SerialPort::new(&alloc);
    let _ac = alloc.interface();
    let capture = alloc.interface();
    let _ms = alloc.interface();
    let playback = alloc.interface();
    let mut class = Whole::new(Vec::leak(bytes), [Alt::new(capture), Alt::new(playback)]);
    let mut dev = device(&alloc, (0x0483, 0x5740), SERIAL, Some(0x0040));
    steps
        .iter()
        .map(|&step| {
            {
                let mut s = dev.bus().0.lock().unwrap();
                s.sent.clear();
                s.stalled = false;
                s.in_done = false;
                match step {
                    Step::Setup(p) => (s.setup, s.cut) = (Some(p), None),
                    Step::Cut(p, n) => (s.setup, s.cut) = (Some(p), Some(n)),
                    Step::Reset => s.reset = true,
                }
            }
            for _ in 0..200 {
                dev.poll(&mut [&mut serial, &mut class]);
            }
            let s = dev.bus().0.lock().unwrap();
            Answer {
                sent: s.sent.clone(),
                stalled: s.stalled,
            }
        })
        .collect()
}

/// The serial number the static sessions' device carries.
const SERIAL: &str = "0123";

fn get_status(recipient: u8, index: u8) -> [u8; 8] {
    [0x80 | recipient, 0, 0, 0, index, 0, 2, 0]
}

const GET_CONFIGURATION: [u8; 8] = [0x80, 8, 0, 0, 0, 0, 1, 0];

/// usb-device 0.3.2 keeps a static reply (`accept_in_static`) until its
/// data stage ends; a reset, a new SETUP or a host that stops reading
/// leaves it set, and usb-device's next buffered reply would send its
/// bytes. After a configuration read cut short, then after a reset too,
/// every other request is answered as on a device that was never cut.
#[test]
fn a_configuration_read_cut_short_answers_nothing_else_with_its_bytes() {
    let cfg = bytes(&DUPLEX);
    let asks = [
        get_descriptor(1, 0),
        get_descriptor(3, 0),
        get_descriptor(3, 1),
        get_descriptor(3, 2),
        get_descriptor(3, 3),
        get_descriptor(3, 4),
        get_descriptor(0x0F, 0),
        get_status(0, 0),
        get_status(1, 3),
        get_status(2, 0x83),
        GET_CONFIGURATION,
        get_interface(5),
        get_interface(0),
        get_config(9),
    ];
    let clean = static_session(cfg.clone(), &asks);
    assert_eq!(clean[0].sent.len(), 18);
    assert_eq!(clean[13].sent, cfg[..9]);
    for before in [
        vec![Step::Cut(get_config(0x03FF), 2)],
        vec![Step::Cut(get_config(0x03FF), 2), Step::Reset],
        vec![Step::Cut(get_config(0x03FF), 40), Step::Reset],
    ] {
        for (i, &ask) in asks.iter().enumerate() {
            let mut steps = before.clone();
            steps.push(Step::Setup(ask));
            let got = static_steps(cfg.clone(), &steps).pop().unwrap();
            assert_eq!(got.stalled, clean[i].stalled, "request {i}");
            assert_eq!(got.sent, clean[i].sent, "request {i} after a cut read");
        }
    }
}

fn get_config(len: u16) -> [u8; 8] {
    let [lo, hi] = len.to_le_bytes();
    [0x80, 6, 0, 2, 0, 0, lo, hi]
}

/// The shipping layout with playback: 332 B, past the 256-B buffer,
/// served whole through usb-device's own poll and cut to each wLength
/// (Linux asks 9 first, Windows 255).
#[test]
fn duplex_layout_reaches_the_host_past_the_control_buffer() {
    let want = bytes(&DUPLEX);
    assert_eq!(want.len(), 332);
    assert!(want.len() > CONTROL_BUF);
    assert_eq!(usize::from(le16(&want, 2)), want.len());
    let lens = [9u16, 255, 331, 332, 512, 0x03FF];
    let a = static_session(
        want.clone(),
        &lens.iter().map(|&n| get_config(n)).collect::<Vec<_>>(),
    );
    for (n, a) in lens.iter().zip(&a) {
        assert!(!a.stalled, "wLength {n}");
        assert_eq!(
            a.sent,
            want[..usize::from(*n).min(want.len())],
            "wLength {n}"
        );
    }
}

/// The full layout: 375 B, served whole.
#[test]
fn full_layout_reaches_the_host_past_the_control_buffer() {
    let want = bytes(&FULL);
    assert_eq!(want.len(), 375);
    let a = static_session(want.clone(), &[get_config(9), get_config(0x03FF)]);
    assert!(!a[0].stalled && !a[1].stalled);
    assert_eq!(a[0].sent, want[..9]);
    assert_eq!(a[1].sent, want);
}

/// Every Part cable (ADR 0243) beside both streams fits the static: 417
/// B, served whole.
#[test]
fn part_cables_fit_beside_both_streams() {
    let l = StaticLayout {
        cables: CABLES,
        ..FULL
    };
    let want = bytes(&l);
    assert_eq!(want.len(), 417);
    assert!(want.len() <= usb_config::BUF);
    let a = static_session(want.clone(), &[get_config(0x03FF)]);
    assert_eq!(a[0].sent, want);
}

/// A layout with both streams parsed: six interfaces, one Audio function
/// over four, AudioControl's two chains, the capture's alts, MIDI on 4,
/// the playback's two alts on 5, the feedback's pairing, and full speed's
/// periodic budget with the largest alts on together.
fn parses_whole(c: &[u8], six: bool) {
    assert_eq!(c[4], 6);
    let mut at = 0;
    while at < c.len() {
        assert!(c[at] >= 2);
        at += usize::from(c[at]);
    }
    assert_eq!(at, c.len());
    let d = chain(c);
    let iads: Vec<&&[u8]> = d.iter().filter(|x| x[1] == 0x0B).collect();
    assert_eq!(iads[1][2..5], [2, 4, 0x01]);

    // AudioControl: the header lists 3, MIDI's 4 and 5; its total is the
    // header and four terminals.
    let ac = d.iter().position(|x| x[1] == 4 && x[2] == 2).unwrap();
    let hdr = d[ac + 1];
    assert_eq!(hdr[..5], [11, 0x24, 0x01, 0x00, 0x01]);
    assert_eq!(hdr[7..], [3, 3, 4, 5]);
    let total: usize = d[ac + 1..ac + 6].iter().map(|x| x.len()).sum();
    assert_eq!(usize::from(le16(hdr, 5)), total);
    // Capture: IT 1 (line, L R first) into OT 2 (USB).
    let (it1, ot2, it3, ot4) = (d[ac + 2], d[ac + 3], d[ac + 4], d[ac + 5]);
    let ch = if six { 6 } else { 2 };
    assert_eq!(
        (it1[2], it1[3], le16(it1, 4), it1[7]),
        (0x02, 1, 0x0603, ch)
    );
    assert_eq!(le16(it1, 8), 0x0003);
    assert_eq!((ot2[2], ot2[3], le16(ot2, 4), ot2[7]), (0x03, 2, 0x0101, 1));
    // Playback: IT 3 (USB, stereo) into OT 4 (line: the main out).
    assert_eq!((it3[2], it3[3], le16(it3, 4), it3[7]), (0x02, 3, 0x0101, 2));
    assert_eq!((ot4[2], ot4[3], le16(ot4, 4), ot4[7]), (0x03, 4, 0x0603, 3));

    // Each alt setting: its format's channels and its endpoints.
    let mut alts: Vec<(u8, u8, Vec<&[u8]>)> = Vec::new();
    for x in &d {
        match x[1] {
            4 => alts.push((x[2], x[3], vec![x])),
            _ => {
                if let Some(a) = alts.last_mut() {
                    a.2.push(x)
                }
            }
        }
    }
    let alt = |i: u8, n: u8| &alts.iter().find(|a| (a.0, a.1) == (i, n)).unwrap().2;
    let eps = |v: &Vec<&[u8]>| -> Vec<Vec<u8>> {
        v.iter().filter(|x| x[1] == 5).map(|x| x.to_vec()).collect()
    };
    let channels = |v: &Vec<&[u8]>| v.iter().find(|x| x[1] == 0x24 && x[2] == 2).unwrap()[4];
    for (i, n) in [(3, 0), (5, 0)] {
        assert!(eps(alt(i, n)).is_empty(), "alt 0 takes no bandwidth");
    }
    // MIDIStreaming on 4, its class unchanged.
    assert_eq!(alt(4, 0)[0][5..7], [0x01, 0x03]);
    // Capture alt 1 stereo (and alt 2 six channels), asynchronous IN.
    let s1 = eps(alt(3, 1));
    assert_eq!(channels(alt(3, 1)), 2);
    assert_eq!(
        (s1[0][2], s1[0][3], le16(&s1[0], 4)),
        (0x84, 0x05, uac::PACKET)
    );
    let biggest = if six {
        let s2 = eps(alt(3, 2));
        assert_eq!(channels(alt(3, 2)), 6);
        assert_eq!((s2[0][2], s2[0][3]), (0x84, 0x05));
        assert_eq!(le16(&s2[0], 4), usb_config::SIX_PACKET);
        le16(&s2[0], 4)
    } else {
        assert!(!alts.iter().any(|a| (a.0, a.1) == (3, 2)));
        le16(&s1[0], 4)
    };
    // Playback alt 1: an asynchronous OUT synced by the feedback IN that
    // follows it (Linux: the data's bSynchAddress is the feedback's, the
    // feedback's own is 0, its type isochronous); 3 bytes, 10.14.
    let p = eps(alt(5, 1));
    assert_eq!(channels(alt(5, 1)), 2);
    assert_eq!(p.len(), 2);
    let (data, fb) = (&p[0], &p[1]);
    assert_eq!((data[0], data[2], data[3]), (9, 0x03, 0x05));
    assert_eq!(le16(data, 4), uac_out::PACKET);
    assert_eq!(data[8], fb[2]);
    assert_eq!(
        (fb[0], fb[2] & 0x80, fb[3], le16(fb, 4)),
        (9, 0x80, 0x11, 3)
    );
    assert_eq!((fb[6], fb[7], fb[8]), (1, uac_out::REFRESH, 0));
    // Every interface's bNumEndpoints is what follows it.
    for (i, n, v) in &alts {
        assert_eq!(usize::from(v[0][4]), eps(v).len(), "interface {i} alt {n}");
    }
    // Full speed's periodic budget (USB 2.0 § 5.6.4: 90 % of 1,500 B, 9 B
    // a transaction) with the largest capture, the playback and its
    // feedback on together.
    let periodic = biggest + le16(data, 4) + le16(fb, 4) + 3 * 9;
    assert!(periodic <= 1_350, "{periodic} B a frame");
    // Every endpoint address once in any alt, the capture's in both.
    let mut addrs: Vec<u8> = d
        .iter()
        .filter(|x| x[1] == 5)
        .map(|x| x[2])
        .filter(|&a| a != 0x84)
        .collect();
    let all = addrs.len();
    addrs.sort();
    addrs.dedup();
    assert_eq!(addrs.len(), all);
}

#[test]
fn duplex_layout_parses_whole() {
    parses_whole(&bytes(&DUPLEX), false);
}

#[test]
fn full_layout_parses_whole() {
    parses_whole(&bytes(&FULL), true);
}

/// The host starts and stops the capture and the playback with
/// SET_INTERFACE, each read back; settings past alt 1 stall; MIDI's and
/// AudioControl's only setting is still taken; a bus reset stops both.
#[test]
fn duplex_alt_settings_reach_the_class() {
    let a = static_session(
        bytes(&DUPLEX),
        &[
            set_interface(3, 1),
            set_interface(5, 1),
            get_interface(3),
            get_interface(5),
            set_interface(5, 2),
            set_interface(3, 2),
            get_interface(5),
            set_interface(5, 0),
            get_interface(5),
            set_interface(4, 0),
            set_interface(2, 0),
        ],
    );
    assert!(!a[0].stalled && !a[1].stalled);
    assert_eq!((a[2].sent.clone(), a[3].sent.clone()), (vec![1], vec![1]));
    assert!(a[4].stalled && a[5].stalled, "alt 2 is no setting");
    assert_eq!(a[6].sent, [1]);
    assert!(!a[7].stalled);
    assert_eq!(a[8].sent, [0]);
    assert!(!a[9].stalled && !a[10].stalled);
}

fn set_feature(recipient: u8, feature: u8, index: u8) -> [u8; 8] {
    [recipient, 3, feature, 0, index, 0, 0, 0]
}

/// After the host configured the device, set remote wakeup and halted an
/// endpoint, a read cut short still answers GET_CONFIGURATION, every
/// GET_STATUS and GET_INTERFACE as usb-device would; a reset after it
/// answers as a reset device.
#[test]
fn a_cut_read_keeps_the_device_state_usb_device_keeps() {
    let cfg = bytes(&DUPLEX);
    let set = [
        Step::Setup([0x00, 9, 1, 0, 0, 0, 0, 0]),
        Step::Setup(set_feature(0, 1, 0)),
        Step::Setup(set_feature(2, 0, 0x83)),
        Step::Setup(set_interface(5, 1)),
    ];
    let asks = [
        GET_CONFIGURATION,
        get_status(0, 0),
        get_status(2, 0x83),
        get_status(2, 0x02),
        get_interface(5),
        get_interface(3),
    ];
    for reset in [false, true] {
        let mut clean: Vec<Step> = set.to_vec();
        let mut cut: Vec<Step> = set.to_vec();
        cut.push(Step::Cut(get_config(0x03FF), 3));
        if reset {
            clean.push(Step::Reset);
            cut.push(Step::Reset);
        }
        for (i, &ask) in asks.iter().enumerate() {
            let want = {
                let mut s = clean.clone();
                s.push(Step::Setup(ask));
                static_steps(cfg.clone(), &s).pop().unwrap()
            };
            let mut s = cut.clone();
            s.push(Step::Setup(ask));
            let got = static_steps(cfg.clone(), &s).pop().unwrap();
            assert_eq!(
                (got.sent, got.stalled),
                (want.sent, want.stalled),
                "request {i}, reset {reset}"
            );
        }
    }
}
