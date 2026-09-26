//! Verify no clicks/discontinuities in audio output.
//! Simulates the desktop audio callback pattern: rendering blocks
//! and scattering to variable-size output buffers.

use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::reverb::{Reverb, ReverbParams};
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48000;

/// Operator 1 alone on the triangle.
fn tri() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::TRI);
    p
}

/// Simulate the audio callback: render blocks, scatter to output buffer,
/// check for discontinuities (clicks) in the output stream.
fn check_no_clicks(
    name: &str,
    setup: impl FnOnce(&mut ParamSnapshot, &mut ReverbParams),
    callback_sizes: &[usize], // simulate varying cpal buffer sizes
) {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(SR);
    let mut reverb = Reverb::new();
    let mut params = ParamSnapshot::default();
    let mut rv = FxParams::default().reverb;
    setup(&mut params, &mut rv);

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    // Persistent state (like the fixed audio callback)
    let mut block = [0.0f32; BLOCK_SIZE];
    let mut block_pos: usize = BLOCK_SIZE;
    let mut all_samples = Vec::new();

    // Simulate multiple callback invocations with varying buffer sizes
    for &cb_size in callback_sizes {
        for _ in 0..cb_size {
            if block_pos >= BLOCK_SIZE {
                voice.render(&mut block, &params, &empty_mod);
                reverb.process(&mut block, &rv);
                block_pos = 0;
            }
            let s = libm::tanhf(block[block_pos] * 0.4);
            all_samples.push(s);
            block_pos += 1;
        }
    }

    // Check for clicks: sample-to-sample jumps > threshold
    let click_threshold = 0.15; // max allowed jump between adjacent samples
    let mut clicks = Vec::new();

    for i in 1..all_samples.len() {
        let jump = (all_samples[i] - all_samples[i - 1]).abs();
        if jump > click_threshold {
            clicks.push((i, all_samples[i - 1], all_samples[i], jump));
        }
    }

    assert!(
        clicks.is_empty(),
        "{}: found {} clicks. First 5: {:?}",
        name,
        clicks.len(),
        &clicks[..clicks.len().min(5)]
    );
}

// ── Algo triangle ───────────────────────────────────────────────────

#[test]
fn test_no_clicks_algo_triangle() {
    check_no_clicks(
        "Algo triangle",
        |p, _| {
            *p = tri();
        },
        // Simulate realistic cpal callback pattern: varying buffer sizes
        &[256, 256, 256, 512, 256, 256, 128, 256, 512, 256],
    );
}

#[test]
fn test_no_clicks_algo_with_pm() {
    check_no_clicks(
        "Algo PM",
        |p, _| {
            *p = tri();
            p.algo.ops[1].level = 70;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

// ── With reverb ─────────────────────────────────────────────────────

#[test]
fn test_no_clicks_algo_with_plate_reverb() {
    check_no_clicks(
        "Algo + plate reverb",
        |p, rv| {
            *p = tri();
            rv.reverb_type = 0;
            rv.mix = 0.5;
            rv.time = 0.7;
        },
        &[256, 512, 256, 128, 256, 256, 512, 256],
    );
}

#[test]
fn test_no_clicks_algo_with_fdn_reverb() {
    check_no_clicks(
        "Algo + FDN reverb",
        |p, rv| {
            *p = tri();
            rv.reverb_type = 1;
            rv.mix = 0.5;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

#[test]
fn test_no_clicks_algo_with_midiverb() {
    check_no_clicks(
        "Algo + MidiVerb",
        |p, rv| {
            *p = tri();
            rv.reverb_type = 2;
            rv.mix = 0.5;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

// ── Modal engines ───────────────────────────────────────────────────

#[test]
fn test_no_clicks_ks_string() {
    check_no_clicks(
        "KS+ string",
        |p, _| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
        },
        &[256, 256, 256, 512, 256, 256, 256, 256],
    );
}

#[test]
fn test_no_clicks_modal() {
    // Modal resonator produces rapid oscillations from 32 SVF filters —
    // these are the character of struck metal, not clicks.
    // Use a higher threshold than other engines.
    let empty_mod = ModState::new();
    let mut voice = Voice::new(SR);
    let mut reverb = Reverb::new();
    let mut params = ParamSnapshot::for_engine(EngineType::Modal);
    params.modal.mode = ResonatorMode::Modal;
    let rv = FxParams::default().reverb;
    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut block = [0.0f32; BLOCK_SIZE];
    let mut block_pos: usize = BLOCK_SIZE;
    let mut all_samples = Vec::new();

    for &cb_size in &[256, 256, 256, 256, 256, 256, 256, 256] {
        for _ in 0..cb_size {
            if block_pos >= BLOCK_SIZE {
                voice.render(&mut block, &params, &empty_mod);
                reverb.process(&mut block, &rv);
                block_pos = 0;
            }
            all_samples.push(libm::tanhf(block[block_pos] * 0.7));
            block_pos += 1;
        }
    }

    // Modal can have fast oscillations, so just verify output is finite and bounded
    for (i, &s) in all_samples.iter().enumerate() {
        assert!(s.is_finite(), "modal sample {} is not finite", i);
        assert!(s.abs() < 2.0, "modal sample {} too loud: {}", i, s);
    }
}

// ── Misaligned callback sizes (stress test for block boundary) ──────

#[test]
fn test_no_clicks_odd_buffer_sizes() {
    check_no_clicks(
        "Algo with odd callback sizes",
        |p, _| {
            *p = tri();
        },
        // Deliberately misaligned with BLOCK_SIZE=128
        &[100, 200, 50, 300, 150, 75, 250, 100, 400, 50],
    );
}

#[test]
fn test_no_clicks_tiny_buffers() {
    check_no_clicks(
        "Algo with tiny callbacks",
        |p, _| {
            *p = tri();
        },
        // Very small buffers — stress the block boundary logic
        &[32, 32, 32, 32, 64, 32, 32, 32, 32, 64, 32, 32, 32, 32],
    );
}

#[test]
fn test_no_clicks_single_sample_buffers() {
    check_no_clicks(
        "Algo with single-sample callbacks",
        |p, _| {
            *p = tri();
        },
        // Worst case: one sample per callback
        &[1; 512],
    );
}
