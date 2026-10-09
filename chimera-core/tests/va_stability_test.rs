//! VA STABILITY: drift, trim, HF tracking, scale error, jitter, seeds
//! (spec § Stability, § Testing). Bounds are the plan's; the simulated
//! values are in brackets beside each.

use chimera_core::dsp::clampf;
use chimera_core::dsp::va::stability::{
    Character, DRIFT_OWN_CENTS, DRIFT_SHARED_CENTS, JITTER_CENTS, SALT_SCALE, SALT_TAU, SALT_TRIM,
    Seed, Stability, TRIM_CENTS, depth,
};

const RATES: [f32; 4] = [
    44_100.0 / 64.0,
    48_000.0 / 64.0,
    96_000.0 / 64.0,
    192_000.0 / 64.0,
];
const BLOCK: f32 = 48_000.0 / 64.0;
const C7: f32 = 2093.0;
const C4: f32 = 261.63;
const A2: f32 = 110.0;

fn stab(rate: f32, s: Seed) -> Stability {
    let mut st = Stability::NEW;
    st.prepare(rate);
    st.seed(s);
    for _ in 0..(30.0 * rate) as usize {
        st.step(1.0);
    }
    st
}

/// Walk 0's rms, hysteretic (±0.1) crossings a second and clamp fraction.
fn walk_stats(rate: f32, s: Seed, secs: f32) -> (f64, f64, f64) {
    let mut st = stab(rate, s);
    let n = (secs * rate) as usize;
    let (mut sq, mut cross, mut state, mut clip) = (0.0f64, 0usize, 0i32, 0usize);
    for _ in 0..n {
        st.step(1.0);
        let raw = st.walk_raw(0);
        let v = clampf(raw, -1.0, 1.0);
        sq += (v as f64) * (v as f64);
        if raw.abs() > 1.0 {
            clip += 1;
        }
        let next = if v > 0.1 {
            1
        } else if v < -0.1 {
            -1
        } else {
            state
        };
        if next != state && state != 0 {
            cross += 1;
        }
        state = next;
    }
    (
        (sq / n as f64).sqrt(),
        cross as f64 / secs as f64,
        clip as f64 / n as f64,
    )
}

fn cents(f: f32, f0: f32) -> f64 {
    1200.0 * (f as f64 / f0 as f64).log2()
}

#[test]
fn depth_curve() {
    assert_eq!(depth(1.0), 0.0);
    assert_eq!(depth(0.0), 1.0);
    assert_eq!(depth(0.5), 0.25);
    assert!((depth(0.7) - 0.09).abs() <= 1e-6, "{}", depth(0.7));
    assert_eq!(depth(f32::NAN), 1.0);
}

#[test]
fn walk_is_the_band() {
    for s in 0..8 {
        let (rms, cr, clip) = walk_stats(BLOCK, Seed::of(0, s), 600.0);
        println!("seed {s}: rms {rms:.3} crossings/s {cr:.3} clamp {clip:.4}");
        assert!((0.25..=0.35).contains(&rms), "seed {s}: rms {rms}");
        assert!((0.12..=0.40).contains(&cr), "seed {s}: crossings {cr}");
        assert!(clip <= 0.015, "seed {s}: clamp {clip}");
    }
}

#[test]
fn walks_are_rate_independent() {
    let means: Vec<(f64, f64)> = RATES
        .iter()
        .map(|&rate| {
            let (mut r, mut c) = (0.0, 0.0);
            for s in 0..8 {
                let (rms, cr, _) = walk_stats(rate, Seed::of(0, s), 600.0);
                r += rms / 8.0;
                c += cr / 8.0;
            }
            (r, c)
        })
        .collect();
    let (r48, c48) = means[1];
    for (rate, &(r, c)) in RATES.iter().zip(&means) {
        println!(
            "rate {rate}: rms {r:.4} ({:+.1}%), crossings {c:.3} ({:+.1}%)",
            100.0 * (r / r48 - 1.0),
            100.0 * (c / c48 - 1.0)
        );
        assert!(
            (r / r48 - 1.0).abs() <= 0.10,
            "rate {rate}: rms {r} vs {r48}"
        );
        assert!(
            (c / c48 - 1.0).abs() <= 0.20,
            "rate {rate}: crossings {c} vs {c48}"
        );
    }
}

