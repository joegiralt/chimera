use crate::addr::{BlockRef, Blocks};
use crate::block::Block;
use crate::dsp::fx_bus::FxParams;
use crate::hw::MAX_PARTS;
use crate::mod_path::ModDestRegistry;
use crate::modulation::ModState;
use crate::params::{EngineType, ParamSnapshot};
use crate::part::PartParams;

pub const POOL_SIZE: usize = 32;
pub const NAME_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ChainType {
    #[default]
    PizzaPoly = 0,
    Modal = 1,
    Fm = 2,
}

impl ChainType {
    pub const ALL: [ChainType; 3] = [ChainType::PizzaPoly, ChainType::Modal, ChainType::Fm];

    /// Short display label for the chain type.
    pub fn label(self) -> &'static str {
        match self {
            ChainType::PizzaPoly => "Pizza",
            ChainType::Modal => "Modal",
            ChainType::Fm => "FM",
        }
    }

    /// The engine this chain plays (spec §6).
    pub const fn engine(self) -> EngineType {
        match self {
            ChainType::PizzaPoly => EngineType::Pizza,
            ChainType::Modal => EngineType::Modal,
            ChainType::Fm => EngineType::Fm,
        }
    }
}

#[derive(Clone)]
#[repr(C)]
pub struct Sound {
    pub name: [u8; NAME_LEN],
    pub chain_type: ChainType,
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,
}

impl Sound {
    pub fn init(chain_type: ChainType) -> Self {
        let mut name = [0u8; NAME_LEN];
        let tag = b"(init)";
        name[..tag.len()].copy_from_slice(tag);
        Self {
            name,
            chain_type,
            params: ParamSnapshot::for_engine(chain_type.engine()),
            // No pre-wired routes: the matrix starts empty on every chain
            // (spec §4 "FM pre-wire removed").
            mod_state: ModState::new(),
            dest_registry: ModDestRegistry::new(),
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        core::str::from_utf8(&self.name[..end]).unwrap_or("???")
    }
}

pub struct SoundPool {
    slots: [Option<Sound>; POOL_SIZE],
}

impl SoundPool {
    pub fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
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

impl Part {
    /// An init Sound of `chain_type` with part 1's mix settings.
    pub fn new(chain_type: ChainType) -> Self {
        Self {
            sound: Sound::init(chain_type),
            loaded_from: None,
            mix: PartParams::default(),
        }
    }

    /// Replace the Sound with an init one; channel and mix stay.
    pub fn load_init(&mut self, chain_type: ChainType) {
        self.sound = Sound::init(chain_type);
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

impl Performance {
    pub fn new() -> Self {
        Self {
            name: *b"New Performance\0",
            parts: core::array::from_fn(|i| Part { mix: PartParams::for_part(i), ..Part::new(ChainType::PizzaPoly) }),
            fx: FxParams::default(),
        }
    }

    /// Part `part` as the pages edit it: its Sound plus the shared FX.
    pub fn edit(&mut self, part: usize) -> PartEdit<'_> {
        PartEdit { part: &mut self.parts[part], fx: &mut self.fx }
    }
}

/// One Part and the Performance's FX, borrowed together so a page can
/// address any block by `BlockRef`.
pub struct PartEdit<'a> {
    pub part: &'a mut Part,
    pub fx: &'a mut FxParams,
}

impl Blocks for PartEdit<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        match b {
            BlockRef::Chorus => Some(&self.fx.chorus),
            BlockRef::Delay => Some(&self.fx.delay),
            BlockRef::Reverb => Some(&self.fx.reverb),
            BlockRef::Pizza
            | BlockRef::Modal
            | BlockRef::Fm
            | BlockRef::FmOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::AmpEnv
            | BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Out => self.part.sound.params.block(b),
        }
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::Chorus => Some(&mut self.fx.chorus),
            BlockRef::Delay => Some(&mut self.fx.delay),
            BlockRef::Reverb => Some(&mut self.fx.reverb),
            BlockRef::Pizza
            | BlockRef::Modal
            | BlockRef::Fm
            | BlockRef::FmOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::AmpEnv
            | BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Out => self.part.sound.params.block_mut(b),
        }
    }
}
