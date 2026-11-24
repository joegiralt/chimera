// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SEM, the Oberheim SEM-1A VCF from its circuit (ADR 0192): the firmware
//! against the circuit lab's model on its stimuli, MODE's notch, RES's
//! law, the ring that never holds, LEVEL and the LED, smooth sweeps, and
//! its codes on old and new Sounds.

mod common;

use chimera_core::block::DiskCode;
use chimera_core::dsp::filter::sem::{IN, OUT, k_pot, sem_k};
use chimera_core::dsp::filter::{Fed, Filter, FilterKind, FilterMode, FilterMods};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::storage::RecordTag;
use chimera_hal::BLOCK_SIZE;
use common::codec_util::{decode, encode, filter_payload, sound_file as file};
use common::filter_ref::{mag_db, rms_db};

const SR: u32 = 48_000;
const FS: f32 = SR as f32;

fn params(bp: bool, fc: f32, res: f32, morph: f32, level: f32) -> FilterParams {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::Sem);
    assert!(p.set_mode(if bp {
        FilterMode::Bp12
    } else {
        FilterMode::Lp12
    }));
    (p.cutoff, p.resonance, p.morph, p.level) = (fc, res, morph, level);
    p
}

/// `n` samples through a fresh SEM at a held setting, fed `x(i)`.
fn render(p: &FilterParams, n: usize, x: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut f = Filter::new();
    let mut out = Vec::with_capacity(n);
    for b in 0..n.div_ceil(BLOCK_SIZE) {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        f.process(&mut buf, p, SR, &FilterMods::NONE);
        out.extend_from_slice(&buf);
    }
    out.truncate(n);
    out
}

fn sine(amp: f32, f: f32) -> impl Fn(usize) -> f32 {
    move |i| amp * (core::f32::consts::TAU * f * i as f32 / FS).sin()
}

/// The output's level at `f` over gain, dB, for a sine of `amp`: half a
/// second to settle, then whole periods read.
fn gain_db(p: &FilterParams, amp: f32, f: f32) -> f32 {
    let n = 48_000;
    let y = render(p, n, sine(amp, f));
    let per = FS / f;
    let take = ((24_000.0 / per).floor() * per) as usize;
    mag_db(&y[n - take..], f, FS) - 20.0 * amp.log10()
}

struct Case {
    drive: f32,
    bp: bool,
    ctl: Vec<[f32; 3]>,
    x: Vec<f32>,
    y: Vec<f32>,
}

/// The lab's fixtures (`semc-fixtures`, `export.rs` on `sem-circuit-lab`):
/// `fw::Fw` on its stimuli, the controls a block's, no couplings.
fn fixtures() -> Vec<Case> {
    let raw = include_bytes!("fixtures/sem_fw_v1.bin");
    let mut words = raw.as_chunks::<4>().0.iter().copied().peekable();
    let mut word = || words.next().unwrap();
    let mut cases = Vec::new();
    for _ in 0..6 {
        let blocks = u32::from_le_bytes(word()) as usize;
        let drive = f32::from_le_bytes(word());
        let bp = u32::from_le_bytes(word()) != 0;
        let ctl = (0..blocks)
            .map(|_| [0; 3].map(|_| f32::from_le_bytes(word())))
            .collect();
        let n = blocks * BLOCK_SIZE;
        let x = (0..n).map(|_| f32::from_le_bytes(word())).collect();
        let y = (0..n).map(|_| f32::from_le_bytes(word())).collect();
        cases.push(Case {
            drive,
            bp,
            ctl,
            x,
            y,
        });
    }
    cases
}

