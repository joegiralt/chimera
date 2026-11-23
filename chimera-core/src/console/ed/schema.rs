// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! What `ed describe` describes (ADR 0289): an engine's pages, the
//! mixer's, an engine's mod matrix, SETTINGS', MIDI MAP's grammar or
//! ORBIT's. A `Schema` is parsed once at the boundary from its token and
//! walked by `Pages`. Its pages are shared, then each round's: a pad's on
//! DRUM, a Part's on the mixer, a ring's on ORBIT.

use core::fmt::{self, Write};
use core::mem::MaybeUninit;

use super::addr::{PAD, PART};
use super::lit::lit;
use super::settings::{LEAVES, Leaf};
use crate::dsp::drum::types::PadId;
use crate::params::EngineType;
use crate::project::PartId;
use crate::ui::block_def::{BlockDef, ChainDef2, SlotBinding};
use crate::ui::block_registry::{
    ALGO_FORMANT_CHAIN, DRUM_CHAIN, MIXER_CHANNEL_CHAIN, MODAL_PLUCK_CHAIN, TAPE_CHAIN, VA_CHAIN,
};
use crate::ui::orbit::ORBIT_CHAIN;
use chimera_engine::addr::BlockRef;
use chimera_engine::block::DiskCode;
use chimera_engine::orbit::RingIdx;

/// A schema the editor describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schema {
    /// An engine's pages, the followed Part's (protocol 3's).
    Engine(EngineType),
    /// MIX + B*n*: each Part's PRT, VOI, EQ and SND, then the buses.
    Mixer,
    /// An engine's MTX: its sources, the fixed columns, the amounts. One
    /// page, from the matrix's own tables, not a chain.
    Matrix(EngineType),
    /// SETTINGS' leaves with a param, in tree order (`settings::LEAVES`):
    /// shared, then TUNING and LIVE SNAP once a Part.
    Settings,
    /// MIDI MAP's binding grammar: one page, from the binding model's own
    /// tables, not a chain (none without `midi-map`).
    Map,
    /// ORBIT's pages (`ORBIT_CHAIN`): ORB, CUSTOM, CHRD and SCN shared,
    /// then RHYTHM, VOICE and NOTES once a ring (none without `orbit`).
    Orbit,
}

lit!(
    /// The token of `Schema::Mixer`.
    MIXER = "MIXER"
);
lit!(
    /// The tokens of `Schema::Matrix`: `MTX-ALGO`.
    MTX = "MTX-"
);
lit!(
    /// The token of `Schema::Settings`.
    SETTINGS = "SETTINGS"
);
lit!(
    /// The token of `Schema::Map`.
    MAP = "MAP"
);
lit!(
    /// The token of `Schema::Orbit`.
    ORBIT = "ORBIT"
);
lit!(
    /// A ring's round's prefix, before its number: `RING2_`.
    RING = "RING"
);

/// How many schemas there are: `ed schema`'s hashes, one each.
pub const SCHEMAS: usize = 2 * EngineType::ALL.len() + 4;

/// What a schema's pages go round: nothing, each pad, each Part or each
/// ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundKind {
    None,
    Pads,
    Parts,
    Rings,
}

/// The round a page is walked in: the shared pages, or one pad's, one
/// Part's or one ring's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    Shared,
    Pad(PadId),
    Part(PartId),
    Ring(RingIdx),
}

impl Round {
    /// Its pad, on a pad's round.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.pad"))]
    pub const fn pad(self) -> Option<PadId> {
        match self {
            Round::Pad(p) => Some(p),
            Round::Shared | Round::Part(_) | Round::Ring(_) => None,
        }
    }

    /// Its ring, on a ring's round.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.ring"))]
    pub const fn ring(self) -> Option<RingIdx> {
        match self {
            Round::Ring(r) => Some(r),
            Round::Shared | Round::Pad(_) | Round::Part(_) => None,
        }
    }

    /// The prefix of its pages' labels and panels: `PAD3_`, `PART3_`,
    /// `RING2_`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.prefix"))]
    pub fn prefix(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            Round::Shared => Ok(()),
            Round::Pad(p) => {
                w.write_str(PAD.get())?;
                super::lit::uint(w, u32::from(p.number()))?;
                w.write_char('_')
            }
            Round::Part(p) => {
                w.write_str(PART.get())?;
                super::lit::uint(w, u32::from(p.number()))?;
                w.write_char('_')
            }
            Round::Ring(r) => {
                w.write_str(RING.get())?;
                super::lit::uint(w, u32::from(r.get()) + 1)?;
                w.write_char('_')
            }
        }
    }
}

