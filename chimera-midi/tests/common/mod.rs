// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(dead_code)]
use chimera_midi::stream::*;
use chimera_midi::value::*;

/// An owned copy of `Parsed`, so results outlive the parser borrow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Rt(RealTime),
    Voice(u8, Voice),
    Mode(u8, ChannelMode),
    Common(Common),
    SysEx(Vec<u8>),
    Fault(Fault),
}

pub fn own(p: Parsed<'_>) -> Item {
    match p {
        Parsed::RealTime(r) => Item::Rt(r),
        Parsed::Event(Event::Voice(c, v)) => Item::Voice(c.get(), v),
        Parsed::Event(Event::Mode(c, m)) => Item::Mode(c.get(), m),
        Parsed::Event(Event::Common(c)) => Item::Common(c),
        Parsed::Event(Event::SysEx(s)) => Item::SysEx(s.bytes().to_vec()),
        Parsed::Event(Event::Fault(f)) => Item::Fault(f),
    }
}

pub fn run<const N: usize>(p: &mut Parser<N>, bytes: &[u8]) -> Vec<Item> {
    bytes.iter().filter_map(|&b| p.push(b).map(own)).collect()
}

pub fn parse(bytes: &[u8]) -> Vec<Item> {
    run(&mut Parser::<32>::new(), bytes)
}

pub fn key(n: u8) -> Key {
    Key::new(n).unwrap()
}
pub fn u7(n: u8) -> U7 {
    U7::new(n).unwrap()
}
pub fn vel(n: u8) -> Velocity {
    Velocity::new(n).unwrap()
}

/// Bytes through `Parser` then `Controllers`: what layer 2 said, per voice event.
pub fn hear_bytes(
    c: &mut chimera_midi::ctl::Controllers,
    bytes: &[u8],
) -> Vec<(u8, chimera_midi::ctl::Heard)> {
    let mut p = Parser::<32>::new();
    let mut out = Vec::new();
    for &b in bytes {
        if let Some(Parsed::Event(Event::Voice(ch, v))) = p.push(b) {
            out.push((ch.get(), c.hear(ch, v)));
        }
    }
    out
}
