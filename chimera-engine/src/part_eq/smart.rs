//! SMART EQ (ADR 0296, #396): each Part's ROLE and the master's SMART EQ
//! and AMOUNT, and the state of the four fixed cut bands it adds to every
//! Part's EQ. A lower role is cut where a higher one plays; FRONT never
//! is. The DSP is bank 2's (`dsp::smart`); these are the types bank 1
//! keeps for the settings, the stub's test and ITCM's rest.

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use super::Live;
use crate::dsp::{Hz, Unit};
use crate::hw::MAX_PARTS;
use crate::params::EngineType;
use crate::project::PartId;

/// A Part's place in the mix: FRONT is kept clear, MID yields to FRONT,
/// BACK to both. The project's mix, never the Sound's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
    Front = 0,
    Mid = 1,
    Back = 2,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Front, Role::Mid, Role::Back];
    /// What a project without roles loads.
    pub const DEFAULT: Role = Role::Mid;

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(c: u8) -> Option<Role> {
        match c {
            0 => Some(Role::Front),
            1 => Some(Role::Mid),
            2 => Some(Role::Back),
            _ => None,
        }
    }

    /// A stored or knob value, as the other choices read theirs (`as`
    /// truncates and saturates; NaN is 0): past BACK is BACK.
    pub fn from_f32(v: f32) -> Role {
        match Role::from_code(v as u8) {
            Some(r) => r,
            None => Role::Back,
        }
    }

    pub const fn ident(self) -> &'static str {
        match self {
            Role::Front => "FRONT",
            Role::Mid => "MID",
            Role::Back => "BACK",
        }
    }

    /// The coarse view's letter.
    pub const fn letter(self) -> &'static str {
        match self {
            Role::Front => "F",
            Role::Mid => "M",
            Role::Back => "B",
        }
    }

    /// How far `self` outranks `lower`, as the share of the deepest cut
    /// `lower` takes where `self` plays: one rank 2/3, two (FRONT over
    /// BACK) all of it, none at or under `lower`.
    #[inline(always)]
    pub const fn depth_over(self, lower: Role) -> Unit {
        match (self, lower) {
            (Role::Front, Role::Mid) | (Role::Mid, Role::Back) => Unit::new(2.0 / 3.0),
            (Role::Front, Role::Back) => Unit::ONE,
            (Role::Front, Role::Front)
            | (Role::Mid, Role::Front | Role::Mid)
            | (Role::Back, Role::Front | Role::Mid | Role::Back) => Unit::ZERO,
        }
    }
}

impl Default for Role {
    fn default() -> Self {
        Role::DEFAULT
    }
}

/// ROLE's choices, by `Role::code`.
pub const ROLE_NAMES: [&str; Role::ALL.len()] =
    [Role::Front.ident(), Role::Mid.ident(), Role::Back.ident()];

/// What a Part plays, as SMART EQ keys from it: a DRUM Part's LOW band
/// keys the kick's fast dip (owner, 2026-11-19); any other Part's is
/// heard slowly, as every band is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Drum,
    Tonal,
}

impl PartKind {
    pub const fn of(e: EngineType) -> PartKind {
        match e {
            EngineType::Drum => PartKind::Drum,
            EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => {
                PartKind::Tonal
            }
        }
    }
}

/// The four places SMART EQ cuts, low to high, two octaves apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Low,
    LowMid,
    HighMid,
    High,
}

/// The bands a Part has.
pub const BANDS: usize = 4;

impl Band {
    pub const ALL: [Band; BANDS] = [Band::Low, Band::LowMid, Band::HighMid, Band::High];

    pub const fn index(self) -> usize {
        match self {
            Band::Low => 0,
            Band::LowMid => 1,
            Band::HighMid => 2,
            Band::High => 3,
        }
    }

    /// The band's centre.
    pub const fn centre(self) -> Hz {
        Hz::new(match self {
            Band::Low => 100.0,
            Band::LowMid => 400.0,
            Band::HighMid => 1_600.0,
            Band::High => 6_400.0,
        })
    }

