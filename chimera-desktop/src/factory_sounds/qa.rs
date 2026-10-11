//! QA before UAT (spec § 4): each factory Sound played headlessly through
//! the real `Instrument` (as `loudness_test` plays a scene: Part 1, P1,
//! after the limiter, FX off), measured by pure gates over the sample
//! buffers, then `report.txt` and the owner's `listening.html` with the
//! renders, all in the ledger folder. `write` is the only I/O.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use chimera_core::block::DiskCode;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::limiter::OUTPUT_GAIN;
use chimera_core::dsp::modulator::law::{B_TIME, speed_ranges};
use chimera_core::dsp::modulator::{EnvSlot, EnvType};
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::storage::FactoryId;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use super::source::{Script, Source, patch, to_sound, trimmed};
use crate::listen::{self, CLICK_RATIO, Player, Row, qa_takes};

const SR: f32 = SAMPLE_RATE as f32;

/// ADR 0109's reference (ADR 0063's at the master gain): ALGO INIT's C4
/// at velocity 100 on P1.
pub const REFERENCE_LUFS: f32 = -11.0;
/// The loudness gate's half-width.
pub const LOUDNESS_LU: f32 = 2.0;
/// Gain reduction a single note may take, and the chord `stab`.
pub const LIMIT_SINGLE_DB: f32 = 1.0;
pub const LIMIT_CHORD_DB: f32 = 3.0;
/// A render's mean must sit under this.
pub const DC_DBFS: f32 = -60.0;
/// Harmonic over inharmonic energy, at C6 and C7.
pub const ALIAS_DB: f32 = 40.0;
/// Silence, and how long past the release it may take to get there.
pub const TAIL_DBFS: f32 = -90.0;
pub const TAIL_GRACE_S: f32 = 2.0;
/// MODAL rings on past note-off (Rings): its own limit.
pub const MODAL_TAIL_S: f32 = 10.0;
/// The window the tail is judged over, at its limit.
const TAIL_WINDOW_S: f32 = 0.1;
/// The tap must move the spectrum at least this (RMS over the ERB bands).
pub const TAP_DB: f32 = 6.0;
/// Two Sounds within this on every ERB band are samey.
pub const SAMEY_DB: f32 = 3.0;
pub const ERB_BANDS: usize = 32;
/// The ERB bands span this.
const ERB_LO_HZ: f32 = 50.0;
const ERB_HI_HZ: f32 = 20_000.0;
/// The aliasing renders' notes: C6 and C7.
pub const ALIAS_NOTES: [u8; 2] = [84, 96];
/// After the last note-off, each script's tail (but `long`'s 2 s).
const TAIL_S: f32 = 1.5;
/// The level render holds the home note this long (`loudness_test`).
const LEVEL_HOLD_S: f32 = 1.0;
/// The limiter's lookahead: the output is a block late.
const LATE: usize = BLOCK_SIZE;

/// One note of a script: seconds on and off, the note and its velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub on: f32,
    pub off: f32,
    pub note: u8,
    pub vel: u8,
}

/// A script's notes and its length; `mode` overrides the Source's demo.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub hits: Vec<Hit>,
    pub secs: f32,
    pub mode: Option<PartMode>,
}

fn hit(on: f32, off: f32, note: u8, vel: u8) -> Hit {
    Hit { on, off, note, vel }
}

/// Up two octaves of a major seventh and back: `texture`'s sixteen notes.
const TEXTURE: [u8; 16] = [0, 4, 7, 11, 12, 16, 19, 23, 24, 23, 19, 16, 12, 11, 7, 4];

