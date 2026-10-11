//! `just factory-sounds`: the public factory Sounds (spec § 3, ADR 0134)
//! from `tools/factory-sounds/` into `target/factory-sounds/public/`.
//! `factory-sounds fmt` rewrites each Source with its display comments.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use chimera_core::storage::BuildStamp;
use chimera_desktop::factory_sounds::build::{self, SOUND_FILES};
use chimera_desktop::factory_sounds::source;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        None | Some("build") => run_build(),
        Some("fmt") => run_fmt(),
        Some(a) => Err(format!("factory-sounds [build | fmt], not {a}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("factory-sounds: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dir() -> PathBuf {
    PathBuf::from(ROOT).join("tools/factory-sounds")
}

fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(ROOT)
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The version and the commit the Sources were built from; dirty when the
/// Sources differ from it.
fn stamp() -> Result<BuildStamp, String> {
    let hash = git(&["rev-parse", "--short=8", "HEAD"]).ok_or("git rev-parse failed")?;
    let dirty = git(&["status", "--porcelain", "--", "tools/factory-sounds"])
        .is_some_and(|s| !s.is_empty());
    Ok(BuildStamp::new(
        VERSION,
        &hash.chars().take(8).collect::<String>(),
        dirty,
    ))
}

fn run_build() -> Result<(), String> {
    let out = PathBuf::from(ROOT).join("target/factory-sounds/public");
    let man = build::public(&dir(), &out, &stamp()?)?;
    for e in &man.sounds {
        println!(
            "S{}.A  {:<16} {:>5} B  {:08X}",
            e.id.get(),
            e.name,
            e.size,
            e.crc
        );
    }
    println!(
        "{} Sounds, built {}\n{}",
        man.sounds.len(),
        man.stamp,
        out.display()
    );
    Ok(())
}

fn run_fmt() -> Result<(), String> {
    for f in SOUND_FILES {
        let p = dir().join(f);
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        source::parse(&text, f)?;
        let canon = source::fmt(&text);
        if canon != text {
            std::fs::write(&p, &canon).map_err(|e| format!("{}: {e}", p.display()))?;
            println!("{f}");
        }
    }
    Ok(())
}
