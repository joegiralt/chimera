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

#[derive(Clone)]
pub struct ModDestRegistry {
    pub entries: [Option<ModDestEntry>; MAX_REGISTRY_DESTS],
    pub count: usize,
}

impl ModDestRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_REGISTRY_DESTS],
            count: 0,
        }
    }

    pub fn add(&mut self, path: ParamPath, label: [u8; LABEL_LEN]) {
        // No duplicates
        if self.is_primed(path) {
            return;
        }
        // Find first empty slot
        if self.count >= MAX_REGISTRY_DESTS {
            return;
        }
        self.entries[self.count] = Some(ModDestEntry { path, label });
        self.count += 1;
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
