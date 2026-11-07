use chimera_midi::usb::{Cable, Packer, unpack};

fn pack(bytes: &[u8]) -> Vec<[u8; 4]> {
    let mut p = Packer::new();
    bytes.iter().filter_map(|&b| p.push(b)).collect()
}

fn unpacked(pkts: &[[u8; 4]]) -> Vec<u8> {
    pkts.iter()
        .filter_map(|&p| unpack(p))
        .flat_map(|(_, b)| b.as_slice().to_vec())
        .collect()
}

#[test]
fn note_on_unpacks() {
    let (c, b) = unpack([0x09, 0x90, 60, 100]).unwrap();
    assert_eq!((c, b.as_slice()), (Cable::ZERO, &[0x90, 60, 100][..]));
}

#[test]
fn cable_is_returned() {
    let (c, b) = unpack([0x19, 0x90, 60, 100]).unwrap();
    assert_eq!((c.get(), b.as_slice()), (1, &[0x90, 60, 100][..]));
}

#[test]
fn every_cable_unpacks_in_range() {
    for n in 0..16u8 {
        let (c, _) = unpack([n << 4 | 0x09, 0x90, 60, 100]).unwrap();
        assert_eq!(c.get(), n);
    }
}

#[test]
fn reserved_cins_drop() {
    assert!(unpack([0x00, 1, 2, 3]).is_none());
    assert!(unpack([0x01, 1, 2, 3]).is_none());
}

#[test]
fn sysex_spans_packets() {
    let id = [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7];
    let pkts = pack(&id);
    assert_eq!(pkts, [[0x04, 0xF0, 0x7E, 0x7F], [0x07, 0x06, 0x01, 0xF7]]);
    assert_eq!(unpacked(&pkts), id);
}

#[test]
fn realtime_inside_sysex() {
    let mut p = Packer::new();
    assert_eq!(p.push(0xF0), None);
    assert_eq!(p.push(0x01), None);
    assert_eq!(p.push(0xF8), Some([0x0F, 0xF8, 0, 0]));
    assert_eq!(p.push(0x02), Some([0x04, 0xF0, 0x01, 0x02]));
    assert_eq!(p.push(0xF7), Some([0x05, 0xF7, 0, 0]));
}

#[test]
fn round_trip_every_type() {
    let mut msgs: Vec<Vec<u8>> = vec![
        vec![0x80, 60, 0],
        vec![0x90, 60, 100],
        vec![0xA0, 60, 50],
        vec![0xB0, 7, 99],
        vec![0xC0, 5],
        vec![0xD0, 40],
        vec![0xE0, 1, 64],
        vec![0xF1, 0x23],
        vec![0xF2, 1, 2],
        vec![0xF3, 9],
        vec![0xF6],
    ];
    msgs.extend((0xF8..=0xFF).map(|b| vec![b]));
    for n in 0..=5u8 {
        let mut s = vec![0xF0];
        s.extend((0..n).map(|i| i + 1));
        s.push(0xF7);
        msgs.push(s);
    }
    for m in msgs {
        assert_eq!(unpacked(&pack(&m)), m, "{m:02X?}");
    }
}

/// Each channel message's packet: CIN is the status's high nibble.
#[test]
fn channel_packets_carry_their_cin() {
    let cases: [(&[u8], [u8; 4]); 7] = [
        (&[0x81, 60, 0], [0x08, 0x81, 60, 0]),
        (&[0x92, 60, 100], [0x09, 0x92, 60, 100]),
        (&[0xA3, 60, 50], [0x0A, 0xA3, 60, 50]),
        (&[0xB4, 7, 99], [0x0B, 0xB4, 7, 99]),
        (&[0xC5, 5], [0x0C, 0xC5, 5, 0]),
        (&[0xD6, 40], [0x0D, 0xD6, 40, 0]),
        (&[0xEF, 1, 64], [0x0E, 0xEF, 1, 64]),
    ];
    for (m, p) in cases {
        assert_eq!(pack(m), [p], "{m:02X?}");
    }
}

