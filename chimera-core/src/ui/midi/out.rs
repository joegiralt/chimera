// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI out's pass (spec §§ 7, 8.4, 9-10): with MIDI CONFIG › ECHO on, the
//! echo plan's identity replies and value echo, then PARAM OUT's NRPNs
//! (ADR 0258) and a dump asked for (ADR 0301), into DIN's ring and USB's,
//! each message whole when it fits, else a later pass (DIN) or lost
//! (USB). Bank 2's, as `ui::midi` is.

use chimera_midi::sysex::{Identity, Maker};
use chimera_midi::value::{U7, U14};

use super::super::UiState;
use crate::out_ring::OutPorts;

/// `v`, past 127 held at 127.
const fn u7(v: u8) -> U7 {
    match U7::new(v) {
        Some(u) => u,
        None => U7::MAX,
    }
}

/// The version's bytes: major, minor, patch, 0; each past 127 holds at
/// 127, and a pre-release suffix is left out.
pub const fn version(s: &str) -> [U7; 4] {
    let b = s.as_bytes();
    let mut out = [0u8; 4];
    let (mut i, mut k) = (0, 0);
    while i < b.len() && k < 3 {
        match b[i] {
            b'0'..=b'9' => {
                let d = (b[i] - b'0') as u16;
                let v = out[k] as u16 * 10 + d;
                out[k] = if v > 127 { 127 } else { v as u8 };
            }
            b'.' => k += 1,
            _ => break,
        }
        i += 1;
    }
    [u7(out[0]), u7(out[1]), u7(out[2]), U7::ZERO]
}

/// Chimera on the PreenFM3: maker 7D (owner, spec § 13: swap for a bought
/// MMA ID before anything is sold), family "CH", member 1 (the Blackbox
/// port takes 2). A named static, so its bytes are bank 2's.
pub(super) static ID: Identity = Identity {
    maker: Maker::NonCommercial,
    family: U14::from_parts(u7(0x48), u7(0x43)),
    member: U14::from_parts(U7::ZERO, u7(1)),
    version: version(env!("CARGO_PKG_VERSION")),
};

/// The reply's length: `F0`, 13 bytes, `F7`.
pub const REPLY_LEN: usize = 15;

impl UiState {
    /// The out pass, after `hear_in` (its clock): with ECHO on, the echo
    /// plan's messages out of `out`'s ports (`EchoPlan::pump`); off,
    /// nothing, and an inquiry heard is dropped (spec § 8.4). The door's
    /// `out`.
    pub(in crate::ui) fn out_in(&mut self, out: &OutPorts) {
        let (on, at, now) = (self.midi.echo(), self.active_part, self.cc.last);
        let params = on && self.midi.param_out();
        let super::MidiUi {
            echo,
            ctl,
            param_out,
            dump,
            ..
        } = self.cc.ui();
        if on {
            echo.pump(&self.project, ctl, at, now, out);
        } else {
            echo.silence();
        }
        // PARAM OUT (ADR 0258): the hand's turns, after the echo.
        if params {
            param_out.pump(&self.project, out);
        } else {
            param_out.clear();
        }
        // A dump asked for (ADR 0301): last, into the room the others left.
        if on {
            dump.pump(&self.project, out);
        } else {
            dump.silence();
        }
    }
}
