// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

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

use crate::dsp::comp::{CompParams, MasterComp};
use crate::dsp::ease::step_of;
use crate::dsp::{Hz, clampf};
use crate::hw::{Cost, DAC_PAIRS, MAX_PARTS};
use crate::instrument::{PartAudio, Sides};
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

/// The mix through fixed sections: LOW, HIGH or HIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ranged {
    Low,
    High,
    Hit,
}

/// What a key has the detector hear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The whole mix: FULL.
    Mix,
    Ranged(Ranged),
    Part(PartId),
}

/// Each key with its name on the box, in code order (checked below): the
/// stored codes, frozen and append-only (ADR 0045). A range's disk ident
/// is its name; a Part's is its name without the space (`part_ident`),
/// since an ident is one token.
const KEY_TABLE: [(CompKey, &str); KEYS] = [
    (CompKey::Full, "FULL"),
    (CompKey::Low, "LOW"),
    (CompKey::High, "HIGH"),
    (CompKey::Hit, "HIT"),
    (CompKey::Part(PartId::ALL[0]), "PART 1"),
    (CompKey::Part(PartId::ALL[1]), "PART 2"),
    (CompKey::Part(PartId::ALL[2]), "PART 3"),
    (CompKey::Part(PartId::ALL[3]), "PART 4"),
    (CompKey::Part(PartId::ALL[4]), "PART 5"),
    (CompKey::Part(PartId::ALL[5]), "PART 6"),
];

// Row `c` is code `c`: `from_code` and `KEY_NAMES` index the table by code.
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
        n[c] = KEY_TABLE[c].1;
        c += 1;
    }
    n
};

/// The Parts' disk idents: `PART1` ..= `PART6`, each its name less the
/// space (checked below).
static PART_IDENTS: [&str; MAX_PARTS] = ["PART1", "PART2", "PART3", "PART4", "PART5", "PART6"];

// A Part's ident is its name's bytes but the space.
const _: () = {
    let mut p = 0;
    while p < MAX_PARTS {
        let (name, ident) = (
            KEY_TABLE[RANGES as usize + p].1.as_bytes(),
            PART_IDENTS[p].as_bytes(),
        );
        assert!(name.len() == ident.len() + 1);
        let (mut i, mut j) = (0, 0);
        while i < name.len() {
            if name[i] != b' ' {
                assert!(
                    name[i] == ident[j],
                    "a Part's ident is its name less the space"
                );
                j += 1;
            }
            i += 1;
        }
        assert!(j == ident.len());
        p += 1;
    }
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

    /// The key stored as `c`: the table's row.
    pub const fn from_code(c: u8) -> Option<CompKey> {
        if (c as usize) < KEYS {
            Some(KEY_TABLE[c as usize].0)
        } else {
            None
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

    /// What the detector hears.
    #[inline(always)]
    pub const fn source(self) -> Source {
        match self {
            CompKey::Full => Source::Mix,
            CompKey::Low => Source::Ranged(Ranged::Low),
            CompKey::High => Source::Ranged(Ranged::High),
            CompKey::Hit => Source::Ranged(Ranged::Hit),
            CompKey::Part(p) => Source::Part(p),
        }
    }

    pub fn ident(self) -> &'static str {
        match self.source() {
            Source::Part(p) => PART_IDENTS[p.index()],
            Source::Mix | Source::Ranged(_) => self.name(),
        }
    }

    pub fn name(self) -> &'static str {
        KEY_NAMES[usize::from(self.code())]
    }

    /// The Part it keys from, which passes unducked.
    pub const fn part(self) -> Option<PartId> {
        match self.source() {
            Source::Part(p) => Some(p),
            Source::Mix | Source::Ranged(_) => None,
        }
    }

    /// What plays: the key itself with bank 2's door open, FULL closed.
    #[inline(always)]
    pub fn in_effect(self, door: Door) -> CompKey {
        match door {
            Door::Open => self,
            Door::Closed => CompKey::Full,
        }
    }
}

/// Bank 2's door as KEY finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    Open,
    Closed,
}

/// What KEY reserves from the budget beside the bus (ADR 0303), cycles a
/// sample: nothing at FULL, so every bill is then as before. Each covers
/// what the keyed loop runs past FULL's, counted in the ORBIT build's
/// `kern::run` at 1.606 cycles an instruction (`tools/loop_count.py`'s
/// factor), with room; ADR 0303 has the counts. Bank 1's: admission runs
/// with the door open or closed (closed, FULL plays and these over-bill),
/// and whether or not the compressor is on.
pub mod bill {
    use super::{CompKey, Ranged, Source};
    use crate::hw::Cost;

    /// LOW or HIGH: the detector's left and right through one section.
    pub const RANGE: Cost = Cost(52);
    /// HIT: through two.
    pub const HIT: Cost = Cost(72);
    /// PART n: its pass on its pair, a DRUM Part's side with it, less
    /// what its bus's peak saves on the mix's.
    pub const PART: Cost = Cost(56);

