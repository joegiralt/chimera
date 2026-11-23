// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DIN OUT's byte ring (spec § 4.2): the UI queues whole messages, the
//! TX interrupt pops bytes; a message that doesn't fit is refused whole.

use chimera_engine::midi_in::Port;
use chimera_engine::out_ring::{OUT_RING_LEN, OutPorts, OutRing};
use proptest::prelude::*;

#[test]
fn out_ring_whole_messages_only() {
    let r = OutRing::new();
    let msg = [0xB0, 74, 64];
    let fit = OUT_RING_LEN / msg.len();
    for _ in 0..fit {
        assert!(r.push_msg(&msg));
    }
    // 255 of 256 bytes taken: a 3-byte message is refused, nothing of it kept.
    assert!(!r.push_msg(&msg));
    assert!(r.push_msg(&[0xF8]), "one byte still fits");
    assert!(!r.push_msg(&[0xF8]));
    let mut out = [0u8; OUT_RING_LEN + 8];
    let n = r.pop_into(&mut out);
    assert_eq!(n, fit * msg.len() + 1);
    assert!(out[..n - 1].chunks(3).all(|c| c == msg));
    assert_eq!(out[n - 1], 0xF8);
    assert_eq!(r.pop_into(&mut out), 0);
    assert!(r.is_empty());
    // Past the wrap, a message still goes in whole.
    let big = [0x55u8; OUT_RING_LEN];
    assert!(r.push_msg(&big));
    assert!(!r.push_msg(&[1]));
    assert_eq!(r.pop_into(&mut out[..100]), 100);
    assert!(!r.push_msg(&[1u8; 101]), "100 free");
    assert!(r.push_msg(&[1u8; 100]));
    let n = r.pop_into(&mut out);
    assert_eq!(n, OUT_RING_LEN);
    assert!(out[..156].iter().all(|&b| b == 0x55) && out[156..n].iter().all(|&b| b == 1));
    assert!(!r.push_msg(&[0u8; OUT_RING_LEN + 1]), "never fits");
    assert!(r.is_empty());
}

#[test]
fn empty_message_is_no_message() {
    let r = OutRing::new();
    assert!(r.push_msg(&[]));
    assert!(r.is_empty());
}

#[derive(Clone, Debug)]
enum Op {
    Push(Vec<u8>),
    Pop(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 1..40).prop_map(Op::Push),
        (0usize..64).prop_map(Op::Pop),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// Any interleaving: the bytes come out in the order their messages
    /// went in, each accepted message whole, each refused one not at all,
    /// and a refusal only when it truly didn't fit.
    #[test]
    fn out_ring_spsc_order(ops in proptest::collection::vec(op(), 1..200)) {
        let r = OutRing::new();
        let mut sent = Vec::new();
        let mut got = Vec::new();
        let mut held = 0usize;
        for o in ops {
            match o {
                Op::Push(m) => {
                    let ok = r.push_msg(&m);
                    prop_assert_eq!(ok, held + m.len() <= OUT_RING_LEN);
                    if ok {
                        held += m.len();
                        sent.extend_from_slice(&m);
                    }
                }
                Op::Pop(k) => {
                    let mut b = vec![0u8; k];
                    let n = r.pop_into(&mut b);
                    prop_assert_eq!(n, k.min(held));
                    held -= n;
                    got.extend_from_slice(&b[..n]);
                }
            }
        }
        let mut b = [0u8; OUT_RING_LEN];
        let n = r.pop_into(&mut b);
        got.extend_from_slice(&b[..n]);
        prop_assert_eq!(got, sent);
    }
}

/// A producer thread and a consumer thread: every byte arrives, in order.
#[test]
fn out_ring_spsc_threads() {
    use std::sync::Arc;
    let r = Arc::new(OutRing::new());
    let w = Arc::clone(&r);
    let total = 20_000usize;
    let producer = std::thread::spawn(move || {
        let mut i = 0usize;
        while i < total {
            let len = 1 + i % 15;
            let m: Vec<u8> = (i..i + len).map(|x| x as u8).collect();
            if w.push_msg(&m) {
                i += len;
            } else {
                std::thread::yield_now();
            }
        }
    });
    let mut got = Vec::with_capacity(total + 15);
    let mut b = [0u8; 7];
    while got.len() < total {
        let n = r.pop_into(&mut b);
        got.extend_from_slice(&b[..n]);
        if n == 0 {
            std::thread::yield_now();
        }
    }
    producer.join().unwrap();
    assert!(got.iter().enumerate().all(|(i, &b)| b == i as u8));
}