/// Spec § 4's render scripts, velocity 100 unless noted.
pub fn plan(s: Script, home: u8) -> Plan {
    let one = |held: f32, note: u8, tail: f32| Plan {
        hits: vec![hit(0.0, held, note, 100)],
        secs: held + tail,
        mode: None,
    };
    match s {
        Script::Bass => one(0.3, home, TAIL_S),
        Script::Short => one(0.08, 60, TAIL_S),
        Script::Long => one(2.0, home, 2.0),
        Script::Stab => Plan {
            hits: [48, 52, 55, 59].map(|n| hit(0.0, 0.25, n, 100)).to_vec(),
            secs: 0.25 + TAIL_S,
            mode: None,
        },
        // Sixteenths at 120 BPM, velocity 40 rising to 127.
        Script::Texture => Plan {
            hits: TEXTURE
                .iter()
                .enumerate()
                .map(|(i, &d)| {
                    let on = i as f32 * 0.125;
                    let vel = 40 + (87.0 * i as f32 / 15.0).round() as u8;
                    hit(on, on + 0.1, home.saturating_add(d).min(127), vel)
                })
                .collect(),
            secs: 2.0 + TAIL_S,
            mode: None,
        },
        // C2, G2, C3, each overlapping the next, on a MONO Part.
        Script::Glide => Plan {
            hits: vec![
                hit(0.0, 0.5, 36, 100),
                hit(0.4, 0.9, 43, 100),
                hit(0.8, 1.4, 48, 100),
            ],
            secs: 1.4 + TAIL_S,
            mode: Some(PartMode::Mono),
        },
    }
}

/// The block an instant falls in.
fn block(t: f32) -> usize {
    (t * SR / BLOCK_SIZE as f32).round() as usize
}

/// A played plan: P1 after the limiter (interleaved stereo), and the most
/// gain reduction the limiter took in any block, dB (0 or more).
struct Take {
    stereo: Vec<f32>,
    reduced_db: f32,
}

impl Take {
    fn mono(&self) -> Vec<f32> {
        self.stereo.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }
}

/// `plan` played by `snd` on Part 1 in `mode`, offline at 48 kHz through
/// `Instrument`: what `loudness_test`'s scenes measure.
fn play(snd: &Sound, mode: PartMode, plan: &Plan) -> Take {
    let mut perf = Performance::new();
    let e = perf.edit(PartId::ALL[0]);
    **e.sound = snd.clone();
    e.mix.mode = plan.mode.unwrap_or(mode);
    let shared = Box::new(AudioShared::from_performance(&perf));
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let channel = shared.parts[0].mix.channel;
    let ev = |note: u8, kind| NoteEvent {
        channel,
        note: MidiNote::new(note).expect("a MIDI note"),
        kind,
    };
    let energy = |p: &[f32]| p.iter().map(|v| v * v).sum::<f32>();
    let mut t = Take {
        stereo: Vec::new(),
        reduced_db: 0.0,
    };
    let mut prev_in = 0.0f32;
    for b in 0..block(plan.secs) {
        for h in plan.hits.iter().filter(|h| block(h.off) == b) {
            inst.handle_with(ev(h.note, NoteKind::Off), &shared, &GateView::empty());
        }
        for h in plan.hits.iter().filter(|h| block(h.on) == b) {
            let v = Velocity::new(h.vel).expect("a velocity");
            inst.handle_with(ev(h.note, NoteKind::On(v)), &shared, &GateView::empty());
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &GateView::empty());
        let out = &dac.out()[0];
        // The block out against the block in before it, over the rest gain.
        if prev_in > 1e-12 {
            let gr = 20.0
                * ((energy(out) / prev_in).sqrt() / OUTPUT_GAIN)
                    .max(1e-9)
                    .log10();
            t.reduced_db = t.reduced_db.max(-gr);
        }
        prev_in = energy(&dac.input()[0]);
        t.stereo.extend_from_slice(out);
    }
    t
}

/// The Source's Sound played under `s` at 48 kHz, mono `(L + R)/2`.
pub fn render(src: &Source, s: Script) -> Result<Vec<f32>, String> {
    Ok(play(&to_sound(src)?, src.demo, &plan(s, src.home)).mono())
}

/// A gate's outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    /// Not measured against the bar, and why: the owner's ear decides.
    Exempt(String),
}

