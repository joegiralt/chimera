//! The configuration descriptor the unit serves, enumerated through
//! usb-device on a fake bus: the console's CDC (usbd-serial) with USB
//! MIDI's function, as `chimera-board`'s `connect` builds the device, and
//! the console alone (bank 2 closed at boot). Each must reach the host
//! whole: past usb-device's control buffer, GET_DESCRIPTOR fails and the
//! host gives the port up (11b82462 on the unit).

use std::sync::Mutex;

use chimera_midi::usb_class::{Ids, MS_TOTAL, descriptors};
use usb_device::bus::PollResult;
use usb_device::class_prelude::*;
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid};
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

/// EP0 only: one SETUP in, every IN packet kept.
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

struct Midi<'a> {
    ids: Ids,
    out: EndpointOut<'a, Fake>,
    inp: EndpointIn<'a, Fake>,
}

impl UsbClass<Fake> for Midi<'_> {
    fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> Result<()> {
        descriptors(w, self.ids, &self.out, &self.inp)
    }

    fn get_string(&self, index: StringIndex, _: LangID) -> Option<&str> {
        (index == self.ids.name).then_some("Chimera")
    }
}

/// What a GET_DESCRIPTOR of `kind` and `index` brings back, and whether
/// the device stalled it.
fn ask(midi: bool, kind: u8, index: u8) -> (Vec<u8>, bool) {
    let alloc = UsbBusAllocator::new(Fake::default());
    let mut serial = SerialPort::new(&alloc);
    let mut class = midi.then(|| Midi {
        ids: Ids {
            ac: alloc.interface(),
            ms: alloc.interface(),
            name: alloc.string(),
        },
        out: alloc.bulk(64),
        inp: alloc.bulk(64),
    });
    // As `chimera-board`'s `connect`, the serial a 96-bit UID in hex.
    let strings = StringDescriptors::default()
        .manufacturer("Chimera")
        .product("Chimera")
        .serial_number("0123456789ABCDEF01234567");
    let mut dev = UsbDeviceBuilder::new(&alloc, UsbVidPid(0x0483, 0x5740))
        .strings(&[strings])
        .unwrap()
        .self_powered(true)
        .max_power(100)
        .unwrap()
        .composite_with_iads()
        .device_release(0x0020)
        .build();
    dev.bus().0.lock().unwrap().setup = Some([0x80, 6, index, kind, 0x09, 0x04, 0xFF, 0x03]);
    for _ in 0..200 {
        match class.as_mut() {
            Some(m) => dev.poll(&mut [&mut serial, m]),
            None => dev.poll(&mut [&mut serial]),
        };
    }
    let s = dev.bus().0.lock().unwrap();
    (s.sent.clone(), s.stalled)
}

/// The configuration whole, its wTotalLength the bytes sent, and its
/// descriptors chained to the end.
fn configuration(midi: bool, interfaces: u8) -> Vec<u8> {
    let (c, stalled) = ask(midi, 2, 0);
    assert!(!stalled, "GET_DESCRIPTOR(configuration) stalled");
    assert!(c.len() >= 9, "no configuration: {} B", c.len());
    assert_eq!(usize::from(u16::from_le_bytes([c[2], c[3]])), c.len());
    assert_eq!(c[4], interfaces);
    let mut at = 0;
    while at < c.len() {
        assert!(c[at] >= 2, "a zero-length descriptor at {at}");
        at += usize::from(c[at]);
    }
    assert_eq!(at, c.len());
    c
}

#[test]
fn console_alone_reaches_the_host() {
    configuration(false, 2);
}

#[test]
fn console_and_midi_reach_the_host() {
    let c = configuration(true, 4);
    // MIDIStreaming's class-specific total: from its MS header to the end.
    let ms = c
        .windows(3)
        .position(|w| w == [7, 0x24, 0x01])
        .expect("an MS header");
    assert_eq!(c.len() - ms, usize::from(MS_TOTAL));
}

#[test]
fn device_strings_and_bos_reach_the_host() {
    let whole = |kind, index| {
        let (d, stalled) = ask(true, kind, index);
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