fn popped(r: &OutRing) -> Vec<u8> {
    let mut b = [0u8; OUT_RING_LEN];
    let n = r.pop_into(&mut b);
    b[..n].to_vec()
}

/// One pass: each message whole into DIN's ring and copied to USB's.
#[test]
fn tee_copies_each_message() {
    let (din, usb) = (OutRing::new(), OutRing::new());
    let mut t = OutPorts::new(&din, &usb).tee();
    assert!(t.send(&[0xB0, 20, 1]) && t.send(&[21, 2]));
    assert_eq!(popped(&din), [0xB0, 20, 1, 21, 2]);
    assert_eq!(popped(&usb), [0xB0, 20, 1, 21, 2]);
}

/// DIN full: nothing goes anywhere, the pass stops.
#[test]
fn tee_holds_back_on_din_only() {
    let (din, usb) = (OutRing::new(), OutRing::new());
    assert!(din.push_msg(&[0; OUT_RING_LEN - 2]));
    let mut t = OutPorts::new(&din, &usb).tee();
    assert!(!t.send(&[0xB0, 20, 1]));
    assert!(usb.is_empty(), "nothing copied of a message DIN refused");
}

/// USB full: DIN sends on; the copy and the rest of the pass lost, each
/// counted on USB's ring, even one that would fit again.
#[test]
fn tee_loses_the_rest_of_the_pass_on_usb() {
    let (din, usb) = (OutRing::new(), OutRing::new());
    assert!(usb.push_msg(&[0; OUT_RING_LEN - 2]));
    let mut t = OutPorts::new(&din, &usb).tee();
    assert!(t.send(&[0xB0, 20, 1]));
    assert!(t.send(&[0xF8]));
    assert_eq!(popped(&din), [0xB0, 20, 1, 0xF8]);
    assert_eq!(usb.take_dropped(), 2);
    assert_eq!(popped(&usb).len(), OUT_RING_LEN - 2, "nothing of either");
    // The next pass starts whole.
    let mut t = OutPorts::new(&din, &usb).tee();
    assert!(t.send(&[0xF8]));
    assert_eq!(popped(&usb), [0xF8]);
}

/// A reply out of one port only; only DIN's can wait.
#[test]
fn tee_sends_to_one_port() {
    let (din, usb) = (OutRing::new(), OutRing::new());
    let mut t = OutPorts::new(&din, &usb).tee();
    assert!(t.send_to(Port::Usb, &[0xF0, 0xF7]));
    assert!(din.is_empty());
    assert_eq!(popped(&usb), [0xF0, 0xF7]);
    assert!(din.push_msg(&[0; OUT_RING_LEN]));
    assert!(!t.send_to(Port::Din, &[0xF0, 0xF7]), "DIN's waits");
    assert!(usb.is_empty());
}

/// DIN alone (the desktop, a build without USB): copies go nowhere, uncounted.
#[test]
fn tee_din_alone() {
    let din = OutRing::new();
    let mut t = OutPorts::din(&din).tee();
    assert!(t.send(&[0xF8]) && t.send_to(Port::Usb, &[0xF8]));
    assert_eq!(popped(&din), [0xF8]);
}

/// A port nobody reads: what waits and what was lost, both gone.
#[test]
fn discard_forgets_losses() {
    let (din, usb) = (OutRing::new(), OutRing::new());
    assert!(usb.push_msg(&[0; OUT_RING_LEN]));
    let mut t = OutPorts::new(&din, &usb).tee();
    assert!(t.send(&[0xF8]));
    usb.discard();
    assert!(usb.is_empty());
    assert_eq!(usb.take_dropped(), 0);
    assert!(usb.push_msg(&[0; OUT_RING_LEN]), "the whole ring free");
}

#[test]
fn clear_drops_what_waits() {
    let r = OutRing::new();
    assert!(r.push_msg(&[1, 2, 3]));
    r.clear();
    assert!(r.is_empty());
    assert!(r.push_msg(&[0; OUT_RING_LEN]), "the whole ring free");
}
