//! `report.txt` and the owner's `listening.html` (spec § 4): each Sound's
//! and demo's gates, the page of everything that passed, and the ledger
//! they are written to.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use chimera_core::block::DiskCode;

use super::super::source::{Badge, Round, Source};
use super::baseline::{AUDIO_CHANGED, Baseline, RESIDUAL_NOTE};
use super::demo::DemoReport;
use super::measure::{ERB_BANDS, SAMEY_DB, SILENT_FLOOR_DB, Verdict};
use super::render::UNIT_REV;
use super::{Check, Report};
use crate::listen::{self, Player, Row, Section};
use crate::privacy::PORT_IDS;

/// The ledger folder, refused under `/tmp` (the scratchpad included) or
/// the system temp dir, or when not absolute or holding `..`: renders and
/// verdicts are durable (durable ledgers rule).
pub fn checked_ledger(p: &Path) -> Result<PathBuf, String> {
    let bad = |why: &str| Err(format!("ledger {}: {why}", p.display()));
    if !p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return bad("must be an absolute path without ..");
    }
    let tmp = [PathBuf::from("/tmp"), std::env::temp_dir()];
    if tmp.iter().any(|t| p.starts_with(t)) {
        return bad("never /tmp or the scratchpad");
    }
    Ok(p.to_path_buf())
}

const DEFAULT_LEDGER: &str = "~/dev/chimera/.superpowers/ledgers/showcase";

/// `$CHIMERA_SHOWCASE_LEDGER`, else the showcase ledger, checked.
pub fn ledger() -> Result<PathBuf, String> {
    let p = std::env::var_os("CHIMERA_SHOWCASE_LEDGER")
        .map_or_else(|| PathBuf::from(DEFAULT_LEDGER), PathBuf::from);
    checked_ledger(&p)
}

/// `report.txt`: each Sound's gates, the samey pairs, and the verdict.
pub fn report_text(reports: &[Report], samey: &[(usize, usize)]) -> String {
    let mut o = String::new();
    for r in reports {
        let ok = if r.passes() { "PASS" } else { "FAIL" };
        let private = if r.private { "  PRIVATE" } else { "" };
        let _ = writeln!(o, "S{} {}{private}  {ok}", r.id.get(), r.name);
        let _ = writeln!(o, "  voices on the unit: {} ({UNIT_REV})", r.voices);
        if let Some(round) = r.changed {
            let _ = writeln!(o, "  {}", changed_line(round));
        }
        if let Some(b) = &r.baseline {
            match (b.changed(), b.worst) {
                (Some((a, db)), _) => {
                    let _ = writeln!(o, "  {AUDIO_CHANGED} (worst {db:.1} dB: {})", a.slug());
                    for l in b.lines(&r.free_lfos).iter().skip(1) {
                        let _ = writeln!(o, "  {l}");
                    }
                }
                (None, Some((a, db))) if db.is_finite() => {
                    let _ = writeln!(o, "  baseline: worst residual {db:.1} dB: {}", a.slug());
                }
                (None, Some(_)) => {
                    let _ = writeln!(o, "  baseline: identical");
                }
                (None, None) => {}
            }
            for &a in &b.missing {
                let _ = writeln!(o, "  baseline: no {}, not compared", r.file(a));
            }
        }
        for g in &r.gates {
            let v = match &g.verdict {
                Verdict::Pass => "PASS".to_string(),
                Verdict::Fail => "FAIL".to_string(),
                Verdict::Exempt(why) => format!("EXEMPT ({why})"),
            };
            // A tail at the dB floor is silence, not a number.
            let value = if g.check == Check::Tail && g.value <= SILENT_FLOOR_DB {
                "silent".to_string()
            } else {
                format!("{:.2}", g.value)
            };
            let _ = writeln!(
                o,
                "  {:<18} {value:>8} {:<6} {v}",
                g.check.label(),
                g.check.unit()
            );
        }
    }
    for &(i, j) in samey {
        let _ = writeln!(
            o,
            "samey: {} and {}, within {SAMEY_DB} dB on all {ERB_BANDS} ERB bands: by ear",
            reports[i].name, reports[j].name
        );
    }
    for r in reports {
        if let Some((a, db)) = r.baseline.as_ref().and_then(Baseline::changed) {
            let _ = writeln!(
                o,
                "audio changed by engine update: S{} {}, worst {db:.1} dB: {}",
                r.id.get(),
                r.name,
                a.slug()
            );
        }
    }
    if any_changed(reports) {
        let _ = writeln!(o, "{RESIDUAL_NOTE}");
    }
    let pass = reports.iter().all(Report::passes);
    let _ = writeln!(
        o,
        "{} Sounds: {}",
        reports.len(),
        if pass { "all pass" } else { "FAIL" }
    );
    o
}

