// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `VarOsc`'s modulated read phase (the MOD path, ADR 0158): the read
//! phase beside the clean clock, FM through zero, backward reads, the
//! reversing turn and `settle`. Tolerances per the VA MODEs ruling R1.

mod common;

use chimera_core::dsp::va::osc::{Correction, Morph, NO_WRAP, Ramps, Unit, VarOsc, ZEROS, wave_at};
use chimera_core::dsp::va::stability::WrapJitter;
use chimera_hal::BLOCK_SIZE;
use common::spectrum::{alias_db, alias_db_lattice, peak_hz};

const SR: f32 = 48_000.0;
const B: usize = BLOCK_SIZE;
const C5: u8 = 72;
const C6: u8 = 84;
/// The alias gate's window: a 2¹² settle and 2¹⁵ samples.
const GATE_BLOCKS: usize = ((1 << 12) + (1 << 15)) / B;
/// `peak_hz`'s window, 2¹⁷ samples.
const PITCH_BLOCKS: usize = (1 << 17) / B;
const GRID: [f32; 7] = [0.0, 1.0 / 6.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0];

fn hz(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

fn ramps(inc: f32, wave: f32, shape: f32) -> Ramps {
    Ramps {
        inc: (inc, 0.0),
        wave: (wave, 0.0),
        shape: (shape, 0.0),
    }
}

fn morph(wave: f32, shape: f32) -> Morph {
    Morph {
        wave: Unit::new(wave),
        shape: Unit::new(shape),
    }
}

fn osc(c: Correction) -> VarOsc {
    let mut o = VarOsc::NEW.with_correction(c);
    o.prepare(SR);
    o
}

/// One block, plain.
fn plain(o: &mut VarOsc, r: &Ramps) -> [f32; B] {
    let (mut out, mut mk) = ([0.0f32; B], [[NO_WRAP; 2]; B]);
    o.begin(r, B);
    let mut jit = WrapJitter::NONE;
    o.render::<B, false, false, false, false>(
        &mut out,
        &mut mk,
        r,
        &mut jit,
        &mut || WrapJitter::NONE,
        &ZEROS,
        (0.0, 0.0),
    );
    out
}

/// One block on the MOD path, read rate `ie·(1 + k.0·m) + k.1·m`.
fn modulated(o: &mut VarOsc, r: &Ramps, m: &[f32; B], k: (f32, f32)) -> [f32; B] {
    let (mut out, mut mk) = ([0.0f32; B], [[NO_WRAP; 2]; B]);
    o.begin(r, B);
    let mut jit = WrapJitter::NONE;
    o.render::<B, false, false, false, true>(
        &mut out,
        &mut mk,
        r,
        &mut jit,
        &mut || WrapJitter::NONE,
        m,
        k,
    );
    out
}

/// `blocks` MOD blocks, `m` of the sample index.
fn take_mod(
    c: Correction,
    r: &Ramps,
    blocks: usize,
    k: (f32, f32),
    m: impl Fn(usize) -> f32,
) -> Vec<f32> {
    let mut o = osc(c);
    let mut y = Vec::with_capacity(blocks * B);
    for b in 0..blocks {
        let mb: [f32; B] = core::array::from_fn(|i| m(b * B + i));
        y.extend(modulated(&mut o, r, &mb, k));
    }
    y
}

/// The largest second difference from sample 2 on.
fn d2(y: &[f32]) -> f64 {
    (2..y.len())
        .map(|n| (f64::from(y[n]) - 2.0 * f64::from(y[n - 1]) + f64::from(y[n - 2])).abs())
        .fold(0.0, f64::max)
}

/// The distance between two phases around the cycle.
fn circ(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(1.0);
    d.min(1.0 - d)
}

#[test]
fn mod_with_no_modulation_matches_plain() {
    let inc = hz(C6) / SR;
    let mut worst = 0.0f32;
    for x in GRID {
        for s in [0.0, 0.5, 1.0] {
            let r = ramps(inc, x, s);
            let (mut a, mut b) = (osc(Correction::On), osc(Correction::On));
            let mut err = 0.0f32;
            for _ in 0..2_000 {
                let (ya, yb) = (plain(&mut a, &r), modulated(&mut b, &r, &ZEROS, (1.0, 0.0)));
                for (p, q) in ya.iter().zip(&yb) {
                    err = err.max((p - q).abs());
                }
            }
            assert!(
                circ(a.phase(), b.read_phase()) <= 1e-6,
                "WAVE {x:.3} SHAPE {s}: read drifted"
            );
            println!("WAVE {x:.3} SHAPE {s}: max |MOD − plain| {err:.2e}");
            assert!(err <= 1e-3, "WAVE {x:.3} SHAPE {s}: {err}");
            worst = worst.max(err);
        }
    }
    println!("worst {worst:.2e}");
}

#[test]
fn backward_is_forward_reversed() {
    let f0 = hz(C6);
    let inc = f0 / SR;
    let mut worst = f64::MIN;
    for x in GRID {
        for s in [0.0, 0.5] {
            let r = ramps(inc, x, s);
            // FM form, m = −2: the read runs at −inc
            let back = take_mod(Correction::On, &r, PITCH_BLOCKS, (1.0, 0.0), |_| -2.0);
            let fwd = take_mod(Correction::On, &r, GATE_BLOCKS, (1.0, 0.0), |_| 0.0);
            let (on, ahead) = (alias_db(&back, SR, &[f0]), alias_db(&fwd, SR, &[f0]));
            let f = peak_hz(&back, SR, f0);
            let cents = 1200.0 * (f / f64::from(f0)).log2();
            println!(
                "WAVE {x:.3} SHAPE {s}: backward {on:.1} dB (forward {ahead:.1}), {cents:+.3} cents"
            );
            assert!(back.iter().all(|v| v.is_finite()), "WAVE {x} SHAPE {s}");
            assert!(on <= -45.0, "WAVE {x:.3} SHAPE {s}: {on}");
            assert!(cents.abs() <= 1.0, "WAVE {x:.3} SHAPE {s}: {cents} cents");
            worst = worst.max(on);
        }
    }
    println!("worst backward {worst:.2} dB");
}

#[test]
fn through_zero_is_corrected() {
    let fc = hz(C5);
    let fm = fc;
    let inc = fc / SR;
    let m = |n: usize| 3.0 * (core::f32::consts::TAU * fm * n as f32 / SR).sin();
    let mut worst = f64::MIN;
    for x in GRID {
        let r = ramps(inc, x, 0.0);
        let on = take_mod(Correction::On, &r, GATE_BLOCKS, (1.0, 0.0), m);
        let off = take_mod(Correction::Off, &r, GATE_BLOCKS, (1.0, 0.0), m);
        assert!(on.iter().all(|v| v.is_finite()), "WAVE {x}: NaN");
        let (a_on, a_off) = (
            alias_db_lattice(&on, SR, fc, fm),
            alias_db_lattice(&off, SR, fc, fm),
        );
        println!("WAVE {x:.3}: through zero {a_on:.1} dB (Off {a_off:.1})");
        // the sine has no edges: nothing to correct, nothing aliasing
        assert!(
            a_off <= -60.0 || a_on <= a_off - 12.0,
            "WAVE {x:.3}: {a_on} vs {a_off}"
        );
        worst = worst.max(a_on);
    }
    println!("worst through zero {worst:.2} dB");
}

#[test]
fn a_turn_is_continuous_and_corrected() {
    // the saw's own steps set its click limit; the triangle has none, so
    // a jump at a turn would stand out there
    for (name, wave) in [("saw", 2.0 / 3.0), ("tri", 1.0 / 3.0)] {
        turns_of(name, morph(wave, 0.0));
    }
}

/// A turn every 37.3 samples at C5 on `w`: each turn's BLAMP, the read
/// retracing, the click rule against `w` held.
fn turns_of(name: &str, w: Morph) {
    let inc = hz(C5) / SR;
    let span = 37.3f64;
    let n = 4_800;
    let mut o = osc(Correction::On);
    let held: Vec<f32> = (0..n).map(|_| o.tick_mod(inc, w, 0.0, None).out).collect();
    let mut o = osc(Correction::On);
    let r0 = o.read_phase();
    let (mut y, mut turns, mut dir) = (Vec::with_capacity(n), 0usize, 1.0f32);
    for i in 0..n {
        // sample i runs from time i to i + 1: a turn inside it is `t` ago
        let next = span * (turns + 1) as f64;
        let turn = (next < (i + 1) as f64).then(|| ((i + 1) as f64 - next) as f32);
        let before = o.read_phase();
        let tk = o.tick_mod(inc, w, 0.0, turn);
        y.push(tk.out);
        let Some(t) = turn else { continue };
        turns += 1;
        // the turn's BLAMP: the slope a cycle there × the rate's jump
        let at = before + dir * inc * (1.0 - t);
        let want = wave_at(at.rem_euclid(1.0), w, inc).1 * (-2.0 * dir * inc);
        let got = o
            .last_edges()
            .iter()
            .find(|e| e.1 == 0.0 && (e.0 - t).abs() < 1e-6);
        let got = got.map_or(0.0, |e| e.2);
        assert!(
            (got - want).abs() <= 1e-3 * want.abs() + 1e-7,
            "{name} turn {turns}: BLAMP {got} vs {want}"
        );
        dir = -dir;
        if turns % 2 == 0 {
            // back where the first leg started, then `t` on, forward
            let d = circ(o.read_phase(), r0 + inc * t);
            assert!(d <= 1e-4, "{name} turn {turns}: read {d} off");
        }
    }
    assert!(turns >= 100, "{name}: {turns} turns");
    assert!(y.iter().all(|v| v.is_finite()), "{name}");
    let (d, lim) = (d2(&y), 1.5 * d2(&held) + 1e-6);
    println!("{name}, {turns} turns: Δ² {d:.3e} vs limit {lim:.3e}");
    assert!(d <= lim, "{name}: {d} > {lim}");
}

#[test]
fn settle_keeps_the_pair_coherent() {
    let inc = hz(C5) / SR;
    let r = ramps(inc, 1.0 / 6.0, 0.3);
    let pm = |n: usize| 0.004 * (core::f32::consts::TAU * n as f32 / 700.0).sin() + 0.001;
    // the read leaves the clock: PM offsets for 50 blocks
    let lead = |o: &mut VarOsc| {
        for b in 0..50 {
            let mb: [f32; B] = core::array::from_fn(|i| pm(b * B + i));
            modulated(o, &r, &mb, (0.0, 1.0));
        }
    };
    let mut o = osc(Correction::On);
    lead(&mut o);
    let read = o.read_phase();
    assert!(
        circ(read, o.phase()) > 0.01,
        "the read never left the clock"
    );
    let mut stale = o;
    o.settle();
    assert_eq!(o.phase(), read);
    let mut fresh = osc(Correction::On);
    fresh.set_phase(read);
    let (mut err, mut drift) = (0.0f32, 0.0f32);
    for b in 0..50 {
        let (ys, yf, yx) = (
            plain(&mut o, &r),
            plain(&mut fresh, &r),
            plain(&mut stale, &r),
        );
        if b == 0 {
            continue;
        }
        for i in 0..B {
            err = err.max((ys[i] - yf[i]).abs());
            drift = drift.max((yx[i] - yf[i]).abs());
        }
    }
    println!("settled vs fresh {err:.2e}; unsettled {drift:.2e}");
    assert!(err <= 1e-4, "{err}");
    // without `settle` the pair reads two phases: the test can see it
    assert!(drift > 1e-2, "{drift}");
}
