//! An APPROVED Sound's renders against a baseline's (`--baseline`): the
//! residual, and the engine changes it bisects to (`CAUSES`).

use std::path::Path;

use super::super::source::Badge;
use super::{Audition, Report};
use crate::listen;

/// A render whose residual against its baseline is over this was changed
/// by the engine, not by its Source: re-listen.
pub const AUDIO_CHANGED_DB: f32 = -40.0;
/// The page's line for it; the badge stays.
pub const AUDIO_CHANGED: &str = "AUDIO CHANGED BY ENGINE UPDATE: re-listen";

/// A residual at or under this still nulls: the same waveform, moved
/// (coherent drift), and its size says how far. Near 0 dB the takes no
/// longer null at all (a re-phased LFO decorrelates them), so the size
/// says nothing. Between the two clusters #495 left: -6 to +3 dB, and
/// -12.6 to -39.4 dB. It measures coherent drift, not timbre.
pub const COHERENT_DB: f32 = -11.0;
/// What the page and `report.txt` say the residual measures.
pub const RESIDUAL_NOTE: &str = "AUDIO CHANGED compares each render with the baseline's: its \
     residual measures coherent drift, not timbre (near 0 dB the takes no longer null at all).";
/// The bisect evidence beside the baseline WAVs: `S<id>` and, for each
/// engine change its residual bisects to, `#<issue> <dB>`; `#` comments.
pub const CAUSES: &str = "causes.txt";

/// An engine change a Sound's residual bisects to (`CAUSES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineChange {
    /// #485: the VA sub flips at the shaped saw's edge.
    SubFlip,
    /// #489: FREE LFOs play their Part's clock (ADR 0135).
    LfoClock,
}

impl EngineChange {
    fn parse(w: &str) -> Option<Self> {
        match w {
            "#485" => Some(Self::SubFlip),
            "#489" => Some(Self::LfoClock),
            _ => None,
        }
    }

    /// Its page line at `db`, naming the Sound's `free_lfos` for #489.
    fn line(self, db: f32, free_lfos: &[String]) -> String {
        match self {
            Self::SubFlip => {
                format!("TIMBRE CHANGED {db:.1} dB: #485 the sub flips at the shaped saw's edge")
            }
            Self::LfoClock => {
                let (lfos, verb) = match free_lfos {
                    [] => ("its LFOs".to_string(), "run"),
                    [one] => (one.clone(), "runs"),
                    many => (many.join(" and "), "run"),
                };
                format!(
                    "LFO MOTION CHANGED {db:.1} dB: #489 FREE {lfos} now {verb} on the Part clock"
                )
            }
        }
    }
}

/// `text` (a `CAUSES` file) for the Sound `id`: each change and its
/// residual. A bad line fails with its number.
pub fn causes(text: &str, id: u32) -> Result<Vec<(EngineChange, f32)>, String> {
    let mut out = Vec::new();
    for (n, l) in text.lines().enumerate() {
        // `#` starts a comment only at the line's start: `#485` is a word.
        if l.trim_start().starts_with('#') {
            continue;
        }
        let words: Vec<&str> = l.split_whitespace().collect();
        let bad = |why: &str| Err(format!("{CAUSES}:{}: {why}", n + 1));
        let Some((first, rest)) = words.split_first() else {
            continue;
        };
        let Some(sid) = first.strip_prefix('S').and_then(|v| v.parse::<u32>().ok()) else {
            return bad("an S<id> first");
        };
        if rest.is_empty() || rest.len() % 2 != 0 {
            return bad("each change with its residual: #<issue> <dB>");
        }
        let mut these = Vec::new();
        for p in rest.chunks(2) {
            let (Some(c), Ok(db)) = (EngineChange::parse(p[0]), p[1].parse::<f32>()) else {
                return bad(&format!("{} {}: not #485 or #489 and a dB", p[0], p[1]));
            };
            these.push((c, db));
        }
        if sid == id {
            out = these;
        }
    }
    Ok(out)
}

