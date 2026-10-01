//! The Parts and the shared FX: what the audio plays.

use crate::addr::{BlockRead, BlockRef, Blocks};
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
/// `Project`'s. A Part is never reached whole by `&mut`, so no Origin is
/// swapped in from elsewhere:
///
/// ```compile_fail,E0616
/// use chimera_core::preset::Performance;
/// let (mut a, mut b) = (Performance::new(), Performance::new());
/// core::mem::swap(&mut a.parts[0], &mut b.parts[0]);
/// ```
pub struct Performance {
    pub(in crate::project) parts: [Part; MAX_PARTS],
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

    pub fn parts(&self) -> &[Part; MAX_PARTS] {
        &self.parts
    }

    /// Part `p` as the pages edit it: its Sound and mix plus the shared FX.
    pub fn edit(&mut self, p: PartId) -> PartEdit<'_> {
        let Part { sound, mix, .. } = &mut self.parts[p.index()];
        PartEdit {
            sound,
            mix,
            fx: &mut self.fx,
        }
    }
}

/// One Part's Sound and mix settings and the Performance's FX, borrowed
/// together so a page can address any block by `BlockRef`. Never the Part
/// itself, so its Origin can't be swapped:
///
/// ```compile_fail,E0609
/// use chimera_core::preset::Performance;
/// use chimera_core::project::{PartId, Project};
/// let (mut p, _) = Project::boxed();
/// let mut other = Performance::new();
/// core::mem::swap(
///     p.edit_part(PartId::ALL[0]).part,
///     other.edit(PartId::ALL[0]).part,
/// );
/// ```
pub struct PartEdit<'a> {
    pub sound: &'a mut Sound,
    pub mix: &'a mut PartParams,
    pub fx: &'a mut FxParams,
}

impl BlockRead for PartEdit<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        part_block(self.sound, self.mix, self.fx, b)
    }
}

impl Blocks for PartEdit<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        part_block_mut(self.sound, self.mix, self.fx, b)
    }
}

/// `PartEdit`'s shared twin: a Part as the pages read it, never written
/// through, so reading it leaves `Project::rev` where it was.
pub struct PartRead<'a> {
    pub sound: &'a Sound,
    pub mix: &'a PartParams,
    pub fx: &'a FxParams,
}

impl<'a> PartRead<'a> {
    /// `BlockRead::block`, for as long as the Part is borrowed, not the view.
    pub fn block(&self, b: BlockRef) -> Option<&'a dyn Block> {
        part_block(self.sound, self.mix, self.fx, b)
    }
}

impl BlockRead for PartRead<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        PartRead::block(self, b)
    }
}

/// A Part's block `b` (its Sound's or its mix), or the FX's; `None` for the
/// blocks the UI holds.
pub fn part_block<'a>(
    sound: &'a Sound,
    mix: &'a PartParams,
    fx: &'a FxParams,
    b: BlockRef,
) -> Option<&'a dyn Block> {
    match b {
        BlockRef::Chorus => Some(&fx.chorus),
        BlockRef::Delay => Some(&fx.delay),
        BlockRef::Reverb => Some(&fx.reverb),
        BlockRef::Tape => Some(&fx.tape),
        BlockRef::Comp => Some(&fx.comp),
        BlockRef::Part => Some(mix),
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
        | BlockRef::Pitch => sound.params.block(b),
    }
}

/// `part_block`, mutable.
pub fn part_block_mut<'a>(
    sound: &'a mut Sound,
    mix: &'a mut PartParams,
    fx: &'a mut FxParams,
    b: BlockRef,
) -> Option<&'a mut dyn Block> {
    match b {
        BlockRef::Chorus => Some(&mut fx.chorus),
        BlockRef::Delay => Some(&mut fx.delay),
        BlockRef::Reverb => Some(&mut fx.reverb),
        BlockRef::Tape => Some(&mut fx.tape),
        BlockRef::Comp => Some(&mut fx.comp),
        BlockRef::Part => Some(mix),
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
        | BlockRef::Pitch => sound.params.block_mut(b),
    }
}
