// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! #695 (owner 2026-11-22): CLEAN LP4 fully open (CUTOFF 20 kHz) is
//! bypassed, its LEVEL kept. Over the top third of an octave the cascade
//! opens toward Nyquist and the dry comes in, so a sweep to the top or
//! away from it never steps (`clean::OpenTop`).

use chimera_core::dsp::filter::{Clean, Fed, FilterMode, FilterMods};
use chimera_core::params::FilterParams;
use chimera_core::taps::{TapLane, TapRows};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn lp4() -> FilterParams {
    let p = FilterParams::default();
    assert_eq!(p.mode(), FilterMode::Lp24, "CLEAN's default mode");
    p
}

/// Noise at −12 dBFS, the same every run.
fn noise(n: usize) -> Vec<f32> {
    let mut s = 0x1234_5678u32;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as i32 as f32 / i32::MAX as f32) * 0.25
        })
        .collect()
}

fn sine(hz: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.25 * (core::f32::consts::TAU * hz * i as f32 / SR as f32).sin())
        .collect()
}

/// `x` through CLEAN, block b at cutoff `fc(b)`.
fn run(p: &FilterParams, x: &[f32], fc: impl Fn(usize) -> f32) -> Vec<f32> {
    let (mut f, mut p, mut y) = (Clean::new(), *p, Vec::new());
    for (b, blk) in x.as_chunks::<BLOCK_SIZE>().0.iter().enumerate() {
        p.cutoff = fc(b);
        let mut buf = *blk;
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        y.extend_from_slice(&buf);
    }
    y
}

#[test]
fn lp4_wide_open_passes_its_input_untouched() {
    let x = noise(64 * BLOCK_SIZE);
    for res in [0.0, 0.5, 1.0] {
        let mut p = lp4();
        p.resonance = res;
        let y = run(&p, &x, |_| 20_000.0);
        let worst = x
            .iter()
            .zip(&y)
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(worst == 0.0, "RES {res}: off by {worst}");
    }
}

fn rms_db(x: &[f32]) -> f32 {
    10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
}

/// A 15 kHz sine (LP4 at 20 kHz takes 4 dB off it) under a sweep: no
/// block's level moves more than `limit` dB from the last, and no sample
/// steps further than the sine's own.
fn smooth(name: &str, y: &[f32], x: &[f32], limit: f32) {
    let lv: Vec<f32> = y
        .as_chunks::<BLOCK_SIZE>()
        .0
        .iter()
        .skip(4)
        .map(|c| rms_db(c))
        .collect();
    let worst = lv
        .windows(2)
        .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(worst < limit, "{name}: a block's level moved {worst} dB");
    let step = |v: &[f32]| v.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(
        step(y) <= 1.05 * step(x),
        "{name}: step {} over the input's {}",
        step(y),
        step(x)
    );
}

#[test]
fn sweeping_to_the_top_and_away_never_steps() {
    let n = 420;
    let x = sine(15_000.0, n * BLOCK_SIZE);
    // 12 kHz to the top over 200 blocks, 20 held, back over 200.
    let fc = |b: usize| {
        let t = match b {
            0..200 => b as f32 / 200.0,
            200..220 => 1.0,
            _ => 1.0 - (b - 220) as f32 / 200.0,
        };
        (12_000.0f32 * (20_000.0f32 / 12_000.0).powf(t)).min(20_000.0)
    };
    let y = run(&lp4(), &x, fc);
    // The plain sweep's own steepest block, where the cutoff crosses the
    // sine, is 0.36 dB; a switch onto the dry would be 4.
    smooth("sweep", &y, &x, 0.4);
    // At the top it is the input.
    let top = &y[205 * BLOCK_SIZE..215 * BLOCK_SIZE];
    assert!(
        top.iter().zip(&x[205 * BLOCK_SIZE..]).all(|(a, b)| a == b),
        "held at the top"
    );
}

/// A knob jump onto the top and off it in one block ramps across that
/// block: no sample steps past the sine's own.
#[test]
fn a_jump_onto_the_top_and_off_ramps_across_its_block() {
    let x = sine(15_000.0, 60 * BLOCK_SIZE);
    let fc = |b: usize| {
        if (20..40).contains(&b) {
            20_000.0
        } else {
            8_000.0
        }
    };
    let y = run(&lp4(), &x, fc);
    let step = |v: &[f32]| v.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(
        step(&y) <= 1.05 * step(&x),
        "step {} over the input's {}",
        step(&y),
        step(&x)
    );
}

/// A 110 Hz saw at ±0.3, sample `n`.
fn saw(n: usize) -> f32 {
    0.3 * (2.0 * (n as f32 * 110.0 / SR as f32).fract() - 1.0)
}

