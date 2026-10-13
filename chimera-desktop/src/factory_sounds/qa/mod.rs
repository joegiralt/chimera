//! QA before UAT (spec § 4): each factory Sound played headlessly
//! (`render`), measured dry (its sends at 0) by pure gates (`measure`);
//! the listening renders carry its sends through the FX bus at NEW's
//! settings, the first of them a phrase (a listening render, never a
//! gate). An APPROVED Sound's renders against a baseline (`baseline`), the
//! demo projects (`demo`), then `report.txt` and the owner's
//! `listening.html` in the ledger folder (`page`, the only I/O).

pub mod baseline;
pub mod demo;
pub mod measure;
pub mod page;
pub mod render;

use chimera_core::hw::SampleBudget;
use chimera_core::part::PartMode;
use chimera_core::storage::FactoryId;
use chimera_hal::SAMPLE_RATE;

use super::about::{self, Fact};
use super::phrase::{Plan, hit, phrase_plan, plan};
use super::source::{self, Badge, Phrase, Round, Script, Source, patch, to_sound, trimmed};
use crate::listen;
use baseline::Baseline;
use measure::{
    ALIAS_NOTES, ERB_BANDS, LIMIT_CHORD_DB, LIMIT_SINGLE_DB, REFERENCE_LUFS, TAIL_WINDOW_S,
    Verdict, aliasing, clicks, db, erb_bands, finite, held_dc, limiter, limiter_bar, loudness,
    spectral_error, tail, tail_within, tap,
};
use render::{LATE, Take, UNIT_BUDGET, dried, heard_through, note_ons, play, play_at, voices_of};

const SR: f32 = SAMPLE_RATE as f32;

/// The level render holds the home note this long (`loudness_test`).
const LEVEL_HOLD_S: f32 = 1.0;

/// `a` played as the page plays it, at `budget`: its most voices at once.
fn voices_at(src: &Source, a: Audition, budget: SampleBudget) -> Result<usize, String> {
    let p = match a {
        Audition::Phrase(ph) => phrase_plan(ph, src.home),
        Audition::Script(sc) => plan(sc, src.home),
    };
    Ok(play_at(&to_sound(src)?, src.demo, &p, budget).voices)
}

/// `src`'s render `a` sounds more voices at once at an unlimited budget
/// than at the unit's: the voice budget changed it.
pub fn over_budget(src: &Source, a: Audition) -> Result<bool, String> {
    Ok(voices_at(src, a, SampleBudget::for_cpu(u32::MAX))? > voices_at(src, a, UNIT_BUDGET)?)
}

/// After `compare`: a changed Sound's voice budget is a cause only when a
/// render of it is proven `over_budget`.
pub fn voice_cause(r: &mut Report, src: &Source) -> Result<(), String> {
    let Some(b) = r.baseline.as_mut() else {
        return Ok(());
    };
    b.voice_budget = None;
    if b.changed().is_none() {
        return Ok(());
    }
    for x in &r.renders {
        if over_budget(src, x.audition)? {
            b.voice_budget = Some(r.voices);
            break;
        }
    }
    Ok(())
}

/// What a listening render plays: the Sound's phrase, or a render script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audition {
    Phrase(Phrase),
    Script(Script),
}

impl Audition {
    /// Its WAV's name: `phrase-bass`, or the script's word.
    pub(super) fn slug(self) -> String {
        match self {
            Audition::Phrase(p) => format!("phrase-{}", p.word()),
            Audition::Script(s) => s.word().into(),
        }
    }

    /// The player's label: `BASS PHRASE`, `LONG`.
    pub(super) fn label(self) -> String {
        match self {
            Audition::Phrase(p) => format!("{} PHRASE", p.word().to_uppercase()),
            Audition::Script(s) => s.word().to_uppercase(),
        }
    }

    /// The bar its limiter reading is flagged over: a pad's or sweep's
    /// chords (as `stab`) 3 dB, else 1 dB.
    pub fn bar(self) -> f32 {
        match self {
            Audition::Phrase(Phrase::Pad | Phrase::Sweep) => LIMIT_CHORD_DB,
            Audition::Phrase(_) => LIMIT_SINGLE_DB,
            Audition::Script(s) => limiter_bar(Some(s)),
        }
    }
}

