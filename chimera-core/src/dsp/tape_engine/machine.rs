//! The machines' label table (spec § 2 Machines, ADR 0093): each machine
//! labels the shared params its own way, dims what it ignores and has its
//! own defaults, so a player is nudged toward what it does (Elektron
//! Digitakt II, as the owner cites it; Octatrack manual OS 1.25,
//! § Machines). START, END, TAPE, AGE and SPACE read the same everywhere.
//! The UI writes a machine's defaults when MODEL changes (Task 15), never
//! on load; the neutral stays LOOP's.

use super::params::{StretchType, TapeEngineParams as P, TapeModel, Wow};
use crate::block::ParamId;

/// One labelled control on a machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub id: ParamId,
    pub label: &'static str,
    pub short: &'static str,
    pub default: f32,
}

/// A machine: its model page's name, the home page's POSITION and MOTION
/// cells (`None` dims one), and its model page's cells in order.
#[derive(Clone, Copy, Debug)]
pub struct Machine {
    pub model: TapeModel,
    pub page: &'static str,
    pub deck: [Option<Cell>; 2],
    pub cells: &'static [Cell],
}

const fn cell(id: ParamId, label: &'static str, short: &'static str, default: f32) -> Cell {
    Cell {
        id,
        label,
        short,
        default,
    }
}

/// Every model page's last cell.
const WOW: Cell = cell(P::WOW, "WOW", "WOW", Wow::Capstan as u8 as f32);
const POS: Option<Cell> = Some(cell(P::POSITION, "POSITION", "POS", 0.0));
const WALK: Option<Cell> = Some(cell(P::MOTION, "WALK", "WLK", 0.0));
/// MOTION 0.5: the source's own speed, so a fresh STRETCH or SCRUB plays.
const SPEED: Option<Cell> = Some(cell(P::MOTION, "SPEED", "SPD", 0.5));
const LENGTH: Cell = cell(P::SIZE, "LENGTH", "LEN", 1.0);
const XFADE: Cell = cell(P::XFADE, "XFADE", "XFD", 0.1);

/// By `TapeModel::index`.
pub static MACHINES: [Machine; 7] = [
    Machine {
        model: TapeModel::Oneshot,
        page: "ONE",
        deck: [None, None],
        // SLICE's value 0: OFF.
        cells: &[cell(P::SLICE, "SLICE", "SLC", 0.0), WOW],
    },
    Machine {
        model: TapeModel::Loop,
        page: "LOOP",
        deck: [POS, WALK],
        cells: &[LENGTH, XFADE, WOW],
    },
    Machine {
        model: TapeModel::Tron,
        page: "TRON",
        deck: [None, None],
        // LENGTH 1: 8 s. FADE 0.3: 32 ms.
        cells: &[LENGTH, cell(P::DRAG, "FADE", "FDE", 0.3), WOW],
    },
    Machine {
        model: TapeModel::Stretch,
        page: "STRCH",
        deck: [POS, SPEED],
        // D-TIME 0.5: 50 ms.
        cells: &[
            cell(P::SIZE, "D-TIME", "DTM", 0.5),
            cell(P::TYPE, "TYPE", "TYP", StretchType::Mono as u8 as f32),
            WOW,
        ],
    },
    Machine {
        model: TapeModel::Transwave,
        page: "WAVE",
        deck: [
            Some(cell(P::POSITION, "FRAME", "FRM", 0.0)),
            Some(cell(P::MOTION, "SCAN", "SCN", 0.0)),
        ],
        cells: &[cell(P::SIZE, "RANGE", "RNG", 1.0), WOW],
    },
    Machine {
        model: TapeModel::Scrub,
        page: "SCRUB",
        deck: [Some(cell(P::POSITION, "FINGER", "FNG", 0.0)), SPEED],
        cells: &[LENGTH, XFADE, cell(P::DRAG, "DRAG", "DRG", 0.3), WOW],
    },
    Machine {
        model: TapeModel::PingPong,
        page: "PING",
        deck: [POS, WALK],
        cells: &[LENGTH, WOW],
    },
];

pub fn machine(m: TapeModel) -> &'static Machine {
    &MACHINES[usize::from(m.index())]
}

/// `m`'s cell for `id`: on DECK or its model page.
pub fn cell_of(m: TapeModel, id: ParamId) -> Option<&'static Cell> {
    let k = machine(m);
    k.deck.iter().flatten().chain(k.cells).find(|c| c.id == id)
}

/// The short a machine gives `id` under `label` (`WALK` is `WLK`).
pub fn short_of(id: ParamId, label: &str) -> Option<&'static str> {
    MACHINES
        .iter()
        .flat_map(|k| k.deck.iter().flatten().chain(k.cells))
        .find(|c| c.id == id && c.label == label)
        .map(|c| c.short)
}

/// What the UI writes when MODEL becomes `m`: its home cells and its model
/// page's, WOW aside (a flavour the player keeps across machines).
pub fn defaults(m: TapeModel) -> impl Iterator<Item = (ParamId, f32)> {
    let k = machine(m);
    k.deck
        .iter()
        .flatten()
        .chain(k.cells)
        .filter(|c| c.id != P::WOW)
        .map(|c| (c.id, c.default))
}
