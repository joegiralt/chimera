//! A Part's share of the voice pool (ADR 0281): MAX VOICES, a cap the
//! Part never grows past (at it, a new note steals one of its own), and
//! RESERVE, a floor at or below which another Part's note can't steal its
//! voices. The defaults, AUTO and 0, change nothing.
//!
//! The rule an edit must keep (`Shares::refusal`) lives here, once: a
//! Part's reserve within its own cap (`VoiceLimit`), the six reserves
//! within the pool (`Limits`). A project saved past it (an old or
//! hand-edited file) loads as it is, an explicit second type (`Over`),
//! plays as saved and may only come down.

use crate::hw::{MAX_PARTS, MAX_VOICES};
use crate::project::PartId;

/// A number of voices, 0..=`POOL` by construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VoiceCount(u8);

const _: () = assert!(MAX_VOICES < u8::MAX as usize);
// A `VoiceSet` is a `u8`, a bit a voice: its count never passes the pool's.
const _: () = assert!(u8::BITS as usize == MAX_VOICES);

impl VoiceCount {
    pub const ZERO: VoiceCount = VoiceCount(0);
    /// The whole pool.
    pub const POOL: VoiceCount = VoiceCount(MAX_VOICES as u8);
    /// How many counts there are, 0 to `POOL`: each cell's list length.
    pub const VALUES: u8 = Self::POOL.0 + 1;

    pub const fn new(n: u8) -> Option<VoiceCount> {
        if n <= Self::POOL.0 {
            Some(VoiceCount(n))
        } else {
            None
        }
    }

    /// `n`, held to the pool.
    pub const fn saturating(n: u8) -> VoiceCount {
        if n <= Self::POOL.0 {
            VoiceCount(n)
        } else {
            Self::POOL
        }
    }

    /// A cell's stored value: truncated, then held to the pool (NaN and
    /// below 0 are 0).
    pub fn from_f32(v: f32) -> VoiceCount {
        // `as` saturates: NaN and negatives are 0, past 255 is 255.
        Self::saturating(v as u8)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// The bits set in `mask`, a set of the pool's voices (`VoiceSet`): at
    /// most its 8 bits, which are the pool's, so a count by construction.
    pub const fn of_pool_bits(mask: u8) -> VoiceCount {
        VoiceCount(mask.count_ones() as u8)
    }

    /// `delta` along 0..=`POOL`, saturating at both ends.
    pub const fn stepped(self, delta: i8) -> VoiceCount {
        Self::saturating(self.0.saturating_add_signed(delta))
    }
}

/// A cap: 1..=`POOL` by construction (a Part always gets a voice).
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

/// A list's end: where MIX + turn runs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Low,
    High,
}

impl Edge {
    /// The end a turn of `delta` runs to.
    pub const fn of(delta: i8) -> Edge {
        if delta > 0 { Edge::High } else { Edge::Low }
    }
}

/// MAX VOICES: none (AUTO, the pool's rules alone) or a cap. Its cell
/// lists AUTO, then 1 to 8: a choice's place is its code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaxVoices {
    #[default]
    Auto,
    Cap(VoiceCap),
}

impl MaxVoices {
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

    /// Place `c` in the list (0 AUTO, else a cap of `c`): total, as every
    /// count is a place.
    pub const fn at(c: VoiceCount) -> MaxVoices {
        match VoiceCap::new(c.0) {
            Some(cap) => MaxVoices::Cap(cap),
            None => MaxVoices::Auto,
        }
    }

    /// A cell's stored value, held to the list.
    pub fn from_f32(v: f32) -> MaxVoices {
        Self::at(VoiceCount::from_f32(v))
    }

    /// `delta` along the list, saturating.
    pub const fn stepped(self, delta: i8) -> MaxVoices {
        Self::at(VoiceCount(self.code()).stepped(delta))
    }

    /// The list's end `e`: AUTO or a cap of the pool.
    pub const fn ended(e: Edge) -> MaxVoices {
        match e {
            Edge::Low => MaxVoices::Auto,
            Edge::High => MaxVoices::Cap(VoiceCap(VoiceCount::POOL)),
        }
    }
}

/// RESERVE: the voices of a Part no other Part's note may steal. Its cell
/// lists 0 to 8.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reserve(VoiceCount);

impl Reserve {
    pub const NONE: Reserve = Reserve(VoiceCount::ZERO);

    pub const fn new(n: VoiceCount) -> Reserve {
        Reserve(n)
    }

    pub const fn count(self) -> VoiceCount {
        self.0
    }

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

    /// A cell's stored value, held to the list.
    pub fn from_f32(v: f32) -> Reserve {
        Reserve(VoiceCount::from_f32(v))
    }

    pub const fn stepped(self, delta: i8) -> Reserve {
        Reserve(self.0.stepped(delta))
    }

