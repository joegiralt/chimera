//! TAPE's STRETCH (spec § 2 STRETCH, Why period-locking): two grains read
//! at the note's pitch, spliced every D-TIME/2 under one complementary fade
//! while a time pointer runs at MOTION's speed. Precedent: Akai S950
//! Operator's Manual p. 32 (TIMESTRETCH, D-TIME, MONO and POLY); the JUCE
//! forum's Akai-style two-grain stretch (2024).

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::{Pos, SourceView};
use chimera_core::dsp::tape_engine::stretch::{
    GrainClock, WRAP_SAMPLES, d_time_s, lock, locks, time_speed,
};
use chimera_core::dsp::tape_engine::{
    INC_MAX, NOTE_ON_RAMP, Probe, StretchType, TapeEngine, TapeEngineParams, TapeModel,
};
use chimera_hal::BLOCK_SIZE;
use common::XorShift;
use common::tape::{held_plain, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;
/// The sines' period, frames: not a whole number, as a real source's isn't.
const P: f32 = 109.3;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    *TapeEngine::init_in_place(&mut slot)
}

fn stretch(kind: StretchType, period: f32) -> TapeEngineParams {
    TapeEngineParams {
        model: TapeModel::Stretch,
        kind,
        period,
        ..TapeEngineParams::default()
    }
}

/// `blocks` blocks of `e`, `p(block)` each, with each block's probe and
/// grain clock.
fn run(
    e: &mut TapeEngine,
    p: &dyn Fn(usize) -> TapeEngineParams,
    v: &SourceView<'_>,
    blocks: usize,
) -> (Vec<f32>, Vec<Probe>, Vec<GrainClock>) {
    let (mut out, mut probes, mut clocks) = (Vec::new(), Vec::new(), Vec::new());
    let mut b = [0.0; B];
    for n in 0..blocks {
        e.render(&mut b, &p(n), v, SR);
        out.extend_from_slice(&b);
        probes.push(e.probe());
        clocks.push(e.grain());
    }
    (out, probes, clocks)
}

