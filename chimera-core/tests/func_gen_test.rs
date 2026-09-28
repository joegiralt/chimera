//! Envelope B (filter-routing spec § Envelope B, § Tests "Envelope B").

use chimera_core::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use chimera_core::dsp::modulator::law::{B_RATE, B_TIME, BURST_LEN, block_rate_max};
use chimera_core::dsp::modulator::{EnvForm, Func, FuncParams, LfoForm};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

/// `FuncParams` running `f`, with the three sliders.
fn fp(f: Func, rise: f32, fall: f32, shape: f32) -> FuncParams {
    let mut p = FuncParams {
        rise,
        fall,
        shape,
        ..FuncParams::ENV
    };
    p.set_func(f);
    p
}

fn coefs(p: &FuncParams, per_sample: bool) -> BCoefs {
    BCoefs::new(p, &Slides::default(), SR, per_sample)
}

/// `n` ticks from a note-on with the key held for `key_for` of them.
fn run(p: &FuncParams, n: usize, key_for: usize) -> (Vec<f32>, FuncGen) {
    let c = coefs(p, true);
    let mut g = FuncGen::new();
    g.set(&c);
    g.note_on(p.func());
    let out = (0..n)
        .map(|i| {
            let key = i < key_for;
            if !key {
                g.key_up();
            }
            g.tick(&c, key)
        })
        .collect();
    (out, g)
}

fn secs(n: usize) -> f32 {
    n as f32 / SR as f32
}

#[test]
fn ad_ignores_key_up_and_ends_idle() {
    let p = fp(Func::Env(EnvForm::Ad), 0.2, 0.3, 0.5);
    let (out, g) = run(&p, SR as usize, 10);
    let top = out
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m })
        .0;
    assert!(out[top] > 0.99, "reaches 1");
    assert!((secs(top) - B_TIME.at(0.2)).abs() < 2.0 / SR as f32);
    let idle = out.iter().rposition(|&v| v > 0.0).unwrap() + 1;
    assert!((secs(idle - top) - B_TIME.at(0.3)).abs() < 2.0 / SR as f32);
    assert!(g.is_idle());
}

#[test]
fn ahr_holds_while_held_then_falls() {
    let p = fp(Func::Env(EnvForm::Ahr), 0.1, 0.2, 0.5);
    let (out, g) = run(&p, 30_000, 20_000);
    assert!(out[5_000..20_000].iter().all(|&v| v == 1.0), "holds at 1");
    let gone = out.iter().rposition(|&v| v > 0.0).unwrap() + 1;
    assert!((secs(gone - 20_000) - B_TIME.at(0.2)).abs() < 2.0 / SR as f32);
    assert!(g.is_idle());
    // A key-up during the rise falls from where it is.
    let (out, _) = run(&p, 2_000, 50);
    assert!((out[50] - out[49]).abs() < 0.02, "no step at key-up");
}

#[test]
fn cycle_period_is_rise_plus_fall() {
    let p = fp(Func::Env(EnvForm::Cycle), 0.1, 0.15, 0.5);
    let (out, g) = run(&p, 4 * SR as usize, usize::MAX);
    let lows: Vec<usize> = (1..out.len() - 1)
        .filter(|&i| out[i] < out[i - 1] && out[i] <= out[i + 1])
        .collect();
    let period = secs(lows[2] - lows[1]);
    assert!((period - (B_TIME.at(0.1) + B_TIME.at(0.15))).abs() < 3.0 / SR as f32);
    assert!(!g.is_idle(), "CYCLE is never idle");
}

/// SHAPE bottom, centre and top: log, linear and exp; at the ends a
/// segment has covered 94 % or 6 % of its swing at half its time.
#[test]
fn shape_bends_the_segments() {
    for (shape, want) in [(0.0f32, 16.0 / 17.0), (0.5, 0.5), (1.0, 1.0 / 17.0)] {
        let p = fp(Func::Env(EnvForm::Ad), 0.3, 0.3, shape);
        let half = (B_TIME.at(0.3) * SR as f32 / 2.0) as usize;
        let (out, _) = run(&p, half, usize::MAX);
        assert!(
            (out[half - 1] - want).abs() < 0.01,
            "SHAPE {shape}: {}",
            out[half - 1]
        );
    }
}

/// FREE: TILT bottom, centre and top give saw, triangle and ramp, and FALL
/// offsets the phase.
#[test]
fn lfo_free_tilts_and_offsets() {
    let at = |tilt: f32, phase_off: f32, phase: f32| {
        let p = fp(Func::Lfo(LfoForm::Free), 0.3, phase_off, tilt);
        let c = coefs(&p, true);
        let mut g = FuncGen::new();
        g.set(&c);
        let n = (phase * SR as f32 / B_RATE.at(0.3)).round() as u32;
        g.advance(&c, true, n);
        g.output()
    };
    assert!((at(0.0, 0.0, 0.25) - 0.5).abs() < 0.01, "saw: 1 − φ");
    assert!(at(0.5, 0.0, 0.25).abs() < 0.01, "triangle");
    assert!((at(1.0, 0.0, 0.25) + 0.5).abs() < 0.01, "ramp: φ");
    assert!(
        (at(0.0, 0.25, 0.0) - at(0.0, 0.0, 0.25)).abs() < 0.01,
        "FALL is PHASE"
    );
}

