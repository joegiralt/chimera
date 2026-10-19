//! `VaEngine`: two oscillators, the operator, the sub, the MODE duck and
//! STABILITY (spec § Oscillator, § Combine, § Stability, § Testing).
//! Levels are the morph's (÷ `VA_CHAIN_GAIN`) unless said.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::ease::{at, step_of};
use chimera_core::dsp::va::combine::with_sub;
use chimera_core::dsp::va::osc::{Morph, NO_WRAP, Ramps, Unit, VarOsc, ZEROS, wave_at};
use chimera_core::dsp::va::{VA_CHAIN_GAIN, VaEngine, VaMode, VaParams};
use chimera_core::hw::Cost;
use chimera_core::project::PartId;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_hal::BLOCK_SIZE;
use common::spectrum::{alias_db, alias_db_lattice, peak_hz, tri_reference};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;
const A3: u8 = 57;
const A4: u8 = 69;
const C2: u8 = 36;
const C5: u8 = 72;
const C6: u8 = 84;
const C7: u8 = 96;
const GATE_BLOCKS: usize = ((1 << 12) + (1 << 15)) / B;
const GRID: [f32; 7] = [0.0, 1.0 / 6.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0];
const ANCHORS: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
/// A sweep: held, 100 ms of blocks, held.
const PRE: usize = 10;
const SWEEP: usize = 75;
const SWEEP_LEN: usize = PRE + SWEEP + 20;

fn hz_of(note: u8, cents: f32) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0 + cents / 100.0) / 12.0)
}

fn part(n: u8) -> PartId {
    PartId::new(n).unwrap()
}

/// A fresh engine on (part 0, voice 0).
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

/// A fresh engine's note: `set(0, ..)` before its note-on, then each block.
fn take_on(
    who: (u8, usize),
    prep: impl FnOnce(&mut VaEngine),
    note: u8,
    base: VaParams,
    blocks: usize,
    mut set: impl FnMut(usize, &mut VaParams),
) -> Vec<f32> {
    with_engine(|e| {
        e.place(VoiceIdx::ALL[who.1]);
        prep(e);
        let mut p = base;
        set(0, &mut p);
        e.hold();
        e.note_on(note, &p, part(who.0), SR);
        run(e, &mut p, blocks, set)
    })
}

fn take(
    note: u8,
    base: VaParams,
    blocks: usize,
    set: impl FnMut(usize, &mut VaParams),
) -> Vec<f32> {
    take_on((0, 0), |_| {}, note, base, blocks, set)
}

fn held(note: u8, base: VaParams, blocks: usize) -> Vec<f32> {
    take(note, base, blocks, |_, _| {})
}

/// The click rule's render: sines, STAB 1, no sub, OSC 2 in tune.
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

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// The largest second difference from block 2 on.
fn d2(y: &[f32]) -> f64 {
    (2 * B..y.len())
        .map(|n| (f64::from(y[n]) - 2.0 * f64::from(y[n - 1]) + f64::from(y[n - 2])).abs())
        .fold(0.0, f64::max)
}

/// The click rule: `(swept, limit)`.
fn click(swept: &[f32], held: &[Vec<f32>]) -> (f64, f64) {
    let h = held.iter().map(|y| d2(y)).fold(0.0, f64::max);
    (d2(swept), 1.5 * h + 1e-6)
}

/// The knob's value at block `b` of a sweep from `v0` to `v1`.
fn ramp_at(b: usize, v0: f32, v1: f32) -> f32 {
    let t = ((b + 1).saturating_sub(PRE) as f32 / SWEEP as f32).min(1.0);
    v0 + (v1 - v0) * t
}

/// A knob swept `v0` → `v1` and held at 0, ¼, ½, ¾ and 1 of the way.
fn sweep(
    note: u8,
    base: VaParams,
    knob: fn(&mut VaParams, f32),
    (v0, v1): (f32, f32),
) -> (Vec<f32>, Vec<Vec<f32>>) {
    let swept = take(note, base, SWEEP_LEN, |b, p| knob(p, ramp_at(b, v0, v1)));
    let held = [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|q| take(note, base, SWEEP_LEN, |_, p| knob(p, v0 + (v1 - v0) * q)))
        .collect();
    (swept, held)
}

#[test]
fn every_anchor_in_tune_at_a4() {
    let blocks = (1 << 17) / B;
    for w in ANCHORS {
        let base = VaParams {
            wave: [w; 2],
            amount: 0.0,
            ..sine()
        };
        let f = peak_hz(&held(A4, base, blocks), SR as f32, 440.0);
        let c = 1200.0 * (f / 440.0).log2();
        println!("WAVE {w:.3}: {f:.4} Hz ({c:+.4} cents)");
        assert!(c.abs() <= 1.0, "WAVE {w}: {c} cents");
    }
}

