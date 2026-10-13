//! `factory-sounds qa` (spec § 4): every public Sound and demo project
//! rendered and gated into the showcase ledger; with `--private` the ports
//! into their own ledger (`build::private_qa_ledger`), never the public one.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::qa::{self, baseline, demo, measure, page};
use super::source::Source;
use super::{build, phrase, project};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// `--private`: the port Sources' folder, the ledger their QA goes to,
/// and the check each port's `edits` passes first (`BankEdits`).
pub struct Private<'a> {
    pub ports: &'a Path,
    pub ledger: PathBuf,
    pub edits: &'a dyn build::EditsCheck,
}

/// One `qa` run.
pub struct Qa<'a> {
    /// The public Sources (`tools/factory-sounds/`).
    pub dir: &'a Path,
    /// The public ledger.
    pub ledger: PathBuf,
    pub private: Option<Private<'a>>,
    pub wav: bool,
    /// `--baseline <dir>`: each APPROVED Sound's renders against it.
    pub baseline: Option<&'a Path>,
}

/// Every public Sound and demo rendered and gated into the public ledger,
/// then with `--private` the ports into theirs; fails unless all pass.
pub fn qa(run: &Qa) -> Result<(), String> {
    let (placeholders, all): (Vec<_>, Vec<_>) =
        build::sounds(run.dir, run.private.as_ref().map(|p| p.ports))?
            .into_iter()
            .partition(|s| s.is_placeholder());
    let (ported, public): (Vec<_>, Vec<_>) = all.into_iter().partition(|s| s.from.is_some());
    if let Some(p) = &run.private {
        build::verify(&ported, p.edits)?;
    }
    let (reports, samey) = gated(&public, run.baseline)?;
    let (demos, unheard) = demo_reports(run.dir, &public, &run.ledger)?;
    let mut pass = page::write(
        &run.ledger,
        &reports,
        &samey,
        &placeholders,
        &demos,
        run.wav,
    )?;
    print!("{}", page::report_text(&reports, &samey));
    print!("{}", page::demo_text(&demos));
    println!("{}", run.ledger.display());
    if let Some(p) = &run.private {
        let (reports, samey) = gated(&ported, None)?;
        pass &= page::write_private(&p.ledger, &reports, &samey, run.wav)?;
        print!("{}", page::report_text(&reports, &samey));
        println!("{}", p.ledger.display());
    }
    match (unheard, pass) {
        (Some(gap), _) => Err(gap),
        (None, true) => Ok(()),
        (None, false) => Err("a gate failed: see report.txt".into()),
    }
}

/// Each Sound's gates (with a baseline, each APPROVED Sound's renders
/// compared against it, `baseline::compare`), and the samey pairs among them.
type Gated = (Vec<qa::Report>, Vec<(usize, usize)>);
fn gated(all: &[Source], baseline: Option<&Path>) -> Result<Gated, String> {
    phrase::sweeps(all)?;
    let mut reports = all.iter().map(qa::gates).collect::<Result<Vec<_>, _>>()?;
    if let Some(dir) = baseline {
        if !dir.is_dir() {
            return Err(format!("baseline {}: not a directory", dir.display()));
        }
        for (r, s) in reports.iter_mut().zip(all) {
            baseline::compare(r, dir)?;
            qa::voice_cause(r, s)?;
        }
    }
    let bands: Vec<_> = reports.iter().map(|r| r.bands).collect();
    let samey = measure::samey(&bands);
    Ok((reports, samey))
}

/// Each demo project rendered and gated (`demo::demo_gates`), a held one
/// reported unrendered (`demo::held_report`), plus the effects no demo
/// plays, held or not (`project::coverage`), which fail the run after
/// every gate is heard; one only held demos play is pending. None: none.
type Demos = (Vec<demo::DemoReport>, Option<String>);
fn demo_reports(dir: &Path, sounds: &[Source], ledger: &Path) -> Result<Demos, String> {
    let demos = build::demos(dir)?;
    if demos.is_empty() {
        return Ok((Vec::new(), None));
    }
    project::check(&demos, sounds)?;
    let cov = project::coverage(&demos, sounds)?;
    let unheard = (!cov.unheard.is_empty()).then(|| {
        let words: Vec<String> = cov.unheard.iter().map(|e| e.label()).collect();
        format!(
            "{}: no demo setting plays {} (fx. or hear. lines)",
            project::FILE,
            words.join(", ")
        )
    });
    let stims = if demos.iter().any(|d| d.held.is_none()) {
        Some(stims(&ledger.join("stimuli"))?)
    } else {
        None
    };
    let reports = demos
        .iter()
        .map(|d| match &stims {
            Some(s) if d.held.is_none() => demo::demo_gates(d, sounds, s),
            _ => demo::held_report(d, sounds, &cov),
        })
        .collect::<Result<_, _>>()?;
    Ok((reports, unheard))
}

/// `chimera-utils stim`'s hats and snare from `dir`, written there first
/// when missing.
fn stims(dir: &Path) -> Result<demo::Stims, String> {
    let wav = |n: &str| dir.join(format!("{n}.wav"));
    if !wav("hats").is_file() || !wav("snare").is_file() {
        let ok = Command::new("cargo")
            .args([
                "run",
                "-q",
                "--profile",
                "utils",
                "-p",
                "chimera-utils",
                "--",
                "stim",
            ])
            .arg(dir)
            .current_dir(ROOT)
            .status()
            .map_err(|e| format!("chimera-utils stim: {e}"))?
            .success();
        if !ok {
            return Err("chimera-utils stim failed".into());
        }
    }
    let read = |n: &str| -> Result<Vec<f32>, String> {
        let p = wav(n);
        let err = |e: hound::Error| format!("{}: {e}", p.display());
        let r = hound::WavReader::open(&p).map_err(err)?;
        let spec = r.spec();
        if spec.channels != 1 || spec.sample_rate != 48_000 {
            return Err(format!("{}: not mono at 48 kHz", p.display()));
        }
        let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
        r.into_samples::<i32>()
            .map(|v| v.map(|v| v as f32 / full).map_err(err))
            .collect()
    };
    Ok(demo::Stims {
        hats: read("hats")?,
        snare: read("snare")?,
    })
}