/// The firmware SEM is the lab's firmware model (#720), which nulls the
/// circuit to −32 dB on average: on each of the lab's six fixtures (bass,
/// brass BP at +12 dB and RES 1, the MODE sweep, the pad, the cutoff
/// sweep at RES 1 and +12 dB, the RES sweep) the residual sits under
/// −90 dB of the fixture's level (−103 to −138 measured). What differs
/// is f32 against the lab's f64 coefficients and `tan_accurate` against
/// `tan`.
#[test]
fn the_firmware_is_the_labs_model() {
    let cases = fixtures();
    assert_eq!(cases.len(), 6);
    for (c, case) in cases.iter().enumerate() {
        let mut f = Filter::new();
        let mut p = params(case.bp, 1000.0, 0.0, 0.0, (case.drive.log2() + 2.0) / 4.0);
        let mut y = Vec::new();
        for (b, ctl) in case.ctl.iter().enumerate() {
            (p.cutoff, p.resonance, p.morph) = (ctl[0], ctl[1], ctl[2]);
            let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| case.x[b * BLOCK_SIZE + i]);
            f.process(&mut buf, &p, SR, &FilterMods::NONE);
            y.extend_from_slice(&buf);
        }
        let err: Vec<f32> = y.iter().zip(&case.y).map(|(a, b)| a - b).collect();
        let null = rms_db(&err) - rms_db(&case.y);
        eprintln!("case {c}: null {null:.1} dB");
        assert!(null < -90.0, "case {c}: null {null:.1} dB");
    }
}

/// MODE's notch sits at `FREQ·√((1 − m)/m)` (PS19 between HP and LP): at
/// m ¼, ½ and ¾ and FREQ 1 kHz, 1732, 1000 and 577 Hz, at least 30 dB
/// under the response a third of an octave either side; at ½ the two bands
/// sit 6 dB down.
#[test]
fn mode_notches_where_the_pot_puts_it() {
    for m in [0.25f32, 0.5, 0.75] {
        let p = params(false, 1000.0, 0.0, m, 0.5);
        let at = 1000.0 * ((1.0 - m) / m).sqrt();
        let deep = gain_db(&p, 0.1, at);
        for side in [at / 1.26, at * 1.26] {
            let g = gain_db(&p, 0.1, side);
            assert!(
                g - deep > 30.0,
                "m {m}: {deep:.1} at {at:.0} vs {g:.1} at {side:.0}"
            );
        }
    }
    let p = params(false, 1000.0, 0.0, 0.5, 0.5);
    for f in [50.0, 15000.0] {
        let g = gain_db(&p, 0.1, f);
        assert!((g + 6.02).abs() < 0.5, "{f}: {g:.2}");
    }
}

/// RES keeps the circuit's law (ADR 0192, not spread): the pot's k is
/// 3.02 at RES 0, 1.10 at ½, 0.27 at 0.9 and 0.006 at 1, and BANDPASS's
/// peak at FREQ is 1/k: Q 0.33, 0.90, 3.5, then the last tenth of the knob
/// to Q 52 at 1 kHz.
#[test]
fn res_keeps_the_circuits_law() {
    for (r, want) in [
        (0.0, 3.015_1),
        (0.5, 1.097_7),
        (0.9, 0.270_3),
        (1.0, 0.006_0),
    ] {
        let k = k_pot(r);
        assert!((k - want).abs() < 2e-4, "RES {r}: {k}");
    }
    for r in [0.0f32, 0.5, 0.9] {
        let p = params(true, 1000.0, r, 0.0, 0.5);
        let g = gain_db(&p, 0.01, 1000.0);
        let want = -20.0 * sem_k(1000.0, r).log10();
        assert!((g - want).abs() < 0.3, "RES {r}: {g:.2} vs {want:.2}");
    }
    assert!(1.0 / sem_k(1000.0, 0.9) < 4.0);
    assert!(1.0 / sem_k(1000.0, 1.0) > 50.0);
    // k falls all the way: monotone in RES.
    let mut last = f32::INFINITY;
    for i in 0..=100 {
        let k = sem_k(1000.0, i as f32 / 100.0);
        assert!(k < last && k > 0.0);
        last = k;
    }
}

