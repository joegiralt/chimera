// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The delay TYPEs side by side (ADR 0202): TAPE against GRAIN at several
//! SIZE, DENSITY, PITCH and TEXTURE settings and a shimmer, on a pluck
//! phrase, a chord stab and a percussive loop, through the real
//! `Instrument` (the Part's DELAY send, the bus, the master and limiter),
//! raw: 32-bit float, no gain, nothing matched (the A/B method,
//! ADR 0196). Each GRAIN take's
//! level against TAPE's on the same phrase goes to `levels.json`.

use chimera_core::dsp::SendLevel;
use chimera_core::dsp::delay::{DelayParams, DelayType};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::factory::factory_entries;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{EncodedSound, Loads};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::tracked::Tracked;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::fmt::Write as _;
use std::path::Path;

const SR: f64 = SAMPLE_RATE as f64;
/// The Part's DELAY send: the dry stays in, the return beside it, at the
/// gain the linear knob's 0.5 gave (`SendLevel::from_gain`, ADR 0278).
const SEND: SendLevel = SendLevel::step::<91>();
const TAIL_S: f64 = 5.0;

/// (seconds, note, on) and the phrase's length.
pub(crate) struct Phrase {
    pub name: &'static str,
    pub sound: Sound,
    pub notes: Vec<(f64, u8, bool)>,
    pub len: f64,
}

fn factory(name: &str) -> Result<Sound, String> {
    let (_, bytes) = factory_entries()
        .find(|(n, _)| n.as_str().trim() == name)
        .ok_or(format!("no factory Sound {name}"))?;
    let mut s = Sound::neutral(EngineType::Algo);
    EncodedSound::new(bytes)
        .decode_into(&mut s)
        .map_err(|e| format!("{e:?}"))?;
    Ok(s)
}

pub(crate) fn phrases() -> Result<Vec<Phrase>, String> {
    let step = 60.0 / 120.0 / 4.0;
    // A rising pluck figure in 16ths, each note a 16th long.
    let pluck: Vec<_> = [60u8, 63, 67, 70, 72, 70, 67, 63]
        .iter()
        .enumerate()
        .flat_map(|(i, &n)| {
            let t = i as f64 * step;
            [(t, n, true), (t + step * 0.9, n, false)]
        })
        .collect();
    // Two stabs of C minor 9, an 8th long, a bar apart.
    let stab: Vec<_> = [0.0, 1.0]
        .iter()
        .flat_map(|&t| {
            [48u8, 55, 58, 62, 63]
                .iter()
                .flat_map(move |&n| [(t, n, true), (t + 0.25, n, false)])
        })
        .collect();
    // Two bars of a struck loop: low hits on the beat, high ones between.
    let pattern = [36u8, 0, 72, 0, 60, 0, 72, 84, 36, 36, 72, 0, 60, 0, 72, 79];
    let perc: Vec<_> = (0..2)
        .flat_map(|bar| {
            pattern
                .iter()
                .enumerate()
                .filter(|(_, n)| **n != 0)
                .flat_map(move |(i, &n)| {
                    let t = (bar * 16 + i) as f64 * step;
                    [(t, n, true), (t + step * 0.5, n, false)]
                })
        })
        .collect();
    let mut pluck_s = Sound::init(EngineType::Modal);
    pluck_s.params.modal.mode = ResonatorMode::String;
    let mut perc_s = Sound::init(EngineType::Modal);
    perc_s.params.modal.mode = ResonatorMode::Modal;
    perc_s.params.modal.damp = 0.3;
    Ok(vec![
        Phrase {
            name: "pluck",
            sound: pluck_s,
            notes: pluck,
            len: 8.0 * step,
        },
        Phrase {
            name: "stab",
            sound: factory("TX BRASS")?,
            notes: stab,
            len: 1.25,
        },
        Phrase {
            name: "perc",
            sound: perc_s,
            notes: perc,
            len: 32.0 * step,
        },
    ])
}

/// The takes: TAPE as it ships at TIME 300 ms FDBK 0.45, then GRAIN at the
/// same TIME and FDBK over its own knobs, PITCH +12 TEXTURE 0.4 at FDBK
/// 0.45, 0.9 and 0.98 (the feedback held under unity), then the shimmer.
fn settings() -> Vec<(&'static str, DelayParams)> {
    let tape = DelayParams {
        time_ms: 300.0,
        feedback: 0.45,
        mix: 1.0,
        ..DelayParams::DEFAULT
    };
    let g = |size, density, pitch, texture| {
        tape.with(DelayParams::TYPE, DelayType::Grain.value())
            .with(DelayParams::SIZE, size)
            .with(DelayParams::DENSITY, density)
            .with(DelayParams::PITCH, pitch)
            .with(DelayParams::TEXTURE, texture)
    };
    vec![
        ("tape", tape),
        ("grain", g(0.6, 0.6, 0.0, 0.2)),
        ("grain_p12", g(0.6, 0.6, 12.0, 0.2)),
        ("grain_m12", g(0.6, 0.6, -12.0, 0.2)),
        ("grain_p7", g(0.6, 0.6, 7.0, 0.2)),
        ("grain_stutter", g(0.25, 0.85, 0.0, 0.0)),
        ("grain_sparse", g(0.45, 0.2, 0.0, 0.5)),
        ("grain_cloud", g(0.85, 0.8, 0.0, 0.9)),
        ("grain_p12_t04", g(0.6, 0.6, 12.0, 0.4)),
        (
            "grain_p12_t04_fb090",
            DelayParams {
                feedback: 0.9,
                ..g(0.6, 0.6, 12.0, 0.4)
            },
        ),
        (
            "grain_p12_t04_fb098",
            DelayParams {
                feedback: 0.98,
                ..g(0.6, 0.6, 12.0, 0.4)
            },
        ),
        (
            "shimmer",
            DelayParams {
                feedback: 0.92,
                tone: 0.9,
                ..g(0.7, 0.7, 12.0, 0.3)
            },
        ),
    ]
}