/// Some Sound's audio changed since the baseline.
fn any_changed(reports: &[Report]) -> bool {
    reports
        .iter()
        .any(|r| r.baseline.as_ref().and_then(Baseline::changed).is_some())
}

/// An approved Sound whose CRC no longer matches.
fn changed_line(round: Round) -> String {
    format!("CHANGED SINCE {round}: re-listen")
}

/// The owner's page: every Sound that passes, badged APPROVED or NEW,
/// then each placeholder, then each passing demo project under DEMO
/// PROJECTS; a Sound or demo a gate failed is named in a flagged OFF THE
/// PAGE row. Per Sound its name, description, demo mode,
/// LUFS, `edits`, exemptions and samey pairs to judge by ear, what it is
/// (`about`), and a player per render with its LUFS and peak.
pub fn page(
    reports: &[Report],
    samey: &[(usize, usize)],
    placeholders: &[Source],
    demos: &[DemoReport],
) -> String {
    let mut sounds: Vec<((bool, Badge), Row)> = reports
        .iter()
        .enumerate()
        .filter(|(_, r)| r.passes())
        .map(|(i, r)| {
            let mut lines = vec![(r.note.clone(), false)];
            if let Some(round) = r.changed {
                lines.push((changed_line(round), true));
            }
            if let Some(b) = &r.baseline {
                lines.extend(b.lines(&r.free_lfos).into_iter().map(|l| (l, true)));
            }
            let lufs = r.lufs().map_or(String::new(), |l| {
                format!(" · level (1 s, dry) {l:.1} LUFS")
            });
            let long = r
                .long_lufs
                .map_or(String::new(), |l| format!(" · long (dry) {l:.1} LUFS"));
            lines.push((
                format!(
                    "S{} · {} · {} VOICES{lufs}{long}",
                    r.id.get(),
                    r.demo.disk_ident(),
                    r.voices
                ),
                false,
            ));
            if let Some(e) = &r.edits {
                lines.push((format!("edits: {e}"), false));
            }
            for g in &r.gates {
                if let Verdict::Exempt(why) = &g.verdict {
                    lines.push((format!("{}: {why}", g.check.label()), true));
                }
            }
            for &(a, b) in samey.iter().filter(|p| p.0 == i || p.1 == i) {
                let other = &reports[if a == i { b } else { a }].name;
                lines.push((format!("samey with {other}: by ear"), true));
            }
            let row = Row {
                title: r.name.clone(),
                badge: Some(r.badge.word()),
                lines,
                facts: r.about.clone(),
                players: r
                    .renders
                    .iter()
                    .map(|x| {
                        let bar = x.audition.bar();
                        let over = x.limited_db.is_some_and(|d| d > bar);
                        let limited = x.limited_db.map_or(String::new(), |d| {
                            let past = if over {
                                format!(", over the {bar} dB bar")
                            } else {
                                String::new()
                            };
                            format!(" · limiter {d:.1} dB{past}")
                        });
                        Player {
                            label: std::iter::once(x.audition.label())
                                .chain(x.with.iter().map(|w| format!("WITH {w}")))
                                .collect::<Vec<_>>()
                                .join(" "),
                            file: r.wav(x.audition),
                            info: format!(
                                "{:.1} LUFS · peak L {:.1} · R {:.1} dBFS{limited}",
                                x.lufs, x.peak_dbfs[0], x.peak_dbfs[1]
                            ),
                            flag: over,
                        }
                    })
                    .collect(),
                section: if r.private {
                    Section::Private
                } else {
                    Section::Sounds
                },
            };
            let coherent = r.baseline.as_ref().is_some_and(Baseline::coherent);
            ((!coherent, r.badge), row)
        })
        .collect();
    sounds.extend(placeholders.iter().map(|s| {
        let row = Row {
            title: s.name.as_str().into(),
            badge: Some(Badge::Placeholder.word()),
            lines: vec![
                (s.note.as_str().into(), false),
                (
                    format!("S{}: waits on the formant engine", s.id.get()),
                    false,
                ),
            ],
            facts: Vec::new(),
            players: Vec::new(),
            section: Section::Sounds,
        };
        ((true, Badge::Placeholder), row)
    }));
    // A coherent engine change first (heard first), then APPROVED, NEW and
    // the placeholders; Source order within each.
    sounds.sort_by_key(|(b, _)| *b);
    let mut rows: Vec<Row> = sounds.into_iter().map(|(_, r)| r).collect();
    rows.extend(demos.iter().filter(|d| d.passes()).map(demo_row));
    let off_line = |what: String| {
        (
            format!("Off the page (a gate failed, see report.txt): {what}"),
            true,
        )
    };
    let off: Vec<(String, bool)> = reports
        .iter()
        .filter(|r| !r.passes())
        .map(|r| off_line(format!("S{} {}", r.id.get(), r.name)))
        .chain(
            demos
                .iter()
                .filter(|d| !d.passes())
                .map(|d| off_line(format!("{} {}", d.id.stem(), d.name))),
        )
        .collect();
    if !off.is_empty() {
        rows.insert(
            0,
            Row {
                title: "OFF THE PAGE".into(),
                badge: None,
                lines: off,
                facts: Vec::new(),
                players: Vec::new(),
                section: Section::Sounds,
            },
        );
    }
    let intro = format!(
        "Every Sound that is ready: APPROVED (passed your listening round), NEW (not yet heard), \
         and the PLACEHOLDERS still to come. Each plays over a phrase (BASS, LEAD, PAD, ARP or \
         WONKY, four bars; SWEEP, twelve, three held chords each swelling the filter open and \
         closed; on its demo mode), then under its render scripts, offline through the real \
         Instrument (Part 1, P1); a render marked WITH plays its sends through NEW's effects. The \
         phrase is for your ear, not a gate. Under each Sound, what it is, read from the Sound \
         itself. Every gate passed (measured dry) or is marked for your ear: the lines in colour. \
         Every render plays the voices the unit gives the Sound, stealing past them; VOICES is \
         that count on {UNIT_REV}.{}{}",
        if demos.is_empty() {
            ""
        } else {
            " Under DEMO PROJECTS, each effects demo: every Part's STAB and SHORT on its \
             channel, then the HATS and SNARE stimuli, through the project's own effects and \
             each setting its \"what to try\" invites, offline through the real Instrument and \
             mixer."
        },
        if any_changed(reports) {
            format!(" {RESIDUAL_NOTE}")
        } else {
            String::new()
        }
    );
    listen::rows_page("Factory Sounds · listening", &intro, &rows)
}

