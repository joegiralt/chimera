//! Filter A/B against the unit: one Sound file (a library `get`), one
//! event script (notes and CCs, the unit's `take.py` plays the same list),
//! rendered through the real `Instrument` at each VOICE; the lab harness's
//! filter-alone render beside it; and the measures the unit's recordings
//! get. Levels are raw: nothing is matched before the level and low-band
//! columns, so a flat gain between voicings shows.

use chimera_core::dsp::filter::{Filter, FilterKind, FilterMods, K2040Voice};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::project::Loads;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::storage::{Check, Decode, Framer, SoundDecoder};
use chimera_core::tracked::Tracked;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::fmt::Write as _;
use std::path::Path;

const SR: f64 = SAMPLE_RATE as f64;
pub const SCRIPTS: [&str; 4] = ["acid", "bass", "sweep", "orbit"];
pub const RES_CC: [u8; 4] = [19, 64, 95, 114];
const PRE_S: f64 = 1.0;

#[derive(Clone, Copy)]
pub enum Ev {
    On(u8, u8),
    Off(u8),
    Cc(u8, u8),
}

pub struct Script {
    pub ev: Vec<(f64, Ev)>,
    pub tail: f64,
}

pub fn script(events: &Path, name: &str) -> Result<Script, String> {
    let txt = std::fs::read_to_string(events).map_err(|e| format!("{}: {e}", events.display()))?;
    let v: serde_json::Value = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
    let s = &v[name];
    let ev = s["events"]
        .as_array()
        .ok_or(format!("no script {name}"))?
        .iter()
        .map(|e| {
            let (t, k) = (e[0].as_f64().unwrap(), e[1].as_str().unwrap());
            let (a, b) = (e[2].as_u64().unwrap() as u8, e[3].as_u64().unwrap() as u8);
            let ev = match k {
                "on" => Ev::On(a, b),
                "off" => Ev::Off(a),
                _ => Ev::Cc(a, b),
            };
            (t, ev)
        })
        .collect();
    Ok(Script { ev, tail: s["tail"].as_f64().unwrap_or(1.0) })
}

/// A Sound file's bytes (a library `get`) decoded as the firmware does.
pub fn load_sound(path: &Path) -> Result<Sound, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut s = Sound::neutral(EngineType::Algo);
    for apply in [false, true] {
        let mut d = SoundDecoder::new(&mut s);
        let mut f = Framer::new(bytes.len() as u32).map_err(|e| format!("{e:?}"))?;
        f.push(&bytes, &mut |e| if apply { d.apply(e) } else { d.event(e) })
            .map_err(|e| format!("{e:?}"))?;
        f.finish().map_err(|e| format!("{e:?}"))?;
        if apply { d.commit() } else { d.end() }.map_err(|e| format!("{e:?}"))?;
    }
    Ok(s)
}

/// CC74's place as the MIDI map lands it (0–255 span): CUTOFF's law.
fn cutoff_of(v: u8) -> f32 {
    20.0 * 2f32.powf(f32::from(v) / 127.0 * 9.965_784)
}

fn res_of(v: u8) -> f32 {
    f32::from(v) / 127.0
}

fn voice_of(name: &str) -> K2040Voice {
    if name == "exp" { K2040Voice::Exp } else { K2040Voice::Rev2 }
}

