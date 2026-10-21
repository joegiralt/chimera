//! The machines' label table (spec § 2 Machines, ADR 0093, ADR 0178):
//! each machine labels the shared params its own way, shows only what it
//! reads and has its own defaults, so a player is nudged toward what it
//! does (Elektron Digitakt II, as the owner cites it; Octatrack manual OS
//! 1.25, § Machines). DECK holds MODEL and every knob that moves the
//! machine's glyph; the model page TAPE, AGE and WOW in the same cells on
//! every machine, then whatever else it reads.
//! The UI writes a machine's defaults when MODEL changes (Task 15), never
//! on load; the neutral stays PLAY in LOOP.

use super::params::{
    FADE_HOLD, KeyMode, Retrig, STRETCH_DEFAULTS, StretchType, TapeEngineParams as P, TapeMachine,
    Wow,
};
use crate::block::ParamId;
use crate::dsp::modulator::EnvSpeed;
use crate::params::EnvParams as E;

/// One labelled control on a machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub id: ParamId,
    pub label: &'static str,
    pub short: &'static str,
    // `CellDefault` flat: the enum's tag would add 4 B a cell.
    value: f32,
    choice: bool,
}

impl Cell {
    pub const fn default(&self) -> CellDefault {
        if self.choice {
            CellDefault::Choice(self.value as u8)
        } else {
            CellDefault::Unit(self.value)
        }
    }
}

/// A cell's default: a fraction of its range, or a choice's place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellDefault {
    Unit(f32),
    Choice(u8),
}

impl CellDefault {
    /// As `Block::set` takes it.
    pub fn value(self) -> f32 {
        match self {
            CellDefault::Unit(v) => v,
            CellDefault::Choice(c) => f32::from(c),
        }
    }
}

/// A machine: its model page's name, DECK's cells after MODEL (the
/// glyph's knobs) and its model page's after TAPE, AGE and WOW.
#[derive(Clone, Copy, Debug)]
pub struct Machine {
    pub machine: TapeMachine,
    pub page: &'static str,
    pub deck: &'static [Cell],
    pub cells: &'static [Cell],
    /// ENV 1's cells a MODEL turn writes (`env_defaults`): a table, not a
    /// match, so a machine's are data (ADR 0204).
    pub env: &'static [(ParamId, f32)],
}

/// DECK's cells after MODEL.
pub const DECK_CELLS: usize = 5;
/// The model page's cells after TAPE, AGE and WOW.
pub const PAGE_CELLS: usize = 3;

const fn cell(id: ParamId, label: &'static str, short: &'static str, default: f32) -> Cell {
    Cell {
        id,
        label,
        short,
        value: default,
        choice: false,
    }
}

const fn choice(id: ParamId, label: &'static str, short: &'static str, default: u8) -> Cell {
    Cell {
        id,
        label,
        short,
        value: default as f32,
        choice: true,
    }
}

// The window is the source's: a machine change keeps it (`defaults`).
const START: Cell = cell(P::START, "START", "STA", 0.0);
const END: Cell = cell(P::END, "END", "END", 1.0);
const POS: Cell = cell(P::POSITION, "POSITION", "POS", 0.0);
const WALK: Cell = cell(P::MOTION, "WALK", "WLK", 0.0);
/// MOTION 0.5: the source's own speed, so a fresh STRETCH or SCRUB plays.
const SPEED: Cell = cell(P::MOTION, "SPEED", "SPD", 0.5);
const LENGTH: Cell = cell(P::SIZE, "LENGTH", "LEN", 1.0);
/// LOOP's LENGTH 0.8: a loop shorter than the window (about 1 s of a 4 s
/// source), so POSITION and WALK move it from the first turn (#698).
const LOOP_LENGTH: Cell = cell(P::SIZE, "LENGTH", "LEN", LOOP_LENGTH_DEFAULT);
/// LOOP's and PING's LENGTH, and INIT's.
pub const LOOP_LENGTH_DEFAULT: f32 = 0.8;
const XFADE: Cell = cell(P::XFADE, "XFADE", "XFD", 0.1);
/// SCRUB's LENGTH 66/128: a 70 ms loop on any source (`SCRUB_LEN_MAX_S`),
/// so FINGER moves it from the first turn (ADR 0205).
pub const SCRUB_LENGTH_DEFAULT: f32 = 66.0 / 128.0;
/// FADE 0.3: 32 ms (`fade_default`).
const FADE: Cell = cell(P::DRAG, "FADE", "FDE", 0.3);

