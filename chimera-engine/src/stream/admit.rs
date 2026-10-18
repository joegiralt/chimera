//! Admission (slice 1 § 5): will a new streamed note fit the card's byte
//! budget, and if not, which playing voice goes.

use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::voice_alloc::VoiceIdx;

/// Head of a source kept in RAM, in milliseconds.
pub const HEAD_MS: u32 = 100;
/// What admission lets streams ask of the card, bytes a second: bench B4's
/// formula, N × 41,143 × 1.2 × 0.8 at N = 8: 0.8 of what B4 held. B4 ran
/// eight 24 kHz streams at 1.2× (395 KB/s) for 10 min with 0 empty rings
/// and 0 late blocks (ledger tape-plan/m2/b4-1p2.txt). Raising it needs B4
/// re-run at the new rate.
pub const STREAM_BUDGET_BPS: u32 = 315_978;
/// Worst-case tape wow, in cents.
pub const WOW_CENTS: f32 = 33.0;

/// A voice's card cost, bytes a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bill(pub u32);

/// A read ratio: source frames per frame at the source's own rate. Finite
/// and at least 0: a NaN or infinite one is the largest, so it bills the
/// most and is refused.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ReadRatio(f32);

impl ReadRatio {
    pub const ONE: ReadRatio = ReadRatio(1.0);

    pub fn new(r: f32) -> ReadRatio {
        match r {
            r if r.is_nan() => ReadRatio(f32::MAX),
            r => ReadRatio(clampf(r, 0.0, f32::MAX)),
        }
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// The bill at the fastest the voice can read: 12-bit packing in 1.5-byte
/// frames, 4608 framed bytes per 4032 of data, scaled by `ratio_max`. A
/// bill past `u32::MAX` saturates.
pub fn bill(rate: u16, ratio_max: ReadRatio) -> Bill {
    let bps = rate as f32 * 1.5 * 4608.0 / 4032.0 * ratio_max.get();
    Bill((bps + 0.5) as u32)
}

/// The fastest read ratio a note can reach: its distance from the root,
/// plus the pitch route's reach, the bend range and wow. Not clamped below
/// 1 on purpose: notes below the root read slower.
pub fn ratio_max(note: u8, root_cents: f32, pitch_reach_cents: f32, bend_cents: f32) -> ReadRatio {
    let cents = note as f32 * 100.0 - root_cents + pitch_reach_cents + bend_cents + WOW_CENTS;
    ReadRatio::new(exp2(cents / 1200.0))
}

/// How long a playing voice has been so: `since` counts blocks from its
/// release when `released`, from its start when held. Bigger is older.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Age {
    pub released: bool,
    pub since: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Ok,
    /// Stop this voice, then ask again: a steal frees one voice at a time.
    Steal(VoiceIdx),
    Refuse,
}

/// Admit `new` against `budget`. Released voices go before held, the oldest
/// first; a bill over the whole budget refuses outright.
pub fn admit(
    new: Bill,
    playing: impl Iterator<Item = (VoiceIdx, Bill, Age)>,
    budget: u32,
) -> Admit {
    if new.0 > budget {
        return Admit::Refuse;
    }
    let mut used = 0u32;
    let mut victim: Option<(VoiceIdx, Age)> = None;
    for (i, b, age) in playing {
        used = used.saturating_add(b.0);
        let older = victim.is_none_or(|(_, v)| (age.released, age.since) > (v.released, v.since));
        if older {
            victim = Some((i, age));
        }
    }
    if used.saturating_add(new.0) <= budget {
        return Admit::Ok;
    }
    victim.map_or(Admit::Refuse, |(i, _)| Admit::Steal(i))
}

/// `HEAD_MS` of frames, rounded up to even.
pub fn head_frames(rate: u16) -> u32 {
    (rate as u32 * HEAD_MS).div_ceil(1000).next_multiple_of(2)
}