/// What a gate checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    Loudness,
    /// The level render (`None`) or a script's.
    Limiter(Option<Script>),
    Clicks(Script),
    /// A held home note at this velocity.
    Dc(u8),
    /// At this note.
    Aliasing(u8),
    Tail,
    Tap,
    /// The phrase render holds no NaN or infinity (the count of samples
    /// that are not finite).
    Finite(Phrase),
}

impl Check {
    pub(super) fn label(&self) -> String {
        match self {
            Check::Loudness => "loudness".into(),
            Check::Limiter(None) => "limiter level".into(),
            Check::Limiter(Some(s)) => format!("limiter {}", s.word()),
            Check::Clicks(s) => format!("clicks {}", s.word()),
            Check::Dc(v) => format!("DC v{v}"),
            Check::Aliasing(n) => format!("aliasing C{}", n / 12 - 1),
            Check::Tail => "tail".into(),
            Check::Tap => "tap".into(),
            Check::Finite(p) => format!("finite {} phrase", p.word()),
        }
    }

    pub(super) fn unit(&self) -> &'static str {
        match self {
            Check::Loudness => "LUFS",
            Check::Limiter(_) => "dB GR",
            Check::Clicks(_) => "x",
            Check::Dc(_) => "dB rel",
            Check::Aliasing(_) | Check::Tap => "dB",
            Check::Tail => "dBFS",
            Check::Finite(_) => "bad",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gate {
    pub check: Check,
    pub value: f32,
    pub verdict: Verdict,
}

/// One listening render, for the page: P1 as heard, interleaved stereo.
#[derive(Clone, Debug)]
pub struct Render {
    pub audition: Audition,
    pub stereo: Vec<f32>,
    pub lufs: f32,
    /// Left and right.
    pub peak_dbfs: [f32; 2],
    /// The effects heard on it (its sends), full words: empty when dry.
    pub with: Vec<&'static str>,
    /// A wet render's limiting, and always the phrase's, dB: information,
    /// not a gate (the gates measure dry).
    pub limited_db: Option<f32>,
    /// The most voices it sounded at once (`UNIT_BUDGET`).
    pub voices: usize,
}

/// A Sound's QA: every gate, every render, and its `long` render's ERB
/// bands (for `samey`).
#[derive(Clone, Debug)]
pub struct Report {
    pub id: FactoryId,
    pub name: String,
    pub note: String,
    pub demo: PartMode,
    pub edits: Option<String>,
    /// A port (it has a `from` line): the page's PRIVATE section.
    pub private: bool,
    pub gates: Vec<Gate>,
    pub renders: Vec<Render>,
    /// The dry `long` render's LUFS, beside the level figure.
    pub long_lufs: Option<f32>,
    pub bands: [f32; ERB_BANDS],
    pub badge: Badge,
    /// Approved in this round, but the Sound has changed since: NEW again.
    pub changed: Option<Round>,
    /// What it is (`about`), from the built Sound.
    pub about: Vec<Fact>,
    /// An APPROVED Sound's renders against `--baseline` (`compare`).
    pub baseline: Option<Baseline>,
    /// Its routed FREE LFOs (`about::free_lfos`), for a #489 cause.
    pub free_lfos: Vec<String>,
    /// The voices the unit sounds for it (`unit_voices`).
    pub voices: usize,
}

impl Report {
    pub fn passes(&self) -> bool {
        self.gates.iter().all(|g| g.verdict != Verdict::Fail)
    }

    fn wav(&self, a: Audition) -> String {
        format!("renders/{}", self.file(a))
    }

    pub(super) fn file(&self, a: Audition) -> String {
        format!("{}-{}.wav", self.id, a.slug())
    }

    fn lufs(&self) -> Option<f32> {
        self.gates
            .iter()
            .find(|g| g.check == Check::Loudness)
            .map(|g| g.value)
    }
}

/// A take as the page plays it, with its peaks and LUFS.
fn heard(audition: Audition, t: Take, with: &[&'static str], limited_db: Option<f32>) -> Render {
    let peak = |c: usize| {
        db(t.stereo
            .iter()
            .skip(c)
            .step_by(2)
            .fold(0f32, |m, v| m.max(v.abs())))
    };
    Render {
        audition,
        lufs: listen::lufs(&t.stereo),
        peak_dbfs: [peak(0), peak(1)],
        with: with.to_vec(),
        limited_db,
        voices: t.voices,
        stereo: t.stereo,
    }
}

fn hz(n: u8) -> f32 {
    440.0 * 2f32.powf((n as f32 - 69.0) / 12.0)
}

/// The home note at `vel`, held `LEVEL_HOLD_S`, then `tail` s: the level
/// render and its kin.
fn held_note(home: u8, vel: u8, tail: f32) -> Plan {
    Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, home, vel)],
        secs: LEVEL_HOLD_S + tail,
        mode: None,
    }
}

