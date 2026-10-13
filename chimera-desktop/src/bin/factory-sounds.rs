//! `just factory-sounds`: the public factory Sounds (spec § 3, ADR 0134)
//! from `tools/factory-sounds/` into `target/factory-sounds/public/`.
//! `factory-sounds fmt [--level]` rewrites each Source with its display
//! comments (`--level`: each `trim` centred on −11 LUFS). `factory-sounds
//! qa [--wav] [--baseline <dir>]` renders and gates every Sound (spec § 4)
//! into the ledger: `report.txt`; with `--wav` the renders and
//! `listening.html` (every passing Sound, badged, and the placeholders);
//! with `--baseline` each APPROVED Sound's renders against the WAVs in
//! `<dir>`, flagged AUDIO CHANGED over −40 dB.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use chimera_core::storage::BuildStamp;
use chimera_desktop::factory_sounds::build::{self, SOUND_FILES};
use chimera_desktop::factory_sounds::{qa, source};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        None | Some("build") => run_build(),
        Some("fmt") => match args.get(1).map(String::as_str) {
            None => run_fmt(false),
            Some("--level") => run_fmt(true),
            Some(a) => Err(format!("fmt [--level], not {a}")),
        },
        Some("qa") => qa_args(&args[1..]).and_then(|(wav, base)| run_qa(wav, base)),
        Some(a) => Err(format!(
            "factory-sounds [build | fmt [--level] | qa [--wav] [--baseline <dir>]], not {a}"
        )),
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
    let all = build::sources(&dir())?;
    for e in &man.sounds {
        // What an `approved` line names for it now.
        let heard = all
            .iter()
            .find(|s| s.id == e.id)
            .map(source::approval_crc)
            .transpose()?
            .unwrap_or_default();
        println!(
            "S{}.A  {:<16} {:>5} B  {:08X}  sound {:08X}  approval {heard:08X}",
            e.id.get(),
            e.name,
            e.size,
            e.crc,
            e.sound
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

fn run_fmt(level: bool) -> Result<(), String> {
    for f in SOUND_FILES {
        let p = dir().join(f);
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let all = source::parse(&text, f)?;
        let levelled = if level {
            qa::levelled(&text, &all)?
        } else {
            text.clone()
        };
        // Against the file as read: a new trim alone is a change too.
        let canon = source::fmt(&levelled);
        if canon != text {
            std::fs::write(&p, &canon).map_err(|e| format!("{}: {e}", p.display()))?;
            println!("{f}");
        }
    }
    Ok(())
}

/// `qa`'s flags: `--wav`, and `--baseline <dir>`.
fn qa_args(args: &[String]) -> Result<(bool, Option<PathBuf>), String> {
    let (mut wav, mut base) = (false, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--wav" => wav = true,
            "--baseline" => base = Some(PathBuf::from(it.next().ok_or("--baseline <dir>")?)),
            a => return Err(format!("qa [--wav] [--baseline <dir>], not {a}")),
        }
    }
    Ok((wav, base))
}

/// Every Sound rendered and gated into the ledger; fails unless all pass.
/// With a baseline, each APPROVED Sound's renders are compared against it
/// first (`qa::compare`).
fn run_qa(wav: bool, baseline: Option<PathBuf>) -> Result<(), String> {
    let (placeholders, all): (Vec<_>, Vec<_>) = build::sources(&dir())?
        .into_iter()
        .partition(|s| s.is_placeholder());
    source::check(&all)?;
    qa::sweeps(&all)?;
    let mut reports = all.iter().map(qa::gates).collect::<Result<Vec<_>, _>>()?;
    if let Some(dir) = &baseline {
        if !dir.is_dir() {
            return Err(format!("baseline {}: not a directory", dir.display()));
        }
        for r in &mut reports {
            qa::compare(r, dir)?;
        }
    }
    let bands: Vec<_> = reports.iter().map(|r| r.bands).collect();
    let samey = qa::samey(&bands);
    let ledger = qa::ledger()?;
    let pass = qa::write(&ledger, &reports, &samey, &placeholders, wav)?;
    print!("{}", qa::report_text(&reports, &samey));
    println!("{}", ledger.display());
    if pass {
        Ok(())
    } else {
        Err("a gate failed: see report.txt".into())
    }
}
