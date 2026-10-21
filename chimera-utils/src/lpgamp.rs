//! `lpg-amp render OUTDIR SOUND EVENTS`: the VCA's low-pass gate (ADR 0195)
//! on its listening set, through the real `Instrument` (Part 1, sends 0,
//! after the limiter, mono (L+R)/2), raw: 32-bit float, no gain, never
//! normalised (filters-last/ab-method.md). Then the method's mandatory
//! check on the same pipeline and writer: the 2040's REV2 against EXPNDR on
//! SOUND (`unitab::engine`), its Δlevel and third-octave Δ printed.

use crate::unitab::{self, Script};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{FilterKind, K2040Voice};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::Loads;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::tracked::Tracked;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::path::Path;

const SR: f64 = SAMPLE_RATE as f64;

/// A phrase: (seconds, note, velocity, length in seconds), then a tail.
struct Phrase {
    notes: Vec<(f64, u8, u8, f64)>,
    tail: f64,
}

/// `notes` in 8ths at 120 BPM, each held `len`, twice round.
fn eighths(notes: &[(u8, u8)], len: f64) -> Phrase {
    let n = notes.len();
    Phrase {
        notes: (0..2 * n)
            .map(|i| (i as f64 * 0.25, notes[i % n].0, notes[i % n].1, len))
            .collect(),
        tail: 2.0,
    }
}

/// `p` on `snd` through `Instrument`, as `unitab::engine` plays a script.
fn engine(snd: &Sound, p: &Phrase) -> Vec<f32> {
    let mut s = snd.clone();
    s.params.out.sends = [0.0; FX_SENDS];
    let sound = Tracked::fresh(s);
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
    let ev = |n: u8, kind| NoteEvent {
        channel: MidiChannel::clamped(0),
        note: MidiNote::new(n).unwrap(),
        kind,
    };
    let mut evs: Vec<(f64, u8, Option<u8>)> = p
        .notes
        .iter()
        .flat_map(|&(t, n, v, l)| [(t, n, Some(v)), (t + l, n, None)])
        .collect();
    evs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.is_some().cmp(&b.2.is_some())));
    let end = evs.last().map_or(0.0, |e| e.0) + p.tail;
    let blocks = (end * SR) as usize / BLOCK_SIZE;
    let (mut out, mut next) = (Vec::with_capacity(blocks * BLOCK_SIZE), 0);
    for b in 0..blocks {
        let t = (b * BLOCK_SIZE) as f64 / SR;
        while next < evs.len() && evs[next].0 <= t {
            let (_, n, v) = evs[next];
            let kind = match v {
                Some(v) => NoteKind::On(Velocity::new(v).unwrap()),
                None => NoteKind::Off,
            };
            inst.handle_with(ev(n, kind), &shared, &gv);
            next += 1;
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &gv);
        out.extend(dac.out()[0].chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    out
}

fn gate(mut s: Sound, lpg: f32, decay: f32, strike: f32) -> Sound {
    s.params.out.gate = [lpg, decay, strike];
    s
}

/// The bongo: a sine folded before the filter on ALGO, no route into VCA,
/// STRIKE 1 at LPG 1.
fn bongo() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = AlgoParams::single(WaveId::W1);
    s.params.fx.set_type(FxType::Fold);
    s.params.fx.pots = [0.55, 0.5, 0.5];
    s.params.fx.mix = 1.0;
    gate(s, 1.0, 0.35, 1.0)
}

/// VA INIT's saw as a pluck: ENV 1 (into VCA, INIT's) at A 0, D 0.35, S 0,
/// R 0.35.
fn pluck() -> Sound {
    let mut s = Sound::init(EngineType::Va);
    let e = &mut s.params.envelopes[0];
    (e.attack, e.decay, e.sustain, e.release) = (0.0, 0.45, 0.0, 0.45);
    assert!(s.mod_state.fixed_present(FixedCol::Vca) & (1 << ModSource::Env1.index()) != 0);
    s
}

fn sem(mut s: Sound) -> Sound {
    s.params.filter.set_kind(FilterKind::Sem);
    (s.params.filter.cutoff, s.params.filter.resonance) = (1_800.0, 0.55);
    s
}

