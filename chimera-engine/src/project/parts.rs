//! The Parts and the shared FX: what the audio plays.

use crate::addr::{BlockRead, BlockRef, BlockView, BlockViewMut, Blocks};
use crate::block::Block;
use crate::dsp::fx_bus::FxParams;
use crate::hw::MAX_PARTS;
use crate::params::EngineType;
use crate::part::PartParams;
use crate::preset::Sound;
use crate::tracked::Tracked;

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
/// Only `project` makes one (with `controller-maps` the error is the
/// missing private `map_kept`, so no code is named):
///
/// ```compile_fail
/// use chimera_engine::params::EngineType;
/// use chimera_engine::part::PartParams;
/// use chimera_engine::preset::Sound;
/// use chimera_engine::project::{Loads, Origin, Part};
/// use chimera_engine::tracked::Tracked;
/// let _ = Part {
///     sound: Tracked::fresh(Sound::init(EngineType::Algo)),
///     origin: Origin::Init(EngineType::Algo),
///     mix: PartParams::default(),
///     loads: Loads::NONE,
/// };
/// ```
pub struct Part {
    /// Written only through its `Tracked`: every write, by any path (an
    /// edit, a load, the codec), gives it a stamp no Sound had, which the
    /// publish carries, so the audio re-reads this Part alone (ADR 0112).
    pub sound: Tracked<Sound>,
    pub(in crate::project) origin: Origin,
    pub mix: PartParams,
    pub(in crate::project) loads: Loads,
    /// KEEP MAP (ADR 0253): the `sound_crc` the Part had once its kept
    /// rows were written over a load's. While it matches, the Part reads
    /// Clean. Never saved.
    #[cfg(feature = "controller-maps")]
    pub(in crate::project) map_kept: Option<u32>,
}

impl Part {
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Bumped by each Sound load into this Part.
    pub fn loads(&self) -> Loads {
        self.loads
    }
}

/// A Part's Sound loads, counted (wrapping): a change is a new Sound, which
/// the audio lands on rather than glides to (ADR 0107).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loads(u8);

impl Loads {
    /// No load yet.
    pub const NONE: Self = Self(0);

    pub(in crate::project) fn bump(&mut self) {
        *self = self.next();
    }

    /// The count after one more load: what a Sound loaded outside a
    /// Project (the bench's) is published with.
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

pub(super) fn new_part(i: usize) -> Part {
    Part {
        sound: Tracked::fresh(Sound::init(EngineType::Algo)),
        origin: Origin::Init(EngineType::Algo),
        mix: PartParams::for_part(i),
        loads: Loads::default(),
        #[cfg(feature = "controller-maps")]
        map_kept: None,
    }
}

/// All Parts + FX: what the audio plays. The pool and the name are the
/// `Project`'s. A Part is never reached whole by `&mut`, so no Origin is
/// swapped in from elsewhere:
///
/// ```compile_fail,E0616
/// use chimera_engine::preset::Performance;
/// let (mut a, mut b) = (Performance::new(), Performance::new());
/// core::mem::swap(&mut a.parts[0], &mut b.parts[0]);
/// ```
pub struct Performance {
    pub(in crate::project) parts: [Part; MAX_PARTS],
    /// Chorus, delay and reverb: shared by every Part, not per Sound.
    pub fx: FxParams,
}

// `init_in_place` writes each field: a new one fails to build here first.
crate::in_place::field_list!(Performance => Performance { parts, fx });

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

    /// `new`, built in place a Part at a time: no 6 KB temporary on the
    /// stack, one Part's at most.
    pub fn init_in_place(slot: &mut core::mem::MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; each of the `MAX_PARTS` Parts
        // and `fx` (every field, `field_list!` above) is written once, through raw pointers into the slot,
        // before `assume_init_mut`.
        unsafe {
            let parts = core::ptr::addr_of_mut!((*p).parts).cast::<Part>();
            for i in 0..MAX_PARTS {
                parts.add(i).write(new_part(i));
            }
            core::ptr::addr_of_mut!((*p).fx).write(FxParams::default());
            slot.assume_init_mut()
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
/// use chimera_engine::preset::Performance;
/// use chimera_engine::project::{PartId, Project};
/// let (mut p, _) = Project::boxed();
/// let mut other = Performance::new();
/// core::mem::swap(
///     p.edit_part(PartId::ALL[0]).part,
///     other.edit(PartId::ALL[0]).part,
/// );
/// ```
pub struct PartEdit<'a> {
    /// Its stamp moves only if the Sound is written: a mix or FX edit
    /// leaves the Part's knobs unread.
    pub sound: &'a mut Tracked<Sound>,
    pub mix: &'a mut PartParams,
    pub fx: &'a mut FxParams,
}

impl BlockRead for PartEdit<'_> {
    fn block(&self, b: BlockRef) -> Option<BlockView<'_>> {
        part_block(self.sound, self.mix, self.fx, b)
    }
}

impl Blocks for PartEdit<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>> {
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
    pub fn block(&self, b: BlockRef) -> Option<BlockView<'a>> {
        part_block(self.sound, self.mix, self.fx, b)
    }
}

impl BlockRead for PartRead<'_> {
    fn block(&self, b: BlockRef) -> Option<BlockView<'_>> {
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
) -> Option<BlockView<'a>> {
    let plain = |b: &'a dyn Block| Some(BlockView::Plain(b));
    match b {
        BlockRef::Chorus => plain(&fx.chorus),
        BlockRef::Delay => plain(&fx.delay),
        BlockRef::Reverb => plain(&fx.reverb),
        BlockRef::Tape => plain(&fx.tape),
        BlockRef::Comp => plain(&fx.comp),
        BlockRef::Part => plain(mix),
        BlockRef::Theme
        | BlockRef::Midi
        | BlockRef::PartMix(_)
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Tempo
        | BlockRef::Random => None,
        BlockRef::Modal
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch => sound.params.block(b),
    }
}

/// `part_block`, mutable.
pub fn part_block_mut<'a>(
    sound: &'a mut Tracked<Sound>,
    mix: &'a mut PartParams,
    fx: &'a mut FxParams,
    b: BlockRef,
) -> Option<BlockViewMut<'a>> {
    let plain = |b: &'a mut dyn Block| Some(BlockViewMut::Plain(b));
    match b {
        BlockRef::Chorus => plain(&mut fx.chorus),
        BlockRef::Delay => plain(&mut fx.delay),
        BlockRef::Reverb => plain(&mut fx.reverb),
        BlockRef::Tape => plain(&mut fx.tape),
        BlockRef::Comp => plain(&mut fx.comp),
        BlockRef::Part => plain(mix),
        BlockRef::Theme
        | BlockRef::Midi
        | BlockRef::PartMix(_)
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Tempo
        | BlockRef::Random => None,
        BlockRef::Modal
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch => sound.params.block_mut(b),
    }
}