    /// LOW and LOW MID: where Parts pile into mud, a lower role yields
    /// more and to quieter tails.
    #[inline(always)]
    pub const fn muddy(self) -> bool {
        matches!(self, Band::Low | Band::LowMid)
    }

    /// The band whose turn `n` is: each comes up in order.
    #[inline(always)]
    pub(crate) const fn of_turn(n: u8) -> Band {
        Band::ALL[n as usize % BANDS]
    }

    /// Its name on PRT › EQ.
    pub const fn name(self) -> &'static str {
        match self {
            Band::Low => "LOW",
            Band::LowMid => "LOW MID",
            Band::HighMid => "HIGH MID",
            Band::High => "HIGH",
        }
    }
}

/// How deep a band is cut, 0 to `CutDb::MAX` dB (positive is down).
/// AMOUNT's curve (`dsp::smart::KNOTS`) sets how deep each band may go.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct CutDb(f32);

impl CutDb {
    pub const ZERO: CutDb = CutDb(0.0);
    /// The deepest any cut goes: the kick's dip, BACK under FRONT, at
    /// AMOUNT 127 (`dsp::smart::KNOTS`, checked there).
    pub const MAX: CutDb = CutDb(27.0);

    /// `db` held to 0..=`MAX`; NaN is 0.
    pub const fn new(db: f32) -> CutDb {
        CutDb(if db.is_nan() {
            0.0
        } else {
            crate::dsp::clampf(db, 0.0, Self::MAX.0)
        })
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    /// A step from here toward `target` by a one-pole's `pole` (0 to 1):
    /// `target + pole · (self − target)`, between the two, so in range
    /// with no clamp. Under `land` on the way to no cut, it lands on it.
    #[inline(always)]
    pub(crate) fn toward(self, target: CutDb, pole: Unit, land: f32) -> CutDb {
        let next = target.0 + pole.get() * (self.0 - target.0);
        if target.0 == 0.0 && next < land {
            CutDb::ZERO
        } else {
            CutDb(next)
        }
    }

    /// This cap's share `u`.
    #[inline(always)]
    pub(crate) fn share(self, u: Unit) -> CutDb {
        CutDb(self.0 * u.get())
    }
}

/// How deep SMART EQ may cut at an AMOUNT: BACK under FRONT (one rank
/// apart is 2/3 of each, `Role::depth_over`), in LOW and LOW MID (`mud`),
/// in HIGH MID and HIGH (`clear`), and the kick's dip in LOW (`kick`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caps {
    pub mud: CutDb,
    pub clear: CutDb,
    pub kick: CutDb,
}

impl Caps {
    /// Nothing may be cut: OFF, AMOUNT 0.
    pub const NONE: Caps = Caps {
        mud: CutDb::ZERO,
        clear: CutDb::ZERO,
        kick: CutDb::ZERO,
    };
}

/// SMART EQ's switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Switch {
    Off = 0,
    On = 1,
}

impl Switch {
    pub const ALL: [Switch; 2] = [Switch::Off, Switch::On];

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(c: u8) -> Option<Switch> {
        match c {
            0 => Some(Switch::Off),
            1 => Some(Switch::On),
            _ => None,
        }
    }

    /// As LO CUT reads its value: 0 (and NaN, and below) off.
    pub fn from_f32(v: f32) -> Switch {
        if v as u8 == 0 {
            Switch::Off
        } else {
            Switch::On
        }
    }

    pub const fn ident(self) -> &'static str {
        match self {
            Switch::Off => "OFF",
            Switch::On => "ON",
        }
    }
}

/// SMART EQ's choices, by `Switch::code`.
pub const SWITCH_NAMES: [&str; Switch::ALL.len()] = [Switch::Off.ident(), Switch::On.ident()];

/// AMOUNT: a code 0..=127, the share of the deepest cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount(u8);

impl Amount {
    pub const MAX: u8 = 127;
    pub const ZERO: Amount = Amount(0);
    /// Where AMOUNT starts: half the deepest cut.
    pub const DEFAULT: Amount = Amount(64);

    pub const fn from_code(c: u8) -> Option<Amount> {
        if c <= Self::MAX {
            Some(Amount(c))
        } else {
            None
        }
    }

