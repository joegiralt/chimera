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

#[derive(Clone)]
#[repr(C)]
pub struct Patch {
    pub name: [u8; NAME_LEN],
    pub chain_type: ChainType,
    pub params: ParamSnapshot,
    pub mod_state: ModState,
}

impl Patch {
    pub fn init(chain_type: ChainType) -> Self {
        let mut name = [0u8; NAME_LEN];
        let tag = b"(init)";
        name[..tag.len()].copy_from_slice(tag);
        Self {
            name,
            chain_type,
            params: ParamSnapshot::default(),
            mod_state: ModState::default(),
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
