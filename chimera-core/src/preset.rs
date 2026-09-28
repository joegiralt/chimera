use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::addr::{BlockRef, Blocks};
use crate::block::Block;
use crate::block::ParamId;
use crate::dsp::fx_bus::FxParams;
use crate::hw::MAX_PARTS;
use crate::in_place::by_value;
use crate::mod_path::ModDestRegistry;
use crate::modulation::ModState;
use crate::params::{EngineType, ParamSnapshot};
use crate::part::{CHANNEL_SPECS, PartParams};

pub const POOL_SIZE: usize = 32;
pub const NAME_LEN: usize = 16;

#[derive(Clone)]
#[repr(C)]
pub struct Sound {
    pub name: [u8; NAME_LEN],
    /// Carries the engine: `engine()` reads it from here, the one place.
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,
}

impl Sound {
    pub fn init(engine: EngineType) -> Self {
        let mut name = [0u8; NAME_LEN];
        let tag = b"(init)";
        name[..tag.len()].copy_from_slice(tag);
        Self {
            name,
            params: ParamSnapshot::for_engine(engine),
            // No pre-wired routes: the matrix starts empty on every chain
            // (spec §4 "FM pre-wire removed").
            mod_state: ModState::new(),
            dest_registry: ModDestRegistry::new(),
        }
    }

    /// The engine this Sound plays, and so the chain the UI shows for it.
    pub fn engine(&self) -> EngineType {
        self.params.engine()
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        core::str::from_utf8(&self.name[..end]).unwrap_or("???")
    }
}

pub struct SoundPool {
    slots: [Option<Sound>; POOL_SIZE],
}

impl Default for SoundPool {
    fn default() -> Self {
        Self::new()
    }
}

impl SoundPool {
    pub fn new() -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(Self::init_in_place) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every slot is written once
        // before `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).slots).cast::<Option<Sound>>();
            for i in 0..POOL_SIZE {
                slots.add(i).write(None);
            }
            slot.assume_init_mut()
        }
    }

    pub fn get(&self, index: usize) -> Option<&Sound> {
        self.slots.get(index)?.as_ref()
    }

    pub fn store(&mut self, index: usize, sound: Sound) {
        if index < POOL_SIZE {
            self.slots[index] = Some(sound);
        }
    }

    pub fn clear(&mut self, index: usize) {
        if index < POOL_SIZE {
            self.slots[index] = None;
        }
    }

    pub fn slot_count(&self) -> usize {
        POOL_SIZE
    }
}

/// A slot playing one Sound, with its MIDI channel, mode, output and mix.
pub struct Part {
    pub sound: Sound,
    pub loaded_from: Option<u8>,
    pub mix: PartParams,
}

/// System › MIDI Setup: every Part's channel, param `n` for Part n + 1.
impl Block for [Part; MAX_PARTS] {
    fn specs(&self) -> &'static [crate::block::ParamSpec] {
        &CHANNEL_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        self.as_slice()
            .get(id.0 as usize)
            .map_or(0.0, |p| p.mix.get(PartParams::CHANNEL))
    }

    fn write(&mut self, id: ParamId, v: f32) {
        if let Some(p) = self.as_mut_slice().get_mut(id.0 as usize) {
            p.mix.write(PartParams::CHANNEL, v);
        }
    }
}

impl Part {
    /// An init Sound of `engine` with part 1's mix settings.
    pub fn new(engine: EngineType) -> Self {
        Self {
            sound: Sound::init(engine),
            loaded_from: None,
            mix: PartParams::default(),
        }
    }

    /// Replace the Sound with an init one; channel and mix stay.
    pub fn load_init(&mut self, engine: EngineType) {
        self.sound = Sound::init(engine);
        self.loaded_from = None;
    }

    pub fn load_from_pool(&mut self, pool: &SoundPool, slot: usize) {
        if let Some(p) = pool.get(slot) {
            self.sound = p.clone();
            self.loaded_from = Some(slot as u8);
        }
    }

    pub fn save_to_pool(&self, pool: &mut SoundPool, slot: usize) {
        pool.store(slot, self.sound.clone());
    }
}

/// All Parts + FX: the whole setup you play and save. The `SoundPool` is
/// not part of it (it stays on the UI side).
pub struct Performance {
    pub name: [u8; NAME_LEN],
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
    pub fn new() -> Self {
        Self {
            name: *b"New Performance\0",
            parts: core::array::from_fn(|i| Part {
                mix: PartParams::for_part(i),
                ..Part::new(EngineType::Algo)
            }),
            fx: FxParams::default(),
        }
    }

    /// Part `part` as the pages edit it: its Sound plus the shared FX.
    pub fn edit(&mut self, part: usize) -> PartEdit<'_> {
        PartEdit {
            part: &mut self.parts[part],
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
        BlockRef::Theme | BlockRef::Channels => None,
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Drive
        | BlockRef::Filter
        | BlockRef::Folder
        | BlockRef::AmpEnv
        | BlockRef::FilterEnv
        | BlockRef::AuxEnv
        | BlockRef::Lfo
        | BlockRef::Out => part.sound.params.block(b),
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
        BlockRef::Theme | BlockRef::Channels => None,
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Drive
        | BlockRef::Filter
        | BlockRef::Folder
        | BlockRef::AmpEnv
        | BlockRef::FilterEnv
        | BlockRef::AuxEnv
        | BlockRef::Lfo
        | BlockRef::Out => part.sound.params.block_mut(b),
    }
}
