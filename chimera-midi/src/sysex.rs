//! Universal Device Inquiry: the request and Chimera's identity reply.

use crate::out::Writer;
use crate::stream::SysEx;
use crate::value::{U7, U14};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Identity,
}

/// `F0 7E <7F|own> 06 01 F7`.
pub fn request(s: SysEx<'_>, own: U7) -> Option<Request> {
    match *s.bytes() {
        [0x7E, d, 0x06, 0x01] if d == 0x7F || d == own.get() => Some(Request::Identity),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Maker {
    /// 0x7D: non-commercial use.
    NonCommercial,
    One(U7),
    /// Extended ID: `00 a b`.
    Three(U7, U7),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub maker: Maker,
    pub family: U14,
    pub member: U14,
    pub version: [U7; 4],
}

/// `F0 7E dev 06 02 maker family(lo hi) member(lo hi) version F7`.
pub fn identity_reply(dev: U7, id: &Identity, w: &mut Writer) -> bool {
    let mut b = [0u8; 15];
    b[..4].copy_from_slice(&[0x7E, dev.get(), 0x06, 0x02]);
    let mut n = 4;
    match id.maker {
        Maker::NonCommercial => b[n..n + 1].copy_from_slice(&[0x7D]),
        Maker::One(a) => b[n..n + 1].copy_from_slice(&[a.get()]),
        Maker::Three(a, c) => b[n..n + 3].copy_from_slice(&[0, a.get(), c.get()]),
    }
    n += if matches!(id.maker, Maker::Three(..)) {
        3
    } else {
        1
    };
    for v in [id.family, id.member] {
        b[n] = v.lsb().get();
        b[n + 1] = v.msb().get();
        n += 2;
    }
    for v in id.version {
        b[n] = v.get();
        n += 1;
    }
    w.sysex(&b[..n])
}
