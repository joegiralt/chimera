//! VA sync, the sub and the five operators (spec § Combine, § Testing).

mod common;

use chimera_core::dsp::va::VaMode;
use chimera_core::dsp::va::combine::{
    self, Crusher, Operator, Peaks, bits, combine, ring_gain, with_sub, xor16, xor16_scaled,
};
use chimera_core::dsp::va::osc::{Correction, Morph, R, Sub, Unit, VarOsc, peak_bound};
use common::spectrum::{alias_db, alias_db_lattice, peak_hz};

const SR: f32 = 48_000.0;
const C5: f32 = 523.25;
const C6: f32 = 1046.50;
const GATE_LEN: usize = (1 << 12) + (1 << 15);
const GRID: [f32; 7] = [0.0, 1.0 / 6.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0];

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

/// `n + 1` points from `-hi` to `hi`.
fn span(hi: f32, n: usize) -> impl Iterator<Item = f32> {
    (0..=n).map(move |i| -hi + 2.0 * hi * i as f32 / n as f32)
}

fn crusher(x: f32) -> Crusher {
    let mut c = Crusher::NEW;
    c.set(x);
    c
}

#[test]
fn operators_are_the_table() {
    let pk = Peaks { a: 0.87, b: 0.35 };
    let k = ring_gain(pk);
    assert!((k - 0.9 / (0.87 * 0.35)).abs() <= 1e-6, "{k}");
    let steps = |hi: f32| ((2.0 * hi * 16.0) as usize).max(1);
    for xi in 0..=16 {
        let x = xi as f32 / 16.0;
        let crush = crusher(x);
        for a in span(pk.a, steps(pk.a)) {
            for b in span(pk.b, steps(pk.b)) {
                let mix = (1.0 - x) * a + x * b;
                let half = 0.5 * (a + b);
                let want = [
                    (VaMode::Mix, mix),
                    (VaMode::Sync, mix),
                    (VaMode::Ring, (1.0 - x) * half + x * k * a * b),
                    (
                        VaMode::Xor,
                        (1.0 - x) * half + x * 0.87 * xor16(a / 0.87, b / 0.87),
                    ),
                    (VaMode::Bits, 0.61 * crush.process(half / 0.61)),
                ];
                for (mode, w) in want {
                    let y = combine(mode, a, b, x, k, pk, &crush);
                    assert!(
                        (y - w).abs() <= 1e-6,
                        "{mode:?} a {a} b {b} x {x}: {y} vs {w}"
                    );
                }
            }
        }
    }
}

#[test]
fn xor16_works_on_words() {
    assert_eq!(xor16(0.5, 0.25), 8192.0 / 32768.0);
    for a in span(1.0, 64) {
        assert_eq!(xor16(a, a), 0.0, "{a}");
    }
    assert_eq!(xor16(-1.0, 0.0), -32767.0 / 32768.0);
    assert_eq!(xor16(3.0, 0.0), 32767.0 / 32768.0, "inputs clamp");
}

#[test]
fn bits_follow_the_log_curve() {
    assert!((bits(0.0) - 16.0).abs() <= 1e-4);
    assert!((bits(1.0 / 3.0) - 8.0).abs() <= 1e-4);
    assert!((bits(2.0 / 3.0) - 4.0).abs() <= 1e-4);
    assert!((bits(1.0) - 2.0).abs() <= 1e-4);
    let (lo, hi) = (crusher(0.0), crusher(1.0));
    for v in span(1.0, 4096) {
        let y = hi.process(v);
        assert!(
            [-1.0, -0.5, 0.0, 0.5, 1.0].contains(&y),
            "x 1: crush({v}) = {y}"
        );
        assert!((lo.process(v) - v).abs() <= 2f32.powi(-15), "x 0: {v}");
    }
}