/// Corrected and uncorrected alias of `base` at `note`, masking `f0s`.
fn gate(note: u8, base: VaParams, f0s: &[f32]) -> (f64, f64) {
    let on = alias_db(&held(note, base, GATE_BLOCKS), SR as f32, f0s);
    let y = take_on(
        (0, 0),
        |e| e.uncorrected(),
        note,
        base,
        GATE_BLOCKS,
        |_, _| {},
    );
    (on, alias_db(&y, SR as f32, f0s))
}

#[test]
fn alias_gate_in_mix_and_sync() {
    let f0 = hz_of(C6, 0.0);
    let mut worst = (f64::MIN, String::new());
    for mode in [VaMode::Mix, VaMode::Sync] {
        for x in GRID {
            for s in [0.0, 0.5, 1.0] {
                let base = VaParams {
                    wave: [x; 2],
                    shape: [s; 2],
                    mode,
                    coarse: [0, if mode == VaMode::Sync { 7 } else { 0 }],
                    ..sine()
                };
                let (on, off) = gate(C6, base, &[f0]);
                let at = format!("{mode:?} WAVE {x:.3} SHAPE {s}");
                println!("{at}: {on:.1} dB (Off {off:.1})");
                assert!(on <= -45.0, "{at}: {on}");
                assert!(off <= -60.0 || on <= off - 12.0, "{at}: {on} vs {off}");
                if on > worst.0 {
                    worst = (on, at);
                }
            }
        }
    }
    for x in GRID {
        for s in [0.0, 0.5, 1.0] {
            let base = VaParams {
                wave: [x; 2],
                shape: [s; 2],
                sub: 1.0,
                ..sine()
            };
            let on = alias_db(&held(C6, base, GATE_BLOCKS), SR as f32, &[0.5 * f0]);
            let at = format!("Mix SUB 1 WAVE {x:.3} SHAPE {s}");
            println!("{at}: {on:.1} dB");
            assert!(on <= -45.0, "{at}: {on}");
            if on > worst.0 {
                worst = (on, at);
            }
        }
    }
    println!("worst {:.2} dB at {}", worst.0, worst.1);
}

