// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The chorus slot's TYPEs side by side (#794): OFF, I, II, I+II, FLANGER
//! and PHASER on a chord stab, a held sine and a pluck phrase, through the
//! real `Instrument` (the Part's CHORUS send at 1, the dry beside the
//! return, the master and limiter), stereo 32-bit float, nothing matched
//! (the A/B method). Then each TYPE's host cost: ns a sample in the
//! chorus's own loop, against I+II's.

use crate::grain_listen::{Phrase, phrases, render_stereo};
use chimera_core::dsp::SendLevel;
use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus, MODE_NAMES};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::path::Path;
use std::time::Instant;

/// The send: dry and return at equal level, as the Juno sums them (unity,
/// the taper's top, ADR 0278).
const SEND: SendLevel = SendLevel::FULL;
/// Kept past the phrase: FLANGER's and PHASER's ring is far shorter.
const KEEP_S: f64 = 1.0;

/// A held A3 on ALGO's init Sound (a sine carrier), two seconds.
fn sine() -> Phrase {
    Phrase {
        name: "sine",
        sound: Sound::init(EngineType::Algo),
        notes: vec![(0.0, 57, true), (2.0, 57, false)],
        len: 2.0,
    }
}

/// `x` as a stereo 32-bit float WAV.
fn write_stereo(path: &Path, x: &[[f32; 2]]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    for s in x.iter().flatten() {
        w.write_sample(*s).map_err(err)?;
    }
    w.finalize().map_err(err)
}

/// ns a sample of `mode` at its defaults on a sounding send, best of five.
fn cost(mode: u8) -> f64 {
    let p = ChorusParams {
        mode,
        ..ChorusParams::DEFAULT
    };
    let mut c = Box::new(JunoChorus::new());
    let mut out = Stereo::SILENT;
    let blocks = 20_000;
    let send = |b: usize| -> [f32; BLOCK_SIZE] {
        core::array::from_fn(|i| (((b * BLOCK_SIZE + i) as f32) * 0.0571).sin() * 0.5)
    };
    let sends: Vec<_> = (0..64).map(send).collect();
    // Past the ease: the steady loop.
    for b in 0..200 {
        c.process_wet(&sends[b % 64], &p, SAMPLE_RATE, &mut out);
    }
    (0..5)
        .map(|_| {
            let t = Instant::now();
            for b in 0..blocks {
                c.process_wet(&sends[b % 64], &p, SAMPLE_RATE, &mut out);
                std::hint::black_box(&out);
            }
            t.elapsed().as_nanos() as f64 / (blocks * BLOCK_SIZE) as f64
        })
        .fold(f64::MAX, f64::min)
}

/// `chorus-listen OUTDIR`: `<phrase>_<type>.wav` for every phrase and
/// TYPE, each take's peak printed; then each TYPE's ns a sample.
pub fn run(args: &[String]) -> Result<(), String> {
    let [dir] = args else {
        return Err("chorus-listen OUTDIR".into());
    };
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let all = phrases()?;
    let stab = all
        .into_iter()
        .find(|p| p.name == "stab")
        .ok_or("no stab")?;
    for ph in [stab, sine()] {
        let keep = ((ph.len + KEEP_S) * f64::from(SAMPLE_RATE)) as usize;
        for (mode, name) in MODE_NAMES.iter().enumerate() {
            let fx = FxParams {
                chorus: ChorusParams {
                    mode: mode as u8,
                    ..ChorusParams::DEFAULT
                },
                ..FxParams::OFF
            };
            let mut x = render_stereo(&ph, [SEND, SendLevel::OFF, SendLevel::OFF], fx);
            x.truncate(keep);
            let take = name.replace('+', "_").to_lowercase();
            let file = format!("{}_{take}.wav", ph.name);
            write_stereo(&dir.join(&file), &x)?;
            let peak = x.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
            println!("{file}: peak {:.1} dBFS", 20.0 * peak.max(1e-12).log10());
        }
    }
    let both = cost(3);
    for (mode, name) in MODE_NAMES.iter().enumerate().skip(1) {
        let ns = cost(mode as u8);
        println!("{name}: {ns:.2} ns a sample, {:.2}x I+II", ns / both);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SEND;
    use chimera_core::dsp::{Gain, SendLevel};

    /// The A/B keeps the level it had on the linear knob (ADR 0278).
    #[test]
    fn the_chorus_send_is_unity() {
        assert_eq!(SEND, SendLevel::from_gain(Gain::UNITY));
    }
}