/// By `TapeMachine::index`.
pub static MACHINES: [Machine; 7] = [
    Machine {
        machine: TapeMachine::One,
        page: "ONE",
        deck: &[START, END],
        // HOLD: a drum one-shot plays to END; RETRIG STACKs.
        cells: &[
            cell(P::DRAG, "FADE", "FDE", FADE_HOLD),
            choice(P::RETRIG, "RETRIG", "RTG", Retrig::Stack as u8),
        ],
        env: &ONE_ENV,
    },
    Machine {
        machine: TapeMachine::Loop,
        page: "LOOP",
        deck: &[POS, WALK, LOOP_LENGTH, XFADE],
        cells: &[FADE],
        env: &[],
    },
    Machine {
        machine: TapeMachine::Ping,
        page: "PING",
        deck: &[POS, WALK, LOOP_LENGTH],
        cells: &[FADE],
        env: &[],
    },
    Machine {
        machine: TapeMachine::Tron,
        page: "TRON",
        // LENGTH 1: 8 s. TRON draws its FADE: the snap back is the machine.
        deck: &[LENGTH, FADE],
        cells: &[],
        env: &[],
    },
    Machine {
        machine: TapeMachine::Scrub,
        page: "SCRUB",
        deck: &[
            cell(P::POSITION, "FINGER", "FNG", 0.0),
            SPEED,
            cell(P::SIZE, "LENGTH", "LEN", SCRUB_LENGTH_DEFAULT),
            XFADE,
            cell(P::DRAG, "DRAG", "DRG", 0.3),
        ],
        // KEY POSITION: a key places the finger and plays at ROOT.
        cells: &[choice(P::KEY, "KEY", "KEY", KeyMode::Position as u8)],
        env: &[],
    },
    Machine {
        machine: TapeMachine::Stretch,
        page: "STRCH",
        // Two TYPEs (ADR 0204), a turn landing on GRAIN: LENGTH 2 s at
        // SPEED 1×, GRAIN 80 ms, SPRAY 40 %, END HOLD; CLASSIC's D-TIME
        // 50 ms from POSITION 0. Each TYPE's cells dim on the other. The
        // bellows draw TYPE, SPEED, and LENGTH or D-TIME and POSITION.
        deck: &[
            choice(P::TYPE, "TYPE", "TYP", StretchType::Grain as u8),
            SPEED,
            cell(P::LENGTH, "LENGTH", "LEN", STRETCH_DEFAULTS[0]),
            cell(P::SIZE, "D-TIME", "DTM", 0.5),
            POS,
        ],
        cells: &[
            cell(P::GRAIN, "GRAIN", "GRN", STRETCH_DEFAULTS[1]),
            cell(P::SPRAY, "SPRAY", "SPR", STRETCH_DEFAULTS[2]),
            choice(P::FINISH, "END", "END", 0),
        ],
        env: &ONE_ENV,
    },
    Machine {
        machine: TapeMachine::Wave,
        page: "WAVE",
        deck: &[
            cell(P::POSITION, "FRAME", "FRM", 0.0),
            cell(P::MOTION, "SCAN", "SCN", 0.0),
            cell(P::SIZE, "RANGE", "RNG", 1.0),
        ],
        cells: &[],
        env: &[],
    },
];

const _: () = {
    let mut i = 0;
    while i < MACHINES.len() {
        assert!(MACHINES[i].machine as usize == i);
        assert!(MACHINES[i].deck.len() <= DECK_CELLS);
        assert!(MACHINES[i].cells.len() <= PAGE_CELLS);
        i += 1;
    }
};

pub fn machine(m: TapeMachine) -> &'static Machine {
    &MACHINES[usize::from(m.index())]
}

/// `m`'s cell for `id`: on DECK or its model page.
pub fn cell_of(m: TapeMachine, id: ParamId) -> Option<&'static Cell> {
    let k = machine(m);
    k.deck.iter().chain(k.cells).find(|c| c.id == id)
}

/// What the UI writes when MODEL becomes `m`: its cells but the window
/// (START and END, the source's) and the wear (TAPE, AGE and WOW, a
/// flavour the player keeps across machines), but TRON's: its wear is its
/// identity (`TRON_WEAR`).
pub fn defaults(m: TapeMachine) -> impl Iterator<Item = (ParamId, f32)> {
    let k = machine(m);
    let wear: &[(ParamId, f32)] = if m == TapeMachine::Tron {
        &TRON_WEAR
    } else {
        &[]
    };
    k.deck
        .iter()
        .chain(k.cells)
        .filter(|c| !matches!(c.id, P::START | P::END))
        .map(|c| (c.id, c.value))
        .chain(wear.iter().copied())
}

/// TRON's wear at a MODEL change (#698): an M400's tape is never still,
/// TAPE 0.3 under WARBLE's wobble.
pub const TRON_WEAR: [(ParamId, f32); 2] = [(P::TAPE, 0.3), (P::WOW, Wow::Warble as u8 as f32)];

/// ENV 1's cells (`BlockRef::Env(Env1)`) the UI writes when MODEL becomes
/// `m`. ONE: the hit opens at the envelope's floor (FAST, 0.2 ms; the
/// 1 ms note-on ramp keeps it click-free) and holds at full, so the sample
/// is its own envelope, as an MPC pad's (#698); STRETCH's too, so a
/// stretched strike keeps it and plays at ONE's level (ADR 0204). The
/// others leave ENV 1 be.
pub fn env_defaults(m: TapeMachine) -> &'static [(ParamId, f32)] {
    machine(m).env
}

const ONE_ENV: [(ParamId, f32); 3] = [
    (E::SPEED, EnvSpeed::Fast as u8 as f32),
    (E::ATTACK, 0.0),
    (E::SUSTAIN, 1.0),
];