    /// A stored or knob value, rounded (halves up) and clamped; NaN is 0.
    pub fn from_f32(v: f32) -> Amount {
        let c = (v + 0.5) as u8;
        Amount(if c > Self::MAX { Self::MAX } else { c })
    }

    pub const fn code(self) -> u8 {
        self.0
    }

    pub const fn unit(self) -> Unit {
        Unit::new(self.0 as f32 / Self::MAX as f32)
    }
}

/// The master's SMART EQ as the mix stores it: AMOUNT's code in bits 0–6,
/// ON in bit 7. Every byte is a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartEq(u8);

const ON_BIT: u8 = 0x80;
const AMOUNT_BITS: u8 = 0x7f;
const _: () = assert!(Amount::MAX == AMOUNT_BITS);

impl SmartEq {
    /// OFF, AMOUNT at its default: what an old project loads.
    pub const OFF: SmartEq = SmartEq::new(Switch::Off, Amount::DEFAULT);

    pub const fn new(s: Switch, a: Amount) -> SmartEq {
        SmartEq(a.0 | if matches!(s, Switch::On) { ON_BIT } else { 0 })
    }

    pub const fn switch(self) -> Switch {
        if self.0 & ON_BIT == 0 {
            Switch::Off
        } else {
            Switch::On
        }
    }

    pub const fn amount(self) -> Amount {
        Amount(self.0 & AMOUNT_BITS)
    }

    pub const fn with_switch(self, s: Switch) -> SmartEq {
        SmartEq::new(s, self.amount())
    }

    pub const fn with_amount(self, a: Amount) -> SmartEq {
        SmartEq::new(self.switch(), a)
    }

    /// ON with an AMOUNT: there is something to cut. Otherwise SMART EQ
    /// only releases what it cut, then rests (bank 1's stub skips it).
    pub const fn listens(self) -> bool {
        matches!(self.switch(), Switch::On) && self.0 & AMOUNT_BITS != 0
    }
}

impl Default for SmartEq {
    fn default() -> Self {
        SmartEq::OFF
    }
}

/// One band filter's memory (its two state words).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BandTaps {
    pub(crate) s1: f32,
    pub(crate) s2: f32,
}

impl BandTaps {
    pub(crate) const ZERO: BandTaps = BandTaps { s1: 0.0, s2: 0.0 };
}

/// A band's dip, what its band-pass adds to the bus: 0 unity, about −0.5
/// at −6 dB, within `MIN..=MAX`. A little over 0 where a neighbour's cut
/// is being kept out of this band (`dsp::smart::dips`). Zero bytes are
/// unity, so ITCM's boot zeroing is no cut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Dip(f32);

impl Dip {
    pub(crate) const UNITY: Dip = Dip(0.0);
    /// The deepest: `CutDb::MAX` alone is −0.955; with its neighbours'
    /// share taken off (`UNMIX`'s rows sum to 1.36 at most in size).
    const MIN: f32 = -1.375;
    /// The most a band gives back against its neighbours' skirts
    /// (`UNMIX`'s largest negative terms, 0.19 of a full cut each side).
    const MAX: f32 = 0.375;

    /// `v` held to `MIN..=MAX`; NaN is unity.
    #[inline(always)]
    pub(crate) fn new(v: f32) -> Dip {
        Dip(if v.is_nan() {
            0.0
        } else {
            crate::dsp::clampf(v, Self::MIN, Self::MAX)
        })
    }

    #[inline(always)]
    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

/// A mean square (power), 0 or `FLOOR` and over, finite: under `FLOOR`
/// (−300 dB) is silence, so a follower never falls subnormal.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub(crate) struct Power(f32);

impl Power {
    pub(crate) const ZERO: Power = Power(0.0);
    const FLOOR: f32 = 1e-30;

    /// `v`, or 0 if it is under `FLOOR`, negative, NaN or inf.
    #[inline(always)]
    pub(crate) fn new(v: f32) -> Power {
        Power(if v.is_finite() && v >= Self::FLOOR {
            v
        } else {
            0.0
        })
    }

