//! The master compressor (FX diet spec § Master comp, § Testing).

use chimera_core::dsp::comp::{CompParams, MasterComp, STEP};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const PAIRS: usize = 3;

type Out = [[f32; 2 * BLOCK_SIZE]; PAIRS];

fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

/// Each channel's samples (pair-major, L then R) through `comp`, `p(block)`
/// each block; the same layout back.
fn run(
    comp: &mut MasterComp,
    ch: &[Vec<f32>; 2 * PAIRS],
    p: impl Fn(usize) -> CompParams,
) -> [Vec<f32>; 2 * PAIRS] {
    let n = ch[0].len();
    let mut got: [Vec<f32>; 2 * PAIRS] = Default::default();
    for b in 0..n / BLOCK_SIZE {
        let mut out: Out = core::array::from_fn(|k| {
            core::array::from_fn(|i| ch[2 * k + i % 2][b * BLOCK_SIZE + i / 2])
        });
        comp.process(&mut out, &p(b), SR);
        for (c, g) in got.iter_mut().enumerate() {
            g.extend((0..BLOCK_SIZE).map(|i| out[c / 2][2 * i + c % 2]));
        }
    }
    got
}

/// `x` on pair 1 left, silence elsewhere.
fn pair1(x: Vec<f32>) -> [Vec<f32>; 2 * PAIRS] {
    let n = x.len();
    let mut ch: [Vec<f32>; 2 * PAIRS] = core::array::from_fn(|_| vec![0.0; n]);
    ch[0] = x;
    ch
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// THRESH −20 dB, RATIO `r` (an index into `RATIOS`), default timing, MIX 1.
fn hard(r: u8) -> CompParams {
    CompParams {
        thresh: 0.5,
        ratio: r,
        ..CompParams::default()
    }
}

/// Spec § Testing: bypassed it is bit-identical: the defaults (1:1, no
/// makeup), and MIX 0 at any ratio.
#[test]
fn bypass_is_bit_identical() {
    let ch: [Vec<f32>; 2 * PAIRS] =
        core::array::from_fn(|c| noise(32 * BLOCK_SIZE, 1.5, 7 + c as u32));
    let off = CompParams {
        mix: 0.0,
        ..hard(7)
    };
    for p in [CompParams::default(), off] {
        let mut comp = Box::new(MasterComp::new());
        assert_eq!(run(&mut comp, &ch, |_| p), ch, "{p:?}");
        assert_eq!(comp.gr_db(), 0.0);
        assert!(!comp.is_running());
    }
}

/// Bypassed, it runs nothing: no pass touches the samples. A signalling
/// NaN keeps its exact bits, which any arithmetic on it would quiet.
#[test]
fn bypass_runs_nothing() {
    let snan = f32::from_bits(0x7f80_0001);
    let off = CompParams {
        mix: 0.0,
        ..hard(7)
    };
    for p in [CompParams::default(), off] {
        let mut out: Out = [[snan; 2 * BLOCK_SIZE]; PAIRS];
        let mut comp = Box::new(MasterComp::new());
        comp.process(&mut out, &p, SR);
        assert!(
            out.iter().flatten().all(|s| s.to_bits() == snan.to_bits()),
            "{p:?}"
        );
        assert!(!comp.is_running());
    }
}

/// Switched off (1:1), it releases, then fades to an exact bypass.
#[test]
fn it_returns_to_an_exact_bypass() {
    let sec = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let ch = pair1(noise(3 * sec, 1.0, 9));
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &ch, |b| {
        if b * BLOCK_SIZE < sec {
            hard(7)
        } else {
            hard(0)
        }
    });
    assert_ne!(out[0][..sec], ch[0][..sec]);
    assert_eq!(out[0][2 * sec..], ch[0][2 * sec..]);
    assert_eq!(comp.gr_db(), 0.0);
    assert!(!comp.is_running(), "stopped once released");
}

/// Spec § Testing: above the threshold the output holds the ratio (static
/// curve, DC at each level after 1 s).
#[test]
fn the_static_curve_holds_the_ratio() {
    for r in [2u8, 4, 7] {
        let p = hard(r);
        let (t, ratio) = (p.thresh_db(), p.ratio());
        for level in [-30.0f32, -20.0, -12.0, -6.0, 0.0] {
            let x = 10f32.powf(level / 20.0);
            let n = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
            let mut comp = Box::new(MasterComp::new());
            let out = run(&mut comp, &pair1(vec![x; n]), |_| p);
            let got = db(out[0][n - 1] / x) + level;
            let want = if level > t {
                t + (level - t) / ratio
            } else {
                level
            };
            assert!(
                (got - want).abs() < 0.05,
                "{ratio}:1 at {level} dB: {got}, want {want}"
            );
        }
    }
}

/// MAKEUP lifts everything by its dB, below the threshold too.
#[test]
fn makeup_adds_its_gain() {
    let p = CompParams {
        makeup: 0.25,
        ..hard(4)
    };
    let n = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &pair1(vec![0.01; n]), |_| p);
    let got = db(out[0][n - 1] / 0.01);
    assert!((got - 6.0).abs() < 0.05, "{got}");
}