/// Running status: every packet still carries the full status and its CIN.
#[test]
fn running_status_packets_carry_the_status() {
    assert_eq!(
        pack(&[0x90, 0x3C, 0x64, 0x3E, 0x64]),
        [[0x09, 0x90, 0x3C, 0x64], [0x09, 0x90, 0x3E, 0x64]]
    );
    assert_eq!(
        pack(&[0xC1, 5, 6, 7]),
        [[0x0C, 0xC1, 5, 0], [0x0C, 0xC1, 6, 0], [0x0C, 0xC1, 7, 0]]
    );
}

/// Two NRPNs as the echo writes them: the second wholly under running
/// status.
#[test]
fn nrpn_under_running_status() {
    let bytes = [
        0xB0, 0x63, 0x01, 0x62, 0x02, 0x06, 0x40, 0x26, 0x00, // first
        0x63, 0x01, 0x62, 0x03, 0x06, 0x10, 0x26, 0x7F, // second
    ];
    assert_eq!(
        pack(&bytes),
        [
            [0x0B, 0xB0, 0x63, 0x01],
            [0x0B, 0xB0, 0x62, 0x02],
            [0x0B, 0xB0, 0x06, 0x40],
            [0x0B, 0xB0, 0x26, 0x00],
            [0x0B, 0xB0, 0x63, 0x01],
            [0x0B, 0xB0, 0x62, 0x03],
            [0x0B, 0xB0, 0x06, 0x10],
            [0x0B, 0xB0, 0x26, 0x7F],
        ]
    );
}

/// A status inside a SysEx ends it: the unsent part is dropped, the new
/// message packs whole, and a late F7 is stray.
#[test]
fn a_status_aborts_sysex() {
    assert_eq!(
        pack(&[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0x90, 0x3C, 0x64, 0xF7]),
        [[0x04, 0xF0, 0x7E, 0x7F], [0x09, 0x90, 0x3C, 0x64]]
    );
}

// USB MIDI's IN queue, its Configured edge and OUT's cable filter.

use chimera_midi::usb::{ByteRing, Edge, InQueue, Left, cable_zero};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use usb_device::UsbError;
use usb_device::device::UsbDeviceState as S;

/// USB's out ring: bytes, and the messages the UI lost.
#[derive(Default)]
struct Ring {
    b: RefCell<VecDeque<u8>>,
    dropped: Cell<u32>,
    /// Losses the UI counts just after a take: a concurrent `lose`.
    late: Cell<u32>,
}

impl Ring {
    fn of(bytes: &[u8]) -> Self {
        let r = Ring::default();
        r.put(bytes);
        r
    }

    fn put(&self, bytes: &[u8]) {
        self.b.borrow_mut().extend(bytes);
    }

    fn len(&self) -> usize {
        self.b.borrow().len()
    }
}

impl ByteRing for Ring {
    fn pop(&self) -> Option<u8> {
        self.b.borrow_mut().pop_front()
    }
    fn is_empty(&self) -> bool {
        self.b.borrow().is_empty()
    }
    fn clear(&self) {
        self.b.borrow_mut().clear();
    }
    fn take_dropped(&self) -> u32 {
        self.dropped.replace(self.late.take())
    }
}

/// An IN endpoint answering each write from `script` (then `Ok`), keeping
/// what it took and every write it was offered.
#[derive(Default)]
struct Endpoint {
    script: VecDeque<UsbError>,
    took: Vec<[u8; 4]>,
    offered: Vec<Vec<u8>>,
}

impl Endpoint {
    fn answering(script: &[UsbError]) -> Self {
        Endpoint {
            script: script.iter().copied().collect(),
            ..Endpoint::default()
        }
    }

    fn write(&mut self, b: &[u8]) -> usb_device::Result<usize> {
        self.offered.push(b.to_vec());
        if let Some(e) = self.script.pop_front() {
            return Err(e);
        }
        self.took.extend(b.as_chunks::<4>().0);
        Ok(b.len())
    }
}

fn send(q: &mut InQueue, ring: &Ring, up: bool, ep: &mut Endpoint) -> u32 {
    q.send(ring, up, |b| ep.write(b))
}

/// `n` note-ons, keys from 0, each its own status.
fn notes(n: u8) -> Vec<u8> {
    (0..n).flat_map(|k| [0x90, k, 100]).collect()
}