    #[inline(always)]
    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

/// A band's level as the rule reads it: dB over SMART EQ's silence
/// (`dsp::smart::SILENCE`), 0 and up. Zero bytes are silence.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub(crate) struct Level(f32);

impl Level {
    pub(crate) const SILENT: Level = Level(0.0);

    /// `over` dB over silence; under it, NaN included, is silent
    /// (`clampf` gives a NaN its low bound).
    #[inline(always)]
    pub(crate) fn over_silence(over: f32) -> Level {
        Level(crate::dsp::clampf(over, 0.0, f32::MAX))
    }

    /// dB over silence, as a number for the rule's sums: finite and 0 or
    /// more by construction.
    #[inline(always)]
    pub(crate) const fn over(self) -> f32 {
        self.0
    }
}

/// Blocks counted since something, saturating: a kick's last onset, the
/// gap between its last two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Blocks(u16);

impl Blocks {
    pub(crate) const ZERO: Blocks = Blocks(0);

    #[inline(always)]
    pub(crate) const fn of(n: u16) -> Blocks {
        Blocks(n)
    }

    #[inline(always)]
    pub(crate) const fn after(self, n: Blocks) -> Blocks {
        Blocks(self.0.saturating_add(n.0))
    }

    #[inline(always)]
    pub(crate) const fn get(self) -> u16 {
        self.0
    }
}

/// Whether a Part's cuts have anywhere to go: `Rest` with none cut, none
/// wanted and its dips back at unity, when its cuts need no step. Zero
/// bytes are `Rest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Cuts {
    Rest = 0,
    Move = 1,
}

/// How a Part's bus filters ran last block: its memory is that rate's, so
/// a change of rate starts them empty. Zero bytes are `Thin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Hears {
    /// Listening only, decimated: LOW and LOW MID at a quarter of the
    /// rate, HIGH MID and HIGH at half.
    Thin = 0,
    /// Cutting: every band at the full rate, heard as it cuts.
    Full = 1,
}

/// One Part's four bands: their filters on its bus (`mid`) and a DRUM
/// pad's side, the levels heard, the cuts decided and the gains last
/// applied, and its kick as a key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PartSmart {
    pub(crate) mid: [BandTaps; BANDS],
    pub(crate) side: [BandTaps; BANDS],
    /// Each band's followed power, and its dB as the rule last read it.
    pub(crate) level: [Power; BANDS],
    pub(crate) heard: [Level; BANDS],
    /// The slow cuts' targets as the rule last set them, and the cuts.
    pub(crate) want: [CutDb; BANDS],
    pub(crate) cut: [CutDb; BANDS],
    /// The dips last applied (`cut` and `kick`, each band's neighbours
    /// allowed for), and the cuts they were made from.
    pub(crate) dip: [Dip; BANDS],
    pub(crate) dipped: [CutDb; BANDS],
    /// The LOW band's low-pass energy, followed fast: a DRUM Part's kick.
    pub(crate) key: Power,
    /// `key`'s recent peak, falling slowly: the hit `key` is measured from.
    pub(crate) key_peak: Power,
    /// `key`'s floor (down at once, up slowly): an onset rises over it.
    pub(crate) floor: Power,
    /// The kick's fast LOW dip on this Part, on top of `cut`'s slow LOW,
    /// and its target as the kick's rule last set it.
    pub(crate) kick: CutDb,
    pub(crate) kick_want: CutDb,
    /// Its own kick's onsets: blocks since the last, and between the last
    /// two (0: not known).
    pub(crate) since: Blocks,
    pub(crate) gap: Blocks,
    pub(crate) hears: Hears,
    pub(crate) cuts: Cuts,
}

impl PartSmart {
    pub(crate) const REST: PartSmart = PartSmart {
        mid: [BandTaps::ZERO; BANDS],
        side: [BandTaps::ZERO; BANDS],
        level: [Power::ZERO; BANDS],
        heard: [Level::SILENT; BANDS],
        want: [CutDb::ZERO; BANDS],
        cut: [CutDb::ZERO; BANDS],
        dip: [Dip::UNITY; BANDS],
        dipped: [CutDb::ZERO; BANDS],
        key: Power::ZERO,
        key_peak: Power::ZERO,
        floor: Power::ZERO,
        kick: CutDb::ZERO,
        kick_want: CutDb::ZERO,
        since: Blocks::ZERO,
        gap: Blocks::ZERO,
        hears: Hears::Thin,
        cuts: Cuts::Rest,
    };

