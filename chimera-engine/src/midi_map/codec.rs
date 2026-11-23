// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A Sound's map on the card (MIDI phase 1 spec § 6.4), bank 2's behind
//! `MidiDoor`. `Bindings` (0x001F): a version byte, then 8 B an entry,
//! `[tag:3 | kind:5] [number hi] [number lo] [block] [param] [min] [max]
//! [flags: bit 0 echo]`; version 2, written only for a map with a SysEx
//! entry (#846, ADR 0241), adds the entry's SysEx key, 4 B LE (0 for the
//! rest): 12 B an entry, a SysEx entry `[4 << 5 | JUMP] [shape] [0]`.
//! `MidiMap` (0x001E), read only: 5 B an entry,
//! `(cc, block, param, min, max)`, each CC as JUMP without echo.
//!
//! An entry this firmware can't hold (an unknown tag or kind, a kind its
//! control can't take, a reserved CC, an MSB outside `Cc14Msb`, an unknown
//! address or a target that isn't `Mappable`) is skipped and counted. A
//! duplicate target keeps the last. A version it can't read leaves the map
//! empty, counted as one skip (#816). A bad length is `Bounds`.

use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;
use chimera_midi::value::{U7, U14};

use super::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, MAP_LEN, Mappable, MidiMap,
    NrpnKind, Span, SxKey,
};
use crate::storage::RecordBuf;
use crate::storage::{addr_code, addr_of};
use chimera_midi::sxmap::Shape;

/// Which of a Sound's two map records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRecord {
    /// 0x001E, read only.
    Legacy,
    /// 0x001F.
    Bindings,
}

/// A payload of the wrong shape: the file is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds;

const VERSION: u8 = 1;
const ENTRY_LEN: usize = 8;
/// With SysEx keys.
const VERSION_SX: u8 = 2;
const ENTRY_SX_LEN: usize = 12;
const LEGACY_LEN: usize = 5;
const ECHO: u8 = 1;

/// The control, the entry byte's top three bits. Frozen (ADR 0045).
const TAG_CC: u8 = 0;
const TAG_CC14: u8 = 1;
const TAG_NRPN: u8 = 2;
const TAG_PRESSURE: u8 = 3;
const TAG_SYSEX: u8 = 4;

/// How a control is read, whatever the control.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Absolute(Takeover),
    Relative(ByteEncoding),
    IncDec,
    Button(ButtonMode),
}

/// Each kind's code, the entry byte's low five bits, is its index. Frozen
/// (ADR 0045): append only, never reorder.
const KINDS: [Kind; 10] = [
    Kind::Absolute(Takeover::Jump),
    Kind::Absolute(Takeover::Pickup),
    Kind::Absolute(Takeover::Scale),
    Kind::Relative(ByteEncoding::TwosComplement),
    Kind::Relative(ByteEncoding::BinaryOffset),
    Kind::Relative(ByteEncoding::SignBitDown),
    Kind::Relative(ByteEncoding::SignBitUp),
    Kind::IncDec,
    Kind::Button(ButtonMode::Momentary),
    Kind::Button(ButtonMode::Toggle),
];
const _: () = assert!(KINDS.len() <= 32);

/// `input` as `[tag | kind, number hi, number lo]`.
fn input_bytes(input: Input) -> [u8; 3] {
    let (tag, kind, hi, lo) = match input {
        Input::Cc(c, k) => {
            let k = match k {
                CcKind::Absolute(t) => Kind::Absolute(t),
                CcKind::Relative(e) => Kind::Relative(e),
                CcKind::Button(b) => Kind::Button(b),
            };
            (TAG_CC, k, 0, c.get())
        }
        Input::Cc14(m, t) => (TAG_CC14, Kind::Absolute(t), 0, m.get()),
        Input::Nrpn(n, k) => {
            let k = match k {
                NrpnKind::Absolute(t) => Kind::Absolute(t),
                NrpnKind::Relative(e) => Kind::Relative(e),
                NrpnKind::IncDec => Kind::IncDec,
                NrpnKind::Button(b) => Kind::Button(b),
            };
            (TAG_NRPN, k, n.msb().get(), n.lsb().get())
        }
        Input::Pressure(t) => (TAG_PRESSURE, Kind::Absolute(t), 0, 0),
        Input::SysEx(k) => (
            TAG_SYSEX,
            Kind::Absolute(Takeover::Jump),
            k.shape().map_or(0, Shape::bits),
            0,
        ),
    };
    // Every kind is in the table.
    let code = KINDS.iter().position(|&x| x == kind).unwrap_or(0) as u8;
    [tag << 5 | code, hi, lo]
}

