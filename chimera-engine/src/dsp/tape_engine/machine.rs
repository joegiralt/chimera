//! The machines' label table (spec § 2 Machines, ADR 0093, ADR 0178):
//! each machine labels the shared params its own way, shows only what it
//! reads and has its own defaults, so a player is nudged toward what it
//! does (Elektron Digitakt II, as the owner cites it; Octatrack manual OS
//! 1.25, § Machines). DECK holds MODEL and every knob that moves the
//! machine's glyph; the model page TAPE, AGE and WOW in the same cells on
//! every machine, then whatever else it reads.
//! The UI writes a machine's defaults when MODEL changes (Task 15), never
//! on load; the neutral stays WALK in LOOP. WALK (ADR 0203) holds what
//! ONE, LOOP, PING and TRON were: DIRECTION and TRON are its cells.

use super::params::{
    Direction, FADE_HOLD, KeyMode, Retrig, STRETCH_DEFAULTS, StretchType, TapeEngineParams as P,
    TapeMachine, Tron, fade_default,
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
const POS: Cell = cell(P::POSITION, "POSITION", "POS", 0.0);
/// MOTION 0.5: the source's own speed, so a fresh STRETCH or SCRUB plays.
const SPEED: Cell = cell(P::MOTION, "SPEED", "SPD", 0.5);
const LENGTH: Cell = cell(P::SIZE, "LENGTH", "LEN", 1.0);
/// LOOP's and PING's LENGTH 0.8: a loop shorter than the window (about
/// 1 s of a 4 s source), so POSITION and DRIFT move it from the first turn
/// (#698). INIT's.
pub const LOOP_LENGTH_DEFAULT: f32 = 0.8;
const XFADE: Cell = cell(P::XFADE, "XFADE", "XFD", 0.1);
/// SCRUB's LENGTH 66/128: a 70 ms loop on any source (`SCRUB_LEN_MAX_S`),
/// so FINGER moves it from the first turn (ADR 0205).
pub const SCRUB_LENGTH_DEFAULT: f32 = 66.0 / 128.0;

/// By `TapeMachine::index`.
pub static MACHINES: [Machine; 4] = [
    // WALK (ADR 0203): what ONE, LOOP, PING and TRON were. LENGTH and
    // FADE default by DIRECTION (`defaults`); DIRECTION is kept.
    Machine {
        machine: TapeMachine::Walk,
        page: "WALK",
        deck: &[
            LENGTH,
            cell(P::DRAG, "FADE", "FDE", FADE_HOLD),
            choice(P::MODE, "DIRECTION", "DIR", Direction::Loop as u8),
            POS,
            cell(P::DRIFT, "DRIFT", "DFT", 0.0),
        ],
        cells: &[
            choice(P::TRON, "TRON", "TRN", Tron::Off as u8),
            XFADE,
            choice(P::RETRIG, "RETRIG", "RTG", Retrig::Stack as u8),
        ],
        env: &ONE_ENV,
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

/// What the UI writes when MODEL becomes `m`, WALK in `d`: its cells but
/// the window (START and END, the source's), WALK's DIRECTION (kept) and
/// the wear (TAPE, AGE and WOW, a flavour the player keeps across
/// machines). WALK's LENGTH and FADE are `d`'s (`direction_defaults`).
pub fn defaults(m: TapeMachine, d: Direction) -> impl Iterator<Item = (ParamId, f32)> {
    let k = machine(m);
    let play = m == TapeMachine::Walk;
    k.deck
        .iter()
        .chain(k.cells)
        .filter(move |c| !matches!(c.id, P::START | P::END | P::MODE))
        .map(move |c| match c.id {
            P::SIZE | P::DRAG if play => (c.id, by_direction(c.id, d)),
            _ => (c.id, c.value),
        })
}

/// WALK's LENGTH or FADE default in `d`: once through, the whole window
/// held to its end (ONE's, ADR 0180); around a loop, LOOP's (#698).
fn by_direction(id: ParamId, d: Direction) -> f32 {
    match id {
        P::SIZE if d.once() => 1.0,
        P::SIZE => LOOP_LENGTH_DEFAULT,
        _ => fade_default(d),
    }
}

/// What the UI writes when DIRECTION goes from `was` to `now`: crossing
/// between once through and a loop, LENGTH and FADE take `now`'s
/// defaults, as a MODEL turn between ONE and LOOP did (ADR 0203). Within
/// a side, nothing.
pub fn direction_defaults(was: Direction, now: Direction) -> &'static [(ParamId, f32)] {
    match (was.once(), now.once()) {
        (false, true) => &ONCE_DEFAULTS,
        (true, false) => &LOOP_DEFAULTS,
        _ => &[],
    }
}

const ONCE_DEFAULTS: [(ParamId, f32); 2] = [(P::SIZE, 1.0), (P::DRAG, FADE_HOLD)];
const LOOP_DEFAULTS: [(ParamId, f32); 2] = [(P::SIZE, LOOP_LENGTH_DEFAULT), (P::DRAG, 0.3)];

/// ENV 1's cells (`BlockRef::Env(Env1)`) the UI writes when MODEL becomes
/// `m`'s ENV 1 a MODEL turn writes, `d` WALK's DIRECTION: WALK once
/// through, or DIRECTION crossing to once through, and STRETCH open at
/// the envelope's floor (FAST, 0.2 ms; the 1 ms note-on ramp keeps it
/// click-free) and hold at full, so the sample is its own envelope, as an
/// MPC pad's (#698; a stretched strike plays at ONE's level, ADR 0204).
/// WALK around a loop and the others leave ENV 1 be.
pub fn env_defaults(m: TapeMachine, d: Direction) -> &'static [(ParamId, f32)] {
    match m {
        TapeMachine::Walk if !d.once() => &[],
        _ => machine(m).env,
    }
}

const ONE_ENV: [(ParamId, f32); 3] = [
    (E::SPEED, EnvSpeed::Fast as u8 as f32),
    (E::ATTACK, 0.0),
    (E::SUSTAIN, 1.0),
];