    /// Its bands run at the full rate: cutting, or not yet let go.
    pub(crate) fn runs_full(&self) -> bool {
        self.hears == Hears::Full || self.cuts == Cuts::Move
    }

    /// The cuts the bands play: `cut`, LOW the deeper of its slow cut and
    /// the kick's fast dip.
    #[inline(always)]
    pub(crate) fn applied(&self) -> [CutDb; BANDS] {
        let mut c = self.cut;
        if self.kick > c[0] {
            c[0] = self.kick;
        }
        c
    }
}

/// LOW's and LOW MID's filter: Chamberlin's state-variable form, exact
/// enough far under the rate (400 Hz of 12 kHz at most) and quiet in
/// `f32` there, where a direct form's round-off is not. `bp += g · hp`,
/// `lp += g · bp`; `k · bp` is the unity-peak band-pass, `lp` LOW's
/// low-pass (the kick's key). Three multiply-adds a sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sv {
    /// `2 sin(π f / rate)`.
    pub(crate) g: f32,
}

/// HIGH MID's and HIGH's filter: the bilinear band-pass (a trapezoidal SVF's
/// response exactly) as a transposed direct form II, unity at its
/// centre: `y = b x + s1`, `s1 = s2 − a1 y`, `s2 = −b x − a2 y`. Four
/// multiply-adds a sample; its round-off is far under the signal this
/// high in the band (a centre at 1/30 of the rate or over).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bq {
    pub(crate) b: f32,
    pub(crate) a1: f32,
    pub(crate) a2: f32,
}

/// The four bands' filters, full and decimated, and the block's time
/// constants at a sample rate: made once (`dsp::smart`'s `Rate::at`, its
/// tangents and powers some 600 instructions) and kept with the state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rate {
    /// The sample rate these are for: 0, none yet (ITCM's zero bytes).
    pub(crate) hz: u32,
    pub(crate) low: [Sv; 2],
    pub(crate) high: [Bq; 2],
    pub(crate) thin_low: [Sv; 2],
    pub(crate) thin_high: [Bq; 2],
    /// One-pole coefficients a block: the level's rise and fall, the
    /// key's fall, the kick dip's and the cut's attack and release.
    pub(crate) rise: Unit,
    pub(crate) fall: Unit,
    pub(crate) key_fall: Unit,
    /// The most the key's floor rises a turn of the kick's rule, as a
    /// gain of power.
    pub(crate) floor_rise: f32,
    pub(crate) kick_attack: Unit,
    pub(crate) kick_release: Unit,
    pub(crate) attack: Unit,
    pub(crate) release: Unit,
    /// The kick's hold: whole to this gap, none from that one; and how
    /// soon after an onset another counts.
    pub(crate) hold_full: Blocks,
    pub(crate) hold_none: Blocks,
    pub(crate) refract: Blocks,
}

impl Rate {
    /// None made: the bytes ITCM's boot zeroing leaves.
    pub(crate) const NONE: Rate = Rate {
        hz: 0,
        low: [Sv { g: 0.0 }; 2],
        high: [Bq {
            b: 0.0,
            a1: 0.0,
            a2: 0.0,
        }; 2],
        thin_low: [Sv { g: 0.0 }; 2],
        thin_high: [Bq {
            b: 0.0,
            a1: 0.0,
            a2: 0.0,
        }; 2],
        rise: Unit::ZERO,
        fall: Unit::ZERO,
        key_fall: Unit::ZERO,
        floor_rise: 1.0,
        kick_attack: Unit::ZERO,
        kick_release: Unit::ZERO,
        attack: Unit::ZERO,
        release: Unit::ZERO,
        hold_full: Blocks::ZERO,
        hold_none: Blocks::ZERO,
        refract: Blocks::ZERO,
    };
}

