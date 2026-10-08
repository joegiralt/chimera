//! Stamps the build with the git hash and a dirty flag (#406). Only the
//! `stamp` feature (the firmware) reads git; every other build carries the
//! fixed "dev" so tests and goldens do not churn per commit.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    let (hash, dirty) = if std::env::var_os("CARGO_FEATURE_STAMP").is_some() {
        // Edits to tracked files, and the index, move the stamp.
        println!("cargo:rerun-if-changed=../.git/index");
        for d in [
            "chimera-core",
            "chimera-engine",
            "chimera-board",
            "chimera-stm32",
        ] {
            println!("cargo:rerun-if-changed=../{d}/src");
        }
        let hash = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".into());
        let dirty =
            git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        (hash, dirty)
    } else {
        ("dev".to_string(), false)
    };
    println!("cargo:rustc-env=CHIMERA_HASH={hash}");
    println!("cargo:rustc-env=CHIMERA_DIRTY={}", dirty as u8);
}
