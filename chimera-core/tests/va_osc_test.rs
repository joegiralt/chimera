//! The VA variable-wave oscillator (spec § Oscillator, § Testing).

mod common;

use chimera_core::dsp::va::blep::Corrector;
use chimera_core::dsp::va::osc::{
    self, Correction, MAX_INC, MIN_INC, Morph, R, Unit, VarOsc, peak_bound, wave_at,
};
use common::spectrum::{alias_db, fundamental, peak_hz, tri_reference};

const SR: f32 = 48_000.0;
const C1: f32 = 32.703;
const C2: f32 = 65.406;
const C4: f32 = 261.626;
const C6: f32 = 1046.50;
const C7: f32 = 2093.005;
const C8: f32 = 4186.009;
const A4: f32 = 440.0;
const GATE_LEN: usize = (1 << 12) + (1 << 15);
const GRID: [f32; 7] = [0.0, 1.0 / 6.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0];
const ANCHORS: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
const CYCLE: f32 = 1.0 / 4800.0;

fn morph(wave: f32, shape: f32) -> Morph {
    Morph {
        wave: Unit::new(wave),
        shape: Unit::new(shape),
    }
}

fn osc_at(sr: f32, c: Correction) -> VarOsc {
    let mut o = VarOsc::NEW.with_correction(c);
    o.prepare(sr);
    o
}

fn render_at(m: Morph, hz: f32, sr: f32, n: usize, c: Correction) -> Vec<f32> {
    let mut o = osc_at(sr, c);
    let inc = o.inc_for(hz);
    (0..n).map(|_| o.tick(inc, m, false, None).out).collect()
}

fn render(m: Morph, hz: f32, n: usize, c: Correction) -> Vec<f32> {
    render_at(m, hz, SR, n, c)
}

/// One cycle at inc 1/4800, sampled midway between the 1/4800 grid the
/// edges sit on (as the simulation did): (phase, sample) pairs.
fn cycle(m: Morph, c: Correction) -> Vec<(f32, f32)> {
    let mut o = osc_at(SR, c);
    o.set_phase(0.5 * CYCLE);
    o.tick(CYCLE, m, false, None);
    let mut p = o.phase();
    (0..4800)
        .map(|_| {
            let y = o.tick(CYCLE, m, false, None).out;
            let s = (p, y);
            p = o.phase();
            s
        })
        .collect()
}

fn ys(c: &[(f32, f32)]) -> Vec<f32> {
    c.iter().map(|&(_, y)| y).collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// Phase distance on the circle.
fn dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(1.0);
    d.min(1.0 - d)
}

#[test]
fn corrected_saw_beats_naive() {
    // 1 kHz is 48 samples a cycle at 48 kHz: every fold lands on a harmonic
    // and the gate masks it; a quarter-tone up keeps the folds apart.
    let hz = 1000.0 * 2f32.powf(1.0 / 24.0);
    let inc = hz / SR;
    let (mut p, mut c) = (0.0f32, Corrector::NEW);
    let (mut on, mut off) = (Vec::new(), Vec::new());
    for _ in 0..GATE_LEN {
        p += inc;
        if p >= 1.0 {
            p -= 1.0;
            c.edge(p / inc, 2.0, 0.0);
        }
        on.push(c.push(1.0 - 2.0 * p));
        off.push(1.0 - 2.0 * p);
    }
    let (a, b) = (alias_db(&on, SR, &[hz]), alias_db(&off, SR, &[hz]));
    println!("saw: corrected {a:.1} dB, naive {b:.1} dB");
    assert!(a <= -45.0 && a <= b - 12.0, "{a} vs {b}");
}