/// Samples from `from` until `gr(n)` first crosses `level` dB.
fn crossing(out: &[f32], x: &[f32], from: usize, rising: bool, level: f32) -> usize {
    (from..out.len())
        .find(|&n| {
            let gr = -db(out[n] / x[n]);
            if rising { gr >= level } else { gr <= level }
        })
        .unwrap()
        - from
}

/// Spec § Testing: the attack and release time constants hold. A DC step
/// from −40 to 0 dBFS into 20:1 at −20 dB wants 19 dB of reduction: it is
/// 63.2 % there after ATTACK's τ, and after the step back down 36.8 % is
/// left after RELEASE's τ, each within 5 % and one `STEP`.
#[test]
fn attack_and_release_time_constants_hold() {
    for (attack, release) in [(0.5f32, 0.5f32), (2.0 / 3.0, 0.25)] {
        let p = CompParams {
            attack,
            release,
            ..hard(7)
        };
        let target = 20.0 * (1.0 - 1.0 / p.ratio());
        let sec = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
        let mut x = vec![0.01; sec];
        x.extend(vec![1.0; 2 * sec]);
        x.extend(vec![0.01; 3 * sec]);
        let mut comp = Box::new(MasterComp::new());
        let out = run(&mut comp, &pair1(x.clone()), |_| p);
        let tol = |tau: f32| 0.05 * tau * SR as f32 + STEP as f32;
        let (ta, tr) = (p.attack_s() * SR as f32, p.release_s() * SR as f32);
        let a = crossing(&out[0], &x, sec, true, 0.632 * target) as f32;
        assert!((a - ta).abs() <= tol(p.attack_s()), "attack {a} vs {ta}");
        let r = crossing(&out[0], &x, 3 * sec, false, 0.368 * target) as f32;
        assert!((r - tr).abs() <= tol(p.release_s()), "release {r} vs {tr}");
    }
}

/// Spec § Testing: one linked gain on every pair. The same signal on three
/// channels of three pairs comes out bit-identical; a quiet pair is ducked
/// by a loud one.
#[test]
fn the_linked_gain_is_identical_across_pairs() {
    let n = 64 * BLOCK_SIZE;
    let same = noise(n, 0.3, 11);
    let mut ch: [Vec<f32>; 2 * PAIRS] = core::array::from_fn(|c| noise(n, 0.8, 20 + c as u32));
    (ch[0], ch[3], ch[4]) = (same.clone(), same.clone(), same.clone());
    let p = CompParams {
        thresh: 0.0,
        ..hard(7)
    };
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &ch, |_| p);
    assert_eq!(out[0], out[3]);
    assert_eq!(out[0], out[4]);
    assert_ne!(out[0], same, "it compressed");

    let quiet = noise(n, 0.01, 5);
    let mut ch: [Vec<f32>; 2 * PAIRS] = core::array::from_fn(|_| vec![0.0; n]);
    (ch[0], ch[5]) = (vec![1.0; n], quiet.clone());
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &ch, |_| hard(7));
    let tail = n / 2..;
    let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
    let ducked = db(rms(&out[5][tail.clone()]) / rms(&quiet[tail]));
    assert!(ducked < -15.0, "{ducked} dB");
    assert!(comp.gr_db() > 15.0, "{}", comp.gr_db());
}