fn verdict(ok: bool) -> Verdict {
    if ok { Verdict::Pass } else { Verdict::Fail }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// The home note at v100 held 1 s, LUFS: −11 ± 2 LU, unless set by ear.
pub fn loudness(lufs: f32, by_ear: Option<&str>) -> Verdict {
    match by_ear {
        Some(why) => Verdict::Exempt(format!("level by ear: {why}")),
        None => verdict((lufs - REFERENCE_LUFS).abs() <= LOUDNESS_LU),
    }
}

/// The most gain reduction the limiter took under `script` (`None`: the
/// level render): 1 dB for a single note, 3 dB for `stab`.
pub fn limiter(reduced_db: f32, script: Option<Script>) -> Verdict {
    let bar = if matches!(script, Some(Script::Stab)) {
        LIMIT_CHORD_DB
    } else {
        LIMIT_SINGLE_DB
    };
    verdict(reduced_db <= bar)
}

/// `listen::qa`'s click ratio over the whole render (note-offs and edges
/// included), each note-on in `onsets` judged as a take's onset; finite;
/// DC under −60 dBFS. The value is the click ratio.
pub fn clicks(x: &[f32], onsets: &[usize]) -> (f32, Verdict) {
    let q = qa_takes(x, onsets);
    let mean = x.iter().sum::<f32>() / x.len().max(1) as f32;
    let dc_ok = mean.is_finite() && db(mean.abs()) < DC_DBFS;
    (
        q.click_ratio,
        verdict(q.passes() && dc_ok && q.click_ratio <= CLICK_RATIO),
    )
}

/// `x`'s power spectrum under a 4-term Blackman-Harris window (sidelobes
/// at −92 dB), with its bin width in Hz.
fn spectrum(x: &[f32], windowed: bool) -> (Vec<f64>, f64) {
    let n = x.len().max(1);
    let w = |i: usize| {
        if !windowed {
            return 1.0;
        }
        let p = std::f64::consts::TAU * i as f64 / n as f64;
        0.358_75 - 0.488_29 * p.cos() + 0.141_28 * (2.0 * p).cos() - 0.011_68 * (3.0 * p).cos()
    };
    let mut buf: Vec<Complex<f64>> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| Complex::new(v as f64 * w(i), 0.0))
        .collect();
    buf.resize(n, Complex::new(0.0, 0.0));
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let p = buf[..n / 2 + 1]
        .iter()
        .map(|c| c.norm_sqr() / (n as f64 * n as f64))
        .collect();
    (p, SR as f64 / n as f64)
}

/// A modulator's sidebands (an LFO on CUTOFF or pitch, up to 25 Hz) sit
/// this close to their harmonic: not aliases.
const SIDEBAND_HZ: f64 = 25.0;

/// Harmonic over inharmonic energy, dB, for a note at `f0`: a bin is
/// harmonic within `SIDEBAND_HZ` plus 0.3 % (5 cents of detune; at least
/// 8 bins, the window's main lobe) of a multiple of `f0 / 2`, so a sub an
/// octave down counts as harmonic. An alias folds back between them.
fn harmonic_db(x: &[f32], f0: f32) -> f32 {
    let (p, bin) = spectrum(x, true);
    let g = f0 as f64 / 2.0;
    let (mut h, mut inh) = (0.0f64, 0.0f64);
    for (k, &e) in p.iter().enumerate() {
        let f = k as f64 * bin;
        if !(20.0..=ERB_HI_HZ as f64).contains(&f) {
            continue;
        }
        let near = (f / g).round() * g;
        if near > 0.0 && (f - near).abs() <= (SIDEBAND_HZ + 0.003 * near).max(8.0 * bin) {
            h += e;
        } else {
            inh += e;
        }
    }
    (10.0 * (h.max(1e-30) / inh.max(1e-30)).log10()) as f32
}

/// For a harmonic Sound, `x` (a held note at `f0`) keeps inharmonic
/// energy 40 dB under the harmonic. Bells, vocal and MODAL are exempt
/// and marked for listening.
pub fn aliasing(x: &[f32], f0: f32, harmonic: bool) -> (f32, Verdict) {
    if !harmonic {
        return (
            f32::NAN,
            Verdict::Exempt("inharmonic: for listening".into()),
        );
    }
    let v = harmonic_db(x, f0);
    (v, verdict(v >= ALIAS_DB))
}

