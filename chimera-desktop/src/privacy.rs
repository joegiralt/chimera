//! Privacy guard: which tracked files must never exist (TX81Z banks, port Sources).

use std::ops::RangeInclusive;

/// Sound ids reserved for the private TX81Z ports.
pub const PORT_IDS: RangeInclusive<u32> = 9_100_010..=9_100_025;

const BANK_EXTS: [&str; 4] = ["syx", "bnk", "mix", "wav"];
const PRIVATE_DIR: &str = "tools/factory-sounds/private/";
const SEEDS_DIR: &str = "chimera-desktop/fuzz/seeds/";

/// `(path, reason)` for every file that must not be tracked.
pub fn offences(files: &[(&str, &[u8])]) -> Vec<String> {
    files
        .iter()
        .filter_map(|&(path, bytes)| reason(path, bytes).map(|r| format!("{path}: {r}")))
        .collect()
}

fn reason(path: &str, bytes: &[u8]) -> Option<&'static str> {
    let ext = path
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    if path.starts_with(PRIVATE_DIR) {
        Some("private port directory")
    } else if BANK_EXTS.contains(&ext.as_str()) && !(ext == "wav" && path.starts_with(SEEDS_DIR)) {
        Some("bank or audio file")
    } else if ext == "toml" && has_port_from(bytes) {
        Some("TX81Z port Source")
    } else {
        None
    }
}

/// A `from = "tx81z` line, any case, any spacing.
fn has_port_from(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).lines().any(|l| {
        let l = l.trim_start().to_ascii_lowercase();
        l.strip_prefix("from")
            .and_then(|r| r.trim_start().strip_prefix('='))
            .is_some_and(|r| r.trim_start().starts_with("\"tx81z"))
    })
}
