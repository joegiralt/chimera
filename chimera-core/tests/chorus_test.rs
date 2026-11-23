// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Juno chorus (FX diet spec § Chorus).

mod common;
use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_hal::BLOCK_SIZE;
use common::{SR, rms};

/// `seconds` of a 220 Hz sine at 0.5 through the chorus.
fn render(p: &ChorusParams, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let mut c = Box::new(JunoChorus::new());
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut out = Stereo::SILENT;
    for b in 0..(seconds * SR as f32) as usize / BLOCK_SIZE {
        let send: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
            let n = (b * BLOCK_SIZE + i) as f32;
            0.5 * (2.0 * core::f32::consts::PI * 220.0 * n / SR as f32).sin()
        });
        c.process_wet(&send, p, SR, &mut out);
        l.extend_from_slice(&out.l);
        r.extend_from_slice(&out.r);
    }
    (l, r)
}

#[test]
fn the_mono_sum_keeps_the_wet_and_the_sides_differ() {
    for mode in 1..=3 {
        let p = ChorusParams {
            mode,
            rate: 0.5,
            depth: 0.5,
            mix: 1.0,
        };
        let (l, r) = render(&p, 4.0);
        let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a + b) / 2.0).collect();
        assert!(
            rms(&mono) >= 0.501 * rms(&l),
            "mode {mode}: mono {} vs L {}",
            rms(&mono),
            rms(&l)
        );
        assert!(l != r, "mode {mode}");
    }
}

/// Review Focus: at full DEPTH the inverted tap reaches below one sample
/// and clamps; every read is a blend of two stored samples.
#[test]
fn the_chorus_stays_bounded_at_full_depth_and_rate() {
    for mode in 1..=3 {
        let p = ChorusParams {
            mode,
            rate: 1.0,
            depth: 1.0,
            mix: 1.0,
        };
        let (l, r) = render(&p, 2.0);
        for &s in l.iter().chain(&r) {
            assert!(s.is_finite() && s.abs() <= 0.5 + 1e-6, "mode {mode}: {s}");
        }
    }
}

use chimera_core::block::DiskCode;
use chimera_core::dsp::chorus::{ChorusMode, MODE_NAMES};

const FLANGER: u8 = 4;
const PHASER: u8 = 5;

/// `render` at `mode`'s defaults, a `hz` sine at 0.5.
fn sine_through(mode: u8, hz: f32, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let p = ChorusParams {
        mode,
        ..ChorusParams::DEFAULT
    };
    let mut c = Box::new(JunoChorus::new());
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut out = Stereo::SILENT;
    for b in 0..(seconds * SR as f32) as usize / BLOCK_SIZE {
        let send: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
            let n = (b * BLOCK_SIZE + i) as f32;
            0.5 * (2.0 * core::f32::consts::PI * hz * n / SR as f32).sin()
        });
        c.process_wet(&send, &p, SR, &mut out);
        // The dry beside the return, as the mixer sums them at SEND 1.
        l.extend(out.l.iter().zip(&send).map(|(w, d)| w + d));
        r.extend(out.r.iter().zip(&send).map(|(w, d)| w + d));
    }
    (l, r)
}

/// The quietest and loudest 20 ms windows past the first half second, dB.
fn swing_db(x: &[f32]) -> f32 {
    let w = SR as usize / 50;
    let levels: Vec<f32> = x[SR as usize / 2..].chunks(w).map(rms).collect();
    let (lo, hi) = levels
        .iter()
        .fold((f32::MAX, 0.0f32), |(a, b), &v| (a.min(v), b.max(v)));
    20.0 * (hi / lo.max(1e-9)).log10()
}

#[test]
fn flanger_and_phaser_are_types_of_the_slot() {
    assert_eq!(MODE_NAMES, ["OFF", "I", "II", "I+II", "FLANGER", "PHASER"]);
    for (code, m) in [(FLANGER, ChorusMode::Flanger), (PHASER, ChorusMode::Phaser)] {
        assert_eq!(ChorusMode::from_u8(code), m);
        assert_eq!(m.disk_code(), code);
        assert_eq!(ChorusMode::from_disk_code(code), Some(m));
    }
}

/// Beside the dry, each sweeps a notch through a steady tone: its level
/// swings. A chorus barely does at the same tone.
#[test]
fn flanger_and_phaser_sweep_notches_through_a_tone() {
    for mode in [FLANGER, PHASER] {
        let (l, r) = sine_through(mode, 700.0, 6.0);
        assert!(swing_db(&l) > 12.0, "mode {mode} L: {}", swing_db(&l));
        assert!(swing_db(&r) > 12.0, "mode {mode} R: {}", swing_db(&r));
        assert!(l != r, "mode {mode}: the sides sweep apart");
    }
}

/// Feedback under one: a full-scale sine at full DEPTH and RATE stays
/// finite and under the feedback's peak gain.
#[test]
fn flanger_and_phaser_stay_bounded() {
    for mode in [FLANGER, PHASER] {
        let p = ChorusParams {
            mode,
            rate: 1.0,
            depth: 1.0,
            mix: 1.0,
        };
        let (l, r) = render(&p, 2.0);
        for &s in l.iter().chain(&r) {
            assert!(s.is_finite() && s.abs() <= 2.0, "mode {mode}: {s}");
        }
    }
}

/// The feedback rings out, then the lines are cleared: the return goes
/// to +0 bit for bit and the idle skip runs again.
#[test]
fn a_fed_back_tail_ends_in_exact_silence() {
    for mode in [FLANGER, PHASER] {
        let p = ChorusParams {
            mode,
            ..ChorusParams::DEFAULT
        };
        let mut c = Box::new(JunoChorus::new());
        let mut out = Stereo::SILENT;
        let tone: [f32; BLOCK_SIZE] = core::array::from_fn(|i| (i as f32 * 0.3).sin());
        for _ in 0..200 {
            c.process_wet(&tone, &p, SR, &mut out);
        }
        let silent = [0.0f32; BLOCK_SIZE];
        for _ in 0..(2 * SR as usize / BLOCK_SIZE) {
            c.process_wet(&silent, &p, SR, &mut out);
        }
        assert!(
            out.l.iter().chain(&out.r).all(|v| v.to_bits() == 0),
            "mode {mode}"
        );
    }
}

/// Into and out of each new TYPE on a held tone: no step larger than the
/// tone's own between two samples.
#[test]
fn type_changes_never_click() {
    let hz = 220.0;
    let step = 2.0 * core::f32::consts::PI * hz / SR as f32 * 0.5 * 2.5;
    for (from, to) in [(1, FLANGER), (FLANGER, 1), (3, PHASER), (PHASER, FLANGER)] {
        let mut c = Box::new(JunoChorus::new());
        let mut out = Stereo::SILENT;
        let (mut prev, mut worst) = (0.0f32, 0.0f32);
        for b in 0..(SR as usize / BLOCK_SIZE) {
            let mode = if b < 300 { from } else { to };
            let p = ChorusParams {
                mode,
                ..ChorusParams::DEFAULT
            };
            let send: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                let n = (b * BLOCK_SIZE + i) as f32;
                0.5 * (2.0 * core::f32::consts::PI * hz * n / SR as f32).sin()
            });
            c.process_wet(&send, &p, SR, &mut out);
            for &v in &out.l {
                if b > 250 {
                    worst = worst.max((v - prev).abs());
                }
                prev = v;
            }
        }
        assert!(worst < step, "{from} -> {to}: {worst} vs {step}");
    }
}
