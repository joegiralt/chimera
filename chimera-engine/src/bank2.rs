//! Bank 2's side of the link (ADR 0137) and bank 1's state of its door.
//!
//! With `bank2`, the header placeholder and the door table land in the
//! `.bank2` section at 0x08100000 (`chimera-stm32/build.rs`), which the
//! Justfile splits into the `.bk2`. Bank 1 never names them: it reaches the
//! door only through the address a judged `Door` carries.

use chimera_image::{BANK2_BASE, Door, HEADER_LEN};
use core::sync::atomic::{AtomicUsize, Ordering};

pub const DOOR_MAGIC: u32 = 0xB2D0_0001;
pub const TABLE_LEN: usize = 4096;
/// `probe(x)` is `x + PROBE_ADDS`.
pub const PROBE_ADDS: u32 = 42;
/// Where the link puts the door: right after the header (`bank2.x` asserts
/// `__bank2_door` is this).
pub const DOOR_ADDR: u32 = BANK2_BASE + HEADER_LEN;

/// The door table: what bank 1 may call or read in bank 2. Reached only
/// inside `DoorCell::with`: never copy `probe` or `table` out of it, they
/// point into bank 2, which an install erases.
#[repr(C)]
pub struct Bank2Door {
    pub magic: u32,
    pub probe: extern "Rust" fn(u32) -> u32,
    /// Byte `i` is `i as u8 ^ 0xB2`.
    pub table: &'static [u8; TABLE_LEN],
    /// TAPE's deck (`TapeEngine` is bank 1's proxy of it).
    #[cfg(feature = "bank2")]
    pub tape: crate::dsp::tape_engine::TapeDoor,
    /// The reverb's engines (ADR 0142; `Reverb::play` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub reverb: crate::dsp::rev::ReverbDoor,
    /// The VA's `MOD` oscillators (ADR 0163; `VaEngine::render_fm` is
    /// bank 1's side).
    #[cfg(feature = "bank2")]
    pub va: crate::dsp::va::osc::door::VaDoor,
    /// The LFO page's glyphs and scope curve (ADR 0177; `lfo_glyph::paint`
    /// and `curve` are bank 1's side).
    #[cfg(feature = "bank2")]
    pub lfo: crate::lfo_glyph::LfoDoor,
    /// FLT › FX's curve (`voice_fx::look::trace` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub look: crate::dsp::voice_fx::look::LookDoor,
    /// FUZZ's kernel (ADR 0157; `Fuzz::play` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub fuzz: crate::dsp::voice_fx::fuzz::FuzzDoor,
}

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.header")]
#[used]
static HEADER: [u8; 256] = chimera_image::UNSEALED_HEADER;

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door")]
#[used]
static DOOR: Bank2Door = Bank2Door {
    magic: DOOR_MAGIC,
    probe,
    table: &TABLE,
    tape: crate::dsp::tape_engine::TapeDeck::DOOR,
    reverb: crate::dsp::rev::Reverb::DOOR,
    va: crate::dsp::va::osc::door::DOOR,
    lfo: crate::lfo_glyph::LfoDoor::DOOR,
    look: crate::dsp::voice_fx::look::LookDoor::DOOR,
    fuzz: crate::dsp::voice_fx::fuzz::Fuzz::DOOR,
};

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.text.probe")]
#[inline(never)]
fn probe(x: u32) -> u32 {
    x.wrapping_add(PROBE_ADDS)
}

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.rodata.table")]
static TABLE: [u8; TABLE_LEN] = {
    let mut t = [0u8; TABLE_LEN];
    let mut i = 0;
    while i < TABLE_LEN {
        t[i] = i as u8 ^ 0xB2;
        i += 1;
    }
    t
};

/// Bank 1's state of the door: closed (0) or the open door's address.
/// Close it before anything erases or programs bank 2.
pub struct DoorCell(AtomicUsize);

impl DoorCell {
    pub const fn new() -> Self {
        DoorCell(AtomicUsize::new(0))
    }

    /// Opens on a judged door at this link's `DOOR_ADDR`; a door anywhere
    /// else (a header the seal does not cover, gone wrong) leaves it closed.
    /// True if open.
    ///
    /// # Safety
    /// `d` was opened with this build's seal (`chimera_board::bank2_seal()`).
    pub unsafe fn open(&self, d: &Door) -> bool {
        let ok = d.door_addr() == DOOR_ADDR;
        self.0
            .store(if ok { DOOR_ADDR as usize } else { 0 }, Ordering::Release);
        ok
    }

    pub fn close(&self) {
        self.0.store(0, Ordering::Release);
    }

    pub fn is_open(&self) -> bool {
        self.0.load(Ordering::Acquire) != 0
    }

    /// `f` on the door table, `None` closed. Nothing borrowed from the
    /// table may leave `f` (see `Bank2Door`); nothing closes the cell or
    /// writes bank 2 while `f` runs.
    pub fn with<R>(&self, f: impl FnOnce(&Bank2Door) -> R) -> Option<R> {
        let a = self.0.load(Ordering::Acquire);
        if a == 0 {
            return None;
        }
        // SAFETY: only `open` stores a nonzero `a`, and only `DOOR_ADDR`
        // from a `Door` its caller vouched was opened with this build's
        // seal: a `Good` verdict read live from this chip's bank 2
        // (`Bank2Memory`'s contract). The seal is the hash of this link's
        // `.bk2` body, so that body is in bank 2, and at `DOOR_ADDR` (its
        // `__bank2_door`, linker-asserted) is its `DOOR`: a `Bank2Door`,
        // aligned. The borrow ends with `f`, and bank 2 is written only
        // with the cell closed, so it never outlives the image. On a host,
        // `open_host` stores `DOOR`'s own address, a `'static`.
        Some(f(unsafe { &*(a as *const Bank2Door) }))
    }

    /// Opens on this build's own `DOOR`, linked into the host's binary:
    /// the host's tests of what bank 2 runs.
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    pub fn open_host(&self) {
        self.0.store(&raw const DOOR as usize, Ordering::Release);
    }
}

impl Default for DoorCell {
    fn default() -> Self {
        Self::new()
    }
}

pub static DOOR_CELL: DoorCell = DoorCell::new();
