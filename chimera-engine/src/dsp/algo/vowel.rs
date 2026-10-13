//! Which of the VOWEL macro's formants an operator follows. The codes are
//! frozen (ADR 0045), and the vowel table, FC's law and the slots
//! (spec §§ 4-5).

use super::formant::FC_MIN;
use super::kinds::Kinds;
use super::math::{clampf, exp2};
use super::plan::OPS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VowelSlot {
    /// By order among the AUTO operators (spec § 5).
    Auto,
    F1,
    F2,
    F3,
    /// Not on the vowel.
    Off,
}

pub const VSLOT_NAMES: [&str; 5] = ["AUTO", "F1", "F2", "F3", "OFF"];
pub const VSLOT_IDENTS: [&str; 5] = VSLOT_NAMES;

impl VowelSlot {
    pub const fn from_code(c: u8) -> Option<VowelSlot> {
        Some(match c {
            0 => VowelSlot::Auto,
            1 => VowelSlot::F1,
            2 => VowelSlot::F2,
            3 => VowelSlot::F3,
            4 => VowelSlot::Off,
            _ => return None,
        })
    }

    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// A vowel's formant: the slot an operator follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formant {
    F1,
    F2,
    F3,
}

/// FC follows the note from here (MIDI 60).
pub const KEY_REF: f32 = 60.0;

/// Peterson & Barney (1952), adult male averages, Hz, in path order
/// U O A E I.
pub static VOWEL_HZ: [[f32; 3]; 5] = [
    [300.0, 870.0, 2240.0],
    [570.0, 840.0, 2410.0],
    [730.0, 1090.0, 2440.0],
    [530.0, 1840.0, 2480.0],
    [270.0, 2290.0, 3010.0],
];

/// `log2(Hz / A's Hz)`, a test recomputes it from `VOWEL_HZ`.
pub static VOWEL_OCT: [[f32; 3]; 5] = [
    [-1.282_934, -0.325_240_83, -0.123_382_41],
    [-0.356_934_55, -0.375_866_9, -0.017_848_001],
    [0.0, 0.0, 0.0],
    [-0.461_904_1, 0.755_377_65, 0.023_458_973],
    [-1.434_937, 1.071_019_5, 0.302_882_34],
];

/// A slot's octave offset from A at VOWEL `vowel` (0-100, 25 a step).
pub fn vowel_delta(slot: Formant, vowel: f32) -> f32 {
    let x = clampf(vowel * 0.04, 0.0, 4.0);
    let i = (x as usize).min(3);
    let (a, b) = (VOWEL_OCT[i][slot as usize], VOWEL_OCT[i + 1][slot as usize]);
    a + (b - a) * (x - i as f32)
}

/// A formant operator's centre, Hz: `FRMT·2^(Δ_slot + KEY·(pitch − 60)/1200)`,
/// floored at 20 Hz. `frmt` 0-168, `vowel` and `key` 0-100.
pub fn fc_hz(frmt: f32, vowel: f32, slot: Option<Formant>, key: f32, pitch: f32) -> f32 {
    let d = slot.map_or(0.0, |s| vowel_delta(s, vowel));
    let o = frmt * (1.0 / 24.0) + d + key * (pitch - KEY_REF) * (1.0 / 1200.0);
    (100.0 * exp2(o)).max(FC_MIN)
}

/// Each operator's slot (spec § 4): tables and OFF have none; AUTO takes
/// F1, F2, F3 by order among the AUTO formant operators, then none.
pub fn resolve_slots(kinds: &Kinds, vslot: [VowelSlot; OPS]) -> [Option<Formant>; OPS] {
    let mut out = [None; OPS];
    let mut autos = 0;
    for (op, slot) in out.iter_mut().enumerate() {
        if !kinds.get(op).is_formant() {
            continue;
        }
        *slot = match vslot[op] {
            VowelSlot::F1 => Some(Formant::F1),
            VowelSlot::F2 => Some(Formant::F2),
            VowelSlot::F3 => Some(Formant::F3),
            VowelSlot::Off => None,
            VowelSlot::Auto => {
                autos += 1;
                [
                    Some(Formant::F1),
                    Some(Formant::F2),
                    Some(Formant::F3),
                    None,
                ][(autos - 1).min(3)]
            }
        };
    }
    out
}
