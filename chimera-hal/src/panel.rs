// Copyright (c) 2017 martnak (the ILI9341 init list, martnak/STM32-ILI9341, MIT, via PreenFM3; notice below)
// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later AND MIT

// The ILI9341 init list in `INIT` comes from PreenFM3's `lib/Src/ili9341.c`,
// whose `ILI9341_Init` bases it on martnak/STM32-ILI9341
// (https://github.com/martnak/STM32-ILI9341), under the MIT licence:
//
// Copyright (c) 2017 martnak
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

//! The ILI9341 over its serial link (ADR 0326): the bytes `init` and a
//! flush send, and the health check that re-inits a panel that left its
//! mode. SPI1 runs the link at 50 MHz, past the datasheet's write cycle; a
//! byte it garbles can turn a CASET into DISPOFF or COLMOD, and nothing a
//! flush sends brings that back. Pure over `Link`, so a host test drives it
//! against a register-level panel.

use crate::Ms;

/// The panel's wires, as the shell drives them.
pub trait Link {
    /// DC low, CS low, `c`, CS high.
    fn cmd(&mut self, c: u8);
    /// DC high, CS low, `d`, CS high.
    fn data(&mut self, d: &[u8]);
    /// `c`, then the one byte the panel answers (an 8-bit read: no dummy
    /// clock), at a read's clock. `None`: the link never answered.
    fn read(&mut self, c: u8) -> Option<u8>;
    /// RESX: low holds the panel in reset.
    fn reset(&mut self, low: bool);
    fn delay_us(&mut self, us: u32);
}

/// The commands the shell sends by name (datasheet V1.11 § 8.1).
pub mod cmd {
    pub const SWRESET: u8 = 0x01;
    pub const RDDPM: u8 = 0x0A;
    pub const RDDMADCTL: u8 = 0x0B;
    pub const RDDCOLMOD: u8 = 0x0C;
    pub const SLPOUT: u8 = 0x11;
    pub const DISPON: u8 = 0x29;
    pub const CASET: u8 = 0x2A;
    pub const PASET: u8 = 0x2B;
    pub const RAMWR: u8 = 0x2C;
    pub const MADCTL: u8 = 0x36;
    pub const COLMOD: u8 = 0x3A;
    pub const PGAMCTRL: u8 = 0xE0;
    pub const NGAMCTRL: u8 = 0xE1;
}

/// MADCTL: MX | BGR.
pub const MADCTL: u8 = 0x48;
/// COLMOD: 16 bits a pixel.
pub const COLMOD: u8 = 0x55;

/// Datasheet V1.11 § 8.2, in the order PreenFM3's `ili9341.c` (after
/// martnak/STM32-ILI9341) sends them:
/// each command, its parameter count, its parameters. Gamma stays at the
/// reset tables; the shell pushes THEME's after.
#[rustfmt::skip]
const INIT: [u8; 64] = [
    0xCB, 5, 0x39, 0x2C, 0x00, 0x34, 0x02,
    0xCF, 3, 0x00, 0xC1, 0x30,
    0xE8, 3, 0x85, 0x00, 0x78,
    0xEA, 2, 0x00, 0x00,
    0xED, 4, 0x64, 0x03, 0x12, 0x81,
    0xF7, 1, 0x20,
    0xC0, 1, 0x23,
    0xC1, 1, 0x10,
    0xC5, 2, 0x3E, 0x28,
    0xC7, 1, 0x86,
    cmd::MADCTL, 1, MADCTL,
    cmd::COLMOD, 1, COLMOD,
    0xB1, 2, 0x00, 0x18,
    0xB6, 3, 0x08, 0x82, 0x27,
    0xF2, 1, 0x00,
    0x26, 1, 0x01,
];

/// Hardware reset, SWRESET, the init table, sleep out, display on.
pub fn init(l: &mut impl Link) {
    l.reset(true);
    l.delay_us(12_500);
    l.reset(false);
    l.delay_us(150_000);
    l.cmd(cmd::SWRESET);
    l.delay_us(12_500);
    let mut rest = &INIT[..];
    while let [c, n, tail @ ..] = rest {
        let n = usize::from(*n);
        let (Some(d), Some(next)) = (tail.get(..n), tail.get(n..)) else {
            break;
        };
        l.cmd(*c);
        l.data(d);
        rest = next;
    }
    l.cmd(cmd::SLPOUT);
    l.delay_us(150_000);
    l.cmd(cmd::DISPON);
}

