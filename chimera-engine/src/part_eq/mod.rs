//! Each Part's channel EQ (ADR 0284, #342): TILT, dark to bright about
//! 810 Hz, and LO CUT, a 150 Hz high-pass, at the end of the Part's
//! channel, after its Sound and before its level, pan and sends. The
//! settings are the project's mix (`PartParams::eq`), never the Sound's.
//! One EQ a Part: SMART EQ (#396) adds its cuts here.

pub mod bill;
pub mod door;
pub mod dsp;
pub mod smart;

use core::marker::PhantomData;

use crate::dsp::ease::Ease;
use crate::dsp::{Db, Gain, Unit};
use crate::project::PartId;

/// TILT: a step in `MIN_STEP..=MAX_STEP`, each 1/128 of the 20 dB travel.
/// `FLAT` (step 0) is exactly 0 dB; −64 is −10 dB, +63 is +9.84 dB. Its
/// code (CC/NRPN value, the stored bits) is `step − MIN_STEP`: CC 64 is
/// flat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tilt(i8);

impl Tilt {
    pub const MIN_STEP: i8 = -64;
    pub const MAX_STEP: i8 = 63;
    /// The steps end to end, `MIN_STEP..=MAX_STEP`.
    pub const SPAN: usize = (Self::MAX_STEP as i16 - Self::MIN_STEP as i16 + 1) as usize;
    pub const FLAT: Tilt = Tilt(0);
    /// Steps from flat to the dark end, where the shelves reach `FULL`.
    const STEPS_TO_FULL: f32 = -(Self::MIN_STEP as f32);
    /// The shelves' reach at the dark end (the Jellymix's ±10 dB).
    pub const FULL: Db = Db::new(10.0);
    /// `code` of `FLAT`.
    const CODE_FLAT: i16 = -(Self::MIN_STEP as i16);

    /// `step`, clamped to the knob's travel.
    pub const fn at(step: i8) -> Tilt {
        Tilt(if step < Self::MIN_STEP {
            Self::MIN_STEP
        } else if step > Self::MAX_STEP {
            Self::MAX_STEP
        } else {
            step
        })
    }

    /// Every step, dark to bright.
    pub fn all() -> impl Iterator<Item = Tilt> {
        (Self::MIN_STEP..=Self::MAX_STEP).map(Tilt)
    }

    /// A stored or knob value (a step as `f32`), rounded (halves up) and
    /// clamped; NaN is flat. As a code: `as` truncates the shifted value,
    /// a floor above 0, and saturates below it (bank 1's `write`, no
    /// `roundf`).
    pub fn from_f32(v: f32) -> Tilt {
        if v.is_nan() {
            return Tilt::FLAT;
        }
        let c = (v + (Self::CODE_FLAT as f32 + 0.5)) as u8;
        Tilt((if c > TILT_BITS { TILT_BITS } else { c }) as i8 - Self::CODE_FLAT as i8)
    }

    pub const fn step(self) -> i8 {
        self.0
    }

    /// The high shelf's gain; the low shelf's is its negative.
    pub fn db(self) -> Db {
        Db::new(f32::from(self.0) * (Self::FULL.get() / Self::STEPS_TO_FULL))
    }

    /// 0..=127, flat at 64.
    pub const fn code(self) -> u8 {
        (self.0 as i16 + Self::CODE_FLAT) as u8
    }

    pub const fn from_code(c: u8) -> Option<Tilt> {
        let s = c as i16 - Self::CODE_FLAT;
        if s <= Self::MAX_STEP as i16 {
            Some(Tilt(s as i8))
        } else {
            None
        }
    }
}

/// LO CUT: the 150 Hz high-pass in or out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LoCut {
    Off = 0,
    On = 1,
}

impl LoCut {
    pub const ALL: [LoCut; 2] = [LoCut::Off, LoCut::On];

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(c: u8) -> Option<LoCut> {
        match c {
            0 => Some(LoCut::Off),
            1 => Some(LoCut::On),
            _ => None,
        }
    }

    /// A stored or knob value, as the other choices read theirs (`as`
    /// truncates and saturates): 0 (and NaN, and below) is off, 1 and
    /// past it on.
    pub fn from_f32(v: f32) -> LoCut {
        if v as u8 == 0 { LoCut::Off } else { LoCut::On }
    }

    pub const fn ident(self) -> &'static str {
        match self {
            LoCut::Off => "OFF",
            LoCut::On => "ON",
        }
    }

    /// How far in the high-pass is crossfaded: none off, all on.
    pub const fn amount(self) -> Unit {
        match self {
            LoCut::Off => Unit::ZERO,
            LoCut::On => Unit::ONE,
        }
    }

    /// Its bit in `PartEq`.
    const fn bit(self) -> u8 {
        match self {
            LoCut::Off => 0,
            LoCut::On => LO_CUT_BIT,
        }
    }
}

