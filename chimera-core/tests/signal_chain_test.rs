use chimera_core::dsp::drive::Drive;
use chimera_core::dsp::filter::SvfFilter;
use chimera_core::dsp::voice::Voice;
use chimera_core::dsp::wavefolder::Wavefolder;
use chimera_core::params::{DriveParams, FilterParams, FolderParams, ParamSnapshot};

// ── Drive ───────────────────────────────────────────────────────────

#[test]
fn test_drive_passthrough_at_zero() {
    let drive = Drive::new();
    let params = DriveParams::default(); // drive=0
    let mut buf = [0.5, -0.5, 0.3, -0.3];
    let original = buf;
    drive.process(&mut buf, &params);
    for (a, b) in buf.iter().zip(original.iter()) {
        assert!((a - b).abs() < 0.001, "drive=0 should passthrough");
    }
}

#[test]
fn test_drive_clips_signal() {
    let drive = Drive::new();
    let mut params = DriveParams::default();
    params.drive.set(1.0); // full drive
    params.mix.set(1.0);

    let mut buf = [1.0; 4];
    drive.process(&mut buf, &params);
    for &s in &buf {
        assert!(s.abs() <= 1.5, "drive should soft-clip, got {}", s);
    }
}

#[test]
fn test_drive_output_bounded() {
    let drive = Drive::new();
    let mut params = DriveParams::default();
    params.drive.set(1.0);
    params.mix.set(1.0);

    let mut buf = [5.0, -5.0, 10.0, -10.0];
    drive.process(&mut buf, &params);
    for &s in &buf {
        assert!(s.abs() < 3.0, "drive output should be bounded, got {}", s);
    }
}

// ── Filter ──────────────────────────────────────────────────────────

#[test]
fn test_filter_lowpass_attenuates_high_freq() {
    let mut filter = SvfFilter::new();
    let mut params = FilterParams::default();
    params.cutoff.set(200.0); // very low cutoff
    params.mode = 2; // LP4

    // Generate a high-frequency signal (5kHz at 48kHz = fast oscillation)
    let mut buf = [0.0f32; 64];
    for (i, s) in buf.iter_mut().enumerate() {
        *s = if i % 5 < 3 { 1.0 } else { -1.0 };
    }

    let pre_energy: f32 = buf.iter().map(|s| s * s).sum();
    filter.process(&mut buf, &params, 48000);
    let post_energy: f32 = buf.iter().map(|s| s * s).sum();

    assert!(
        post_energy < pre_energy * 0.1,
        "LP4 at 200Hz should attenuate high freq: pre={} post={}",
        pre_energy,
        post_energy
    );
}

#[test]
fn test_filter_passes_low_freq() {
    let mut filter = SvfFilter::new();
    let mut params = FilterParams::default();
    params.cutoff.set(5000.0);
    params.mode = 2; // LP4

    // Low frequency signal: one cycle over 128 samples ≈ 375Hz at 48kHz
    let mut buf = [0.0f32; 64];
    for (i, s) in buf.iter_mut().enumerate() {
        *s = libm::sinf(i as f32 * 2.0 * core::f32::consts::PI / 128.0);
    }

    let pre_energy: f32 = buf.iter().map(|s| s * s).sum();
    filter.process(&mut buf, &params, 48000);
    let post_energy: f32 = buf.iter().map(|s| s * s).sum();

    assert!(
        post_energy > pre_energy * 0.5,
        "LP4 at 5kHz should pass low freq: pre={} post={}",
        pre_energy,
        post_energy
    );
}

#[test]
fn test_filter_output_stable() {
    let mut filter = SvfFilter::new();
    let mut params = FilterParams::default();
    params.resonance.set(0.99); // near self-oscillation
    params.cutoff.set(1000.0);
    params.mode = 2; // LP4

    let mut buf = [0.0f32; 64];
    buf[0] = 1.0; // impulse

    for _ in 0..10 {
        filter.process(&mut buf, &params, 48000);
        let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max < 100.0,
            "filter should not explode at high resonance, got {}",
            max
        );
    }
}

