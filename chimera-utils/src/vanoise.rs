// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! VA NOISE: Sounds and event scripts through the real `Instrument` (Part
//! 1, sends 0), mono (L+R)/2 after the limiter, raw. `vanoise hash SOUND
//! EVENTS` prints each MODE × script's SHA-256 (the bit-identity check
//! against main); `vanoise check` the A/B method's pipeline pair.

use crate::unitab::{Ev, Script, load_sound, script};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::dsp::va::{NoiseKind, VaMode};
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::Loads;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::tracked::Tracked;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use sha2::Digest;
use std::path::Path;

const SR: f64 = SAMPLE_RATE as f64;
/// Lead-in before the first event.
const PRE_S: f64 = 0.05;
pub const SCRIPTS: [&str; 2] = ["bass", "stab"];

/// `snd` playing `sc`; `tweak(t, sound)` runs every block (t from the first
/// event) and returns whether it changed the Sound, which is then published
/// as the UI publishes a knob's turn.
pub fn engine(snd: &Sound, sc: &Script, tweak: &dyn Fn(f64, &mut Sound) -> bool) -> Vec<f32> {
    let mut s = snd.clone();
    s.params.out.sends = [chimera_core::dsp::SendLevel::OFF; FX_SENDS];
    tweak(-PRE_S, &mut s);
    let mut sound = Tracked::fresh(s);
    let loads = Loads::NONE.next();
    let mut pubr = KnobPublisher::new();
    let mut shared = Box::<AudioShared>::default();
    shared.publish_sound(0, &sound, loads, &mut pubr);
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let gv = GateView::empty();
    let ch = MidiChannel::clamped(0);
    let ev = |n: u8, kind| NoteEvent {
        channel: ch,
        note: MidiNote::new(n).unwrap(),
        kind,
    };
    let end = PRE_S + sc.ev.last().map_or(0.0, |e| e.0) + sc.tail;
    let blocks = (end * SR) as usize / BLOCK_SIZE;
    let (mut out, mut next) = (Vec::with_capacity(blocks * BLOCK_SIZE), 0);
    for b in 0..blocks {
        let t = (b * BLOCK_SIZE) as f64 / SR - PRE_S;
        while next < sc.ev.len() && sc.ev[next].0 <= t {
            match sc.ev[next].1 {
                Ev::On(n, v) => {
                    inst.handle_with(ev(n, NoteKind::On(Velocity::new(v).unwrap())), &shared, &gv)
                }
                Ev::Off(n) => inst.handle_with(ev(n, NoteKind::Off), &shared, &gv),
                Ev::Cc(..) => {}
            }
            next += 1;
        }
        if tweak(t, &mut sound) {
            shared.publish_sound(0, &sound, loads, &mut pubr);
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &gv);
        out.extend(dac.out()[0].chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    out
}

fn sounds(owner: &str) -> Result<[(&'static str, Sound); 2], String> {
    Ok([
        ("owner", load_sound(Path::new(owner))?),
        ("init", Sound::init(EngineType::Va)),
    ])
}

/// `vanoise hash OWNER.A EVENTS`: every MODE × script × Sound, NOISE untouched.
pub fn hash(args: &[String]) -> Result<(), String> {
    let [owner, events] = args else {
        return Err("vanoise hash OWNER.A EVENTS".into());
    };
    for (name, snd) in sounds(owner)? {
        for s in SCRIPTS {
            let sc = script(Path::new(events), s)?;
            for m in VaMode::ALL {
                let y = engine(&snd, &sc, &|t, x| {
                    if t < -PRE_S + 1e-9 {
                        x.params.va.mode = m;
                    }
                    false
                });
                let mut h = sha2::Sha256::new();
                for v in &y {
                    h.update(v.to_le_bytes());
                }
                let rms =
                    (y.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / y.len() as f64).sqrt();
                println!("{name} {s} {m:?} rms {rms:.5} {:x}", h.finalize());
            }
        }
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.split_first() {
        Some((c, rest)) if c == "hash" => hash(rest),
        Some((c, [p])) if c == "show" => show(p),
        Some((c, rest)) if c == "check" => check(rest),
        Some((c, rest)) if c == "render" => render(rest),
        _ => Err("vanoise hash|show|check|render …".into()),
    }
}

/// `vanoise show SOUND`: the VA and filter params.
pub fn show(path: &str) -> Result<(), String> {
    let s = load_sound(Path::new(path))?;
    println!(
        "{:?} {:?}\n{:#?}\n{:?}\nfx {:?}\nenv {:?}\nmod {:?}",
        s.name,
        s.engine(),
        s.params.va,
        s.params.filter,
        s.params.fx,
        s.params.envelopes,
        s.mod_state
    );
    Ok(())
}

/// `vanoise check SOUND EVENTS OUTDIR`: the A/B method's pipeline check
/// through this page's `engine` and writer: 2040 REV2 and EXPNDR at RES 64
/// and 114, CUTOFF 1.20 kHz, on the bass script (`unitab cmp` the files).
pub fn check(args: &[String]) -> Result<(), String> {
    use chimera_core::dsp::filter::K2040Voice;
    let [snd, events, dir] = args else {
        return Err("vanoise check SOUND EVENTS OUTDIR".into());
    };
    let snd = load_sound(Path::new(snd))?;
    let sc = script(Path::new(events), "bass")?;
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for r in [64u8, 114] {
        for (tag, v) in [("rev2", K2040Voice::Rev2), ("exp", K2040Voice::Exp)] {
            let y = engine(&snd, &sc, &|t, s| {
                if t < -PRE_S + 1e-9 {
                    s.params.filter.voice = v;
                    s.params.filter.resonance = f32::from(r) / 127.0;
                    s.params.filter.cutoff = 1200.0;
                }
                false
            });
            crate::reference::write_f32(
                &dir.join(format!("check_r{r}_{tag}.wav")),
                &y,
                SAMPLE_RATE,
            )?;
        }
    }
    Ok(())
}

/// One render of the page's set.
#[derive(Clone)]
pub struct Job {
    pub snd: usize,
    pub open: bool,
    pub script: &'static str,
    pub kind: NoiseKind,
    pub level: f32,
    /// LEVEL 0 → 1 over 4 s.
    pub sweep: bool,
    pub name: String,
}

/// The LEVELs each KIND plays at.
pub const LEVELS: [f32; 3] = [0.4, 0.7, 1.0];

fn kind_tag(k: NoiseKind) -> String {
    format!("{k:?}").to_lowercase()
}

pub fn jobs() -> Vec<Job> {
    let mut v = Vec::new();
    let mut push = |snd: usize, open: bool, script, kind, level: f32, sweep| {
        let f = if open { "open" } else { "flt" };
        let tag = if sweep {
            "sweep".into()
        } else {
            format!("l{:03}", (level * 100.0).round() as u32)
        };
        let k = if level == 0.0 && !sweep {
            "none".into()
        } else {
            kind_tag(kind)
        };
        let name = format!("{}_{f}_{script}_{k}_{tag}", ["owner", "init"][snd]);
        v.push(Job {
            snd,
            open,
            script,
            kind,
            level,
            sweep,
            name,
        });
    };
    // INIT VA's own 2040 is open at its defaults: one set
    for (snd, script, opens) in [(0, "bass", &[false, true][..]), (1, "stab", &[false][..])] {
        for &open in opens {
            push(snd, open, script, NoiseKind::White, 0.0, false);
            for kind in NoiseKind::ALL {
                for l in LEVELS {
                    push(snd, open, script, kind, l, false);
                }
            }
        }
    }
    for snd in [0, 1] {
        for kind in NoiseKind::ALL {
            push(snd, true, "held", kind, 0.0, true);
        }
        // the reference Sound sweeps through its own 2040 too
        if snd == 0 {
            for kind in NoiseKind::ALL {
                push(snd, false, "held", kind, 0.0, true);
            }
        }
        push(snd, true, "held", NoiseKind::White, 0.0, false);
        for kind in NoiseKind::ALL {
            push(snd, true, "held", kind, 1.0, false);
        }
    }
    v
}

fn apply(j: &Job, s: &mut Sound) {
    s.params.va.noise_kind = j.kind;
    s.params.va.noise = j.level;
    if j.open {
        s.params.filter.cutoff = 20_000.0;
        s.params.filter.resonance = 0.0;
    }
}

/// `vanoise render OWNER.A EVENTS OUTDIR`: the page's set, 32-bit float,
/// unscaled; then `levels.tsv`.
pub fn render(args: &[String]) -> Result<(), String> {
    let [owner, events, dir] = args else {
        return Err("vanoise render OWNER.A EVENTS OUTDIR".into());
    };
    let snds = sounds(owner)?;
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let jobs = jobs();
    use rayon::prelude::*;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .map_err(|e| e.to_string())?;
    let rows: Vec<String> = pool.install(|| {
        jobs.par_iter()
            .map(|j| -> Result<String, String> {
                let sc = script(Path::new(events), j.script)?;
                let y = engine(&snds[j.snd].1, &sc, &|t, s| {
                    if t < -PRE_S + 1e-9 {
                        apply(j, s);
                        return false;
                    }
                    if !j.sweep {
                        return false;
                    }
                    s.params.va.noise = (t / 4.0).clamp(0.0, 1.0) as f32;
                    true
                });
                crate::reference::write_f32(&dir.join(format!("{}.wav", j.name)), &y, SAMPLE_RATE)?;
                let m = measure(&y);
                Ok(format!(
                    "{}\t{:.2}\t{:.2}\t{:.0}\t{:.1}\t{:.3}",
                    j.name, m.0, m.1, m.2, m.3, m.4
                ))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    let mut tsv = String::from("name\tloud_db\tlufs\tcentroid_hz\thf2k_db\tpeak\n");
    for r in rows {
        tsv.push_str(&r);
        tsv.push('\n');
    }
    std::fs::write(dir.join("levels.tsv"), tsv).map_err(|e| e.to_string())
}

/// Loud-half RMS (dB), gated LUFS, spectral centroid (Hz), the power share
/// above 2 kHz (dB) and the peak.
pub fn measure(y: &[f32]) -> (f64, f64, f64, f64, f64) {
    let x: Vec<f64> = y.iter().map(|v| f64::from(*v)).collect();
    let mut e: Vec<f64> = x
        .chunks(4800)
        .map(|c| c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64)
        .collect();
    e.sort_by(|a, b| b.total_cmp(a));
    let h = &e[..(e.len() / 2).max(1)];
    let loud = 10.0 * (h.iter().sum::<f64>() / h.len() as f64).max(1e-30).log10();
    let lufs = crate::voice::lufs_gated(std::slice::from_ref(&x));
    let (f, p) = crate::sig::welch(&x, SR, 8192);
    let tot: f64 = p.iter().sum();
    let cen = f.iter().zip(&p).map(|(f, p)| f * p).sum::<f64>() / tot.max(1e-30);
    let hf = f
        .iter()
        .zip(&p)
        .filter(|(f, _)| **f >= 2000.0)
        .map(|(_, p)| p)
        .sum::<f64>()
        / tot.max(1e-30);
    let peak = x.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    (loud, lufs, cen, 10.0 * hf.max(1e-30).log10(), peak)
}
