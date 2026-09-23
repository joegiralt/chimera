use chimera_core::modulation::ModState;
use chimera_core::dsp::drive::Drive;
use chimera_core::dsp::filter::SvfFilter;
use chimera_core::dsp::voice::Voice;
use chimera_core::dsp::wavefolder::Wavefolder;
use chimera_core::params::{DriveParams, FilterParams, FolderParams, ParamSnapshot};

const SR: u32 = 48000;

/// Goertzel: measure magnitude at a specific frequency.
fn goertzel(buf: &[f32], target_freq: f32, sample_rate: u32) -> f32 {
    let n = buf.len() as f32;
    let k = (target_freq * n / sample_rate as f32).round();
    let w = 2.0 * core::f32::consts::PI * k / n;
    let coeff = 2.0 * libm::cosf(w);

    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;

    for &sample in buf {
        let s0 = sample + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }

    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    libm::sqrtf(power.abs()) / n
}

/// Generate a sine wave buffer.
fn sine_buf(freq: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| libm::sinf(i as f32 * 2.0 * core::f32::consts::PI * freq / SR as f32))
        .collect()
}

/// Sum harmonic energy (2nd through 8th) as brightness metric.
fn harmonic_energy(buf: &[f32], fundamental: f32) -> f32 {
    (2..=8)
        .map(|h| goertzel(buf, fundamental * h as f32, SR))
        .sum()
}

/// RMS of a buffer.
fn rms(buf: &[f32]) -> f32 {
    let sum_sq: f32 = buf.iter().map(|s| s * s).sum();
    libm::sqrtf(sum_sq / buf.len() as f32)
}

// ── Drive spectral tests ────────────────────────────────────────────

#[test]
fn test_drive_adds_harmonics() {
    let drive = Drive::new();
    let freq = 440.0;

    let mut clean = sine_buf(freq, 4096);
    let clean_harmonics = harmonic_energy(&clean, freq);

    let mut params = DriveParams::default();
    params.drive = 0.8;
    params.mix = 1.0;
    drive.process(&mut clean, &params);
    let driven_harmonics = harmonic_energy(&clean, freq);

    assert!(
        driven_harmonics > clean_harmonics + 0.001,
        "drive should add harmonics: clean={} driven={}",
        clean_harmonics,
        driven_harmonics
    );
}

#[test]
fn test_drive_at_zero_preserves_spectrum() {
    let drive = Drive::new();
    let freq = 440.0;

    let original = sine_buf(freq, 4096);
    let mut processed = original.clone();
    let params = DriveParams::default(); // drive=0
    drive.process(&mut processed, &params);

    let orig_h = harmonic_energy(&original, freq);
    let proc_h = harmonic_energy(&processed, freq);

    assert!(
        (orig_h - proc_h).abs() < 0.001,
        "drive=0 should not change spectrum: orig={} proc={}",
        orig_h,
        proc_h
    );
}

#[test]
fn test_drive_more_drive_more_harmonics() {
    let drive = Drive::new();
    let freq = 440.0;

    let measure = |amount: f32| -> f32 {
        let mut buf = sine_buf(freq, 4096);
        let mut params = DriveParams::default();
        params.drive = amount;
        params.mix = 1.0;
        drive.process(&mut buf, &params);
        harmonic_energy(&buf, freq)
    };

    let low = measure(0.2);
    let mid = measure(0.5);
    let high = measure(0.9);

    assert!(
        mid > low,
        "more drive = more harmonics: low={} mid={}",
        low,
        mid
    );
    assert!(
        high > mid,
        "more drive = more harmonics: mid={} high={}",
        mid,
        high
    );
}

#[test]
fn test_drive_tone_changes_spectrum() {
    let drive = Drive::new();
    let freq = 440.0;

    let measure = |tone: f32| -> f32 {
        let mut buf = sine_buf(freq, 4096);
        let mut params = DriveParams::default();
        params.drive = 0.6;
        params.tone = tone;
        params.mix = 1.0;
        drive.process(&mut buf, &params);
        harmonic_energy(&buf, freq)
    };

    let dark = measure(0.1);
    let bright = measure(0.9);

    assert!(
        (dark - bright).abs() > 0.001,
        "tone should change harmonic content: dark={} bright={}",
        dark,
        bright
    );
}

// ── Filter spectral tests ───────────────────────────────────────────

