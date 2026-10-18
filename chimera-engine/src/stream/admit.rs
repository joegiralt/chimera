//! Admission (slice 1 § 5): will a new streamed note fit the card's byte
//! budget, and if not, which playing voice goes.

use crate::dsp::algo::math::exp2;

/// Head of a source kept in RAM, in milliseconds.
pub const HEAD_MS: u32 = 100;
/// What admission lets streams ask of the card, bytes a second: bench B4's
/// formula, N × 41,143 × 1.2 × 0.8 at N = 8. B4 ran eight 24 kHz streams
/// at `Ratio::ONE` (329 KB/s) with 0 empty rings, so this sits under what
/// the card was shown to hold. Raising it needs B4 re-run at the new rate.
pub const STREAM_BUDGET_BPS: u32 = 315_978;
/// Worst-case tape wow, in cents.
pub const WOW_CENTS: f32 = 33.0;

/// A voice's card cost, bytes a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bill(pub u32);

/// The bill at the fastest the voice can read: 12-bit packing in 1.5-byte
/// frames, 4608 framed bytes per 4032 of data, scaled by `ratio_max`.
pub fn bill(rate: u16, ratio_max: f32) -> Bill {
    let bps = rate as f32 * 1.5 * 4608.0 / 4032.0 * ratio_max;
    if !bps.is_finite() {
        return Bill(u32::MAX);
    }
    Bill((bps + 0.5) as u32)
}

/// The fastest read ratio a note can reach: its distance from the root,
/// plus the pitch route's reach, the bend range and wow. Not clamped below
/// 1 on purpose: notes below the root read slower.
pub fn ratio_max(note: u8, root_cents: f32, pitch_reach_cents: f32, bend_cents: f32) -> f32 {
    let cents = note as f32 * 100.0 - root_cents + pitch_reach_cents + bend_cents + WOW_CENTS;
    exp2(cents / 1200.0)
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
    Steal(usize),
    Refuse,
}

/// Admit `new` against `budget`. Released voices go before held, the oldest
/// first; a bill over the whole budget refuses outright.
pub fn admit(new: Bill, playing: impl Iterator<Item = (usize, Bill, Age)>, budget: u32) -> Admit {
    if new.0 > budget {
        return Admit::Refuse;
    }
    let mut used = 0u32;
    let mut victim: Option<(usize, Age)> = None;
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