/// A `held_note` take's LUFS over the held note (a block late).
fn held_lufs(t: &Take) -> f32 {
    let held = (LEVEL_HOLD_S * SR) as usize;
    listen::lufs(&t.stereo[2 * LATE..2 * (LATE + held)])
}

/// Every gate on `src`, through the renders its gates and `plays` need.
pub fn gates(src: &Source) -> Result<Report, String> {
    // Every gate plays dry; the page's renders carry the sends.
    let wet = to_sound(src)?;
    let crc = source::approval_crc(src)?;
    let with = heard_through(&wet);
    let snd = dried(&wet);
    let within = tail_within(src, &snd);
    let exempt = listen::click_exempt(&snd);
    let mut gates = Vec::new();
    let mut gate = |check, (value, verdict)| {
        gates.push(Gate {
            check,
            value,
            verdict,
        })
    };

    // The level render: the home note at v100, held 1 s, then its tail.
    let level = held_note(src.home, 100, within + 2.0 * TAIL_WINDOW_S);
    let t = play(&snd, src.demo, &level);
    let lufs = held_lufs(&t);
    gate(
        Check::Loudness,
        (
            lufs,
            loudness(lufs, src.level.as_ref().map(|p| (p.lufs, p.why.as_str()))),
        ),
    );
    gate(
        Check::Limiter(None),
        (t.reduced_db, limiter(t.reduced_db, None)),
    );
    // DC where an offset would sit: the held home note, at v100 and at
    // v127 (velocity drives a fold or a bit crush harder).
    gate(Check::Dc(100), held_dc(&t.stereo, &level.hits));
    let hard = held_note(src.home, 127, 0.05);
    gate(
        Check::Dc(127),
        held_dc(&play(&snd, src.demo, &hard).stereo, &hard.hits),
    );
    gate(
        Check::Tail,
        tail(&t.mono(), (LEVEL_HOLD_S * SR) as usize + LATE, within),
    );

    let mut renders = Vec::new();
    // The phrase first, as heard (its sends on): for the ear, not a gate,
    // but it must be finite.
    if let Some(ph) = src.phrase {
        let t = play(&wet, src.demo, &phrase_plan(ph, src.home));
        gate(Check::Finite(ph), finite(&t.stereo));
        let r = t.reduced_db;
        renders.push(heard(Audition::Phrase(ph), t, &with, Some(r)));
    }
    let mut dry_long = None;
    for &s in &src.plays {
        let p = plan(s, src.home);
        let t = play(&snd, src.demo, &p);
        let mono = t.mono();
        gate(
            Check::Limiter(Some(s)),
            (t.reduced_db, limiter(t.reduced_db, Some(s))),
        );
        gate(Check::Clicks(s), clicks(&mono, &note_ons(&p), exempt));
        if s == Script::Long {
            dry_long = Some((mono, listen::lufs(&t.stereo)));
        }
        let (t, limited_db) = if with.is_empty() {
            (t, None)
        } else {
            let w = play(&wet, src.demo, &p);
            let r = w.reduced_db;
            (w, Some(r))
        };
        renders.push(heard(Audition::Script(s), t, &with, limited_db));
    }

    // `long`'s held part, past the attack.
    let held_part = |x: &[f32]| x[(0.25 * SR) as usize..(2.0 * SR) as usize].to_vec();
    let harmonic = src.harmonic;
    for n in ALIAS_NOTES {
        let x = if harmonic {
            held_part(&play(&snd, src.demo, &plan(Script::Long, n)).mono())
        } else {
            Vec::new()
        };
        gate(Check::Aliasing(n), aliasing(&x, hz(n), harmonic));
    }

    let (long, long_lufs) = match dry_long {
        Some(x) => x,
        None => {
            let t = play(&snd, src.demo, &plan(Script::Long, src.home));
            (t.mono(), listen::lufs(&t.stereo))
        }
    };
    if !src.taps.is_empty() {
        let mut bare = snd.clone();
        bare.params.taps = Default::default();
        let without = play(&bare, src.demo, &plan(Script::Long, src.home)).mono();
        let err = spectral_error(&erb_bands(&long), &erb_bands(&without));
        gate(Check::Tap, (err, tap(err)));
    }

    Ok(Report {
        id: src.id,
        name: src.name.as_str().into(),
        note: src.note.as_str().into(),
        demo: src.demo,
        edits: src.edits.as_ref().map(|e| e.text.clone()),
        private: src.from.is_some(),
        gates,
        renders,
        long_lufs: Some(long_lufs),
        bands: erb_bands(&long),
        badge: src.badge(crc),
        changed: src.changed(crc),
        about: about::of(src)?,
        baseline: None,
        free_lfos: about::free_lfos(&wet),
        voices: voices_of(&snd, src.demo),
    })
}