impl Schema {
    /// Every schema, in hash order.
    pub const ALL: [Schema; SCHEMAS] = [
        Schema::Engine(EngineType::ALL[0]),
        Schema::Engine(EngineType::ALL[1]),
        Schema::Engine(EngineType::ALL[2]),
        Schema::Engine(EngineType::ALL[3]),
        Schema::Engine(EngineType::ALL[4]),
        Schema::Mixer,
        Schema::Matrix(EngineType::ALL[0]),
        Schema::Matrix(EngineType::ALL[1]),
        Schema::Matrix(EngineType::ALL[2]),
        Schema::Matrix(EngineType::ALL[3]),
        Schema::Matrix(EngineType::ALL[4]),
        Schema::Settings,
        Schema::Map,
        Schema::Orbit,
    ];

    /// Its place in `ALL`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.index"))]
    pub const fn index(self) -> usize {
        match self {
            Schema::Engine(e) => engine_index(e),
            Schema::Mixer => EngineType::ALL.len(),
            Schema::Matrix(e) => EngineType::ALL.len() + 1 + engine_index(e),
            Schema::Settings => 2 * EngineType::ALL.len() + 1,
            Schema::Map => 2 * EngineType::ALL.len() + 2,
            Schema::Orbit => 2 * EngineType::ALL.len() + 3,
        }
    }

    /// The schema `w` is the token of.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.parse"))]
    pub fn parse(w: &[u8]) -> Option<Schema> {
        if w.eq_ignore_ascii_case(MIXER.bytes()) {
            return Some(Schema::Mixer);
        }
        if w.eq_ignore_ascii_case(SETTINGS.bytes()) {
            return Some(Schema::Settings);
        }
        if w.eq_ignore_ascii_case(MAP.bytes()) {
            return Some(Schema::Map);
        }
        if w.eq_ignore_ascii_case(ORBIT.bytes()) {
            return Some(Schema::Orbit);
        }
        let mtx = MTX.bytes();
        match w.split_at_checked(mtx.len()) {
            Some((head, e)) if head.eq_ignore_ascii_case(mtx) => engine(e).map(Schema::Matrix),
            Some(_) | None => engine(w).map(Schema::Engine),
        }
    }

    /// The chain it walks; `None` for a schema described from its own
    /// tables (the matrix, MIDI MAP), the tree's leaves (SETTINGS), or
    /// ORBIT in a build without it.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.chain"))]
    pub const fn chain(self) -> Option<&'static ChainDef2> {
        match self {
            Schema::Engine(e) => Some(engine_chain(e)),
            Schema::Mixer => Some(&MIXER_CHANNEL_CHAIN),
            // ORBIT's pages, with its code only.
            Schema::Orbit if cfg!(feature = "orbit") => Some(&ORBIT_CHAIN),
            Schema::Orbit => None,
            Schema::Matrix(_) | Schema::Settings | Schema::Map => None,
        }
    }

    /// The tree's leaves it walks: SETTINGS', else none.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.leaves"))]
    pub const fn leaves(self) -> &'static [Leaf] {
        match self {
            Schema::Settings => &LEAVES,
            Schema::Engine(_) | Schema::Mixer | Schema::Matrix(_) | Schema::Map | Schema::Orbit => {
                &[]
            }
        }
    }

    /// What its rounds go over.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.rounds"))]
    pub const fn rounds(self) -> RoundKind {
        match self {
            Schema::Engine(EngineType::Drum) => RoundKind::Pads,
            Schema::Engine(
                EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va,
            ) => RoundKind::None,
            Schema::Mixer | Schema::Settings => RoundKind::Parts,
            Schema::Matrix(_) | Schema::Map => RoundKind::None,
            Schema::Orbit => RoundKind::Rings,
        }
    }

    /// `def` is described once a round, not once: a cell binds what the
    /// round names (the pad on DRUM, the Part or its OUT on the mixer and
    /// SETTINGS, the selected ring on ORBIT).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.in_round"))]
    pub const fn in_round(self, def: &BlockDef) -> bool {
        let mut i = 0;
        while i < def.params.len() {
            let b = def.params[i].binding;
            let hit = match self.rounds() {
                RoundKind::None => false,
                RoundKind::Pads => pad_binding(b),
                RoundKind::Parts => part_binding(b),
                RoundKind::Rings => matches!(b, SlotBinding::SelectedRing(_)),
            };
            if hit {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Its token, `ALGO`…, `MIXER`, `MTX-ALGO`…, `SETTINGS`, `MAP`,
    /// `ORBIT`, into `w`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.write"))]
    pub fn write(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            Schema::Engine(e) => w.write_str(e.disk_ident()),
            Schema::Mixer => w.write_str(MIXER.get()),
            Schema::Matrix(e) => {
                w.write_str(MTX.get())?;
                w.write_str(e.disk_ident())
            }
            Schema::Settings => w.write_str(SETTINGS.get()),
            Schema::Map => w.write_str(MAP.get()),
            Schema::Orbit => w.write_str(ORBIT.get()),
        }
    }
}

/// The chain the editor walks for engine `e`.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.schema.engine_chain")
)]
pub const fn engine_chain(e: EngineType) -> &'static ChainDef2 {
    match e {
        EngineType::Algo => &ALGO_FORMANT_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
        EngineType::Tape => &TAPE_CHAIN,
        EngineType::Va => &VA_CHAIN,
        EngineType::Drum => &DRUM_CHAIN,
    }
}