/// A note of `p(block)` on a fresh engine at `pitch(block)` cents.
fn note(
    p: &dyn Fn(usize) -> TapeEngineParams,
    pitch: &dyn Fn(usize) -> f32,
    v: &SourceView<'_>,
    blocks: usize,
) -> Vec<f32> {
    let mut e = fresh();
    e.set_pitch(pitch(0));
    e.note_on(60, &p(0), v, SR);
    let mut out = Vec::new();
    let mut b = [0.0; B];
    for n in 0..blocks {
        e.set_pitch(pitch(n));
        e.render(&mut b, &p(n), v, SR);
        out.extend_from_slice(&b);
    }
    out
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

fn max_bend(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0f32, |s, w| s.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

/// Rising zero crossings, linearly interpolated: the frequency over `x`.
fn zero_cross_hz(x: &[f32]) -> f64 {
    let mut t = Vec::new();
    for i in 1..x.len() {
        let (a, b) = (x[i - 1] as f64, x[i] as f64);
        if a < 0.0 && b >= 0.0 {
            t.push((i - 1) as f64 + a / (a - b));
        }
    }
    (t.len() - 1) as f64 / (t[t.len() - 1] - t[0]) * SR as f64
}

/// The samples where the clock spliced, from each block's clock: the fade
/// restarted (`pos` fell), `pos` samples before the block's end; with the
/// length the new fade latched.
fn splices(clocks: &[GrainClock]) -> Vec<(usize, u32)> {
    let mut at = Vec::new();
    for (n, w) in clocks.windows(2).enumerate() {
        if w[1].pos < w[0].pos + B as u32 {
            at.push(((n + 2) * B - w[1].pos as usize, w[1].len));
        }
    }
    at
}

#[test]
fn laws() {
    assert!((d_time_s(0.0) - 0.010).abs() < 1e-8);
    assert!((d_time_s(1.0) - 0.250).abs() < 1e-6);
    assert!((d_time_s(0.5) - 0.050).abs() < 1e-6);
    for (m, s) in [
        (-1.0, -1.0),
        (-0.5, -0.5),
        (0.0, 0.0),
        (0.5, 1.0),
        (1.0, 2.0),
    ] {
        assert_eq!(time_speed(m), s, "MOTION {m}");
    }
    // As near `t` as a whole number of periods from the old head allows.
    assert_eq!(lock(1_000.0, 0.0, 100.0), 0.0);
    assert_eq!(lock(1_000.0, 40.0, 100.0), 0.0);
    assert_eq!(lock(1_000.0, 60.0, 100.0), 100.0);
    assert_eq!(lock(10.0, 1_000.0, 100.0), 1_010.0);
    // MONO, pitched, and two periods inside both the window and the D-TIME.
    assert!(locks(StretchType::Mono, 100.0, 200, 200));
    assert!(!locks(StretchType::Mono, 100.5, 200, 1_000));
    assert!(!locks(StretchType::Mono, 100.5, 1_000, 200));
    assert!(!locks(StretchType::Mono, 0.0, 1_000, 1_000));
    assert!(!locks(StretchType::Poly, 100.0, 1_000, 1_000));
}

/// MOTION 0.25, 0.5 and 1 (time at 0.5, 1 and 2): the pitch is the note's
/// within a cent, MONO on a sine, and a click in the source comes out at
/// its time over the speed, within one D-TIME.
#[test]
fn stretch_keeps_pitch() {
    let held = held_plain(&sine(48_000, P, 4 * 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let want = 48_000.0 / P as f64;
    for motion in [0.25, 0.5, 1.0] {
        for size in [0.0, 0.5, 1.0] {
            let p = TapeEngineParams {
                motion,
                size,
                ..stretch(StretchType::Mono, P)
            };
            let out = note(&|_| p, &|_| 0.0, &v, 1_100);
            let hz = zero_cross_hz(&out[4_800..]);
            let cents = 1_200.0 * (hz / want).log2();
            assert!(
                cents.abs() < 1.0,
                "MOTION {motion}, SIZE {size}: {cents:.3} cents"
            );
        }
    }

    // A 2 ms burst, so a seam's fade can't hide it.
    let c = 24_000;
    let mut src = vec![0.0; 2 * 48_000];
    for (k, y) in src[c..c + 96].iter_mut().enumerate() {
        *y = if k % 2 == 0 { 0.9 } else { -0.9 };
    }
    let held = held_plain(&src, 48_000);
    let v = SourceView::strict(&held).unwrap();
    for motion in [0.25, 0.5, 1.0] {
        for size in [0.0, 0.5] {
            let p = TapeEngineParams {
                motion,
                size,
                ..stretch(StretchType::Poly, 0.0)
            };
            let out = note(&|_| p, &|_| 0.0, &v, 1_600);
            let at = out.iter().position(|y| y.abs() > 0.05).expect("the click");
            let due = c as f32 / time_speed(motion);
            let d_time = d_time_s(size) * SR as f32;
            assert!(
                (at as f32 - due).abs() <= d_time,
                "MOTION {motion}, SIZE {size}: at {at}, due {due}"
            );
        }
    }
}

/// SIZE 0 → 1 → 0 in 50 ms, in 60 and in 300 blocks, again and again: on
/// a DC source the output holds its value within 1e-6 every sample (the old
/// grain's gain is 1 − the new one's), and each splice comes exactly the
/// length its fade latched after the one before.
#[test]
fn stretch_fades_are_complementary() {
    let held = held_plain(&vec![0.5; 48_000], 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (sweep, motion) in [(38, 0.5), (60, 1.0), (300, -1.0)] {
        let p = |n: usize| {
            let x = (n % sweep) as f32 / sweep as f32;
            TapeEngineParams {
                size: 1.0 - (2.0 * x - 1.0).abs(),
                motion,
                ..stretch(StretchType::Poly, 0.0)
            }
        };
        let mut e = fresh();
        e.note_on(60, &p(0), &v, SR);
        let (out, _, clocks) = run(&mut e, &p, &v, 1_500);
        for (i, y) in out.iter().enumerate().skip(NOTE_ON_RAMP as usize) {
            assert!((y - 0.5).abs() <= 1e-6, "sweep {sweep}: {y} at {i}");
        }
        let s = splices(&clocks);
        assert!(s.len() > 20, "sweep {sweep}: {} splices", s.len());
        for w in s[1..].windows(2) {
            assert_eq!(
                w[1].0 - w[0].0,
                w[0].1 as usize,
                "sweep {sweep}: the splice at {} came before its fade ended",
                w[1].0
            );
        }
    }
}

/// The held source read straight at `inc` 1, from each quarter frame: its
/// own worst step and bend, 12-bit grain and interpolation included.
fn own(v: &SourceView<'_>) -> (f32, f32) {
    [0.0, 0.25, 0.5, 0.75]
        .map(|f| {
            let x: Vec<f32> = (0..v.frames() - 3)
                .map(|i| v.read(Pos::from_f32(i as f32 + f)))
                .collect();
            (max_step(&x), max_bend(&x))
        })
        .into_iter()
        .fold((0.0f32, 0.0f32), |a, b| (a.0.max(b.0), a.1.max(b.1)))
}

/// A sine, MONO, at every D-TIME and at 0.5× and 2× time, the window
/// wrapping: each splice lands in phase, so no step or bend passes the
/// held sine's own, read straight.
#[test]
fn mono_splices_are_click_free() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let (own_step, own_bend) = own(&v);
    for size in [0.0, 0.25, 0.5, 0.75, 1.0] {
        for motion in [0.25, 1.0] {
            let p = TapeEngineParams {
                size,
                motion,
                ..stretch(StretchType::Mono, P)
            };
            let out = note(&|_| p, &|_| 0.0, &v, 1_500);
            let (step, bend) = (max_step(&out[B..]), max_bend(&out[B..]));
            assert!(
                step <= 1.02 * own_step && bend <= 1.1 * own_bend,
                "SIZE {size}, MOTION {motion}: step {step} ({own_step}), bend {bend} ({own_bend})"
            );
        }
    }
}

/// POSITION, SIZE and pitch jumping together, fast (60 and 300 blocks to
/// a sweep) and slow, MONO and POLY: no step past the same note's at a
/// fixed setting, at either pitch.
#[test]
fn modulation_is_click_free() {
    let held = held_plain(&sine(48_000, P, 2 * 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (kind, period) in [(StretchType::Mono, P), (StretchType::Poly, 0.0)] {
        let base = TapeEngineParams {
            motion: 0.5,
            size: 0.3,
            ..stretch(kind, period)
        };
        let fixed = [0.0, 1_200.0]
            .map(|c| max_step(&note(&|_| base, &|_| c, &v, 3_000)[B..]))
            .into_iter()
            .fold(0.0f32, f32::max);
        for half in [30, 150, 1_500] {
            let up = |n: usize| (n / half) % 2 == 1;
            let p = |n: usize| TapeEngineParams {
                position: if up(n) { 0.8 } else { 0.1 },
                size: if (n / (half / 3 + 1)) % 2 == 1 {
                    1.0
                } else {
                    0.0
                },
                motion: if up(n) { 1.0 } else { -0.6 },
                ..base
            };
            let jumps = max_step(
                &note(
                    &p,
                    &|n| if (n / 7) % 2 == 1 { 1_200.0 } else { 0.0 },
                    &v,
                    3_000,
                )[B..],
            );
            assert!(
                jumps <= 1.25 * fixed,
                "{kind:?}, every {half} blocks: {jumps} against {fixed}"
            );
        }
    }
}

/// White noise, POLY: two grains' fades make no 10 ms RMS stray 3 dB from
/// the render's mean, at any D-TIME or speed (on a source long enough that
/// time never nears its end, where grains start early).
#[test]
fn poly_noise_wobbles_under_3db() {
    let mut rng = XorShift(0x57E7_0004);
    let noise: Vec<f32> = (0..6 * 48_000)
        .map(|_| (rng.next_u32() as f32 / u32::MAX as f32 - 0.5) * 0.8)
        .collect();
    let held = held_plain(&noise, 48_000);
    let v = SourceView::strict(&held).unwrap();
    for size in [0.0, 0.5, 1.0] {
        for motion in [0.25, 0.5, 1.0] {
            let p = TapeEngineParams {
                size,
                motion,
                ..stretch(StretchType::Poly, 0.0)
            };
            let out = note(&|_| p, &|_| 0.0, &v, 1_500);
            let win: Vec<f64> = out[4_800..]
                .as_chunks::<480>()
                .0
                .iter()
                .map(|c| c.iter().map(|&y| (y * y) as f64).sum::<f64>() / 480.0)
                .collect();
            let mean = win.iter().sum::<f64>() / win.len() as f64;
            for (k, w) in win.iter().enumerate() {
                let db = 10.0 * (w / mean).log10();
                assert!(
                    db.abs() < 3.0,
                    "SIZE {size}, MOTION {motion}: {db:.2} dB at window {k}"
                );
            }
        }
    }
}

/// The params, MONO and POLY, of `same_as_poly`'s runs.
fn mono_poly(p: TapeEngineParams, v: &SourceView<'_>) -> [(Vec<f32>, Vec<Probe>); 2] {
    [StretchType::Mono, StretchType::Poly].map(|kind| {
        let q = |n: usize| TapeEngineParams {
            kind,
            position: if (n / 50) % 2 == 1 { 0.7 } else { p.position },
            ..p
        };
        let mut e = fresh();
        e.note_on(60, &q(0), v, SR);
        let (out, probes, _) = run(&mut e, &q, v, 800);
        (out, probes)
    })
}

/// PERIOD 0 (an unpitched source): MONO plays exactly as POLY.
#[test]
fn unpitched_mono_is_poly() {
    let held = held_plain(&sine(48_000, P, 24_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (size, motion) in [(0.0, 1.0), (0.6, -1.0), (1.0, 0.3)] {
        let p = TapeEngineParams {
            size,
            motion,
            ..stretch(StretchType::Mono, 0.0)
        };
        let [mono, poly] = mono_poly(p, &v);
        assert!(mono.0 == poly.0, "SIZE {size}, MOTION {motion}");
    }
}

/// Review Focus 5: a PERIOD that doesn't fit twice in the window or the
/// D-TIME (W; the D-TIME; just over half either) plays MONO as POLY, bit
/// for bit, every head inside the window; just inside half, MONO locks.
#[test]
fn oversized_period_plays_as_poly() {
    let w = 4_800u32;
    let held = held_plain(&sine(48_000, P, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    // SIZE 1's D-TIME (12 000 frames) is past W; SIZE 0.5's is 2 400.
    let d = (d_time_s(0.5) * 48_000.0).round();
    for (size, period, plays_poly) in [
        (1.0, w as f32, true),
        (0.5, d, true),
        (1.0, (w / 2 + 1) as f32, true),
        (0.5, d / 2.0 + 1.0, true),
        (1.0, (w / 2 - 1) as f32, false),
        (0.5, d / 2.0, false),
    ] {
        let p = TapeEngineParams {
            size,
            motion: 0.7,
            period,
            ..stretch(StretchType::Mono, 0.0)
        };
        let [mono, poly] = mono_poly(p, &v);
        assert_eq!(mono.0 == poly.0, plays_poly, "SIZE {size}, PERIOD {period}");
        for pr in mono.1.iter().chain(&poly.1) {
            let other = pr.other.expect("two grains");
            assert!(pr.head < w && other < w, "PERIOD {period}: {pr:?}");
        }
    }
}

/// MOTION 0: the time pointer stands still for 2 s while the grains play.
#[test]
fn freeze_holds_time() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for position in [0.0, 0.3] {
        let p = TapeEngineParams {
            position,
            size: 0.4,
            ..stretch(StretchType::Mono, P)
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (out, _, clocks) = run(&mut e, &|_| p, &v, 1_500);
        let t = clocks[0].t;
        assert!(clocks.iter().all(|c| c.t == t), "POSITION {position}");
        // POSITION of the window, its last two frames left out.
        assert_eq!(t.whole(), (position * 47_998.0) as u32);
        assert!(splices(&clocks).len() > 50);
        assert!(out[B..].iter().any(|y| y.abs() > 0.5), "it sounds");
    }
}

/// MOTION −1: time runs backward, one frame a sample, while every grain's
/// head runs forward.
#[test]
fn reverse_time_plays_grains_forward() {
    let held = held_plain(&sine(48_000, P, 2 * 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        position: 0.8,
        motion: -1.0,
        size: 0.5,
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (_, probes, clocks) = run(&mut e, &|_| p, &v, 1_000);
    for (k, c) in clocks.windows(2).enumerate() {
        let fell = c[0].t.whole() as i64 - c[1].t.whole() as i64;
        assert!((63..=65).contains(&fell), "block {k}: T moved {fell}");
    }
    let mut runs = 0;
    for (k, w) in probes.windows(2).enumerate() {
        // Between splices each grain plays on forward.
        if clocks[k + 1].pos > clocks[k].pos {
            runs += 1;
            assert!(w[1].head > w[0].head, "block {k}: {w:?}");
            assert!(w[1].other > w[0].other, "block {k}: {w:?}");
        }
    }
    assert!(runs > 800);
}

/// The first grain is pending: SIZE taken at note-on (1, 250 ms) and then
/// dropped to 0 (10 ms) splices within the eased D-TIME, not after the
/// 125 ms the note-on latched.
#[test]
fn run_up_follows_the_block() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = |n: usize| TapeEngineParams {
        size: if n == 0 { 1.0 } else { 0.0 },
        motion: 0.5,
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.note_on(60, &p(0), &v, SR);
    let (_, _, clocks) = run(&mut e, &p, &v, 40);
    let first = splices(&clocks)[0].0;
    assert!(first < 20 * B, "first splice at {first}");
}

/// Time wraps in a window of 4 800 frames again and again, MONO and POLY,
/// at 0.5×, 1× and 2×. The splice's fade carries the output from the
/// window's end to its start, and grains that reach the end wrap under the
/// lift. No step passes the sine's own, and MONO, whose splices land in
/// phase and whose grains wrap by whole periods, no bend either. (POLY's
/// linear fades bend the output by up to twice their slope where one ends
/// and the next starts: the S950's splice, by design.)
#[test]
fn time_wraps_under_the_fade() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let own = own(&v);
    for (kind, period) in [(StretchType::Mono, P), (StretchType::Poly, 0.0)] {
        for motion in [0.25, 0.5, 1.0] {
            let p = TapeEngineParams {
                end: 0.1,
                size: 0.3,
                motion,
                ..stretch(kind, period)
            };
            let mut e = fresh();
            e.note_on(60, &p, &v, SR);
            let (out, _, clocks) = run(&mut e, &|_| p, &v, 1_500);
            let laps = clocks.windows(2).filter(|c| c[1].t < c[0].t).count();
            assert!(laps >= 5, "{kind:?}, MOTION {motion}: {laps} laps");
            let (step, bend) = (max_step(&out[B..]), max_bend(&out[B..]));
            let bent = kind == StretchType::Mono && bend > 1.1 * own.1;
            assert!(
                step <= 1.1 * own.0 && !bent,
                "{kind:?}, MOTION {motion}: step {step}, bend {bend} against {own:?}"
            );
        }
    }
}

/// A window shorter than a grain's travel (SIZE 1's 250 ms on 4 800
/// frames): grains wrap back in it. POLY wraps by the window, which isn't
/// whole periods, so it jumps: what the grain added rides on and fades over
/// `LIFT_S`, so no step passes the sine's own (a corner stays, as under
/// PING-PONG's retargeted clamp). MONO wraps by whole periods: no bend
/// either. Nothing past the window is read.
#[test]
fn grain_wraps_are_click_free() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let own = own(&v);
    // Past the window the source holds full scale: a grain reading there
    // (or a tap) steps.
    let mut src = sine(48_000, P, 4_800);
    src.resize(48_000, 1.0);
    let held = held_plain(&src, 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (kind, period) in [(StretchType::Poly, 0.0), (StretchType::Mono, P)] {
        for motion in [0.25, 0.5, 1.0] {
            let p = TapeEngineParams {
                end: 0.1,
                size: 1.0,
                motion,
                ..stretch(kind, period)
            };
            let mut e = fresh();
            e.note_on(60, &p, &v, SR);
            let (out, probes, clocks) = run(&mut e, &|_| p, &v, 1_500);
            let wraps = e.events() as usize - splices(&clocks).len();
            assert!(wraps > 10, "{kind:?}, MOTION {motion}: {wraps} wraps");
            // MONO wraps back by the fewest whole periods that are
            // `WRAP_SAMPLES` of travel (at `inc` 1, 2 periods of 109.3),
            // re-reading its last cycles; POLY by the window.
            let back = (WRAP_SAMPLES as f32 / P).ceil() * P;
            let mut seen = 0;
            for (c, w) in clocks[1..].windows(2).zip(probes[1..].windows(2)) {
                if c[1].pos > c[0].pos && w[1].head < w[0].head {
                    let jump = (w[0].head + B as u32 - w[1].head) as f32;
                    let want = if kind == StretchType::Mono {
                        back
                    } else {
                        4_798.0
                    };
                    assert!(
                        (jump - want).abs() <= 2.0,
                        "{kind:?}: back {jump}, want {want}"
                    );
                    seen += 1;
                }
            }
            assert!(seen > 3, "{kind:?}, MOTION {motion}: {seen} wraps seen");
            let (step, bend) = (max_step(&out[B..]), max_bend(&out[B..]));
            let bent = kind == StretchType::Mono && bend > 1.1 * own.1;
            assert!(
                step <= 1.1 * own.0 && !bent,
                "{kind:?}, MOTION {motion}: {step}, {bend} against {own:?}"
            );
        }
    }
}

/// The shortest window the floor allows at `inc` 16 (4 096 frames) wraps
/// POLY grains under the lift, as any window does: a slow sine there steps
/// no more than its own. (A window under the floor would be a waveform
/// loop, wrapped bare.)
#[test]
fn shortest_window_wraps_under_the_lift() {
    let held = held_plain(&sine(48_000, 2_000.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let own = 16.0 * core::f32::consts::TAU / 2_000.0;
    let p = TapeEngineParams {
        start: 0.5,
        end: 0.5,
        size: 0.0,
        motion: 1.0,
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.set_pitch(4_800.0);
    e.note_on(60, &p, &v, SR);
    let (out, _, clocks) = run(&mut e, &|_| p, &v, 600);
    assert!(
        e.events() as usize > splices(&clocks).len() + 10,
        "it wraps"
    );
    let step = max_step(&out[B..]);
    assert!(step <= 1.25 * own, "{step} against {own}");
}

/// Splices come once a D-TIME/2 at most, and a grain wraps at most once in
/// `WRAP_SAMPLES` samples (a block): at `inc` 16 on the shortest window, at
/// the shortest D-TIME, a block holds at most one splice and the two
/// grains' wraps.
#[test]
fn event_rate_is_bounded() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    tiny_events(&held, P);
    // Sources under the floor are waveform loops, wrapped inline: only
    // splices are events.
    for frames in [8, 64] {
        let held = held_plain(&sine(48_000, frames as f32 / 2.0, frames), 48_000);
        tiny_events(&held, frames as f32 / 2.0);
    }
}

/// `event_rate_is_bounded` on `held` (period `period`), at the shortest
/// window and D-TIME at `inc` 16.
fn tiny_events(held: &[u8], period: f32) {
    let v = SourceView::strict(held).unwrap();
    for kind in [StretchType::Mono, StretchType::Poly] {
        let p = TapeEngineParams {
            start: 0.5,
            end: 0.5,
            size: 0.0,
            motion: 1.0,
            ..stretch(kind, period)
        };
        let mut e = fresh();
        e.set_pitch(4_800.0);
        e.note_on(127, &p, &v, SR);
        run(&mut e, &|_| p, &v, 4);
        let before = e.events();
        let blocks = 300;
        let (out, probes, _) = run(&mut e, &|_| p, &v, blocks);
        // A waveform loop's grains stay within a stride of their window.
        let reach = v.frames() + INC_MAX as u32;
        for pr in &probes {
            assert!(pr.head <= reach && pr.other.unwrap() <= reach, "{pr:?}");
        }
        let per = (e.events() - before) as f32 / blocks as f32;
        let most = 1 + 2 * B / WRAP_SAMPLES as usize;
        assert!(
            per <= most as f32,
            "{kind:?}, {} frames: {per} a block, at most {most}",
            v.frames()
        );
        assert!(out.iter().all(|y| y.is_finite()));
    }
}

/// SIZE 1 at +12 st, MOTION 0.5: every grain starts where time is, the
/// window's end included (no frozen time there): within a frame of `T`.
#[test]
fn time_moves_at_high_pitch() {
    let held = held_plain(&sine(48_000, P, 96_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        size: 1.0,
        motion: 0.5,
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.set_pitch(1_200.0);
    e.note_on(60, &p, &v, SR);
    let (_, probes, clocks) = run(&mut e, &|_| p, &v, 1_600);
    let w = 96_000 - 2;
    let mut latest = 0;
    for (b, (c, pr)) in clocks.windows(2).zip(&probes[1..]).enumerate() {
        if c[1].pos < c[0].pos {
            // The new grain's start: its head less what it read since, at
            // `inc` 2, unwrapped.
            let at = (b + 2) * B - c[1].pos as usize;
            let start = (pr.other.unwrap() as usize + w - 2 * c[1].pos as usize) % w;
            assert!(start.abs_diff(at % w) <= 1, "splice at {at}: {start}");
            latest = latest.max(start);
        }
    }
    let fade = 2 * (d_time_s(1.0) * 24_000.0) as usize;
    assert!(latest + fade >= w, "grains start no later than {latest}");
}

/// Each splice starts its grain at `T` as it stands at that sample: POLY
/// at 0.5× time, the new grain's head at a splice block's end is
/// `T(splice) + pos` within a frame.
#[test]
fn splices_start_at_the_time_pointer() {
    let held = held_plain(&sine(48_000, P, 96_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        size: 0.13,
        motion: 0.25,
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (_, probes, clocks) = run(&mut e, &|_| p, &v, 600);
    let mut n = 0;
    for (b, (c, pr)) in clocks.windows(2).zip(&probes[1..]).enumerate() {
        if c[1].pos < c[0].pos {
            let at = (b + 2) * B - c[1].pos as usize;
            let want = at as f64 * 0.5 + c[1].pos as f64;
            let got = pr.other.unwrap() as f64;
            assert!(
                (got - want).abs() <= 1.0,
                "splice at {at}: {got} against {want}"
            );
            n += 1;
        }
    }
    assert!(n > 20);
}

/// END lowered past the first grain while it runs up: it wraps back in the
/// window from where it read, so the output carries on without a step.
#[test]
fn end_moved_in_the_run_up_is_click_free() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let own = own(&v);
    let p = |n: usize| TapeEngineParams {
        position: 0.5,
        size: 1.0,
        motion: 0.5,
        end: if n < 5 { 1.0 } else { 0.1 },
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.note_on(60, &p(0), &v, SR);
    let (out, probes, _) = run(&mut e, &p, &v, 40);
    assert!(probes[6].head < 4_800, "it wrapped: {:?}", probes[6]);
    let step = max_step(&out[B..]);
    assert!(step <= 1.1 * own.0, "{step} against {own:?}");
}

/// SIZE grown from 0 to 1 while the first fade runs up: its gain is
/// re-aimed each block, so it climbs to exactly 1 as the fade ends and
/// never past it.
#[test]
fn run_up_gain_lands_on_one() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = |n: usize| TapeEngineParams {
        size: if n == 0 { 0.0 } else { 1.0 },
        motion: 0.5,
        ..stretch(StretchType::Poly, 0.0)
    };
    let mut e = fresh();
    e.note_on(60, &p(0), &v, SR);
    let (_, probes, clocks) = run(&mut e, &p, &v, 200);
    let first = clocks
        .iter()
        .position(|c| c.pos < B as u32 && c.len > 1_000);
    let n = first.unwrap_or(clocks.len()).max(2);
    for (k, pr) in probes[..n - 1].iter().enumerate() {
        assert!(pr.gain <= 1.0 + 1e-5, "block {k}: gain {}", pr.gain);
    }
    let before = &probes[n - 2];
    let c = clocks[n - 2];
    let want = 1.0 - c.left() as f32 / c.len as f32;
    assert!(
        (before.gain - want).abs() < 0.05,
        "{} against {want}",
        before.gain
    );
}

/// A note-on on a sounding STRETCH ramps from where the output was; `hold`
/// before a fresh note starts it from 0.
#[test]
fn retrigger_ramps_from_last() {
    let held = held_plain(&sine(48_000, P, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        motion: 0.5,
        size: 0.2,
        ..stretch(StretchType::Mono, P)
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _, _) = run(&mut e, &|_| p, &v, 20);
    let steady = max_step(&out[B..]);
    for position in [0.0, 50.0 / 48_000.0, 0.3] {
        let q = TapeEngineParams { position, ..p };
        e.note_on(60, &q, &v, SR);
        out.extend(run(&mut e, &|_| q, &v, 4).0);
    }
    let worst = max_step(&out[B..]);
    assert!(worst <= 2.0 * steady, "{worst} against {steady}");

    assert!(out[out.len() - 1].abs() > 0.05, "ends off zero");
    e.hold();
    e.note_on(60, &TapeEngineParams { position: 0.3, ..p }, &v, SR);
    assert_eq!(run(&mut e, &|_| p, &v, 1).0[0], 0.0);
}

/// Host time a sample of `blocks` blocks of `p` on `v` at `(note, pitch)`:
/// the best of seven.
fn host_time(p: TapeEngineParams, v: &SourceView<'_>, (n, c): (u8, f32), blocks: usize) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..7 {
        let mut e = fresh();
        e.set_pitch(c);
        e.note_on(n, &p, v, SR);
        let mut b = [0.0; B];
        let t = std::time::Instant::now();
        for _ in 0..blocks {
            e.render(&mut b, &p, v, SR);
            std::hint::black_box(&b);
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    best
}

/// STRETCH's per-call work (spec § 5) in host time: MONO at the shortest
/// D-TIME and `inc` 16 on the shortest window (a splice in 240 samples, a
/// grain wrapping once in `WRAP_SAMPLES`) against the same on a window it
/// never wraps in. The bill's basis is the M7 count: the same loop as
/// LOOP's fade path (217) and these events amortised; the host's ratio to
/// LOOP's fade path is printed beside it, for the record.
///
/// Host timing is load-sensitive: run it alone, `cargo test --release -p
/// chimera-core --test tape_stretch_test stretch_costs -- --ignored`.
#[test]
#[ignore = "load-sensitive host timing; the bench rows replace it"]
fn stretch_costs_within_its_bill() {
    let long = held_plain(&sine(48_000, 100.7, 480_000), 48_000);
    let lv = SourceView::strict(&long).unwrap();
    let blocks = 20_000;
    let at_clamp = (127, 4_800.0);
    let fade = host_time(
        TapeEngineParams {
            xfade: 0.5,
            ..TapeEngineParams::default()
        },
        &lv,
        at_clamp,
        blocks,
    );
    let [short, long] = [0.0, 1.0].map(|end| {
        let p = TapeEngineParams {
            end,
            size: 0.0,
            motion: 1.0,
            ..stretch(StretchType::Mono, 100.7)
        };
        host_time(p, &lv, at_clamp, blocks)
    });
    eprintln!(
        "STRETCH at inc 16: {:.2} × LOOP's fade path on the shortest window, {:.2} on a long one",
        short / fade,
        long / fade
    );
    // Sources under the floor: waveform loops, wrapped inline.
    for frames in [8u32, 64] {
        let held = held_plain(&sine(48_000, frames as f32 / 2.0, frames), 48_000);
        let tv = SourceView::strict(&held).unwrap();
        let p = TapeEngineParams {
            size: 0.0,
            motion: 1.0,
            ..stretch(StretchType::Mono, frames as f32 / 2.0)
        };
        let ratio = host_time(p, &tv, at_clamp, blocks) / fade;
        eprintln!(
            "STRETCH on {frames} frames at inc 16: {ratio:.2} × LOOP's fade path, {:.2} × its long run",
            ratio * fade / long
        );
        let slack = if cfg!(debug_assertions) { 1.5 } else { 1.0 };
        assert!(ratio * fade / long <= slack * 1.15, "{frames} frames");
    }
    let events = short / long;
    eprintln!(
        "its events: {:.3} × the plain run (the M7 count amortises 7 %)",
        events
    );
    let slack = if cfg!(debug_assertions) { 1.5 } else { 1.0 };
    assert!(events <= slack * 1.10, "events cost {events:.3} × the run");
}