/// The SEM never holds a tone (its manual's "minimum resonance" at CCW,
/// the lab's ring-outs): at RES 1, a burst rings and decays at 100 Hz, 1
/// kHz and 10 kHz, at LEVEL ½ and driven at LEVEL 1, with STABILITY's
/// spread at its widest and with FB. Nothing is left 2.4 s after the
/// burst (the 100 Hz ring falls 40 dB in 0.4 s).
#[test]
fn it_never_self_oscillates() {
    use chimera_core::dsp::filter::Spread;
    let spread = Spread {
        cut: (1.0, 1.0),
        res: (0.88, 0.88),
    };
    for fc in [100.0f32, 1000.0, 10000.0] {
        for level in [0.5f32, 1.0] {
            for (spread, fb) in [(Spread::NONE, 0.0f32), (spread, 0.0), (Spread::NONE, 0.5)] {
                let p = params(false, fc, 1.0, 0.0, level);
                let mods = FilterMods {
                    spread,
                    fb: Fed::new(fb, fb),
                    ..FilterMods::NONE
                };
                let mut f = Filter::new();
                let mut tail = 0.0f32;
                for b in 0..2000 {
                    let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                        let n = b * BLOCK_SIZE + i;
                        if n < 96 {
                            0.8 * ((n % 24) as f32 / 12.0 - 1.0)
                        } else {
                            0.0
                        }
                    });
                    f.process(&mut buf, &p, SR, &mods);
                    if b >= 1850 {
                        tail = buf.iter().fold(tail, |m, v| m.max(v.abs()));
                    }
                }
                assert!(tail < 1e-4, "{fc} Hz, LEVEL {level}, fb {fb}: {tail}");
                assert!(!f.rings(), "{fc} Hz, LEVEL {level}, fb {fb}");
            }
        }
    }
}

/// LEVEL drives the circuit (no clipper): ½ is unity, so MODE's LP sits
/// at 0 dB in its pass band as CLEAN's LP2 does, and 1 is ×4, +12 dB; the
/// diodes squash a full-RES peak gradually, never clipping it hard, and
/// the LED reads A14's BP past a third of its swing.
#[test]
fn level_drives_and_lights_the_led() {
    for (level, want) in [(0.5f32, 0.0f32), (1.0, 12.04), (0.0, -12.04)] {
        let g = gain_db(&params(false, 2000.0, 0.0, 0.0, level), 0.1, 100.0);
        assert!((g - want).abs() < 0.3, "LEVEL {level}: {g:.2}");
    }
    // A full-RES peak: +12 dB of drive gains far less than 12 dB.
    let soft = gain_db(&params(false, 1000.0, 1.0, 0.0, 0.5), 0.02, 1000.0);
    let hot = gain_db(&params(false, 1000.0, 1.0, 0.0, 1.0), 0.5, 1000.0) - 12.04;
    assert!(soft - hot > 10.0, "{soft:.1} vs {hot:.1}");
    let led = |p: &FilterParams, amp: f32| {
        let mut f = Filter::new();
        let mut c = 0.0f32;
        for b in 0..40 {
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| sine(amp, 300.0)(b * BLOCK_SIZE + i));
            c = c.max(f.process(&mut buf, p, SR, &FilterMods::NONE).get());
        }
        c
    };
    assert_eq!(led(&params(false, 1000.0, 0.0, 0.0, 0.5), 0.5), 0.0);
    assert!(led(&params(false, 300.0, 0.95, 0.0, 1.0), 0.8) > 0.5);
    // One VCO's worth is `IN` volts; the output undoes it.
    assert!((IN * OUT + 1.0).abs() < 1e-6);
}

/// A sweep of MODE, FREQ or RES across blocks is smooth: each block ramps
/// from the last, so a held sine under a full sweep in one second leaves
/// nothing above 6 kHz but its own harmonics, which the SEM keeps under
/// −70 dB (no zipper, a design rule).
#[test]
fn sweeps_never_zipper() {
    let n = 48_000;
    for which in 0..3 {
        let mut f = Filter::new();
        let mut p = params(false, 1000.0, 0.5, 0.0, 0.5);
        let mut y = Vec::new();
        for b in 0..n / BLOCK_SIZE {
            let t = b as f32 * BLOCK_SIZE as f32 / n as f32;
            match which {
                0 => p.morph = t,
                1 => p.cutoff = 200.0 * 2f32.powf(4.0 * t),
                _ => p.resonance = 0.95 * t,
            }
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| sine(0.3, 110.0)(b * BLOCK_SIZE + i));
            f.process(&mut buf, &p, SR, &FilterMods::NONE);
            y.extend_from_slice(&buf);
        }
        // Energy above 6 kHz: a 4th-order difference, which a block step
        // would light up. Against the signal's own.
        let hi: Vec<f32> = y
            .windows(5)
            .map(|w| w[0] - 4.0 * w[1] + 6.0 * w[2] - 4.0 * w[3] + w[4])
            .collect();
        let r = rms_db(&hi) - rms_db(&y);
        assert!(r < -70.0, "sweep {which}: {r:.1} dB");
    }
}