#[test]
fn test_filter_lp_removes_highs() {
    let mut filter = SvfFilter::new();
    let mut params = FilterParams::default();
    params.cutoff.set(500.0);
    params.mode = 2; // LP4

    // Mix of 200Hz (below cutoff) and 2000Hz (above cutoff)
    let mut buf: Vec<f32> = (0..4096)
        .map(|i| {
            let t = i as f32 / SR as f32;
            libm::sinf(200.0 * 2.0 * core::f32::consts::PI * t)
                + libm::sinf(2000.0 * 2.0 * core::f32::consts::PI * t)
        })
        .collect();

    let pre_low = goertzel(&buf, 200.0, SR);
    let pre_high = goertzel(&buf, 2000.0, SR);

    filter.process(&mut buf, &params, SR);

    let post_low = goertzel(&buf, 200.0, SR);
    let post_high = goertzel(&buf, 2000.0, SR);

    assert!(
        post_low > pre_low * 0.3,
        "LP should pass 200Hz: pre={} post={}",
        pre_low,
        post_low
    );
    assert!(
        post_high < pre_high * 0.2,
        "LP should attenuate 2000Hz: pre={} post={}",
        pre_high,
        post_high
    );
}

#[test]
fn test_filter_hp_removes_lows() {
    let mut filter = SvfFilter::new();
    let mut params = FilterParams::default();
    params.cutoff.set(1000.0);
    params.mode = 5; // HP4

    let mut buf: Vec<f32> = (0..4096)
        .map(|i| {
            let t = i as f32 / SR as f32;
            libm::sinf(200.0 * 2.0 * core::f32::consts::PI * t)
                + libm::sinf(4000.0 * 2.0 * core::f32::consts::PI * t)
        })
        .collect();

    let pre_low = goertzel(&buf, 200.0, SR);
    let pre_high = goertzel(&buf, 4000.0, SR);

    filter.process(&mut buf, &params, SR);

    let post_low = goertzel(&buf, 200.0, SR);
    let post_high = goertzel(&buf, 4000.0, SR);

    assert!(
        post_low < pre_low * 0.2,
        "HP should attenuate 200Hz: pre={} post={}",
        pre_low,
        post_low
    );
    assert!(
        post_high > pre_high * 0.3,
        "HP should pass 4000Hz: pre={} post={}",
        pre_high,
        post_high
    );
}

#[test]
fn test_filter_bp_passes_center() {
    let mut filter = SvfFilter::new();
    let mut params = FilterParams::default();
    params.cutoff.set(1000.0);
    params.resonance.set(0.7);
    params.mode = 3; // BP2

    let mut buf: Vec<f32> = (0..4096)
        .map(|i| {
            let t = i as f32 / SR as f32;
            libm::sinf(200.0 * 2.0 * core::f32::consts::PI * t)
                + libm::sinf(1000.0 * 2.0 * core::f32::consts::PI * t)
                + libm::sinf(5000.0 * 2.0 * core::f32::consts::PI * t)
        })
        .collect();

    filter.process(&mut buf, &params, SR);

    let low = goertzel(&buf, 200.0, SR);
    let center = goertzel(&buf, 1000.0, SR);
    let high = goertzel(&buf, 5000.0, SR);

    assert!(
        center > low && center > high,
        "BP should pass center freq: low={} center={} high={}",
        low,
        center,
        high
    );
}

#[test]
fn test_filter_resonance_boosts_cutoff() {
    let freq = 1000.0;

    let measure_peak = |reso: f32| -> f32 {
        let mut filter = SvfFilter::new();
        let mut params = FilterParams::default();
        params.cutoff.set(freq);
        params.resonance.set(reso);
        params.mode = 1; // LP2

        // White-ish noise (sum of many sines)
        let mut buf: Vec<f32> = (0..4096)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let mut s = 0.0;
                for f in &[200.0, 500.0, 1000.0, 2000.0, 4000.0] {
                    s += libm::sinf(f * 2.0 * core::f32::consts::PI * t);
                }
                s * 0.2
            })
            .collect();

        filter.process(&mut buf, &params, SR);
        goertzel(&buf, freq, SR)
    };

    let no_reso = measure_peak(0.0);
    let hi_reso = measure_peak(0.9);

    assert!(
        hi_reso > no_reso * 1.5,
        "high resonance should boost cutoff freq: no_reso={} hi_reso={}",
        no_reso,
        hi_reso
    );
}

