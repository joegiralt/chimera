//! What `ed describe` describes (ADR 0289): an engine's pages, or the
//! mixer's. A `Schema` is parsed once at the boundary from its token and
//! walked by `Pages`. Its pages are shared, then each round's: a pad's on
//! DRUM, a Part's on the mixer.

use core::fmt::{self, Write};
use core::mem::MaybeUninit;

use super::lit::lit;
use crate::dsp::drum::types::PadId;
use crate::params::EngineType;
use crate::project::PartId;
use crate::ui::block_def::{BlockDef, ChainDef2, SlotBinding};
use crate::ui::block_registry::{
    ALGO_FORMANT_CHAIN, DRUM_CHAIN, MIXER_CHANNEL_CHAIN, MODAL_PLUCK_CHAIN, TAPE_CHAIN, VA_CHAIN,
};
use chimera_engine::addr::BlockRef;
use chimera_engine::block::DiskCode;

/// A schema the editor describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schema {
    /// An engine's pages, the followed Part's (protocol 3's).
    Engine(EngineType),
    /// MIX + B*n*: each Part's PRT, VOI, EQ and SND, then the buses.
    Mixer,
}

lit!(
    /// The token of `Schema::Mixer`.
    MIXER = "MIXER"
);
lit!(
    /// A Part's round's prefix, before its number.
    PART = "PART"
);

/// How many schemas there are: `ed schema`'s hashes, one each.
pub const SCHEMAS: usize = EngineType::ALL.len() + 1;

/// What a schema's pages go round: nothing, each pad or each Part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundKind {
    None,
    Pads,
    Parts,
}

/// The round a page is walked in: the shared pages, or one pad's or one
/// Part's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    Shared,
    Pad(PadId),
    Part(PartId),
}

impl Round {
    /// Its pad, on a pad's round.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.pad"))]
    pub const fn pad(self) -> Option<PadId> {
        match self {
            Round::Pad(p) => Some(p),
            Round::Shared | Round::Part(_) => None,
        }
    }

    /// The prefix of its pages' labels and panels: `PAD3_`, `PART3_`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.prefix"))]
    pub fn prefix(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            Round::Shared => Ok(()),
            Round::Pad(p) => write!(w, "PAD{}_", p.number()),
            Round::Part(p) => {
                w.write_str(PART.get())?;
                super::lit::uint(w, u32::from(p.number()))?;
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
    ];

    /// Its place in `ALL`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.index"))]
    pub const fn index(self) -> usize {
        match self {
            Schema::Engine(e) => {
                let mut i = 0;
                while i < EngineType::ALL.len() {
                    if EngineType::ALL[i] as u8 == e as u8 {
                        return i;
                    }
                    i += 1;
                }
                0
            }
            Schema::Mixer => EngineType::ALL.len(),
        }
    }

    /// The schema `w` is the token of.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.parse"))]
    pub fn parse(w: &[u8]) -> Option<Schema> {
        if w.eq_ignore_ascii_case(MIXER.bytes()) {
            return Some(Schema::Mixer);
        }
        EngineType::ALL
            .into_iter()
            .find(|e| e.disk_ident().as_bytes().eq_ignore_ascii_case(w))
            .map(Schema::Engine)
    }

    /// The chain it walks.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.chain"))]
    pub const fn chain(self) -> &'static ChainDef2 {
        match self {
            Schema::Engine(EngineType::Algo) => &ALGO_FORMANT_CHAIN,
            Schema::Engine(EngineType::Modal) => &MODAL_PLUCK_CHAIN,
            Schema::Engine(EngineType::Tape) => &TAPE_CHAIN,
            Schema::Engine(EngineType::Va) => &VA_CHAIN,
            Schema::Engine(EngineType::Drum) => &DRUM_CHAIN,
            Schema::Mixer => &MIXER_CHANNEL_CHAIN,
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
            Schema::Mixer => RoundKind::Parts,
        }
    }

    /// `def` is described once a round, not once: a cell binds what the
    /// round names (the pad on DRUM, the Part or its OUT on the mixer).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.in_round"))]
    pub const fn in_round(self, def: &BlockDef) -> bool {
        let mut i = 0;
        while i < def.params.len() {
            let b = def.params[i].binding;
            let hit = match self.rounds() {
                RoundKind::None => false,
                RoundKind::Pads => pad_binding(b),
                RoundKind::Parts => part_binding(b),
            };
            if hit {
                return true;
            }
            i += 1;
        }
        false
    }

    /// `ALGO`…, `MIXER`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.schema.token"))]
    pub fn token(self) -> &'static str {
        match self {
            Schema::Engine(e) => e.disk_ident(),
            Schema::Mixer => MIXER.get(),
        }
    }
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
