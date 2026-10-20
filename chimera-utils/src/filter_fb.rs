//! FB's A/B (#393, ADR 0179): a bass pluck in the spirit of the Kobol
//! Expander's 'Surfer' (patch sheet #37), on the INIT VA voice through
//! `Instrument`, each TYPE at FB 0, ½ and full, RES 0.5. Each WAV is RMS
//! matched to −22 dBFS (32-bit float, mono, 48 kHz); `levels.md` lists the
//! raw peak and the gain. Our own line, 110 BPM 16ths, E1–A2.

use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::fmt::Write as _;
use std::path::Path;

const SR: f32 = SAMPLE_RATE as f32;
const TARGET_DB: f32 = -22.0;

/// (16th, note, gate in 16ths), two bars.
const LINE: [(u32, u8, u32); 20] = [
    (0, 33, 1),
    (2, 33, 1),
    (3, 45, 1),
    (4, 33, 1),
    (6, 36, 1),
    (7, 33, 1),
    (8, 40, 2),
    (10, 33, 1),
    (11, 43, 1),
    (12, 33, 1),
    (14, 31, 1),
    (15, 33, 1),
    (16, 33, 1),
    (18, 45, 1),
    (19, 33, 1),
    (20, 36, 2),
    (23, 38, 1),
    (24, 40, 1),
    (26, 28, 2),
    (29, 31, 2),
];

fn ev(note: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::clamped(0),
        note: MidiNote::new(note).expect("a note"),
        kind,
    }
}

/// The pluck through `kind` at FB `fb`: ENV 1 a 130 ms decay into the VCA
/// and FREQ (+64), FREQ 110 Hz, RES 0.5.
fn pluck(kind: FilterKind, fb: f32) -> Vec<f32> {
    let mut sound = Sound::init(EngineType::Va);
    let p = &mut sound.params;
    p.filter.set_kind(kind);
    (p.filter.cutoff, p.filter.resonance, p.filter.fb) = (110.0, 0.5, fb);
    let e = &mut p.envelopes[0];
    (e.attack, e.decay, e.sustain, e.release) = (0.0, 0.47, 0.0, 0.40);
    sound
        .mod_state
        .set_fixed(ModSource::Env1.index(), FixedCol::Freq, 64);
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = sound.params.clone();
    *shared.parts[0].mod_state = sound.mod_state.clone();
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let gv = GateView::empty();
    let step = 60.0 / 110.0 / 4.0;
    let mut evs = Vec::new();
    for rep in 0..2 {
        for &(s, n, g) in &LINE {
            let t = (rep * 32 + s) as f32 * step;
            evs.push((t, ev(n, NoteKind::On(Velocity::DEFAULT))));
            evs.push((t + (g as f32 - 0.4) * step, ev(n, NoteKind::Off)));
        }
    }
    evs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let blocks = ((64.0 * step + 0.6) * SR) as usize / BLOCK_SIZE;
    let (mut out, mut next) = (Vec::with_capacity(blocks * BLOCK_SIZE), 0);
    for b in 0..blocks {
        let t = (b * BLOCK_SIZE) as f32 / SR;
        while next < evs.len() && evs[next].0 <= t {
            inst.handle_with(evs[next].1, &shared, &gv);
            next += 1;
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &gv);
        out.extend(dac.input()[0].chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    out
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

pub fn run(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut t = String::from(
        "| file | TYPE | FB | raw peak (dBFS) | raw RMS (dBFS) | gain (dB) |\n|---|---|---|---|---|---|\n",
    );
    for (kind, name) in [
        (FilterKind::Svf, "clean"),
        (FilterKind::Dirty, "dirty"),
        (FilterKind::Ladder, "ladder"),
        (FilterKind::K2040, "2040"),
    ] {
        for (fb, tag) in [(0.0f32, "fb0"), (0.5, "fb50"), (1.0, "fb100")] {
            let y = pluck(kind, fb);
            let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let rms = (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt();
            if !peak.is_finite() {
                return Err(format!("{name} {tag}: not finite"));
            }
            let gain = TARGET_DB - db(rms);
            let g = 10f32.powf(gain / 20.0);
            let v: Vec<f32> = y.iter().map(|s| s * g).collect();
            let file = format!("surfer_{name}_{tag}.wav");
            crate::reference::write_f32(&dir.join(&file), &v, SAMPLE_RATE)?;
            let _ = writeln!(
                t,
                "| `{file}` | {} | {fb} | {:.1} | {:.1} | {gain:+.1} |",
                name.to_uppercase(),
                db(peak),
                db(rms)
            );
            println!("{file}: peak {:.1} dBFS, RMS {:.1}", db(peak), db(rms));
        }
    }
    std::fs::write(dir.join("levels.md"), t).map_err(|e| e.to_string())
}
