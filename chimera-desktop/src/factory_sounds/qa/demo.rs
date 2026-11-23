// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The demo projects' renders (spec § 4, § 5): every setting's Parts on
//! their channels, then the listening stimuli, through the project's own
//! effects, gated.

use chimera_core::block::DiskCode;
use chimera_core::dsp::SendLevel;
use chimera_core::dsp::fx_bus::FX_SENDS;
use chimera_core::project::{PartId, Performance};
use chimera_hal::BLOCK_SIZE;

use super::super::phrase::plan;
use super::super::project::{self, Coverage, Demo, DemoId, Setting};
use super::super::source::{self, Script, Source, to_sound};
use super::measure::{LIMIT_CHORD_DB, Verdict, clicks, finite, verdict};
use super::render::{LATE, UNIT_BUDGET, note_ons, play_on, play_stim};
use super::{Audition, SR, heard};
use crate::listen;

/// What a demo render plays: a Part's script on its channel, or a
/// listening stimulus into the FX bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoTake {
    Part(PartId, Script),
    Hats,
    Snare,
}

impl DemoTake {
    /// Its WAV name's end: `part1-stab`, `hats`.
    pub(super) fn slug(self) -> String {
        match self {
            DemoTake::Part(p, s) => format!("part{}-{}", p.index() + 1, s.word()),
            DemoTake::Hats => "hats".into(),
            DemoTake::Snare => "snare".into(),
        }
    }

    pub(super) fn label(self) -> String {
        match self {
            DemoTake::Part(p, s) => format!("PART {} {}", p.index() + 1, s.word().to_uppercase()),
            DemoTake::Hats => "HATS".into(),
            DemoTake::Snare => "SNARE".into(),
        }
    }
}

/// What a demo gate checked, on which take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoCheck {
    /// The count of samples that are not finite.
    Finite(DemoTake),
    Clicks(DemoTake),
    Limiter(DemoTake),
}

impl DemoCheck {
    pub(super) fn label(self) -> String {
        let (what, t) = match self {
            DemoCheck::Finite(t) => ("finite", t),
            DemoCheck::Clicks(t) => ("clicks", t),
            DemoCheck::Limiter(t) => ("limiter", t),
        };
        format!("{what} {}", t.label().to_lowercase())
    }