    pub const fn ended(e: Edge) -> Reserve {
        match e {
            Edge::Low => Reserve::NONE,
            Edge::High => Reserve(VoiceCount::POOL),
        }
    }
}

/// A reserve over its own Part's cap: it can't be honoured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverCap {
    reserve: Reserve,
    cap: VoiceCap,
}

impl OverCap {
    pub const fn reserve(self) -> Reserve {
        self.reserve
    }

    pub const fn cap(self) -> VoiceCap {
        self.cap
    }
}

/// A Part's two cells as set or saved, lawful or not: what the Part block
/// holds, and what an old project may load past the rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub max: MaxVoices,
    pub reserve: Reserve,
}

impl Asked {
    pub const AUTO: Asked = Asked {
        max: MaxVoices::Auto,
        reserve: Reserve::NONE,
    };

    /// The pair, if its reserve is within its cap.
    pub const fn lawful(self) -> Result<VoiceLimit, OverCap> {
        VoiceLimit::new(self.max, self.reserve)
    }

    /// By how many voices the reserve passes the cap: none when lawful.
    pub const fn over_cap(self) -> VoiceCount {
        match self.max {
            // Both are counts: their difference is one.
            MaxVoices::Cap(c) => VoiceCount(self.reserve.code().saturating_sub(c.0.0)),
            MaxVoices::Auto => VoiceCount::ZERO,
        }
    }
}

/// A Part's MAX VOICES and RESERVE, its reserve within its cap by
/// construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceLimit {
    max: MaxVoices,
    reserve: Reserve,
}

impl VoiceLimit {
    /// Today's: no cap, no reserve.
    pub const AUTO: VoiceLimit = VoiceLimit {
        max: MaxVoices::Auto,
        reserve: Reserve::NONE,
    };

    pub const fn new(max: MaxVoices, reserve: Reserve) -> Result<VoiceLimit, OverCap> {
        match max {
            MaxVoices::Cap(cap) if reserve.0.0 > cap.0.0 => Err(OverCap { reserve, cap }),
            _ => Ok(VoiceLimit { max, reserve }),
        }
    }

    pub const fn max(self) -> MaxVoices {
        self.max
    }

    pub const fn reserve(self) -> Reserve {
        self.reserve
    }

    pub const fn asked(self) -> Asked {
        Asked {
            max: self.max,
            reserve: self.reserve,
        }
    }
}

/// The six Parts' reserves summed, past the pool or not: 0..=48
/// (`MAX_PARTS` × `POOL`) by construction, as it sums exactly one
/// `Reserve` (0..=`POOL`) a Part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reserved(u8);

/// Six Parts' worth of `POOL` fits the sum's `u8`.
const _: () = assert!(MAX_PARTS * MAX_VOICES <= u8::MAX as usize);

impl Reserved {
    pub const NONE: Reserved = Reserved(0);

    /// Each Part's reserve `r` summed: at most `MAX_PARTS` × `POOL`
    /// (asserted above), so
    /// no overflow.
    pub const fn of(r: &[Reserve; MAX_PARTS]) -> Reserved {
        let (mut sum, mut i) = (0u8, 0);
        while i < MAX_PARTS {
            sum += r[i].0.0;
            i += 1;
        }
        Reserved(sum)
    }

    /// The sum in voices, 0..=48: for its words.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Within the pool, or past it.
    pub const fn fits(self) -> Result<Booked, Overbooked> {
        match VoiceCount::new(self.0) {
            Some(n) => Ok(Booked(n)),
            None => Err(Overbooked(self)),
        }
    }

    /// By how many voices it passes the pool.
    const fn excess(self) -> u8 {
        self.0.saturating_sub(VoiceCount::POOL.0)
    }
}

/// The six Parts' reserves summed, within the pool by construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Booked(VoiceCount);

/// The six Parts' reserves summed past the pool: they can't all hold.
/// 9..=48 (`MAX_PARTS` × `POOL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Overbooked(Reserved);

impl Booked {
    pub const fn new(sum: u8) -> Option<Booked> {
        match VoiceCount::new(sum) {
            Some(n) => Some(Booked(n)),
            None => None,
        }
    }

    /// Each Part's reserve `r` summed: booked, or past the pool.
    pub const fn sum(r: &[Reserve; MAX_PARTS]) -> Result<Booked, Overbooked> {
        Reserved::of(r).fits()
    }

    pub const fn count(self) -> VoiceCount {
        self.0
    }
}

impl Overbooked {
    pub const fn sum(self) -> Reserved {
        self.0
    }
}

/// Why an edit can't be honoured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The Part's RESERVE over its own MAX VOICES.
    OverCap(OverCap),
    /// The six Parts' reserves past the pool.
    Overbooked(Overbooked),
}