/// The `Input` these bytes name, if this firmware has it; `key` is a
/// version 2 entry's SysEx key, and a SysEx entry without one is none.
fn input_of(tag_kind: u8, hi: u8, lo: u8, key: Option<u32>) -> Option<Input> {
    let kind = *KINDS.get(usize::from(tag_kind & 0x1F))?;
    Some(match (tag_kind >> 5, kind) {
        (TAG_CC, k) if hi == 0 => {
            let k = match k {
                Kind::Absolute(t) => CcKind::Absolute(t),
                Kind::Relative(e) => CcKind::Relative(e),
                Kind::Button(b) => CcKind::Button(b),
                Kind::IncDec => return None,
            };
            Input::Cc(LearnCc::new(lo)?, k)
        }
        (TAG_CC14, Kind::Absolute(t)) if hi == 0 => Input::Cc14(Cc14Msb::new(lo)?, t),
        (TAG_NRPN, k) => {
            let n = U14::from_parts(U7::new(hi)?, U7::new(lo)?);
            Input::Nrpn(
                n,
                match k {
                    Kind::Absolute(t) => NrpnKind::Absolute(t),
                    Kind::Relative(e) => NrpnKind::Relative(e),
                    Kind::IncDec => NrpnKind::IncDec,
                    Kind::Button(b) => NrpnKind::Button(b),
                },
            )
        }
        (TAG_PRESSURE, Kind::Absolute(t)) if hi == 0 && lo == 0 => Input::Pressure(t),
        (TAG_SYSEX, Kind::Absolute(Takeover::Jump)) if lo == 0 => {
            Input::SysEx(SxKey::new(Shape::from_bits(hi)?, key?)?)
        }
        _ => return None,
    })
}

/// `e` into `m`: over its target's entry (a param appears once) or after.
fn put(m: &mut MidiMap, e: Binding) {
    match m.entries().iter().position(|x| x.target == e.target) {
        Some(i) => m.set(i, e),
        None => {
            // At most 16 entries are read, so there is always room.
            if m.learn(e.input, e.target).is_ok() {
                m.set(m.len() - 1, e);
            }
        }
    }
}

/// Checks `p`, a `which` record, and decodes it onto `m` from empty if
/// there is one: the entries skipped (none counted without `m`).
#[inline(never)]
pub fn decode(which: MapRecord, p: &[u8], m: Option<&mut MidiMap>) -> Result<u16, Bounds> {
    let (entries, n) = match which {
        MapRecord::Bindings => {
            let (&v, rest) = p.split_first().ok_or(Bounds)?;
            let n = if v == VERSION_SX {
                ENTRY_SX_LEN
            } else {
                ENTRY_LEN
            };
            if v != VERSION && v != VERSION_SX {
                return Ok(match m {
                    Some(m) => {
                        *m = MidiMap::EMPTY;
                        1
                    }
                    None => 0,
                });
            }
            if !rest.len().is_multiple_of(n) {
                return Err(Bounds);
            }
            (rest, n)
        }
        MapRecord::Legacy if p.len().is_multiple_of(LEGACY_LEN) => (p, LEGACY_LEN),
        MapRecord::Legacy => return Err(Bounds),
    };
    if entries.len() / n > MAP_LEN {
        return Err(Bounds);
    }
    let Some(m) = m else {
        return Ok(0);
    };
    *m = MidiMap::EMPTY;
    let mut lost = 0;
    for e in entries.chunks_exact(n) {
        let (input, rest, feedback) = match which {
            MapRecord::Bindings => (
                input_of(
                    e[0],
                    e[1],
                    e[2],
                    e.get(8..12)
                        .map(|k| u32::from_le_bytes([k[0], k[1], k[2], k[3]])),
                ),
                &e[3..7],
                if e[7] & ECHO != 0 {
                    Feedback::Value
                } else {
                    Feedback::Off
                },
            ),
            MapRecord::Legacy => (Input::cc(e[0]), &e[1..5], Feedback::Off),
        };
        let &[block, id, min, max] = rest else {
            continue;
        };
        let target = addr_of(block, id).and_then(Mappable::new);
        let Some((input, target)) = input.zip(target) else {
            lost += 1;
            continue;
        };
        put(
            m,
            Binding {
                input,
                target,
                span: Span { min, max },
                feedback,
            },
        );
    }
    Ok(lost)
}

/// The bytes `encode` writes for `m`: none for an empty map.
pub fn encoded_len(m: &MidiMap) -> usize {
    if m.is_empty() {
        return 0;
    }
    let sx = m
        .entries()
        .iter()
        .any(|e| matches!(e.input, Input::SysEx(_)));
    1 + m.len() * if sx { ENTRY_SX_LEN } else { ENTRY_LEN }
}

/// `m` as a `Bindings` payload into `r`; nothing for an empty map.
#[inline(never)]
pub fn encode(m: &MidiMap, r: &mut RecordBuf) {
    if m.is_empty() {
        return;
    }
    let sx = m
        .entries()
        .iter()
        .any(|e| matches!(e.input, Input::SysEx(_)));
    r.u8(if sx { VERSION_SX } else { VERSION });
    for e in m.entries() {
        r.bytes(&input_bytes(e.input));
        r.bytes(&addr_code(e.target.addr()));
        r.u8(e.span.min);
        r.u8(e.span.max);
        r.u8(if e.feedback == Feedback::Value {
            ECHO
        } else {
            0
        });
        if sx {
            let k = match e.input {
                Input::SysEx(k) => k.key(),
                _ => 0,
            };
            r.bytes(&k.to_le_bytes());
        }
    }
}
