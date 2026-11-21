//! The master compressor's KEY (ADR 0303): what its detector listens to.
//! FULL is the compressor as it was (`MasterComp::process`, bank 1's, bit
//! for bit). LOW, HIGH and HIT filter the detector's copy of the mix and
//! never the audio; PART n keys it from that Part's bus, after its EQ and
//! before its LEVEL, and passes that Part unducked. All but FULL run in
//! bank 2 (`kern`, an S4 group) behind `door::KeyDoor`; a closed door
//! plays FULL.

pub mod door;
pub mod kern;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::Hz;
use crate::dsp::comp::{CompParams, MasterComp};
use crate::dsp::ease::Ease;
use crate::hw::{Cost, DAC_PAIRS, MAX_PARTS};
use crate::instrument::{MIX_GAINS, PartAudio, Sides};
use crate::project::PartId;

/// KEY: the detector's source. FULL, the three ranges, then the Parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CompKey {
    /// The whole mix: zero bytes, the default.
    Full = 0,
    /// The mix under `LOW_CORNER`: the kick pumps it.
    Low,
    /// The mix over `HIGH_CORNER`: the bass no longer pumps it.
    High,
    /// The mix K-weighted (ITU-R BS.1770): every hit pushes alike.
    Hit,
    /// That Part's bus, which then passes unducked.
    Part(PartId),
}

/// LOW's corner, a 12 dB/octave Butterworth low-pass.
pub const LOW_CORNER: Hz = Hz::new(150.0);
/// HIGH's corner, the matching high-pass.
pub const HIGH_CORNER: Hz = Hz::new(150.0);

/// The codes before the Parts'.
const RANGES: u8 = 4;
/// KEY's choices in all.
pub const KEYS: usize = RANGES as usize + MAX_PARTS;

/// Each key with its frozen disk ident (ADR 0045) and its name on the
/// box, in code order (checked below). Append-only.
const KEY_TABLE: [(CompKey, &str, &str); KEYS] = [
    (CompKey::Full, "FULL", "FULL"),
    (CompKey::Low, "LOW", "LOW"),
    (CompKey::High, "HIGH", "HIGH"),
    (CompKey::Hit, "HIT", "HIT"),
    (CompKey::Part(PartId::ALL[0]), "PART1", "PART 1"),
    (CompKey::Part(PartId::ALL[1]), "PART2", "PART 2"),
    (CompKey::Part(PartId::ALL[2]), "PART3", "PART 3"),
    (CompKey::Part(PartId::ALL[3]), "PART4", "PART 4"),
    (CompKey::Part(PartId::ALL[4]), "PART5", "PART 5"),
    (CompKey::Part(PartId::ALL[5]), "PART6", "PART 6"),
];

// Row `c` is code `c`: `ident` and `KEY_NAMES` index the table by code.
const _: () = {
    let mut c = 0;
    while c < KEYS {
        assert!(
            KEY_TABLE[c].0.code() as usize == c,
            "KEY_TABLE is in code order"
        );
        c += 1;
    }
};

/// KEY's names, by code.
pub static KEY_NAMES: [&str; KEYS] = {
    let mut n = [""; KEYS];
    let mut c = 0;
    while c < KEYS {
        n[c] = KEY_TABLE[c].2;
        c += 1;
    }
    n
};

static KEY_IDENTS: [&str; KEYS] = {
    let mut n = [""; KEYS];
    let mut c = 0;
    while c < KEYS {
        n[c] = KEY_TABLE[c].1;
        c += 1;
    }
    n
};

impl CompKey {
    /// Every key, in code order.
    pub const ALL: [CompKey; KEYS] = {
        let mut a = [CompKey::Full; KEYS];
        let mut c = 0;
        while c < KEYS {
            a[c] = KEY_TABLE[c].0;
            c += 1;
        }
        a
    };