/// The engine's loop runs the operators as `Operator` has them (block
/// constants folded, RING and XOR at twice the level, XOR's words by one
/// saturating convert): against the reference `combine`, RING and MIX are
/// the same bits (doubled), XOR's words are the reference's within the one
/// truncation step the folded scale can move and its output is their XOR
/// exactly, BITS lands within one level of the reference (the ½ and `1/q`
/// folded into the levels move only a tie).
#[test]
fn the_engine_operators_are_the_reference() {
    let mut bits_off = (0usize, 0usize);
    for pk in [
        Peaks { a: 0.87, b: 0.87 },
        Peaks { a: 0.26, b: 0.87 },
        Peaks { a: 0.35, b: 0.26 },
    ] {
        let k = ring_gain(pk);
        let p = pk.a.max(pk.b);
        let q = 0.5 * (pk.a + pk.b);
        for xi in 0..=8 {
            let x = xi as f32 / 8.0;
            let crush = crusher(x);
            for mode in VaMode::ALL {
                let op = Operator::new(mode, pk, &crush);
                for a in span(pk.a, 64) {
                    for b in span(pk.b, 64) {
                        let got = op.at(a, b, x, op.scale * k) / op.scale;
                        let want = combine(mode, a, b, x, k, pk, &crush);
                        match mode {
                            VaMode::Mix | VaMode::Sync | VaMode::Ring => {
                                assert_eq!(got.to_bits(), want.to_bits(), "{mode:?} {a} {b} {x}")
                            }
                            VaMode::Xor => {
                                // each word, read back through a XOR with 0
                                let word = |v: f32| {
                                    (xor16_scaled(v, 0.0, 32767.0 / p, p) * 32768.0 / p).round() as i32
                                };
                                let refw = |v: f32| (v / p * 32767.0) as i32;
                                for v in [a, b] {
                                    assert!((word(v) - refw(v)).abs() <= 1, "{v} of {p}");
                                }
                                let xr = ((word(a) ^ word(b)) << 16 >> 16) as f32 / 32768.0;
                                let half = 0.5 * (a + b);
                                let mine = half + (p * xr - half) * x;
                                assert!((got - mine).abs() <= 1e-6, "{a} {b} {x}: {got} {mine}");
                            }
                            VaMode::Bits => {
                                let step = q / crush.levels();
                                assert!((got - want).abs() <= step * 1.0001, "{a} {b} {x}");
                                bits_off.0 += usize::from(got != want);
                                bits_off.1 += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    println!("BITS off by a level at {} of {} points", bits_off.0, bits_off.1);
    assert!(bits_off.0 * 100 <= bits_off.1, "{bits_off:?}");
}

#[test]
fn every_operator_is_peak_safe() {
    let mut worst = 0.0f32;
    for pk in [
        Peaks { a: 0.87, b: 0.87 },
        Peaks { a: 0.28, b: 0.87 },
        Peaks { a: 0.2, b: 0.2 },
    ] {
        let k = ring_gain(pk);
        // BITS' levels are non-integer between the anchors, so x runs finer
        for xi in 0..=32 {
            let x = xi as f32 / 32.0;
            let crush = crusher(x);
            for a in span(pk.a, 128) {
                for b in span(pk.b, 128) {
                    for mode in VaMode::ALL {
                        let c = combine(mode, a, b, x, k, pk, &crush);
                        // 0.2222 is the sub mix's worst point (≈ 0.922)
                        for sub in [0.0, 0.2222, 0.5, 1.0] {
                            for s in [R, -R] {
                                let y = with_sub(c, s, sub).abs();
                                assert!(y <= 0.95, "{mode:?} {pk:?} x {x} a {a} b {b}: {y}");
                                worst = worst.max(y);
                            }
                        }
                    }
                }
            }
        }
    }
    println!("peak {worst:.4} (limit 0.95, margin {:.4})", 0.95 - worst);
}

#[test]
fn sync_restarts_the_slave() {
    let (mut m, mut s) = (osc(Correction::On), osc(Correction::On));
    let mut resets = 0;
    let mut worst = 0.0f32;
    for _ in 0..4000 {
        let wrap = m.tick(0.0123, morph(0.0, 0.0), None).wrap;
        s.tick(0.031, morph(1.0, 0.5), wrap);
        if let Some(t) = wrap {
            let e = (s.phase() - t * 0.031).abs();
            assert!(e <= 1e-6, "t {t}: phase {} vs {}", s.phase(), t * 0.031);
            worst = worst.max(e);
            resets += 1;
        }
    }
    println!("{resets} resets, worst {worst:.1e}");
    assert!(resets >= 48, "{resets}");
}

#[test]
fn sync_splits_around_the_reset() {
    let (mut m, mut s) = (osc(Correction::On), osc(Correction::On));
    m.set_phase(0.95);
    s.set_phase(0.80);
    let wrap = m.tick(0.125, morph(0.0, 0.0), None).wrap;
    let t = wrap.expect("the master wraps");
    assert!((t - 0.6).abs() <= 1e-5, "{t}");
    s.tick(0.25, morph(2.0 / 3.0, 1.0), wrap);
    let e = s.last_edges();
    let want = [(0.7, 0.5237, 0.0), (0.6, -0.1047, 0.0), (0.1, 0.5237, 0.0)];
    assert_eq!(e.len(), 3, "{e:?}");
    for (got, w) in e.iter().zip(want) {
        assert!(
            (got.0 - w.0).abs() <= 1e-4
                && (got.1 - w.1).abs() <= 1e-4
                && (got.2 - w.2).abs() <= 1e-4,
            "{e:?}"
        );
    }
    assert!((s.phase() - 0.15).abs() <= 1e-6, "{}", s.phase());
}

/// The slave's output, synced to a master at `hz`.
fn synced(m: Morph, hz: f32, ratio: f32, c: Correction) -> Vec<f32> {
    let (mut master, mut slave) = (osc(c), osc(c));
    let (im, is) = (master.inc_for(hz), slave.inc_for(hz * ratio));
    (0..GATE_LEN)
        .map(|_| {
            let wrap = master.tick(im, morph(0.0, 0.0), None).wrap;
            slave.tick(is, m, wrap).out
        })
        .collect()
}

#[test]
fn sync_passes_the_alias_gate() {
    let mut worst = (f64::MIN, 0.0, 0.0, 0.0);
    for ratio in [2f32.powf(7.0 / 12.0), 2.37] {
        for x in GRID {
            for s in [0.0, 0.5, 1.0] {
                let on = alias_db(&synced(morph(x, s), C6, ratio, Correction::On), SR, &[C6]);
                let off = alias_db(&synced(morph(x, s), C6, ratio, Correction::Off), SR, &[C6]);
                println!("×{ratio:.3} WAVE {x:.3} SHAPE {s}: {on:.1} dB (Off {off:.1})");
                assert!(on <= -45.0, "×{ratio} WAVE {x} SHAPE {s}: {on}");
                assert!(
                    on <= off - 12.0,
                    "×{ratio} WAVE {x} SHAPE {s}: {on} vs {off}"
                );
                if on > worst.0 {
                    worst = (on, ratio, x, s);
                }
            }
        }
    }
    println!(
        "worst {:.2} dB at ×{:.3} WAVE {:.3} SHAPE {}",
        worst.0, worst.1, worst.2, worst.3
    );
}

#[test]
fn ring_passes_the_alias_gate_detuned() {
    let crush = Crusher::NEW;
    let detune = 2f32.powf(6.0 / 1200.0);
    let mut worst = (f64::MIN, String::new());
    for ratio in [detune, 2.0 * detune] {
        for w1 in GRID {
            for w2 in [w1, 1.0 / 3.0] {
                for s in [0.0, 0.5] {
                    for x in [0.5, 1.0] {
                        let (m1, m2) = (morph(w1, s), morph(w2, s));
                        let (mut a, mut b) = (osc(Correction::On), osc(Correction::On));
                        let (i1, i2) = (a.inc_for(C5), b.inc_for(C5 * ratio));
                        let pk = Peaks {
                            a: peak_bound(m1, i1),
                            b: peak_bound(m2, i2),
                        };
                        let k = ring_gain(pk);
                        let y: Vec<f32> = (0..GATE_LEN)
                            .map(|_| {
                                let ya = a.tick(i1, m1, None).out;
                                let yb = b.tick(i2, m2, None).out;
                                combine(VaMode::Ring, ya, yb, x, k, pk, &crush)
                            })
                            .collect();
                        let db = alias_db_lattice(&y, SR, C5, C5 * ratio);
                        let at = format!("×{ratio:.4} WAVE {w1:.3}/{w2:.3} SHAPE {s} x {x}");
                        println!("{at}: {db:.1} dB");
                        assert!(db <= -45.0, "{at}: {db}");
                        if db > worst.0 {
                            worst = (db, at);
                        }
                    }
                }
            }
        }
    }
    println!("worst {:.2} dB at {}", worst.0, worst.1);
}

/// The sub under a master at `hz`, with the master's wrap ticks.
fn sub_under(hz: f32, n: usize) -> (Vec<f32>, Vec<usize>) {
    let (mut m, mut sub) = (osc(Correction::On), Sub::NEW);
    let inc = m.inc_for(hz);
    let mut wraps = Vec::new();
    let y = (0..n)
        .map(|i| {
            let wrap = m.tick(inc, morph(0.0, 0.0), None).wrap;
            if wrap.is_some() {
                wraps.push(i);
            }
            sub.tick(wrap)
        })
        .collect();
    (y, wraps)
}

#[test]
fn sub_is_an_octave_down() {
    let (y, wraps) = sub_under(440.0, 1 << 17);
    let cents = 1200.0 * (peak_hz(&y, SR, 220.0) / 220.0).log2();
    println!("sub at {cents:+.4} cents of 220 Hz");
    assert!(cents.abs() <= 1.0, "{cents}");
    // one sample late: a wrap in tick i lies between samples i − 1 and i,
    // which come out of ticks i and i + 1
    let sign = |v: f32| v > 0.0;
    for w in wraps.windows(2) {
        let (i, j) = (w[0], w[1]);
        if j + 1 >= y.len() {
            break;
        }
        assert!(
            y[i] != 0.0 && y[i + 1] != 0.0 && sign(y[i]) != sign(y[i + 1]),
            "no flip at {i}: {} {}",
            y[i],
            y[i + 1]
        );
        for (n, v) in y.iter().enumerate().take(j).skip(i + 2) {
            assert_eq!(v.abs(), R, "sample {n} between flips at {i} and {j}");
            assert_eq!(sign(*v), sign(y[i + 1]), "sample {n} flipped early");
        }
    }
}

#[test]
fn sub_passes_the_alias_gate() {
    let (y, _) = sub_under(C6, GATE_LEN);
    let db = alias_db(&y, SR, &[0.5 * C6]);
    println!("sub at C6/2: {db:.1} dB");
    assert!(db <= -45.0, "{db}");
}

#[test]
fn sub_carries_its_state() {
    let mut s = Sub::NEW;
    assert!(s.high());
    s.tick(Some(0.5));
    assert!(!s.high());
    s.set_high(true);
    assert!(s.high());
    s.tick(Some(0.5));
    s.reset();
    assert!(s.high());
    assert_eq!(s.tick(None), 0.0, "reset clears the held sample");
    assert_eq!(s.tick(None), R);
}

#[test]
fn xor_provenance_is_declared() {
    use chimera_core::dsp::provenance::{Patent, Provenance};
    assert!(combine::PROVENANCE.iter().any(
        |p| matches!(p, Provenance::Textbook { cite } if cite.contains("Kleimola") && cite.contains("DAFx-08"))
    ));
    assert!(
        combine::PATENT
            .iter()
            .any(|l| l.technique.contains("XOR") && l.patent == Patent::NotChecked)
    );
}