/// A Sound's renders against the baseline's.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    /// The render with the largest residual, and it, dB.
    pub worst: Option<(Audition, f32)>,
    /// Renders with no baseline WAV: not compared.
    pub missing: Vec<Audition>,
    /// What its change bisects to (`CAUSES`), each with its residual.
    pub causes: Vec<(EngineChange, f32)>,
    /// The voices the unit plays, when the voice budget is proven a cause
    /// (`voice_cause`).
    pub voice_budget: Option<usize>,
}

impl Baseline {
    /// The worst render, when it is over `AUDIO_CHANGED_DB`.
    pub fn changed(&self) -> Option<(Audition, f32)> {
        self.worst.filter(|&(_, db)| db > AUDIO_CHANGED_DB)
    }

    /// Changed, and some of it still nulls (`COHERENT_DB`): heard first.
    pub fn coherent(&self) -> bool {
        let still = |db: f32| db > AUDIO_CHANGED_DB && db <= COHERENT_DB;
        match (self.changed(), self.causes.as_slice()) {
            (None, _) => false,
            (Some((_, db)), []) => still(db),
            (Some(_), c) => c.iter().any(|&(_, db)| still(db)),
        }
    }

    /// The page's flagged lines: `AUDIO_CHANGED`, then each cause.
    pub fn lines(&self, free_lfos: &[String]) -> Vec<String> {
        if self.changed().is_none() {
            return Vec::new();
        }
        std::iter::once(AUDIO_CHANGED.to_string())
            .chain(self.causes.iter().map(|&(c, db)| c.line(db, free_lfos)))
            .chain(
                self.voice_budget
                    .map(|n| format!("voice budget: the unit plays {n} voices")),
            )
            .collect()
    }
}

/// `new − old` against `old`, dB (energy over energy: the RMS ratio), the
/// two aligned sample for sample as written and the shorter padded with
/// silence. Identical: −∞.
pub fn residual_db(new: &[f32], old: &[f32]) -> f32 {
    let at = |x: &[f32], i: usize| x.get(i).copied().unwrap_or(0.0) as f64;
    let diff: f64 = (0..new.len().max(old.len()))
        .map(|i| (at(new, i) - at(old, i)).powi(2))
        .sum();
    if diff == 0.0 {
        return f32::NEG_INFINITY;
    }
    let base: f64 = old.iter().map(|&v| (v as f64).powi(2)).sum();
    (10.0 * (diff / base).log10()) as f32
}

/// An APPROVED Sound's renders, as `write` writes them, against the WAVs
/// of the same names in `dir` (`--baseline`), into `r.baseline`; any other
/// Sound is not compared. A missing WAV is noted, not flagged.
pub fn compare(r: &mut Report, dir: &Path) -> Result<(), String> {
    if r.badge != Badge::Approved {
        r.baseline = None;
        return Ok(());
    }
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let c = dir.join(CAUSES);
    let mut b = Baseline {
        worst: None,
        missing: Vec::new(),
        causes: if c.is_file() {
            causes(&read(&c)?, r.id.get()).map_err(|e| format!("{}/{e}", dir.display()))?
        } else {
            Vec::new()
        },
        voice_budget: None,
    };
    let unit = |v: i16| v as f32 / 32_767.0;
    for x in &r.renders {
        let p = dir.join(r.file(x.audition));
        if !p.is_file() {
            b.missing.push(x.audition);
            continue;
        }
        let err = |e: hound::Error| format!("{}: {e}", p.display());
        let w = hound::WavReader::open(&p).map_err(err)?;
        if w.spec().channels != 2 || w.spec().bits_per_sample != 16 {
            return Err(format!("{}: not 16-bit stereo", p.display()));
        }
        let old: Vec<f32> = w
            .into_samples::<i16>()
            .map(|v| v.map(unit))
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let new: Vec<f32> = listen::wav_samples(&x.stereo, 2).map(unit).collect();
        let db = residual_db(&new, &old);
        if b.worst.is_none_or(|(_, w)| db > w) {
            b.worst = Some((x.audition, db));
        }
    }
    r.baseline = Some(b);
    Ok(())
}
