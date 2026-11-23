// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bytes out: whole messages into a buffer, running status within a burst.

use crate::ctl::{DataValue, FreeCc};
use crate::value::{Channel, U7, U14};

/// The MSB CC of a 14-bit pair: 1..=31 minus 6 (its LSB is MSB + 32).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cc14Msb(u8);

impl Cc14Msb {
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        match n {
            1..=5 | 7..=31 => Some(Self(n)),
            _ => None,
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Microseconds `bytes` take on the line: 320 at 31,250 baud.
/// `as u32`: callers pass a message's bytes, far below the 13.4 M that
/// would overflow.
pub const fn din_us(bytes: usize) -> u32 {
    bytes as u32 * 320
}

/// The status a writer last sent: the next writer on the same line may
/// leave it out (`Writer::after`). Only a writer makes one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Running(Option<u8>);

impl Running {
    /// Nothing sent yet: the next message sends its status.
    pub const NONE: Self = Self(None);
}

/// Writes whole messages or nothing. Running status holds until
/// `clear_status` or a SysEx.
pub struct Writer<'a> {
    buf: &'a mut [u8],
    len: usize,
    status: Option<u8>,
}

impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            len: 0,
            status: None,
        }
    }

    /// A writer whose line last carried `r`: its first message leaves out
    /// a status equal to `r`'s.
    pub fn after(buf: &'a mut [u8], r: Running) -> Self {
        Self {
            buf,
            len: 0,
            status: r.0,
        }
    }

    /// The status the line now runs on.
    pub const fn running(&self) -> Running {
        Running(self.status)
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The next message sends its status byte.
    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// One control-change status, then each `(controller, value)` pair.
    fn ccs(&mut self, ch: Channel, pairs: &[(u8, u8)]) -> bool {
        let st = 0xB0 | ch.get();
        let head = usize::from(self.status != Some(st));
        let need = head + 2 * pairs.len();
        if self.buf.len() - self.len < need {
            return false;
        }
        if head == 1 {
            self.buf[self.len] = st;
            self.len += 1;
            self.status = Some(st);
        }
        for &(c, v) in pairs {
            self.buf[self.len] = c;
            self.buf[self.len + 1] = v;
            self.len += 2;
        }
        true
    }

    pub fn cc(&mut self, ch: Channel, cc: FreeCc, v: U7) -> bool {
        self.ccs(ch, &[(cc.get(), v.get())])
    }

    pub fn cc14(&mut self, ch: Channel, msb: Cc14Msb, v: U14) -> bool {
        self.ccs(
            ch,
            &[(msb.get(), v.msb().get()), (msb.get() + 32, v.lsb().get())],
        )
    }

    /// CC 99, 98, 6 and, for `Fine`, 38.
    pub fn nrpn(&mut self, ch: Channel, n: U14, v: DataValue) -> bool {
        let (m, l) = (n.msb().get(), n.lsb().get());
        match v {
            DataValue::Coarse(d) => self.ccs(ch, &[(99, m), (98, l), (6, d.get())]),
            DataValue::Fine(d) => self.ccs(
                ch,
                &[(99, m), (98, l), (6, d.msb().get()), (38, d.lsb().get())],
            ),
        }
    }

    /// `F0 body F7`; every body byte must be a data byte. Cancels running status.
    pub fn sysex(&mut self, body: &[u8]) -> bool {
        if body.iter().any(|&b| b > 127) || self.buf.len() - self.len < body.len() + 2 {
            return false;
        }
        self.buf[self.len] = 0xF0;
        self.buf[self.len + 1..self.len + 1 + body.len()].copy_from_slice(body);
        self.buf[self.len + 1 + body.len()] = 0xF7;
        self.len += body.len() + 2;
        self.status = None;
        true
    }
}