    pub const fn of(key: CompKey) -> Cost {
        match key.source() {
            Source::Mix => Cost::ZERO,
            Source::Ranged(Ranged::Low | Ranged::High) => RANGE,
            Source::Ranged(Ranged::Hit) => HIT,
            Source::Part(_) => PART,
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

/// How much of the passing Part is put back: 0 none ..= 1 all of what
/// the gain took. It fades over `PASS_FADE_S`. Only `new` makes one, so
/// every one is in range.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Weight(f32);

impl Weight {
    pub const NONE: Weight = Weight(0.0);
    pub const ALL: Weight = Weight(1.0);

    /// `w` held to 0..=1; NaN is none.
    #[inline(always)]
    pub fn new(w: f32) -> Weight {
        Weight(if w.is_nan() { 0.0 } else { clampf(w, 0.0, 1.0) })
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Who is put back after the gain, and by how much: the pass and its
/// fade as one value, so nobody passing has no weight to hold.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Passing {
    /// Zero bytes: at rest.
    Nobody = 0,
    Part {
        id: PartId,
        weight: Weight,
    },
}

/// What the ranged detector's sections were made for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Made {
    /// Zero bytes: no ranged key has run since the rest.
    Nothing = 0,
    For {
        key: Ranged,
        rate: u32,
    },
}

/// The pass's fade in and out, seconds.
pub const PASS_FADE_S: f32 = 0.01;

/// KEY's state, in the compressor (AXI): the ranged detector's sections
/// and their memory, and the pass. Bank 2's `kern` alone runs it; bank 1
/// only asks whether it is at rest. Zero bytes are `REST`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct KeyState {
    /// The sections, as `made` says.
    sections: [Biquad; SECTIONS],
    /// Each section's memory, left and right.
    memory: [[Memory; 2]; SECTIONS],
    made: Made,
    passing: Passing,
}

impl KeyState {
    pub const REST: KeyState = KeyState {
        sections: [Biquad::ZERO; SECTIONS],
        memory: [[Memory::ZERO; 2]; SECTIONS],
        made: Made::Nothing,
        passing: Passing::Nobody,
    };

    /// Nobody passing: under FULL, nothing for bank 2 to do.
    #[inline(always)]
    pub fn at_rest(&self) -> bool {
        matches!(self.passing, Passing::Nobody)
    }

    /// Who passes, and by how much.
    pub fn passing(&self) -> Passing {
        self.passing
    }
}

/// Whether the compressor's block is bank 2's to play: any KEY but FULL,
/// or a pass still fading out under FULL.
#[inline(always)]
pub(crate) fn asked(p: &CompParams, comp: &MasterComp) -> bool {
    p.key != CompKey::Full || !comp.key_state().at_rest()
}

/// A Part's share of its pair this block, as the mix plays it: its left
/// and right gains (pan, LEVEL and expression) at the block's start and
/// end. `mix_sided` fills it from the gains it mixes with, so the pass
/// puts back exactly what went in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartShare {
    /// `(from, to)`.
    pub left: (f32, f32),
    pub right: (f32, f32),
}

impl PartShare {
    /// The Part's left and right in its pair at sample `i`, from its bus
    /// (`mid`) and side: each gain at the block's start plus its step
    /// `i + 1` times, as `mix_sided`'s passes and `glide` sum them.
    #[inline(always)]
    pub fn at(&self, i: usize, mid: f32, side: f32) -> (f32, f32) {
        let n = (i + 1) as f32;
        let gain = |(from, to): (f32, f32)| from + step_of((from, to), BLOCK_SIZE) * n;
        (
            (mid - side) * gain(self.left),
            (mid + side) * gain(self.right),
        )
    }
}

/// The Parts as the mix has them when the master section runs: what PART
/// n listens to and puts back.
pub(crate) struct Tap<'a> {
    /// Each Part's bus, after its EQ and before its LEVEL.
    pub(crate) buses: &'a [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub(crate) sides: &'a Sides,
    /// Each Part's share of its pair; `None` with no voices this block.
    pub(crate) shares: &'a [Option<PartShare>; MAX_PARTS],
    /// For each Part's output pair.
    pub(crate) parts: &'a [PartAudio; MAX_PARTS],
}

/// The block as bank 1 hands it to `kern::run`.
pub(crate) struct Ask<'a, 'b> {
    pub(crate) comp: &'a mut MasterComp,
    pub(crate) out: &'a mut [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS],
    pub(crate) params: &'a CompParams,
    pub(crate) sample_rate: u32,
    /// `None` where no Parts are (the bus alone): PART n hears silence.
    pub(crate) tap: Option<&'a Tap<'b>>,
}

/// What the master section did to the pairs.
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
        // SAFETY: every field is an `f32` or a `repr(u8)` enum whose
        // first variant is 0 and holds nothing.
        let zero: KeyState = unsafe { core::mem::zeroed() };
        assert_eq!(zero, KeyState::REST);
        assert!(zero.at_rest());
        // SAFETY: `repr(u8)`, and 0 is `Full`'s tag.
        let p: CompKey = unsafe { core::mem::zeroed() };
        assert_eq!(p, CompKey::Full);
        assert!(size_of::<KeyState>() <= 96);
    }

    #[test]
    fn a_weight_is_always_in_range() {
        for w in [
            -1.0,
            -0.0,
            0.0,
            0.3,
            1.0,
            1.0001,
            7.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            let g = Weight::new(w).get();
            assert!((0.0..=1.0).contains(&g), "{w} -> {g}");
        }
        assert_eq!(Weight::new(f32::NAN), Weight::NONE);
    }

    #[test]
    fn idents_are_tokens_and_codes_are_the_table() {
        for (c, k) in CompKey::ALL.into_iter().enumerate() {
            assert_eq!(CompKey::from_code(c as u8), Some(k));
            assert!(!k.ident().contains(' '), "{k:?}");
        }
        for c in KEYS as u8..=u8::MAX {
            assert_eq!(CompKey::from_code(c), None, "{c}");
        }
    }
}