/// The peak, dBFS, over `TAIL_WINDOW_S` from `within_s` after the
/// note-off at sample `off`: silent (−90 dBFS) there. A render that ends
/// before the window fails.
pub fn tail(x: &[f32], off: usize, within_s: f32) -> (f32, Verdict) {
    let from = off + (within_s * SR) as usize;
    let to = from + (TAIL_WINDOW_S * SR) as usize;
    let Some(w) = x.get(from..to) else {
        return (f32::NAN, Verdict::Fail);
    };
    let peak = w.iter().fold(0f32, |m, v| m.max(v.abs()));
    let v = if peak.is_finite() { db(peak) } else { f32::NAN };
    (v, verdict(v <= TAIL_DBFS))
}

/// How long after note-off the Sound must fall silent: MODAL 10 s; any
/// other the longest release (A: its SPEED's range; B: FALL) among the
/// ENVs routed into the VCA (any slot may be, as the voice reads it),
/// ENV 1's when none is, plus 2 s.
pub fn tail_within(s: &Source, snd: &Sound) -> f32 {
    if s.engine == EngineType::Modal {
        return MODAL_TAIL_S;
    }
    let m = &snd.mod_state;
    let routed = |e: EnvSlot| {
        let i = ModSource::of_env(e).index();
        m.fixed_present(FixedCol::Vca) & (1 << i) != 0 && m.fixed_amount(i, FixedCol::Vca) != 0
    };
    let release = |e: EnvSlot| {
        let p = &snd.params.envelopes[e.index()];
        match p.env_type {
            EnvType::A => speed_ranges(p.speed).dec_rel.at(p.release),
            EnvType::B => B_TIME.at(p.func.fall),
        }
    };
    let longest = EnvSlot::ALL
        .into_iter()
        .filter(|&e| routed(e))
        .map(release)
        .reduce(f32::max)
        .unwrap_or_else(|| release(EnvSlot::Env1));
    longest + TAIL_GRACE_S
}

/// ERB-rate (Glasberg and Moore): `21.4 · log10(1 + 0.00437 f)`.
fn erb_rate(f: f32) -> f32 {
    21.4 * (1.0 + 0.004_37 * f).log10()
}

/// `x`'s energy in 32 bands equally spaced in ERB-rate from 50 Hz to
/// 20 kHz, dB (floor −120): the spectral metric `tap` and `samey` use.
pub fn erb_bands(x: &[f32]) -> [f32; ERB_BANDS] {
    let (p, bin) = spectrum(x, false);
    let (lo, hi) = (erb_rate(ERB_LO_HZ), erb_rate(ERB_HI_HZ));
    let mut e = [0f64; ERB_BANDS];
    for (k, &v) in p.iter().enumerate() {
        let f = (k as f64 * bin) as f32;
        if !(ERB_LO_HZ..ERB_HI_HZ).contains(&f) {
            continue;
        }
        let b = ((erb_rate(f) - lo) / (hi - lo) * ERB_BANDS as f32) as usize;
        e[b.min(ERB_BANDS - 1)] += v;
    }
    e.map(|v| (10.0 * v.max(1e-12).log10()) as f32)
}

/// RMS over the bands of their difference, dB.
pub fn spectral_error(a: &[f32; ERB_BANDS], b: &[f32; ERB_BANDS]) -> f32 {
    let ss: f32 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
    (ss / ERB_BANDS as f32).sqrt()
}

/// The vocal Sounds' tap must be audible: 6 dB of spectral error between
/// the render with it and without.
pub fn tap(err_db: f32) -> Verdict {
    verdict(err_db >= TAP_DB)
}

/// Every pair `(i, j)`, `i < j`, within 3 dB on all 32 bands: reported
/// for a by-ear verdict.
pub fn samey(bands: &[[f32; ERB_BANDS]]) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    for i in 0..bands.len() {
        for j in i + 1..bands.len() {
            if bands[i]
                .iter()
                .zip(&bands[j])
                .all(|(a, b)| (a - b).abs() <= SAMEY_DB)
            {
                v.push((i, j));
            }
        }
    }
    v
}