#[test]
fn an_empty_queue_writes_nothing() {
    let (mut q, ring, mut ep) = (InQueue::new(), Ring::default(), Endpoint::default());
    assert_eq!(send(&mut q, &ring, true, &mut ep), 0);
    assert!(ep.offered.is_empty());
}

#[test]
fn the_ring_goes_out_packed() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    let ring = Ring::of(&[0x90, 60, 100, 0x80, 60, 0]);
    assert_eq!(send(&mut q, &ring, true, &mut ep), 0);
    assert_eq!(ep.offered, [vec![0x09, 0x90, 60, 100, 0x08, 0x80, 60, 0]]);
    assert!(ring.is_empty());
}

#[test]
fn sixteen_packets_a_write() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    let ring = Ring::of(&notes(20));
    send(&mut q, &ring, true, &mut ep);
    let sizes: Vec<_> = ep.offered.iter().map(Vec::len).collect();
    assert_eq!(sizes, [64, 16]);
    assert_eq!(ep.took, pack(&notes(20)));
}

#[test]
fn would_block_holds_every_packet_for_the_next_call() {
    let mut q = InQueue::new();
    let ring = Ring::of(&notes(20));
    let mut ep = Endpoint::answering(&[UsbError::WouldBlock]);
    assert_eq!(send(&mut q, &ring, true, &mut ep), 0);
    // Sixteen packed and held, the rest left in the ring.
    assert_eq!((ep.offered.len(), ep.took.len(), ring.len()), (1, 0, 4 * 3));
    ring.put(&[0xB0, 1, 2]);
    send(&mut q, &ring, true, &mut ep);
    assert_eq!(ep.offered[1], ep.offered[0]);
    let mut all = notes(20);
    all.extend([0xB0, 1, 2]);
    assert_eq!(ep.took, pack(&all));
}

#[test]
fn would_block_again_still_holds() {
    let mut q = InQueue::new();
    let ring = Ring::of(&notes(2));
    let mut ep = Endpoint::answering(&[UsbError::WouldBlock, UsbError::WouldBlock]);
    send(&mut q, &ring, true, &mut ep);
    send(&mut q, &ring, true, &mut ep);
    send(&mut q, &ring, true, &mut ep);
    assert_eq!(ep.offered.len(), 3);
    assert_eq!(ep.took, pack(&notes(2)));
}

#[test]
fn any_other_error_drops_and_counts_the_packets() {
    let mut q = InQueue::new();
    let ring = Ring::of(&notes(20));
    let mut ep = Endpoint::answering(&[UsbError::InvalidEndpoint]);
    // The first 16 refused and counted; the last 4 still go.
    assert_eq!(send(&mut q, &ring, true, &mut ep), 16);
    assert_eq!(ep.took, pack(&notes(20))[16..]);
}

#[test]
fn the_uis_lost_echo_is_counted() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    let ring = Ring::of(&notes(1));
    ring.dropped.set(3);
    assert_eq!(send(&mut q, &ring, true, &mut ep), 3);
    assert_eq!(ring.dropped.get(), 0);
}

#[test]
fn not_configured_discards_uncounted() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    let ring = Ring::of(&notes(3));
    ring.dropped.set(3);
    assert_eq!(send(&mut q, &ring, false, &mut ep), 0);
    assert!(ep.offered.is_empty());
    assert!(ring.is_empty());
    assert_eq!(ring.dropped.get(), 0);
}

#[test]
fn not_configured_drops_what_was_held() {
    let mut q = InQueue::new();
    let ring = Ring::of(&notes(2));
    let mut ep = Endpoint::answering(&[UsbError::WouldBlock]);
    send(&mut q, &ring, true, &mut ep);
    send(&mut q, &ring, false, &mut ep);
    ring.put(&[0x80, 7, 0]);
    send(&mut q, &ring, true, &mut ep);
    assert_eq!(ep.took, pack(&[0x80, 7, 0]));
}