/// `e`'s place in `EngineType::ALL`.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.schema.engine_index")
)]
const fn engine_index(e: EngineType) -> usize {
    let mut i = 0;
    while i < EngineType::ALL.len() {
        if EngineType::ALL[i] as u8 == e as u8 {
            return i;
        }
        i += 1;
    }
    0
}

/// The engine `w` names, as `ed hello` names it.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.engine"))]
fn engine(w: &[u8]) -> Option<EngineType> {
    EngineType::ALL
        .into_iter()
        .find(|e| e.disk_ident().as_bytes().eq_ignore_ascii_case(w))
}

/// Whether a cell bound `b` resolves against the pad (DRUM's): a pad's
/// param, its machine's knobs, its filter's panel, its routes. FLT's
/// sub-pages' panel cells count: on DRUM they read the pad's KIND.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.schema.pad_binding")
)]
const fn pad_binding(b: SlotBinding) -> bool {
    match b {
        SlotBinding::Param(a) => a.block.is_pad(),
        SlotBinding::SelectPad
        | SlotBinding::Knob(_)
        | SlotBinding::PadFilterPanel(_)
        | SlotBinding::FilterPanel(_)
        | SlotBinding::SelectRoute
        | SlotBinding::SelectedRoute(_) => true,
        SlotBinding::Empty
        | SlotBinding::SelectedOp(_)
        | SlotBinding::SelectOp
        | SlotBinding::Legacy(_)
        | SlotBinding::FxPanel(_)
        | SlotBinding::EnvPanel(..)
        | SlotBinding::LfoPanel(..)
        | SlotBinding::ModalPanel(..)
        | SlotBinding::OpPanel(..)
        | SlotBinding::Machine(_)
        | SlotBinding::Source
        | SlotBinding::ReverbPanel(..)
        | SlotBinding::SelectedRing(_)
        | SlotBinding::Orbit(_) => false,
    }
}

/// Whether a cell bound `b` is a Part's own: the active Part's mix or its
/// Sound's OUT (the sends), which a round names.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.schema.part_binding")
)]
const fn part_binding(b: SlotBinding) -> bool {
    match b {
        SlotBinding::Param(a) => matches!(a.block, BlockRef::Part | BlockRef::Out),
        SlotBinding::Empty
        | SlotBinding::SelectPad
        | SlotBinding::Knob(_)
        | SlotBinding::PadFilterPanel(_)
        | SlotBinding::FilterPanel(_)
        | SlotBinding::SelectRoute
        | SlotBinding::SelectedRoute(_)
        | SlotBinding::SelectedOp(_)
        | SlotBinding::SelectOp
        | SlotBinding::Legacy(_)
        | SlotBinding::FxPanel(_)
        | SlotBinding::EnvPanel(..)
        | SlotBinding::LfoPanel(..)
        | SlotBinding::ModalPanel(..)
        | SlotBinding::OpPanel(..)
        | SlotBinding::Machine(_)
        | SlotBinding::Source
        | SlotBinding::ReverbPanel(..)
        | SlotBinding::SelectedRing(_)
        | SlotBinding::Orbit(_) => false,
    }
}

/// The hashes `ed schema` answered this boot, one a schema. Uninit past
/// `made`'s bits, so `EdState::new`, which bank 1 builds in place
/// (`take_ed`), writes one byte for them.
pub struct Hashes {
    /// Bit `i`: schema `i`'s hash is made.
    made: u16,
    h: [MaybeUninit<u64>; SCHEMAS],
}

const _: () = assert!(SCHEMAS <= u16::BITS as usize);

impl Hashes {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.new"))]
    pub const fn new() -> Hashes {
        Hashes {
            made: 0,
            h: [MaybeUninit::uninit(); SCHEMAS],
        }
    }

    /// `s`'s hash: kept, or made now with `make` and kept.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.of"))]
    pub fn of(&mut self, s: Schema, make: impl FnOnce() -> u64) -> u64 {
        let i = s.index();
        let Some(slot) = self.h.get_mut(i) else {
            return make();
        };
        let bit = 1 << i;
        if self.made & bit != 0 {
            // SAFETY: bit `i` of `made` is set only below, after slot `i`
            // is written.
            return unsafe { slot.assume_init_read() };
        }
        let h = make();
        slot.write(h);
        self.made |= bit;
        h
    }
}