/// A Part's EQ as its mix stores it: TILT's code in bits 0–6, LO CUT in
/// bit 7. Every byte is a setting, so the byte is total. Zero is TILT
/// −64: `FLAT` is the default, not zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartEq(u8);

/// LO CUT's bit.
const LO_CUT_BIT: u8 = 0x80;
/// TILT's code's bits.
const TILT_BITS: u8 = 0x7f;
const _: () = assert!(Tilt::at(Tilt::MAX_STEP).code() == TILT_BITS);
const _: () = assert!(Tilt::at(Tilt::MIN_STEP).code() == 0);
const _: () = assert!(Tilt::SPAN == TILT_BITS as usize + 1);

impl PartEq {
    /// 0 dB, LO CUT off: what an old project loads, and the bypass.
    pub const FLAT: PartEq = PartEq::new(Tilt::FLAT, LoCut::Off);

    pub const fn new(t: Tilt, c: LoCut) -> PartEq {
        PartEq(t.code() | c.bit())
    }

    pub const fn tilt(self) -> Tilt {
        // Seven bits are a code, 0..=127 (the asserts above): always one.
        match Tilt::from_code(self.0 & TILT_BITS) {
            Some(t) => t,
            None => Tilt::FLAT,
        }
    }

    pub const fn lo_cut(self) -> LoCut {
        if self.0 & LO_CUT_BIT == 0 {
            LoCut::Off
        } else {
            LoCut::On
        }
    }

    /// TILT's code into its bits, LO CUT's kept.
    pub const fn with_tilt(self, t: Tilt) -> PartEq {
        PartEq((self.0 & LO_CUT_BIT) | t.code())
    }

    /// LO CUT's bit, TILT's kept.
    pub const fn with_lo_cut(self, c: LoCut) -> PartEq {
        PartEq((self.0 & TILT_BITS) | c.bit())
    }

    /// Flat and off: nothing to hear, the Part's bus untouched.
    pub const fn is_flat(self) -> bool {
        self.0 == Self::FLAT.0
    }
}

impl Default for PartEq {
    fn default() -> Self {
        PartEq::FLAT
    }
}

/// Whether a Part's EQ runs. Zero bytes are `Bypass`, so ITCM's zeroed
/// boot state (ADR 0286) is every Part bypassed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Live {
    /// Flat, off and settled: the Part's bus is not touched.
    Bypass = 0,
    Running = 1,
}

/// One filter's memory: LO CUT's low-pass and TILT's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Taps {
    pub(crate) cut: f32,
    pub(crate) tilt: f32,
}

impl Taps {
    pub(crate) const ZERO: Taps = Taps {
        cut: 0.0,
        tilt: 0.0,
    };

    /// Both finite: a NaN or inf in has not reached them.
    pub(crate) fn finite(self) -> bool {
        (self.cut + self.tilt).is_finite()
    }
}

/// A TPT one-pole's gain, `t / (1 + t)` of its warped `t = tan(π fc/fs)`:
/// in (0, 1) for a corner under Nyquist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OnePole(f32);

impl OnePole {
    pub(crate) const fn of_tan(t: f32) -> OnePole {
        OnePole(t / (1.0 + t))
    }

    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

/// TILT's `k − 1/k`: how much of the low-pass the output takes off (0 at
/// flat, negative dark).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LpShare(f32);

impl LpShare {
    pub(crate) const fn of(k: Gain) -> LpShare {
        LpShare(k.get() - 1.0 / k.get())
    }

    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

/// A block's end coefficients (`dsp::Coeffs::of`): TILT's high-shelf gain
/// `k`, low-pass share `c` and low-pass `g`; LO CUT's amount `a`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Coeffs {
    pub(crate) k: Gain,
    pub(crate) c: LpShare,
    pub(crate) g: OnePole,
    pub(crate) a: Unit,
}

impl Coeffs {
    /// The bytes ITCM's boot zeroing leaves; never run (a Part wakes on
    /// flat's own).
    const ZERO: Coeffs = Coeffs {
        k: Gain::ZERO,
        c: LpShare(0.0),
        g: OnePole(0.0),
        a: Unit::ZERO,
    };
}

/// A sample's move of each coefficient across a block, `Coeffs::step_to`:
/// signed, so untyped beyond its field's name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CoeffStep {
    pub(crate) k: f32,
    pub(crate) c: f32,
    pub(crate) g: f32,
    pub(crate) a: f32,
}

/// A setting a `Glide` eases: its value as the `Ease`'s `f32` and back.
pub(crate) trait Eased: Copy {
    fn of(v: f32) -> Self;
    fn get(self) -> f32;
}

impl Eased for Db {
    fn of(v: f32) -> Db {
        Db::new(v)
    }
    fn get(self) -> f32 {
        Db::get(self)
    }
}

impl Eased for Unit {
    fn of(v: f32) -> Unit {
        Unit::new(v)
    }
    fn get(self) -> f32 {
        Unit::get(self)
    }
}