#[test]
fn sync_resets_the_phase_at_note_on() {
    let p = fp(Func::Lfo(LfoForm::Sync), 0.5, 0.0, 0.5);
    let c = coefs(&p, false);
    let mut g = FuncGen::new();
    g.set(&c);
    g.advance(&c, true, 1_000);
    g.note_on(Func::Lfo(LfoForm::Sync));
    assert!(
        (g.output() + 1.0).abs() < 1e-6,
        "the wave restarts at PHASE 0"
    );
}

#[test]
fn lfv_is_bounded_steps_by_delta_and_slews() {
    let steps = |slew: f32| {
        let p = fp(Func::Lfo(LfoForm::Lfv), 0.6, 0.4, slew);
        let (out, _) = run(&p, 5 * SR as usize, usize::MAX);
        assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)), "within ±1");
        let cycle = (SR as f32 / B_RATE.at(0.6)) as usize;
        for w in out.chunks(cycle).collect::<Vec<_>>().windows(2) {
            assert!(
                (w[1][0] - w[0][0]).abs() <= 0.4 + 1e-3,
                "DELTA bounds a cycle's move"
            );
        }
        out.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    };
    assert!(steps(0.8) < steps(0.0), "SLEW smooths");
}

#[test]
fn burst_forms() {
    let len = |fall: f32| (BURST_LEN.at(fall) * SR as f32) as usize;
    // AD: one burst LENGTH long, key held or not.
    let p = fp(Func::Burst(EnvForm::Ad), 0.9, 0.2, 0.5);
    let (out, g) = run(&p, 3 * len(0.2), usize::MAX);
    let last = out.iter().rposition(|&v| v > 0.0).unwrap();
    assert!((last as i64 - len(0.2) as i64).abs() < 64, "{last}");
    assert!(g.is_idle());
    // AHR: sustains while held.
    let p = fp(Func::Burst(EnvForm::Ahr), 0.9, 0.2, 0.5);
    let (out, _) = run(&p, 4 * len(0.2), 3 * len(0.2));
    assert!(
        out[2 * len(0.2)..3 * len(0.2)].iter().any(|&v| v > 0.5),
        "pulses while held"
    );
    // CYCLE: repeats while held; the running burst ends after key-up.
    let p = fp(Func::Burst(EnvForm::Cycle), 0.9, 0.2, 0.5);
    let (out, g) = run(&p, 6 * len(0.2), 3 * len(0.2) + len(0.2) / 2);
    assert!(out[2 * len(0.2)..3 * len(0.2)].iter().any(|&v| v > 0.3));
    assert!(out[5 * len(0.2)..].iter().all(|&v| v == 0.0));
    assert!(g.is_idle());
}

/// TILT bottom, centre and top put the loudest pulse at the start, middle
/// and end of the burst.
#[test]
fn burst_tilt_moves_the_loudest_pulse() {
    for (tilt, where_) in [(0.0f32, 0.0f32), (0.5, 0.5), (1.0, 1.0)] {
        let p = fp(Func::Burst(EnvForm::Ad), 0.9, 0.3, tilt);
        let n = (BURST_LEN.at(0.3) * SR as f32) as usize;
        let (out, _) = run(&p, n, usize::MAX);
        let loudest = out
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m })
            .0;
        assert!(
            (loudest as f32 / n as f32 - where_).abs() < 0.1,
            "TILT {tilt}: {loudest} of {n}"
        );
    }
}

/// The block rate ÷ 8 (93.75 Hz at 48 kHz), and at the desktop's 44.1 kHz
/// its own 86.1 Hz (Review Focus 2).
#[test]
fn per_block_rates_stop_at_an_eighth_of_the_block_rate() {
    for sr in [48_000u32, 44_100] {
        let p = fp(Func::Lfo(LfoForm::Free), 1.0, 0.0, 1.0); // 800 Hz asked
        let c = BCoefs::new(&p, &Slides::default(), sr, false);
        let mut g = FuncGen::new();
        g.set(&c);
        let v0 = g.output();
        g.advance(&c, true, BLOCK_SIZE as u32);
        // A ramp moves 2·rate/fs per sample: the clamp over a block.
        let moved = g.output() - v0;
        assert_eq!(block_rate_max(sr), sr as f32 / BLOCK_SIZE as f32 / 8.0);
        let want = 2.0 * block_rate_max(sr) * BLOCK_SIZE as f32 / sr as f32;
        assert!((moved - want).abs() < 1e-4, "{sr}: {moved} vs {want}");
    }
}