pub fn render(args: &[String]) -> Result<(), String> {
    let [dir, snd, events] = args else {
        return Err("lpg-amp render OUTDIR SOUND EVENTS".into());
    };
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let drum = drum();
    let bass = eighths(
        &[
            (36, 110),
            (36, 80),
            (48, 100),
            (36, 70),
            (39, 105),
            (36, 80),
            (43, 115),
            (46, 90),
        ],
        0.18,
    );
    let mut takes: Vec<(String, Sound, &Phrase)> = vec![("bongo".into(), bongo(), &drum)];
    for (tag, a) in [("0", 0.0), ("half", 0.5), ("1", 1.0)] {
        takes.push((
            format!("pluck_lpg_{tag}"),
            gate(pluck(), a, 0.0, 0.0),
            &bass,
        ));
    }
    for (tag, a) in [("0", 0.0), ("1", 1.0)] {
        takes.push((
            format!("sem_lpg_{tag}"),
            gate(sem(pluck()), a, 0.4, 0.0),
            &bass,
        ));
    }
    for (tag, d) in [("0", 0.0), ("half", 0.5), ("1", 1.0)] {
        takes.push((format!("decay_{tag}"), gate(pluck(), 0.0, d, 0.0), &bass));
    }
    write(dir, &takes)?;
    check(dir, snd, events)
}

/// `lpg-amp strike OUTDIR SOUND EVENTS`: STRIKE on a held saw chord at 0,
/// ½ and 1, and the bongo; then the method's check.
pub fn strike(args: &[String]) -> Result<(), String> {
    let [dir, snd, events] = args else {
        return Err("lpg-amp strike OUTDIR SOUND EVENTS".into());
    };
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let drum = drum();
    // Two chords held 1.5 s, struck at 110 then 70.
    let chord = |at: f64, v: u8| [48, 55, 60, 64].map(|n| (at, n, v, 1.5));
    let chords = Phrase {
        notes: chord(0.0, 110).into_iter().chain(chord(2.0, 70)).collect(),
        tail: 2.0,
    };
    let mut takes: Vec<(String, Sound, &Phrase)> = vec![("bongo".into(), bongo(), &drum)];
    for (tag, k) in [("0", 0.0), ("half", 0.5), ("1", 1.0)] {
        takes.push((format!("chord_strike_{tag}"), gate(held(), 0.5, 0.4, k), &chords));
    }
    write(dir, &takes)?;
    check(dir, snd, events)
}

/// VA INIT's saw held: ENV 1 (into VCA) at A 0, D 0.3, S 0.6, R 0.4.
fn held() -> Sound {
    let mut s = pluck();
    let e = &mut s.params.envelopes[0];
    (e.decay, e.sustain, e.release) = (0.3, 0.6, 0.4);
    s
}

fn drum() -> Phrase {
    eighths(
        &[
            (48, 112),
            (55, 70),
            (48, 92),
            (52, 60),
            (55, 104),
            (48, 72),
            (60, 124),
            (55, 84),
        ],
        0.08,
    )
}

fn write(dir: &Path, takes: &[(String, Sound, &Phrase)]) -> Result<(), String> {
    for (name, s, p) in takes {
        let y = engine(s, p);
        let pk = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (y.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / y.len() as f64).sqrt();
        println!(
            "{name}: {:.2} s, peak {:.1} dBFS, RMS {:.1} dBFS",
            y.len() as f64 / SR,
            20.0 * f64::from(pk).log10(),
            20.0 * rms.log10()
        );
        crate::reference::write_f32(&dir.join(format!("{name}.wav")), &y, SAMPLE_RATE)?;
    }
    Ok(())
}

fn check(dir: &Path, snd: &str, events: &str) -> Result<(), String> {
    // The method's mandatory check: REV2 against EXPNDR on SOUND, through
    // this file's own writer, measured off the files.
    let snd = unitab::load_sound(Path::new(snd))?;
    let sc: Script = unitab::script(Path::new(events), "bass")?;
    for r in [64u8, 114] {
        let mut files = Vec::new();
        for (v, tag) in [(K2040Voice::Rev2, "rev2"), (K2040Voice::Exp, "exp")] {
            let p = dir.join(format!("check_r{r}_{tag}.wav"));
            crate::reference::write_f32(&p, &unitab::engine(&snd, v, &sc, r), SAMPLE_RATE)?;
            files.push(p);
        }
        let (a, b) = (unitab::take(&files[0])?, unitab::take(&files[1])?);
        let p = unitab::pair(&a, &b);
        // 40 Hz … 8 kHz of the 25 Hz … 16 kHz thirds, less Δlevel.
        let flat = p.bands[2..p.bands.len() - 3]
            .iter()
            .map(|d| (d - p.d_level).abs())
            .fold(0.0, f64::max);
        println!(
            "check RES {r}: Δlevel {:+.1} dB, third-octave Δ within ±{flat:.2} dB (40 Hz–8 kHz)",
            p.d_level
        );
    }
    Ok(())
}
