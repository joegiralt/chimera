// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use common::Rig;
use common::{SR, goertzel, tri};

/// Render a voice, then change a parameter mid-note, render more.
/// Returns (before_rms, after_rms) for comparison.
fn render_with_param_change(
    setup: impl FnOnce(&mut ParamSnapshot),
    tweak: impl FnOnce(&mut ParamSnapshot),
    blocks_before: usize,
    blocks_after: usize,
) -> (f32, f32, Vec<f32>, Vec<f32>) {
    let empty_mod = ModState::new();
    let mut voice = Rig::new(chimera_hal::SAMPLE_RATE);
    let mut params = ParamSnapshot::default();
    setup(&mut params);

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    // Render "before" blocks
    let mut before_buf = Vec::new();
    let mut block = [0.0f32; 64];
    for _ in 0..blocks_before {
        voice.render(&mut block, &params, &empty_mod);
        before_buf.extend_from_slice(&block);
    }

    // Tweak the parameter
    tweak(&mut params);

    // Render "after" blocks
    let mut after_buf = Vec::new();
    for _ in 0..blocks_after {
        voice.render(&mut block, &params, &empty_mod);
        after_buf.extend_from_slice(&block);
    }

    let rms = |buf: &[f32]| -> f32 {
        libm::sqrtf(buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32)
    };

    (rms(&before_buf), rms(&after_buf), before_buf, after_buf)
}

/// Blocks after a stage setting's change: its 20 ms ease settles in 15.
const EASED: usize = 48;

/// The last 8 blocks, after the ease.
fn settled(after: &[f32]) -> &[f32] {
    &after[after.len() - 8 * 64..]
}

fn harmonic_energy(buf: &[f32], f0: f32) -> f32 {
    (2..=8).map(|h| goertzel(buf, f0 * h as f32, SR)).sum()
}

// ── Voice chain: live parameter tests────────────────────────────────

#[test]
fn test_filter_cutoff_sweep_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            *p = tri();
            p.filter.cutoff = 10000.0;
        },
        |p| {
            p.filter.cutoff = 200.0;
        }, // close the filter
        16,
        16,
    );
    assert!(
        after_rms < before_rms * 0.6,
        "closing filter should reduce level: before={} after={}",
        before_rms,
        after_rms
    );
}

#[test]
fn test_filter_resonance_mid_note() {
    // The same note twice, RES raised mid-note or left at 0: the control
    // takes out the note's own onset.
    let render = |res: f32| {
        render_with_param_change(
            |p| {
                *p = tri();
                p.filter.cutoff = 1000.0;
                p.filter
                    .set_mode(chimera_core::dsp::filter::FilterMode::Lp12);
                p.filter.resonance = 0.0;
            },
            |p| p.filter.resonance = res,
            8,
            8,
        )
        .3
    };
    // A triangle has no 4th harmonic: read the odd ones either side of
    // the 1 kHz cutoff, C4's 3rd and 5th, over the fundamental: CLEAN's
    // trim (ADR 0096) lowers the whole note as RES nears onset, so only
    // the shape shows the resonance.
    let near = |b: &[f32]| {
        (goertzel(b, 3.0 * 261.63, SR) + goertzel(b, 5.0 * 261.63, SR)) / goertzel(b, 261.63, SR)
    };
    let (dead, live) = (render(0.0), render(0.9));
    let (control, raised) = (near(&dead), near(&live));
    // Live, 1.89×; with RES left dead, 1.00×. The bar was 2× on the level
    // alone, before the trim.
    assert!(
        raised > 1.5 * control,
        "resonance should boost near the cutoff: {raised} vs {control} with RES 0"
    );
    // The level: the trim at RES 0.9 (0.586 on LP12, −4.64 dB) takes the
    // fundamental down with it, less the lower `k`'s +1.18 dB there (the
    // LP12 response at r = 0.26 of the cutoff, prewarped): −3.46 dB by the
    // law, −3.06 measured over the RES ramp's block. Without the trim it
    // would rise 1.2 dB.
    use chimera_core::dsp::filter::FilterMode;
    use chimera_core::dsp::filter::clean::{clean_k, trim};
    let w = |f: f64| (core::f64::consts::PI * f / f64::from(SR)).tan();
    let r = w(261.63) / w(1000.0);
    let lp = |k: f64| 1.0 / (1.0 - r * r).hypot(k * r);
    let k = f64::from(clean_k(0.9));
    let law = 20.0 * (f64::from(trim(FilterMode::Lp12, clean_k(0.9))) * lp(k) / lp(2.0)).log10();
    let drop = 20.0 * f64::from(goertzel(&live, 261.63, SR) / goertzel(&dead, 261.63, SR)).log10();
    assert!(
        (drop - law).abs() <= 1.0,
        "fundamental {drop:+.2} dB at RES 0.9, the law {law:+.2}"
    );
}

// ── Drive: live parameter tests ─────────────────────────────────────