/// Whether the reverb returned anything this block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Returned {
    Sounding,
    /// At rest: nothing played, nothing to run.
    Rests,
}

/// The reverb's return as a member of SMART EQ (owner, 2026-11-19: "we
/// should give the reverb its role of its own"): it yields as a Part of
/// its role would, the kick's dip with it, and makes nothing yield. Its
/// bands cut left and right alike (`cuts.mid` the left's memory,
/// `cuts.side` the right's); it is heard as their sum, decimated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wash {
    pub(crate) cuts: PartSmart,
    pub(crate) listen: [BandTaps; BANDS],
}

impl Wash {
    pub(crate) const REST: Wash = Wash {
        cuts: PartSmart::REST,
        listen: [BandTaps::ZERO; BANDS],
    };
}

/// The members whose bands still run at the full rate whatever their
/// role says: a Part (or the reverb's return) moved to FRONT keeps its
/// cuts until they let go, up to some seconds, and is billed as cut
/// until then. A bit a Part, and the return's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StillCut(u8);

impl StillCut {
    pub const NONE: StillCut = StillCut(0);
    const WASH: u8 = 1 << MAX_PARTS;
    const _FITS: () = assert!(MAX_PARTS < 8);

    /// As the last block left them (`dsp::smart::run`).
    pub fn of(st: &SmartState) -> StillCut {
        st.still
    }

    /// Bank 2's: counted once a block, so the bill reads a byte.
    #[inline(always)]
    pub(crate) fn count(st: &SmartState) -> StillCut {
        let mut bits = 0u8;
        for p in PartId::ALL {
            if st.part(p).runs_full() {
                bits |= 1 << p.index();
            }
        }
        if st.wash.cuts.runs_full() {
            bits |= Self::WASH;
        }
        StillCut(bits)
    }

    pub const fn part(self, p: PartId) -> bool {
        self.0 & (1 << p.index()) != 0
    }

    pub const fn wash(self) -> bool {
        self.0 & Self::WASH != 0
    }
}

/// Every Part's SMART EQ and the reverb return's, in `PartEqs` (ITCM on
/// the unit). All-zero bytes are `REST`: nothing heard, nothing cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmartState {
    pub(crate) parts: [PartSmart; MAX_PARTS],
    pub(crate) wash: Wash,
    /// The filters and time constants at the rate in use.
    pub(crate) rate: Rate,
    /// Blocks run, modulo `TURNS`: whose turn it is (the slow rule's
    /// band or the kick's rule, the band read in dB, the Part whose view
    /// is published).
    pub(crate) turn: u8,
    pub(crate) live: Live,
    /// Who still runs cut, for the bill.
    pub(crate) still: StillCut,
}

impl SmartState {
    /// A round of turns: every band's rule (one every other block) and
    /// every Part's view come up evenly.
    pub(crate) const TURNS: u8 = (2 * BANDS * MAX_PARTS) as u8;

    pub const REST: SmartState = SmartState {
        parts: [PartSmart::REST; MAX_PARTS],
        wash: Wash::REST,
        rate: Rate::NONE,
        turn: 0,
        live: Live::Bypass,
        still: StillCut::NONE,
    };

    #[inline(always)]
    pub(crate) fn part(&self, p: PartId) -> &PartSmart {
        &self.parts[p.index()]
    }

    #[inline(always)]
    pub(crate) fn part_mut(&mut self, p: PartId) -> &mut PartSmart {
        &mut self.parts[p.index()]
    }

    /// Listening or still releasing: bank 1's stub calls the door.
    pub fn live(&self) -> bool {
        self.live == Live::Running
    }

    /// Part `p`'s cut in `b` as last decided, the kick's dip with it.
    pub fn cut(&self, p: PartId, b: Band) -> CutDb {
        self.parts[p.index()].applied()[b.index()]
    }

    /// Part `p`'s slow cut in `b` alone, without the kick's dip.
    pub fn slow_cut(&self, p: PartId, b: Band) -> CutDb {
        self.parts[p.index()].cut[b.index()]
    }

