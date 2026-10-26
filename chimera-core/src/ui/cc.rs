//! Mapped controls move the Sound (spec §§ 5-7): bank 1's side of
//! `MidiUiDoor` (ADR 0198, 0217). Bindings resolve in bank 2 (`ui::midi`),
//! its table at its own slot after the door (`MIDI_UI_DOOR_ADDR`); here
//! are the state's home and the stubs. Closed, what MIDI in heard is
//! drained and dropped (spec § 13): nothing moves.

use core::mem::MaybeUninit;

use chimera_hal::Ms;
use chimera_midi::ctl::Dir;

use super::UiState;
use super::fmt::FmtBuf;
use super::hint::{Gesture, Hint};
use super::learn::{LearnNote, Line, Sensed};
use super::midi::MidiUi;
#[cfg(feature = "midi-map")]
use super::settings::midi_map::MapText;
use crate::addr::ParamAddr;
#[cfg(feature = "midi-map")]
use crate::dsp::voice_fx::params::FxType;
use crate::hw::MAX_PARTS;
use crate::midi_in::{HeardCc, HeardEv};
#[cfg(feature = "midi-map")]
use crate::midi_map::Binding;
use crate::project::PartId;

/// The MIDI UI's state: in AXI `.bss`, not in D2 with `UiState` (bench
/// builds' D2 is full). Bank 1 keeps the hand's moves, the moves in flight
/// to the audio, this pass's room and the clock; bank 2 builds the rest
/// (`MidiUi`, ~5.7 KB) the first time the door runs, so bank 1 carries no
/// image of it.
pub struct MidiSlot {
    pub(super) built: bool,
    /// The Parts whose map changed since the door last ran.
    pub(super) stale: u8,
    /// The hand's knob moves this pass.
    pub(super) hand: u8,
    /// Knob moves published since the audio last took a publish.
    pub(super) flight: [u8; MAX_PARTS],
    /// The knob writes left this pass.
    pub(super) room: [u8; MAX_PARTS],
    pub(super) last: Ms,
    pub(super) ui: MaybeUninit<MidiUi>,
}

const _: () = assert!(MAX_PARTS <= 8, "`stale` holds a bit a Part");

impl MidiSlot {
    const fn new() -> Self {
        Self {
            built: false,
            stale: 0,
            hand: 0,
            flight: [0; MAX_PARTS],
            room: [0; MAX_PARTS],
            last: Ms(0),
            ui: MaybeUninit::uninit(),
        }
    }

    /// The firmware's one slot, zeroed `.bss` in AXI.
    #[cfg(target_os = "none")]
    pub(super) fn take() -> &'static mut MidiSlot {
        use core::sync::atomic::{AtomicBool, Ordering};
        static mut SLOT: MidiSlot = MidiSlot::new();
        static TAKEN: AtomicBool = AtomicBool::new(false);
        assert!(!TAKEN.swap(true, Ordering::AcqRel));
        // SAFETY: the flag lets exactly one caller past, so this is the
        // only reference to `SLOT` ever made.
        unsafe { &mut *core::ptr::addr_of_mut!(SLOT) }
    }

    /// A host's: one leaked per `UiState`, as its arena is.
    #[cfg(not(target_os = "none"))]
    pub(super) fn take() -> &'static mut MidiSlot {
        extern crate alloc;
        alloc::boxed::Box::leak(alloc::boxed::Box::new(MidiSlot::new()))
    }

    /// A new project: no ramp or bin survives it.
    pub(super) fn clear(&mut self) {
        self.built = false;
        self.stale = 0;
    }

    /// Part `p`'s map changed (a learn, a page edit): its entries' state
    /// resets before the door next runs.
    #[inline]
    pub(super) fn reset_part(&mut self, p: PartId) {
        self.stale |= 1 << p.index();
    }

    /// The knob moves the hand made this pass, at most: its turns and the
    /// reel's (`handle_input`), which the moves heard after it leave room
    /// for.
    pub(super) fn hand(&mut self, moves: u8) {
        self.hand = moves;
    }
}

