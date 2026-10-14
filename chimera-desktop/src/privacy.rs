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
    } else if ext == "toml" && values(bytes, "from").any(|v| v.starts_with("tx81z")) {
        Some("TX81Z port Source")
    } else if ext == "toml" && values(bytes, "id").any(|v| port_id(&v)) {
        Some("a port's id (PORT_IDS)")
    } else {
        None
    }
}

/// `s91000nn…`, lowercased, in `PORT_IDS`.
fn port_id(v: &str) -> bool {
    v.strip_prefix('s')
        .and_then(|d| d.get(..7)?.parse::<u32>().ok())
        .is_some_and(|id| PORT_IDS.contains(&id))
}

/// Each `key = "value"` line's value from its opening quote (`"` or
/// `'`), lowercased: any case, any spacing.
fn values(bytes: &[u8], key: &str) -> impl Iterator<Item = String> {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    text.lines()
        .filter_map(|l| {
            let v = l
                .trim_start()
                .strip_prefix(key)?
                .trim_start()
                .strip_prefix('=')?
                .trim_start();
            Some(v.strip_prefix(['"', '\''])?.to_string())
        })
        .collect::<Vec<_>>()
        .into_iter()
}