/// The `trim` that centres a Source measured at `lufs` on −11 LUFS, one
/// place after the point, refused (never clipped) when it would push OUT
/// LEVEL out of range: the error names the Source's trim line (or its id
/// line when it has none).
pub fn level_trim(src: &Source, lufs: f32) -> Result<f32, String> {
    let t = ((src.trim + REFERENCE_LUFS - lufs) * 10.0).round() / 10.0;
    let level = patch(src)?.params.out.volume;
    let line = if src.trim_line > 0 {
        src.trim_line
    } else {
        src.id_line
    };
    trimmed(level, t).map_err(|m| format!("{}:{line}: {} {m}", src.file, src.name.as_str()))?;
    Ok(t + 0.0) // never -0.0
}

/// `text` with each Source's `trim` centred on −11 LUFS (`fmt --level`):
/// measured, trimmed, measured again (the limiter bends a loud first
/// guess), then `backed_off` while a limiter gate would fail. Only Sounds shown NEW: placeholders, approved Sounds and
/// levels pinned by ear keep theirs.
pub fn levelled(text: &str, all: &[Source]) -> Result<String, String> {
    let mut trims = Vec::new();
    for s in all.iter().filter(|s| s.level.is_none()) {
        if s.badge(source::approval_crc(s)?) != Badge::New {
            continue;
        }
        let mut t = s.clone();
        for _ in 0..2 {
            t.trim = level_trim(&t, level_lufs(&t)?)?;
        }
        t.trim = backed_off(t.trim, |trim| {
            let mut u = t.clone();
            u.trim = trim;
            over_limit(&u)
        })?;
        println!("{} trim {:+.1} dB", s.name.as_str(), t.trim);
        trims.push((s, t.trim));
    }
    Ok(source::set_trims(text, &trims))
}

/// How far `backed_off` may go: 6 dB.
const BACK_OFF_STEPS: u8 = 60;

/// `trim` stepped down 0.1 dB at a time while `over` it (a limiter gate
/// would fail): the smallest cut that passes. Refused past 6 dB.
pub fn backed_off(
    trim: f32,
    mut over: impl FnMut(f32) -> Result<bool, String>,
) -> Result<f32, String> {
    let tenths = (trim * 10.0).round();
    for k in 0..=BACK_OFF_STEPS {
        let t = (tenths - k as f32) / 10.0 + 0.0;
        if !over(t)? {
            return Ok(t);
        }
    }
    Err(format!(
        "trim {trim:+.1}: still over the limiter's bar {} dB down",
        BACK_OFF_STEPS / 10
    ))
}

/// Some limiter gate on `src` fails: the level render's, or a script's
/// (`gates`, dry).
pub fn over_limit(src: &Source) -> Result<bool, String> {
    let snd = dried(&to_sound(src)?);
    let mut takes = vec![(None, held_note(src.home, 100, 0.05))];
    takes.extend(src.plays.iter().map(|&s| (Some(s), plan(s, src.home))));
    Ok(takes
        .iter()
        .any(|(s, p)| limiter(play(&snd, src.demo, p).reduced_db, *s) == Verdict::Fail))
}

/// The level render's LUFS (`fmt --level` centres it).
pub fn level_lufs(src: &Source) -> Result<f32, String> {
    let snd = dried(&to_sound(src)?);
    Ok(held_lufs(&play(
        &snd,
        src.demo,
        &held_note(src.home, 100, 0.05),
    )))
}