/// The door's `sensed`: `UiState::sensed_in`.
type SensedFn = fn(&UiState) -> Option<Sensed>;
/// The door's `words`: `learn::words`, one of sensing's lines.
type Words = fn(Sensed, &str, Line, &mut FmtBuf);
/// The door's `note`: `learn::note_words`, learn's other words.
type Note = fn(Option<LearnNote>, &str, &mut FmtBuf);

/// The door's `map_in`: `UiState::map_in`, MIDI MAP's keys.
#[cfg(feature = "midi-map")]
type MapIn = fn(&mut UiState, [i8; 6], bool);
/// The door's `map_text`: `page::text`, a binding's words on MIDI MAP.
#[cfg(feature = "midi-map")]
type MapTextFn = fn(&Binding, FxType, bool, &mut MapText);
/// The door's `focus_note`: `UiState::focus_note`, the pickup hint's way.
#[cfg(feature = "midi-map")]
type FocusNote = fn(&UiState) -> Option<Dir>;

/// The door's `hear`: `UiState::hear_in`.
type Hear = fn(&mut UiState, &mut dyn Iterator<Item = HeardEv>, Ms, bool);

/// Without `midi-map` (the bench builds, #507) nothing heard is applied:
/// the ring drained and dropped, as with the door closed.
#[cfg(not(feature = "midi-map"))]
fn deaf(_: &mut UiState, ev: &mut dyn Iterator<Item = HeardEv>, _: Ms, _: bool) {
    ev.for_each(drop);
}

/// The entries, private: called only through `through`, so no pointer
/// into bank 2 leaves it.
#[repr(C)]
pub struct MidiUiDoor {
    magic: Magic,
    hear: Hear,
    hand: fn(&mut UiState, PartId, ParamAddr),
    sensed: SensedFn,
    words: Words,
    note: Note,
    #[cfg(feature = "midi-map")]
    map_in: MapIn,
    #[cfg(feature = "midi-map")]
    map_text: MapTextFn,
    #[cfg(feature = "midi-map")]
    focus_note: FocusNote,
}

/// `MidiUiDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`), and bank 2 reads corrupt.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2C0_0217);