#[test]
fn ring_gate_detuned_at_c5() {
    let f1 = hz_of(C5, 0.0);
    let mut worst = (f64::MIN, String::new());
    for crse in [0i8, 12] {
        let f2 = hz_of(C5, f32::from(crse) * 100.0 + 6.0);
        for w1 in GRID {
            for w2 in [w1, 1.0 / 3.0] {
                for s in [0.0, 0.5] {
                    for x in [0.5, 1.0] {
                        let base = VaParams {
                            wave: [w1, w2],
                            shape: [s; 2],
                            mode: VaMode::Ring,
                            amount: x,
                            fine: [0.0, 6.0],
                            coarse: [0, crse],
                            ..sine()
                        };
                        let db = alias_db_lattice(&held(C5, base, GATE_BLOCKS), SR as f32, f1, f2);
                        let at = format!("CRSE2 {crse} WAVE {w1:.3}/{w2:.3} SHAPE {s} x {x}");
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

/// The modes the knobs sweep in, each with OSC 2 apart from OSC 1.
fn apart() -> [(VaMode, VaParams); 3] {
    [
        (
            VaMode::Mix,
            VaParams {
                fine: [0.0, 6.0],
                ..sine()
            },
        ),
        (
            VaMode::Ring,
            VaParams {
                mode: VaMode::Ring,
                fine: [0.0, 6.0],
                ..sine()
            },
        ),
        (
            VaMode::Sync,
            VaParams {
                mode: VaMode::Sync,
                coarse: [0, 7],
                ..sine()
            },
        ),
    ]
}

type Set = fn(&mut VaParams, f32);
type Knob = (&'static str, Set, (f32, f32));

const SWEPT: [Knob; 6] = [
    ("AMOUNT", |p, v| p.amount = v, (0.0, 1.0)),
    ("STAB", |p, v| p.stability = v, (0.0, 1.0)),
    ("FINE1", |p, v| p.fine[0] = v, (-100.0, 100.0)),
    ("FINE2", |p, v| p.fine[1] = v, (-100.0, 100.0)),
    ("SHAPE1", |p, v| p.shape[0] = v, (0.0, 1.0)),
    ("SHAPE2", |p, v| p.shape[1] = v, (0.0, 1.0)),
];

#[test]
fn knobs_sweep_without_clicks() {
    let mut worst = (0.0f64, String::new());
    for (name, knob, range) in SWEPT {
        for (mode, base) in apart() {
            let (swept, held) = sweep(A4, base, knob, range);
            let (d, lim) = click(&swept, &held);
            println!(
                "{name} in {mode:?}: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
                100.0 * d / lim
            );
            assert!(d <= lim, "{name} in {mode:?}: {d} > {lim}");
            if d / lim > worst.0 {
                worst = (d / lim, format!("{name} in {mode:?}"));
            }
        }
    }
    println!("worst {:.0}% of the limit at {}", 100.0 * worst.0, worst.1);
}

#[test]
fn sub_ramps_per_sample() {
    let c = held(A4, sine(), SWEEP_LEN);
    let one = held(A4, VaParams { sub: 1.0, ..sine() }, SWEEP_LEN);
    let s: Vec<f32> = c
        .iter()
        .zip(&one)
        .map(|(c, o)| 2f32.sqrt() * o - c)
        .collect();
    let y = take(A4, sine(), SWEEP_LEN, |b, p| p.sub = ramp_at(b, 0.0, 1.0));
    let mut err = 0.0f32;
    for (n, &v) in y.iter().enumerate() {
        let (b, i) = (n / B, n % B);
        let ft = (ramp_at(b.saturating_sub(1), 0.0, 1.0), ramp_at(b, 0.0, 1.0));
        let ft = if b == 0 { (ft.1, ft.1) } else { ft };
        let sub = at(ft.0, step_of(ft, B), i);
        err = err.max((v - with_sub(c[n], s[n], sub)).abs());
    }
    println!("max |y − with_sub| {err:.2e}");
    assert!(err <= 1e-6, "{err}");
}

#[test]
fn the_click_rule_catches_a_step() {
    let (mut swept, held) = sweep(A4, apart()[0].1, SWEPT[0].1, SWEPT[0].2);
    for v in &mut swept[2000..] {
        *v += 0.002;
    }
    let (d, lim) = click(&swept, &held);
    println!("stepped: Δ² {d:.3e} vs limit {lim:.3e}");
    assert!(d > lim, "{d} <= {lim}");
}

/// A pair's tuning: OSC 2 a fifth up with SYNC in it, else +6 cents.
fn pair_base(m0: VaMode, m1: VaMode) -> VaParams {
    if m0 == VaMode::Sync || m1 == VaMode::Sync {
        VaParams {
            coarse: [0, 7],
            ..sine()
        }
    } else {
        VaParams {
            fine: [0.0, 6.0],
            ..sine()
        }
    }
}

/// The MIX/SYNC/RING matrix, XOR → BITS (the same duck; one pair
/// closes the matrix at the cost of a constant), and FM in and out (its
/// read phase settles onto the clock at the swap).
const PAIRS: [(VaMode, VaMode); 18] = [
    (VaMode::Mix, VaMode::Sync),
    (VaMode::Mix, VaMode::Ring),
    (VaMode::Sync, VaMode::Mix),
    (VaMode::Sync, VaMode::Ring),
    (VaMode::Ring, VaMode::Mix),
    (VaMode::Ring, VaMode::Sync),
    (VaMode::Xor, VaMode::Bits),
    (VaMode::Mix, VaMode::Fm),
    (VaMode::Fm, VaMode::Mix),
    (VaMode::Sync, VaMode::Fm),
    (VaMode::Fm, VaMode::Ring),
    (VaMode::Mix, VaMode::Am),
    (VaMode::Am, VaMode::Ring),
    (VaMode::Mix, VaMode::Max),
    (VaMode::Max, VaMode::Sync),
    (VaMode::Mix, VaMode::Phase),
    (VaMode::Phase, VaMode::Fm),
    (VaMode::Fm, VaMode::Phase),
];
const AT: usize = 20;

#[test]
fn mode_changes_never_click() {
    let mut worst = (0.0f64, String::new());
    for note in [A4, A3] {
        for (m0, m1) in PAIRS {
            let base = pair_base(m0, m1);
            let y = take(note, base, 2 * AT, |b, p| {
                p.mode = if b < AT { m0 } else { m1 }
            });
            let held: Vec<_> = [m0, m1]
                .iter()
                .map(|&m| held(note, VaParams { mode: m, ..base }, 2 * AT))
                .collect();
            let (d, lim) = click(&y, &held);
            let at = format!("note {note} {m0:?} → {m1:?}");
            println!(
                "{at}: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
                100.0 * d / lim
            );
            assert!(d <= lim, "{at}: {d} > {lim}");
            if d / lim > worst.0 {
                worst = (d / lim, at);
            }
        }
    }
    println!("worst {:.0}% of the limit at {}", 100.0 * worst.0, worst.1);
}

/// WAVE's ramp crosses one segment edge a block at most, the rest carried
/// into the next (ADR 0118): a jump end to end lands in two blocks, passing
/// through the middle segment; driven end to end every block it swings
/// between the ends' reach. Neither clicks against the anchors held, and a
/// drive that stops lands within two blocks.
#[test]
fn wave_driven_end_to_end_lands_in_two_blocks_without_a_click() {
    let mid = |v: f32| (1.0 / 3.0..2.0 / 3.0).contains(&v);
    let mut worst = (0.0f64, String::new());
    for note in [A4, A3] {
        for k in 0..2 {
            let base = VaParams {
                amount: k as f32,
                ..sine()
            };
            let anchors: Vec<_> = ANCHORS
                .iter()
                .map(|&w| {
                    let mut p = base;
                    p.wave[k] = w;
                    held(note, p, 2 * AT)
                })
                .collect();
            for (w0, w1) in [(0.0, 1.0), (1.0, 0.0)] {
                // the jump: through the middle segment, then home
                with_engine(|e| {
                    let mut p = base;
                    p.wave[k] = w0;
                    e.hold();
                    e.note_on(note, &p, part(0), SR);
                    run(e, &mut p, AT, |_, _| {});
                    p.wave[k] = w1;
                    run(e, &mut p, 1, |_, _| {});
                    let first = e.wave_now()[k].unwrap();
                    assert!(mid(first), "{w0} → {w1}: block 1 at {first}");
                    run(e, &mut p, 1, |_, _| {});
                    assert_eq!(e.wave_now()[k], Some(w1), "{w0} → {w1}: block 2");
                });
                let y = take(note, base, 2 * AT, |b, p| {
                    p.wave[k] = if b < AT { w0 } else { w1 }
                });
                let (d, lim) = click(&y, &anchors);
                let at = format!("note {note} WAVE{} {w0} → {w1}", k + 1);
                println!(
                    "{at}: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
                    100.0 * d / lim
                );
                assert!(d <= lim, "{at}: {d} > {lim}");
                if d / lim > worst.0 {
                    worst = (d / lim, at);
                }
            }
            // driven end to end every block, then held at 1
            let drive = |b: usize| {
                if b < AT {
                    0.0
                } else if b < 2 * AT {
                    (b % 2) as f32
                } else {
                    1.0
                }
            };
            with_engine(|e| {
                let mut p = base;
                e.hold();
                e.note_on(note, &p, part(0), SR);
                for b in 0..2 * AT + 2 {
                    p.wave[k] = drive(b);
                    run(e, &mut p, 1, |_, _| {});
                    if (AT..2 * AT).contains(&b) {
                        let v = e.wave_now()[k].unwrap();
                        assert!(v == 0.0 || mid(v), "block {b}: {v}");
                    }
                }
                assert_eq!(
                    e.wave_now()[k],
                    Some(1.0),
                    "the drive stops: home in 2 blocks"
                );
            });
            let y = take(note, base, 3 * AT, |b, p| p.wave[k] = drive(b));
            let (d, lim) = click(&y, &anchors);
            let at = format!("note {note} WAVE{} every block", k + 1);
            println!(
                "{at}: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
                100.0 * d / lim
            );
            assert!(d <= lim, "{at}: {d} > {lim}");
            if d / lim > worst.0 {
                worst = (d / lim, at);
            }
        }
    }
    println!("worst {:.0}% of the limit at {}", 100.0 * worst.0, worst.1);
}

/// SYNC's master stops at 7 kHz (ADR 0118): asked for 8.4 kHz it plays
/// 7 kHz, as a master asked for 9.9 kHz does (the same bits), so the synced
/// wave repeats every 48 samples (7 master cycles, to the phase's f32
/// rounding); a pitch rising through the ceiling stays finite, peak-safe
/// and click-free against the two held.
#[test]
fn sync_master_stops_at_its_ceiling() {
    // CRSE2 +24: the slave sits at the oscillators' own top in both asks,
    // so only the master's ceiling is under test
    let base = VaParams {
        mode: VaMode::Sync,
        coarse: [0, 24],
        ..sine()
    };
    // C8 is 4186 Hz: +1200 cents asks 8.4 kHz, +1500 cents 9.9 kHz
    let at = |cents: f32| {
        with_engine(|e| {
            let mut p = base;
            e.set_pitch(cents);
            e.hold();
            e.note_on(108, &p, part(0), SR);
            run(e, &mut p, AT, |_, _| {})
        })
    };
    let (y, z) = (at(1200.0), at(1500.0));
    assert_eq!(y, z, "both at the ceiling");
    let tail = &y[y.len() - 4 * B..];
    let period = (48..tail.len())
        .map(|n| (tail[n] - tail[n - 48]).abs())
        .fold(0.0f32, f32::max);
    println!("SYNC master at the ceiling: |y[n] − y[n − 48]| ≤ {period:.1e}");
    assert!(period <= 1e-5, "{period}");
    // rising through the ceiling, a block at a time
    let held: Vec<_> = [0.0, 1200.0].map(at).into();
    let y = with_engine(|e| {
        let mut p = base;
        e.hold();
        e.note_on(108, &p, part(0), SR);
        let mut y = Vec::new();
        for b in 0..2 * AT {
            e.set_pitch(1200.0 * (b as f32 / AT as f32).min(1.0));
            y.extend(run(e, &mut p, 1, |_, _| {}));
        }
        y
    });
    assert!(y.iter().all(|v| v.is_finite()));
    let pk = peak(&y) * VA_CHAIN_GAIN;
    let (d, lim) = click(&y, &held);
    println!(
        "rising through 7 kHz: peak {pk:.3}, Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
        100.0 * d / lim
    );
    assert!(pk <= 0.95, "{pk}");
    assert!(d <= lim, "{d} > {lim}");
}

/// A block starts two runs an oscillator at most (one WAVE segment
/// crossing, ADR 0118): WAVE jumped to a random target every block, in every
/// MODE, from every segment (the jumps from the third segment down are the
/// ones whose rounding could reach past ⅓).
#[test]
fn a_block_never_starts_a_third_run() {
    let mut x = 0x2545_F491u32;
    let mut uni = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x as f32 / u32::MAX as f32
    };
    let mut most = 0u8;
    for mode in VaMode::ALL {
        with_engine(|e| {
            let mut p = VaParams { mode, ..sine() };
            e.hold();
            e.note_on(A4, &p, part(0), SR);
            for b in 0..600 {
                // every other block from high in the third segment
                p.wave = if b % 2 == 0 {
                    [0.7 + 0.3 * uni(), 0.7 + 0.3 * uni()]
                } else {
                    [uni(), uni()]
                };
                p.shape = [uni(), uni()];
                run(e, &mut p, 1, |_, _| {});
                let r = e.runs();
                assert!(r[0] <= 2 && r[1] <= 2, "{mode:?} block {b}: {r:?} runs");
                most = most.max(r[0].max(r[1]));
            }
        });
    }
    println!("most runs in a block: {most}");
    assert_eq!(most, 2, "the drive crosses an edge");
    // the oscillator itself, handed ramps across both edges in one block
    // (past the engine's reach): still two runs, the second to the end
    for (w0, w1) in [(0.05f32, 0.95f32), (0.95, 0.05), (0.0, 1.0), (1.0, 0.0)] {
        let mut o = VarOsc::NEW;
        o.prepare(SR as f32);
        let r = Ramps {
            inc: (0.01, 0.0),
            wave: (w0, (w1 - w0) / B as f32),
            shape: (0.3, 0.0),
        };
        let (mut out, mut mk) = ([0.0f32; B], [[NO_WRAP; 2]; B]);
        o.begin(&r, B);
        o.render::<B, false, false, false, false>(
            &mut out,
            &mut mk,
            &r,
            &mut 1.0,
            &mut || 1.0,
            &ZEROS,
            (0.0, 0.0),
        );
        assert_eq!(o.runs(), 2, "{w0} → {w1}");
        assert!(
            out.iter().all(|v| v.is_finite() && v.abs() < 1.0),
            "{w0} → {w1}"
        );
    }
}

#[test]
fn the_duck_is_two_blocks_each_way() {
    let g_down = |j: usize| 0.5 * (1.0 + (std::f64::consts::PI * (j + 1) as f64 / 128.0).cos());
    let (mut viol, mut slack) = (0, f64::MAX);
    for (m0, m1) in PAIRS {
        let base = pair_base(m0, m1);
        let steady = [m0, m1]
            .iter()
            .map(|&m| peak(&held(A4, VaParams { mode: m, ..base }, 2 * AT)[2 * B..]))
            .fold(0.0f32, f32::max) as f64;
        with_engine(|e| {
            let mut p = VaParams { mode: m0, ..base };
            e.hold();
            e.note_on(A4, &p, part(0), SR);
            run(e, &mut p, AT, |_, _| {});
            p.mode = m1;
            let mut y = Vec::new();
            for b in 0..4 {
                y.extend(run(e, &mut p, 1, |_, _| {}));
                let want = if b < 1 { m0 } else { m1 };
                assert_eq!(e.mode(), want, "{m0:?} → {m1:?} after block {b}");
            }
            for (j, &v) in y.iter().enumerate() {
                let g = if j < 128 {
                    g_down(j)
                } else {
                    1.0 - g_down(j - 128)
                };
                // the margin under the bound plus its absolute tolerance
                slack = slack.min(g * steady + 1e-6 - f64::from(v.abs()));
                if f64::from(v.abs()) > g * steady + 1e-6 {
                    viol += 1;
                    println!("{m0:?} → {m1:?} sample {j}: {v} > {}", g * steady);
                }
            }
        });
    }
    println!("least margin under the duck's bound + 1e-6: {slack:.2e} (of 1e-6 tolerance)");
    assert_eq!(viol, 0);
}

#[test]
fn every_knob_is_audible() {
    let base = VaParams {
        wave: [0.3, 0.7],
        shape: [0.3; 2],
        amount: 0.3,
        sub: 0.3,
        stability: 1.0,
        ..VaParams::default()
    };
    let cases: [(&str, VaParams, Set, (f32, f32)); 16] = [
        ("WAVE1", base, |p, v| p.wave[0] = v, (0.0, 1.0)),
        ("WAVE2", base, |p, v| p.wave[1] = v, (0.0, 1.0)),
        ("SUB", base, |p, v| p.sub = v, (0.0, 1.0)),
        ("STAB", base, |p, v| p.stability = v, (0.0, 1.0)),
        ("FINE1", base, |p, v| p.fine[0] = v, (-100.0, 100.0)),
        ("FINE2", base, |p, v| p.fine[1] = v, (-100.0, 100.0)),
        ("SHAPE1", base, |p, v| p.shape[0] = v, (0.0, 1.0)),
        ("SHAPE2", base, |p, v| p.shape[1] = v, (0.0, 1.0)),
        ("AMOUNT Mix", base, |p, v| p.amount = v, (0.0, 1.0)),
        (
            "AMOUNT Sync",
            VaParams {
                mode: VaMode::Sync,
                coarse: [0, 7],
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
        (
            "AMOUNT Ring",
            VaParams {
                mode: VaMode::Ring,
                fine: [0.0, 6.0],
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
        (
            "AMOUNT Xor",
            VaParams {
                mode: VaMode::Xor,
                fine: [0.0, 6.0],
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
        (
            "AMOUNT Am",
            VaParams {
                mode: VaMode::Am,
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
        (
            "AMOUNT Phase",
            VaParams {
                mode: VaMode::Phase,
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
        (
            "AMOUNT Fm",
            VaParams {
                mode: VaMode::Fm,
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
        (
            "AMOUNT Max",
            VaParams {
                mode: VaMode::Max,
                ..base
            },
            |p, v| p.amount = v,
            (0.0, 1.0),
        ),
    ];
    for (name, base, knob, (lo, hi)) in cases {
        let at = |v| take(A4, base, 750, |_, p| knob(p, v));
        let (y0, y1) = (at(lo), at(hi));
        let diff: Vec<f32> = y0.iter().zip(&y1).map(|(a, b)| a - b).collect();
        let r = rms(&diff) / rms(&y0);
        println!("{name}: rms(y0 − y1)/rms(y0) {r:.3}");
        assert!(r >= 0.1, "{name}: {r}");
    }
}

#[test]
fn ring_gain_is_continuous() {
    with_engine(|e| {
        let mut p = VaParams {
            mode: VaMode::Ring,
            amount: 1.0,
            wave: [1.0; 2],
            // OSC 2's narrow pulse makes the cap bind
            shape: [0.6, 1.0],
            ..VaParams::default()
        };
        e.hold();
        e.note_on(A4, &p, part(0), SR);
        let mut last: Option<[f32; 2]> = None;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for b in 0..SWEEP_LEN {
            run(e, &mut p, 1, |_, p| p.shape[0] = ramp_at(b, 0.6, 1.0));
            let k = e.ring_k();
            if let Some(l) = last {
                assert_eq!(k[0], l[1], "block {b}");
            }
            (lo, hi) = (lo.min(k[1]), hi.max(k[1]));
            last = Some(k);
        }
        println!("k {lo:.4} … {hi:.4}");
        assert!(hi - lo > 0.1, "the cap never moved: {lo} {hi}");
    });
}

#[test]
fn xor_of_identical_oscillators_is_silent() {
    let base = VaParams {
        mode: VaMode::Xor,
        amount: 1.0,
        ..VaParams::default()
    };
    let r = rms(&held(A4, base, 200));
    println!("rms {r:.2e}");
    assert!(r < 1e-3, "{r}");
}

#[test]
fn peak_safe_everywhere() {
    let mut worst = (0.0f32, String::new());
    for note in [C2, C6] {
        for mode in VaMode::ALL {
            for amount in [0.0, 0.5, 1.0] {
                // 0.2222 is the sub mix's worst point
                for sub in [0.0, 0.2222, 0.5, 1.0] {
                    for w in ANCHORS {
                        for s in [0.0, 1.0] {
                            let base = VaParams {
                                wave: [w; 2],
                                shape: [s; 2],
                                mode,
                                amount,
                                sub,
                                ..VaParams::default()
                            };
                            let pk = peak(&held(note, base, 188));
                            let at = format!(
                                "note {note} {mode:?} x {amount} SUB {sub} WAVE {w:.3} SHAPE {s}"
                            );
                            assert!(pk <= 0.95, "{at}: {pk}");
                            if pk > worst.0 {
                                worst = (pk, at);
                            }
                        }
                    }
                }
            }
        }
    }
    println!(
        "peak {:.4} (limit 0.95, margin {:.4}) at {}",
        worst.0,
        0.95 - worst.0,
        worst.1
    );
}

#[test]
fn a_restrike_keeps_phase() {
    for again in [A4, C5] {
        let base = sine();
        let y = with_engine(|e| {
            let mut p = base;
            e.hold();
            e.note_on(A4, &p, part(0), SR);
            let mut y = run(e, &mut p, 75, |_, _| {});
            let before = e.osc_phase();
            e.note_on(again, &p, part(0), SR);
            let after = e.osc_phase();
            for k in 0..2 {
                assert!(
                    (after[k] - before[k]).abs() <= 1e-6,
                    "osc {k}: {} vs {}",
                    after[k],
                    before[k]
                );
            }
            y.extend(run(e, &mut p, 75, |_, _| {}));
            y
        });
        let held: Vec<_> = [A4, again].iter().map(|&n| held(n, base, 150)).collect();
        if again == A4 {
            assert_eq!(y, held[0], "a re-strike changed the render");
        }
        let (d, lim) = click(&y, &held);
        println!("re-strike on {again}: Δ² {d:.3e} / limit {lim:.3e}");
        assert!(d <= lim, "{again}: {d} > {lim}");
    }
}

#[test]
fn stab_one_resets_an_idle_engine() {
    let base = VaParams::default();
    let a = take_on((0, 0), |_| {}, A4, base, 375, |_, _| {});
    let b = take_on((3, 5), |_| {}, A4, base, 375, |_, _| {});
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn sub_follows_osc1_tracked() {
    let base = VaParams {
        wave: [0.0; 2],
        amount: 0.0,
        stability: 0.0,
        ..VaParams::default()
    };
    let blocks = (1 << 17) / B + 1;
    let y0 = held(C7, base, blocks);
    let y1 = held(C7, VaParams { sub: 1.0, ..base }, blocks);
    let s: Vec<f32> = y0
        .iter()
        .zip(&y1)
        .map(|(c, o)| 2f32.sqrt() * o - c)
        .collect();
    let f1 = peak_hz(&y0, SR as f32, hz_of(C7, 0.0));
    let fs = peak_hz(&s, SR as f32, (0.5 * f1) as f32);
    let c = 1200.0 * (2.0 * fs / f1).log2();
    println!(
        "OSC 1 {f1:.3} Hz ({:+.2} cents), sub {fs:.3} Hz: {c:+.4} cents off half",
        1200.0 * (f1 / f64::from(hz_of(C7, 0.0))).log2()
    );
    assert!(c.abs() <= 0.5, "{c}");
}

#[test]
fn steal_glide_glides() {
    with_engine(|e| {
        let mut p = VaParams::default();
        e.hold();
        e.note_on(A4 + 12, &p, part(0), SR);
        run(e, &mut p, 4, |_, _| {});
        e.glide_on(A4, &p, part(0), (SR, 0.1));
        assert!((e.slide() - 2.0).abs() < 1e-6, "{}", e.slide());
        let mut hit = None;
        for b in 1..=2000 {
            run(e, &mut p, 1, |_, _| {});
            if hit.is_none() && e.slide().log2() <= 1.0 - 0.632 {
                hit = Some(b);
            }
        }
        let want = 0.1 * SR as f32 / B as f32;
        println!("63 % at block {hit:?} (want {want})");
        assert!((hit.unwrap() as f32 - want).abs() <= 1.0, "{hit:?}");
        assert_eq!(e.slide(), 1.0);
    });
}

#[test]
fn bills_follow_the_mode() {
    let bills = [530, 540, 530, 550, 530, 590, 540, 590, 650];
    assert!(VaEngine::mode_cost(VaMode::Sync) > VaEngine::mode_cost(VaMode::Mix));
    for (mode, bill) in VaMode::ALL.into_iter().zip(bills) {
        assert_eq!(VaEngine::mode_cost(mode), Cost(bill), "{mode:?}");
        let p = VaParams {
            mode,
            ..VaParams::default()
        };
        assert_eq!(VaEngine::cost(&p), VaEngine::mode_cost(mode));
        with_engine(|e| {
            assert_eq!(e.playing_cost(), None);
            e.note_on(A4, &p, part(0), SR);
            assert_eq!(e.playing_cost(), Some(VaEngine::mode_cost(e.mode())));
            assert_eq!(e.mode(), mode);
        });
    }
}

#[test]
fn midi_extremes_stay_finite() {
    let cases: [(u8, i8, f32, f32); 2] = [(127, 24, 2400.0, 100.0), (0, -24, 0.0, 0.0)];
    for (note, crse, pitch, fine) in cases {
        for mode in VaMode::ALL {
            let base = VaParams {
                coarse: [crse; 2],
                fine: [fine; 2],
                mode,
                sub: 0.5,
                ..VaParams::default()
            };
            let y = with_engine(|e| {
                let mut p = base;
                e.set_pitch(pitch);
                e.note_on(note, &p, part(0), SR);
                run(e, &mut p, 750, |_, _| {})
            });
            let at = format!("note {note} CRSE {crse} pitch {pitch} {mode:?}");
            assert!(y.iter().all(|v| v.is_finite()), "{at}");
            assert!(peak(&y) <= 0.95, "{at}: {}", peak(&y));
            assert!(rms(&y) > 1e-3, "{at}: {}", rms(&y));
        }
    }
}

#[test]
fn out_of_range_values_clamp() {
    let nan = f32::NAN;
    let mut n = 0;
    for wave in [1.7, -0.3, nan] {
        for shape in [-0.4, 1.9, nan] {
            for stability in [-1.0, 2.0, nan] {
                for mode in VaMode::ALL {
                    let base = VaParams {
                        wave: [wave; 2],
                        shape: [shape; 2],
                        amount: 2.0,
                        sub: 3.0,
                        stability,
                        fine: [nan, 400.0],
                        mode,
                        ..VaParams::default()
                    };
                    let y = held(A4, base, 40);
                    let at = format!("wave {wave} shape {shape} stab {stability} {mode:?}");
                    assert!(y.iter().all(|v| v.is_finite()), "{at}");
                    assert!(peak(&y) <= 0.95, "{at}: {}", peak(&y));
                    n += 1;
                }
            }
        }
    }
    assert_eq!(n, 243);
}

#[test]
fn mode_scrolled_every_block_settles() {
    const SCROLL: [VaMode; 7] = [
        VaMode::Mix,
        VaMode::Sync,
        VaMode::Phase,
        VaMode::Ring,
        VaMode::Fm,
        VaMode::Am,
        VaMode::Max,
    ];
    let base = pair_base(VaMode::Sync, VaMode::Mix);
    let mut modes = Vec::new();
    let y = with_engine(|e| {
        let mut p = base;
        e.hold();
        e.note_on(A4, &p, part(0), SR);
        let y = run(e, &mut p, 60, |b, p| {
            p.mode = if b < 40 { SCROLL[b % 7] } else { VaMode::Ring };
        });
        // replayed for mode() after each block
        let mut p = base;
        with_engine(|f| {
            f.hold();
            f.note_on(A4, &p, part(0), SR);
            for b in 0..60 {
                run(f, &mut p, 1, |_, p| {
                    p.mode = if b < 40 { SCROLL[b % 7] } else { VaMode::Ring };
                });
                modes.push(f.mode());
            }
        });
        y
    });
    let held: Vec<_> = SCROLL
        .iter()
        .map(|&m| held(A4, VaParams { mode: m, ..base }, 60))
        .collect();
    let (d, lim) = click(&y, &held);
    println!(
        "scrolled: Δ² {d:.3e} / limit {lim:.3e} ({:.0}%)",
        100.0 * d / lim
    );
    assert!(d <= lim, "{d} > {lim}");
    // the last change is at block 39 → 40; within 4 blocks of it
    let settled = (40..60)
        .find(|&b| modes[b..].iter().all(|&m| m == VaMode::Ring))
        .unwrap();
    println!("Ring from block {settled}");
    assert!(settled < 40 + 4, "{settled}");
}

/// The engine's oscillator `k` alone under a WAVE sweep at SHAPE `s`,
/// against the triangle-kernel reference along its own phase (as
/// `va_osc_test`'s `tracks`): max error.
fn wave_tracks(k: usize, s: f32) -> f64 {
    let base = VaParams {
        shape: [s; 2],
        amount: k as f32,
        ..sine()
    };
    let y = take(A4, base, SWEEP_LEN, |b, p| p.wave[k] = ramp_at(b, 0.0, 1.0));
    let n = y.len();
    // the engine's phase, replayed in f32: STAB 1 starts it at 0
    let inc = 440.0f32 * (1.0 / 48_000.0f32);
    let mut phase = vec![0.0f64];
    let mut p = 0.0f32;
    for _ in 0..n {
        p += inc;
        if p >= 1.0 {
            p -= 1.0;
        }
        phase.push(f64::from(p));
    }
    // tick i (1-based) plays WAVE (i − PRE·B)/(SWEEP·B), from block PRE on
    let set = |t: f64| {
        // `Unit` clamps it to 0..=1
        let w = (t - (PRE * B) as f64) / (SWEEP * B) as f64;
        Morph {
            wave: Unit::new(w as f32),
            shape: Unit::new(s),
        }
    };
    let f = |t: f64| {
        let i = (t.max(0.0) as usize).min(n);
        let q = (phase[i] + f64::from(inc) * (t - i as f64)).rem_euclid(1.0);
        f64::from(wave_at(q as f32, set(t), inc).0)
    };
    let reference = tri_reference(f, n + 1);
    (3..=n)
        .map(|i| (f64::from(y[i - 1]) - reference[i]).abs())
        .fold(0.0, f64::max)
}

#[test]
fn wave_sweeps_track_the_oscillator() {
    for k in 0..2 {
        for s in [0.0, 0.5, 1.0] {
            let e = wave_tracks(k, s);
            println!("WAVE{} sweep at SHAPE {s}: {e:.2e}", k + 1);
            assert!(e <= 2e-3, "WAVE{} SHAPE {s}: {e}", k + 1);
        }
    }
}

/// WAVE driven past two segment boundaries every block (an audio-rate
/// LFO on WAVE): each block splits into three runs, and the render stays
/// finite and peak-safe in every MODE, with SHAPE moving too.
#[test]
fn wave_crossing_segments_every_block_stays_safe() {
    for mode in VaMode::ALL {
        let base = VaParams {
            mode,
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let y = take(A4, base, 300, |b, p| {
            let w = if b % 2 == 0 { 0.0 } else { 1.0 };
            p.wave = [w, 1.0 - w];
            p.shape = [(b % 7) as f32 / 6.0; 2];
        });
        assert!(y.iter().all(|v| v.is_finite()), "{mode:?}");
        assert!(peak(&y) <= 0.95, "{mode:?}: {}", peak(&y));
        assert!(rms(&y) > 1e-3, "{mode:?}");
    }
}
