//! Each Part's channel EQ (ADR 0284, #342): TILT, dark to bright about
//! 810 Hz, and LO CUT, a 150 Hz high-pass, at the end of the Part's
//! channel, after its Sound and before its level, pan and sends. The
//! settings are the project's mix (`PartParams::eq`), never the Sound's.
//! One EQ a Part: SMART EQ (#396) adds its cuts here.

pub mod door;
pub mod dsp;

/// TILT: a step in `MIN_STEP..=MAX_STEP`, each 1/128 of the 20 dB travel.
/// `FLAT` (step 0) is exactly 0 dB; −64 is −10 dB, +63 is +9.84 dB. Its
/// code (CC/NRPN value, the stored digit) is `64 + step`: CC 64 is flat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tilt(i8);

impl Tilt {
    pub const MIN_STEP: i8 = -64;
    pub const MAX_STEP: i8 = 63;
    pub const FLAT: Tilt = Tilt(0);
    /// Steps from flat to the dark end, where the shelves reach `FULL_DB`.
    const STEPS_TO_FULL: f32 = 64.0;
    /// The shelves' reach at the dark end (the Jellymix's ±10 dB).
    pub const FULL_DB: f32 = 10.0;
    /// `code` of `FLAT`.
    const CODE_FLAT: i16 = 64;

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

    /// A stored or knob value (a step as `f32`), rounded and clamped; NaN
    /// is flat.
    pub fn from_f32(v: f32) -> Tilt {
        if v.is_nan() {
            return Tilt::FLAT;
        }
        let lo = f32::from(Self::MIN_STEP);
        let hi = f32::from(Self::MAX_STEP);
        Tilt(libm::roundf(crate::dsp::clampf(v, lo, hi)) as i8)
    }

    pub const fn step(self) -> i8 {
        self.0
    }

    /// The high shelf's gain in dB; the low shelf's is its negative.
    pub fn db(self) -> f32 {
        f32::from(self.0) * (Self::FULL_DB / Self::STEPS_TO_FULL)
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
        match self {
            LoCut::Off => 0,
            LoCut::On => 1,
        }
    }

    pub const fn from_code(c: u8) -> Option<LoCut> {
        match c {
            0 => Some(LoCut::Off),
            1 => Some(LoCut::On),
            _ => None,
        }
    }

    pub const fn ident(self) -> &'static str {
        match self {
            LoCut::Off => "OFF",
            LoCut::On => "ON",
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

impl PartEq {
    /// 0 dB, LO CUT off: what an old project loads, and the bypass.
    pub const FLAT: PartEq = PartEq::new(Tilt::FLAT, LoCut::Off);

    pub const fn new(t: Tilt, c: LoCut) -> PartEq {
        let cut = match c {
            LoCut::Off => 0,
            LoCut::On => LO_CUT_BIT,
        };
        PartEq(t.code() | cut)
    }

    pub const fn from_raw(b: u8) -> PartEq {
        PartEq(b)
    }

    pub const fn raw(self) -> u8 {
        self.0
    }

    pub const fn tilt(self) -> Tilt {
        // Seven bits are a code, 0..=127 (the asserts above).
        Tilt((((self.0 & TILT_BITS) as i16) - Tilt::CODE_FLAT) as i8)
    }

    pub const fn lo_cut(self) -> LoCut {
        if self.0 & LO_CUT_BIT == 0 {
            LoCut::Off
        } else {
            LoCut::On
        }
    }

    pub const fn with_tilt(self, t: Tilt) -> PartEq {
        PartEq::new(t, self.lo_cut())
    }

    pub const fn with_lo_cut(self, c: LoCut) -> PartEq {
        PartEq::new(self.tilt(), c)
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
    pub(crate) const ZERO: Taps = Taps { cut: 0.0, tilt: 0.0 };
}

/// A block's end coefficients (`dsp::Coeffs`): TILT's `k`, `c` and
/// low-pass gain `g`, LO CUT's amount `a`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Coeffs {
    pub(crate) k: f32,
    pub(crate) c: f32,
    pub(crate) g: f32,
    pub(crate) a: f32,
}

/// A Part's EQ as the audio keeps it: the eases (TILT in dB, LO CUT's
/// amount 0..1), the last block's coefficients, and the filters' memory
/// on its bus (`mid`) and a DRUM pad's side (`side`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelEq {
    pub(crate) tilt: crate::dsp::ease::Ease,
    pub(crate) cut: crate::dsp::ease::Ease,
    pub(crate) at: Coeffs,
    pub(crate) mid: Taps,
    pub(crate) side: Taps,
    pub(crate) live: Live,
}

/// Every Part's EQ state: on the unit an ITCM resident (`home`), on a host
/// the Instrument's own. All-zero bytes are `REST`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartEqs {
    pub(crate) parts: [ChannelEq; crate::hw::MAX_PARTS],
}

impl PartEqs {
    /// Every Part bypassed: the bytes ITCM's boot zeroing leaves.
    pub const REST: PartEqs = PartEqs {
        parts: [ChannelEq {
            tilt: crate::dsp::ease::Ease::ZERO,
            cut: crate::dsp::ease::Ease::ZERO,
            at: Coeffs {
                k: 0.0,
                c: 0.0,
                g: 0.0,
                a: 0.0,
            },
            mid: Taps::ZERO,
            side: Taps::ZERO,
            live: Live::Bypass,
        }; crate::hw::MAX_PARTS],
    };

    /// A Part's EQ runs (or settles): bank 1's test for the door call.
    pub fn any_live(&self) -> bool {
        self.parts.iter().any(|p| p.live == Live::Running)
    }

    /// Whether Part `p`'s EQ runs: for the tests.
    pub fn running(&self, p: usize) -> bool {
        self.parts[p].live == Live::Running
    }
}

/// Where the audio keeps `PartEqs`: on the unit `PART_EQS`, an ITCM
/// resident (ADR 0266, 0286; D2, where the `Instrument` lives, is full),
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
        EqHome {
            eqs: PartEqs::REST,
        }
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