#[test]
fn anchors_are_their_plain_selves() {
    let tri = |p: f32| {
        if p < 0.25 {
            4.0 * p
        } else if p < 0.75 {
            2.0 - 4.0 * p
        } else {
            4.0 * p - 4.0
        }
    };
    type Plain = (f32, &'static [f32], fn(f32, &dyn Fn(f32) -> f32) -> f32);
    let cases: [Plain; 4] = [
        (0.0, &[], |p, _| 0.2828 * (std::f32::consts::TAU * p).sin()),
        (1.0 / 3.0, &[0.25, 0.75], |p, t| 0.3464 * t(p)),
        (2.0 / 3.0, &[0.0], |p, _| 0.3464 * (1.0 - 2.0 * p)),
        (1.0, &[0.0, 0.5], |p, _| if p < 0.5 { 0.2 } else { -0.2 }),
    ];
    for (x, edges, f) in cases {
        let mut worst = 0.0f32;
        for (p, y) in cycle(morph(x, 0.0), Correction::Off) {
            if edges.iter().any(|&e| dist(p, e) < 2.0 * CYCLE) {
                continue;
            }
            worst = worst.max((y - f(p, &tri)).abs());
        }
        println!("WAVE {x:.3}: max err {worst:.1e}");
        assert!(worst <= 1e-4, "WAVE {x}: {worst}");
    }
}

#[test]
fn wave_morph_is_a_linear_crossfade() {
    let n = 4000;
    let r = |x| render(morph(x, 0.3), C6, n, Correction::Off);
    let (sine, tri, saw) = (r(0.0), r(1.0 / 3.0), r(2.0 / 3.0));
    for (x, a, b) in [(1.0 / 6.0, &sine, &tri), (0.5, &tri, &saw)] {
        for (i, y) in r(x).iter().enumerate() {
            let want = 0.5 * a[i] + 0.5 * b[i];
            assert!((y - want).abs() <= 1e-6, "WAVE {x} at {i}: {y} vs {want}");
        }
    }
}

#[test]
fn shape_does_what_the_table_says() {
    // sine: 0.3069·sin(3 sin θ)
    for (p, y) in cycle(morph(0.0, 1.0), Correction::Off) {
        let want = 0.3069 * (3.0 * (std::f32::consts::TAU * p).sin()).sin();
        assert!((y - want).abs() <= 1e-3, "sine at {p}: {y} vs {want}");
    }
    // triangle: trough at 0.525, peak at 0.475
    let tri = cycle(morph(1.0 / 3.0, 1.0), Correction::Off);
    let lo = tri.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0;
    let hi = tri.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0;
    assert!(
        dist(lo, 0.525) <= CYCLE && dist(hi, 0.475) <= CYCLE,
        "{lo} {hi}"
    );
    // pulse: high on [0.225, 0.275)
    for (p, y) in cycle(morph(1.0, 1.0), Correction::Off) {
        if dist(p, 0.225).min(dist(p, 0.275)) < 2.0 * CYCLE {
            continue;
        }
        assert_eq!(y > 0.0, (0.225..0.275).contains(&p), "pulse at {p}: {y}");
    }
    // saw: steps at 0.875 and 0.125, each 0.5237 high
    let jumps = |c: &[(f32, f32)]| -> Vec<(f32, f32)> {
        (0..c.len())
            .map(|i| (c[i], c[(i + 1) % c.len()]))
            .filter(|(a, b)| b.1 - a.1 > 0.1)
            .map(|(a, b)| {
                (
                    0.5 * (a.0 + b.0 + if b.0 < a.0 { 1.0 } else { 0.0 }),
                    b.1 - a.1,
                )
            })
            .collect()
    };
    let saw = jumps(&cycle(morph(2.0 / 3.0, 1.0), Correction::Off));
    assert_eq!(saw.len(), 2, "{saw:?}");
    for (at, (p, h)) in [0.125, 0.875].iter().zip(&saw) {
        assert!(
            dist(*p, *at) <= CYCLE && (h - 0.5237).abs() <= 1e-3,
            "{saw:?}"
        );
    }
    // SHAPE 0 is the plain saw: one step of 2·R√3 at 0
    let plain = jumps(&cycle(morph(2.0 / 3.0, 0.0), Correction::Off));
    assert_eq!(plain.len(), 1);
    assert!(dist(plain[0].0, 0.0) <= CYCLE && (plain[0].1 - 0.6928).abs() <= 1e-3);
}

#[test]
fn level_holds_across_shape() {
    let (mut lv, mut dc, mut pk) = (0.0f64, 0.0f64, 0.0f32);
    for x in ANCHORS {
        for i in 0..=20 {
            let y = ys(&cycle(morph(x, i as f32 / 20.0), Correction::Off));
            let l = db(rms(&y) / f64::from(R));
            let mean = y.iter().map(|&v| f64::from(v)).sum::<f64>() / y.len() as f64;
            let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
            assert!(l.abs() <= 1.0, "WAVE {x} SHAPE {i}/20: {l:.3} dB");
            assert!(
                mean.abs() <= 1e-3 * f64::from(R),
                "WAVE {x} SHAPE {i}/20: DC {mean}"
            );
            assert!(peak <= 0.9, "WAVE {x} SHAPE {i}/20: peak {peak}");
            (lv, dc, pk) = (
                lv.max(l.abs()),
                dc.max(mean.abs() / f64::from(R)),
                pk.max(peak),
            );
        }
    }
    println!("level ±{lv:.4} dB, |DC| {dc:.1e}·R, peak {pk:.3}");
}

#[test]
fn level_across_wave() {
    for s in [0.0, 0.5, 1.0] {
        let worst = (0..=60)
            .map(
                |i| db(rms(&ys(&cycle(morph(i as f32 / 60.0, s), Correction::Off))) / f64::from(R)),
            )
            .fold(f64::MAX, f64::min);
        println!("SHAPE {s}: lowest level {worst:.2} dB");
        assert!(worst >= -2.5, "SHAPE {s}: {worst}");
    }
}

/// |F| and the in-phase part of one cycle's fundamental.
fn fund(m: Morph) -> (f64, f64) {
    let (re, im) = fundamental(&ys(&cycle(m, Correction::Off)), SR, SR * CYCLE);
    (f64::from(re).hypot(f64::from(im)), -f64::from(im))
}

#[test]
fn fundamental_follows_the_anchors() {
    for s in [0.0, 0.5, 1.0] {
        let anchor: Vec<f64> = ANCHORS.iter().map(|&x| fund(morph(x, s)).0).collect();
        let mut worst = f64::MAX;
        for i in 0..=60 {
            let x = i as f32 / 60.0;
            let k = ((3.0 * x) as usize).min(2);
            let f = f64::from(3.0 * x) - k as f64;
            let (mag, inphase) = fund(morph(x, s));
            let rel = db(mag / ((1.0 - f) * anchor[k] + f * anchor[k + 1]));
            worst = worst.min(rel);
            assert!(
                rel >= -1.0 && inphase > 0.0,
                "SHAPE {s} WAVE {x}: {rel} dB, {inphase}"
            );
        }
        println!("SHAPE {s}: fundamental vs anchor lerp, worst {worst:+.3} dB");
    }
}

#[test]
fn split_saw_keeps_its_fundamental() {
    let d = db(fund(morph(2.0 / 3.0, 1.0)).0 / fund(morph(2.0 / 3.0, 0.0)).0);
    println!("split saw fundamental {d:+.3} dB");
    assert!(d.abs() <= 1.0, "{d}");
}

#[test]
fn in_tune_at_every_rate() {
    let mut worst = 0.0f64;
    for sr in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for x in GRID {
            let y = render_at(morph(x, 0.0), A4, sr, 1 << 17, Correction::On);
            let cents = 1200.0 * (peak_hz(&y, sr, A4) / 440.0).log2();
            worst = worst.max(cents.abs());
            assert!(cents.abs() <= 1.0, "{sr} Hz, WAVE {x}: {cents} cents");
        }
    }
    println!("worst {worst:.4} cents");
}