    /// The kick's fast LOW dip on Part `p`.
    pub fn kick(&self, p: PartId) -> CutDb {
        self.parts[p.index()].kick
    }

    /// Blocks since Part `p`'s kick last began (its onset).
    pub fn since_kick(&self, p: PartId) -> u16 {
        self.parts[p.index()].since.get()
    }

    /// The reverb return's cut in `b` as last decided, the kick's dip
    /// with it.
    pub fn wash_cut(&self, b: Band) -> CutDb {
        self.wash.cuts.applied()[b.index()]
    }

    /// Part `p`'s dip in `b` as last applied (`G − 1` of the band alone).
    pub fn dip(&self, p: PartId, b: Band) -> f32 {
        self.parts[p.index()].dip[b.index()].get()
    }

    /// Whether Part `p`'s side filters hold anything: for the tests.
    pub fn side_rests(&self, p: PartId) -> bool {
        self.parts[p.index()].side == [BandTaps::ZERO; BANDS]
    }

    /// Every filter's and level's memory, for the tests' subnormal check.
    pub fn memory(&self) -> impl Iterator<Item = f32> + '_ {
        let listen = self.wash.listen.iter().flat_map(|t| [t.s1, t.s2]);
        let members = self.parts.iter().chain([&self.wash.cuts]);
        members
            .flat_map(|p| {
                let taps = p.mid.iter().chain(&p.side).flat_map(|t| [t.s1, t.s2]);
                taps.chain(p.level.iter().map(|l| l.get())).chain([
                    p.key.get(),
                    p.key_peak.get(),
                    p.kick.get(),
                ])
            })
            .chain(listen)
    }
}

/// A Part's four live cuts as the UI reads them: a byte a band (LOW in
/// bits 0–7), each `STEPS` a dB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutView(u32);

impl CutView {
    /// Steps a dB in a band's byte: `CutDb::MAX` (27 dB) is 216, so the
    /// deepest cut reads whole, never wrapped.
    const STEPS: f32 = 8.0;
    pub const NONE: CutView = CutView(0);

    #[inline(always)]
    pub fn of(cuts: &[CutDb; BANDS]) -> CutView {
        let mut w = 0u32;
        for (b, c) in Band::ALL.iter().zip(cuts) {
            w |= ((c.get() * Self::STEPS + 0.5) as u32 & 0xff) << (8 * b.index());
        }
        CutView(w)
    }

    pub fn cut(self, b: Band) -> CutDb {
        CutDb::new((self.0 >> (8 * b.index()) & 0xff) as f32 / Self::STEPS)
    }

    /// The deepest of the four.
    pub fn deepest(self) -> (Band, CutDb) {
        let mut best = (Band::Low, self.cut(Band::Low));
        for b in Band::ALL {
            if self.cut(b) > best.1 {
                best = (b, self.cut(b));
            }
        }
        best
    }

    pub const fn word(self) -> u32 {
        self.0
    }
}

/// Each Part's live cuts, published by the audio once a block it runs
/// (relaxed stores, never blocking) and read by the UI's views.
pub struct SmartView {
    parts: [AtomicU32; MAX_PARTS],
    wash: AtomicU32,
}

impl SmartView {
    pub const fn new() -> SmartView {
        SmartView {
            parts: [const { AtomicU32::new(0) }; MAX_PARTS],
            wash: AtomicU32::new(0),
        }
    }

    /// The reverb return's cuts.
    #[inline(always)]
    pub fn publish_wash(&self, v: CutView) {
        self.wash.store(v.0, Relaxed);
    }

    pub fn read_wash(&self) -> CutView {
        CutView(self.wash.load(Relaxed))
    }

    #[inline(always)]
    pub fn publish(&self, p: PartId, v: CutView) {
        self.parts[p.index()].store(v.0, Relaxed);
    }

    pub fn read(&self, p: PartId) -> CutView {
        CutView(self.parts[p.index()].load(Relaxed))
    }
}

impl Default for SmartView {
    fn default() -> Self {
        SmartView::new()
    }
}

/// What SMART EQ is doing to each Part now.
pub static SMART_VIEW: SmartView = SmartView::new();
