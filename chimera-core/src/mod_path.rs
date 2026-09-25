//! The mod destination registry: parameters a sound has primed for
//! modulation, by semantic address (spec §2, §4).

use crate::addr::ParamAddr;

/// The registry holds exactly as many destinations as the matrix and
/// `ModState` can route: a primed address past that would report success
/// but never appear (final review I2 of issue #21).
pub const MAX_REGISTRY_DESTS: usize = crate::modulation::MAX_MOD_DESTS;
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
    /// The registry already holds `MAX_REGISTRY_DESTS` destinations.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::BlockRef;

    /// Filling the registry through `add` alone: once it holds
    /// `MAX_REGISTRY_DESTS` (the matrix capacity), the next distinct
    /// modulatable address is refused with `Full` (issue #21).
    #[test]
    fn add_refuses_once_at_matrix_capacity() {
        let mut reg = ModDestRegistry::new();
        let mut refused = None;
        for b in BlockRef::ALL {
            for s in b.specs() {
                let addr = ParamAddr::new(b, s.id);
                if !addr.modulatable() {
                    continue;
                }
                let before = reg.len();
                if let Err(e) = reg.add(addr, [0; LABEL_LEN]) {
                    refused.get_or_insert((before, e));
                }
            }
        }
        assert_eq!(refused, Some((MAX_REGISTRY_DESTS, RegistryError::Full)));
        assert_eq!(reg.len(), MAX_REGISTRY_DESTS);
    }
}