/// Into `dir` (checked as a ledger): `report.txt`, naming each Sound or
/// demo a failing gate keeps off the page; with `wav`, each passing
/// Sound's renders as `renders/S<id>-<script>.wav` and each passing
/// demo's as `renders/P<id>-<setting>-<take>.wav` (a failing one's old
/// renders removed), and `listening.html`. A failing run without `wav`
/// removes an old page. True when every Sound and demo passes. The
/// public ledger: it refuses a port, and with `wav` removes any render of
/// a `PORT_IDS` id (it is published).
pub fn write(
    dir: &Path,
    reports: &[Report],
    samey: &[(usize, usize)],
    placeholders: &[Source],
    demos: &[DemoReport],
    wav: bool,
) -> Result<bool, String> {
    if let Some(r) = reports.iter().find(|r| r.private) {
        return Err(format!(
            "S{} {}: a private port; the public ledger refuses it (qa --private)",
            r.id.get(),
            r.name
        ));
    }
    if wav {
        // A backstop: no port's render stays in the published ledger.
        let r = checked_ledger(dir)?.join("renders");
        match std::fs::read_dir(&r) {
            Ok(entries) => {
                for e in entries {
                    let p = e.map_err(|e| format!("{}: {e}", r.display()))?.path();
                    let port = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .filter(|n| n.ends_with(".wav"))
                        .and_then(|n| n.strip_prefix('S')?.get(..7)?.parse::<u32>().ok())
                        .is_some_and(|id| PORT_IDS.contains(&id));
                    if port {
                        std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", r.display())),
        }
    }
    write_ledger(dir, reports, samey, placeholders, demos, wav)
}

/// The ports' own ledger (`qa --private`): ports only, no placeholders or
/// demos; else as `write`.
pub fn write_private(
    dir: &Path,
    reports: &[Report],
    samey: &[(usize, usize)],
    wav: bool,
) -> Result<bool, String> {
    if let Some(r) = reports.iter().find(|r| !r.private) {
        return Err(format!(
            "S{} {}: not a port; the private ledger holds ports only",
            r.id.get(),
            r.name
        ));
    }
    write_ledger(dir, reports, samey, &[], &[], wav)
}

fn write_ledger(
    dir: &Path,
    reports: &[Report],
    samey: &[(usize, usize)],
    placeholders: &[Source],
    demos: &[DemoReport],
    wav: bool,
) -> Result<bool, String> {
    let dir = checked_ledger(dir)?;
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
    let pass = reports.iter().all(Report::passes) && demos.iter().all(DemoReport::passes);
    let mut txt = report_text(reports, samey);
    txt.push_str(&demo_text(demos));
    for r in reports.iter().filter(|r| !r.passes()) {
        let _ = writeln!(
            txt,
            "off the page: S{} {} (a gate failed)",
            r.id.get(),
            r.name
        );
    }
    for d in demos.iter().filter(|d| !d.passes()) {
        let _ = writeln!(
            txt,
            "off the page: {} {} (a gate failed)",
            d.id.stem(),
            d.name
        );
    }
    let path = dir.join("report.txt");
    std::fs::write(&path, txt).map_err(|e| err(&path, e))?;
    let html = dir.join("listening.html");
    if wav {
        let r = dir.join("renders");
        std::fs::create_dir_all(&r).map_err(|e| err(&r, e))?;
        // A Sound or demo off the page leaves no old render to pass as
        // current.
        let unrender = |stale: String| -> Result<(), String> {
            for e in std::fs::read_dir(&r).map_err(|e| err(&r, e))? {
                let p = e.map_err(|e| err(&r, e))?.path();
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&stale) && n.ends_with(".wav"))
                {
                    std::fs::remove_file(&p).map_err(|e| err(&p, e))?;
                }
            }
            Ok(())
        };
        for rep in reports {
            if rep.passes() {
                for x in &rep.renders {
                    listen::write_wav_stereo(&dir.join(rep.wav(x.audition)), &x.stereo)?;
                }
            } else {
                unrender(format!("S{}-", rep.id.get()))?;
            }
        }
        for d in demos {
            if d.passes() && d.held.is_none() {
                for x in &d.renders {
                    listen::write_wav_stereo(&r.join(d.file(x)), &x.stereo)?;
                }
            } else {
                unrender(format!("{}-", d.id.stem()))?;
            }
        }
        std::fs::write(&html, page(reports, samey, placeholders, demos))
            .map_err(|e| err(&html, e))?;
    } else if !pass {
        // An old page could show a now failing Sound as passed.
        match std::fs::remove_file(&html) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(&html, e)),
            _ => {}
        }
    }
    Ok(pass)
}

