//! The VA MODEs past the first five (#601–#605, #613), one section each.
//! Levels are the morph's (÷ `VA_CHAIN_GAIN`) unless said.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::va::{VA_CHAIN_GAIN, VaEngine, VaMode, VaParams, fm_index};
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;
use common::spectrum::{alias_db_lattice, peak_hz};

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
/// (measured: −23 to −48 dB at BLEND 1, ADR 0159). Most of that is the
/// 2-point kernel at up to 6× the note's read rate (BLEND 1, `I` = 5); the
/// rate's corners alone stay at about −58 dB or under (a sine carrier
/// under a saw modulator), and a saw modulator adds about 10 dB more
/// (private issue #650). CRSE 7 plays the just
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
            .map(|&m| held(A4, VaParams { mode: m, ..base }, 2 * AT))
            .collect();
        let (d, lim) = click(&y, &held);
        println!(
            "SUB 0.5 {m0:?} → {m1:?}: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
            100.0 * d / lim
        );
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
                    let (a, b) = (c2 / 2 + k, (c2 + 1) / 2 - k);
                    let d = f64::from(y[a]) - f64::from(y[b]);
                    s += d * d;
                }
                ((s / (span - 1) as f64).sqrt(), c2)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        println!("turn near {}: centre {:.1}, RMS {e:.2e}", 2400 * m, c2 as f64 / 2.0);
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
/// carries 18–28 dB more above 8 kHz, gated at 6. Printed, not gated: a
/// saw (its own edges are steps either way: within 2 dB), and CRSE2 +7,
/// where OSC 2 is half a cycle on at each wrap, so a sine's or triangle's
/// reset from there is the same corner as a turn (within 0.1 dB).
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
        if w2 < SAW && crse != 7 {
            assert!(soft_ <= hard - 6.0, "WAVE2 {w2} CRSE2 {crse}: {soft_} vs {hard}");
        }
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
    println!("worst {:.1} dB at {}; exceptions {exceptions:#?}", worst.0, worst.1);
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
/// after an odd number of turns, out to MIX and back in, it plays as a
/// fresh engine started at those phases does.
#[test]
fn entering_soft_sync_starts_forward() {
    let base = VaParams {
        wave: [SAW, 0.3],
        shape: [0.3, 0.0],
        ..soft(0.8)
    };
    let tail = 10;
    let (back, settled) = with_engine(|e| {
        let mut p = base;
        e.hold();
        e.note_on(A4, &p, PartId::new(0).unwrap(), SR);
        // 26.7 ms a block of 64 at A4: a few turns, an odd count somewhere
        run(e, &mut p, 3, |_, _| {});
        p.mode = VaMode::Mix;
        run(e, &mut p, 4, |_, _| {});
        assert_eq!(e.mode(), VaMode::Mix);
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
