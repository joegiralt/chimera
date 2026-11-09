//! Bank 2's side of the link (ADR 0137) and bank 1's state of its door.
//!
//! With `bank2`, the header placeholder and the door table land in the
//! `.bank2` section at 0x08100000 (`chimera-stm32/build.rs`), which the
//! Justfile splits into the `.bk2`. Bank 1 never names them: it reaches the
//! door only through the address a judged `Door` carries.

use chimera_image::{BANK2_BASE, Door, HEADER_LEN};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const DOOR_MAGIC: u32 = 0xB2D0_0001;
/// The door's data table: Phase 1's test payload (a `.bk2` with data
/// behind the door, plan 2026-10-13). The seal and CRC prove every byte of
/// the image; the table proves only that data reads through the door, and
/// its first and last bytes do that at any length (ADR 0251).
pub const TABLE_LEN: usize = 16;
/// `probe(x)` is `x + PROBE_ADDS`.
pub const PROBE_ADDS: u32 = 42;
/// Where the link puts the door: right after the header (`bank2.x` asserts
/// `__bank2_door` is this).
pub const DOOR_ADDR: u32 = BANK2_BASE + HEADER_LEN;
/// The board's table (`chimera_board::bank2::BoardDoor`): the card
/// interrupt's streaming work. A crate the engine can't name keeps its own
/// table at its own fixed slot after the door (`bank2.x` asserts each),
/// reached through its `DoorSlot`.
pub const BOARD_DOOR_ADDR: u32 = DOOR_ADDR + 0x100;
/// The UI's table (`chimera_core::bank2::UiDoor`): TAPE's cells and strip.
pub const UI_DOOR_ADDR: u32 = DOOR_ADDR + 0x180;
/// The bench's table (`chimera-stm32`'s `bench::BenchDoor`, ADR 0194): its
/// run, in a bench build only.
pub const BENCH_DOOR_ADDR: u32 = DOOR_ADDR + 0x200;
/// ORBIT's pages' table (`chimera_core::ui::orbit::door::OrbitUiDoor`, ADR
/// 0208), in a build with `orbit` only.
pub const ORBIT_UI_DOOR_ADDR: u32 = DOOR_ADDR + 0x280;
/// The MIDI UI's table (`chimera_core::ui::cc::MidiUiDoor`, ADR 0217):
/// bindings resolving.
pub const MIDI_UI_DOOR_ADDR: u32 = DOOR_ADDR + 0x300;

/// A fixed slot after the door and the type `bank2.x` links there: a crate
/// the engine can't name declares its table's slot once, beside the
/// assert, and reads it only through `DoorCell::with_slot`.
///
/// ```compile_fail,E0133
/// use chimera_engine::bank2::{DoorSlot, UI_DOOR_ADDR};
/// let _ = DoorSlot::<u32>::new(UI_DOOR_ADDR); // unsafe: the pairing is vouched
/// ```
pub struct DoorSlot<T> {
    at: u32,
    _t: PhantomData<fn() -> T>,
}

impl<T> DoorSlot<T> {
    /// # Safety
    /// `bank2.x` links a `T` at `at` in every build with `bank2` (its slot's
    /// assert).
    pub const unsafe fn new(at: u32) -> Self {
        DoorSlot {
            at,
            _t: PhantomData,
        }
    }
}

impl<T> Clone for DoorSlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for DoorSlot<T> {}

