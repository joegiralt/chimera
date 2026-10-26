//! DIN OUT's pass (spec §§ 9-10): a Device Inquiry heard queues one
//! identity reply, written into the out ring whole when it fits, else
//! the next pass. Bank 2's, as `ui::midi` is.

use chimera_midi::out::Writer;
use chimera_midi::sysex::{Identity, Maker, identity_reply};
use chimera_midi::value::{U7, U14};

use super::super::UiState;
use crate::out_ring::OutRing;

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
static ID: Identity = Identity {
    maker: Maker::NonCommercial,
    family: U14::from_parts(u7(0x48), u7(0x43)),
    member: U14::from_parts(U7::ZERO, u7(1)),
    version: version(env!("CARGO_PKG_VERSION")),
};

/// The reply's length: `F0`, 13 bytes, `F7`.
pub const REPLY_LEN: usize = 15;

impl UiState {
    /// The out pass: a pending identity reply into `ring`, whole. The
    /// door's `out`.
    pub(in crate::ui) fn out_in(&mut self, ring: &OutRing) {
        let m = self.cc.ui();
        if !m.reply {
            return;
        }
        let mut b = [0u8; REPLY_LEN];
        let mut w = Writer::new(&mut b);
        // Our device byte is 0: the parser hears 7F or 0 (`midi_in`).
        if !identity_reply(U7::ZERO, &ID, &mut w) {
            return;
        }
        let n = w.len();
        if ring.push_msg(&b[..n]) {
            m.reply = false;
        }
    }
}