impl MidiUiDoor {
    pub const OWN: MidiUiDoor = MidiUiDoor {
        magic: MAGIC,
        #[cfg(feature = "midi-map")]
        hear: UiState::hear_in,
        #[cfg(not(feature = "midi-map"))]
        hear: deaf,
        hand: UiState::hand_in,
        sensed: UiState::sensed_in,
        words: super::midi::learn::words,
        note: super::midi::learn::note_words,
        #[cfg(feature = "midi-map")]
        map_in: UiState::map_in,
        #[cfg(feature = "midi-map")]
        map_text: super::midi::page::text,
        #[cfg(feature = "midi-map")]
        focus_note: UiState::focus_note,
    };
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `MIDI_UI_DOOR`, a `MidiUiDoor`, at
// `MIDI_UI_DOOR_ADDR` in every firmware build with `bank2` (its assert,
// `__bank2_midi_ui_door`).
const SLOT: chimera_engine::bank2::DoorSlot<MidiUiDoor> =
    unsafe { chimera_engine::bank2::DoorSlot::new(chimera_engine::bank2::MIDI_UI_DOOR_ADDR) };

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.midi_ui")]
#[used]
static MIDI_UI_DOOR: MidiUiDoor = MidiUiDoor::OWN;

/// The MIDI UI's table in judged door `j` answers (the boot's door check,
/// run before the door opens).
#[cfg(feature = "bank2")]
pub fn door_ok(j: &chimera_engine::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

/// `f` on the table: bank 2's through the door on the unit, `None` closed;
/// this build's own elsewhere. A host test under `bank2` answers as the
/// unit does: only while the door cell is open.
#[inline(always)]
fn through<R>(f: impl FnOnce(&MidiUiDoor) -> R) -> Option<R> {
    #[cfg(all(feature = "bank2", target_os = "none"))]
    {
        crate::bank2::DOOR_CELL.with_slot(SLOT, f)
    }
    #[cfg(not(all(feature = "bank2", target_os = "none")))]
    {
        #[cfg(feature = "bank2")]
        if !crate::bank2::DOOR_CELL.is_open() {
            return None;
        }
        Some(f(&MidiUiDoor::OWN))
    }
}

impl UiState {
    /// This pass's control messages (spec §§ 5-7), through the door: the
    /// control ring's `CtrlEvent`s, or a test's plain `CcEvent`s. After
    /// `handle_input`, before the publish. `taken`: the audio has taken
    /// the last publish (`Writer::taken`). Closed, drained and dropped.
    pub fn hear_ccs(&mut self, ev: impl Iterator<Item = impl HeardCc>, now: Ms, taken: bool) {
        let mut ev = ev.map(HeardCc::heard);
        if through(|d| (d.hear)(self, &mut ev, now, taken)).is_none() {
            ev.for_each(drop);
        }
    }

    /// What sensing has made of the control learn heard (`sensed_in`).
    /// Closed, nothing: learn never heard one.
    #[cfg(feature = "midi-map")]
    pub(super) fn sensed(&self) -> Option<Sensed> {
        through(|d| (d.sensed)(self)).flatten()
    }

    /// The hand wins: a turn of `a` on Part `p` stops what its bindings
    /// were moving (`hand_in`). Closed, nothing moves.
    pub(super) fn cc_hand(&mut self, p: PartId, a: ParamAddr) {
        if cfg!(feature = "midi-map") {
            through(|d| (d.hand)(self, p, a));
        }
    }
}

#[cfg(feature = "midi-map")]
impl UiState {
    /// MIDI MAP's keys this pass: A to F's detents and MIX+MINUS
    /// (`map_in`). Closed, nothing.
    pub(super) fn map_keys(&mut self, d: [i8; 6], delete: bool) {
        through(|t| (t.map_in)(self, d, delete));
    }
}

impl UiState {
    /// The focused cell's pickup, if one waits: the way the knob must go
    /// (spec § 8.3, `focus_note`). Closed, or without `midi-map`, none.
    pub fn pickup(&self) -> Option<Dir> {
        #[cfg(feature = "midi-map")]
        return through(|d| (d.focus_note)(self)).flatten();
        #[cfg(not(feature = "midi-map"))]
        None
    }
}

/// MIDI MAP's words for `e`, its target `live`, into `out` (`map_text`).
/// Closed, none.
#[cfg(feature = "midi-map")]
pub(super) fn map_text(e: &Binding, fx: FxType, live: bool, out: &mut MapText) {
    through(|d| (d.map_text)(e, fx, live, out));
}

/// Whether bank 2 answers: closed, MIDI MAP reads `NEEDS UPDATE`.
#[cfg(feature = "midi-map")]
pub(super) fn map_open() -> bool {
    through(|_| ()).is_some()
}

/// The pickup hint: which way the knob must turn till its binding takes
/// the param (spec § 8.3).
pub const PICKUP_UP: Hint = &[Gesture("MOVE THE KNOB UP", "TO PICK UP")];
pub const PICKUP_DOWN: Hint = &[Gesture("MOVE THE KNOB DOWN", "TO PICK UP")];

/// The pickup hint for `d`.
pub fn pickup_hint(d: Dir) -> Hint {
    match d {
        Dir::Up => PICKUP_UP,
        Dir::Down => PICKUP_DOWN,
    }
}

/// Learn's words for `n` on the cell labelled `label` (`note`): armed,
/// a refusal, sensing's guess. Closed, none.
pub(super) fn learn_note(n: Option<LearnNote>, label: &str, out: &mut FmtBuf) {
    through(|d| (d.note)(n, label, out));
}

/// Sensing's line `line` for `s` on the cell labelled `label`, into `out`
/// (`learn::words`). Closed, none.
#[cfg(feature = "midi-map")]
pub(super) fn learn_words(s: Sensed, label: &str, line: Line, out: &mut FmtBuf) {
    through(|d| (d.words)(s, label, line, out));
}
