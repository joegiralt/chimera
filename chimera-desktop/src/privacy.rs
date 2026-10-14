//! Privacy guard: which tracked files must never exist (raw TX81Z bank data; ADR 0138).

use std::ops::RangeInclusive;

/// Sound ids for TX81Z ports still in private staging. The first 16
/// (S9100010-25) shipped public (ADR 0138); new imports start here.
pub const PORT_IDS: RangeInclusive<u32> = 9_100_070..=9_100_099;

const BANK_EXTS: [&str; 4] = ["syx", "bnk", "mix", "wav"];
const PRIVATE_DIR: &str = "tools/factory-sounds/private/";
const SEEDS_DIR: &str = "chimera-desktop/fuzz/seeds/";

/// `(path, reason)` for every file that must not be tracked.
pub fn offences(files: &[(&str, &[u8])]) -> Vec<String> {
    files
        .iter()
        .filter_map(|&(path, _)| reason(path).map(|r| format!("{path}: {r}")))
        .collect()
}

fn reason(path: &str) -> Option<&'static str> {
    let ext = path
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    if path.starts_with(PRIVATE_DIR) {
        Some("private port directory")
    } else if BANK_EXTS.contains(&ext.as_str()) && !(ext == "wav" && path.starts_with(SEEDS_DIR)) {
        Some("bank or audio file")
    } else {
        None
    }
}
