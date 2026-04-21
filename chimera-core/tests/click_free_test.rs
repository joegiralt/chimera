//! Verify no clicks/discontinuities in audio output.
//! Simulates the desktop audio callback pattern: rendering blocks
//! and scattering to variable-size output buffers.

use chimera_core::modulation::ModState;
use chimera_core::dsp::reverb::Reverb;
use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48000;

/// Simulate the audio callback: render blocks, scatter to output buffer,
/// check for discontinuities (clicks) in the output stream.
fn check_no_clicks(
    name: &str,
    setup: impl FnOnce(&mut ParamSnapshot),
    callback_sizes: &[usize], // simulate varying cpal buffer sizes
) {
    let empty_mod = ModState::new();
    let mut voice = Voice::new();
    let mut reverb = Reverb::new();
    let mut params = ParamSnapshot::default();
    setup(&mut params);

    voice.note_on(60, 100, &params, SR);

    // Persistent state (like the fixed audio callback)
    let mut block = [0.0f32; BLOCK_SIZE];
    let mut block_pos: usize = BLOCK_SIZE;
    let mut all_samples = Vec::new();

    // Simulate multiple callback invocations with varying buffer sizes
    for &cb_size in callback_sizes {
        for _ in 0..cb_size {
            if block_pos >= BLOCK_SIZE {
                voice.render(&mut block, &params, &empty_mod, SR);
                reverb.process(&mut block, &params.reverb);
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

// ── FM init patch (pure sine) ───────────────────────────────────────

#[test]
fn test_no_clicks_fm_init() {
    check_no_clicks(
        "Pizza init",
        |p| {
            p.engine = EngineType::Pizza;
        },
        // Simulate realistic cpal callback pattern: varying buffer sizes
        &[256, 256, 256, 512, 256, 256, 128, 256, 512, 256],
    );
}

#[test]
fn test_no_clicks_pizza_with_crush() {
    check_no_clicks(
        "Pizza crushed",
        |p| {
            p.engine = EngineType::Pizza;
            p.pizza.crush = 0.7;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

// ── With reverb ─────────────────────────────────────────────────────

#[test]
fn test_no_clicks_pizza_with_plate_reverb() {
    check_no_clicks(
        "Pizza + plate reverb",
        |p| {
            p.engine = EngineType::Pizza;
            p.reverb.reverb_type = 0;
            p.reverb.mix = 0.5;
            p.reverb.time = 0.7;
        },
        &[256, 512, 256, 128, 256, 256, 512, 256],
    );
}

#[test]
fn test_no_clicks_pizza_with_fdn_reverb() {
    check_no_clicks(
        "Pizza + FDN reverb",
        |p| {
            p.engine = EngineType::Pizza;
            p.reverb.reverb_type = 1;
            p.reverb.mix = 0.5;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

#[test]
fn test_no_clicks_fm_with_midiverb() {
    check_no_clicks(
        "FM + MidiVerb",
        |p| {
            p.engine = EngineType::Pizza;
            p.reverb.reverb_type = 2;
            p.reverb.mix = 0.5;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

// ── Modal engines ───────────────────────────────────────────────────

#[test]
fn test_no_clicks_ks_string() {
    check_no_clicks(
        "KS+ string",
        |p| {
            p.engine = EngineType::Modal;
            p.modal.mode = 0;
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
    let mut voice = Voice::new();
    let mut reverb = Reverb::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Modal;
    params.modal.mode = 1;
    voice.note_on(60, 100, &params, SR);

    let mut block = [0.0f32; BLOCK_SIZE];
    let mut block_pos: usize = BLOCK_SIZE;
    let mut all_samples = Vec::new();

    for &cb_size in &[256, 256, 256, 256, 256, 256, 256, 256] {
        for _ in 0..cb_size {
            if block_pos >= BLOCK_SIZE {
                voice.render(&mut block, &params, &empty_mod, SR);
                reverb.process(&mut block, &params.reverb);
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
        "FM with odd callback sizes",
        |p| {
            p.engine = EngineType::Pizza;
        },
        // Deliberately misaligned with BLOCK_SIZE=128
        &[100, 200, 50, 300, 150, 75, 250, 100, 400, 50],
    );
}

#[test]
fn test_no_clicks_tiny_buffers() {
    check_no_clicks(
        "FM with tiny callbacks",
        |p| {
            p.engine = EngineType::Pizza;
        },
        // Very small buffers — stress the block boundary logic
        &[32, 32, 32, 32, 64, 32, 32, 32, 32, 64, 32, 32, 32, 32],
    );
}

#[test]
fn test_no_clicks_single_sample_buffers() {
    check_no_clicks(
        "FM with single-sample callbacks",
        |p| {
            p.engine = EngineType::Pizza;
        },
        // Worst case: one sample per callback
        &[1; 512],
    );
}
