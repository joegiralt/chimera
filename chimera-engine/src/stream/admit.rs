// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Admission (slice 1 § 5): will a new streamed note fit the card's byte
//! budget, and if not, which playing voice goes.

use chimera_hal::MidiNote;
use chimera_hal::units::BytesPerSec;

use super::layout::{CHUNK_BYTES, RAW_CHUNK_BYTES};
use super::units::{Blocks, Cents, SourceRate};
use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::voice_alloc::{VoiceIdx, VoiceSet};

/// Head of a source kept in RAM, in milliseconds.
pub const HEAD_MS: u32 = 100;
/// What admission lets streams ask of the card, bytes a second: bench B4's
/// formula, N × 41,143 × 1.2 × 0.8 at N = 8: 0.8 of what B4 held. B4 ran
/// eight 24 kHz streams at 1.2× (395 KB/s) for 10 min with 0 empty rings
/// and 0 late blocks (ledger tape-plan/m2/b4-1p2.txt). Raising it needs B4
/// re-run at the new rate.
pub const STREAM_BUDGET_BPS: BytesPerSec = BytesPerSec::new(315_978);
/// Twice B4's load. The unit's 2x run (8 x 24 kHz at 2.4x, 2026-10-20,
/// ledger stress/b4-2p4x-sustain8.txt) streamed clean, but B6's UI freeze
/// rose to 328 ms against its 200 ms (b6-2p4x-ui8.txt): the budget stays
/// until the UI's card waits shrink (#703).
pub const STREAM_BUDGET_2X_BPS: BytesPerSec = BytesPerSec::new(2 * STREAM_BUDGET_BPS.get());
/// Worst-case tape wow.
pub const WOW_CENTS: Cents = Cents::new(33.0);

/// A voice's card cost, bytes a second: only `bill` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bill(u32);

impl Bill {
    /// Nothing on the card.
    pub const ZERO: Bill = Bill(0);

    /// Any bill: for the tests' fakes.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn new(bps: u32) -> Bill {
        Bill(bps)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

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
/// frames, each chunk's data read as its raw blocks, scaled by
/// `ratio_max`. A bill past `u32::MAX` saturates.
pub const fn bill(rate: SourceRate, ratio_max: ReadRatio) -> Bill {
    let bps = rate.get() as f32 * 1.5 * RAW_CHUNK_BYTES as f32 / CHUNK_BYTES.get() as f32
        * ratio_max.get();
    Bill((bps + 0.5) as u32)
}

/// The fastest read ratio a note can reach: its distance from the root,
/// plus the pitch route's reach and the bend (wow is the caller's: in
/// `reach` when TAPE runs). Not clamped below 1 on purpose: notes below
/// the root read slower.
pub fn ratio_max(note: MidiNote, root: Cents, reach: Cents, bend: Cents) -> ReadRatio {
    let cents = note.get() as f32 * 100.0 - root.get() + reach.get() + bend.get();
    ReadRatio::new(exp2(cents / 1200.0))
}

/// How long a playing voice has been so: `since` its start while held, its
/// release once released. Bigger is older. `level`: a held voice's VCA
/// gain last block, in 65 535ths (`LOUD` for a fresh note); `swell`
/// whether it is still in its attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    Held {
        since: Blocks,
        level: u16,
        swell: Swell,
    },
    Released {
        since: Blocks,
    },
}

/// Where a held note's VCA is: still rising from its strike, or past it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swell {
    /// Struck under `ATTACK_BLOCKS` ago, or its gain has risen every block
    /// since (a slow attack): it ranks loud.
    Attack,
    /// Its gain has stopped rising once: sustain, decay, or a plateau.
    Past,
}

/// A fresh note's `Age::Held` level: full.
pub const LOUD: u16 = u16::MAX;
/// A held note this young (50 ms at 48 kHz) is in its attack, whatever its
/// gain does: a chord struck over a few ms keeps its notes.
pub const ATTACK_BLOCKS: Blocks = Blocks::new(38);

impl Age {
    /// A note just struck.
    pub const FRESH: Age = Age::Held {
        since: Blocks::ZERO,
        level: LOUD,
        swell: Swell::Attack,
    };
    /// Shed or cut: older than any other.
    pub const DYING: Age = Age::Released { since: Blocks::MAX };

    pub const fn released(self) -> bool {
        matches!(self, Age::Released { .. })
    }

    /// A block on: older while still held or still released, else new to
    /// its state; `level` the VCA's now. A held note stays in its attack
    /// for `ATTACK_BLOCKS`, then while its gain still rises.
    pub fn after_block(self, held: bool, level: u16) -> Age {
        match self {
            Age::Held {
                since,
                level: was,
                swell,
            } if held => {
                let since = since.next();
                let rising = swell == Swell::Attack && level > was;
                let swell = if since < ATTACK_BLOCKS || rising {
                    Swell::Attack
                } else {
                    Swell::Past
                };
                Age::Held {
                    since,
                    level,
                    swell,
                }
            }
            Age::Released { since } if !held => Age::Released {
                since: since.next(),
            },
            _ if held => Age::Held {
                since: Blocks::ZERO,
                level,
                swell: Swell::Attack,
            },
            _ => Age::Released {
                since: Blocks::ZERO,
            },
        }
    }