#[test]
fn test_drive_amount_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = tri();
            p.fx.set_type(FxType::Soft);
            p.fx.pots[0] = 0.0;
        },
        |p| {
            p.fx.pots[0] = 0.9;
        },
        8,
        EASED,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(settled(&after), f0);
    assert!(
        h_after > h_before,
        "drive should add harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── Wavefolder: live parameter tests ────────────────────────────────

#[test]
fn test_folder_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = tri();
            p.fx.set_type(FxType::Fold);
            p.fx.pots[0] = 0.0;
        },
        |p| {
            p.fx.pots[0] = 0.8;
        },
        8,
        EASED,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(settled(&after), f0);
    assert!(
        h_after > h_before,
        "folder should add harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── KS+ String: live parameter tests ────────────────────────────────

/// BODY is a model-page setting, latched at note-on (spec § 1): a
/// mid-note change leaves the sounding note alone.
#[test]
fn test_string_body_latched_at_note_on() {
    let setup = |p: &mut ParamSnapshot| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
        p.modal.body = 0.0;
    };
    let (_, _, _, kept) = render_with_param_change(setup, |_| {}, 8, 8);
    let (_, _, _, turned) = render_with_param_change(setup, |p| p.modal.body = 0.8, 8, 8);
    assert!(
        kept.iter()
            .zip(&turned)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "BODY moved a sounding note"
    );
}

/// ENS DEPTH, RATE and MIX are latched at note-on too (spec § 1).
#[test]
fn test_string_ens_latched_at_note_on() {
    let setup = |p: &mut ParamSnapshot| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
        (p.modal.ens_depth, p.modal.ens_rate, p.modal.ens_mix) = (0.5, 0.5, 0.5);
    };
    let (_, _, _, kept) = render_with_param_change(setup, |_| {}, 8, 8);
    let (_, _, _, turned) = render_with_param_change(
        setup,
        |p| (p.modal.ens_depth, p.modal.ens_rate, p.modal.ens_mix) = (1.0, 1.0, 0.0),
        8,
        8,
    );
    assert!(
        kept.iter()
            .zip(&turned)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "ENS moved a sounding note"
    );
}

#[test]
fn test_string_structure_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
            p.modal.structure = 0.0;
        },
        |p| {
            p.modal.structure = 0.7;
        },
        8,
        8,
    );
    let diff: f32 = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / before.len() as f32;
    assert!(diff > 0.001, "structure should change sound: diff={}", diff);
}

#[test]
fn test_string_bright_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
            p.modal.bright = 1.0 - 0.1;
        },
        |p| {
            p.modal.bright = 1.0 - 0.9;
        },
        4,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        (h_before - h_after).abs() > 0.0001,
        "bright should change harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── Modal resonator: live parameter tests ───────────────────────────

#[test]
fn test_bank_damp_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::Modal;
            p.modal.damp = 0.8;
        },
        |p| {
            p.modal.damp = 0.1;
        }, // shorten the ring dramatically
        8,
        16,
    );
    // After shortening DAMP, the sound should die faster
    assert!(
        after_rms < before_rms || after_rms < 0.01,
        "reducing DAMP should quiet the sound: before={} after={}",
        before_rms,
        after_rms
    );
}

#[test]
fn test_bank_bright_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::Modal;
            p.modal.bright = 0.1;
        },
        |p| {
            p.modal.bright = 1.0;
        },
        4,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        (h_before - h_after).abs() > 0.0001,
        "bank bright should change spectrum: before={} after={}",
        h_before,
        h_after
    );
}

// ── Volume: live parameter test ─────────────────────────────────────

#[test]
fn test_volume_mid_note() {
    let (before_rms, _, _, after) = render_with_param_change(
        |p| {
            *p = tri();
            p.out.volume = 0.8;
        },
        |p| {
            p.out.volume = 0.1;
        },
        8,
        EASED,
    );
    let tail = settled(&after);
    let after_rms = libm::sqrtf(tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32);
    assert!(
        after_rms < before_rms * 0.3,
        "reducing volume should reduce level: before={} after={}",
        before_rms,
        after_rms
    );
}

// ── Algo: live parameter tests ──────────────────────────────────────

/// Operator 1 alone, a sine: INIT is routed FM (ADR 0049).
fn sine(p: &mut ParamSnapshot) {
    *p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::W1);
}

#[test]
fn test_algo_wave_change_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        sine,
        |p| {
            let saw = chimera_core::dsp::algo::kinds::OpKind::Table(WaveId::SAW);
            p.algo.try_set_kind(0, saw).unwrap()
        },
        8,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        h_after > h_before * 2.0,
        "a saw has more harmonics than a sine: before {h_before}, after {h_after}"
    );
}

#[test]
fn test_algo_level_change_mid_note() {
    let (before_rms, after_rms, _, _) =
        render_with_param_change(sine, |p| p.algo.ops[0].level = 60, 8, 64);
    assert!(after_rms < before_rms * 0.1, "{before_rms} → {after_rms}");
}