/// What a gate checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    Loudness,
    /// The level render (`None`) or a script's.
    Limiter(Option<Script>),
    Clicks(Script),
    /// At this note.
    Aliasing(u8),
    Tail,
    Tap,
}

impl Check {
    fn label(&self) -> String {
        match self {
            Check::Loudness => "loudness".into(),
            Check::Limiter(None) => "limiter level".into(),
            Check::Limiter(Some(s)) => format!("limiter {}", s.word()),
            Check::Clicks(s) => format!("clicks {}", s.word()),
            Check::Aliasing(n) => format!("aliasing C{}", n / 12 - 1),
            Check::Tail => "tail".into(),
            Check::Tap => "tap".into(),
        }
    }

    fn unit(&self) -> &'static str {
        match self {
            Check::Loudness => "LUFS",
            Check::Limiter(_) => "dB GR",
            Check::Clicks(_) => "x",
            Check::Aliasing(_) | Check::Tap => "dB",
            Check::Tail => "dBFS",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gate {
    pub check: Check,
    pub value: f32,
    pub verdict: Verdict,
}

/// One script's render, for the page.
#[derive(Clone, Debug)]
pub struct Render {
    pub script: Script,
    pub mono: Vec<f32>,
    pub lufs: f32,
    pub peak_dbfs: f32,
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
    pub bands: [f32; ERB_BANDS],
}

impl Report {
    pub fn passes(&self) -> bool {
        self.gates.iter().all(|g| g.verdict != Verdict::Fail)
    }

    fn wav(&self, s: Script) -> String {
        format!("renders/S{}-{}.wav", self.id.get(), s.word())
    }

    fn lufs(&self) -> Option<f32> {
        self.gates
            .iter()
            .find(|g| g.check == Check::Loudness)
            .map(|g| g.value)
    }
}

fn hz(n: u8) -> f32 {
    440.0 * 2f32.powf((n as f32 - 69.0) / 12.0)
}

/// Every gate on `src`, through the renders its gates and `plays` need.
pub fn gates(src: &Source) -> Result<Report, String> {
    let snd = to_sound(src)?;
    let within = tail_within(src, &snd);
    let mut gates = Vec::new();
    let mut gate = |check, (value, verdict)| {
        gates.push(Gate {
            check,
            value,
            verdict,
        })
    };

    // The level render: the home note at v100, held 1 s, then its tail.
    let level = Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, src.home, 100)],
        secs: LEVEL_HOLD_S + within + 2.0 * TAIL_WINDOW_S,
        mode: None,
    };
    let t = play(&snd, src.demo, &level);
    let held = (LEVEL_HOLD_S * SR) as usize;
    let lufs = listen::lufs(&t.stereo[2 * LATE..2 * (LATE + held)]);
    gate(
        Check::Loudness,
        (lufs, loudness(lufs, src.level_by_ear.as_deref())),
    );
    gate(
        Check::Limiter(None),
        (t.reduced_db, limiter(t.reduced_db, None)),
    );
    gate(Check::Tail, tail(&t.mono(), held + LATE, within));

    let mut renders = Vec::new();
    for &s in &src.plays {
        let p = plan(s, src.home);
        let t = play(&snd, src.demo, &p);
        let mono = t.mono();
        gate(
            Check::Limiter(Some(s)),
            (t.reduced_db, limiter(t.reduced_db, Some(s))),
        );
        let onsets: Vec<usize> = p
            .hits
            .iter()
            .map(|h| block(h.on) * BLOCK_SIZE + LATE)
            .collect();
        gate(Check::Clicks(s), clicks(&mono, &onsets));
        let peak = mono.iter().fold(0f32, |m, v| m.max(v.abs()));
        renders.push(Render {
            script: s,
            lufs: listen::lufs(&t.stereo),
            peak_dbfs: db(peak),
            mono,
        });
    }

    // `long`'s held part, past the attack.
    let held_part = |x: &[f32]| x[(0.25 * SR) as usize..(2.0 * SR) as usize].to_vec();
    let harmonic = src.engine == EngineType::Va;
    for n in ALIAS_NOTES {
        let x = if harmonic {
            held_part(&play(&snd, src.demo, &plan(Script::Long, n)).mono())
        } else {
            Vec::new()
        };
        gate(Check::Aliasing(n), aliasing(&x, hz(n), harmonic));
    }

    let long = match renders.iter().find(|r| r.script == Script::Long) {
        Some(r) => r.mono.clone(),
        None => render(src, Script::Long)?,
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
        edits: src.edits.clone(),
        private: src.from.is_some(),
        gates,
        renders,
        bands: erb_bands(&long),
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
    Ok(t)
}

