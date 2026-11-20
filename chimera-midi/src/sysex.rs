//! Universal Device Inquiry: the request and Chimera's identity reply;
//! the MIDI Tuning Standard's retunes (`mts`); Chimera's own messages:
//! the parameter dump's request and end (#984).

use crate::out::Writer;
use crate::stream::SysEx;
use crate::value::{Channel, Key, U7, U14};

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

/// The head of Chimera's own messages: the identity reply's maker
/// (`7D`, non-commercial) and family, "CH".
pub const CHIMERA: [u8; 3] = [0x7D, 0x43, 0x48];

/// A parameter dump request (#984, ADR 0298): Chimera's own message 1.
const DUMP_REQUEST: u8 = 0x01;
/// A dump's end: message 2.
const DUMP_END: u8 = 0x02;

/// A Part by number on the wire, 0 the first: sixteen fit, however many
/// the box has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartNo(u8);

impl PartNo {
    pub const fn new(n: u8) -> Option<Self> {
        if n < 16 { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Whose values a dump request asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DumpWho {
    /// `00`-`0F`: the Part of that number.
    Part(PartNo),
    /// `10`-`1F`: the Part on that channel.
    Channel(Channel),
    /// `7F`: the Part being edited.
    Editing,
}

impl DumpWho {
    /// The request's `who` byte; `None` for the bytes kept free.
    pub const fn from_wire(b: U7) -> Option<Self> {
        match b.get() {
            n @ 0x00..=0x0F => Some(Self::Part(PartNo(n))),
            n @ 0x10..=0x1F => Some(Self::Channel(Channel::clamped(n & 0x0F))),
            0x7F => Some(Self::Editing),
            _ => None,
        }
    }

    pub const fn wire(self) -> U7 {
        let b = match self {
            Self::Part(p) => p.0,
            Self::Channel(c) => 0x10 | c.get(),
            Self::Editing => 0x7F,
        };
        match U7::new(b) {
            Some(b) => b,
            None => U7::MAX,
        }
    }
}

/// `F0 7D 43 48 01 <who> F7`.
pub fn dump_request(s: SysEx<'_>) -> Option<DumpWho> {
    match *s.bytes() {
        [a, b, c, DUMP_REQUEST, who] if [a, b, c] == CHIMERA => {
            DumpWho::from_wire(U7::new(who)?)
        }
        _ => None,
    }
}

/// `F0 7D 43 48 02 <part> F7`: the dump of `part` is all sent.
pub fn dump_end(part: PartNo, w: &mut Writer) -> bool {
    let [a, b, c] = CHIMERA;
    w.sysex(&[a, b, c, DUMP_END, part.get()])
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

/// A MIDI Tuning Standard message (MMA CA-020/021): the keys it retunes.
/// The tuning program and bank are not kept: Chimera has one MTS table
/// (`chimera_engine::tuning`), which every Part set to MTS plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mts<'a> {
    /// Single-note changes (`08 02`, `08 07`): `kk xx yy zz` groups.
    Notes(&'a [u8]),
    /// A bulk dump (`08 01`, `08 04`): 128 `xx yy zz` groups, key 0 up.
    Dump(&'a [u8]),
    /// Scale/octave (`08 08` a byte a class, `08 09` two): C up.
    Octave { wide: bool, data: &'a [u8] },
}

/// `s` as MTS, if it is a whole one for us (device 7F or `own`).
pub fn mts(s: SysEx<'_>, own: U7) -> Option<Mts<'_>> {
    let b = s.bytes();
    let (&[id, dev, 0x08, sub], rest) = b.split_first_chunk::<4>()? else {
        return None;
    };
    if !(id == 0x7E || id == 0x7F) || !(dev == 0x7F || dev == own.get()) {
        return None;
    }
    // Bytes `skip` in: a count, then that many 4-byte groups.
    let notes = |skip: usize| {
        let (&n, g) = rest.get(skip..)?.split_first()?;
        (g.len() == 4 * usize::from(n)).then_some(Mts::Notes(g))
    };
    match sub {
        0x02 => notes(1),
        0x07 => notes(2),
        0x01 | 0x04 if id == 0x7E => {
            // [bank] program, name, data, checksum: the XOR of every byte
            // before it.
            let head = if sub == 0x04 { 18 } else { 17 };
            let (&cs, body) = rest.split_last()?;
            let data = body.get(head..).filter(|d| d.len() == 3 * 128)?;
            let x = b[..b.len() - 1].iter().fold(0, |a, &x| a ^ x) & 0x7F;
            (x == cs).then_some(Mts::Dump(data))
        }
        0x08 | 0x09 => {
            let wide = sub == 0x09;
            let data = rest
                .get(3..)
                .filter(|d| d.len() == if wide { 24 } else { 12 })?;
            Some(Mts::Octave { wide, data })
        }
        _ => None,
    }
}

impl Mts<'_> {
    /// `f(key, offset)` for each key retuned: its offset from 12-TET in
    /// semitones, the key kept on the MIDI range.
    pub fn each(&self, mut f: impl FnMut(Key, f32)) {
        let mut put = |k: u8, st: f32| {
            let key = Key::clamped(k);
            f(key, within(st, 0.0, 127.0) - f32::from(key.get()));
        };
        // `xx yy zz`: semitone, then 14 bits of one; 7F 7F 7F is no change.
        let abs = |g: &[u8]| {
            (g != [0x7F; 3]).then(|| {
                f32::from(g[0]) + f32::from(u16::from(g[1]) << 7 | u16::from(g[2])) / 16384.0
            })
        };
        match *self {
            Mts::Notes(g) => g.as_chunks::<4>().0.iter().for_each(|g| {
                if let Some(st) = abs(&g[1..]) {
                    put(g[0], st)
                }
            }),
            Mts::Dump(d) => d.as_chunks::<3>().0.iter().enumerate().for_each(|(k, g)| {
                if let Some(st) = abs(g) {
                    put(k as u8, st)
                }
            }),
            Mts::Octave { wide, data } => {
                let cents = |pc: usize| {
                    if wide {
                        let v = u16::from(data[2 * pc]) << 7 | u16::from(data[2 * pc + 1]);
                        (f32::from(v) - 8192.0) * 100.0 / 8192.0
                    } else {
                        f32::from(data[pc]) - 64.0
                    }
                };
                for k in 0..128u8 {
                    put(k, f32::from(k) + cents(usize::from(k % 12)) / 100.0);
                }
            }
        }
    }
}

/// `x` within `lo..=hi` (`clamp` links float formatting).
const fn within(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}