/// Every held demo's pending lines, once each, in order.
fn pending_lines(demos: &[DemoReport]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in demos
        .iter()
        .filter_map(|d| d.held.as_ref())
        .flat_map(|h| &h.pending)
    {
        if !out.contains(l) {
            out.push(l.clone());
        }
    }
    out
}

/// `report.txt`'s demo lines: each project's gates by setting.
pub fn demo_text(demos: &[DemoReport]) -> String {
    let mut o = String::new();
    for d in demos {
        let ok = match (&d.held, d.passes()) {
            (Some(h), _) => format!("HELD ({})", h.reason),
            (None, true) => "PASS".into(),
            (None, false) => "FAIL".into(),
        };
        let _ = writeln!(o, "{} {}  {ok}", d.id.stem(), d.name);
        for (i, s) in d.settings.iter().enumerate() {
            let _ = writeln!(o, "  setting {}: {s}", i + 1);
            for g in d.gates.iter().filter(|g| g.setting == i) {
                let v = match &g.verdict {
                    Verdict::Pass => "PASS".to_string(),
                    Verdict::Fail => "FAIL".to_string(),
                    Verdict::Exempt(why) => format!("EXEMPT ({why})"),
                };
                let _ = writeln!(
                    o,
                    "    {:<22} {:>8.2} {:<6} {v}",
                    g.check.label(),
                    g.value,
                    g.check.unit()
                );
            }
        }
    }
    for l in pending_lines(demos) {
        let _ = writeln!(o, "{l}");
    }
    if !demos.is_empty() {
        let pass = demos.iter().all(DemoReport::passes);
        let held = demos.iter().filter(|d| d.held.is_some()).count();
        let _ = writeln!(
            o,
            "{} demo projects: {}{}",
            demos.len(),
            if pass { "all pass" } else { "FAIL" },
            if held > 0 {
                format!(" ({held} held)")
            } else {
                String::new()
            }
        );
    }
    o
}