/// The script on `snd` at `voice` through `Instrument` (Part 1, sends 0),
/// CC71 at `res_cc` from the start, CCs published as the UI does (one
/// publish per change, the audio's knob ramps): mono (L+R)/2 after the
/// limiter, `PRE_S` of lead-in as the unit's take has.
pub fn engine(snd: &Sound, voice: K2040Voice, sc: &Script, res_cc: u8) -> Vec<f32> {
    let mut s = snd.clone();
    s.params.filter.voice = voice;
    s.params.filter.resonance = res_of(res_cc);
    s.params.out.sends = [0.0; FX_SENDS];
    let mut sound = Tracked::fresh(s);
    let loads = Loads::NONE.next();
    let mut pubr = KnobPublisher::new();
    let mut shared = Box::<AudioShared>::default();
    shared.publish_sound(0, &sound, loads, &mut pubr);
    let mut inst = Box::new(Instrument::new(SAMPLE_RATE, SampleBudget::for_cpu(CPU_HZ_REV_V)));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let gv = GateView::empty();
    let ch = MidiChannel::clamped(0);
    let ev = |n: u8, kind| NoteEvent { channel: ch, note: MidiNote::new(n).unwrap(), kind };
    let end = PRE_S + sc.ev.last().map_or(0.0, |e| e.0) + sc.tail;
    let blocks = (end * SR) as usize / BLOCK_SIZE;
    let (mut out, mut next) = (Vec::with_capacity(blocks * BLOCK_SIZE), 0);
    for b in 0..blocks {
        let t = (b * BLOCK_SIZE) as f64 / SR - PRE_S;
        let mut moved = false;
        while next < sc.ev.len() && sc.ev[next].0 <= t {
            match sc.ev[next].1 {
                Ev::On(n, v) => inst.handle_with(ev(n, NoteKind::On(Velocity::new(v).unwrap())), &shared, &gv),
                Ev::Off(n) => inst.handle_with(ev(n, NoteKind::Off), &shared, &gv),
                Ev::Cc(74, v) => {
                    sound.params.filter.cutoff = cutoff_of(v);
                    moved = true;
                }
                Ev::Cc(71, v) => {
                    sound.params.filter.resonance = res_of(v);
                    moved = true;
                }
                Ev::Cc(..) => {}
            }
            next += 1;
        }
        if moved {
            shared.publish_sound(0, &sound, loads, &mut pubr);
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &gv);
        out.extend(dac.out()[0].chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    out
}

/// The lab harness's way (`kobol::ours`): a plain saw at ½ into the 2040
/// alone at LEVEL ½, the same notes, CUTOFF and RES, no envelopes, VCA,
/// voice mixer or limiter. Gate by note: silence between notes.
pub fn lab(voice: K2040Voice, sc: &Script, res_cc: u8) -> Vec<f32> {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::K2040);
    p.voice = voice;
    p.level = 0.5;
    p.resonance = res_of(res_cc);
    let mut f = Filter::new();
    let end = PRE_S + sc.ev.last().map_or(0.0, |e| e.0) + sc.tail;
    let blocks = (end * SR) as usize / BLOCK_SIZE;
    let (mut out, mut next, mut note, mut ph) = (Vec::new(), 0, None::<u8>, 0.0f64);
    for b in 0..blocks {
        let t = (b * BLOCK_SIZE) as f64 / SR - PRE_S;
        while next < sc.ev.len() && sc.ev[next].0 <= t {
            match sc.ev[next].1 {
                Ev::On(n, _) => note = Some(n),
                Ev::Off(n) if note == Some(n) => note = None,
                Ev::Cc(74, v) => p.cutoff = cutoff_of(v),
                Ev::Cc(71, v) => p.resonance = res_of(v),
                _ => {}
            }
            next += 1;
        }
        let mut blk = [0.0f32; BLOCK_SIZE];
        if let Some(n) = note {
            let hz = 440.0 * 2f64.powf((f64::from(n) - 69.0) / 12.0);
            for s in blk.iter_mut() {
                *s = (0.5 * (2.0 * ph - 1.0)) as f32;
                ph = (ph + hz / SR).fract();
            }
        }
        f.process(&mut blk, &p, SAMPLE_RATE, &FilterMods::NONE);
        out.extend_from_slice(&blk);
    }
    out
}

/// `unitab render SOUND EVENTS OUTDIR`: eng_* and lab_* WAVs, every
/// script × RES × voice, raw (32-bit float, no gain).
pub fn render(args: &[String]) -> Result<(), String> {
    let [snd, events, dir] = args else { return Err("unitab render SOUND EVENTS OUTDIR".into()) };
    let snd = load_sound(Path::new(snd))?;
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let f = &snd.params.filter;
    println!(
        "{:?}: engine {:?}, filter {:?} {:?} cutoff {:.0} res {:.3} level {:.3} sends {:?}",
        snd.name, snd.engine(), f.kind(), f.voice, f.cutoff, f.resonance, f.level, snd.params.out.sends
    );
    let jobs: Vec<(&str, u8, &str)> = SCRIPTS
        .iter()
        .flat_map(|s| RES_CC.iter().flat_map(move |&r| ["rev2", "exp"].map(|v| (*s, r, v))))
        .collect();
    use rayon::prelude::*;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build().map_err(|e| e.to_string())?;
    pool.install(|| {
        jobs.par_iter().try_for_each(|&(s, r, v)| -> Result<(), String> {
            let sc = script(Path::new(events), s)?;
            let tag = format!("{s}_r{r}_{v}.wav");
            crate::reference::write_f32(&dir.join(format!("eng_{tag}")), &engine(&snd, voice_of(v), &sc, r), SAMPLE_RATE)?;
            crate::reference::write_f32(&dir.join(format!("lab_{tag}")), &lab(voice_of(v), &sc, r), SAMPLE_RATE)
        })
    })
}

// ---- measures ----

fn db(p: f64) -> f64 {
    10.0 * p.max(1e-30).log10()
}

/// A take as mono: a 20-channel MOTU file's inputs 5–6 averaged, else
/// every channel averaged.
pub fn take(path: &Path) -> Result<Vec<f64>, String> {
    let (ch, _) = crate::reference::read_channels(path)?;
    let c0: usize = std::env::var("UNIT_CH").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    Ok(if ch.len() == 20 {
        ch[c0].iter().zip(&ch[c0 + 1]).map(|(a, b)| 0.5 * (a + b)).collect()
    } else {
        let n = ch.len() as f64;
        (0..ch[0].len()).map(|i| ch.iter().map(|c| c[i]).sum::<f64>() / n).collect()
    })
}

/// Mean power over the louder half of 0.1 s frames (dB).
fn loud(x: &[f64]) -> f64 {
    let mut e: Vec<f64> = x.chunks(4800).map(|c| c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).collect();
    e.sort_by(|a, b| b.total_cmp(a));
    let h = &e[..(e.len() / 2).max(1)];
    db(h.iter().sum::<f64>() / h.len() as f64)
}

/// Third-octave band powers (dB), 25 Hz – 16 kHz, from a Welch PSD.
pub const BANDS: usize = 29;
fn thirds(x: &[f64]) -> Vec<f64> {
    let (f, p) = crate::sig::welch(x, SR, 8192);
    (0..BANDS)
        .map(|i| {
            let fc = 25.0 * 2f64.powf(i as f64 / 3.0);
            let (lo, hi) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
            db(f.iter().zip(&p).filter(|(f, _)| **f >= lo && **f < hi).map(|(_, p)| *p).sum::<f64>())
        })
        .collect()
}

/// Power below 200 Hz (dB), from the same PSD.
fn low(x: &[f64]) -> f64 {
    let (f, p) = crate::sig::welch(x, SR, 8192);
    db(f.iter().zip(&p).filter(|(f, _)| **f >= 20.0 && **f < 200.0).map(|(_, p)| *p).sum::<f64>())
}

/// Residual of `b` against `a` after the best lag (±60 ms) and least-squares
/// gain, dB re `a`: the null.
fn null(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let m = (2 * n).next_power_of_two();
    let pad = |x: &[f64]| {
        let mut v = x[..n].to_vec();
        v.resize(m, 0.0);
        crate::sig::fft(&v)
    };
    let (fa, fb) = (pad(a), pad(b));
    let mut c: Vec<_> = fa.iter().zip(&fb).map(|(x, y)| x * y.conj()).collect();
    crate::sig::plan(m, true).process(&mut c);
    let w = (0.06 * SR) as usize;
    let lag = (0..=w)
        .chain(m - w..m)
        .max_by(|&i, &j| c[i].re.total_cmp(&c[j].re))
        .unwrap();
    let sh = |i: usize| -> f64 {
        // b shifted by lag: a[i] ~ b[i - lag]
        let j = if lag <= w { i as isize - lag as isize } else { i as isize + (m - lag) as isize };
        if j >= 0 && (j as usize) < n { b[j as usize] } else { 0.0 }
    };
    let (mut ab, mut bb, mut aa) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let y = sh(i);
        ab += a[i] * y;
        bb += y * y;
        aa += a[i] * a[i];
    }
    let g = ab / bb.max(1e-30);
    let r: f64 = (0..n).map(|i| (a[i] - g * sh(i)).powi(2)).sum();
    db(r / aa.max(1e-30))
}

