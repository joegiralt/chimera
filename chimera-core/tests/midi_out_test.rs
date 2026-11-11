//! DIN OUT's identity reply (spec § 10): a Device Inquiry heard off the
//! control ring queues one reply, which the UI's out pass writes into the
//! out ring whole, first in line.
#![cfg(feature = "midi-map")]

use chimera_core::midi_in::CtrlEvent;
use chimera_core::out_ring::{OUT_RING_LEN, OutRing};
use chimera_core::ui::UiState;
use chimera_core::ui::midi::out::version;
use chimera_hal::Ms;
use chimera_midi::value::U7;

fn inquiry(dev: u8) -> CtrlEvent {
    CtrlEvent::Identity {
        dev: U7::new(dev).unwrap(),
    }
}

/// One UI pass: what was heard, then the out pass.
fn pass(ui: &mut UiState, heard: &[CtrlEvent], ring: &OutRing, now: &mut u32) {
    *now += 10;
    ui.hear_ccs(heard.iter().copied(), Ms(*now), true);
    ui.send_out(ring);
}

fn drain(ring: &OutRing) -> Vec<u8> {
    let mut b = [0u8; OUT_RING_LEN];
    let n = ring.pop_into(&mut b);
    b[..n].to_vec()
}

/// This build's reply: `F0 7E 00 06 02`, maker 7D, family `43 48` ("CH"),
/// member `01 00` (PreenFM3), the version's four bytes, `F7`.
fn golden() -> Vec<u8> {
    let v = version(env!("CARGO_PKG_VERSION")).map(U7::get);
    let mut g = vec![0xF0, 0x7E, 0x00, 0x06, 0x02, 0x7D, 0x43, 0x48, 0x01, 0x00];
    g.extend_from_slice(&v);
    g.push(0xF7);
    g
}

#[test]
fn version_bytes_from_the_crate_version() {
    let v = |s| version(s).map(U7::get);
    assert_eq!(v("0.3.0"), [0, 3, 0, 0]);
    assert_eq!(v("0.2.0"), [0, 2, 0, 0]);
    assert_eq!(v("1.12.4-alpha.1"), [1, 12, 4, 0]);
    assert_eq!(v("200.0.7"), [127, 0, 7, 0], "past 127 holds at 127");
    assert_eq!(v("3"), [3, 0, 0, 0]);
}

#[test]
fn reply_bytes_golden() {
    let mut ui = Box::new(UiState::new());
    let ring = OutRing::new();
    let mut now = 1_000;
    pass(&mut ui, &[inquiry(0x7F)], &ring, &mut now);
    let got = drain(&ring);
    assert_eq!(got.len(), 15);
    assert_eq!(got, golden());
    // The workspace is 0.3.0 today: the bytes in full.
    if env!("CARGO_PKG_VERSION") == "0.3.0" {
        assert_eq!(
            got,
            [
                0xF0, 0x7E, 0x00, 0x06, 0x02, 0x7D, 0x43, 0x48, 0x01, 0x00, 0x00, 0x03, 0x00, 0x00,
                0xF7
            ]
        );
    }
}

#[test]
fn identity_reply_queued_once_per_request() {
    let mut ui = Box::new(UiState::new());
    let ring = OutRing::new();
    let mut now = 1_000;
    // Nothing asked, nothing sent.
    pass(&mut ui, &[], &ring, &mut now);
    assert!(drain(&ring).is_empty());
    // One request (to all, or to our device 0): one reply, and no more.
    for dev in [0x7F, 0x00] {
        pass(&mut ui, &[inquiry(dev)], &ring, &mut now);
        assert_eq!(drain(&ring), golden());
        for _ in 0..5 {
            pass(&mut ui, &[], &ring, &mut now);
        }
        assert!(drain(&ring).is_empty(), "answered once");
    }
    // Two in one pass coalesce, as the echo's dirty set does: one reply.
    pass(&mut ui, &[inquiry(0x7F), inquiry(0x7F)], &ring, &mut now);
    assert_eq!(drain(&ring), golden());
    // A full ring defers the reply, never drops it, and sends it whole.
    let filler = [0xB0u8, 1, 2];
    while ring.push_msg(&filler) {}
    pass(&mut ui, &[inquiry(0x7F)], &ring, &mut now);
    let full = drain(&ring);
    assert!(full.chunks(3).all(|c| c == filler), "no part of a reply");
    pass(&mut ui, &[], &ring, &mut now);
    assert_eq!(drain(&ring), golden(), "sent once there is room");
    pass(&mut ui, &[], &ring, &mut now);
    assert!(drain(&ring).is_empty());
}

/// Controller messages never make a reply.
#[test]
fn only_an_inquiry_replies() {
    use chimera_midi::ctl::{Ctl, FreeCc};
    let mut ui = Box::new(UiState::new());
    let ch = ui
        .project()
        .part(chimera_core::project::PartId::ALL[0])
        .mix
        .channel;
    let ring = OutRing::new();
    let mut now = 1_000;
    let cc = CtrlEvent::Ctl(
        ch,
        Ctl::Cc {
            cc: FreeCc::new(74).unwrap(),
            value: U7::new(3).unwrap(),
        },
    );
    pass(&mut ui, &[cc, CtrlEvent::SensingLost], &ring, &mut now);
    assert!(drain(&ring).is_empty());
}
