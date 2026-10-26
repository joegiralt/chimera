//! Learn senses the kind of control from what it sends (spec § 6.3).

use crate::ctl::{Dir, FreeCc};
use crate::rel::ByteEncoding;
use crate::value::U7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sure {
    Yes,
    /// Only one direction seen: keep asking for the left turn.
    No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guess {
    Absolute,
    Relative(ByteEncoding, Sure),
    Button,
    /// CC n then CC n+32, n in 1..=31.
    Paired14,
}

/// Which control a sample came from. Learn restarts on a different control,
/// so this only has to tell a 14-bit pair's halves and NRPN data from steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeenCtl {
    Cc(FreeCc),
    /// NRPN data entry, coarse (CC 6); the sample is the coarse value.
    Data,
    /// NRPN data entry, fine (CC 38); the sample is the MSB.
    DataFine,
    /// CC 96 / 97; the sample value means nothing.
    Step(Dir),
    Pressure,
}

/// Up to `CAP` samples in arrival order; no heap.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    buf: [(SeenCtl, U7); Seen::CAP],
    len: u8,
}

impl Default for Seen {
    fn default() -> Self {
        Self::new()
    }
}

impl Seen {
    pub const CAP: usize = 24;

    pub const fn new() -> Self {
        Self {
            buf: [(SeenCtl::Pressure, U7::ZERO); Self::CAP],
            len: 0,
        }
    }

    /// False once full: the sample is dropped.
    pub fn push(&mut self, control: SeenCtl, v: U7) -> bool {
        let Some(slot) = self.buf.get_mut(self.len as usize) else {
            return false;
        };
        *slot = (control, v);
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn samples(&self) -> &[(SeenCtl, U7)] {
        &self.buf[..self.len as usize]
    }

    /// An NRPN sent a fine value (CC 38): a 14-bit control, so learn turns echo on.
    pub fn fine_seen(&self) -> bool {
        self.samples().iter().any(|&(c, _)| c == SeenCtl::DataFine)
    }

    /// The control is an NRPN increment/decrement: the caller learns it as
    /// `IncDec`; `classify` has no word for it.
    pub fn stepped(&self) -> bool {
        matches!(self.samples().first(), Some((SeenCtl::Step(_), _)))
    }
}

/// Pure guess from the samples. A stepped `Seen` (see `Seen::stepped`) and
/// an NRPN that sent fine values read as `Absolute`.
pub fn classify(seen: &Seen) -> Guess {
    let samples = seen.samples();
    let Some(&(first, _)) = samples.first() else {
        return Guess::Absolute;
    };
    if matches!(first, SeenCtl::Cc(n) if paired14(n, samples)) {
        return Guess::Paired14;
    }
    if seen.stepped() || seen.fine_seen() {
        return Guess::Absolute;
    }
    let mut vals = [0u8; Seen::CAP];
    let mut len = 0;
    for &(_, v) in samples.iter().filter(|(c, _)| *c == first) {
        vals[len] = v.get();
        len += 1;
    }
    let vals = &vals[..len];
    if is_button(vals) {
        Guess::Button
    } else {
        relative(vals).unwrap_or(Guess::Absolute)
    }
}

/// CC n followed by CC n+32 on more than half of the CC n messages.
fn paired14(n: FreeCc, samples: &[(SeenCtl, U7)]) -> bool {
    if !(1..=31).contains(&n.get()) {
        return false;
    }
    let Some(lsb) = FreeCc::new(n.get() + 32) else {
        return false;
    };
    let msbs = samples.iter().filter(|(c, _)| *c == SeenCtl::Cc(n)).count();
    let paired = samples
        .windows(2)
        .filter(|w| w[0].0 == SeenCtl::Cc(n) && w[1].0 == SeenCtl::Cc(lsb))
        .count();
    paired * 2 > msbs
}

/// Only 0 and 127, or two values more than 64 apart that alternate
/// (a relative encoder repeats its value; a button flips).
fn is_button(vals: &[u8]) -> bool {
    if vals.is_empty() {
        return false;
    }
    if vals.iter().all(|&v| v == 0 || v == 127) {
        return true;
    }
    let (lo, hi) = vals
        .iter()
        .fold((127, 0), |(lo, hi), &v| (v.min(lo), v.max(hi)));
    let two = vals.iter().all(|&v| v == lo || v == hi);
    two && hi - lo > 64 && !has_repeat(vals)
}

fn has_repeat(vals: &[u8]) -> bool {
    vals.windows(2).any(|w| w[0] == w[1])
}

/// Relative: values repeat and sit near 0/127 or 64. The first sample's
/// quarter of the byte range is clockwise; every other sample is the left turn.
fn relative(vals: &[u8]) -> Option<Guess> {
    let near = |v: u8| v <= 24 || v >= 103 || v.abs_diff(64) <= 24;
    if !has_repeat(vals) || !vals.iter().all(|&v| near(v)) {
        return None;
    }
    let quarter = |v: u8| v / 32;
    let cw = quarter(vals[0]);
    if vals.iter().all(|&v| quarter(v) == cw) {
        // one direction: the more common encoding of each byte family
        let e = if cw == 0 || cw == 3 {
            ByteEncoding::TwosComplement
        } else {
            ByteEncoding::BinaryOffset
        };
        return Some(Guess::Relative(e, Sure::No));
    }
    ByteEncoding::ALL
        .into_iter()
        .filter_map(|e| {
            let mut total = 0u32;
            for &v in vals {
                let d = e.decode(U7::new(v)?);
                if (quarter(v) == cw) != (d > 0) || d == 0 {
                    return None;
                }
                total += d.unsigned_abs() as u32;
            }
            Some((total, e))
        })
        .min_by_key(|&(total, _)| total)
        .map(|(_, e)| Guess::Relative(e, Sure::Yes))
}
