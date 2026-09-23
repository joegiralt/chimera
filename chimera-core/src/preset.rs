use crate::mod_path::{ModDestRegistry, ParamPath};
use crate::modulation::ModState;
use crate::params::ParamSnapshot;

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
}

#[derive(Clone)]
#[repr(C)]
pub struct Patch {
    pub name: [u8; NAME_LEN],
    pub chain_type: ChainType,
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,
}

impl Patch {
    pub fn init(chain_type: ChainType) -> Self {
        let mut name = [0u8; NAME_LEN];
        let tag = b"(init)";
        name[..tag.len()].copy_from_slice(tag);
        let mut params = ParamSnapshot::default();
        let (mod_state, dest_registry) = match chain_type {
            ChainType::PizzaPoly => (ModState::default(), ModDestRegistry::new()),
            ChainType::Modal => {
                params.engine = crate::params::EngineType::Modal;
                (ModState::default(), ModDestRegistry::new())
            }
            ChainType::Fm => {
                params.engine = crate::params::EngineType::Fm;
                // Pre-wire: 4 envelope sources → 4 FM operator levels
                let mut reg = ModDestRegistry::new();
                for (op, label) in [(0u8, *b"O1 Lvl\0\0"), (1, *b"O2 Lvl\0\0"), (2, *b"O3 Lvl\0\0"), (3, *b"O4 Lvl\0\0")] {
                    let _ = reg.add(ChainType::Fm, ParamPath::FmOp { op, param: 2 }, label);
                }
                let mut ms = ModState::from_registry(&reg, ChainType::Fm, 4);
                for i in 0..4 {
                    ms.set_amount(i, i, 127); // E(i+1) → Op(i+1) Level full
                }
                (ms, reg)
            }
        };
        Self {
            name,
            chain_type,
            params,
            mod_state,
            dest_registry,
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        core::str::from_utf8(&self.name[..end]).unwrap_or("???")
    }
}

pub struct SoundPool {
    slots: [Option<Patch>; POOL_SIZE],
}

impl SoundPool {
    pub fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    pub fn get(&self, index: usize) -> Option<&Patch> {
        self.slots.get(index)?.as_ref()
    }

    pub fn store(&mut self, index: usize, patch: Patch) {
        if index < POOL_SIZE {
            self.slots[index] = Some(patch);
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

pub struct Track {
    pub patch: Patch,
    pub loaded_from: Option<u8>,
}

impl Track {
    pub fn new(chain_type: ChainType) -> Self {
        Self {
            patch: Patch::init(chain_type),
            loaded_from: None,
        }
    }

    pub fn load_from_pool(&mut self, pool: &SoundPool, slot: usize) {
        if let Some(p) = pool.get(slot) {
            self.patch = p.clone();
            self.loaded_from = Some(slot as u8);
        }
    }

    pub fn save_to_pool(&self, pool: &mut SoundPool, slot: usize) {
        pool.store(slot, self.patch.clone());
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

pub struct Project {
    pub name: [u8; NAME_LEN],
    pub pool: SoundPool,
    pub tracks: [Track; 6],
    pub mixer: MixerState,
}

impl Project {
    pub fn new() -> Self {
        Self {
            name: *b"New Project\0\0\0\0\0",
            pool: SoundPool::new(),
            tracks: core::array::from_fn(|_| Track::new(ChainType::PizzaPoly)),
            mixer: MixerState::default(),
        }
    }
}
