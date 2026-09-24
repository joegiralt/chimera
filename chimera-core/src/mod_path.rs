//! The mod destination registry: parameters a sound has primed for
//! modulation, by semantic address (spec §2, §4).

use crate::addr::ParamAddr;

pub const MAX_REGISTRY_DESTS: usize = 32;
pub const LABEL_LEN: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct ModDestEntry {
    pub addr: ParamAddr,
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
    /// The address's spec is not modulatable.
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

    /// Prime `addr` as a mod destination. Refuses non-modulatable addresses
    /// (spec §4). Priming an already primed address is a no-op success.
    pub fn add(&mut self, addr: ParamAddr, label: [u8; LABEL_LEN]) -> Result<(), RegistryError> {
        if !addr.modulatable() {
            return Err(RegistryError::NotModulatable);
        }
        if self.is_primed(addr) {
            return Ok(());
        }
        if self.count >= MAX_REGISTRY_DESTS {
            return Err(RegistryError::Full);
        }
        self.entries[self.count] = Some(ModDestEntry { addr, label });
        self.count += 1;
        Ok(())
    }

    pub fn remove(&mut self, addr: ParamAddr) {
        if let Some(i) = self.find(addr) {
            // Shift remaining entries down
            for j in i..self.count - 1 {
                self.entries[j] = self.entries[j + 1];
            }
            self.entries[self.count - 1] = None;
            self.count -= 1;
        }
    }

    pub fn find(&self, addr: ParamAddr) -> Option<usize> {
        (0..self.count).find(|&i| self.entries[i].is_some_and(|e| e.addr == addr))
    }

    pub fn is_primed(&self, addr: ParamAddr) -> bool {
        self.find(addr).is_some()
    }

    pub fn get(&self, index: usize) -> Option<&ModDestEntry> {
        if index < self.count {
            self.entries[index].as_ref()
        } else {
            None
        }
    }
}

impl Default for ModDestRegistry {
    fn default() -> Self {
        Self::new()
    }
}