/// The door table: what bank 1 may call or read in bank 2. Reached only
/// inside `DoorCell::with`; its entries are private, called through
/// methods, so no pointer into bank 2 (which an install erases) leaves.
///
/// ```compile_fail,E0616
/// let _ = chimera_engine::bank2::DOOR_CELL.with(|d| d.probe);
/// ```
#[repr(C)]
pub struct Bank2Door {
    pub magic: u32,
    probe: extern "Rust" fn(u32) -> u32,
    /// Byte `i` is `i as u8 ^ 0xB2`.
    table: &'static [u8; TABLE_LEN],
    /// TAPE's deck (`TapeEngine` is bank 1's proxy of it).
    #[cfg(feature = "bank2")]
    pub tape: crate::dsp::tape_engine::TapeDoor,
    /// The reverb's engines (ADR 0142; `Reverb::play` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub reverb: crate::dsp::rev::ReverbDoor,
    /// Streamed admission (`Instrument`'s wrappers are bank 1's side).
    #[cfg(feature = "bank2")]
    pub admit: crate::instrument::AdmitDoor,
    /// Streamed loads: head placement and the load's passes
    /// (`StreamLoad::step`, `stage::heads_for` are bank 1's side).
    #[cfg(feature = "bank2")]
    pub load: crate::project::LoadDoor,
    /// TAPE's machine glyphs' raster (ADR 0173; `tape_glyph::paint` is
    /// bank 1's side).
    #[cfg(feature = "bank2")]
    pub glyph: crate::tape_glyph::GlyphDoor,
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
    /// MODAL's block (ADR 0215; `ModalEngine::render` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub modal: crate::dsp::modal::ModalDoor,
    /// The filters' FB loops (ADR 0190; `filter::fed::through` is bank 1's
    /// side).
    #[cfg(feature = "bank2")]
    pub fed: crate::dsp::filter::fed::FedDoor,
    /// SEM's kernel (ADR 0192; `Sem::process` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub sem: crate::dsp::filter::sem::SemDoor,
    /// VA NOISE's pass (ADR 0197; `Noise::add` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub noise: crate::dsp::va::noise::NoiseDoor,
    /// The delay's TAPE and GRAIN (ADR 0202, 0213; `TapeDelay::run` is
    /// bank 1's side).
    #[cfg(feature = "bank2")]
    pub delay: crate::dsp::delay::DelayDoor,
    /// The chorus's lines (ADR 0213; `JunoChorus::process_wet` is bank 1's
    /// side).
    #[cfg(feature = "bank2")]
    pub chorus: crate::dsp::chorus::ChorusDoor,
    /// The VCA's low-pass gate (ADR 0195; `voice::gate::through` is bank
    /// 1's side).
    #[cfg(feature = "bank2")]
    pub gate: crate::dsp::voice::gate::GateDoor,
    /// ORBIT's runner (ADR 0208; `OrbitRunner::block_ported` and the
    /// rest are bank 1's side).
    #[cfg(all(feature = "bank2", feature = "orbit"))]
    pub orbit: crate::orbit::door::OrbitDoor,
    /// ALGO's block (ADR 0212; `AlgoEngine::render` is bank 1's side).
    #[cfg(feature = "bank2")]
    pub algo: crate::dsp::algo::engine::AlgoDoor,
    /// MIDI in's pipe (`midi_in::feed` and `idle` are bank 1's side).
    #[cfg(feature = "bank2")]
    pub midi: crate::midi_in::MidiDoor,
    /// LFO DEPTH's routes (ADR 0250; `voice::depth::through` is bank 1's
    /// side).
    #[cfg(feature = "bank2")]
    pub depth: crate::dsp::voice::depth::DepthDoor,
}

// The table fits before the board's slot (the link would fail later, and
// less plainly).
#[cfg(all(feature = "bank2", target_pointer_width = "32"))]
const _: () = assert!(size_of::<Bank2Door>() <= (BOARD_DOOR_ADDR - DOOR_ADDR) as usize);

impl Bank2Door {
    #[inline(always)]
    pub fn probe(&self, x: u32) -> u32 {
        (self.probe)(x)
    }

    #[inline(always)]
    pub fn table(&self) -> &[u8; TABLE_LEN] {
        self.table
    }
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
    admit: crate::instrument::Instrument::ADMIT,
    load: crate::project::LoadDoor::DOOR,
    glyph: crate::tape_glyph::GlyphDoor::DOOR,
    va: crate::dsp::va::osc::door::DOOR,
    lfo: crate::lfo_glyph::LfoDoor::DOOR,
    look: crate::dsp::voice_fx::look::LookDoor::DOOR,
    fuzz: crate::dsp::voice_fx::fuzz::Fuzz::DOOR,
    modal: crate::dsp::modal::ModalDoor::DOOR,
    fed: crate::dsp::filter::fed::FedDoor::DOOR,
    sem: crate::dsp::filter::sem::SemDoor::DOOR,
    noise: crate::dsp::va::noise::NoiseDoor::DOOR,
    delay: crate::dsp::delay::DelayDoor::DOOR,
    chorus: crate::dsp::chorus::ChorusDoor::DOOR,
    gate: crate::dsp::voice::gate::GateDoor::DOOR,
    #[cfg(feature = "orbit")]
    orbit: crate::orbit::door::OrbitDoor::DOOR,
    algo: crate::dsp::algo::engine::AlgoDoor::DOOR,
    midi: crate::midi_in::MidiDoor::DOOR,
    depth: crate::dsp::voice::depth::DepthDoor::DOOR,
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

