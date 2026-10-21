//! ngspice, the labs' transistor-level reference: an external tool, never
//! shipped, never linked (ADR 0196). `NGSPICE` names the binary (default
//! `ngspice` on `PATH`). Tests that need it are `#[ignore]`d.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// A PWL source file ngspice's `filesource` reads: `t v` a line.
pub fn write_series(path: &Path, v: &[f64], dt: f64) -> Result<(), String> {
    let mut s = String::with_capacity(v.len() * 28);
    for (i, x) in v.iter().enumerate() {
        let _ = writeln!(s, "{:.9e} {:.9e}", i as f64 * dt, x);
    }
    std::fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
}

/// FNV-1a over the inputs' bits: the cache key for one run.
pub fn stamp<'a>(inputs: impl IntoIterator<Item = &'a f64>) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for v in inputs {
        for b in v.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// Runs `net` in `dir` (as `name`) unless the same netlist already ran on
/// inputs with the same `stamp`; returns the second column of `out.txt`.
pub fn run(dir: &Path, name: &str, net: &str, stamp: &str) -> Result<Vec<f64>, String> {
    let cir = dir.join(name);
    let fresh = std::fs::read_to_string(&cir).is_ok_and(|n| n == net)
        && std::fs::read_to_string(dir.join("done")).is_ok_and(|d| d == stamp);
    if !fresh {
        let _ = std::fs::remove_file(dir.join("done"));
        std::fs::write(&cir, net).map_err(|e| e.to_string())?;
        let ng = std::env::var("NGSPICE").unwrap_or_else(|_| "ngspice".into());
        let out = Command::new(ng)
            .arg("-b")
            .arg(&cir)
            .output()
            .map_err(|e| format!("ngspice: {e}"))?;
        if !out.status.success() {
            return Err(format!("ngspice: {}", String::from_utf8_lossy(&out.stderr)));
        }
        std::fs::write(dir.join("done"), stamp).map_err(|e| e.to_string())?;
    }
    let text =
        std::fs::read_to_string(dir.join("out.txt")).map_err(|e| format!("ngspice output: {e}"))?;
    Ok(text
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1)?.parse().ok())
        .collect())
}
