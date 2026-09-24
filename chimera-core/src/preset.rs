use crate::mod_path::ModDestRegistry;
use crate::modulation::ModState;
use crate::params::{EngineType, ParamSnapshot};

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

pub struct Part {
    pub sound: Sound,
    pub loaded_from: Option<u8>,
}

impl Part {
    pub fn new(chain_type: ChainType) -> Self {
        Self {
            sound: Sound::init(chain_type),
            loaded_from: None,
        }
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

pub struct MixerState {
    pub levels: [f32; 6],
    pub pans: [f32; 6],
    pub sends: [f32; 6],
}

impl Default for MixerState {
    fn default() -> Self {
        Self {
            levels: [0.8; 6],
            pans: [0.0; 6],
            sends: [0.0; 6],
        }
    }
}

pub struct Performance {
    pub name: [u8; NAME_LEN],
    pub pool: SoundPool,
    pub parts: [Part; 6],
    pub mixer: MixerState,
}

impl Performance {
    pub fn new() -> Self {
        Self {
            name: *b"New Performance\0",
            pool: SoundPool::new(),
            parts: core::array::from_fn(|_| Part::new(ChainType::PizzaPoly)),
            mixer: MixerState::default(),
        }
    }
}