/// KIND 8 is SEM, ident "SEM", its two outputs MODE (LP2's code) and
/// BANDPASS (BP2's), MODE the default; the old kinds keep their codes; a
/// SEM Sound round-trips with its MODE knob; a file without the MODE knob
/// loads LP; a mode SEM lacks loads as MODE.
#[test]
fn sem_codes_round_trip_and_old_sounds_load() {
    let k = FilterKind::Sem;
    assert_eq!((k.disk_code(), k.disk_ident()), (8, "SEM"));
    assert_eq!(FilterKind::from_disk_code(8), Some(k));
    for (kind, code) in [
        (FilterKind::Svf, 0),
        (FilterKind::Ladder, 1),
        (FilterKind::K2040, 4),
        (FilterKind::Dirty, 6),
    ] {
        assert_eq!(kind.disk_code(), code);
    }
    assert_eq!(FilterKind::from_disk_code(7), None, "LPG's, kept");
    assert_eq!(k.modes(), [FilterMode::Lp12, FilterMode::Bp12]);
    assert_eq!(k.default_mode(), FilterMode::Lp12);
    for bp in [false, true] {
        let mut s = Sound::init(EngineType::Algo);
        s.params.filter = params(bp, 700.0, 0.8, 0.37, 0.6);
        let d = decode(&encode(&s)).unwrap();
        assert!(d.bits_eq(&s), "bp {bp}");
        assert_eq!(d.params.filter.kind(), k);
        assert_eq!(d.params.filter.morph, 0.37);
    }
    let engine = (RecordTag::Engine, vec![EngineType::Algo.disk_code()]);
    let load = |e: &[(u8, [u8; 4])]| {
        let block = (RecordTag::Block, filter_payload(e));
        decode(&file(&[engine.clone(), block]))
            .unwrap()
            .params
            .filter
    };
    let code = |c: u32| c.to_le_bytes();
    // Old Sounds keep their KIND and MODE; MORPH is inert at 0.
    for (kind, mode) in [(0u32, 3u32), (1, 4), (4, 2), (6, 9)] {
        let f = load(&[(6, code(kind)), (7, code(mode))]);
        assert_eq!(f.kind().disk_code(), kind as u8);
        assert_eq!(f.mode().disk_code(), mode as u8);
        assert_eq!(f.morph, 0.0);
    }
    let f = load(&[(6, code(8)), (7, code(3))]);
    assert_eq!((f.kind(), f.mode(), f.morph), (k, FilterMode::Bp12, 0.0));
    for mode in [2u32, 6, 7, 255] {
        let f = load(&[(6, code(8)), (7, code(mode))]);
        assert_eq!((f.kind(), f.mode()), (k, FilterMode::Lp12), "MODE {mode}");
    }
    let f = load(&[(6, code(8)), (16, 0.75f32.to_bits().to_le_bytes())]);
    assert_eq!((f.kind(), f.morph), (k, 0.75));
}

/// Every state and output is held (`clampf`, a NaN to its floor), so a
/// NaN, an infinite or a huge input leaves the output finite and inside
/// `SEM_BOUND`, and the next ordinary block plays.
#[test]
fn bad_input_stays_bounded() {
    use chimera_core::dsp::filter::sem::SEM_BOUND;
    for bp in [false, true] {
        let p = params(bp, 1000.0, 1.0, 0.3, 1.0);
        let mut f = Filter::new();
        for bad in [f32::NAN, f32::INFINITY, -1e30, 1e9] {
            let mut buf = [bad; BLOCK_SIZE];
            f.process(&mut buf, &p, SR, &FilterMods::NONE);
            assert!(
                buf.iter().all(|y| y.is_finite() && y.abs() <= SEM_BOUND),
                "{bad}"
            );
            let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| sine(0.5, 300.0)(i));
            f.process(&mut buf, &p, SR, &FilterMods::NONE);
            assert!(
                buf.iter().all(|y| y.is_finite() && y.abs() <= SEM_BOUND),
                "after {bad}"
            );
        }
    }
}