/// Corrected and uncorrected alias at `hz`.
fn gate(m: Morph, hz: f32) -> (f64, f64) {
    let on = alias_db(&render(m, hz, GATE_LEN, Correction::On), SR, &[hz]);
    let off = alias_db(&render(m, hz, GATE_LEN, Correction::Off), SR, &[hz]);
    (on, off)
}

#[test]
fn alias_gate_at_c6() {
    let mut worst = (f64::MIN, 0.0, 0.0);
    for x in GRID {
        for s in [0.0, 0.5, 1.0] {
            let (on, off) = gate(morph(x, s), C6);
            println!("WAVE {x:.3} SHAPE {s}: {on:.1} dB (Off {off:.1})");
            assert!(on <= -45.0, "WAVE {x} SHAPE {s}: {on}");
            assert!(
                off <= -60.0 || on <= off - 12.0,
                "WAVE {x} SHAPE {s}: {on} vs {off}"
            );
            if on > worst.0 {
                worst = (on, x, s);
            }
        }
    }
    println!(
        "worst {:.1} dB at WAVE {:.3} SHAPE {}",
        worst.0, worst.1, worst.2
    );
}

#[test]
fn edges_never_overshoot() {
    let mut pk = 0.0f32;
    for hz in [C2, C4, C6, C7] {
        for x in GRID {
            for s in [0.0, 0.5, 1.0] {
                let y = render(morph(x, s), hz, 8192, Correction::On);
                let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                assert!(peak <= 0.9, "{hz} Hz WAVE {x} SHAPE {s}: {peak}");
                pk = pk.max(peak);
            }
        }
    }
    println!("peak {pk:.3}");
}

