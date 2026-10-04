//! Tempo for the synced TYPEs (reverb TYPEs spec § Tempo sync): the
//! note values a synced knob walks, and the fold that keeps one in range.

use crate::dsp::clampf;

/// The beat's length, in samples at `Tempo::RATE`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo(pub f32);

impl Tempo {
    /// The rate a `Tempo` counts at.
    pub const RATE: f32 = 48_000.0;
    /// 120 BPM at 48 kHz.
    pub const DEFAULT: Tempo = Tempo(24_000.0);

    /// One beat, in ms.
    pub fn beat_ms(self) -> f32 {
        self.0 * 1000.0 / Self::RATE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivKind {
    Straight,
    /// Two thirds of straight.
    Triplet,
    /// Half again.
    Dotted,
}

/// A note value, `num/den` of a whole note (0/1 is none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Division {
    pub num: u8,
    pub den: u8,
    pub kind: DivKind,
}

const fn d(den: u8, kind: DivKind) -> Division {
    Division { num: 1, den, kind }
}

use DivKind::{Dotted as D, Straight as S, Triplet as T};

const NONE: Division = Division {
    num: 0,
    den: 1,
    kind: S,
};

impl Division {
    /// Its length at `tempo`, in ms.
    pub fn ms(self, tempo: Tempo) -> f32 {
        let beats = 4.0 * self.num as f32 / self.den as f32;
        let k = match self.kind {
            DivKind::Straight => 1.0,
            DivKind::Triplet => 2.0 / 3.0,
            DivKind::Dotted => 1.5,
        };
        beats * k * tempo.beat_ms()
    }
}

impl Division {
    /// This value halved `n` times (doubled for negative `n`): what a
    /// fold plays, still on the grid.
    pub fn folded(self, n: i8) -> Division {
        let (mut num, mut den) = (self.num as u32, self.den as u32);
        if n >= 0 {
            den <<= n.min(7);
        } else {
            num <<= (-(n as i32)).min(7);
        }
        while num % 2 == 0 && den % 2 == 0 && num > 0 {
            (num, den) = (num / 2, den / 2);
        }
        Division {
            num: num.min(255) as u8,
            den: den.min(255) as u8,
            kind: self.kind,
        }
    }
}

impl core::fmt::Display for Division {
    /// `1/8`, `1/8T`, `1/8D`; `0` for none.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.num == 0 {
            return f.write_str("0");
        }
        let mark = match self.kind {
            DivKind::Straight => "",
            DivKind::Triplet => "T",
            DivKind::Dotted => "D",
        };
        write!(f, "{}/{}{mark}", self.num, self.den)
    }
}

/// Each list is the divisions inside its knob's range at 120 BPM.
pub const ROOM_PRE: &[Division] = &[NONE, d(128, S), d(64, T), d(64, S), d(32, T)];
pub const HALL_PRE: &[Division] = &[
    NONE,
    d(128, S),
    d(64, T),
    d(64, S),
    d(32, T),
    d(32, S),
    d(16, T),
    d(32, D),
    d(16, S),
];
pub const GATE_LENGTH: &[Division] = &[
    d(32, T),
    d(32, S),
    d(16, T),
    d(32, D),
    d(16, S),
    d(8, T),
    d(16, D),
    d(8, S),
    d(4, T),
    d(8, D),
    d(4, S),
];
pub const REVERSE_LENGTH: &[Division] = &[
    d(8, T),
    d(16, D),
    d(8, S),
    d(4, T),
    d(8, D),
    d(4, S),
    d(2, T),
    d(4, D),
];
/// HALL's and BLOOM's DECAY under SYNC ALL.
pub const DECAY_BEATS: &[u8] = &[1, 2, 3, 4, 6, 8, 12, 16, 24, 32];

/// Entry `i` of an `n` list is stored as `i/(n − 1)`.
pub fn list_index(p: f32, n: usize) -> usize {
    let top = n.saturating_sub(1) as f32;
    (clampf(p, 0.0, 1.0) * top + 0.5) as usize
}

/// Halvings (+) or doublings (−) a synced time plays at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FoldState(pub i8);

/// How far past a limit a time must go to change the fold.
const HYST: f32 = 0.02;

/// `len` halved or doubled until it fits `lo..=hi`, on the grid. The
/// fold changes only once `len` is 2 % past a limit, out or back in, so
/// clock jitter at a limit cannot flap it.
#[inline(never)]
pub fn fold(len: f32, lo: f32, hi: f32, st: &mut FoldState) -> f32 {
    if !len.is_finite() || len <= 0.0 {
        return lo;
    }
    // 2^−n exactly, from the exponent bits.
    let at = |n: i8| len * f32::from_bits(((127 - n.clamp(-100, 100) as i32) as u32) << 23);
    // Bounded: a range spans at least an octave, and i8 holds any tempo.
    for _ in 0..16 {
        let n = st.0;
        let v = at(n);
        st.0 = if v > hi * (1.0 + HYST) {
            n.saturating_add(1)
        } else if v < lo * (1.0 - HYST) {
            n.saturating_sub(1)
        } else if n > 0 && 2.0 * v < hi * (1.0 - HYST) {
            n - 1
        } else if n < 0 && 0.5 * v > lo * (1.0 + HYST) {
            n + 1
        } else {
            break;
        };
    }
    clampf(at(st.0), lo, hi)
}
