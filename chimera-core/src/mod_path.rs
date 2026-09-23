use crate::addr::{BlockRef, Op, ParamAddr};
use crate::preset::ChainType;
use crate::ui::page::PageId;

pub const MAX_REGISTRY_DESTS: usize = 32;
pub const LABEL_LEN: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamPath {
    /// Standard chain block param: block node index + encoder index
    Block { block: u8, param: u8 },
    /// FM operator param: operator index (0-3) + param within operator
    FmOp { op: u8, param: u8 },
    /// FM operator envelope param: operator index (0-3) + envelope param
    FmEnv { op: u8, param: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct ModDestEntry {
    pub path: ParamPath,
    pub label: [u8; LABEL_LEN],
}

impl ModDestEntry {
    pub fn label_str(&self) -> &str {
        let end = self.label.iter().position(|&b| b == 0).unwrap_or(LABEL_LEN);
        core::str::from_utf8(&self.label[..end]).unwrap_or("???")
    }
}

/// Why `ModDestRegistry::add` refused a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The address's spec is not modulatable (or the path means nothing).
    NotModulatable,
    Full,
}

#[derive(Clone)]
pub struct ModDestRegistry {
    entries: [Option<ModDestEntry>; MAX_REGISTRY_DESTS],
    count: usize,
}

impl ModDestRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_REGISTRY_DESTS],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Prime `path` (as the UI on `chain` means it) as a mod destination.
    /// Refuses non-modulatable addresses (spec §4). Priming an already
    /// primed path is a no-op success.
    pub fn add(&mut self, chain: ChainType, path: ParamPath, label: [u8; LABEL_LEN]) -> Result<(), RegistryError> {
        if !legacy_to_addr(chain, path).is_some_and(ParamAddr::modulatable) {
            return Err(RegistryError::NotModulatable);
        }
        if self.is_primed(path) {
            return Ok(());
        }
        if self.count >= MAX_REGISTRY_DESTS {
            return Err(RegistryError::Full);
        }
        self.entries[self.count] = Some(ModDestEntry { path, label });
        self.count += 1;
        Ok(())
    }

    pub fn remove(&mut self, path: ParamPath) {
        for i in 0..self.count {
            if let Some(entry) = &self.entries[i] {
                if entry.path == path {
                    // Shift remaining entries down
                    for j in i..self.count - 1 {
                        self.entries[j] = self.entries[j + 1];
                    }
                    self.entries[self.count - 1] = None;
                    self.count -= 1;
                    return;
                }
            }
        }
    }

    pub fn find(&self, path: ParamPath) -> Option<usize> {
        for i in 0..self.count {
            if let Some(entry) = &self.entries[i] {
                if entry.path == path {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn is_primed(&self, path: ParamPath) -> bool {
        self.find(path).is_some()
    }

    pub fn get(&self, index: usize) -> Option<&ModDestEntry> {
        if index < self.count {
            self.entries[index].as_ref()
        } else {
            None
        }
    }
}

/// Temporary bridge (deleted in Task 19): the semantic address a UI
/// `ParamPath` means on `chain`. `Block { block: node, param: slot }` names
/// the node's main page, except each chain's MOD node, whose slots mean the
/// envelope sub-page (plan D8).
pub fn legacy_to_addr(chain: ChainType, path: ParamPath) -> Option<ParamAddr> {
    match path {
        ParamPath::FmOp { op, param } => {
            let op = Op::try_from(op).ok()?;
            let a = PageId::FmOp.binding(param as usize)?;
            Some(ParamAddr::new(BlockRef::FmOp(op), a.param))
        }
        ParamPath::FmEnv { op, param } => {
            let op = Op::try_from(op).ok()?;
            let a = PageId::FmEnv1.binding(param as usize)?;
            Some(ParamAddr::new(BlockRef::FmOp(op), a.param))
        }
        ParamPath::Block { block, param } => {
            let page = match (chain, block) {
                (ChainType::PizzaPoly, 0) => PageId::Pizza,
                (ChainType::Modal, 0) => PageId::EngineModal1,
                (ChainType::Fm, 0) => PageId::FmAlg,
                (ChainType::PizzaPoly | ChainType::Fm, 1) => PageId::Drive,
                (ChainType::PizzaPoly | ChainType::Fm, 2) | (ChainType::Modal, 1) => PageId::Filter,
                (ChainType::PizzaPoly | ChainType::Fm, 3) => PageId::Folder,
                (ChainType::PizzaPoly, 4) | (ChainType::Modal, 2) => PageId::Vca,
                _ => return None,
            };
            page.binding(param as usize)
        }
    }
}