#[test]
fn narrow_pulse_and_split_saw_pass_high() {
    for (name, x) in [("pulse", 1.0), ("split saw", 2.0 / 3.0)] {
        for hz in [C7, C8] {
            let (on, off) = gate(morph(x, 1.0), hz);
            println!("{name} at {hz} Hz: {on:.1} dB (Off {off:.1})");
            assert!(
                on <= -45.0 && on <= off - 12.0,
                "{name} {hz}: {on} vs {off}"
            );
        }
    }
}

#[test]
fn pulse_never_narrower_than_a_sample() {
    let mut o = osc_at(SR, Correction::On);
    let inc = o.inc_for(C8);
    assert!((inc - 0.0872).abs() < 1e-4);
    let (mut rise, mut runs) = (None, 0);
    for n in 0..4096 {
        o.tick(inc, morph(1.0, 1.0), false, None);
        for &(t, step, _) in o.last_edges() {
            let at = n as f32 - t;
            if step > 0.0 {
                rise = Some(at);
            } else if let Some(r) = rise.take() {
                assert!(at - r >= 1.0 - 1e-3, "high for {} samples", at - r);
                runs += 1;
            }
        }
    }
    assert!(runs > 300, "{runs}");
}

/// The oscillator under per-sample settings `set(t)` from phase `p0`,
/// against the triangle-kernel reference along its own phase: max error.
fn tracks(inc: f32, p0: f32, n: usize, set: impl Fn(f64) -> Morph, skip: Option<usize>) -> f64 {
    let mut o = osc_at(SR, Correction::On);
    o.set_phase(p0);
    if let Some(k) = skip {
        o.skip_edge(k);
    }
    let mut phase = vec![f64::from(p0)];
    let mut y = vec![0.0f32];
    for i in 1..=n + 1 {
        y.push(o.tick(inc, set(i as f64), true, None).out);
        phase.push(f64::from(o.phase()));
    }
    let f = |t: f64| {
        let i = (t.max(0.0) as usize).min(n);
        let p = (phase[i] + f64::from(inc) * (t - i as f64)).rem_euclid(1.0);
        f64::from(wave_at(p as f32, set(t), inc).0)
    };
    let reference = tri_reference(f, n + 1);
    (3..=n)
        .map(|k| (f64::from(y[k]) - reference[k]).abs())
        .fold(0.0, f64::max)
}

const SWEEP: usize = 4800;

fn ramp(t: f64) -> f32 {
    Unit::new((t / SWEEP as f64) as f32).get()
}

#[test]
fn morphing_tracks_the_reference() {
    let inc = A4 / SR;
    let mut worst = 0.0f64;
    for s in [0.0, 0.5, 1.0] {
        let e = tracks(inc, 0.123, SWEEP, |t| morph(ramp(t), s), None);
        println!("WAVE sweep at SHAPE {s}: {e:.2e}");
        assert!(e <= 2e-3, "WAVE sweep at SHAPE {s}: {e}");
        worst = worst.max(e);
    }
    for x in [0.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0] {
        let e = tracks(inc, 0.123, SWEEP, |t| morph(x, ramp(t)), None);
        println!("SHAPE sweep at WAVE {x:.3}: {e:.2e}");
        assert!(e <= 2e-3, "SHAPE sweep at WAVE {x}: {e}");
        worst = worst.max(e);
    }
    println!("worst {worst:.2e}");
}