/// An `Ease` of a typed setting: TILT's dB, LO CUT's amount.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Glide<T> {
    e: Ease,
    t: PhantomData<T>,
}

impl<T: Eased> Glide<T> {
    pub(crate) const ZERO: Glide<T> = Glide {
        e: Ease::ZERO,
        t: PhantomData,
    };

    /// Lands on `to` now.
    #[inline(always)]
    pub(crate) fn land(&mut self, to: T) {
        self.e.land(to.get());
    }

    /// This block's end toward `target`, `k` from `ease_coeff`.
    #[inline(always)]
    pub(crate) fn step(&mut self, target: T, k: f32) -> T {
        T::of(self.e.step(target.get(), k).1)
    }

    /// Where it stands.
    #[inline(always)]
    pub(crate) fn at(&self) -> T {
        T::of(self.e.at())
    }
}

/// A Part's EQ as the audio keeps it: the glides (TILT's dB, LO CUT's
/// amount), the last block's coefficients, and the filters' memory on its
/// bus (`mid`) and a DRUM pad's side (`side`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelEq {
    pub(crate) tilt: Glide<Db>,
    pub(crate) cut: Glide<Unit>,
    pub(crate) at: Coeffs,
    pub(crate) mid: Taps,
    pub(crate) side: Taps,
    pub(crate) live: Live,
}

impl ChannelEq {
    /// Bypassed: the bytes ITCM's boot zeroing leaves.
    const REST: ChannelEq = ChannelEq {
        tilt: Glide::ZERO,
        cut: Glide::ZERO,
        at: Coeffs::ZERO,
        mid: Taps::ZERO,
        side: Taps::ZERO,
        live: Live::Bypass,
    };
}

/// Every Part's EQ state: on the unit an ITCM resident (`home`), on a host
/// the Instrument's own. All-zero bytes are `REST`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartEqs {
    pub(crate) parts: [ChannelEq; crate::hw::MAX_PARTS],
    /// SMART EQ's four cut bands a Part (ADR 0296), after TILT.
    pub smart: smart::SmartState,
    /// Whether the Parts' bills last carried SMART EQ's bands
    /// (`Instrument::recost`).
    pub(crate) billed: bill::SmartBill,
}

impl PartEqs {
    /// Every Part bypassed: the bytes ITCM's boot zeroing leaves.
    pub const REST: PartEqs = PartEqs {
        parts: [ChannelEq::REST; crate::hw::MAX_PARTS],
        smart: smart::SmartState::REST,
        billed: bill::SmartBill::RESTS,
    };

    /// A Part's EQ runs (or settles): bank 1's test for the door call.
    pub fn any_live(&self) -> bool {
        self.parts.iter().any(|p| p.live == Live::Running) || self.smart.live()
    }

    /// Whether Part `p`'s EQ runs: for the tests.
    pub fn running(&self, p: PartId) -> bool {
        self.parts[p.index()].live == Live::Running
    }
}

/// Where the audio keeps `PartEqs`: on the unit `PART_EQS`, an ITCM
/// resident (ADR 0266, 0286; D2, where the `Instrument` lives, has under
/// 1 KB left over its margin, `memory_budget_test`),
/// and this is a token for it, 0 B; on a host the Instrument's own, so
/// each test's Instrument has its EQ.
pub struct EqHome {
    #[cfg(not(target_os = "none"))]
    eqs: PartEqs,
}

// ITCM, NOLOAD: `shared::zero_itcm` leaves it `REST` at boot, before any
// Instrument is built; `EqHome::new` sets it `REST` again for each.
#[cfg(target_os = "none")]
#[unsafe(link_section = ".itcm")]
static mut PART_EQS: PartEqs = PartEqs::REST;

impl EqHome {
    /// Every Part bypassed. Only `Instrument::init_in_place` makes one.
    pub(crate) fn new() -> EqHome {
        #[cfg(target_os = "none")]
        {
            // SAFETY: on the unit the one Instrument is built in place in
            // `INSTRUMENT`'s slot (the engine's, or before it the bench's),
            // after ITCM's boot zeroing; each build replaces the last, whose
            // `EqHome` (and any borrow through it) is gone with it, and no
            // render runs while it is built. So nothing else holds
            // `PART_EQS`.
            unsafe { core::ptr::addr_of_mut!(PART_EQS).write(PartEqs::REST) };
            EqHome {}
        }
        #[cfg(not(target_os = "none"))]
        EqHome { eqs: PartEqs::REST }
    }

    pub fn get(&mut self) -> &mut PartEqs {
        #[cfg(target_os = "none")]
        {
            // SAFETY: one `EqHome` lives at a time, its Instrument's (`new`),
            // and this borrow is tied to `&mut self`, so it is the only one.
            unsafe { &mut *core::ptr::addr_of_mut!(PART_EQS) }
        }
        #[cfg(not(target_os = "none"))]
        &mut self.eqs
    }
}