/// `ph` on Part 1 with the DELAY send at `SEND`, the chorus and reverb
/// returns off, the delay at `d`: mono (L+R)/2 after the limiter.
fn render(ph: &Phrase, d: DelayParams) -> Vec<f32> {
    let fx = FxParams {
        delay: d,
        ..FxParams::OFF
    };
    render_stereo(ph, [SendLevel::OFF, SEND, SendLevel::OFF], fx)
        .into_iter()
        .map(|[l, r]| 0.5 * (l + r))
        .collect()
}

/// `ph` on Part 1 at `sends` (chorus, delay, reverb) into the bus at
/// `fx`, its tail `TAIL_S`: (L, R) frames after the limiter.
pub(crate) fn render_stereo(ph: &Phrase, sends: [SendLevel; 3], fx: FxParams) -> Vec<[f32; 2]> {
    let mut s = ph.sound.clone();
    s.params.out.sends = sends;
    let sound = Tracked::fresh(s);
    let loads = Loads::NONE.next();
    let mut pubr = KnobPublisher::new();
    let mut shared = Box::<AudioShared>::default();
    shared.fx = fx;
    shared.publish_sound(0, &sound, loads, &mut pubr);
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (
        Box::new(chimera_core::dsp::fx_bus::FxBus::new()),
        Box::new(DacBlocks::new()),
    );
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let gv = GateView::empty();
    let ch = MidiChannel::clamped(0);
    let blocks = ((ph.len + TAIL_S) * SR) as usize / BLOCK_SIZE;
    let (mut out, mut next) = (Vec::with_capacity(blocks * BLOCK_SIZE), 0);
    for b in 0..blocks {
        let t = (b * BLOCK_SIZE) as f64 / SR;
        while next < ph.notes.len() && ph.notes[next].0 <= t {
            let (_, n, on) = ph.notes[next];
            let kind = if on {
                NoteKind::On(Velocity::new(100).unwrap())
            } else {
                NoteKind::Off
            };
            let ev = NoteEvent {
                channel: ch,
                note: MidiNote::new(n).unwrap(),
                kind,
            };
            inst.handle_with(ev, &shared, &gv);
            next += 1;
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &gv);
        out.extend(dac.out()[0].chunks(2).map(|f| [f[0], f[1]]));
    }
    out
}

/// `grain-listen OUTDIR`: every phrase × setting as
/// `<phrase>_<setting>.wav`, and `levels.json`: each take's peak, its
/// loudness (the louder half of its 0.1 s frames, dB) and, for GRAIN,
/// Δlevel and LSD against TAPE on the same phrase.
pub fn run(args: &[String]) -> Result<(), String> {
    let [dir] = args else {
        return Err("grain-listen OUTDIR".into());
    };
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut json = String::from("[\n");
    let phrases = phrases()?;
    let settings = settings();
    for ph in &phrases {
        let takes: Vec<(&str, Vec<f32>)> = settings
            .iter()
            .map(|&(name, d)| (name, render(ph, d)))
            .collect();
        let tape: Vec<f64> = takes[0].1.iter().map(|&v| f64::from(v)).collect();
        for (name, x) in &takes {
            let file = format!("{}_{name}.wav", ph.name);
            crate::reference::write_f32(&dir.join(&file), x, SAMPLE_RATE)?;
            let xf: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
            let peak = xf.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let p = crate::unitab::pair(&tape, &xf);
            writeln!(
                json,
                "  {{\"file\": \"{file}\", \"phrase\": \"{}\", \"take\": \"{name}\", \"peak_db\": {:.2}, \"d_level_db\": {:.2}, \"lsd_db\": {:.2}}},",
                ph.name,
                20.0 * peak.max(1e-12).log10(),
                p.d_level,
                p.lsd
            )
            .unwrap();
            println!(
                "{file}: peak {:.1} dBFS, Δlevel vs TAPE {:+.2} dB, LSD {:.2} dB",
                20.0 * peak.max(1e-12).log10(),
                p.d_level,
                p.lsd
            );
        }
    }
    json.truncate(json.trim_end_matches([',', '\n']).len());
    json.push_str("\n]\n");
    std::fs::write(dir.join("levels.json"), json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::SEND;
    use chimera_core::dsp::{Gain, SendLevel};

    /// The A/B keeps the level it had on the linear knob (ADR 0278).
    #[test]
    fn the_delay_send_is_the_old_half_gain() {
        assert_eq!(SEND, SendLevel::from_gain(Gain::new(0.5)));
    }
}
