// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `just factory-sounds`: the public factory Sounds (spec § 3, ADR 0134)
//! and the demo projects (spec § 5) from `tools/factory-sounds/` into
//! `target/factory-sounds/public/`.
//! `factory-sounds fmt [--level]` rewrites each Source with its display
//! comments (`--level`: each `trim` centred on −11 LUFS). `factory-sounds
//! qa [--wav] [--baseline <dir>]` renders and gates every Sound (spec § 4)
//! and every demo project in `projects.toml` (spec § 5: each setting's
//! Parts, then `chimera-utils stim`'s hats and snare, through its FX) into
//! the ledger: `report.txt`; with `--wav` the renders and `listening.html`
//! (every passing Sound, badged, the placeholders and the demos);
//! with `--baseline` each APPROVED Sound's renders against the WAVs in
//! `<dir>`, flagged AUDIO CHANGED over −40 dB.
//! `--private` (any command) also reads the TX81Z port Sources in
//! `$CHIMERA_TX_PORTS` (spec § 3, § 6; never in the repo): the build puts
//! them in `target/factory-sounds/private/` only, `fmt` rewrites them
//! there, and `qa` pages them under PRIVATE in their own ledger
//! (`build::private_qa_ledger`), never the public one. Each command first
//! verifies every port's `edits` against its bank in `$CHIMERA_TX_BANKS`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use chimera_core::storage::BuildStamp;
use chimera_desktop::factory_sounds::build::{self, SOUND_FILES};
use chimera_desktop::factory_sounds::{qa, run, source};
use chimera_desktop::import::tx81z::BankEdits;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "factory-sounds [build] [--private] | fmt [--level] [--private] | qa [--wav] [--private] [--baseline <dir>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (private, args): (Vec<_>, Vec<_>) = args.into_iter().partition(|a| a == "--private");
    let ports = (!private.is_empty()).then(ports);
    let banks = BankEdits::new(build::banks_dir(std::env::var_os(build::BANKS_VAR)));
    let r = match args.first().map(String::as_str) {
        None | Some("build") if args.len() <= 1 => run_build(ports.as_deref(), &banks),
        Some("fmt") => match args.get(1..).unwrap_or_default() {
            [] => run_fmt(false, ports.as_deref(), &banks),
            [l] if l == "--level" => run_fmt(true, ports.as_deref(), &banks),
            _ => Err(format!("{USAGE}, not {}", args.join(" "))),
        },
        Some("qa") => qa_args(&args[1..]).and_then(|(wav, base)| {
            let private = ports.as_deref().map(|p| run::Private {
                ports: p,
                ledger: build::private_qa_ledger(),
                edits: &banks,
            });
            run::qa(&run::Qa {
                dir: &dir(),
                ledger: qa::page::ledger()?,
                private,
                wav,
                baseline: base.as_deref(),
            })
        }),
        _ => Err(format!("{USAGE}, not {}", args.join(" "))),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("factory-sounds: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `$CHIMERA_TX_PORTS`, else the showcase ledger's `ports/`.
fn ports() -> PathBuf {
    build::ports_dir(std::env::var_os(build::PORTS_VAR))
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

fn run_build(ports: Option<&Path>, banks: &BankEdits) -> Result<(), String> {
    let root = PathBuf::from(ROOT).join("target/factory-sounds");
    if let Some(p) = ports {
        verified(p, banks)?;
    }
    let out = root.join("public");
    let man = build::public(&dir(), &out, &stamp()?)?;
    let all = build::sources(&dir())?;
    list(&man, &all)?;
    println!(
        "{} Sounds, {} demo projects, built {}\n{}",
        man.sounds.len(),
        man.projects.len(),
        man.stamp,
        out.display()
    );
    if let Some(p) = ports {
        let man = build::private(&dir(), p, &root, &stamp()?, Some(banks))?;
        list(&man, &build::ports(p)?)?;
        println!(
            "{} private ports from {}, edits verified against {}, built {}\n{}",
            man.sounds.len(),
            p.display(),
            banks.dir.display(),
            man.stamp,
            root.join("private").display()
        );
    }
    Ok(())
}

/// The ports read, checked and their edits verified against the banks
/// (`$CHIMERA_TX_BANKS`), before a run writes anything.
fn verified(ports: &Path, banks: &BankEdits) -> Result<(), String> {
    let all = build::sounds(&dir(), Some(ports))?;
    let ported: Vec<_> = all.into_iter().filter(|s| s.from.is_some()).collect();
    build::verify(&ported, banks)
}

/// A line per built Sound and demo project.
fn list(man: &build::Manifest, all: &[source::Source]) -> Result<(), String> {
    for e in &man.sounds {
        // What an `approved` line names for it now.
        let heard = all
            .iter()
            .find(|s| s.id == e.id)
            .map(source::approval_crc)
            .transpose()?
            .unwrap_or_default();
        println!(
            "{}.A  {:<16} {:>5} B  {:08X}  sound {:08X}  approval {heard:08X}",
            e.id, e.name, e.size, e.crc, e.sound
        );
    }
    for e in &man.projects {
        println!("{}.A  {:<16}  {:08X}", e.id.stem(), e.name, e.crc);
    }
    Ok(())
}

/// Each public Source file, then with `--private` each port's, in its
/// canonical text.
fn run_fmt(level: bool, ports: Option<&Path>, banks: &BankEdits) -> Result<(), String> {
    let mut files: Vec<PathBuf> = SOUND_FILES.iter().map(|f| dir().join(f)).collect();
    if let Some(p) = ports {
        verified(p, banks)?;
        files.extend(build::port_files(p)?);
    }
    for p in files {
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => return Err(format!("{}: {e}", p.display())),
        };
        let f = p
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let all = source::parse(&text, &f)?;
        let levelled = if level {
            qa::levelled(&text, &all)?
        } else {
            text.clone()
        };
        // Against the file as read: a new trim alone is a change too.
        let canon = source::fmt(&levelled, &f)?;
        if canon != text {
            std::fs::write(&p, &canon).map_err(|e| format!("{}: {e}", p.display()))?;
            println!("{}", p.display());
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
            a => return Err(format!("{USAGE}, not {a}")),
        }
    }
    Ok((wav, base))
}