    /// The stored code: frozen, the Parts' after the ranges'.
    pub const fn code(self) -> u8 {
        match self {
            CompKey::Full => 0,
            CompKey::Low => 1,
            CompKey::High => 2,
            CompKey::Hit => 3,
            CompKey::Part(p) => RANGES + p.index() as u8,
        }
    }

    pub const fn from_code(c: u8) -> Option<CompKey> {
        match c {
            0 => Some(CompKey::Full),
            1 => Some(CompKey::Low),
            2 => Some(CompKey::High),
            3 => Some(CompKey::Hit),
            _ => match PartId::new(c - RANGES) {
                Some(p) => Some(CompKey::Part(p)),
                None => None,
            },
        }
    }

    /// A knob's or a stored value, rounded; NaN and below are FULL, past
    /// the last Part is the last Part.
    pub fn from_f32(v: f32) -> CompKey {
        let c = (v + 0.5) as u8;
        match CompKey::from_code(c) {
            Some(k) => k,
            None => CompKey::Part(PartId::LAST),
        }
    }

    pub fn ident(self) -> &'static str {
        KEY_IDENTS[usize::from(self.code())]
    }

    pub fn name(self) -> &'static str {
        KEY_NAMES[usize::from(self.code())]
    }

    /// The Part it keys from, which passes unducked.
    pub const fn part(self) -> Option<PartId> {
        match self {
            CompKey::Part(p) => Some(p),
            CompKey::Full | CompKey::Low | CompKey::High | CompKey::Hit => None,
        }
    }
}

/// What KEY reserves from the budget beside the bus (ADR 0303), cycles a
/// sample: nothing at FULL, so every bill is then as before. Each covers
/// what the keyed loop runs past FULL's, counted in the ORBIT build's
/// `kern::run` at 1.606 cycles an instruction (`tools/loop_count.py`'s
/// factor), with room; ADR 0303 has the counts. Bank 1's: admission runs
/// with the door open or closed (closed, FULL plays and these over-bill),
/// and whether or not the compressor is on.
pub mod bill {
    use super::CompKey;
    use crate::hw::Cost;

    /// LOW or HIGH: the detector's left and right through one section.
    pub const RANGE: Cost = Cost(52);
    /// HIT: through two.
    pub const HIT: Cost = Cost(72);
    /// PART n: its pass on its pair, a DRUM Part's side with it, less
    /// what its bus's peak saves on the mix's.
    pub const PART: Cost = Cost(56);

    pub const fn of(key: CompKey) -> Cost {
        match key {
            CompKey::Full => Cost::ZERO,
            CompKey::Low | CompKey::High => RANGE,
            CompKey::Hit => HIT,
            CompKey::Part(_) => PART,
        }
    }
}

/// One second-order section, `a0` 1: `y = b0 x + b1 x₁ + b2 x₂ − a1 y₁ −
/// a2 y₂`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Biquad {
    pub b: [f32; 3],
    pub a: [f32; 2],
}

impl Biquad {
    /// Zero bytes: silence out.
    const ZERO: Biquad = Biquad {
        b: [0.0; 3],
        a: [0.0; 2],
    };
}

/// A section's memory for one channel (transposed direct form II).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(transparent)]
struct Memory([f32; 2]);

impl Memory {
    const ZERO: Memory = Memory([0.0; 2]);
}

/// The sections a ranged key runs the detector through.
const SECTIONS: usize = 2;

/// The Part whose bus is put back after the gain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Passing {
    /// Zero bytes: at rest.
    Nobody = 0,
    Part(PartId),
}

/// Whether the passing Part's gains at the last block's end are held, so
/// this block's ramps start where the mix's did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Held {
    /// Zero bytes: its first block, whose ramps start at their ends.
    Fresh = 0,
    Held,
}

/// Whether bank 2 still has a pass to fade out with KEY back at FULL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum KeyLoad {
    /// Zero bytes: FULL needs no call.
    Rests = 0,
    Runs,
}

