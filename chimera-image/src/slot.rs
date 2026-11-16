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

    /// The tag word, the slot's second.
    pub const fn tag_addr(self) -> u32 {
        self.base() + 0x20
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

    /// The set whose bits are `b`'s low two.
    pub(crate) const fn from_bits(b: u8) -> SlotSet {
        SlotSet(b & SlotSet::ALL.0)
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

/// One flash word as a masked read saw it: `ecc` is a double-ECC flag
/// (SR2.DBECCERR), `corrected` a silent single-bit fix (SR2.SNECCERR).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    pub bytes: [u8; 32],
    pub ecc: bool,
    pub corrected: bool,
}

impl Word {
    pub const ERASED: Word = Word {
        bytes: [0xFF; 32],
        ecc: false,
        corrected: false,
    };

    fn clean(&self) -> bool {
        !self.ecc && !self.corrected
    }

    fn erased(&self) -> bool {
        self.clean() && self.bytes == [0xFF; 32]
    }

    /// What a torn H7 erase leaves: no vector table starts SP 0, reset 0.
    fn zeroed(&self) -> bool {
        self.clean() && self.bytes == [0; 32]
    }

    /// `want`, or `want` torn: a program only clears bits, so every 1 in
    /// `want` is still 1, unless ECC flagged or flipped one. A bit-superset
    /// of the code word passes too, but its reset then carries `base + 9`'s
    /// bits: S4..S7 (which must hold ours), or memory unmapped or not set up
    /// at reset. No working trampoline into system memory, RAM or bank 1 can
    /// pass.
    fn may_be(&self, want: &[u8; 32]) -> bool {
        !self.clean() || self.bytes.iter().zip(want).all(|(w, e)| w & e == *e)
    }
}

/// Flash read for the guard: one masked read of the 32-byte word at `addr`.
pub trait SlotRead {
    fn word(&mut self, addr: u32) -> Word;
}

/// How the guard took a slot as ours; the install picks its order from it
/// so that one clean tag or marker survives at every instant (spec § 3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// S4's tag word read clean.
    Tag,
    /// S5's marker read clean, S4's code word ours, torn, erased or zeroed.
    Marker,
    /// Rule (c): nothing but the three Chimera words, maybe torn; a flag
    /// per sector that read wholly 0xFF.
    Scan { s4_erased: bool, s5_erased: bool },
    /// Taken as ours from the boot's slot mark, unread.
    Mark,
}

/// What the guard found, per slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotPlan([Evidence; 2]);

impl SlotPlan {
    pub const fn evidence(&self, s: Slot) -> Evidence {
        self.0[s.index() as usize]
    }

    /// The slots whose two sectors read fully erased.
    pub fn erased(&self) -> SlotSet {
        let all = Evidence::Scan {
            s4_erased: true,
            s5_erased: true,
        };
        Slot::ALL
            .into_iter()
            .filter(|&s| self.evidence(s) == all)
            .fold(SlotSet::NONE, SlotSet::with)
    }
}

/// The slot holds code that isn't Chimera's: nothing may be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotForeign(pub Slot);

/// Judge each slot not in `ours` before anything is written (spec § 3.2).
/// A clean tag, or a clean marker with S4's code word ours, torn or zeroed, makes
/// it ours; otherwise every word must be erased, bar the three Chimera
/// writes, which may be torn. The stub's code bytes never change.
pub fn guard(r: &mut impl SlotRead, ours: SlotSet) -> Result<SlotPlan, SlotForeign> {
    let mut e = [Evidence::Mark; 2];
    for s in Slot::ALL {
        if !ours.has(s) {
            e[s.index() as usize] = judge(r, s).ok_or(SlotForeign(s))?;
        }
    }
    Ok(SlotPlan(e))
}

fn judge(r: &mut impl SlotRead, s: Slot) -> Option<Evidence> {
    let t = r.word(s.tag_addr());
    if t.clean() && t.bytes[0x10..0x1C] == *TAG {
        return Some(Evidence::Tag);
    }
    let m = r.word(s.marker_addr());
    // A marker alone outlives a revert that refilled S4 with other code;
    // a torn erase leaves S4 zeroed, ECC valid (unit, 2026-11-16).
    if m.clean() && m.bytes[..16] == *MARKER {
        let code = r.word(s.base());
        if code.may_be(&code_word(s)) || code.zeroed() {
            return Some(Evidence::Marker);
        }
    }
    let (s4_erased, s5_erased) = scan(r, s)?;
    Some(Evidence::Scan {
        s4_erased,
        s5_erased,
    })
}

/// The three words only Chimera writes in a slot, with their bytes: code,
/// tag, marker.
pub const fn own_words(s: Slot) -> [(u32, [u8; 32]); 3] {
    [
        (s.base(), code_word(s)),
        (s.tag_addr(), tag_word(s)),
        (s.marker_addr(), end_marker(s)),
    ]
}

/// (S4, S5) each wholly erased if the slot may be ours; None if foreign.
fn scan(r: &mut impl SlotRead, s: Slot) -> Option<(bool, bool)> {
    let own = own_words(s);
    let mut erased = [true; 2];
    for addr in (s.base()..s.base() + SLOT_LEN).step_by(32) {
        let w = r.word(addr);
        let ok = match own.iter().find(|(a, _)| *a == addr) {
            Some((_, want)) => w.may_be(want),
            None => w.erased(),
        };
        if !ok {
            return None;
        }
        erased[((addr - s.base()) / (SLOT_LEN / 2)) as usize] &= w.erased();
    }
    Some((erased[0], erased[1]))
}