/// Rows `y_start..y_end`, every column, opened for RAMWR: a flush's head.
pub fn window(l: &mut impl Link, y_start: u16, y_end: u16) {
    l.cmd(cmd::CASET);
    l.data(&[0x00, 0x00, 0x00, 0xEF]);
    let ys = y_start.to_be_bytes();
    let ye = (y_end - 1).to_be_bytes();
    l.cmd(cmd::PASET);
    l.data(&[ys[0], ys[1], ye[0], ye[1]]);
    l.cmd(cmd::RAMWR);
}

/// The panel's mode as RDDPM, RDDMADCTL and RDDCOLMOD answer it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub power: u8,
    pub madctl: u8,
    pub colmod: u8,
}

impl Mode {
    /// What `init` leaves: booster on, sleep out, normal mode, display on;
    /// MADCTL and COLMOD as sent.
    pub const UP: Mode = Mode {
        power: 0x9C,
        madctl: MADCTL,
        colmod: COLMOD,
    };

    /// `UP`, or `UP` with RDDPM's 0xDE, which stock PreenFM3 also reads as
    /// healthy (`TftDisplay::tic`).
    pub fn healthy(self) -> bool {
        matches!(self.power, 0x9C | 0xDE) && self.madctl == MADCTL && self.colmod == COLMOD
    }
}

/// How often a healthy panel is read.
pub const CHECK_MS: u32 = 500;
/// The interval doubles after each re-init, up to 2^`BACKOFF_MAX` × 500 ms
/// (16 s): a fault init can't clear never holds the UI in init.
pub const BACKOFF_MAX: u8 = 5;

/// The check's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Health {
    /// The panel has once answered `healthy`. Until then nothing re-inits:
    /// a link that reads nothing, or reads the registers in another
    /// encoding, never storms.
    armed: bool,
    bad: u8,
    backoff: u8,
    next: Ms,
}

impl Health {
    pub const fn new() -> Self {
        Self {
            armed: false,
            bad: 0,
            backoff: 0,
            next: Ms(0),
        }
    }

    pub fn armed(&self) -> bool {
        self.armed
    }

    /// `read`, as answered at `now` (`None`: a read failed, which counts
    /// as off-mode): true when an armed panel has read wrong twice in a row
    /// and must be re-inited. One wrong read is a read's own noise as
    /// likely as the panel's.
    pub fn judge(&mut self, now: Ms, read: Option<Mode>) -> bool {
        let ok = read.is_some_and(Mode::healthy);
        let reinit = if ok {
            self.armed = true;
            self.bad = 0;
            self.backoff = 0;
            false
        } else if self.armed {
            self.bad += 1;
            self.bad >= 2
        } else {
            false
        };
        if reinit {
            self.bad = 0;
            self.backoff = (self.backoff + 1).min(BACKOFF_MAX);
        }
        self.next = Ms(now.0.wrapping_add(CHECK_MS << self.backoff));
        reinit
    }

    pub fn due(&self, now: Ms) -> bool {
        now.since(self.next) < u32::MAX / 2
    }
}

impl Default for Health {
    fn default() -> Self {
        Self::new()
    }
}

/// The check, when due: true if it re-inited the panel, which then needs
/// THEME's gamma and a full flush from the shell.
pub fn tend(h: &mut Health, now: Ms, l: &mut impl Link) -> bool {
    if !h.due(now) {
        return false;
    }
    let read = (|| {
        Some(Mode {
            power: l.read(cmd::RDDPM)?,
            madctl: l.read(cmd::RDDMADCTL)?,
            colmod: l.read(cmd::RDDCOLMOD)?,
        })
    })();
    let reinit = h.judge(now, read);
    if reinit {
        init(l);
    }
    reinit
}
