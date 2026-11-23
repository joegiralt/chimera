// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A knob glide's block steps the filter's coefficients four times, each
//! sub-block on the steady loop (ADR 0117), where a route's block ramps
//! them per sample. The two must agree to a few percent (0.05 of a 0.8 saw), and a glide's cutoff step
//! per sub-block must be tiny.

use chimera_core::block::GlideMs;
use chimera_core::dsp::filter::{Clean, FilterMods, K2040, Ladder};
use chimera_core::dsp::knobs::coeff;
use chimera_core::params::FilterParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

/// The cutoff a 30 ms knob glide from 200 Hz to 4 kHz plays each block.
fn cutoffs() -> Vec<f32> {
    let c = coeff(GlideMs::KNOB, SAMPLE_RATE);
    let (from, to) = (200.0f32.log2(), 4000.0f32.log2());
    let mut x = from;
    (0..60)
        .map(|_| {
            x = to + (x - to) * c;
            x.exp2()
        })
        .collect()
}

fn saw(b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| 0.8 * (((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0))
}

/// Largest |stepped − per-sample| over the glide, for `run`.
fn worst(mut run: impl FnMut(&FilterParams, &FilterMods, &mut [f32; BLOCK_SIZE], bool)) -> f32 {
    let mut w = 0.0f32;
    let (mods_g, mods_m) = (
        FilterMods {
            glide: true,
            ..FilterMods::NONE
        },
        FilterMods::NONE,
    );
    for res in [0.2, 0.7] {
        // Each path keeps its own state: the runner is called with the
        // flag and owns the two filters.
        for (b, &cutoff) in cutoffs().iter().enumerate() {
            let mut p = FilterParams::default();
            (p.cutoff, p.resonance) = (cutoff, res);
            let (mut a, mut m) = (saw(b), saw(b));
            run(&p, &mods_g, &mut a, true);
            run(&p, &mods_m, &mut m, false);
            for (x, y) in a.iter().zip(&m) {
                w = w.max((x - y).abs());
            }
        }
    }
    w
}

#[test]
fn a_glide_steps_to_within_a_hair_of_the_per_sample_ramp() {
    let c = coeff(GlideMs::KNOB, SAMPLE_RATE);
    // The first block moves 1 − c of the 3.9 octaves, a quarter of it a
    // sub-block.
    let oct = (4000.0f32 / 200.0).log2() * (1.0 - c) / 4.0;
    println!(
        "max cutoff step per sub-block: {oct:.4} octaves = {:.2} %",
        (oct.exp2() - 1.0) * 100.0
    );
    assert!(oct < 0.06);
    let (mut c1, mut c2) = (Clean::new(), Clean::new());
    let a = worst(|p, m, b, g| {
        if g {
            c1.process(b, p, SAMPLE_RATE, m);
        } else {
            c2.process(b, p, SAMPLE_RATE, m);
        }
    });
    let (mut k1, mut k2) = (K2040::new(), K2040::new());
    let b = worst(|p, m, buf, g| {
        if g {
            k1.process(buf, p, SAMPLE_RATE, m);
        } else {
            k2.process(buf, p, SAMPLE_RATE, m);
        }
    });
    let (mut l1, mut l2) = (Ladder::new(), Ladder::new());
    let c3 = worst(|p, m, buf, g| {
        if g {
            l1.process(buf, p, SAMPLE_RATE, m);
        } else {
            l2.process(buf, p, SAMPLE_RATE, m);
        }
    });
    println!("max |stepped - ramped|: CLEAN {a:.5} 2040 {b:.5} LADDER {c3:.5}");
    assert!(a < 0.05 && b < 0.05 && c3 < 0.05);
}