#[test]
fn test_filter_cutoff_sweep_changes_brightness() {
    let measure_brightness = |cutoff: f32| -> f32 {
        let mut filter = SvfFilter::new();
        let mut params = FilterParams::default();
        params.cutoff.set(cutoff);
        params.mode = 2; // LP4

        // Rich signal (square-ish wave with harmonics)
        let mut buf: Vec<f32> = (0..4096)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let f = 200.0;
                libm::sinf(f * 2.0 * core::f32::consts::PI * t)
                    + 0.5 * libm::sinf(f * 2.0 * 2.0 * core::f32::consts::PI * t)
                    + 0.33 * libm::sinf(f * 3.0 * 2.0 * core::f32::consts::PI * t)
                    + 0.25 * libm::sinf(f * 4.0 * 2.0 * core::f32::consts::PI * t)
            })
            .collect();

        filter.process(&mut buf, &params, SR);
        harmonic_energy(&buf, 200.0)
    };

    let dark = measure_brightness(300.0);
    let mid = measure_brightness(1000.0);
    let bright = measure_brightness(5000.0);

    assert!(
        mid > dark,
        "higher cutoff = brighter: dark={} mid={}",
        dark,
        mid
    );
    assert!(
        bright > mid,
        "higher cutoff = brighter: mid={} bright={}",
        mid,
        bright
    );
}

// ── Wavefolder spectral tests ───────────────────────────────────────

#[test]
fn test_folder_adds_harmonics() {
    let folder = Wavefolder::new();
    let freq = 440.0;

    let clean = sine_buf(freq, 4096);
    let clean_h = harmonic_energy(&clean, freq);

    let mut folded = sine_buf(freq, 4096);
    let mut params = FolderParams::default();
    params.fold.set(0.7);
    params.mix.set(1.0);
    folder.process(&mut folded, &params);
    let folded_h = harmonic_energy(&folded, freq);

    assert!(
        folded_h > clean_h + 0.001,
        "folder should add harmonics: clean={} folded={}",
        clean_h,
        folded_h
    );
}

#[test]
fn test_folder_more_fold_more_harmonics() {
    let folder = Wavefolder::new();
    let freq = 440.0;

    let measure = |amount: f32| -> f32 {
        let mut buf = sine_buf(freq, 4096);
        let mut params = FolderParams::default();
        params.fold.set(amount);
        params.mix.set(1.0);
        folder.process(&mut buf, &params);
        harmonic_energy(&buf, freq)
    };

    let low = measure(0.2);
    let high = measure(0.8);

    assert!(
        high > low,
        "more fold = more harmonics: low={} high={}",
        low,
        high
    );
}

// ── Full voice chain spectral tests ─────────────────────────────────

#[test]
fn test_voice_filter_sweep_audible() {
    let empty_mod = ModState::new();
    let freq = 261.6; // middle C
    let f0 = freq;

    let measure = |cutoff: f32| -> f32 {
        let mut voice = Voice::new();
        let mut params = ParamSnapshot::default();
        // Pizza produces harmonics by default
        params.filter.cutoff.set(cutoff);
        params.filter.mode = 2; // LP4

        voice.note_on(60, 100, &params, SR);

        let mut all = Vec::new();
        let mut block = [0.0f32; 64];
        for _ in 0..32 {
            voice.render(&mut block, &params, &empty_mod, SR);
            all.extend_from_slice(&block);
        }
        harmonic_energy(&all, f0)
    };

    let dark = measure(300.0);
    let bright = measure(8000.0);

    assert!(
        bright > dark,
        "filter cutoff sweep should change brightness: dark={} bright={}",
        dark,
        bright
    );
}

#[test]
fn test_voice_drive_adds_grit() {
    let empty_mod = ModState::new();
    let freq = 261.6;

    let measure = |drive_amount: f32| -> f32 {
        let mut voice = Voice::new();
        let mut params = ParamSnapshot::default();
        params.drive.drive = drive_amount;
        params.drive.mix = 1.0;

        voice.note_on(60, 100, &params, SR);

        let mut all = Vec::new();
        let mut block = [0.0f32; 64];
        for _ in 0..32 {
            voice.render(&mut block, &params, &empty_mod, SR);
            all.extend_from_slice(&block);
        }
        harmonic_energy(&all, freq)
    };

    let clean = measure(0.0);
    let dirty = measure(0.8);

    assert!(
        dirty > clean,
        "drive should add harmonic content: clean={} dirty={}",
        clean,
        dirty
    );
}