// ── Wavefolder ──────────────────────────────────────────────────────

#[test]
fn test_folder_passthrough_at_zero() {
    let folder = Wavefolder::new();
    let params = FolderParams::default(); // fold=0
    let mut buf = [0.5, -0.5, 0.8, -0.8];
    let original = buf;
    folder.process(&mut buf, &params);
    for (a, b) in buf.iter().zip(original.iter()) {
        assert!((a - b).abs() < 0.001, "fold=0 should passthrough");
    }
}

#[test]
fn test_folder_output_bounded() {
    let folder = Wavefolder::new();
    let mut params = FolderParams::default();
    params.fold.set(1.0);
    params.mix.set(1.0);

    let mut buf = [5.0, -5.0, 10.0, -10.0];
    folder.process(&mut buf, &params);
    for &s in &buf {
        assert!(
            s.abs() <= 1.01,
            "wavefolder output should be bounded to ±1, got {}",
            s
        );
    }
}

#[test]
fn test_folder_adds_harmonics() {
    let folder = Wavefolder::new();
    let mut params = FolderParams::default();
    params.fold.set(1.0);
    params.mix.set(1.0);

    // Sine wave
    let mut buf = [0.0f32; 64];
    for (i, s) in buf.iter_mut().enumerate() {
        *s = libm::sinf(i as f32 * 2.0 * core::f32::consts::PI / 32.0);
    }

    let zc_before: usize = buf
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count();

    folder.process(&mut buf, &params);

    let zc_after: usize = buf
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count();

    assert!(
        zc_after > zc_before,
        "folding should add zero crossings: before={} after={}",
        zc_before,
        zc_after
    );
}

// ── Voice (full chain) ──────────────────────────────────────────────

#[test]
fn test_voice_silent_when_idle() {
    let mut voice = Voice::new();
    let params = ParamSnapshot::default();
    let mut output = [0.0f32; 64];
    voice.render(&mut output, &params, 48000);
    let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max < 0.001, "idle voice should be silent");
}

#[test]
fn test_voice_produces_sound() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    // Pizza produces sound by default

    voice.note_on(60, 100, &params, 48000);

    let mut output = [0.0f32; 64];
    for _ in 0..4 {
        voice.render(&mut output, &params, 48000);
    }

    let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(
        max > 0.01,
        "voice should produce sound after note_on, got {}",
        max
    );
}

#[test]
fn test_voice_filter_shapes_sound() {
    let mut voice_open = Voice::new();
    let mut voice_closed = Voice::new();
    let mut params_open = ParamSnapshot::default();
    let mut params_closed = ParamSnapshot::default();

    // Both with active Pizza

    // One with open filter, one with very closed filter
    params_open.filter.cutoff.set(15000.0);
    params_closed.filter.cutoff.set(100.0);
    params_closed.filter.mode = 2; // LP4

    voice_open.note_on(60, 100, &params_open, 48000);
    voice_closed.note_on(60, 100, &params_closed, 48000);

    let mut out_open = [0.0f32; 64];
    let mut out_closed = [0.0f32; 64];

    for _ in 0..8 {
        voice_open.render(&mut out_open, &params_open, 48000);
        voice_closed.render(&mut out_closed, &params_closed, 48000);
    }

    // Closed filter should have less energy (high frequencies removed)
    let energy_open: f32 = out_open.iter().map(|s| s * s).sum();
    let energy_closed: f32 = out_closed.iter().map(|s| s * s).sum();

    assert!(
        energy_closed < energy_open,
        "closed filter should reduce energy: open={} closed={}",
        energy_open,
        energy_closed
    );
}

#[test]
fn test_voice_output_bounded() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.pizza.crush = 0.7;
    params.drive.drive.set(1.0);
    params.drive.mix.set(1.0);
    params.folder.fold.set(0.5);
    params.folder.mix.set(1.0);

    voice.note_on(60, 127, &params, 48000);

    let mut output = [0.0f32; 64];
    for _ in 0..20 {
        voice.render(&mut output, &params, 48000);
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max < 10.0, "voice output should stay bounded, got {}", max);
    }
}