fn kink(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0f32, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

/// Spec § Testing: no clicks when a control jumps (the tape's criterion).
/// ATTACK 100 ms, so the gain follows a control, not a transient.
#[test]
fn a_control_jump_does_not_click() {
    let n = 96 * BLOCK_SIZE;
    let x: Vec<f32> = (0..n)
        .map(|i| 0.8 * libm::sinf(2.0 * core::f32::consts::PI * 220.0 * i as f32 / SR as f32))
        .collect();
    let (at_b, win) = (48, 2_400);
    let slow = |r: u8| CompParams {
        attack: 1.0,
        ..hard(r)
    };
    let cases: [(&str, CompParams, CompParams); 6] = [
        (
            "THRESH",
            slow(4),
            CompParams {
                thresh: 0.0,
                ..slow(4)
            },
        ),
        ("RATIO on", slow(0), slow(7)),
        ("RATIO off", slow(7), slow(0)),
        (
            "MAKEUP",
            slow(4),
            CompParams {
                makeup: 1.0,
                ..slow(4)
            },
        ),
        (
            "MIX on",
            CompParams {
                mix: 0.0,
                ..slow(7)
            },
            slow(7),
        ),
        (
            "MIX half",
            slow(7),
            CompParams {
                mix: 0.5,
                ..slow(7)
            },
        ),
    ];
    let mut bad = Vec::new();
    for (name, a, b) in cases {
        let render = |f: &dyn Fn(usize) -> CompParams| {
            let mut comp = Box::new(MasterComp::new());
            run(&mut comp, &pair1(x.clone()), f)[0].clone()
        };
        let moved = render(&|blk| if blk < at_b { a } else { b });
        let (held_a, held_b) = (render(&|_| a), render(&|_| b));
        let w = at_b * BLOCK_SIZE..at_b * BLOCK_SIZE + win;
        let bound = kink(&held_a[w.clone()]).max(kink(&held_b[w.clone()]));
        let k = kink(&moved[w]);
        if k > 1.5 * bound {
            bad.push(name);
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

/// A NaN or out-of-range control renders finite.
#[test]
fn nan_and_out_of_range_controls_render_finite() {
    let ch: [Vec<f32>; 2 * PAIRS] =
        core::array::from_fn(|c| noise(16 * BLOCK_SIZE, 2.0, 3 + c as u32));
    let bad = CompParams {
        thresh: f32::NAN,
        ratio: 200,
        attack: -1.0,
        release: f32::NAN,
        makeup: 9.0,
        mix: 1.0,
    };
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &ch, |_| bad);
    assert!(out.iter().flatten().all(|s| s.is_finite()));
}

/// Spec § Master comp: one detector on the sum of all six channels. A loud
/// DC on any one channel, of any pair and either side, ducks a quiet one.
#[test]
fn the_detector_hears_every_channel() {
    let n = 64 * BLOCK_SIZE;
    let quiet = noise(n, 0.01, 5);
    let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
    for loud in 0..2 * PAIRS {
        let heard = if loud == 0 { 1 } else { 0 };
        let mut ch: [Vec<f32>; 2 * PAIRS] = core::array::from_fn(|_| vec![0.0; n]);
        (ch[loud], ch[heard]) = (vec![1.0; n], quiet.clone());
        let mut comp = Box::new(MasterComp::new());
        let out = run(&mut comp, &ch, |_| hard(7));
        let tail = n / 2..;
        let ducked = db(rms(&out[heard][tail.clone()]) / rms(&quiet[tail]));
        assert!(ducked < -15.0, "loud on channel {loud}: {ducked} dB");
    }
}

/// A NaN MIX reads as the default (1), for the switch as for the DSP: at
/// 20:1 it compresses as MIX 1 does, and at 1:1 it releases to an exact
/// bypass.
#[test]
fn a_nan_mix_reads_as_the_default_and_releases() {
    let sec = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let ch = pair1(noise(3 * sec, 1.0, 13));
    let nan = |r: u8| CompParams {
        mix: f32::NAN,
        ..hard(r)
    };
    let mut comp = Box::new(MasterComp::new());
    let want = run(&mut comp, &ch, |_| hard(7));
    let mut comp = Box::new(MasterComp::new());
    let got = run(&mut comp, &ch, |_| nan(7));
    assert_eq!(got, want);
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &ch, |b| {
        nan(if b * BLOCK_SIZE < sec { 7 } else { 0 })
    });
    assert_eq!(out[0][2 * sec..], ch[0][2 * sec..]);
    assert!(!comp.is_running());
}

/// One inf sample asks for a bounded reduction (the detector's ceiling):
/// at the fastest ATTACK the gain is back within 1 dB four RELEASE time
/// constants later.
#[test]
fn an_inf_sample_recovers() {
    let sec = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let mut x = vec![0.01f32; sec];
    x[sec / 4] = f32::INFINITY;
    let p = CompParams {
        attack: 0.0,
        ..hard(7)
    };
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &pair1(x.clone()), |_| p);
    let at = sec / 4 + (4.0 * p.release_s() * SR as f32) as usize;
    assert!(
        out.iter()
            .flatten()
            .enumerate()
            .all(|(i, s)| s.is_finite() || i == sec / 4)
    );
    let gr = -db(out[0][at] / x[at]);
    assert!(gr < 1.0, "{gr} dB");
}

/// Spec § Master comp: switched off mid-reduction it releases first, and
/// fades out only near unity. 20 ms after RATIO goes to 1:1 (two fade
/// lengths) most of the 19 dB is still there.
#[test]
fn it_releases_before_it_fades_out() {
    let sec = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let x = vec![1.0f32; 2 * sec];
    let mut comp = Box::new(MasterComp::new());
    let out = run(&mut comp, &pair1(x.clone()), |b| {
        hard(if b * BLOCK_SIZE < sec { 7 } else { 0 })
    });
    let gr = -db(out[0][sec + 960] / x[sec + 960]);
    assert!(gr > 10.0, "{gr} dB");
}

/// A new ATTACK or RELEASE takes effect: a compressor that ran at other
/// timings matches one that always ran at these.
#[test]
fn a_timing_change_takes_effect() {
    let sec = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let mut x = vec![0.0f32; sec];
    x.extend(vec![1.0; sec]);
    x.extend(vec![0.01; sec]);
    let at = |attack, release| CompParams {
        attack,
        release,
        ..hard(7)
    };
    let mut fresh = Box::new(MasterComp::new());
    let want = run(&mut fresh, &pair1(x.clone()), |_| at(0.9, 0.1));
    let mut moved = Box::new(MasterComp::new());
    let got = run(&mut moved, &pair1(x.clone()), |b| {
        if b < 8 { at(0.2, 0.8) } else { at(0.9, 0.1) }
    });
    assert_eq!(got[0][sec / 2..], want[0][sec / 2..]);
}