    /// Which voice a steal takes first, the bigger (ADR 0183): a released
    /// one before a held one, the oldest; of held ones the quietest (the
    /// most decayed; one in its attack counts as loud). Among equals one
    /// past its attack goes before one in it, the youngest of them: the
    /// oldest is usually the chord's root (#698), and a slow pad's newest
    /// notes, still swelling, are kept. Of notes all in their attack, the
    /// oldest goes.
    pub(crate) fn rank(self) -> (bool, u32, bool, u32) {
        match self {
            Age::Released { since } => (true, since.get(), false, 0),
            Age::Held {
                since,
                level,
                swell: Swell::Past,
            } => (false, u32::from(LOUD - level), true, u32::MAX - since.get()),
            Age::Held { since, .. } => (false, 0, false, since.get()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Ok,
    /// Stop this voice, then ask again: a steal frees one voice at a time.
    Steal(VoiceIdx),
    Refuse,
}

/// Admit `new` against `budget`. Released voices go before held, the oldest
/// first, then the quietest held (`Age::rank`); a bill over the whole
/// budget refuses outright. Only a voice in `stealable` may go: every voice
/// but another Part's guarded ones (`Allocator::card_stealable`, ADR 0281);
/// with none of them left to free room, refused.
pub fn admit(
    new: Bill,
    playing: impl Iterator<Item = (VoiceIdx, Bill, Age)>,
    budget: BytesPerSec,
    stealable: VoiceSet,
) -> Admit {
    let budget = budget.get();
    if new.0 > budget {
        return Admit::Refuse;
    }
    let mut used = 0u32;
    let mut victim: Option<(VoiceIdx, Age)> = None;
    for (i, b, age) in playing {
        used = used.saturating_add(b.0);
        let older = stealable.contains(i) && victim.is_none_or(|(_, v)| age.rank() > v.rank());
        if older {
            victim = Some((i, age));
        }
    }
    if used.saturating_add(new.0) <= budget {
        return Admit::Ok;
    }
    victim.map_or(Admit::Refuse, |(i, _)| Admit::Steal(i))
}

/// Admits `new` onto a voice whose ring carries `on` already (a fade
/// queued ahead of it, or nothing): one ring serves one stream at a time,
/// so the two bill the larger. `others` is every other voice's bill, a
/// dying one's too: a fade streams until its voice is freed (#764).
/// `Steal(w)`: `w`, a live voice in `stealable` (never a dying one), the
/// first `Age::rank` gives, takes the note behind its own fade, if that
/// makes room; else `Refuse`, nothing stolen: a second victim's bill
/// would stay until its fade ended, freeing nothing in time.
pub fn admit_on(
    new: Bill,
    on: Bill,
    others: impl Iterator<Item = (VoiceIdx, Bill, Age)>,
    budget: BytesPerSec,
    stealable: VoiceSet,
) -> Admit {
    let budget = budget.get();
    if new.0 > budget {
        return Admit::Refuse;
    }
    let mut used = 0u32;
    let mut victim: Option<(VoiceIdx, Bill, Age)> = None;
    for (i, b, age) in others {
        used = used.saturating_add(b.0);
        if stealable.contains(i) && victim.is_none_or(|(_, _, v)| age.rank() > v.rank()) {
            victim = Some((i, b, age));
        }
    }
    if used.saturating_add(new.0.max(on.0)) <= budget {
        return Admit::Ok;
    }
    match victim {
        Some((w, b, _))
            if used
                .saturating_add(on.0)
                .saturating_sub(b.0)
                .saturating_add(new.0.max(b.0))
                <= budget =>
        {
            Admit::Steal(w)
        }
        _ => Admit::Refuse,
    }
}

/// What a mid-note raise to `new` does (#595, #764).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raise {
    /// It fits beside every other bill, a dying voice's too.
    Fits,
    /// Not yet: these released victims go, and once their fades end a
    /// later block admits it; none, if only the fades in flight are over.
    Wait(VoiceSet),
    /// Even every released victim wouldn't make room: nothing goes.
    Hold,
}

/// A raise to `new` beside `others` (every other voice's bill, its age and
/// whether it is dying): a dying voice's bill counts until it is freed,
/// and only released voices in `stealable` (never a dying one) go, the
/// first `Age::rank` gives, while the live voices leave no room.
pub fn raise_room(
    new: Bill,
    others: impl Iterator<Item = (VoiceIdx, Bill, Age, bool)> + Clone,
    budget: BytesPerSec,
    stealable: VoiceSet,
) -> Raise {
    let budget = budget.get();
    let (mut live, mut all) = (0u32, 0u32);
    for (_, b, _, dying) in others.clone() {
        all = all.saturating_add(b.0);
        if !dying {
            live = live.saturating_add(b.0);
        }
    }
    if all.saturating_add(new.0) <= budget {
        return Raise::Fits;
    }
    let victim = |(i, _, age, dying): &(VoiceIdx, Bill, Age, bool)| {
        !dying && age.released() && stealable.contains(*i)
    };
    let mut gone = VoiceSet::EMPTY;
    while live.saturating_add(new.0) > budget {
        let Some((i, b, _, _)) = others
            .clone()
            .filter(|o| victim(o) && !gone.contains(o.0))
            .max_by_key(|o| o.2.rank())
        else {
            return Raise::Hold;
        };
        live = live.saturating_sub(b.0);
        gone = gone.with(i);
    }
    Raise::Wait(gone)
}

/// Streamed notes at ROOT a source of `rate` fits in the budget, `wow`
/// billed when TAPE runs: SOURCE's band says it before a chord steals.
pub fn notes_at_root(rate: SourceRate, wow: bool) -> u32 {
    let cents = if wow { WOW_CENTS } else { Cents::ZERO };
    STREAM_BUDGET_BPS.get()
        / bill(rate, ReadRatio::new(exp2(cents.get() / 1200.0)))
            .0
            .max(1)
}

/// `HEAD_MS` of frames, rounded up to even.
pub fn head_frames(rate: SourceRate) -> u32 {
    (u32::from(rate) * HEAD_MS)
        .div_ceil(1000)
        .next_multiple_of(2)
}