/// Each Part's limit, lawful: each reserve within its cap, the reserves
/// within the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    parts: [VoiceLimit; MAX_PARTS],
    booked: Booked,
}

impl Default for Limits {
    fn default() -> Self {
        Limits::AUTO
    }
}

impl Limits {
    pub const AUTO: Limits = Limits {
        parts: [VoiceLimit::AUTO; MAX_PARTS],
        booked: Booked(VoiceCount::ZERO),
    };

    pub const fn get(&self, p: PartId) -> VoiceLimit {
        self.parts[p.index()]
    }

    /// Part `p` to `l`, if the reserves still fit the pool.
    pub fn try_set(&mut self, p: PartId, l: VoiceLimit) -> Result<(), Refusal> {
        let mut parts = self.parts;
        parts[p.index()] = l;
        let booked = Booked::sum(&parts.map(|l| l.reserve)).map_err(Refusal::Overbooked)?;
        *self = Limits { parts, booked };
        Ok(())
    }

    /// The reserves booked.
    pub const fn booked(&self) -> Booked {
        self.booked
    }
}

/// Each Part's pair as saved past the rule: plays as saved and may only
/// come down (ADR 0281).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Over([Asked; MAX_PARTS]);

impl Over {
    pub const fn get(&self, p: PartId) -> Asked {
        self.0[p.index()]
    }

    /// The reserves summed, past the pool or not.
    fn sum(&self) -> Reserved {
        Reserved::of(&self.0.map(|a| a.reserve))
    }
}

/// What the allocator plays and an edit is judged against: a lawful set,
/// or one saved past the rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shares {
    Lawful(Limits),
    Over(Over),
}

impl Default for Shares {
    fn default() -> Self {
        Shares::AUTO
    }
}

impl Shares {
    pub const AUTO: Shares = Shares::Lawful(Limits::AUTO);

    /// The six Parts' pairs as held, in Part order (each Part's mix's
    /// `voices`): lawful, or loaded past the rule. The one way the
    /// allocator, the UI and the renderer read them.
    pub fn from_parts(asks: &[Asked; MAX_PARTS]) -> Shares {
        let mut l = Limits::AUTO;
        for (p, &a) in PartId::ALL.into_iter().zip(asks) {
            let fits = a.lawful().ok().is_some_and(|v| l.try_set(p, v).is_ok());
            if !fits {
                return Shares::Over(Over(*asks));
            }
        }
        Shares::Lawful(l)
    }

    /// Part `p`'s pair.
    pub const fn ask(&self, p: PartId) -> Asked {
        match self {
            Shares::Lawful(l) => l.get(p).asked(),
            Shares::Over(o) => o.get(p),
        }
    }

    /// The reserves summed, past the pool or not.
    pub fn reserved(&self) -> Reserved {
        match self {
            Shares::Lawful(l) => Reserved(l.booked().count().0),
            Shares::Over(o) => o.sum(),
        }
    }

    /// Why Part `p` may not become `to`, if it may not: the one rule of
    /// ADR 0281. A lawful set refuses anything past the rule; a set saved
    /// past it refuses an edit that raises either excess, its reserve over
    /// its cap or the reserves over the pool, so it may only come down.
    pub fn refusal(&self, p: PartId, to: Asked) -> Option<Refusal> {
        match self {
            Shares::Lawful(l) => match to.lawful() {
                Err(o) => Some(Refusal::OverCap(o)),
                Ok(v) => {
                    let mut next = *l;
                    next.try_set(p, v).err()
                }
            },
            Shares::Over(o) => {
                if to.over_cap() > o.get(p).over_cap()
                    && let Err(over) = to.lawful()
                {
                    return Some(Refusal::OverCap(over));
                }
                let mut next = *o;
                next.0[p.index()] = to;
                match Booked::sum(&next.0.map(|a| a.reserve)) {
                    Err(b) if next.sum().excess() > o.sum().excess() => {
                        Some(Refusal::Overbooked(b))
                    }
                    _ => None,
                }
            }
        }
    }
}

/// What an edit of Part `p`'s pair is judged against: its own pair and the
/// other Parts' reserves summed. Equal while neither changed, so a held
/// refusal stands through any other edit (ADR 0281).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    own: Asked,
    others: Result<Booked, Overbooked>,
}

impl Shares {
    /// Part `p`'s `Seen`. Only the UI asks (bank 2's `part_voices`):
    /// inlined there, not linked in bank 1.
    #[inline(always)]
    pub fn seen(&self, p: PartId) -> Seen {
        let others = PartId::ALL.map(|q| {
            if q == p {
                Reserve::NONE
            } else {
                self.ask(q).reserve
            }
        });
        Seen {
            own: self.ask(p),
            others: Booked::sum(&others),
        }
    }
}
