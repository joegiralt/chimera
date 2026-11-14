//! The two slot stubs and their end markers (ADR 0272): bootloader 1.09
//! jumps to 0x08180000 with MIXER held at power-on and to 0x081C0000 with
//! EDIT held; each stub restarts into bank 1 at 0x08020000, VTOR set.

use crate::header::BANK2_BASE;
use crate::install::Bank2Sector;

/// Found at 0x30 of each slot: the guard reads it by its text.
pub const TAG: &[u8; 12] = b"CHIMERA-SLOT";
/// Found on each slot's last flash word.
pub const MARKER: &[u8; 16] = b"CHIMERA-SLOT-END";
/// The stub's room: a full H7 vector table at VTOR's 1 KB alignment.
pub const RESERVE: u32 = 1024;

const VERSION: u8 = 1;
const SLOT_LEN: u32 = 0x4_0000;
/// DTCM's top: the stub uses no stack.
const STUB_SP: u32 = 0x2002_0000;
const BANK1: u32 = 0x0802_0000;
const SCB_VTOR: u32 = 0xE000_ED08;

/// The stub's code, bytes 8..32 of the code word, as Thumb-2 halfwords.
/// The two loads read the literals at 0x20 and 0x24 (the tag word).
const CODE: [u16; 12] = [
    0x4805, // ldr  r0, [pc, #20]  0x08020000 (literal at 0x20)
    0x4906, // ldr  r1, [pc, #24]  0xE000ED08 (literal at 0x24)
    0x6008, // str  r0, [r1]       VTOR <- bank 1
    0xF3BF, 0x8F4F, // dsb sy
    0xF3BF, 0x8F6F, // isb sy
    0x6801, // ldr  r1, [r0]       bank 1's SP
    0x6842, // ldr  r2, [r0, #4]   bank 1's reset
    0xF381, 0x8808, // msr msp, r1
    0x4710, // bx   r2
];

/// The slot a held button at power-on picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Slot 9, MIXER held: S4 and S5.
    Mixer,
    /// Slot 10, EDIT held: S6 and S7.
    Edit,
}

impl Slot {
    pub const ALL: [Slot; 2] = [Slot::Mixer, Slot::Edit];

    pub const fn index(self) -> u8 {
        self as u8
    }

    pub const fn base(self) -> u32 {
        BANK2_BASE + 0x8_0000 + self.index() as u32 * SLOT_LEN
    }

    /// The end marker's flash word, the slot's last.
    pub const fn marker_addr(self) -> u32 {
        self.base() + SLOT_LEN - 32
    }

    /// (entry, end): the stub's sector and the marker's.
    pub const fn sectors(self) -> (Bank2Sector, Bank2Sector) {
        match self {
            Slot::Mixer => (Bank2Sector::S4, Bank2Sector::S5),
            Slot::Edit => (Bank2Sector::S6, Bank2Sector::S7),
        }
    }

    const fn bit(self) -> u8 {
        1 << self.index()
    }
}

/// A set of slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotSet(u8);

impl SlotSet {
    pub const NONE: SlotSet = SlotSet(0);
    pub const ALL: SlotSet = SlotSet(Slot::Mixer.bit() | Slot::Edit.bit());

    pub const fn has(self, s: Slot) -> bool {
        self.0 & s.bit() != 0
    }

    pub const fn with(self, s: Slot) -> SlotSet {
        SlotSet(self.0 | s.bit())
    }

    /// Bit `index` per slot; only the low two bits are ever set.
    pub const fn bits(self) -> u8 {
        self.0
    }
}

const fn put(w: &mut [u8; 32], at: usize, b: &[u8]) {
    let mut i = 0;
    while i < b.len() {
        w[at + i] = b[i];
        i += 1;
    }
}

/// The slot's first flash word: initial SP, reset (Thumb) and the code.
pub const fn code_word(s: Slot) -> [u8; 32] {
    let mut w = [0u8; 32];
    put(&mut w, 0, &STUB_SP.to_le_bytes());
    put(&mut w, 4, &(s.base() + 9).to_le_bytes());
    let mut i = 0;
    while i < CODE.len() {
        put(&mut w, 8 + 2 * i, &CODE[i].to_le_bytes());
        i += 1;
    }
    w
}

/// The slot's second flash word: the code's literals, then the tag.
pub const fn tag_word(s: Slot) -> [u8; 32] {
    let mut w = [0xFFu8; 32];
    put(&mut w, 0, &BANK1.to_le_bytes());
    put(&mut w, 4, &SCB_VTOR.to_le_bytes());
    put(&mut w, 0x10, TAG);
    w[0x1C] = VERSION;
    w[0x1D] = s.index();
    w
}

/// The slot's last flash word, at `marker_addr`.
pub const fn end_marker(s: Slot) -> [u8; 32] {
    let mut w = [0xFFu8; 32];
    put(&mut w, 0, MARKER);
    w[16] = VERSION;
    w[17] = s.index();
    w
}