/// A passing demo's row in the DEMO PROJECTS section; a held one badged
/// HELD with its reason and what waits on it.
fn demo_row(d: &DemoReport) -> Row {
    let mut lines = vec![(format!("What to try: {}", d.tries), false)];
    if let Some(h) = &d.held {
        lines.push((format!("Held: {}", h.reason), true));
        lines.extend(h.pending.iter().map(|l| (l.clone(), true)));
    }
    lines.push((d.id.stem(), false));
    lines.extend(d.parts.iter().map(|p| (p.clone(), false)));
    for g in &d.gates {
        if let Verdict::Exempt(why) = &g.verdict {
            lines.push((format!("{}: {why}", g.check.label()), true));
        }
    }
    Row {
        title: d.name.clone(),
        badge: d.held.as_ref().map(|_| "HELD"),
        lines,
        facts: Vec::new(),
        players: d
            .renders
            .iter()
            .map(|r| Player {
                label: format!("{} · {}", r.take.label(), d.settings[r.setting]),
                file: format!("renders/{}", d.file(r)),
                info: format!(
                    "{:.1} LUFS · peak L {:.1} · R {:.1} dBFS · limiter {:.1} dB",
                    r.lufs, r.peak_dbfs[0], r.peak_dbfs[1], r.limited_db
                ),
                flag: false,
            })
            .collect(),
        section: Section::Demos,
    }
}
