//! A Part's share of the voice pool (ADR 0281): MAX VOICES, a cap the
//! Part never grows past (at it, a new note steals one of its own), and
//! RESERVE, a floor at or below which another Part's note can't steal its
//! voices. The defaults, AUTO and 0, change nothing.

use crate::hw::{MAX_PARTS, MAX_VOICES};
use crate::project::PartId;

/// A number of voices, 0..=`MAX_VOICES` by construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VoiceCount(u8);

const _: () = assert!(MAX_VOICES <= u8::MAX as usize);

impl VoiceCount {
    pub const ZERO: VoiceCount = VoiceCount(0);
    /// The whole pool.
    pub const POOL: VoiceCount = VoiceCount(MAX_VOICES as u8);

    pub const fn new(n: u8) -> Option<VoiceCount> {
        if n as usize <= MAX_VOICES {
            Some(VoiceCount(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A cap: 1..=`MAX_VOICES` by construction (a Part always gets a voice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VoiceCap(VoiceCount);

impl VoiceCap {
    pub const fn new(n: u8) -> Option<VoiceCap> {
        match VoiceCount::new(n) {
            Some(c) if n > 0 => Some(VoiceCap(c)),
            _ => None,
        }
    }

    pub const fn count(self) -> VoiceCount {
        self.0
    }
}

/// MAX VOICES: none (AUTO, the pool's rules alone) or a cap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaxVoices {
    #[default]
    Auto,
    Cap(VoiceCap),
}

impl MaxVoices {
    /// The cell's list: AUTO, then 1..=8; a choice's code is its place.
    pub const LAST: u8 = MAX_VOICES as u8;

    /// A Part with `live` voices may not take another: steal its own.
    pub const fn reached(self, live: VoiceCount) -> bool {
        match self {
            MaxVoices::Auto => false,
            MaxVoices::Cap(c) => live.0 >= c.0.0,
        }
    }

    /// The disk code and the cell's place: AUTO 0, a cap its count.
    pub const fn code(self) -> u8 {
        match self {
            MaxVoices::Auto => 0,
            MaxVoices::Cap(c) => c.0.0,
        }
    }

    pub const fn from_code(c: u8) -> Option<MaxVoices> {
        if c == 0 {
            return Some(MaxVoices::Auto);
        }
        match VoiceCap::new(c) {
            Some(cap) => Some(MaxVoices::Cap(cap)),
            None => None,
        }
    }
}

/// RESERVE: the voices of a Part no other Part's note may steal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reserve(pub VoiceCount);

impl Reserve {
    pub const NONE: Reserve = Reserve(VoiceCount::ZERO);
    /// The cell's last code: the whole pool.
    pub const LAST: u8 = MAX_VOICES as u8;

    /// A Part with `live` voices is at or below it: guarded. A Part with a
    /// voice counts at least 1, so `NONE` guards nothing.
    pub const fn guards(self, live: VoiceCount) -> bool {
        live.0 <= self.0.0
    }

    pub const fn code(self) -> u8 {
        self.0.0
    }

    pub const fn from_code(c: u8) -> Option<Reserve> {
        match VoiceCount::new(c) {
            Some(n) => Some(Reserve(n)),
            None => None,
        }
    }
}

/// A Part's MAX VOICES and RESERVE.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceLimit {
    pub max: MaxVoices,
    pub reserve: Reserve,
}

impl VoiceLimit {
    /// Today's: no cap, no reserve.
    pub const AUTO: VoiceLimit = VoiceLimit {
        max: MaxVoices::Auto,
        reserve: Reserve::NONE,
    };
}

/// Each Part's limit, as the allocator reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits([VoiceLimit; MAX_PARTS]);

impl Default for Limits {
    fn default() -> Self {
        Limits::AUTO
    }
}

impl Limits {
    pub const AUTO: Limits = Limits([VoiceLimit::AUTO; MAX_PARTS]);

    pub fn of(f: impl Fn(PartId) -> VoiceLimit) -> Limits {
        Limits(PartId::ALL.map(f))
    }

    pub const fn get(&self, p: PartId) -> VoiceLimit {
        self.0[p.index()]
    }

    pub fn set(&mut self, p: PartId, l: VoiceLimit) {
        self.0[p.index()] = l;
    }

    /// Every limit AUTO: the allocator's plain rules (its fast path).
    pub fn is_auto(&self) -> bool {
        *self == Limits::AUTO
    }

    /// The six reserves summed: over `MAX_VOICES` they can't all hold.
    pub fn booked(&self) -> Booked {
        Booked(self.0.iter().map(|l| l.reserve.0.0).sum())
    }
}

/// The Parts' reserves summed, 0..=`MAX_PARTS` × `MAX_VOICES`: the pool
/// holds `MAX_VOICES` of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Booked(u8);

const _: () = assert!(MAX_PARTS * MAX_VOICES <= u8::MAX as usize);

impl Booked {
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Past what the pool holds, the reserves can't all be honoured: by
    /// how many.
    pub const fn excess(self) -> u8 {
        self.0.saturating_sub(MAX_VOICES as u8)
    }
}
