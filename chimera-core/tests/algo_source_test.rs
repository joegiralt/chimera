//! Spec § Principles: `f32` only in the render path. The chip has no
//! double-precision FPU, so `f64` becomes software float (#26), and libm's
//! `f32` trig calls soft-double internally.
//!
//! Best-effort beyond the literal `f64`/`libm` substrings: an unsuffixed
//! literal with nothing on its line to pin it to `f32` defaults to `f64`
//! silently, and a function this module calls into outside `dsp/algo` could
//! reach `f64`/`libm` on our behalf without either word appearing here.

use std::path::{Path, PathBuf};

fn scan_for_bad_words(src: &str, path: &Path) {
    for bad in ["f64", "libm"] {
        assert!(!src.contains(bad), "{} mentions {bad}", path.display());
    }
}

/// A `let` statement whose whole right side is a bare numeric literal, e.g.
/// `let k = 0.5;`: nothing on the line pins it to `f32`, so unless later
/// unified with an `f32` it silently defaults to `f64`.
fn scan_for_unsuffixed_literals(src: &str, path: &Path) {
    for (n, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if !line.starts_with("let ") || !line.ends_with(';') {
            continue;
        }
        let Some((_, rhs)) = line.trim_end_matches(';').rsplit_once('=') else {
            continue;
        };
        let rhs = rhs.trim().strip_prefix('-').unwrap_or(rhs.trim());
        let looks_like_a_bare_float = rhs.contains('.')
            && !rhs.is_empty()
            && rhs
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '_');
        assert!(
            !looks_like_a_bare_float,
            "{}:{}: `{line}` has nothing to pin it to f32",
            path.display(),
            n + 1
        );
    }
}

/// Files this source reaches for a plain function (snake_case import), by
/// `use crate::...` line, outside `dsp/algo` itself. Type and trait imports
/// (PascalCase) are skipped: their impls live where they're defined, in
/// `dsp/algo`, and are scanned directly.
fn external_helpers(src: &str) -> Vec<PathBuf> {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let mut out = Vec::new();
    for line in src.lines() {
        let Some(rest) = line.trim().strip_prefix("use crate::") else {
            continue;
        };
        let rest = rest.trim_end_matches(';');
        let Some((path, names)) = rest.rsplit_once("::") else {
            continue; // `crate::Name`: a root re-export, not a module file
        };
        if path.starts_with("dsp::algo") {
            continue; // scanned directly, in the loop below
        }
        let names = names.trim_matches(['{', '}']);
        let has_fn_import = names
            .split(',')
            .map(str::trim)
            .any(|n| n.starts_with(|c: char| c.is_ascii_lowercase()));
        if !has_fn_import {
            continue;
        }
        let rel = path.replace("::", "/");
        for candidate in [
            root.join(format!("{rel}.rs")),
            root.join(&rel).join("mod.rs"),
        ] {
            if candidate.exists() && !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

#[test]
fn the_algo_module_uses_no_f64_and_no_libm() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/dsp/algo");
    let mut files = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        scan_for_bad_words(&src, &path);
        scan_for_unsuffixed_literals(&src, &path);
        for helper in external_helpers(&src) {
            let helper_src = std::fs::read_to_string(&helper).unwrap();
            scan_for_bad_words(&helper_src, &helper);
        }
        files += 1;
    }
    assert!(files >= 4, "{files} files scanned");
}