/// OP6's row, a 290 Hz sine, for block `b`.
fn op6(b: usize) -> TapRows {
    let mut r = TapRows::new([false, true]);
    let [_, row] = r.rows_mut();
    for (i, v) in row.unwrap().iter_mut().enumerate() {
        *v = (core::f32::consts::TAU * 290.0 * (b * BLOCK_SIZE + i) as f32 / SR as f32).sin();
    }
    r
}

/// 100 blocks of the saw through LP4 at `fc`, RES ½, with FB `fb` and,
/// with `tap`, OP6 into FREQ at ½.
fn modulated(fc: f32, fb: f32, tap: bool) -> Vec<f32> {
    let mut p = lp4();
    (p.cutoff, p.resonance) = (fc, 0.5);
    let (mut f, mut y) = (Clean::new(), Vec::new());
    for b in 0..100 {
        let rows = op6(b);
        let mods = FilterMods {
            fb: Fed::new(fb, fb),
            freq: tap.then(|| TapLane {
                rows: rows.rows().unwrap(),
                from: [0.0, 0.5],
                to: [0.0, 0.5],
            }),
            ..FilterMods::NONE
        };
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| saw(b * BLOCK_SIZE + i));
        f.process(&mut buf, &p, SR, &mods);
        y.extend_from_slice(&buf);
    }
    y
}

fn rms_of_diff(a: &[f32], b: &[f32]) -> f32 {
    (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / a.len() as f32).sqrt()
}

/// FB and a FREQ tap move LP4 fully open (#1135's review, I1): each
/// sample's `g` after them is lifted and weighed, so modulation reaches
/// the top. 0.4 (7b94ccd17), a plain LP4 at 20 kHz, moved it by `BASE`
/// (RMS of modulated − unmodulated). The tap reads 1.1-1.3× it; FB up to
/// 2.6× at 20 kHz, its swing down from the full-band input there, not
/// from a 20 kHz LP4.
#[test]
fn fb_and_taps_still_move_lp4_fully_open() {
    // (FB, tap, 0.4's RMS at 20 kHz, at 18 kHz)
    const BASE: [(f32, bool, [f32; 2]); 3] = [
        (0.2, false, [0.00497, 0.00592]),
        (0.5, false, [0.00767, 0.01106]),
        (0.0, true, [0.01522, 0.01887]),
    ];
    for (j, fc) in [20_000.0, 18_000.0].into_iter().enumerate() {
        let dry = modulated(fc, 0.0, false);
        for (fb, tap, base) in BASE {
            let d = rms_of_diff(&modulated(fc, fb, tap), &dry);
            let ratio = d / base[j];
            assert!(
                (0.5..=3.0).contains(&ratio),
                "{fc} Hz, FB {fb}, tap {tap}: {d} against 0.4's {}",
                base[j]
            );
        }
    }
}

/// The largest second difference past the first blocks.
fn d2(y: &[f32]) -> f32 {
    y[4 * BLOCK_SIZE..]
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0f32, f32::max)
}

/// A 1 kHz sine at RES 0 and 0.9, 8 kHz to the top and back (#1135's
/// review, minor 4): a jump in one block and a sweep over 64 never ring
/// near Nyquist, where a fold or fuzz after would alias it down. Fully
/// open is the input, so its own second difference (0.0043) is the
/// floor: a sweep is 1.0× it at every RES, a jump 1.0× at RES 0.9 and
/// 1.26× at RES 0 and ½, as 0.4's (7b94ccd17) at RES 0. At RES 0.9 0.4
/// read 0.003 and 0.0024, under the input's: it kept RES 0.9's trim at
/// the top, where now the trim rises with the damping to the input's.
#[test]
fn a_resonant_sweep_through_the_top_stays_smooth() {
    let x = sine(1_000.0, 200 * BLOCK_SIZE);
    let own = d2(&x);
    for res in [0.0, 0.5, 0.9] {
        let mut p = lp4();
        p.resonance = res;
        let jump = run(&p, &x, |b| {
            if (40..120).contains(&b) {
                20_000.0
            } else {
                8_000.0
            }
        });
        let sweep = run(&p, &x, |b| {
            let t = match b {
                0..40 => 0.0,
                40..104 => (b - 40) as f32 / 64.0,
                104..120 => 1.0,
                120..184 => 1.0 - (b - 120) as f32 / 64.0,
                _ => 0.0,
            };
            (8_000.0f32 * 2.5f32.powf(t)).min(20_000.0)
        });
        let (j, s) = (d2(&jump) / own, d2(&sweep) / own);
        assert!(j <= 1.3, "RES {res}: a jump {j}× the input's");
        assert!(s <= 1.05, "RES {res}: a sweep {s}× the input's");
    }
}
