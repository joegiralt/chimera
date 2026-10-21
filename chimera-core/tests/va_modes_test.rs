//! The VA MODEs past the first five (#601–#605, #613), one section each.
//! Levels are the morph's (÷ `VA_CHAIN_GAIN`) unless said.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::dc_blocker::DcBlocker;
use chimera_core::dsp::va::combine::{AM_KNEE, MAX_TRIM, am_reach};
use chimera_core::dsp::va::{VA_CHAIN_GAIN, VaEngine, VaMode, VaParams, fm_index, phase_depth};
use chimera_core::dsp::voice::DC_HZ;
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;
use common::spectrum::{alias_db_lattice, fundamental, peak_hz};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;
const C4: u8 = 60;
const A4: u8 = 69;
const C5: u8 = 72;
const C6: u8 = 84;
const GATE_BLOCKS: usize = ((1 << 12) + (1 << 15)) / B;
const PITCH_BLOCKS: usize = (1 << 17) / B;
const ANCHORS: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
const SAW: f32 = 2.0 / 3.0;
const AT: usize = 20;

fn hz_of(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

fn with_engine<R>(f: impl FnOnce(&mut VaEngine) -> R) -> R {
    let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
    f(VaEngine::init_in_place(&mut slot))
}

/// `blocks` blocks of `e`, `set` adjusting `p` before each; morph level.
fn run(
    e: &mut VaEngine,
    p: &mut VaParams,
    blocks: usize,
    mut set: impl FnMut(usize, &mut VaParams),
) -> Vec<f32> {
    let mut y = Vec::with_capacity(blocks * B);
    let mut out = [0.0f32; B];
    for b in 0..blocks {
        set(b, p);
        e.render(&mut out, p, SR);
        y.extend(out.iter().map(|v| v / VA_CHAIN_GAIN));
    }
    y
}

/// A fresh note on part 0, `prep` before its note-on.
fn take_with(
    prep: impl FnOnce(&mut VaEngine),
    note: u8,
    base: VaParams,
    blocks: usize,
    set: impl FnMut(usize, &mut VaParams),
) -> Vec<f32> {
    with_engine(|e| {
        let mut p = base;
        prep(e);
        e.hold();
        e.note_on(note, &p, PartId::new(0).unwrap(), SR);
        run(e, &mut p, blocks, set)
    })
}

fn held(note: u8, base: VaParams, blocks: usize) -> Vec<f32> {
    take_with(|_| {}, note, base, blocks, |_, _| {})
}

/// Sines, STAB 1, no sub, both in tune.
fn sine() -> VaParams {
    VaParams {
        wave: [0.0; 2],
        shape: [0.0; 2],
        stability: 1.0,
        sub: 0.0,
        fine: [0.0; 2],
        ..VaParams::default()
    }
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

/// The largest second difference from block 2 on.
fn d2(y: &[f32]) -> f64 {
    (2 * B..y.len())
        .map(|n| (f64::from(y[n]) - 2.0 * f64::from(y[n - 1]) + f64::from(y[n - 2])).abs())
        .fold(0.0, f64::max)
}

/// The click rule (`va_engine_test`): `(swept, limit)`.
fn click(swept: &[f32], held: &[Vec<f32>]) -> (f64, f64) {
    let h = held.iter().map(|y| d2(y)).fold(0.0, f64::max);
    (d2(swept), 1.5 * h + 1e-6)
}

// ---- FM (#601) ----

fn fm(amount: f32) -> VaParams {
    VaParams {
        mode: VaMode::Fm,
        amount,
        ..sine()
    }
}

/// Through zero, the read's mean rate is OSC 1's own at any index: the
/// pitch stays on the note and the wave repeats every period.
#[test]
fn fm_stays_centred() {
    let f0 = hz_of(A4);
    let period = SR as f64 / f64::from(f0);
    for blend in [0.0, 0.3, 0.6, 1.0] {
        let y = held(A4, fm(blend), PITCH_BLOCKS + 200);
        let f = peak_hz(&y, SR as f32, f0);
        let cents = 1200.0 * (f / f64::from(f0)).log2();
        // one period on, linearly interpolated, over the tail
        let (k, fr) = (period.floor() as usize, period.fract());
        let tail = &y[y.len() - 8192 - k - 1..];
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        for n in 0..8192 {
            let a = f64::from(tail[n]);
            let b = f64::from(tail[n + k]) * (1.0 - fr) + f64::from(tail[n + k + 1]) * fr;
            (xy, xx, yy) = (xy + a * b, xx + a * a, yy + b * b);
        }
        let r = xy / (xx * yy).sqrt();
        println!("FM BLEND {blend}: {f:.3} Hz ({cents:+.3} cents), r(1 period) {r:.5}");
        assert!(cents.abs() <= 1.0, "BLEND {blend}: {cents}");
        assert!(r >= 0.99, "BLEND {blend}: {r}");
    }
}

/// Past index 1 the read's rate goes negative and it runs backwards.
#[test]
fn fm_goes_through_zero() {
    let backwards = |blend: f32| {
        with_engine(|e| {
            let mut p = fm(blend);
            e.hold();
            e.note_on(A4, &p, PartId::new(0).unwrap(), SR);
            (0..50).any(|_| {
                run(e, &mut p, 1, |_, _| {});
                e.read_ran_backwards()
            })
        })
    };
    assert!(backwards(1.0), "BLEND 1: I = 5");
    assert!(!backwards(0.4), "BLEND 0.4: I = 0.8");
}

/// The alias gate on the lattice of both pitches, corrected and not. Where
/// Carson's bandwidth `(I + 1)·f_m + f_c` fits under Nyquist the gate is
/// −45 dB; elsewhere the correction must win 10 dB. A saw or pulse
/// carrier's harmonics reach Nyquist at any index, so its band never fits
/// (measured: −23 to −48 dB at BLEND 1, ADR 0159). Most of that is, by
/// estimate, the 2-point kernel at up to 6× the note's read rate (BLEND 1,
/// `I` = 5); the rate's corners alone stay at about −58 dB or under (a sine
/// carrier under a saw modulator), and a saw modulator adds 1.5 to 19.1 dB
/// more at BLEND 1 (private issue #650). CRSE 7 plays the just
/// fifth (FINE2 +1.955 cents, 3:2): the equal-tempered fifth's lattice,
/// cut at `|i|, |j| ≤ ⌈24 kHz / f⌉ + 1`, leaves a high harmonic's in-band
/// sidebands unmasked (the same patch reads −31 dB there, −59 dB at 3:2).
#[test]
fn fm_alias_gate() {
    let mut worst = (f64::MIN, String::new());
    let mut least = (f64::MAX, String::new());
    let mut fails = Vec::new();
    println!("note crse wave1 wave2 blend: on / off dB (carson Hz)");
    for note in [C5, C6] {
        for crse in [0i8, 12, 7] {
            let (fine, ratio) = match crse {
                7 => (1.955, 1.5),
                c => (0.0, 2f32.powf(f32::from(c) / 12.0)),
            };
            for w1 in ANCHORS {
                for w2 in [0.0, SAW] {
                    for blend in [0.25, 0.5, 1.0] {
                        let base = VaParams {
                            wave: [w1, w2],
                            coarse: [0, crse],
                            fine: [0.0, fine],
                            ..fm(blend)
                        };
                        let fc = hz_of(note);
                        let fm_ = fc * ratio;
                        let i = fm_index(blend, fc / SR as f32);
                        let carson = (i + 1.0) * fm_ + fc;
                        let gate = |prep: fn(&mut VaEngine)| {
                            let y = take_with(prep, note, base, GATE_BLOCKS, |_, _| {});
                            alias_db_lattice(&y, SR as f32, fc, fm_)
                        };
                        let on = gate(|_| {});
                        let off = gate(VaEngine::uncorrected);
                        let fits = carson < 24_000.0 && w1 < SAW;
                        let at = format!("{note} {crse:+} {w1:.3} {w2:.3} {blend}");
                        let tag = if fits { "" } else { " exception" };
                        println!("{at}: {on:.1} / {off:.1} ({carson:.0}){tag}");
                        if fits {
                            if on > -45.0 {
                                fails.push(format!("{at}: {on:.1} dB > −45"));
                            }
                            if on > worst.0 {
                                worst = (on, at);
                            }
                        } else {
                            if on > off - 10.0 {
                                fails.push(format!("{at}: {on:.1} vs off {off:.1}"));
                            }
                            if off - on < least.0 {
                                least = (off - on, at);
                            }
                        }
                    }
                }
            }
        }
    }
    println!("worst where Carson fits {:.1} dB at {}", worst.0, worst.1);
    println!(
        "least correction elsewhere {:.1} dB at {}",
        least.0, least.1
    );
    assert!(fails.is_empty(), "{fails:#?}");
}

/// FM moves OSC 1's phase, not its level: at INIT's tuning (FINE2 +6) and
/// a fifth apart every BLEND sounds as loud as OSC 1 alone. Locked 1:1
/// (both at phase 0, STAB 1) the read's path repeats every cycle and plays
/// a wave of its own level: printed, not gated (+2.0 dB at BLEND 0.5,
/// −1.3 dB at 1, ADR 0159).
#[test]
fn fm_level_matches_mix() {
    let blocks = 2 * SR as usize / B;
    for (coarse, fine, gated) in [(0i8, 6.0, true), (7, 0.0, true), (0, 0.0, false)] {
        let saws = VaParams {
            wave: [SAW; 2],
            coarse: [0, coarse],
            fine: [0.0, fine],
            ..sine()
        };
        let mix = rms(&held(
            C4,
            VaParams {
                mode: VaMode::Mix,
                amount: 0.0,
                ..saws
            },
            blocks,
        ));
        for blend in [0.0, 0.5, 1.0] {
            let y = held(
                C4,
                VaParams {
                    mode: VaMode::Fm,
                    amount: blend,
                    ..saws
                },
                blocks,
            );
            let db = 20.0 * (rms(&y) / mix).log10();
            let tag = if gated { "" } else { " (locked, not gated)" };
            println!("CRSE2 {coarse} FINE2 {fine} BLEND {blend}: {db:+.2} dB{tag}");
            if gated {
                assert!(
                    db.abs() <= 1.5,
                    "CRSE2 {coarse} FINE2 {fine} BLEND {blend}: {db}"
                );
            }
        }
    }
}

/// Leaving FM the read becomes the clock at the duck's swap: 20 blocks on,
/// MIX plays as a fresh engine started at the settled phases does. With
/// the sub on (its flip may land a sample apart at the swap, under the
/// duck's zero) the change still never clicks.
#[test]
fn leaving_fm_settles() {
    let base = VaParams {
        wave: [SAW, 0.3],
        shape: [0.3, 0.0],
        fine: [0.0, 6.0],
        coarse: [0, 7],
        stability: 1.0,
        sub: 0.0,
        amount: 0.8,
        mode: VaMode::Fm,
    };
    let mix = VaParams {
        mode: VaMode::Mix,
        ..base
    };
    let tail = 10;
    let (left, settled) = with_engine(|e| {
        let mut p = base;
        e.hold();
        e.note_on(A4, &p, PartId::new(0).unwrap(), SR);
        run(e, &mut p, AT, |_, _| {});
        p.mode = VaMode::Mix;
        // the swap ends the duck's second block down
        run(e, &mut p, 2, |_, _| {});
        assert_eq!(e.mode(), VaMode::Mix);
        let at = e.osc_phase();
        let y = run(e, &mut p, AT + tail, |_, _| {});
        (y, at)
    });
    let fresh = with_engine(|f| {
        let mut p = mix;
        f.hold();
        f.note_on(A4, &p, PartId::new(0).unwrap(), SR);
        f.set_osc_phase(settled);
        run(f, &mut p, AT + tail, |_, _| {})
    });
    let err = left[AT * B..]
        .iter()
        .zip(&fresh[AT * B..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!("FM → MIX, {AT} blocks on: max |left − fresh| {err:.2e}");
    assert!(err <= 1e-4, "{err}");
    // the sub on, both ways
    for (m0, m1) in [(VaMode::Fm, VaMode::Mix), (VaMode::Mix, VaMode::Fm)] {
        let base = VaParams { sub: 0.5, ..base };
        let y = take_with(
            |_| {},
            A4,
            base,
            2 * AT,
            |b, p| p.mode = if b < AT { m0 } else { m1 },
        );
        let held: Vec<_> = [m0, m1]
            .iter()
            .map(|&m| held(A4, VaParams { mode: m, ..base }, 6 * AT))
            .collect();
        let (d, lim) = click(&y, &held);
        println!(
            "SUB 0.5 {m0:?} → {m1:?}: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
            100.0 * d / lim
        );
        assert!(d <= lim, "{m0:?} → {m1:?}: {d} > {lim}");
    }
}

// ---- LOOPFM (#613) ----

use chimera_core::dsp::va::lbfm::{LEAN, PEAK, b0_cap, lbfm, render_loop, scale_of, target_b0};

const R_RMS: f64 = 0.2;
const TWO_THIRDS: f32 = 2.0 / 3.0;

/// Sines' path (WAVE is the loop's scale here, not a shape), STAB 1, no
/// sub.
fn loopfm(amount: f32, wave: f32) -> VaParams {
    VaParams {
        mode: VaMode::LoopFm,
        amount,
        wave: [wave; 2],
        ..sine()
    }
}

/// The paper's `b0` for feedback `b` (eq. 26), `−b/(1 + √(1 − b²))`.
fn b0_of(b: f32) -> f32 {
    -b / (1.0 + (1.0 - b * b).sqrt())
}

/// The paper's recursion (eq. 11) at 16× the rate, A2, B = 0.5: one
/// period of its real part has the closed form's shape.
#[test]
fn loopfm_matches_the_recursion() {
    let (fs, f0, b) = (16.0 * 48_000.0f64, 110.0f64, 0.5f64);
    let w0 = std::f64::consts::TAU * f0;
    let wc = w0 / (1.0 - b * b).sqrt();
    let t = 1.0 / fs;
    let n = (fs / f0).round() as usize;
    let (mut re, mut im) = (1.0f64, 0.0f64);
    let mut rec = Vec::with_capacity(n);
    for _ in 0..n {
        rec.push(re);
        let (s, c) = ((wc + b * wc * re) * t).sin_cos();
        (re, im) = (re * c - im * s, re * s + im * c);
    }
    let b0 = f64::from(b0_of(b as f32));
    let s = R_RMS * 2f64.sqrt() / (1.0 - b0 * b0).sqrt();
    let rec_rms = (rec.iter().map(|v| (v - b0).powi(2)).sum::<f64>() / n as f64).sqrt();
    let mut best = f64::MAX;
    // the unit delay detunes the recursion a hair: align phase and period
    for dp in -20..=20 {
        for dn in -10..=10 {
            let off = f64::from(dp) * 5e-4;
            let len = n as f64 * (1.0 + f64::from(dn) * 2e-4);
            let err: f64 = (0..n)
                .map(|i| {
                    let ph = (i as f64 / len + off).rem_euclid(1.0) as f32;
                    let cf = f64::from(lbfm(ph, b0 as f32)) / s + b0;
                    (rec[i] - cf).powi(2)
                })
                .sum();
            best = best.min((err / n as f64).sqrt() / rec_rms);
        }
    }
    println!("closed form vs recursion: {:.3} % RMS", 100.0 * best);
    assert!(best <= 0.01, "{best}");
}

#[test]
fn loopfm_stays_in_tune() {
    let f0 = hz_of(A4);
    for blend in [0.0, 0.5, 1.0] {
        for wave in [0.0, TWO_THIRDS, 1.0] {
            let y = held(A4, loopfm(blend, wave), PITCH_BLOCKS);
            let f = peak_hz(&y, SR as f32, f0);
            let cents = 1200.0 * (f / f64::from(f0)).log2();
            println!("LOOPFM BLEND {blend} WAVE {wave:.2}: {f:.3} Hz ({cents:+.3} cents)");
            assert!(cents.abs() <= 1.0, "BLEND {blend} WAVE {wave}: {cents}");
        }
    }
}

/// One oscillator over 40 whole periods (100 samples each).
fn one_osc(b0: f32) -> Vec<f32> {
    const N: usize = 4000;
    let mut out = [0.0f32; N];
    let mut mk = [[0.0f32; 2]; N];
    let (mut ph, mut jit) = (0.0f32, 1.0f32);
    render_loop::<N, false>(
        &mut ph,
        (b0, 0.0),
        (0.01, 0.0),
        &mut out,
        &mut mk,
        &mut jit,
        &mut || 1.0,
    );
    out.to_vec()
}

#[test]
fn loopfm_is_dc_free_at_level() {
    // RMS R up to LEAN, then the peak held at PEAK (ADR 0166)
    // −0.9 is the cap at 100 samples a period
    for b0 in [0.0f32, -0.3, -LEAN, -0.8, -0.9] {
        let y = one_osc(b0);
        let mean = y.iter().map(|&v| f64::from(v)).sum::<f64>() / y.len() as f64;
        let r = rms(&y) / R_RMS;
        let pk = y.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        println!(
            "b0 {b0}: mean {mean:+.2e}, RMS {r:.4} R, peak {:.3} PEAK",
            pk / PEAK
        );
        assert!(mean.abs() <= 1e-4, "b0 {b0}: {mean}");
        if -b0 <= LEAN {
            assert!((r - 1.0).abs() <= 0.01, "b0 {b0}: {r}");
        } else {
            assert!(
                r < 1.0 && (pk / PEAK - 1.0).abs() <= 0.01,
                "b0 {b0}: {r} {pk}"
            );
        }
    }
    // the two halves meet at LEAN
    let (lo, hi) = (scale_of(-LEAN + 1e-4), scale_of(-LEAN - 1e-4));
    assert!((lo / hi - 1.0).abs() <= 1e-3, "{lo} {hi}");
}

/// The cap keeps the first alias at −45 dB at the deepest loop, C5–G8.
#[test]
fn loopfm_alias_gate() {
    for note in [72u8, 79, 84, 91, 96, 103] {
        for wave in [TWO_THIRDS, 1.0] {
            let fc = hz_of(note);
            let y = held(note, loopfm(1.0, wave), GATE_BLOCKS);
            let db = alias_db_lattice(&y, SR as f32, fc, fc);
            let cap = b0_cap(fc / SR as f32);
            println!("LOOPFM note {note} WAVE {wave:.2}: alias {db:.1} dB, |b0| cap {cap:.3}");
            assert!(db <= -45.0, "note {note} WAVE {wave}: {db}");
        }
    }
}

#[test]
fn loopfm_blend_zero_is_osc1_sine() {
    // OSC 2 detuned: BLEND 0 plays OSC 1 alone, a sine at R
    let p = VaParams {
        fine: [0.0, 40.0],
        coarse: [0, 7],
        ..loopfm(0.0, TWO_THIRDS)
    };
    let y = held(C4, p, 2 * SR as usize / B);
    let (re, im) = common::spectrum::fundamental(&y, SR as f32, hz_of(C4));
    let amp = f64::from(re).hypot(f64::from(im));
    let r = rms(&y) / R_RMS;
    println!("BLEND 0: fundamental {amp:.4}, RMS {r:.4} R");
    assert!((r - 1.0).abs() <= 0.01, "{r}");
    // all of its energy is the fundamental
    assert!((amp / (R_RMS * 2f64.sqrt()) - 1.0).abs() <= 0.01, "{amp}");
}

/// B's curve (ADR 0166): the −45 dB harmonic `100^e + 1`, `e =
/// 1.5·WAVE·BLEND`, so `|b0|` climbs evenly to 0.95.
#[test]
fn loopfm_curve_is_even() {
    let at = |x: f32, w: f32| -target_b0(x, w);
    assert_eq!(at(0.0, TWO_THIRDS), 0.0);
    assert_eq!(at(1.0, 0.0), 0.0);
    // BLEND 32, 64, 96, 127 at WAVE 2/3
    for (blend, want) in [(32, 0.197), (64, 0.601), (96, 0.853), (127, 0.9495)] {
        let got = at(blend as f32 / 127.0, TWO_THIRDS);
        println!("BLEND {blend}: |b0| {got:.4}");
        assert!((got - want).abs() <= 2e-3, "BLEND {blend}: {got}");
    }
    // the −45 dB harmonic at the top is the 101st
    assert!((at(1.0, TWO_THIRDS).powi(100) - 0.0056).abs() <= 1e-4);
    // WAVE 1 gets there at BLEND 2/3 and holds; the sign is the paper's
    assert!((at(TWO_THIRDS, 1.0) - at(1.0, TWO_THIRDS)).abs() <= 1e-6);
    assert_eq!(at(0.9, 1.0), at(1.0, 1.0));
    let mut last = 0.0;
    for i in 1..=127 {
        let b = at(i as f32 / 127.0, TWO_THIRDS);
        assert!(
            b > last && target_b0(i as f32 / 127.0, TWO_THIRDS) < 0.0,
            "{i}"
        );
        last = b;
    }
    // the cap shrinks with pitch and binds from about C4 at the top
    assert_eq!(b0_cap(0.5), 0.0);
    assert!(b0_cap(0.45) < 1e-3);
    assert!(b0_cap(0.01) > b0_cap(0.04));
    assert!(b0_cap(hz_of(48) / SR as f32) > at(1.0, TWO_THIRDS));
    assert!(b0_cap(hz_of(C4) / SR as f32) < at(1.0, TWO_THIRDS));
}

/// INIT's VA, held, through the engine: LOOPFM's peak stays within ~6 dB
/// of MIX's saws (the filter LEVEL clipper's knee) at every BLEND, C2–C5,
/// and its RMS within −10..+2.5 dB.
#[test]
fn loopfm_peak_stays_near_mix() {
    let init = VaParams {
        fine: [0.0, 6.0],
        stability: 0.7,
        ..VaParams::default()
    };
    let peak = |y: &[f32]| y[4 * B..].iter().fold(0.0f32, |m, &v| m.max(v.abs()));
    let blocks = SR as usize / B;
    for note in [36u8, 48, 60, 72] {
        let mix = held(
            note,
            VaParams {
                mode: VaMode::Mix,
                ..init
            },
            blocks,
        );
        let (mp, mr) = (peak(&mix), rms(&mix));
        for blend in [32u8, 64, 80, 96, 112, 127] {
            let p = VaParams {
                mode: VaMode::LoopFm,
                amount: f32::from(blend) / 127.0,
                ..init
            };
            let y = held(note, p, blocks);
            let pk = 20.0 * f64::from(peak(&y) / mp).log10();
            let lv = 20.0 * (rms(&y) / mr).log10();
            println!("note {note} BLEND {blend}: peak {pk:+.1} dB, RMS {lv:+.1} dB re MIX");
            assert!(pk <= 6.5, "note {note} BLEND {blend}: peak {pk}");
            assert!(
                (-10.0..=2.5).contains(&lv),
                "note {note} BLEND {blend}: {lv}"
            );
        }
    }
}

/// BLEND swept under a held note, a quarter of the knob at a time and the
/// whole knob in one block, never clicks.
#[test]
fn loopfm_blend_sweeps_do_not_click() {
    let base = VaParams {
        fine: [0.0, 6.0],
        ..loopfm(0.0, TWO_THIRDS)
    };
    for note in [36u8, A4] {
        let mut spans: Vec<(f32, f32, usize)> = (0..4)
            .map(|q| (q as f32 / 4.0, (q + 1) as f32 / 4.0, AT))
            .collect();
        spans.extend([(0.0, 1.0, 1), (1.0, 0.0, 1)]);
        for (lo, hi, len) in spans {
            let y = take_with(
                |_| {},
                note,
                VaParams { amount: lo, ..base },
                2 * AT + len,
                |b, p| {
                    let t = (b.saturating_sub(AT) as f32 / len as f32).min(1.0);
                    p.amount = lo + (hi - lo) * t;
                },
            );
            let held: Vec<_> = [lo, hi]
                .iter()
                .map(|&x| held(note, VaParams { amount: x, ..base }, 2 * AT + len))
                .collect();
            let (d, lim) = click(&y, &held);
            println!("note {note} BLEND {lo} → {hi} in {len}: Δ² {d:.3e} / limit {lim:.3e}");
            assert!(d <= lim, "note {note} {lo} → {hi}: {d} > {lim}");
        }
    }
}

/// BLEND 0 of two sines plays OSC 1 at MIX's level.
#[test]
fn loopfm_level_matches_mix() {
    let blocks = 2 * SR as usize / B;
    let sines = VaParams {
        coarse: [0, 7],
        fine: [0.0, 6.0],
        ..sine()
    };
    let mix = rms(&held(
        C4,
        VaParams {
            mode: VaMode::Mix,
            amount: 0.0,
            ..sines
        },
        blocks,
    ));
    let y = held(
        C4,
        VaParams {
            mode: VaMode::LoopFm,
            amount: 0.0,
            wave: [TWO_THIRDS; 2],
            ..sines
        },
        blocks,
    );
    let db = 20.0 * (rms(&y) / mix).log10();
    println!("LOOPFM BLEND 0 vs MIX: {db:+.2} dB");
    assert!(db.abs() <= 1.5, "{db}");
}

#[test]
fn loopfm_credits_its_paper() {
    use chimera_core::dsp::provenance::Provenance;
    let cites: Vec<_> = chimera_core::dsp::va::lbfm::PROVENANCE
        .iter()
        .filter_map(|p| match p {
            Provenance::Textbook { cite } => Some(*cite),
            _ => None,
        })
        .collect();
    assert!(
        cites
            .iter()
            .any(|c| c.contains("Smyth") && c.contains("CC-BY 4.0"))
    );
}

// ---- AM (#603) ----

fn am(amount: f32) -> VaParams {
    VaParams {
        mode: VaMode::Am,
        amount,
        ..sine()
    }
}

/// OSC 2 as a slow sine (8 Hz at note 24, CRSE2 −24): at the knee (BLEND
/// 85) OSC 1's envelope dips to near 0 and never inverts, the product
/// keeping OSC 1's sign; past it (BLEND 1, ring) it inverts.
#[test]
fn am_is_unipolar() {
    let blocks = SR as usize / B;
    let slow = VaParams {
        coarse: [0, -24],
        ..am(AM_KNEE)
    };
    let a = held(24, slow, blocks);
    let one = held(
        24,
        VaParams {
            mode: VaMode::Mix,
            amount: 0.0,
            ..slow
        },
        blocks,
    );
    let (mut low, mut flips) = (f32::MAX, 0);
    for (&y, &x) in a.iter().zip(&one).skip(2 * B) {
        if x.abs() > 0.05 {
            low = low.min(y / x);
            flips += usize::from(y * x < -1e-4);
        }
    }
    println!("AM BLEND 85, 8 Hz: lowest gain {low:.4}, sign flips {flips}");
    assert_eq!(flips, 0);
    assert!((0.0..0.1).contains(&low), "{low}");
    let ring = held(
        24,
        VaParams {
            amount: 1.0,
            ..slow
        },
        blocks,
    );
    let flips = ring
        .iter()
        .zip(&one)
        .skip(2 * B)
        .filter(|&(&y, &x)| x.abs() > 0.05 && y * x < -1e-4)
        .count();
    println!("AM BLEND 127: sign flips {flips}");
    assert!(flips > 1000, "{flips}");
}

/// Unlike RING, AM keeps OSC 1's line: a fifth up, at the knee.
#[test]
fn am_sidebands_softer_than_ring() {
    let f0 = hz_of(C4);
    let blocks = 2 * SR as usize / B;
    let fifth = VaParams {
        coarse: [0, 7],
        fine: [0.0, 1.955],
        ..sine()
    };
    let line = |mode| {
        let y = held(
            C4,
            VaParams {
                mode,
                amount: if mode == VaMode::Am { AM_KNEE } else { 1.0 },
                ..fifth
            },
            blocks,
        );
        let (re, im) = common::spectrum::fundamental(&y[y.len() - 32768..], SR as f32, f0);
        20.0 * f64::from(re.hypot(im)).log10()
    };
    let alone = {
        let y = held(
            C4,
            VaParams {
                mode: VaMode::Mix,
                amount: 0.0,
                ..fifth
            },
            blocks,
        );
        let (re, im) = common::spectrum::fundamental(&y[y.len() - 32768..], SR as f32, f0);
        20.0 * f64::from(re.hypot(im)).log10()
    };
    let (a, r) = (line(VaMode::Am) - alone, line(VaMode::Ring) - alone);
    println!("carrier vs OSC 1 alone: AM {a:+.1} dB, RING {r:+.1} dB");
    assert!(a.abs() <= 3.0, "{a}");
    assert!(r <= -20.0, "{r}");
}

/// AM plays as loud as OSC 1 alone at every BLEND (saws, unrelated pitch
/// and a fifth).
#[test]
fn am_level_matches_mix() {
    let blocks = 2 * SR as usize / B;
    for (coarse, fine) in [(0i8, 6.0), (7, 0.0), (12, 0.0)] {
        for w in [0.0, SAW] {
            let p = VaParams {
                wave: [SAW, w],
                coarse: [0, coarse],
                fine: [0.0, fine],
                ..sine()
            };
            let mix = rms(&held(
                C4,
                VaParams {
                    mode: VaMode::Mix,
                    amount: 0.0,
                    ..p
                },
                blocks,
            ));
            for blend in [0.0, 0.25, 0.5, AM_KNEE, 0.85, 1.0] {
                let y = held(
                    C4,
                    VaParams {
                        mode: VaMode::Am,
                        amount: blend,
                        ..p
                    },
                    blocks,
                );
                let db = 20.0 * (rms(&y) / mix).log10();
                println!("CRSE2 {coarse} FINE2 {fine} WAVE2 {w:.2} BLEND {blend}: {db:+.2} dB");
                assert!(
                    db.abs() <= 1.5,
                    "CRSE2 {coarse} WAVE2 {w} BLEND {blend}: {db}"
                );
            }
        }
    }
}

/// C5 over the lattice. Up to the knee, wherever MIX at the same BLEND
/// passes −45 dB, AM does (the product of two band-limited waves adds lines
/// at |f1 ± f2|). Past it AM runs into RING's product and is held to RING's
/// own figure at BLEND 1, `max(−45, RING + 0.5)` (ADR 0176: the product's
/// slope kinks, #660).
#[test]
fn am_alias_gate() {
    let mut fails = Vec::new();
    let mut worst = (f64::MIN, String::new());
    for crse in [0i8, 12, 7] {
        // INIT's FINE2 +6 for the unison and the octave (RING's gate does
        // the same); the just fifth as FM's
        let fine = if crse == 7 { 1.955 } else { 6.0 };
        let ratio = 2f32.powf((f32::from(crse) * 100.0 + fine) / 1200.0);
        for w1 in ANCHORS {
            for w2 in [0.0, SAW] {
                for blend in [0.25, 0.5, AM_KNEE, 0.85, 1.0] {
                    let base = VaParams {
                        wave: [w1, w2],
                        coarse: [0, crse],
                        fine: [0.0, fine],
                        ..am(blend)
                    };
                    let (fc, f2) = (hz_of(C5), hz_of(C5) * ratio);
                    let gate = |mode, amount| {
                        let y = held(
                            C5,
                            VaParams {
                                mode,
                                amount,
                                ..base
                            },
                            GATE_BLOCKS,
                        );
                        alias_db_lattice(&y, SR as f32, fc, f2)
                    };
                    let (a, m) = (gate(VaMode::Am, blend), gate(VaMode::Mix, blend));
                    let at = format!("{crse:+} {w1:.3} {w2:.3} {blend:.3}");
                    let limit = if blend > AM_KNEE {
                        let r = gate(VaMode::Ring, 1.0);
                        println!("{at}: AM {a:.1} / MIX {m:.1} / RING 1 {r:.1} dB");
                        (r + 0.5).max(-45.0)
                    } else {
                        println!("{at}: AM {a:.1} / MIX {m:.1} dB");
                        // saw × saw at the fifth, full AM: the product's
                        // slope kinks escape the 2-point kernel (RING reads
                        // −37.4 dB there); an exception, held at −43 (AM
                        // reads −43.5; filed as
                        // private issue #660)
                        if crse == 7 && w1 == SAW && w2 == SAW && blend == AM_KNEE {
                            -43.0
                        } else {
                            -45.0
                        }
                    };
                    if m <= -45.0 {
                        if a > worst.0 {
                            worst = (a, at.clone());
                        }
                        if a > limit {
                            fails.push(format!("{at}: AM {a:.1} dB > {limit:.1}"));
                        }
                    }
                }
            }
        }
    }
    println!("worst where MIX passes {:.1} dB at {}", worst.0, worst.1);
    assert!(fails.is_empty(), "{fails:#?}");
}

/// Today's AM (main before #675) at BLEND `x'`, saws, SUB 0.3, C2, recorded:
/// (CRSE2, FINE2, x', Σy² over 128 blocks, y[4096..4112]).
#[rustfmt::skip]
const AM_TODAY: [(i8, f32, f32, f64, [f32; 16]); 12] = [
    (0, 6.0, 0.25, 3.384703155775411e2, [-1.0969913e-1, -1.10553004e-1, -1.11406155e-1, -1.1225862e-1, -1.1311041e-1, -1.1396145e-1, -1.148118e-1, -1.15661435e-1, -1.1651038e-1, -1.1735861e-1, -1.18206136e-1, -1.1905299e-1, -1.1989911e-1, -1.2074453e-1, -1.2158924e-1, -1.22433245e-1]),
    (0, 6.0, 0.5, 3.5717867415594753e2, [-1.0689808e-1, -1.0767725e-1, -1.08454816e-1, -1.0923075e-1, -1.100051e-1, -1.107778e-1, -1.1154888e-1, -1.1231835e-1, -1.130862e-1, -1.1385243e-1, -1.14617035e-1, -1.1538005e-1, -1.1614142e-1, -1.16901174e-1, -1.1765929e-1, -1.1841582e-1]),
    (0, 6.0, 0.75, 3.885137980230389e2, [-1.02292e-1, -1.0295805e-1, -1.03621274e-1, -1.0428171e-1, -1.0493936e-1, -1.0559419e-1, -1.062462e-1, -1.06895424e-1, -1.0754182e-1, -1.0818544e-1, -1.08826235e-1, -1.0946426e-1, -1.1009944e-1, -1.1073184e-1, -1.1136143e-1, -1.1198821e-1]),
    (0, 6.0, 1.0, 4.3743384203271006e2, [-9.478289e-2, -9.527822e-2, -9.576927e-2, -9.625603e-2, -9.673853e-2, -9.721673e-2, -9.769066e-2, -9.816028e-2, -9.862564e-2, -9.90867e-2, -9.954349e-2, -9.9996e-2, -1.0044422e-1, -1.00888155e-1, -1.0132781e-1, -1.01763174e-1]),
    (7, 1.955, 0.25, 3.308850273750051e2, [-1.1321329e-1, -1.14115484e-1, -1.15016624e-1, -1.1591669e-1, -1.1681577e-1, -1.17713735e-1, -1.1861066e-1, -1.1950652e-1, -1.20401345e-1, -1.2129511e-1, -1.2218781e-1, -1.2307952e-1, -1.23970106e-1, -1.2485967e-1, -1.2574817e-1, -1.2663563e-1]),
    (7, 1.955, 0.5, 3.350467994570457e2, [-1.14977404e-1, -1.1586767e-1, -1.1675551e-1, -1.1764094e-1, -1.1852396e-1, -1.19404554e-1, -1.20282724e-1, -1.21158466e-1, -1.2203179e-1, -1.22902706e-1, -1.2377118e-1, -1.2463728e-1, -1.2550093e-1, -1.2636216e-1, -1.2722096e-1, -1.2807734e-1]),
    (7, 1.955, 0.75, 3.400500056305556e2, [-1.162858e-1, -1.1714425e-1, -1.179985e-1, -1.1884858e-1, -1.1969448e-1, -1.2053616e-1, -1.2137364e-1, -1.2220695e-1, -1.2303606e-1, -1.23860985e-1, -1.24681704e-1, -1.2549827e-1, -1.2631062e-1, -1.2711877e-1, -1.2792271e-1, -1.2872249e-1]),
    (7, 1.955, 1.0, 3.451917193435088e2, [-1.16158776e-1, -1.16948016e-1, -1.1773085e-1, -1.1850728e-1, -1.1927736e-1, -1.2004098e-1, -1.2079822e-1, -1.2154905e-1, -1.22293465e-1, -1.23031475e-1, -1.23763114e-1, -1.24488354e-1, -1.2520717e-1, -1.259196e-1, -1.2662558e-1, -1.2732519e-1]),
    (6, 0.0, 0.25, 3.3118468758382784e2, [-1.0520505e-1, -1.0597487e-1, -1.06743686e-1, -1.0751152e-1, -1.0827837e-1, -1.09044224e-1, -1.0980908e-1, -1.1057293e-1, -1.1133581e-1, -1.1209767e-1, -1.12858556e-1, -1.1361848e-1, -1.1437738e-1, -1.1513528e-1, -1.15892194e-1, -1.16648115e-1]),
    (6, 0.0, 0.5, 3.355462961454688e2, [-9.656589e-2, -9.7151816e-2, -9.773547e-2, -9.831684e-2, -9.889595e-2, -9.9472776e-2, -1.000473e-1, -1.0061955e-1, -1.0118951e-1, -1.0175718e-1, -1.02322586e-1, -1.0288572e-1, -1.0344657e-1, -1.0400513e-1, -1.045614e-1, -1.051154e-1]),
    (6, 0.0, 0.75, 3.404691016617953e2, [-8.439612e-2, -8.472745e-2, -8.505483e-2, -8.537826e-2, -8.5697755e-2, -8.601329e-2, -8.632485e-2, -8.663249e-2, -8.693615e-2, -8.7235875e-2, -8.753164e-2, -8.7823465e-2, -8.811133e-2, -8.839524e-2, -8.86752e-2, -8.895121e-2]),
    (6, 0.0, 1.0, 3.449237987603479e2, [-6.744648e-2, -6.743054e-2, -6.740856e-2, -6.738055e-2, -6.73465e-2, -6.730642e-2, -6.7260295e-2, -6.720814e-2, -6.7149945e-2, -6.708571e-2, -6.701545e-2, -6.6939145e-2, -6.68568e-2, -6.676843e-2, -6.6674024e-2, -6.657357e-2]),
];

/// At BLEND ≤ 85 AM is today's AM rescaled: BLEND `x'·AM_KNEE` renders
/// today's BLEND `x'`, within float rounding of the mapping (the per-sample
/// loop is the same code).
#[test]
fn am_below_the_knee_is_todays() {
    let mut worst = (0.0f64, 0.0f64);
    for (crse, fine, xp, ss, ys) in AM_TODAY {
        let p = VaParams {
            wave: [SAW; 2],
            shape: [0.0; 2],
            coarse: [0, crse],
            fine: [0.0, fine],
            mode: VaMode::Am,
            amount: xp * AM_KNEE,
            sub: 0.3,
            stability: 1.0,
        };
        let y = held(36, p, 128);
        let got: f64 = y.iter().map(|&v| f64::from(v).powi(2)).sum();
        let rel = (got / ss - 1.0).abs();
        let d = y[4096..4112]
            .iter()
            .zip(ys)
            .map(|(&a, b)| f64::from((a - b).abs()))
            .fold(0.0, f64::max);
        println!("CRSE2 {crse} x' {xp}: Σy² rel {rel:.2e}, max |Δy| {d:.2e}");
        assert!(rel <= 1e-5, "CRSE2 {crse} x' {xp}: {rel}");
        assert!(d <= 1e-5, "CRSE2 {crse} x' {xp}: {d}");
        worst = (worst.0.max(rel), worst.1.max(d));
    }
    println!("worst Σy² rel {:.2e}, |Δy| {:.2e}", worst.0, worst.1);
}

/// The reach is continuous and rising through the knee, 1 there, 2 at BLEND
/// 1; a sweep across it on a held note clicks no more than the held BLENDs
/// on either side.
#[test]
fn am_crossover_is_continuous() {
    let e = 1e-4;
    assert!((am_reach(AM_KNEE) - 1.0).abs() < 1e-6);
    assert!((am_reach(1.0) - 2.0).abs() < 1e-6);
    // no step: each side within its slope (127/85 below, 127/42 above)
    assert!((am_reach(AM_KNEE - e) - 1.0).abs() <= 1.51 * e);
    assert!((am_reach(AM_KNEE + e) - 1.0).abs() <= 3.03 * e);
    let mut last = -1.0;
    for i in 0..=1270 {
        let u = am_reach(i as f32 / 1270.0);
        assert!(u > last, "{i}: {u} after {last}");
        last = u;
    }
    let base = VaParams {
        sub: 0.5,
        fine: [0.0, 6.0],
        coarse: [0, 7],
        ..am(0.6)
    };
    let (lo, hi) = (0.6, 0.75);
    let y = take_with(
        |_| {},
        A4,
        base,
        3 * AT,
        |b, p| {
            p.amount = lo + (hi - lo) * (b.saturating_sub(AT) as f32 / AT as f32).min(1.0);
        },
    );
    let held: Vec<_> = [lo, hi]
        .iter()
        .map(|&x| held(A4, VaParams { amount: x, ..base }, 3 * AT))
        .collect();
    let (d, lim) = click(&y, &held);
    println!("BLEND {lo} → {hi} across the knee: Δ² {d:.3e} / limit {lim:.3e}");
    assert!(d <= lim, "{d} > {lim}");
}

/// At BLEND 1 AM is RING's product at BLEND 1: the same wave within level
/// (correlation, saws, the fifth).
#[test]
fn am_top_is_ring() {
    let p = VaParams {
        wave: [SAW; 2],
        coarse: [0, 7],
        fine: [0.0, 1.955],
        ..am(1.0)
    };
    let a = held(C4, p, 200);
    let r = held(
        C4,
        VaParams {
            mode: VaMode::Ring,
            ..p
        },
        200,
    );
    let (a, r) = (&a[100 * B..], &r[100 * B..]);
    let xy: f64 = a
        .iter()
        .zip(r)
        .map(|(&x, &y)| f64::from(x) * f64::from(y))
        .sum();
    let c = xy / (rms(a) * rms(r) * a.len() as f64);
    println!(
        "AM 1 vs RING 1: r {c:.6}, level {:+.2} dB",
        20.0 * (rms(a) / rms(r)).log10()
    );
    assert!(c >= 0.9999, "{c}");
}

/// Unison's ring term carries DC (constant at FINE2 0, FINE2's beat at +6).
/// AM at BLEND 1 carries no more than RING at BLEND 1, and through the
/// voice's DC blocker (ADR 0060) it is under −55 dB re the RMS (ADR 0176;
/// the half-second window's own leakage reads about −59).
#[test]
fn am_ring_dc_is_rings() {
    let blocks = 2 * SR as usize / B;
    for fine in [0.0f32, 6.0] {
        let p = VaParams {
            wave: [SAW; 2],
            fine: [0.0, fine],
            ..am(1.0)
        };
        let dc = |y: &[f32]| {
            let t = &y[y.len() - SR as usize / 2..];
            let m = t.iter().map(|&v| f64::from(v)).sum::<f64>() / t.len() as f64;
            20.0 * (m.abs() / rms(t)).max(1e-12).log10()
        };
        let a = held(C4, p, blocks);
        let r = held(
            C4,
            VaParams {
                mode: VaMode::Ring,
                ..p
            },
            blocks,
        );
        let mut blocked = a.clone();
        DcBlocker::new(DC_HZ, SR).run(&mut blocked);
        let (da, dr, db) = (dc(&a), dc(&r), dc(&blocked));
        println!("FINE2 {fine}: DC AM {da:.1} / RING {dr:.1} / AM blocked {db:.1} dB re RMS");
        assert!(da <= dr + 0.5, "FINE2 {fine}: {da} vs RING {dr}");
        if fine == 0.0 {
            assert!(db <= -55.0, "{db}");
        }
    }
}

/// Entering and leaving AM never clicks beyond the held modes' own.
#[test]
fn am_mode_changes_do_not_click() {
    for (m0, m1) in [(VaMode::Mix, VaMode::Am), (VaMode::Am, VaMode::Mix)] {
        let base = VaParams {
            sub: 0.5,
            fine: [0.0, 6.0],
            coarse: [0, 7],
            amount: 0.8,
            ..sine()
        };
        let y = take_with(
            |_| {},
            A4,
            base,
            2 * AT,
            |b, p| p.mode = if b < AT { m0 } else { m1 },
        );
        let held: Vec<_> = [m0, m1]
            .iter()
            .map(|&m| held(A4, VaParams { mode: m, ..base }, 6 * AT))
            .collect();
        let (d, lim) = click(&y, &held);
        println!("{m0:?} → {m1:?}: Δ² {d:.3e} / limit {lim:.3e}");
        assert!(d <= lim, "{m0:?} → {m1:?}: {d} > {lim}");
    }
}

// ---- MAX (#605) ----

fn max(amount: f32) -> VaParams {
    VaParams {
        mode: VaMode::Max,
        amount,
        ..sine()
    }
}

/// BLEND 1, away from the crossings (the kernel's two samples a side), the
/// output is the larger of the two oscillators, one sample late. OSC 1
/// and OSC 2 alone are MIX at AMOUNT 0 and 1.
#[test]
fn max_is_the_larger() {
    let alone = |mode, amount| VaParams {
        mode,
        amount,
        wave: [SAW, 0.3],
        shape: [0.0, 0.4],
        coarse: [0, 7],
        fine: [0.0, 6.0],
        ..sine()
    };
    let n = 60 * B;
    let a = held(C4, alone(VaMode::Mix, 0.0), 60);
    let b = held(C4, alone(VaMode::Mix, 1.0), 60);
    let y = held(C4, alone(VaMode::Max, 1.0), 60);
    let (mut tested, mut worst) = (0usize, 0.0f32);
    for i in 2 * B..n - 3 {
        let side = |j: usize| a[j] > b[j];
        // the crossing's reach: the samples i-2..=i+1 around the one
        // played at i (late by one)
        if (i - 3..=i + 1).any(|j| side(j) != side(i - 1)) {
            continue;
        }
        let want = (1.0 - MAX_TRIM) * a[i - 1].max(b[i - 1]);
        worst = worst.max((y[i] - want).abs());
        tested += 1;
    }
    println!("MAX BLEND 1: {tested} samples checked, worst |y − max(a, b)| {worst:.2e}");
    assert!(tested > n / 2, "{tested}");
    assert!(worst <= 1e-5, "{worst}");
}

/// The alias gate, on the kinks of `|a − b|` where both waves are
/// continuous (sine, tri): −45 dB, or the kink correction alone
/// (oscillators corrected both ways) 10 dB better than without it. In every
/// case the correction is no worse than 0.5 dB over kink off. Where either
/// wave steps (saw, pulse) a wrap steps `a` through `b`, so `|a − b|` jumps
/// there: a step, not the slope kink the BLAMP rounds. `KINK_STEP` skips
/// the correction at such crossings (a ratio of 4 left cases 3.6 dB worse
/// than kink off, 2 left 1.7, 1.3 left 0.6, 1.15 at most 0.3), and the
/// kink corrector gains at most 0.9 dB there; the step cases are printed,
/// bounded at −34 dB (the measured worst, −34.5) and tracked in private issue #672.
#[test]
fn max_alias_gate() {
    let mut worst = (f64::MIN, String::new());
    let mut least = (f64::MAX, String::new());
    let mut fails = Vec::new();
    println!("note crse wave1 wave2: on / kink off / all off dB (MIX)");
    for note in [C5, C6] {
        for crse in [0i8, 7] {
            let (fine, ratio) = match crse {
                7 => (1.955, 1.5),
                _ => (6.0, 2f32.powf(6.0 / 1200.0)),
            };
            for w1 in ANCHORS {
                for w2 in ANCHORS {
                    let base = VaParams {
                        wave: [w1, w2],
                        coarse: [0, crse],
                        fine: [0.0, fine],
                        ..max(1.0)
                    };
                    let fc = hz_of(note);
                    let f2 = fc * ratio;
                    let gate = |prep: fn(&mut VaEngine)| {
                        let y = take_with(prep, note, base, GATE_BLOCKS, |_, _| {});
                        alias_db_lattice(&y, SR as f32, fc, f2)
                    };
                    let on = gate(|_| {});
                    let kink_off = gate(VaEngine::kink_uncorrected);
                    let all_off = gate(VaEngine::uncorrected);
                    let mix = {
                        let p = VaParams {
                            mode: VaMode::Mix,
                            amount: 0.5,
                            ..base
                        };
                        alias_db_lattice(&held(note, p, GATE_BLOCKS), SR as f32, fc, f2)
                    };
                    let at = format!("{note} {crse:+} {w1:.3} {w2:.3}");
                    println!("{at}: {on:.1} / {kink_off:.1} / {all_off:.1} ({mix:.1})");
                    // the correction never makes it worse
                    if on > kink_off + 0.5 {
                        fails.push(format!("{at}: {on:.1} worse than kink off {kink_off:.1}"));
                    }
                    if w1 >= SAW || w2 >= SAW {
                        if on > -34.0 {
                            fails.push(format!("{at}: {on:.1} dB > −34 (step case)"));
                        }
                    } else if on > -45.0 && on > kink_off - 10.0 {
                        fails.push(format!("{at}: {on:.1} vs kink off {kink_off:.1}"));
                    }
                    if on > worst.0 {
                        worst = (on, at.clone());
                    }
                    if kink_off - on < least.0 {
                        least = (kink_off - on, at);
                    }
                }
            }
        }
    }
    println!("worst {:.1} dB at {}", worst.0, worst.1);
    println!("least kink correction {:.1} dB at {}", least.0, least.1);
    assert!(fails.is_empty(), "{fails:#?}");
}

/// Two lines crossing between samples: the corrector's output is the
/// analytic corner away from the crossing, and rounds it, one sample
/// late (a = the line, b = 0, BLEND 1: `y = 2h·max(a, 0)`).
#[test]
fn max_kink_rounds_a_known_corner() {
    use chimera_core::dsp::va::combine::{MaxKink, max_gain};
    let (slope, cross) = (0.1f32, 10.37f32);
    let mut k = MaxKink::NEW;
    let h = max_gain(1.0);
    let y: Vec<f32> = (0..24)
        .map(|n| k.step(slope * (n as f32 - cross), 0.0, 1.0))
        .collect();
    let mut off = MaxKink::NEW;
    off.off = true;
    let y0: Vec<f32> = (0..24)
        .map(|n| off.step(slope * (n as f32 - cross), 0.0, 1.0))
        .collect();
    // sample n comes out at n + 1; the corner sits between samples 10 and 11
    for n in 0..24 {
        let naive = 2.0 * h * (slope * (n as f32 - cross)).max(0.0);
        if n + 1 < 24 && !(9..=12).contains(&n) {
            assert!((y[n + 1] - naive).abs() < 1e-6, "{n}: {} {naive}", y[n + 1]);
        }
    }
    // the corner is cut: both neighbours rise above the naive V
    assert!(y[11] > y0[11] && y[12] > y0[12], "{:?} {:?}", y, y0);
    // by the BLAMP's t³/6 and (1−t)³/6 of the slope jump 2·h·slope
    let (t, jump) = (0.63f32, 2.0 * h * slope);
    assert!((y[11] - y0[11] - jump * t.powi(3) / 6.0).abs() < 1e-5);
    assert!((y[12] - y0[12] - jump * (1.0 - t).powi(3) / 6.0).abs() < 1e-5);
}

/// Two exactly in-tune equal waves: MAX is `a`, a mix is `a` too, but
/// the trim (made for unrelated waves) leaves MAX 20·log(1 − MAX_TRIM)
/// under it: about −3.2 dB, a stated limit.
#[test]
fn max_in_tune_equal_waves_sit_under_the_mix() {
    let saws = VaParams {
        wave: [SAW; 2],
        fine: [0.0; 2],
        ..sine()
    };
    let blocks = 2 * SR as usize / B;
    let mix = rms(&held(
        C4,
        VaParams {
            mode: VaMode::Mix,
            amount: 0.5,
            ..saws
        },
        blocks,
    ));
    let y = held(
        C4,
        VaParams {
            mode: VaMode::Max,
            amount: 1.0,
            ..saws
        },
        blocks,
    );
    let db = 20.0 * (rms(&y) / mix).log10();
    println!("MAX, in-tune equal saws, BLEND 1: {db:+.2} dB vs MIX");
    assert!(db.abs() <= 3.5, "{db}");
}

/// MAX sounds as loud as the plain mix (MIX at AMOUNT ½): BLEND 0 to 1
/// within ±1.5 dB, at INIT's tuning and a fifth apart.
#[test]
fn max_level_matches_mix() {
    let blocks = 2 * SR as usize / B;
    for (coarse, fine) in [(0i8, 6.0), (7, 0.0)] {
        for w in ANCHORS {
            let waves = VaParams {
                wave: [w; 2],
                coarse: [0, coarse],
                fine: [0.0, fine],
                ..sine()
            };
            let mix = rms(&held(
                C4,
                VaParams {
                    mode: VaMode::Mix,
                    amount: 0.5,
                    ..waves
                },
                blocks,
            ));
            for blend in [0.0, 0.5, 1.0] {
                let y = held(
                    C4,
                    VaParams {
                        mode: VaMode::Max,
                        amount: blend,
                        ..waves
                    },
                    blocks,
                );
                let db = 20.0 * (rms(&y) / mix).log10();
                println!("CRSE2 {coarse} WAVE {w:.3} BLEND {blend}: {db:+.2} dB");
                assert!(
                    db.abs() <= 1.5,
                    "CRSE2 {coarse} WAVE {w} BLEND {blend}: {db}"
                );
            }
        }
    }
}

/// The larger of two zero-mean waves leans positive; the voice's DC
/// blocker (ADR 0060) takes it out: a second's mean at the voice is
/// under 0.3 % of its RMS.
#[test]
fn max_has_no_dc_at_the_voice() {
    use chimera_core::params::EngineType;
    use chimera_core::preset::Sound;
    use chimera_core::{MidiNote, Velocity};
    for (w1, w2) in [(SAW, SAW), (0.0, SAW), (1.0, 0.0)] {
        let mut s = Sound::init(EngineType::Va);
        s.params.va = VaParams {
            wave: [w1, w2],
            ..max(1.0)
        };
        let (p, m) = (&s.params, &s.mod_state);
        let mut v = common::Rig::new(common::SR);
        v.note_on(MidiNote::new(C4).unwrap(), Velocity::DEFAULT, p);
        let mut b = [0.0; B];
        let mut y = Vec::new();
        for n in 0..(3 * common::SR as usize / 2) / B {
            v.render(&mut b, p, m);
            if n * B >= common::SR as usize / 2 {
                y.extend_from_slice(&b);
            }
        }
        let mean = y.iter().map(|&x| f64::from(x)).sum::<f64>() / y.len() as f64;
        let r = rms(&y);
        println!(
            "MAX {w1}/{w2}: mean {mean:.2e}, RMS {r:.3e}, ratio {:.4}",
            mean / r
        );
        assert!(r > 1e-3);
        assert!(mean.abs() <= 0.003 * r, "{mean} of {r}");
    }
}

/// In and out of MAX through the duck: no click past the rule, the kink
/// state restarts under the duck's silence.
#[test]
fn max_mode_changes_do_not_click() {
    for (m0, m1) in [
        (VaMode::Mix, VaMode::Max),
        (VaMode::Max, VaMode::Mix),
        (VaMode::Fm, VaMode::Max),
        (VaMode::Max, VaMode::Sync),
    ] {
        let base = VaParams {
            wave: [SAW, 0.3],
            fine: [0.0, 6.0],
            coarse: [0, 7],
            sub: 0.5,
            amount: 0.8,
            ..sine()
        };
        let y = take_with(
            |_| {},
            A4,
            base,
            2 * AT,
            |b, p| p.mode = if b < AT { m0 } else { m1 },
        );
        let held: Vec<_> = [m0, m1]
            .iter()
            .map(|&m| held(A4, VaParams { mode: m, ..base }, 6 * AT))
            .collect();
        let (d, lim) = click(&y, &held);
        println!("{m0:?} → {m1:?}: Δ² {d:.3e} / limit {lim:.3e}");
        assert!(d <= lim, "{m0:?} → {m1:?}: {d} > {lim}");
    }
}

// ---- SOFT SYNC (#604) ----

const A3: u8 = 57;

/// OSC 2 a saw a fifth up, reversing at OSC 1's wraps, BLEND `amount`.
fn soft(amount: f32) -> VaParams {
    VaParams {
        mode: VaMode::SoftSync,
        wave: [0.0, SAW],
        coarse: [0, 7],
        amount,
        ..sine()
    }
}

/// Correlation of `y` with itself `lag` samples on (linearly
/// interpolated), over its last 8192 samples.
fn r_at(y: &[f32], lag: f64) -> f64 {
    let (k, fr) = (lag.floor() as usize, lag.fract());
    let tail = &y[y.len() - 8192 - k - 1..];
    let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
    for n in 0..8192 {
        let a = f64::from(tail[n]);
        let b = f64::from(tail[n + k]) * (1.0 - fr) + f64::from(tail[n + k + 1]) * fr;
        (xy, xx, yy) = (xy + a * b, xx + a * a, yy + b * b);
    }
    xy / (xx * yy).sqrt()
}

/// Each turn mirrors the read: forward over one of OSC 1's periods, back
/// over the next, so the wave repeats every two and each second period is
/// the first played backwards. At A3 every 11th wrap of 2400/11 samples
/// lands on a sample: around the first two the mirror is checked sample
/// for sample (the best whole or half-sample centre within ±2 of the
/// wrap). Later ones drift off the sample with the f32 clock's rounding
/// (RMS 1.2e-3 at the third, 2e-3 at the fifth), not the mirror.
#[test]
fn soft_sync_mirrors() {
    let t1 = SR as f64 / f64::from(hz_of(A3));
    let y = held(A3, soft(1.0), 200);
    let (r2, r1) = (r_at(&y, 2.0 * t1), r_at(&y, t1));
    println!("r(2 periods) {r2:.5}, r(1 period) {r1:.5}");
    assert!(r2 >= 0.99, "{r2}");
    let span = t1.floor() as usize;
    let mut worst = 0.0f64;
    for m in 1..=2 {
        let at = 2 * 2400 * m;
        let (e, c2) = (at - 4..=at + 4)
            .map(|c2| {
                let mut s = 0.0;
                for k in 1..span {
                    let (a, b) = (c2 / 2 + k, c2.div_ceil(2) - k);
                    let d = f64::from(y[a]) - f64::from(y[b]);
                    s += d * d;
                }
                ((s / (span - 1) as f64).sqrt(), c2)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        println!(
            "turn near {}: centre {:.1}, RMS {e:.2e}",
            2400 * m,
            c2 as f64 / 2.0
        );
        worst = worst.max(e);
    }
    assert!(worst <= 1e-3, "{worst}");
}

/// Power above `lo` Hz of the gate window (Hann), dB.
fn band_db(y: &[f32], lo: f64) -> f64 {
    use common::spectrum::fft;
    let n = 1 << 15;
    let x = &y[4096..4096 + n];
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            f64::from(v) * w
        })
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let bin = f64::from(SR) / n as f64;
    let p: f64 = (1..n / 2)
        .filter(|&k| k as f64 * bin >= lo)
        .map(|k| re[k] * re[k] + im[k] * im[k])
        .sum();
    10.0 * p.log10()
}

/// A turn is a corner, a reset a step: a sine or triangle slave in SYNC
/// carries 18–28 dB more above 8 kHz, gated at 6. Held only to never
/// 3 dB brighter than SYNC: a saw (its own edges are steps either way:
/// within 2 dB), and CRSE2 +7, where OSC 2 is half a cycle on at each
/// wrap, so a sine's or triangle's reset from there is the same corner as
/// a turn (within 0.1 dB).
#[test]
fn soft_sync_is_softer_than_sync() {
    for (w2, crse) in [SAW, 1.0 / 3.0, 0.0]
        .into_iter()
        .flat_map(|w| [3i8, 5, 7, 10].map(|c| (w, c)))
    {
        let base = VaParams {
            wave: [0.0, w2],
            coarse: [0, crse],
            ..soft(1.0)
        };
        let soft_ = band_db(&held(A3, base, GATE_BLOCKS), 8_000.0);
        let hard = band_db(
            &held(
                A3,
                VaParams {
                    mode: VaMode::Sync,
                    ..base
                },
                GATE_BLOCKS,
            ),
            8_000.0,
        );
        println!(
            "WAVE2 {w2:.3} CRSE2 {crse}: above 8 kHz SOFT SYNC {soft_:.1} dB, SYNC {hard:.1} dB ({:+.1})",
            soft_ - hard
        );
        let room = if w2 < SAW && crse != 7 { -6.0 } else { 3.0 };
        assert!(
            soft_ <= hard + room,
            "WAVE2 {w2} CRSE2 {crse}: {soft_} vs {hard}"
        );
    }
}

/// The alias gate on the half-pitch lattice (the wave repeats every two
/// of OSC 1's periods), corrected and not; where −45 dB is missed the
/// correction must win 12 dB (the exceptions printed, ADR 0161).
#[test]
fn soft_sync_alias_gate() {
    let mut worst = (f64::MIN, String::new());
    let mut exceptions = Vec::new();
    let mut fails = Vec::new();
    for note in [C5, C6] {
        let f0 = 0.5 * hz_of(note);
        for x in [0.0, 1.0 / 6.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0] {
            for s in [0.0, 0.5] {
                let base = VaParams {
                    wave: [x; 2],
                    shape: [s; 2],
                    ..soft(1.0)
                };
                let gate = |prep: fn(&mut VaEngine)| {
                    let y = take_with(prep, note, base, GATE_BLOCKS, |_, _| {});
                    common::spectrum::alias_db(&y, SR as f32, &[f0])
                };
                let (on, off) = (gate(|_| {}), gate(VaEngine::uncorrected));
                let at = format!("{note} WAVE {x:.3} SHAPE {s}");
                println!("{at}: {on:.1} dB (off {off:.1})");
                if on > -45.0 {
                    exceptions.push(format!("{at}: {on:.1} / {off:.1}"));
                    if on > off - 12.0 {
                        fails.push(format!("{at}: {on:.1} vs off {off:.1}"));
                    }
                }
                if on > worst.0 {
                    worst = (on, at);
                }
            }
        }
    }
    println!(
        "worst {:.1} dB at {}; exceptions {exceptions:#?}",
        worst.0, worst.1
    );
    assert!(fails.is_empty(), "{fails:#?}");
}

/// The turns move OSC 2's phase, not its level: SOFT SYNC at BLEND 0, ½
/// and 1 sounds as loud as MIX there.
#[test]
fn soft_sync_level_matches_mix() {
    let blocks = 2 * SR as usize / B;
    let saws = VaParams {
        wave: [SAW; 2],
        ..soft(0.0)
    };
    for blend in [0.0, 0.5, 1.0] {
        let at = |mode| {
            rms(&held(
                C4,
                VaParams {
                    mode,
                    amount: blend,
                    ..saws
                },
                blocks,
            ))
        };
        let db = 20.0 * (at(VaMode::SoftSync) / at(VaMode::Mix)).log10();
        println!("BLEND {blend}: {db:+.2} dB");
        assert!(db.abs() <= 1.5, "BLEND {blend}: {db}");
    }
}

/// Entering SOFT SYNC the read starts forward (`settle` at the swap):
/// left while OSC 2 reads backward (an odd number of turns), out to MIX
/// and back in, it plays as a fresh engine started at those phases does.
#[test]
fn entering_soft_sync_starts_forward() {
    let base = VaParams {
        wave: [SAW, 0.3],
        shape: [0.3, 0.0],
        ..soft(0.8)
    };
    let tail = 10;
    // SOFT SYNC for `k` blocks, then MIX through the duck's swap
    let leave = |e: &mut VaEngine, p: &mut VaParams, k: usize| {
        e.hold();
        e.note_on(A4, p, PartId::new(0).unwrap(), SR);
        run(e, p, k, |_, _| {});
        p.mode = VaMode::Mix;
        run(e, p, 4, |_, _| {});
        assert_eq!(e.mode(), VaMode::Mix);
        e.dirs_at_swap()[1]
    };
    // a turn every 109 samples at A4: some start leaves it backward
    let k = (1..20)
        .find(|&k| with_engine(|e| leave(e, &mut { base }, k)) < 0.0)
        .expect("OSC 2 left backward");
    let (back, settled) = with_engine(|e| {
        let mut p = base;
        assert_eq!(leave(e, &mut p, k), -1.0);
        p.mode = VaMode::SoftSync;
        run(e, &mut p, 2, |_, _| {});
        assert_eq!(e.mode(), VaMode::SoftSync);
        let at = e.osc_phase();
        let y = run(e, &mut p, AT + tail, |_, _| {});
        (y, at)
    });
    let fresh = with_engine(|f| {
        let mut p = base;
        f.hold();
        f.note_on(A4, &p, PartId::new(0).unwrap(), SR);
        f.set_osc_phase(settled);
        run(f, &mut p, AT + tail, |_, _| {})
    });
    let err = back[AT * B..]
        .iter()
        .zip(&fresh[AT * B..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!("MIX → SOFT SYNC, {AT} blocks on: max |back − fresh| {err:.2e}");
    assert!(err <= 1e-4, "{err}");
}

// ---- PHASE (#602) ----

fn phase(amount: f32) -> VaParams {
    VaParams {
        mode: VaMode::Phase,
        amount,
        ..sine()
    }
}

/// A phase offset repeats with the cycle, so the pitch is OSC 1's at any
/// BLEND and any ratio.
#[test]
fn phase_keeps_pitch() {
    let f0 = hz_of(A4);
    for crse in [0i8, 7] {
        for blend in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let base = VaParams {
                coarse: [0, crse],
                ..phase(blend)
            };
            let y = held(A4, base, PITCH_BLOCKS + 200);
            let f = peak_hz(&y, SR as f32, f0);
            let cents = 1200.0 * (f / f64::from(f0)).log2();
            println!("PHASE CRSE2 {crse} BLEND {blend}: {f:.3} Hz ({cents:+.3} cents)");
            assert!(cents.abs() <= 1.0, "CRSE2 {crse} BLEND {blend}: {cents}");
        }
    }
}

/// Total energy of the 2nd–8th harmonics of `f0`, Hann-windowed.
fn upper_energy(y: &[f32], f0: f32) -> f64 {
    let n = y.len();
    let w: Vec<f32> = y
        .iter()
        .enumerate()
        .map(|(i, &v)| v * (0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos()))
        .collect();
    (2..=8)
        .map(|k| {
            let (re, im) = fundamental(&w, SR as f32, f0 * k as f32);
            f64::from(re).powi(2) + f64::from(im).powi(2)
        })
        .sum()
}

/// A sine warped by a triangle OSC 2 (SHAPE 1, the CZ's skewed triangle),
/// locked 1:1: the upper harmonics grow with BLEND, the filter sweep a CZ
/// makes with its DCW.
#[test]
fn phase_warps_like_cz() {
    let f0 = hz_of(A4);
    let mut last = -1.0;
    for i in 0..=5 {
        let blend = 0.1 * i as f32;
        let base = VaParams {
            wave: [0.0, 1.0 / 3.0],
            shape: [0.0, 1.0],
            ..phase(blend)
        };
        let y = held(A4, base, 600);
        let e = upper_energy(&y[y.len() - 8192..], f0);
        println!("PHASE BLEND {blend:.1}: harmonics 2-8 energy {e:.3e}");
        assert!(e > last, "BLEND {blend}: {e} <= {last}");
        last = e;
    }
}

/// The alias gate on the lattice of both pitches, corrected and not. A
/// sine modulator under a sine carrier is bandlimited by Carson's rule at
/// index `2π·depth`: its gate is −45 dB where that fits under Nyquist;
/// elsewhere (saw or pulse carrier) the correction must win 10 dB. A saw
/// or pulse modulator's step is a jump of the read, one step of the output
/// (ADR 0160): −45 dB everywhere. A triangle modulator: −45 dB, or the
/// correction wins 10 dB.
#[test]
fn phase_alias_gate() {
    let mut worst = (f64::MIN, String::new());
    let mut least = (f64::MAX, String::new());
    let mut stepped = (f64::MIN, String::new());
    let (mut fails, mut held_out) = (Vec::new(), Vec::new());
    println!("note crse wave1 wave2 blend: on / off dB (carson Hz)");
    for note in [C5, C6] {
        for crse in [0i8, 12, 7] {
            let (fine, ratio) = match crse {
                7 => (1.955, 1.5),
                c => (0.0, 2f32.powf(f32::from(c) / 12.0)),
            };
            for w1 in ANCHORS {
                for w2 in ANCHORS {
                    for blend in [0.25, 0.5, 1.0] {
                        let base = VaParams {
                            wave: [w1, w2],
                            coarse: [0, crse],
                            fine: [0.0, fine],
                            ..phase(blend)
                        };
                        let fc = hz_of(note);
                        let fm_ = fc * ratio;
                        let i = core::f32::consts::TAU * phase_depth(blend);
                        let carson = (i + 1.0) * fm_ + fc;
                        let gate = |prep: fn(&mut VaEngine)| {
                            let y = take_with(prep, note, base, GATE_BLOCKS, |_, _| {});
                            alias_db_lattice(&y, SR as f32, fc, fm_)
                        };
                        let on = gate(|_| {});
                        let off = gate(VaEngine::uncorrected);
                        let fits = carson < 24_000.0 && w1 < SAW && w2 == 0.0;
                        let at = format!("{note} {crse:+} {w1:.3} {w2:.3} {blend}");
                        let tag = if fits { "" } else { " exception" };
                        println!("{at}: {on:.1} / {off:.1} ({carson:.0}){tag}");
                        if w2 > 0.0 {
                            let won = w2 < SAW && on <= off - 10.0;
                            if on > -45.0 && !won {
                                held_out.push(format!("{at}: {on:.1} vs off {off:.1}"));
                            }
                            if w2 >= SAW && on > stepped.0 {
                                stepped = (on, at);
                            }
                            continue;
                        }
                        if fits {
                            if on > -45.0 {
                                fails.push(format!("{at}: {on:.1} dB > -45"));
                            }
                            if on > worst.0 {
                                worst = (on, at);
                            }
                        } else {
                            if on > off - 10.0 {
                                fails.push(format!("{at}: {on:.1} vs off {off:.1}"));
                            }
                            if off - on < least.0 {
                                least = (off - on, at);
                            }
                        }
                    }
                }
            }
        }
    }
    println!("worst where Carson fits {:.1} dB at {}", worst.0, worst.1);
    println!(
        "least correction elsewhere {:.1} dB at {}",
        least.0, least.1
    );
    println!(
        "worst under a saw or pulse modulator {:.1} dB at {}",
        stepped.0, stepped.1
    );
    println!("past the modulator's gate: {held_out:#?}");
    assert!(fails.is_empty(), "{fails:#?}");
    assert!(held_out.is_empty(), "{held_out:#?}");
}

/// PHASE moves OSC 1's phase, not its level: at INIT's tuning and a fifth
/// apart every BLEND sounds as loud as OSC 1 alone, within 1.5 dB.
#[test]
fn phase_level_matches_mix() {
    let blocks = 2 * SR as usize / B;
    for (coarse, fine) in [(0i8, 6.0), (7, 0.0)] {
        let saws = VaParams {
            wave: [SAW; 2],
            coarse: [0, coarse],
            fine: [0.0, fine],
            ..sine()
        };
        let mix = rms(&held(
            C4,
            VaParams {
                mode: VaMode::Mix,
                amount: 0.0,
                ..saws
            },
            blocks,
        ));
        for blend in [0.0, 0.5, 1.0] {
            let y = held(
                C4,
                VaParams {
                    mode: VaMode::Phase,
                    amount: blend,
                    ..saws
                },
                blocks,
            );
            let db = 20.0 * (rms(&y) / mix).log10();
            println!("CRSE2 {coarse} FINE2 {fine} BLEND {blend}: {db:+.2} dB");
            assert!(
                db.abs() <= 1.5,
                "CRSE2 {coarse} FINE2 {fine} BLEND {blend}: {db}"
            );
        }
    }
}

/// PHASE in and out of the duck never clicks: from and to every other
/// MODE, the note started in the source MODE, the sub on or off, OSC 2 a
/// sine/triangle-ish wave or a pulse (the stepped modulator).
#[test]
fn phase_changes_never_click() {
    let base = VaParams {
        wave: [SAW, 0.3],
        shape: [0.3, 0.0],
        fine: [0.0, 6.0],
        coarse: [0, 7],
        amount: 0.8,
        ..sine()
    };
    let pulse = VaParams {
        wave: [SAW, 1.0],
        ..base
    };
    for base in [base, pulse] {
        for sub in [0.0, 0.5] {
            for other in VaMode::ALL.into_iter().filter(|&m| m != VaMode::Phase) {
                for (m0, m1) in [(other, VaMode::Phase), (VaMode::Phase, other)] {
                    let base = VaParams {
                        sub,
                        mode: m0,
                        ..base
                    };
                    let y = take_with(
                        |_| {},
                        A4,
                        base,
                        2 * AT,
                        |b, p| p.mode = if b < AT { m0 } else { m1 },
                    );
                    let held: Vec<_> = [m0, m1]
                        .iter()
                        .map(|&m| held(A4, VaParams { mode: m, ..base }, 6 * AT))
                        .collect();
                    // The swap's window, the duck's four blocks: the new MODE
                    // settles at another relative phase of the OSCs than a
                    // fresh note's, so the limit is the held peak over a long
                    // run (MIX -> PHASE, pulse, SUB .5 read 101 % of a 40
                    // block run's, 73 % of this one)
                    let (d, lim) = click(&y[(AT - 2) * B..(AT + 4) * B], &held);
                    println!(
                        "SUB {sub} WAVE2 {} {m0:?} -> {m1:?}: d2 {d:.3e} / limit {lim:.3e} ({:.0}%)",
                        base.wave[1],
                        100.0 * d / lim
                    );
                    assert!(d <= lim, "{m0:?} -> {m1:?}: {d} > {lim}");
                }
            }
        }
    }
}