    pub(super) fn unit(self) -> &'static str {
        match self {
            DemoCheck::Finite(_) => "bad",
            DemoCheck::Clicks(_) => "x",
            DemoCheck::Limiter(_) => "dB GR",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemoGate {
    /// The setting (`project::settings`) it played under.
    pub setting: usize,
    pub check: DemoCheck,
    pub value: f32,
    pub verdict: Verdict,
}

/// `chimera-utils stim`'s hats and snare, mono at 48 kHz.
#[derive(Clone, Debug)]
pub struct Stims {
    pub hats: Vec<f32>,
    pub snare: Vec<f32>,
}

/// One demo render, P1 as heard, interleaved stereo.
#[derive(Clone, Debug)]
pub struct DemoRender {
    pub setting: usize,
    pub take: DemoTake,
    pub stereo: Vec<f32>,
    pub lufs: f32,
    pub peak_dbfs: [f32; 2],
    pub limited_db: f32,
}

/// A demo project's QA: what it holds, its settings, gates and renders.
#[derive(Clone, Debug)]
pub struct DemoReport {
    pub id: DemoId,
    pub name: String,
    pub tries: String,
    /// `PART 1 · NYLON (APPROVED) · POLY · CH 1`.
    pub parts: Vec<String>,
    /// Each setting's effects heard, in full words.
    pub settings: Vec<String>,
    pub gates: Vec<DemoGate>,
    pub renders: Vec<DemoRender>,
    /// Held (`project::Demo::held`): no gates, no renders.
    pub held: Option<Held>,
}

/// A held demo: why, and the `Coverage::pending_lines` naming it.
#[derive(Clone, Debug)]
pub struct Held {
    pub reason: String,
    pub pending: Vec<String>,
}

impl DemoReport {
    /// A held demo has no gate to fail.
    pub fn passes(&self) -> bool {
        self.gates.iter().all(|g| g.verdict != Verdict::Fail)
    }

    pub(super) fn file(&self, r: &DemoRender) -> String {
        format!("{}-{}-{}.wav", self.id.stem(), r.setting + 1, r.take.slug())
    }
}

/// A demo render's room past its last note, for a long reverb's tail.
pub(super) const DEMO_TAIL_S: f32 = 4.0;

/// Each setting plays every Part's `stab` and `short`, then the hats and
/// the snare.
pub fn demo_takes(d: &Demo) -> Vec<DemoTake> {
    d.parts
        .iter()
        .flat_map(|p| [Script::Stab, Script::Short].map(|s| DemoTake::Part(p.part, s)))
        .chain([DemoTake::Hats, DemoTake::Snare])
        .collect()
}

/// A setting's effects, as the page names them.
pub(super) fn heard_words(s: &Setting) -> String {
    if s.heard.is_empty() {
        return "no effect heard".into();
    }
    s.heard
        .iter()
        .map(|e| e.label())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Where a stimulus starts sounding: after 10 ms or more under −80 dBFS.
pub(super) fn onsets(x: &[f32]) -> Vec<usize> {
    let gap = (0.01 * SR) as usize;
    let mut quiet = gap;
    let mut out = Vec::new();
    for (i, v) in x.iter().enumerate() {
        if v.abs() < 1e-4 {
            quiet += 1;
            continue;
        }
        if quiet >= gap {
            out.push(i);
        }
        quiet = 0;
    }
    out
}

/// `x` without its trailing silence.
pub(super) fn sounding(x: &[f32]) -> &[f32] {
    let end = x.iter().rposition(|v| *v != 0.0).map_or(0, |i| i + 1);
    &x[..end]
}

/// Every setting of `d` (`project::settings`) played: each Part's `stab`
/// and `short` on its channel through the project as built, then the hats
/// and the snare into the same FX bus at the Parts' most send per effect.
/// Gates: finite, clicks (a Part's Sound exempt by `listen::click_exempt`
/// left to the ear), and the limiter at the chord bar (3 dB).
pub fn demo_gates(d: &Demo, sounds: &[Source], stims: &Stims) -> Result<DemoReport, String> {
    let project = project::build(d, sounds)?;
    let settings = project::settings(d, sounds)?;
    let parts = part_lines(d, sounds)?;
    let mut sends = [SendLevel::OFF; FX_SENDS];
    let mut exempt = Vec::new();
    for p in &d.parts {
        let src = demo_source(p, sounds);
        let snd = to_sound(src)?;
        for (m, v) in sends.iter_mut().zip(snd.params.out.sends) {
            if v > *m {
                *m = v;
            }
        }
        // Each Part's click exemption and home note.
        exempt.push((p.part, listen::click_exempt(&snd), src.home));
    }
    let mut perf = Performance::new();
    for p in PartId::ALL {
        let e = perf.edit(p);
        **e.sound = (*project.part(p).sound).clone();
        *e.mix = project.part(p).mix;
    }
    let mut gates = Vec::new();
    let mut renders = Vec::new();
    for (i, s) in settings.iter().enumerate() {
        perf.fx = s.fx;
        for take in demo_takes(d) {
            let (t, onset, ex) = match take {
                DemoTake::Part(part, script) => {
                    let (ex, home) = exempt
                        .iter()
                        .find(|e| e.0 == part)
                        .map_or((None, 60), |e| (e.1, e.2));
                    let mut p = plan(script, home);
                    p.secs += DEMO_TAIL_S;
                    let ch = perf.part(part).mix.channel;
                    (play_on(&perf, ch, &p, UNIT_BUDGET), note_ons(&p), ex)
                }
                DemoTake::Hats | DemoTake::Snare => {
                    let x = sounding(if take == DemoTake::Hats {
                        &stims.hats
                    } else {
                        &stims.snare
                    });
                    let secs = x.len() as f32 / SR + DEMO_TAIL_S;
                    // Block 0 silent, and the limiter's block late.
                    let on = onsets(x)
                        .into_iter()
                        .map(|o| o + BLOCK_SIZE + LATE)
                        .collect();
                    (play_stim(&s.fx, sends, x, secs), on, None)
                }
            };
            let mut gate = |check, (value, verdict)| {
                gates.push(DemoGate {
                    setting: i,
                    check,
                    value,
                    verdict,
                })
            };
            gate(DemoCheck::Finite(take), finite(&t.stereo));
            gate(DemoCheck::Clicks(take), clicks(&t.mono(), &onset, ex));
            gate(
                DemoCheck::Limiter(take),
                (t.reduced_db, verdict(t.reduced_db <= LIMIT_CHORD_DB)),
            );
            let limited_db = t.reduced_db;
            let r = heard(Audition::Script(Script::Stab), t, &[], None);
            renders.push(DemoRender {
                setting: i,
                take,
                lufs: r.lufs,
                peak_dbfs: r.peak_dbfs,
                limited_db,
                stereo: r.stereo,
            });
        }
    }
    Ok(DemoReport {
        id: d.id,
        name: d.name.as_str().into(),
        tries: d.tries.clone(),
        parts,
        settings: settings.iter().map(heard_words).collect(),
        gates,
        renders,
        held: None,
    })
}

/// A checked demo's Part Source (`project::check` found it).
pub(super) fn demo_source<'a>(p: &project::DemoPart, sounds: &'a [Source]) -> &'a Source {
    sounds
        .iter()
        .find(|s| s.name.as_str() == p.sound)
        .expect("project::check found it")
}

/// `PART 1 · NYLON (APPROVED) · POLY · CH 1`, per Part.
pub(super) fn part_lines(d: &Demo, sounds: &[Source]) -> Result<Vec<String>, String> {
    d.parts
        .iter()
        .map(|p| {
            let src = demo_source(p, sounds);
            Ok(format!(
                "PART {} · {} ({}) · {} · CH {}",
                p.part.index() + 1,
                p.sound,
                src.badge(source::approval_crc(src)?).word(),
                p.mode.disk_ident(),
                p.channel.get() + 1
            ))
        })
        .collect()
}

/// A held demo's report: what it holds and would play, why it waits and
/// what of `cov` waits on it; nothing rendered.
pub fn held_report(d: &Demo, sounds: &[Source], cov: &Coverage) -> Result<DemoReport, String> {
    let reason = d.held.clone().ok_or("held_report on a demo not held")?;
    let pending = Coverage {
        unheard: Vec::new(),
        pending: cov
            .pending
            .iter()
            .filter(|(_, ids)| ids.contains(&d.id))
            .cloned()
            .collect(),
    }
    .pending_lines();
    Ok(DemoReport {
        id: d.id,
        name: d.name.as_str().into(),
        tries: d.tries.clone(),
        parts: part_lines(d, sounds)?,
        settings: project::settings(d, sounds)?
            .iter()
            .map(heard_words)
            .collect(),
        gates: Vec::new(),
        renders: Vec::new(),
        held: Some(Held { reason, pending }),
    })
}