/// The level render's LUFS (`fmt --level` centres it).
pub fn level_lufs(src: &Source) -> Result<f32, String> {
    let snd = to_sound(src)?;
    let level = Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, src.home, 100)],
        secs: LEVEL_HOLD_S + 0.05,
        mode: None,
    };
    let t = play(&snd, src.demo, &level);
    let held = (LEVEL_HOLD_S * SR) as usize;
    Ok(listen::lufs(&t.stereo[2 * LATE..2 * (LATE + held)]))
}

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
        for g in &r.gates {
            let v = match &g.verdict {
                Verdict::Pass => "PASS".to_string(),
                Verdict::Fail => "FAIL".to_string(),
                Verdict::Exempt(why) => format!("EXEMPT ({why})"),
            };
            let _ = writeln!(
                o,
                "  {:<18} {:>8.2} {:<6} {v}",
                g.check.label(),
                g.value,
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
    let pass = reports.iter().all(Report::passes);
    let _ = writeln!(
        o,
        "{} Sounds: {}",
        reports.len(),
        if pass { "all pass" } else { "FAIL" }
    );
    o
}

/// The owner's page: per Sound its name, description, demo mode, LUFS,
/// `edits`, exemptions and samey pairs to judge by ear, and a player per
/// render with its LUFS and peak.
pub fn page(reports: &[Report], samey: &[(usize, usize)]) -> String {
    let rows: Vec<Row> = reports
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut lines = vec![(r.note.clone(), false)];
            let lufs = r
                .lufs()
                .map_or(String::new(), |l| format!(" · {l:.1} LUFS"));
            lines.push((
                format!("S{} · {}{lufs}", r.id.get(), r.demo.disk_ident()),
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
            Row {
                title: r.name.clone(),
                lines,
                players: r
                    .renders
                    .iter()
                    .map(|x| Player {
                        label: x.script.word().to_uppercase(),
                        file: r.wav(x.script),
                        info: format!("{:.1} LUFS · peak {:.1} dBFS", x.lufs, x.peak_dbfs),
                    })
                    .collect(),
                private: r.private,
            }
        })
        .collect();
    listen::rows_page(
        "Factory Sounds · listening",
        "Each Sound under its render scripts, played offline through the real Instrument \
         (Part 1, P1, FX off). Every gate passed; lines in colour are for your ear.",
        &rows,
    )
}

/// Into `dir` (checked as a ledger): `report.txt`; with `wav`, each render
/// as `renders/S<id>-<script>.wav` and, when every Sound passes,
/// `listening.html`. A failing run removes an old page. True when every
/// Sound passes.
pub fn write(
    dir: &Path,
    reports: &[Report],
    samey: &[(usize, usize)],
    wav: bool,
) -> Result<bool, String> {
    let dir = checked_ledger(dir)?;
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
    let pass = reports.iter().all(Report::passes);
    let txt = dir.join("report.txt");
    std::fs::write(&txt, report_text(reports, samey)).map_err(|e| err(&txt, e))?;
    if wav {
        let r = dir.join("renders");
        std::fs::create_dir_all(&r).map_err(|e| err(&r, e))?;
        for rep in reports {
            for x in &rep.renders {
                listen::write_wav(&dir.join(rep.wav(x.script)), &x.mono)?;
            }
        }
    }
    let html = dir.join("listening.html");
    if !pass {
        match std::fs::remove_file(&html) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(&html, e)),
            _ => {}
        }
    } else if wav {
        std::fs::write(&html, page(reports, samey)).map_err(|e| err(&html, e))?;
    }
    Ok(pass)
}
