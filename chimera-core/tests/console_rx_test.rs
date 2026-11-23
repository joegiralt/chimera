// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Rx`: `put`'s data a packet a fill (a USB hold), handed out a byte at a
//! time (ADR 0222).

use chimera_core::console::Rx;

/// The host's bytes in 64-byte packets, each fill counted.
struct Host {
    data: Vec<u8>,
    at: usize,
    fills: u32,
}

impl Host {
    fn read(&mut self, buf: &mut [u8]) -> Option<usize> {
        self.fills += 1;
        let n = (self.data.len() - self.at).min(64).min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.at..self.at + n]);
        self.at += n;
        (n > 0).then_some(n)
    }
}

#[test]
fn bytes_come_in_order_one_fill_a_packet() {
    let data: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
    let mut host = Host {
        data: data.clone(),
        at: 0,
        fills: 0,
    };
    let mut rx = Rx::new();
    let mut got = Vec::new();
    loop {
        if let Some(b) = rx.pop() {
            got.push(b);
            continue;
        }
        if !rx.fill(|buf| host.read(buf)) {
            break;
        }
    }
    assert_eq!(got, data);
    // 64 + 64 + 64 + 8, then the empty read that ends it.
    assert_eq!(host.fills, 5);
}

#[test]
fn an_empty_read_leaves_it_empty() {
    let mut rx = Rx::new();
    assert_eq!(rx.pop(), None);
    assert!(!rx.fill(|_| None));
    assert!(!rx.fill(|_| Some(0)));
    assert_eq!(rx.pop(), None);
}

#[test]
fn a_short_packet_hands_out_only_its_bytes() {
    let mut rx = Rx::new();
    assert!(rx.fill(|buf| {
        buf[..3].copy_from_slice(b"abc");
        Some(3)
    }));
    assert_eq!(
        [rx.pop(), rx.pop(), rx.pop(), rx.pop()],
        [Some(b'a'), Some(b'b'), Some(b'c'), None]
    );
}