/// How much of the passing Part is put back: 0 none ..= 1 all of what
/// the gain took. It fades over `PASS_FADE_S`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[repr(transparent)]
struct Weight(f32);

impl Weight {
    const NONE: Weight = Weight(0.0);
    const ALL: Weight = Weight(1.0);
}

/// The pass's fade in and out, seconds.
pub const PASS_FADE_S: f32 = 0.01;

/// KEY's state, in the compressor (AXI): the ranged detector's sections
/// and their memory, and the pass. Bank 2's `kern` alone runs it; bank 1
/// only asks whether it is at rest. Zero bytes are `REST`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct KeyState {
    /// The sections, made for `made` at `rate`.
    sections: [Biquad; SECTIONS],
    /// Each section's memory, left and right.
    memory: [[Memory; 2]; SECTIONS],
    rate: u32,
    /// The passing Part's left and right gains at the last block's end.
    gains: [f32; 2],
    weight: Weight,
    /// The ranged key the sections were made for; FULL, none.
    made: CompKey,
    passing: Passing,
    held: Held,
    load: KeyLoad,
}

impl KeyState {
    pub const REST: KeyState = KeyState {
        sections: [Biquad::ZERO; SECTIONS],
        memory: [[Memory::ZERO; 2]; SECTIONS],
        rate: 0,
        gains: [0.0; 2],
        weight: Weight::NONE,
        made: CompKey::Full,
        passing: Passing::Nobody,
        held: Held::Fresh,
        load: KeyLoad::Rests,
    };

    /// How much of the passing Part is put back now, 0..=1: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pass_weight(&self) -> f32 {
        self.weight.0
    }
}

/// Whether the compressor's block is bank 2's to play: any KEY but FULL,
/// or a pass still fading out under FULL.
#[inline(always)]
pub fn asked(p: &CompParams, comp: &MasterComp) -> bool {
    p.key != CompKey::Full || comp.key_state.load != KeyLoad::Rests
}

/// The Parts as the mix has them when the master section runs: what PART
/// n listens to and puts back.
pub struct Tap<'a> {
    /// Each Part's bus, after its EQ and before its LEVEL.
    pub buses: &'a [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a Sides,
    pub written: &'a [bool; MAX_PARTS],
    /// Each Part's eased gains at this block's end.
    pub gains: &'a [[Ease; MIX_GAINS]; MAX_PARTS],
    /// For each Part's output pair.
    pub parts: &'a [PartAudio; MAX_PARTS],
}

/// The block as bank 1 hands it to `kern::run`.
pub struct Ask<'a, 'b> {
    pub comp: &'a mut MasterComp,
    pub out: &'a mut [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS],
    pub params: &'a CompParams,
    pub sample_rate: u32,
    /// `None` where no Parts are (the bus alone): PART n hears silence.
    pub tap: Option<&'a Tap<'b>>,
}

/// What the compressor did to the pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Pairs {
    /// Bypassed: every sample as it was.
    Untouched,
    Scaled,
}

impl Pairs {
    #[inline(always)]
    pub const fn of(ran: bool) -> Pairs {
        if ran { Pairs::Scaled } else { Pairs::Untouched }
    }
}

const _: () = assert!(bill::of(CompKey::Full).0 == Cost::ZERO.0);

#[cfg(test)]
mod tests {
    use super::*;

    /// `MasterComp::init_in_place` zeroes the compressor: zero bytes are
    /// KEY at rest.
    #[test]
    fn zero_bytes_are_rest() {
        // SAFETY: every field is an `f32`, a `u32` or a `repr(u8)` enum
        // whose first variant is 0.
        let zero: KeyState = unsafe { core::mem::zeroed() };
        assert_eq!(zero, KeyState::REST);
        // SAFETY: `repr(u8)`, and 0 is `Full`'s tag.
        let p: CompKey = unsafe { core::mem::zeroed() };
        assert_eq!(p, CompKey::Full);
        assert!(size_of::<KeyState>() <= 96);
    }
}