#[test]
fn the_reference_catches_a_step() {
    let e = tracks(
        A4 / SR,
        0.123,
        SWEEP,
        |t| morph(2.0 / 3.0, ramp(t)),
        Some(20),
    );
    println!("one uncorrected step: {e:.2e}");
    assert!(e > 2e-3, "{e}");
}

#[test]
fn a_threshold_crossing_backwards_is_an_edge() {
    let mut o = osc_at(SR, Correction::On);
    o.tick(MIN_INC, morph(1.0, 1.0), true, None);
    o.set_phase(0.28);
    o.tick(MIN_INC, morph(1.0, 0.9), true, None);
    let e = o.last_edges();
    assert_eq!(e.len(), 1, "{e:?}");
    let (t, step, _) = e[0];
    assert!((t - 0.778).abs() <= 1e-3, "{t}");
    assert!((0.682..=0.918).contains(&step), "{step}");
}

#[test]
fn a_corner_crossed_backwards_bends_its_own_way() {
    // the triangle's trough (−w/2) moves forward past the phase as SHAPE
    // falls 1 → 0.9; a trough bends up whichever way it is crossed
    let mut o = osc_at(SR, Correction::On);
    o.tick(MIN_INC, morph(1.0 / 3.0, 1.0), true, None);
    o.set_phase(0.53);
    o.tick(MIN_INC, morph(1.0 / 3.0, 0.9), true, None);
    let e = o.last_edges();
    assert_eq!(e.len(), 1, "{e:?}");
    let (t, step, slope) = e[0];
    assert!((t - 0.778).abs() <= 1e-3, "{t}");
    assert_eq!(step, 0.0);
    // j·|r|: j from 2R√3/(w(1 − w)) at w 0.905 to 0.95, |r| 0.0225
    assert!(slope > 0.0 && (0.181..=0.329).contains(&slope), "{slope}");
}

#[test]
fn fast_shape_at_c1_tracks_the_reference() {
    let shape = |t: f64| {
        let m = t.rem_euclid(480.0);
        (if m < 48.0 {
            m / 48.0
        } else if m < 240.0 {
            1.0
        } else if m < 288.0 {
            1.0 - (m - 240.0) / 48.0
        } else {
            0.0
        }) as f32
    };
    for x in [1.0, 2.0 / 3.0, 1.0 / 3.0, 5.0 / 6.0] {
        let e = tracks(C1 / SR, 0.3, 4800, |t| morph(x, shape(t)), None);
        println!("C1, 1 ms SHAPE ramps at WAVE {x:.3}: {e:.2e}");
        assert!(e <= 0.02, "WAVE {x}: {e}");
    }
}

#[test]
fn units_clamp_and_increments_bound() {
    assert_eq!(Unit::new(f32::NAN).get(), 0.0);
    assert_eq!(Unit::new(-0.3).get(), 0.0);
    assert_eq!(Unit::new(1.7).get(), 1.0);
    let o = osc_at(SR, Correction::On);
    assert_eq!(o.inc_for(1e9), MAX_INC);
    assert_eq!(o.inc_for(0.0), MIN_INC);
    assert_eq!(o.inc_for(f32::NAN), MIN_INC);
}

#[test]
fn peak_bound_covers_the_wave() {
    for x in GRID {
        for s in [0.0, 0.5, 1.0] {
            let m = morph(x, s);
            let peak = (0..4800)
                .map(|i| wave_at(i as f32 / 4800.0, m, CYCLE).0.abs())
                .fold(0.0f32, f32::max);
            assert!(peak <= peak_bound(m, CYCLE) + 1e-6, "WAVE {x} SHAPE {s}");
            assert!(peak_bound(m, CYCLE) <= 0.9);
        }
    }
}

#[test]
fn provenance_is_declared() {
    use chimera_core::dsp::provenance::Provenance;
    assert!(
        osc::PROVENANCE
            .iter()
            .any(|p| matches!(p, Provenance::Textbook { .. }))
    );
    assert!(
        osc::PROVENANCE
            .iter()
            .any(|p| matches!(p, Provenance::Original { .. }))
    );
    assert!(osc::PATENT.is_empty());
    assert!(!osc::GPL_CHECK.is_empty());
}