#[test]
fn not_configured_resets_the_packer() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    // Half a note-on, waiting in the packer.
    let ring = Ring::of(&[0x90, 60]);
    send(&mut q, &ring, true, &mut ep);
    ring.put(&[0xF8]);
    send(&mut q, &ring, false, &mut ep);
    // Its running status is gone: these data bytes pack nothing.
    ring.put(&[61, 100]);
    send(&mut q, &ring, true, &mut ep);
    assert!(ep.took.is_empty());
}

#[test]
fn leaving_configured_says_how() {
    for (to, left) in [
        (S::Suspend, Left::Suspend),
        (S::Default, Left::Reset),
        (S::Addressed, Left::Reset),
    ] {
        let mut e = Edge::new();
        assert_eq!(e.poll(S::Configured), None);
        assert!(e.up());
        assert_eq!(e.poll(to), Some(left), "{to:?}");
        assert!(!e.up());
        assert_eq!(e.poll(to), None, "once");
    }
}

#[test]
fn no_edge_without_configured() {
    let mut e = Edge::new();
    for s in [S::Default, S::Addressed, S::Suspend, S::Default] {
        assert_eq!(e.poll(s), None, "{s:?}");
        assert!(!e.up());
    }
}

#[test]
fn coming_back_is_no_edge() {
    let mut e = Edge::new();
    e.poll(S::Configured);
    assert_eq!(e.poll(S::Suspend), Some(Left::Suspend));
    assert_eq!(e.poll(S::Configured), None);
    assert!(e.up());
    assert_eq!(e.poll(S::Configured), None);
    assert_eq!(e.poll(S::Default), Some(Left::Reset));
}

#[test]
fn only_cable_zeros_events_pass() {
    let p = [
        0x09, 0x90, 60, 100, // cable 0's note-on
        0x19, 0x90, 60, 100, // cable 1's
        0x00, 0x00, 0x00, 0x00, // CIN 0
        0x0F, 0xF8, 0, 0, // clock
        0x09, 0x90, // half a packet
    ];
    let got: Vec<_> = cable_zero(&p)
        .map(|e| e.map(|b| b.as_slice().to_vec()))
        .collect();
    assert_eq!(
        got,
        [Some(vec![0x90, 60, 100]), None, None, Some(vec![0xF8])]
    );
}

/// The UI lost an echo (#842): what waits is stale, held and ring both
/// dropped, the packer reset; the UI re-echoes once the ring is empty.
#[test]
fn a_lost_echo_flushes_what_waits() {
    let mut q = InQueue::new();
    let ring = Ring::of(&notes(20));
    let mut ep = Endpoint::answering(&[UsbError::WouldBlock]);
    send(&mut q, &ring, true, &mut ep);
    ring.put(&[0x90, 60]);
    ring.dropped.set(1);
    assert_eq!(send(&mut q, &ring, true, &mut ep), 1);
    assert!(ring.is_empty());
    ring.put(&[0xB0, 7, 9]);
    send(&mut q, &ring, true, &mut ep);
    assert_eq!(ep.took, pack(&[0xB0, 7, 9]));
}

/// The UI lost an echo while its ring stood empty: the loss is taken all
/// the same, so the UI's resend isn't held waiting on it.
#[test]
fn a_loss_on_an_empty_ring_is_taken() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    let ring = Ring::default();
    ring.dropped.set(1);
    assert_eq!(send(&mut q, &ring, true, &mut ep), 1);
    assert_eq!(ring.dropped.get(), 0);
}

/// A loss the UI counts after the take is the next poll's, never erased.
#[test]
fn a_loss_after_the_take_survives() {
    let (mut q, mut ep) = (InQueue::new(), Endpoint::default());
    let ring = Ring::of(&notes(2));
    ring.dropped.set(1);
    ring.late.set(1);
    assert_eq!(send(&mut q, &ring, true, &mut ep), 1);
    assert_eq!(ring.dropped.get(), 1);
}

/// A packet's bytes compare by what they hold: a short message's padding
/// is no part of it.
#[test]
fn bytes_ignore_padding() {
    let a = unpack([0x0C, 0xC0, 5, 0]).unwrap().1;
    let b = unpack([0x0C, 0xC0, 5, 9]).unwrap().1;
    assert_eq!(a, b);
}