#[test]
fn drift_has_its_depth() {
    let mut shared_sq = 0.0f64;
    let mut n = 0usize;
    for s in 0..8 {
        let seed = Seed::of(0, s);
        let mut st = stab(BLOCK, seed);
        let ch = Character::of(seed);
        let walk = |st: &Stability, k: usize| clampf(st.walk_raw(k), -1.0, 1.0);
        for _ in 0..(600.0 * BLOCK) as usize {
            let c = st.step(1.0);
            let w0 = walk(&st, 0);
            shared_sq += (w0 as f64 * DRIFT_SHARED_CENTS as f64).powi(2);
            n += 1;
            for k in 0..2 {
                let want = DRIFT_SHARED_CENTS * w0
                    + DRIFT_OWN_CENTS * walk(&st, k + 1)
                    + TRIM_CENTS * ch.trim[k];
                assert!((c[k] - want).abs() <= 1e-4, "osc {k}: {} vs {want}", c[k]);
            }
        }
    }
    let rms = (shared_sq / n as f64).sqrt();
    println!("shared drift rms {rms:.3} cents");
    assert!((3.0..=4.2).contains(&rms), "{rms}");

    // depth is linear: the same walks at d = 0.25 give a quarter
    let (mut a, mut b) = (stab(BLOCK, Seed::of(1, 2)), stab(BLOCK, Seed::of(1, 2)));
    let (mut sa, mut sb) = (0.0f64, 0.0f64);
    for _ in 0..(600.0 * BLOCK) as usize {
        let (x, y) = (a.step(1.0), b.step(0.25));
        for k in 0..2 {
            sa += (x[k] as f64).powi(2);
            sb += (y[k] as f64).powi(2);
        }
    }
    let ratio = (sb / sa).sqrt();
    println!("d 0.25 / d 1 rms {ratio:.6}");
    assert!((ratio / 0.25 - 1.0).abs() <= 1e-3, "{ratio}");
}

#[test]
fn the_pair_drifts_together() {
    let mut st = stab(BLOCK, Seed::of(0, 3));
    let (mut sa, mut sb, mut sab, mut saa, mut sbb) = (0.0f64, 0.0, 0.0, 0.0, 0.0);
    let n = (600.0 * BLOCK) as usize;
    for _ in 0..n {
        let [a, b] = st.step(1.0).map(f64::from);
        sa += a;
        sb += b;
        sab += a * b;
        saa += a * a;
        sbb += b * b;
    }
    let nn = n as f64;
    let corr = (sab / nn - sa * sb / nn / nn)
        / ((saa / nn - (sa / nn).powi(2)).sqrt() * (sbb / nn - (sb / nn).powi(2)).sqrt());
    println!("corr {corr:.3}");
    assert!(corr >= 0.6, "{corr}");
}

#[test]
fn seeds_spread() {
    let (mut min_std, mut min_gap) = (9.0f32, 9.0f32);
    for salts in [SALT_TRIM, SALT_TAU, SALT_SCALE] {
        for salt in salts {
            for part in 0..8 {
                let t: Vec<f32> = (0..8).map(|v| Seed::of(part, v).draw(salt)).collect();
                let m = t.iter().sum::<f32>() / 8.0;
                let sd = (t.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / 8.0).sqrt();
                assert!(sd >= 0.25, "salt {salt:#x} part {part}: std {sd}");
                min_std = min_std.min(sd);
                for i in 0..8 {
                    for j in 0..i {
                        let g = (t[i] - t[j]).abs();
                        assert!(g > 1e-3, "salt {salt:#x} part {part}: {i} {j} gap {g}");
                        min_gap = min_gap.min(g);
                    }
                }
            }
        }
    }
    let mut sum = 0.0f64;
    for part in 0..16 {
        for slot in 0..128 {
            sum += Seed::of(part, slot).draw(SALT_TRIM[0]) as f64;
        }
    }
    let mean = sum / 2048.0;
    println!("min std {min_std:.3}, min gap {min_gap:.4}, 2048 trims mean {mean:.4}");
    assert!(mean.abs() <= 0.05, "{mean}");
}

#[test]
fn jitter_in_bounds() {
    for d in [1.0f32, 0.25] {
        let mut st = Stability::NEW;
        st.prepare(BLOCK);
        st.seed(Seed::of(3, 5));
        let mut sum = 0.0f64;
        let mut worst = 0.0f32;
        for _ in 0..100_000 {
            let j = st.jitter(d);
            assert!(j.abs() <= JITTER_CENTS * d, "d {d}: {j}");
            worst = worst.max(j.abs());
            sum += j as f64;
        }
        let mean = sum / 1e5;
        println!("d {d}: max |jitter| {worst:.4}, mean {mean:.5}");
        assert!(mean.abs() <= 0.02 * d as f64, "d {d}: mean {mean}");
    }
}