    /// Publishes a door judged and checked (`Checked`): from here every
    /// interrupt's door call goes through it.
    ///
    /// ```compile_fail,E0308
    /// use chimera_engine::bank2::{DOOR_CELL, Judged};
    /// fn open_unchecked(j: Judged) {
    ///     DOOR_CELL.open(j);
    /// }
    /// ```
    pub fn open(&self, _: Checked) {
        self.0.store(DOOR_ADDR as usize, Ordering::Release);
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

    /// `f` on the table in slot `s`; `None` closed, and on a host
    /// (`open_host`'s door is a plain static, with no slots after it),
    /// where those crates call their own code.
    pub fn with_slot<T, R>(&self, s: DoorSlot<T>, f: impl FnOnce(&T) -> R) -> Option<R> {
        let a = self.0.load(Ordering::Acquire);
        if a != DOOR_ADDR as usize {
            return None;
        }
        // SAFETY: the cell holds `DOOR_ADDR` only from `open` on a door
        // judged `Good` for this build's seal (see `with`): this link's
        // body is in bank 2, so at `s.at` is its `T` (`DoorSlot::new`'s
        // contract), and it lives as long as the image does, past `f`.
        Some(f(unsafe { &*(s.at as *const T) }))
    }

    /// Opens on this build's own `DOOR`, linked into the host's binary:
    /// the host's tests of what bank 2 runs.
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    pub fn open_host(&self) {
        self.0.store(&raw const DOOR as usize, Ordering::Release);
    }

    /// `open_host` where this build links bank 2, nothing where it plays
    /// TAPE from bank 1: a host test run under the firmware's features
    /// (`just test-firmware-features`) loads as the unit does, door open.
    #[cfg(feature = "test-support")]
    pub fn open_host_if_linked(&self) {
        #[cfg(feature = "bank2")]
        self.open_host();
    }
}

/// A door judged `Good` for this build's seal, at this link's `DOOR_ADDR`,
/// not yet in the cell: the boot's and an install's checks read its
/// tables through this (`check`) before `DoorCell::open` publishes it, so
/// no interrupt calls through a table they haven't passed (I2).
pub struct Judged(());

/// A judged door that passed its check: what `DoorCell::open` publishes.
pub struct Checked(());

impl Judged {
    /// `d`, if it is at this link's `DOOR_ADDR`; a door anywhere else (a
    /// header the seal does not cover, gone wrong) is none.
    ///
    /// # Safety
    /// `d` was opened with this build's seal (`chimera_board::bank2_seal()`).
    pub unsafe fn of(d: &Door) -> Option<Judged> {
        (d.door_addr() == DOOR_ADDR).then_some(Judged(()))
    }

    /// `f` on the door table, read where it lies (see `DoorCell::with`).
    pub fn with<R>(&self, f: impl FnOnce(&Bank2Door) -> R) -> R {
        // SAFETY: a `Judged` is made only by `of`, on a door its caller
        // vouched was opened with this build's seal and at `DOOR_ADDR`: as
        // `DoorCell::with`'s nonzero address.
        f(unsafe { &*(DOOR_ADDR as usize as *const Bank2Door) })
    }

    /// `f` on the table in slot `s` (see `DoorCell::with_slot`).
    pub fn with_slot<T, R>(&self, s: DoorSlot<T>, f: impl FnOnce(&T) -> R) -> R {
        // SAFETY: this link's body is in bank 2 (`with`), so at `s.at` is
        // its `T` (`DoorSlot::new`'s contract).
        f(unsafe { &*(s.at as usize as *const T) })
    }

    /// The door, if `passes` it: the only way to a `Checked`.
    pub fn check(self, passes: impl FnOnce(&Judged) -> bool) -> Option<Checked> {
        passes(&self).then_some(Checked(()))
    }
}

impl Default for DoorCell {
    fn default() -> Self {
        Self::new()
    }
}

pub static DOOR_CELL: DoorCell = DoorCell::new();