pub struct Pair {
    pub d_level: f64,
    pub d_low: f64,
    pub tilt: f64,
    pub lsd: f64,
    pub null: f64,
    pub bands: Vec<f64>,
}

/// B (EXP) against A (REV2): raw level and low-band differences, the
/// low band's after level matching (tilt), third-octave LSD after
/// matching (bands within 50 dB of A's top), the null.
pub fn pair(a: &[f64], b: &[f64]) -> Pair {
    let d_level = loud(b) - loud(a);
    let d_low = low(b) - low(a);
    let (ta, tb) = (thirds(a), thirds(b));
    let top = ta.iter().cloned().fold(f64::MIN, f64::max);
    let bands: Vec<f64> = ta.iter().zip(&tb).map(|(x, y)| y - x).collect();
    let keep: Vec<f64> = ta.iter().zip(&bands).filter(|(x, _)| **x > top - 50.0).map(|(_, d)| d - d_level).collect();
    let lsd = keep.iter().map(|d| d.abs()).sum::<f64>() / keep.len().max(1) as f64;
    Pair { d_level, d_low, tilt: d_low - d_level, lsd, null: null(a, b), bands }
}

/// `unitab table UNITDIR RENDERDIR OUT.md`: per script × RES, EXP − REV2
/// on the unit (`unit/<s>_r<r>_<v>.wav`), the engine and the lab renders;
/// the unit's own repeat (`<s>_r<r>_rev2b.wav`) where there is one.
pub fn table(args: &[String]) -> Result<(), String> {
    let [unit, rend, out] = args else { return Err("unitab table UNITDIR RENDERDIR OUT.md".into()) };
    let (unit, rend) = (Path::new(unit), Path::new(rend));
    let mut md = String::from(
        "EXP − REV2, raw (no matching) except LSD and null. Δlevel: loud-half RMS; Δlow: power < 200 Hz; tilt: Δlow − Δlevel; LSD: third-octave |Δ| after matching, mean over bands within 50 dB of the top; null: after best lag and gain.\n\n\
         | take | source | Δlevel dB | Δlow dB | tilt dB | LSD dB | null dB |\n|---|---|---|---|---|---|---|\n",
    );
    let mut bands = String::from("\n## Third-octave Δ (EXP − REV2, dB, raw), 25 Hz … 16 kHz\n\n");
    for s in SCRIPTS {
        for r in RES_CC {
            let mut row = |src: &str, a: &Path, b: &Path| -> Result<(), String> {
                if !a.exists() || !b.exists() {
                    return Ok(());
                }
                let p = pair(&take(a)?, &take(b)?);
                let _ = writeln!(
                    md,
                    "| {s} RES {r} | {src} | {:+.1} | {:+.1} | {:+.1} | {:.2} | {:.1} |",
                    p.d_level, p.d_low, p.tilt, p.lsd, p.null
                );
                let _ = writeln!(
                    bands,
                    "- {s} RES {r} {src}: {}",
                    p.bands.iter().map(|d| format!("{d:+.1}")).collect::<Vec<_>>().join(" ")
                );
                Ok(())
            };
            row("unit", &unit.join(format!("{s}_r{r}_rev2.wav")), &unit.join(format!("{s}_r{r}_exp.wav")))?;
            row("unit REV2 repeat", &unit.join(format!("{s}_r{r}_rev2.wav")), &unit.join(format!("{s}_r{r}_rev2b.wav")))?;
            row("engine", &rend.join(format!("eng_{s}_r{r}_rev2.wav")), &rend.join(format!("eng_{s}_r{r}_exp.wav")))?;
            row("lab", &rend.join(format!("lab_{s}_r{r}_rev2.wav")), &rend.join(format!("lab_{s}_r{r}_exp.wav")))?;
        }
    }
    md.push_str(&bands);
    std::fs::write(out, &md).map_err(|e| e.to_string())?;
    print!("{md}");
    Ok(())
}

/// `unitab cmp A B [--match]`: one pair; `--match` RMS-matches B to A
/// first, as the A/B pages did.
pub fn cmp(args: &[String]) -> Result<(), String> {
    let (a, b) = (take(Path::new(&args[0]))?, take(Path::new(&args[1]))?);
    let p = pair(&a, &b);
    println!(
        "Δlevel {:+.1} Δlow {:+.1} tilt {:+.1} LSD {:.2} null {:.1}\nbands {}",
        p.d_level,
        p.d_low,
        p.tilt,
        p.lsd,
        p.null,
        p.bands.iter().map(|d| format!("{d:+.1}")).collect::<Vec<_>>().join(" ")
    );
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.split_first() {
        Some((c, rest)) if c == "render" => render(rest),
        Some((c, rest)) if c == "table" => table(rest),
        Some((c, rest)) if c == "cmp" && rest.len() == 2 => cmp(rest),
        _ => Err("unitab render|table|cmp …".into()),
    }
}
