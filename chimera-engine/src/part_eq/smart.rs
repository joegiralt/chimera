//! SMART EQ (ADR 0296, #396): each Part's ROLE and the master's SMART EQ
//! and AMOUNT, and the state of the four fixed cut bands it adds to every
//! Part's EQ. A lower role is cut where a higher one plays; FRONT never
//! is. The DSP is bank 2's (`dsp::smart`); these are the types bank 1
//! keeps for the settings, the stub's test and ITCM's rest.

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use super::Live;
use crate::dsp::{Hz, Unit};
use crate::hw::MAX_PARTS;
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
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct CutDb(f32);

impl CutDb {
    pub const ZERO: CutDb = CutDb(0.0);
    /// The deepest cut: BACK under FRONT at full AMOUNT.
    pub const MAX: CutDb = CutDb(6.0);

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

    /// `MAX`'s share `u`.
    #[inline(always)]
    pub const fn of(u: Unit) -> CutDb {
        CutDb::new(Self::MAX.0 * u.get())
    }
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

    /// How much of each cut's target this setting asks: AMOUNT, none off.
    #[inline(always)]
    pub const fn share(self) -> Unit {
        if self.listens() {
            self.amount().unit()
        } else {
            Unit::ZERO
        }
    }
}

impl Default for SmartEq {
    fn default() -> Self {
        SmartEq::OFF
    }
}

/// One band filter's memory (a TPT SVF's two integrators).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BandTaps {
    pub(crate) s1: f32,
    pub(crate) s2: f32,
}

impl BandTaps {
    pub(crate) const ZERO: BandTaps = BandTaps { s1: 0.0, s2: 0.0 };
}

/// A band's applied gain less one, `G − 1` (0 unity, −0.5 at −6 dB): zero
/// bytes are unity, so ITCM's boot zeroing is no cut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Dip(pub(crate) f32);

impl Dip {
    pub(crate) const UNITY: Dip = Dip(0.0);
}

/// A band's level as followed: its band-pass's mean square (power).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Power(pub(crate) f32);

/// One Part's four bands: their filters on its bus (`mid`) and a DRUM
/// pad's side, the levels heard, the cuts decided and the gains last
/// applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PartSmart {
    pub(crate) mid: [BandTaps; BANDS],
    pub(crate) side: [BandTaps; BANDS],
    pub(crate) level: [Power; BANDS],
    pub(crate) cut: [CutDb; BANDS],
    pub(crate) dip: [Dip; BANDS],
}

impl PartSmart {
    pub(crate) const REST: PartSmart = PartSmart {
        mid: [BandTaps::ZERO; BANDS],
        side: [BandTaps::ZERO; BANDS],
        level: [Power(0.0); BANDS],
        cut: [CutDb::ZERO; BANDS],
        dip: [Dip::UNITY; BANDS],
    };
}

/// Every Part's SMART EQ, in `PartEqs` (ITCM on the unit). All-zero bytes
/// are `REST`: nothing heard, nothing cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmartState {
    pub(crate) parts: [PartSmart; MAX_PARTS],
    pub(crate) live: Live,
}

impl SmartState {
    pub const REST: SmartState = SmartState {
        parts: [PartSmart::REST; MAX_PARTS],
        live: Live::Bypass,
    };

    /// Listening or still releasing: bank 1's stub calls the door.
    pub fn live(&self) -> bool {
        self.live == Live::Running
    }

    /// Part `p`'s cut in `b` as last decided.
    pub fn cut(&self, p: PartId, b: Band) -> CutDb {
        self.parts[p.index()].cut[b.index()]
    }

    /// Part `p`'s gain in `b` as last applied, linear.
    pub fn gain(&self, p: PartId, b: Band) -> f32 {
        1.0 + self.parts[p.index()].dip[b.index()].0
    }

    /// Every filter's and level's memory, for the tests' subnormal check.
    pub fn memory(&self) -> impl Iterator<Item = f32> + '_ {
        self.parts.iter().flat_map(|p| {
            let taps = p.mid.iter().chain(&p.side).flat_map(|t| [t.s1, t.s2]);
            taps.chain(p.level.iter().map(|l| l.0))
        })
    }
}

/// A Part's four live cuts as the UI reads them: a byte a band (LOW in
/// bits 0–7), each `STEPS` a dB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutView(u32);

impl CutView {
    /// Steps a dB in a band's byte: `CutDb::MAX` is 192.
    const STEPS: f32 = 32.0;
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
pub struct SmartView([AtomicU32; MAX_PARTS]);

impl SmartView {
    pub const fn new() -> SmartView {
        SmartView([const { AtomicU32::new(0) }; MAX_PARTS])
    }

    #[inline(always)]
    pub fn publish(&self, p: usize, v: CutView) {
        if let Some(w) = self.0.get(p) {
            w.store(v.0, Relaxed);
        }
    }

    pub fn read(&self, p: PartId) -> CutView {
        CutView(self.0[p.index()].load(Relaxed))
    }

    /// Every Part's view, for the UI's dirty key.
    pub fn key(&self) -> u32 {
        self.0
            .iter()
            .fold(0u32, |k, w| k.rotate_left(5) ^ w.load(Relaxed))
    }
}

impl Default for SmartView {
    fn default() -> Self {
        SmartView::new()
    }
}

/// What SMART EQ is doing to each Part now.
pub static SMART_VIEW: SmartView = SmartView::new();