#[test]
fn hf_tracking() {
    let (mut lo, mut hi, mut a2, mut pairs) = (0.0f64, 0.0f64, 0.0f64, 0);
    for i in 0..32 {
        let seed = Seed::of(i / 8, i % 8);
        let mut st = Stability::NEW;
        st.prepare(BLOCK);
        st.seed(seed);
        let u = st.character().tau_u;
        for x in u {
            assert!((-0.25..=1.0).contains(&x), "seed {i}: u {x}");
        }
        let c7 = st.tracked([C7; 2], 1.0);
        let at_a2 = st.tracked([A2; 2], 1.0);
        for k in 0..2 {
            let c = cents(c7[k], C7);
            assert!((-14.5..=3.7).contains(&c), "seed {i} osc {k}: C7 {c}");
            lo = lo.min(c);
            hi = hi.max(c);
            let e = cents(at_a2[k], A2);
            assert!(e.abs() <= 1.0, "seed {i} osc {k}: A2 {e}");
            a2 = a2.max(e.abs());
        }
        assert_eq!(
            st.tracked([C7, A2], 0.0).map(f32::to_bits),
            [C7.to_bits(), A2.to_bits()]
        );
        if (u[0] - u[1]).abs() > 0.05 {
            let c4 = st.tracked([C4; 2], 1.0);
            let beat = |f: [f32; 2]| (f[0] as f64 - f[1] as f64).abs();
            assert!(
                beat(c7) > beat(c4),
                "seed {i}: {} vs {}",
                beat(c7),
                beat(c4)
            );
            pairs += 1;
        }
    }
    println!("C7 {lo:.2}..{hi:.2} cents, A2 max |e| {a2:.3}, {pairs} pairs beat faster at C7");
    // the extremes, from u
    for (u, want) in [(1.0f64, -14.43), (-0.25, 3.63)] {
        let c = -1200.0 * (1.0 + 2093.0 * 4e-6 * u).log2();
        assert!((c - want).abs() < 0.01, "u {u}: {c}");
    }
}

#[test]
fn scale_error() {
    let mut worst = [0.0f32; 2];
    for i in 0..32 {
        let mut st = Stability::NEW;
        st.prepare(BLOCK);
        st.seed(Seed::of(i / 8, i % 8));
        assert_eq!(st.scale_cents([6000.0; 2], 1.0), [0.0; 2]);
        for x in st.scale_cents([9600.0; 2], 1.0) {
            assert!(x.abs() <= 7.2, "seed {i}: C7 {x}");
            worst[0] = worst[0].max(x.abs());
        }
        for x in st.scale_cents([4500.0; 2], 1.0) {
            assert!(x.abs() <= 3.0, "seed {i}: A2 {x}");
            worst[1] = worst[1].max(x.abs());
        }
        let z = st.scale_cents([9600.0, 4500.0], 0.0);
        assert_eq!(z.map(f32::to_bits), [0.0f32.to_bits(); 2]);
    }
    println!(
        "scale error max |C7| {:.3}, |A2| {:.3} cents",
        worst[0], worst[1]
    );
}

#[test]
fn hf_tracking_is_rate_independent() {
    let seed = Seed::of(2, 6);
    let at = |rate: f32| {
        let mut st = Stability::NEW;
        st.prepare(rate);
        st.seed(seed);
        st.tracked([C7; 2], 1.0).map(f32::to_bits)
    };
    for rate in RATES {
        assert_eq!(at(rate), at(BLOCK), "rate {rate}");
    }
}

#[test]
fn character_is_the_seeds() {
    let mut st = Stability::NEW;
    st.prepare(BLOCK);
    st.seed(Seed::of(2, 3));
    for _ in 0..10_000 {
        st.step(0.5);
        st.jitter(0.5);
    }
    assert_eq!(st.character(), Character::of(Seed::of(2, 3)));
    assert_ne!(st.character(), Character::of(Seed::of(1, 3)));
}

#[test]
fn depth_zero_is_silent_bits() {
    let mut st = Stability::NEW;
    st.prepare(BLOCK);
    st.seed(Seed::of(4, 1));
    for _ in 0..10_000 {
        assert_eq!(st.step(0.0).map(f32::to_bits), [0.0f32.to_bits(); 2]);
        assert_eq!(st.jitter(0.0).to_bits(), 0.0f32.to_bits());
    }
}
