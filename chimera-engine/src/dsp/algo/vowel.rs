//! Which of the VOWEL macro's formants an operator follows. The codes are
//! frozen (ADR 0045).

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
