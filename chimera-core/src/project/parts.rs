//! The Parts and the shared FX: what the audio plays.

use crate::addr::{BlockRef, Blocks};
use crate::block::Block;
use crate::dsp::fx_bus::FxParams;
use crate::hw::MAX_PARTS;
use crate::params::EngineType;
use crate::part::PartParams;
use crate::preset::Sound;

use super::ids::{PartId, SlotId};

/// Where a Part's Sound came from: set only by a load, a save over its slot
/// and a revert, so its marks are derived, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The slot, its generation and the Sound's `sound_crc` when loaded.
    Slot {
        slot: SlotId,
        generation: u16,
        crc: u32,
    },
    Init(EngineType),
}

/// A slot playing one Sound, with its MIDI channel, mode, output and mix.
/// Only `project` makes one:
///
/// ```compile_fail,E0451
/// use chimera_core::params::EngineType;
/// use chimera_core::part::PartParams;
/// use chimera_core::preset::Sound;
/// use chimera_core::project::{Origin, Part};
/// let _ = Part {
///     sound: Sound::init(EngineType::Algo),
///     origin: Origin::Init(EngineType::Algo),
///     mix: PartParams::default(),
/// };
/// ```
pub struct Part {
    pub sound: Sound,
    pub(in crate::project) origin: Origin,
    pub mix: PartParams,
}

impl Part {
    pub fn origin(&self) -> Origin {
        self.origin
    }
}

fn new_part(i: usize) -> Part {
    Part {
        sound: Sound::init(EngineType::Algo),
        origin: Origin::Init(EngineType::Algo),
        mix: PartParams::for_part(i),
    }
}

/// All Parts + FX: what the audio plays. The pool and the name are the
/// `Project`'s.
pub struct Performance {
    pub parts: [Part; MAX_PARTS],
    /// Chorus, delay and reverb: shared by every Part, not per Sound.
    pub fx: FxParams,
}

impl Default for Performance {
    fn default() -> Self {
        Self::new()
    }
}

impl Performance {
    /// Every Part an INIT Algo Sound on its own channel; the FX off.
    pub fn new() -> Self {
        Self {
            parts: core::array::from_fn(new_part),
            fx: FxParams::default(),
        }
    }

    /// `new`, a Part at a time: no 6 KB temporary on the stack.
    pub(in crate::project) fn reset(&mut self) {
        for (i, p) in self.parts.iter_mut().enumerate() {
            *p = new_part(i);
        }
        self.fx = FxParams::default();
    }

    pub fn part(&self, p: PartId) -> &Part {
        &self.parts[p.index()]
    }

    /// Part `p` as the pages edit it: its Sound plus the shared FX.
    pub fn edit(&mut self, p: PartId) -> PartEdit<'_> {
        PartEdit {
            part: &mut self.parts[p.index()],
            fx: &mut self.fx,
        }
    }
}

/// One Part (Sound + mix settings) and the Performance's FX, borrowed
/// together so a page can address any block by `BlockRef`.
pub struct PartEdit<'a> {
    pub part: &'a mut Part,
    pub fx: &'a mut FxParams,
}

impl Blocks for PartEdit<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        part_block(self.part, self.fx, b)
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        part_block_mut(self.part, self.fx, b)
    }
}

/// `part`'s block `b`, or the FX's; `None` for the blocks the UI holds.
pub fn part_block<'a>(part: &'a Part, fx: &'a FxParams, b: BlockRef) -> Option<&'a dyn Block> {
    match b {
        BlockRef::Chorus => Some(&fx.chorus),
        BlockRef::Delay => Some(&fx.delay),
        BlockRef::Reverb => Some(&fx.reverb),
        BlockRef::Tape => Some(&fx.tape),
        BlockRef::Comp => Some(&fx.comp),
        BlockRef::Part => Some(&part.mix),
        BlockRef::Theme => None,
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Drive
        | BlockRef::Filter
        | BlockRef::Folder
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch => part.sound.params.block(b),
    }
}

/// `part_block`, mutable.
pub fn part_block_mut<'a>(
    part: &'a mut Part,
    fx: &'a mut FxParams,
    b: BlockRef,
) -> Option<&'a mut dyn Block> {
    match b {
        BlockRef::Chorus => Some(&mut fx.chorus),
        BlockRef::Delay => Some(&mut fx.delay),
        BlockRef::Reverb => Some(&mut fx.reverb),
        BlockRef::Tape => Some(&mut fx.tape),
        BlockRef::Comp => Some(&mut fx.comp),
        BlockRef::Part => Some(&mut part.mix),
        BlockRef::Theme => None,
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Drive
        | BlockRef::Filter
        | BlockRef::Folder
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch => part.sound.params.block_mut(b),
    }
}
