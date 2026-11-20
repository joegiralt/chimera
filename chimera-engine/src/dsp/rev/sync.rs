//! Tempo sync (reverb TYPEs spec § Tempo sync): the tempo ORBIT's clock
//! hands the reverb, the note values a synced knob walks, and the fold
//! that keeps a synced time inside its TYPE's range.

use core::fmt::Write;

use crate::dsp::clampf;

/// The tempo, in BPM: free of any sample rate, which the engine applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo(pub f32);

impl Tempo {
    pub const DEFAULT: Tempo = Tempo(120.0);

    /// `bpm` held to ORBIT's clock range (`Bpm::MIN` to `MAX_BPM_EXT`);
    /// NaN is the default.
    pub fn from_bpm(bpm: f32) -> Tempo {
        if bpm.is_nan() {
            return Self::DEFAULT;
        }
        Tempo(clampf(bpm, 30.0, 1_200.0))
    }

    /// A beat of `samples` at `sample_rate`, on a 0.25 BPM grid. Its jitter
    /// is held upstream, in samples (`MidiClock::beat`).
    pub fn from_beat(samples: u32, sample_rate: u32) -> Tempo {
        let bpm = 60.0 * sample_rate as f32 / samples.max(1) as f32;
        Self::from_bpm(libm::roundf(bpm * 4.0) / 4.0)
    }

    /// One beat, in ms.
    pub fn beat_ms(self) -> f32 {
        60_000.0 / self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivKind {
    Straight,
    Triplet,
    Dotted,
}

/// A note value, `num/den` of a whole note; `num` 0 is no time.
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

const ZERO: Division = Division {
    num: 0,
    den: 1,
    kind: S,
};

/// Each synced knob's divisions: those inside its ms range at 120 BPM.
pub const ROOM_PRE: &[Division] = &[ZERO, d(128, S), d(64, T), d(64, S), d(32, T)];
/// HALL's and BLOOM's PRE-DLY.
pub const HALL_PRE: &[Division] = &[
    ZERO,
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
/// HALL's and BLOOM's DECAY under ALL, in beats.
pub const DECAY_BEATS: &[u8] = &[1, 2, 3, 4, 6, 8, 12, 16, 24, 32];

impl Division {
    /// Its length at `t`, ms.
    pub fn ms(self, t: Tempo) -> f32 {
        let k = match self.kind {
            DivKind::Straight => 1.0,
            DivKind::Triplet => 2.0 / 3.0,
            DivKind::Dotted => 1.5,
        };
        4.0 * t.beat_ms() * self.num as f32 / self.den as f32 * k
    }

    /// Its text halved `fold` times (doubled, negative), as it plays,
    /// marked ▾ or ▴ (ADR 0086's marks): `1/8`, `▾1/16T`, `0`. Past 1/64
    /// a triplet or dotted note is its plain fraction (`▾1/192` for
    /// 1/128T, `3/256` for 1/128D): six glyphs fill the focus band.
    pub fn write(self, fold: FoldState, b: &mut impl Write) {
        if self.num == 0 {
            let _ = b.write_str("0");
            return;
        }
        let (mut num, mut den) = (self.num as u32, self.den as u32);
        let n = fold.0.unsigned_abs().min(8) as u32;
        if fold.0 > 0 {
            den <<= n;
        } else {
            num <<= n;
        }
        let mut kind = match self.kind {
            DivKind::Straight => "",
            DivKind::Triplet => "T",
            DivKind::Dotted => "D",
        };
        if den > 64 {
            (num, den) = match self.kind {
                DivKind::Straight => (num, den),
                DivKind::Triplet => (2 * num, 3 * den),
                DivKind::Dotted => (3 * num, 2 * den),
            };
            kind = "";
        }
        while num % 2 == 0 && den % 2 == 0 {
            (num, den) = (num / 2, den / 2);
        }
        let mark = match fold.0 {
            0 => "",
            f if f > 0 => "▾",
            _ => "▴",
        };
        let _ = write!(b, "{mark}{num}/{den}{kind}");
    }
}

/// Entry `round(p·(n − 1))` of a list of `n` (spec: how a division is
/// stored); NaN reads 0.
#[inline(never)]
pub fn index_of(p: f32, n: usize) -> usize {
    let last = n.saturating_sub(1) as f32;
    libm::roundf(clampf(p, 0.0, 1.0) * last) as usize
}

/// The stored value of entry `i` of `n`.
pub fn p_of(i: usize, n: usize) -> f32 {
    if n < 2 {
        0.0
    } else {
        i.min(n - 1) as f32 / (n - 1) as f32
    }
}

/// How many times a synced time is halved (positive) or doubled
/// (negative) to fit. Zero bytes are none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FoldState(pub i8);

/// The fold's hysteresis: a state changes only once the time is this far
/// past a limit, out or in (the delay's `fit`: in only).
pub const HYST: f32 = 1.02;

/// `len`, ms, halved or doubled into `lo..=hi`, staying on the grid; `st`
/// moves only when the time is 2 % past a limit, so clock jitter at a
/// boundary never flips it. Inside that 2 % the time is held to the limit.
pub fn fold(len: f32, lo: f32, hi: f32, st: &mut FoldState) -> f32 {
    let at = |n: i8| libm::ldexpf(len, -(n as i32));
    let mut n = st.0;
    if len > 0.0 && len.is_finite() {
        // Each pass moves one way only, so a dozen settle any tempo.
        for _ in 0..16 {
            // Out 2 % past a limit, or back in 2 % inside the other.
            let halve = at(n) > hi * HYST || n < 0 && at(n + 1) >= lo * HYST;
            let double = at(n) < lo / HYST || n > 0 && at(n - 1) <= hi / HYST;
            n = match (halve, double) {
                (true, _) => n + 1,
                (_, true) => n - 1,
                _ => break,
            };
        }
    } else {
        n = 0;
    }
    st.0 = n;
    clampf(at(n), lo, hi)
}