/// Envelope B from the spec's formulas in f64, `t` samples after a
/// note-on from silence (ADR 0036's reference).
fn reference(f: Func, rise: f32, fall: f32, shape: f32, t: f64) -> f64 {
    let fs = SR as f64;
    let law = |lo: f64, hi: f64, x: f32| lo * (hi / lo).powf(x as f64);
    let frac = |x: f64| x - x.floor();
    let w = 2f64.powf(4.0 * (2.0 * shape as f64 - 1.0));
    let curve = |x: f64| x / (x + (1.0 - x) * w);
    let r = shape as f64;
    let tilt = |p: f64| {
        if p < r {
            p / r
        } else if r < 1.0 {
            (1.0 - p) / (1.0 - r)
        } else {
            p
        }
    };
    match f {
        Func::Env(form) => {
            let (tr, tf) = (law(2e-3, 5.0, rise) * fs, law(2e-3, 5.0, fall) * fs);
            let t = if form == EnvForm::Cycle {
                t % (tr + tf)
            } else {
                t
            };
            if t < tr {
                curve(t / tr)
            } else if t < tr + tf {
                1.0 - curve((t - tr) / tf)
            } else {
                0.0
            }
        }
        Func::Lfo(_) => 2.0 * tilt(frac(t * law(0.05, 800.0, rise) / fs + fall as f64)) - 1.0,
        // CYCLE: the burst repeats every LENGTH; its pulses are the tilting saw.
        Func::Burst(_) => {
            tilt(frac(t / (law(0.01, 20.0, fall) * fs)))
                * tilt(frac(t * law(0.05, 1000.0, rise) / fs))
        }
    }
}

/// The per-sample paths (`tick`, `fill`) and the per-block path
/// (`advance`) each stay within 1e-4 of the f64 reference, allowing ±1
/// sample at a turn (ADR 0036, superseding the spec's 1e-6).
#[test]
fn each_path_matches_an_f64_reference() {
    for (f, rise, fall, shape) in [
        (Func::Env(EnvForm::Cycle), 0.05, 0.08, 0.7),
        (Func::Env(EnvForm::Ad), 0.02, 0.1, 0.2),
        (Func::Lfo(LfoForm::Free), 0.4, 0.1, 0.3),
        (Func::Burst(EnvForm::Cycle), 0.5, 0.1, 0.6),
    ] {
        const BLOCKS: usize = 300;
        let refs: Vec<f64> = (0..BLOCKS * BLOCK_SIZE)
            .map(|n| reference(f, rise, fall, shape, (n + 1) as f64))
            .collect();
        let near = |n: usize, v: f32| {
            (n.saturating_sub(1)..=(n + 1).min(refs.len() - 1))
                .any(|m| (refs[m] - v as f64).abs() <= 1e-4)
        };
        let p = fp(f, rise, fall, shape);
        let c = BCoefs::new(&p, &Slides::default(), SR, true);
        let (mut ticked, mut filled, mut blocked) =
            (FuncGen::new(), FuncGen::new(), FuncGen::new());
        for g in [&mut ticked, &mut filled, &mut blocked] {
            g.set(&c);
            g.note_on(f);
        }
        for b in 0..BLOCKS {
            let mut buf = [0.0f32; BLOCK_SIZE];
            filled.fill(&c, true, &mut buf);
            blocked.advance(&c, true, BLOCK_SIZE as u32);
            for (i, &v) in buf.iter().enumerate() {
                let n = b * BLOCK_SIZE + i;
                let t = ticked.tick(&c, true);
                assert!(near(n, t), "tick {f:?} {n}: {t} vs {}", refs[n]);
                assert!(near(n, v), "fill {f:?} {n}: {v} vs {}", refs[n]);
            }
            let n = (b + 1) * BLOCK_SIZE - 1;
            assert!(
                near(n, blocked.output()),
                "advance {f:?} block {b}: {} vs {}",
                blocked.output(),
                refs[n]
            );
        }
    }
}

/// RISE, FALL and SHAPE routes move their sliders' positions.
#[test]
fn slides_move_the_sliders() {
    let p = fp(Func::Env(EnvForm::Ad), 0.3, 0.3, 0.5);
    let plain = BCoefs::new(&p, &Slides::default(), SR, true);
    for s in [
        Slides {
            rise: 0.2,
            ..Slides::default()
        },
        Slides {
            fall: -0.2,
            ..Slides::default()
        },
        Slides {
            shape: 0.3,
            ..Slides::default()
        },
    ] {
        assert_ne!(BCoefs::new(&p, &s, SR, true), plain, "{s:?}");
    }
}
