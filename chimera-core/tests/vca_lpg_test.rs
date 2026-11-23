// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The VCA's low-pass gate (ADR 0195): LPG, DECAY and STRIKE at 0 leave
//! every Sound as it was, bit for bit.

mod common;

use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::va::VaMode;
use chimera_core::modulation::{FixedCol, ModSource, ModState};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::rig::Rig;

const SR: u32 = chimera_hal::SAMPLE_RATE;

/// One Sound's run: C3 at 100 for 40 blocks, re-struck at 40 for 40 (the
/// VEL ramp, #483), key up, 60 blocks of tail; once uneased, once glided.
fn play(p: &ParamSnapshot, m: &ModState, h: &mut u64) {
    for glided in [false, true] {
        let mut r = Rig::new(SR);
        let mut b = [0.0f32; BLOCK_SIZE];
        let mut out = Vec::new();
        for vel in [100, 40] {
            r.note_on(MidiNote::new(48).unwrap(), Velocity::new(vel).unwrap(), p);
            for _ in 0..40 {
                if glided {
                    r.render_glided(&mut b, p, m);
                } else {
                    r.render(&mut b, p, m);
                }
                out.extend_from_slice(&b);
            }
        }
        r.note_off();
        for _ in 0..60 {
            r.render(&mut b, p, m);
            out.extend_from_slice(&b);
        }
        assert!(out.iter().any(|&x| x != 0.0), "a run sounds");
        for s in out {
            for byte in s.to_bits().to_le_bytes() {
                *h = (*h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
}

/// The matrices a Sound's VCA sees: none, ENV 1 at 127 (TAPE's and VA's
/// INIT), and ENV 1 with LFO 1 at 64 beside it.
fn matrices(engine: EngineType) -> [ModState; 3] {
    let base = Sound::init(engine).mod_state;
    let (env, lfo) = (ModSource::Env1.index(), ModSource::Lfo1.index());
    let mut none = base.clone();
    none.clear_fixed(env, FixedCol::Vca);
    let mut routed = base.clone();
    routed.set_fixed(env, FixedCol::Vca, 127);
    let mut both = routed.clone();
    both.set_fixed(lfo, FixedCol::Vca, 64);
    [none, routed, both]
}

/// Every engine's INIT through each VCA matrix, VEL at 1 and ½, under
/// CLEAN, the 2040, DIRTY, LADDER and SEM; then every VA MODE. The
/// constant is the plain VCA's (ADR 0195), read again on the integrated
/// tree with the gate's door closed: TAPE's INIT is WALK's there. Re-taken
/// for VA AM's corrected product (#660, ADR 0321).
#[test]
fn the_gate_at_zero_is_todays_vca_bit_for_bit() {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for e in [
        EngineType::Algo,
        EngineType::Modal,
        EngineType::Tape,
        EngineType::Va,
    ] {
        for m in matrices(e) {
            for kind in [
                FilterKind::Svf,
                FilterKind::K2040,
                FilterKind::Dirty,
                FilterKind::Ladder,
                FilterKind::Sem,
            ] {
                for vel in [1.0, 0.5] {
                    let mut p = Sound::init(e).params;
                    p.lfos[0].depth = 1.0;
                    p.filter.set_kind(kind);
                    p.filter.cutoff = 2_000.0;
                    p.out.vca_vel = vel;
                    play(&p, &m, &mut h);
                }
            }
        }
    }
    for mode in VaMode::ALL {
        let mut p = Sound::init(EngineType::Va).params;
        p.va.mode = mode;
        p.va.amount = 0.6;
        play(&p, &matrices(EngineType::Va)[1], &mut h);
    }
    assert_eq!(h, 0x05fa_2323_38b4_855d, "hash {h:#018x}");
}

// ============================================================ Plaits

use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::lpg::{Drive, Knobs, Level, Lpg, Onset};
use chimera_core::dsp::va::osc::Unit;
use chimera_core::part::PartParams;
use common::codec_util::{decode, encode, sound_file as file};
use common::filter_ref::rms_db;
use common::sweep::{BPS, Bench, Patch, Tape};

const FS: f64 = 48_000.0;
const TAU: f64 = 2.0 * std::f64::consts::PI;

/// The gate alone at LPG `a`, DECAY `d`, STRIKE `s`: `x` through it, its
/// level the routes' `level` (`None`: no route), a fresh note struck at 0
/// on `note`.
fn gate(a: f32, d: f32, s: f32, x: &[f32], level: Option<&[f32]>, note: u8) -> Vec<f32> {
    gate_hit(a, d, s, x, level, note, Velocity::MAX)
}

/// `gate`, struck at `hit`.
fn gate_hit(
    a: f32,
    d: f32,
    s: f32,
    x: &[f32],
    level: Option<&[f32]>,
    note: u8,
    hit: Velocity,
) -> Vec<f32> {
    let mut g = Lpg::NEW;
    g.strike(Onset::Fresh);
    let mut out = Vec::with_capacity(x.len());
    for b in 0..x.len().div_ceil(BLOCK_SIZE) {
        let at = |v: &[f32], i: usize| v.get(b * BLOCK_SIZE + i).copied().unwrap_or(0.0);
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| at(x, i));
        let lv: Option<[f32; BLOCK_SIZE]> = level.map(|l| core::array::from_fn(|i| at(l, i)));
        let drive = Drive {
            level: lv
                .as_ref()
                .map_or(Level::Open, |l| Level::Routes(l, Unit::new(1.0))),
            knobs: Knobs::of([a, d, s]),
            hit,
            note: MidiNote::new(note).unwrap(),
            fs: FS as f32,
        };
        g.block(&mut buf, &drive);
        out.extend_from_slice(&buf);
    }
    out.truncate(x.len());
    out
}

/// Plaits' `LPGEnvelope` and `LowPassGate` in f64, read off
/// `plaits/dsp/envelope.h`, `fx/low_pass_gate.h` and `voice.cc`: LEVEL
/// patched (`ProcessLP` on the compressed level) or, with `None`, a ping at
/// sample 0 on `note` (`ProcessPing`); `colour` Plaits' COLOR.
fn plaits(x: &[f32], level: Option<&[f32]>, decay: f64, colour: f64, note: f64) -> Vec<f64> {
    let semis = |s: f64| 2f64.powf(s / 12.0);
    let short = 200.0 * 12.0 / FS * semis(-96.0 * decay);
    let tail = 20.0 * 12.0 / FS * semis(-72.0 * decay + 12.0 * colour) - short;
    let attack = 440.0 / FS * semis(note - 69.0) * 12.0 * 2.0;
    let (mut v, mut prev, mut ramp_up) = (0.0f64, 0.0f64, true);
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    let mut y = vec![0.0; x.len()];
    for b in 0..x.len().div_ceil(12) {
        let i0 = b * 12;
        let input = match level {
            Some(l) => {
                let l = f64::from(l[i0]);
                1.3 * l / (0.3 + l.abs())
            }
            None => {
                if ramp_up {
                    v += attack;
                    if v >= 1.0 {
                        (v, ramp_up) = (1.0, false);
                    }
                }
                if ramp_up { v } else { 0.0 }
            }
        };
        let err = input - v;
        let s4 = v.powi(4);
        let t2 = (1.0 - v).powi(2);
        let c = if err > 0.0 {
            0.6
        } else {
            short + (1.0 - s4) * tail
        };
        v += c * err;
        let f = 0.003 + 0.3 * s4 + 0.04 * colour;
        let bleed = (t2 + (1.0 - t2) * colour) * colour * colour;
        let g = f * (std::f64::consts::PI + 3.736e-1 * std::f64::consts::PI.powi(3) * f * f);
        let r = 1.0 / 0.4;
        let h = 1.0 / (1.0 + r * g + g * g);
        let end = (i0 + 12).min(x.len());
        for (k, i) in (i0..end).enumerate() {
            let gm = prev + (v - prev) * (k + 1) as f64 / 12.0;
            let s = f64::from(x[i]) * gm;
            let hp = (s - r * s1 - g * s1 - s2) * h;
            let bp = g * hp + s1;
            s1 = g * hp + bp;
            let lp = g * bp + s2;
            s2 = g * bp + lp;
            y[i] = lp + (s - lp) * bleed;
        }
        prev = v;
    }
    y
}

fn null_db(y: &[f32], want: &[f64]) -> f32 {
    let w: Vec<f32> = want.iter().map(|&v| v as f32).collect();
    let err: Vec<f32> = y.iter().zip(&w).map(|(a, b)| a - b).collect();
    rms_db(&err) - rms_db(&w)
}

/// The lab's band-limited saw at `f` Hz, peak 0.5 (`lpg_gate::tone`).
fn tone_saw(f: f64, n: usize) -> Vec<f32> {
    let kmax = (20_000.0 / f) as usize;
    let mut y = vec![0.0f64; n];
    for k in 1..=kmax {
        let (a, wk) = (1.0 / k as f64, TAU * f * k as f64 / FS);
        for (i, v) in y.iter_mut().enumerate() {
            *v += a * (wk * i as f64).sin();
        }
    }
    let pk = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    y.iter().map(|v| (0.5 * v / pk) as f32).collect()
}

/// The lab's fixture (`lpg_gate::fixtures`, `lpg-circuit-lab`): its second
/// take, DECAY 0.2 at COLOR 0, a 110 Hz saw held 0.1 s, is LPG 1. The gate
/// nulls the lab's f64 port of Plaits there.
#[test]
fn lpg_at_one_is_the_labs_plaits_port() {
    let raw = include_bytes!("fixtures/lpg_plaits_v1.bin");
    let mut words = raw.as_chunks::<4>().0.iter().copied();
    let mut takes = Vec::new();
    while let Some(n) = words.next() {
        let n = u32::from_le_bytes(n) as usize;
        let d = f32::from_le_bytes(words.next().unwrap());
        let c = f32::from_le_bytes(words.next().unwrap());
        let y: Vec<f32> = (0..n)
            .map(|_| f32::from_le_bytes(words.next().unwrap()))
            .collect();
        takes.push((d, c, y));
    }
    assert_eq!(takes.len(), 3);
    let (d, c, want) = &takes[1];
    assert_eq!((*d, *c), (0.2, 0.0));
    let n = want.len();
    let x: Vec<f32> = (0..n)
        .map(|i| (0.5 * (2.0 * (i as f64 * 110.0 / FS).fract() - 1.0)) as f32)
        .collect();
    let held: Vec<f32> = (0..n).map(|i| if i < 4800 { 1.0 } else { 0.0 }).collect();
    let y = gate(1.0, *d, 0.0, &x, Some(&held), 60);
    let want: Vec<f64> = want.iter().map(|&v| f64::from(v)).collect();
    let null = null_db(&y, &want);
    println!("fixture: null {null:.1} dB");
    assert!(null < -90.0, "{null:.1} dB");
    // The f64 port here is the lab's on the same take.
    let ours = plaits(&x, Some(&held), 0.2, 0.0, 60.0);
    assert!(null_db(&y, &ours) < -90.0);
}

/// STRIKE 1 at LPG 1 is Plaits' ping (`ProcessPing`, LEVEL unpatched):
/// the gate nulls the f64 port across DECAYs and notes.
#[test]
fn strike_at_one_is_plaits_ping() {
    let n = 48_000;
    for (d, note) in [(0.3, 48), (0.5, 60), (0.8, 72), (0.1, 36)] {
        let x = tone_saw(220.0, n);
        let y = gate(1.0, d, 1.0, &x, None, note);
        let null = null_db(&y, &plaits(&x, None, f64::from(d), 0.0, f64::from(note)));
        println!("DECAY {d} note {note}: null {null:.1} dB");
        assert!(null < -90.0, "DECAY {d}: {null:.1} dB");
    }
}

// ============================================================ gate checks

/// SuperCollider's Env.perc(1 ms, 10 ms), curve −4: the lab's ping.
fn perc(n: usize) -> Vec<f32> {
    let (a, r, c) = ((0.001 * FS) as usize, 0.010 * FS, -4.0f64);
    let shape = |x: f64| (1.0 - (c * x).exp()) / (1.0 - c.exp());
    (0..n)
        .map(|i| {
            if i < a {
                shape(i as f64 / a as f64) as f32
            } else {
                let x = (i - a) as f64 / r;
                if x >= 1.0 {
                    0.0
                } else {
                    (1.0 - shape(x)) as f32
                }
            }
        })
        .collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// 5 ms RMS every 1 ms, dB (the lab's `env_db`).
fn env_db(y: &[f32]) -> Vec<f64> {
    let (w, hop) = (240, 48);
    (0..(y.len() - w) / hop)
        .map(|k| {
            let s = &y[k * hop..k * hop + w];
            db((s.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / w as f64).sqrt())
        })
        .collect()
}

/// The magnitude-weighted centroid of a Hann-windowed 10 ms at `at`,
/// zero-padded to 2048 (the lab's `centroid_win`).
fn centroid(y: &[f32], at: usize) -> f64 {
    let (n, m) = (480usize, 2048usize);
    let x: Vec<f64> = (0..n)
        .map(|i| (0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()) * f64::from(y[at + i]))
        .collect();
    let (mut num, mut den) = (0.0, 0.0);
    for k in 0..m / 2 {
        let w = TAU * k as f64 / m as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            re += v * (w * i as f64).cos();
            im -= v * (w * i as f64).sin();
        }
        let p = (re * re + im * im).sqrt();
        num += p * k as f64 * FS / m as f64;
        den += p;
    }
    num / den
}

/// Onset centroid, 50 ms centroid, ms to −20 and −40 dB re the loudest
/// 5 ms, and the level at 1 s (the lab's `gate/gate.md` checks).
fn checks(y: &[f32]) -> [f64; 5] {
    let e = env_db(y);
    let (kmax, pk) = e
        .iter()
        .copied()
        .enumerate()
        .fold((0, f64::MIN), |m, (k, v)| if v > m.1 { (k, v) } else { m });
    let cross = |d: f64| {
        e[kmax..]
            .iter()
            .position(|&v| v < pk - d)
            .map_or(f64::NAN, |k| (kmax + k) as f64)
    };
    [
        centroid(y, 48),
        centroid(y, 45 * 48),
        cross(20.0),
        cross(40.0),
        e[1000] - pk,
    ]
}

/// The gate checks on one 220 Hz saw hit at LPG 1, DECAY ½: the lab's
/// ping into the routes, and STRIKE 1 alone. Each against the f64 port's
/// own: within 1 % on brightness, 2 ms on the times, 0.3 dB at 1 s. A
/// strike opens bright and closes over about a second.
#[test]
fn the_gate_opens_bright_and_shuts_as_plaits() {
    let n = (1.6 * FS) as usize;
    let x = tone_saw(220.0, n);
    let p = perc(n);
    for (name, y, want) in [
        (
            "routes",
            gate(1.0, 0.5, 0.0, &x, Some(&p), 57),
            plaits(&x, Some(&p), 0.5, 0.0, 57.0),
        ),
        (
            "strike",
            gate(1.0, 0.5, 1.0, &x, None, 57),
            plaits(&x, None, 0.5, 0.0, 57.0),
        ),
    ] {
        let w: Vec<f32> = want.iter().map(|&v| v as f32).collect();
        let (c, r) = (checks(&y), checks(&w));
        println!(
            "{name}: onset {:.0} Hz, 50 ms {:.0} Hz, −20 dB {} ms, −40 dB {} ms, 1 s {:.1} dB",
            c[0], c[1], c[2], c[3], c[4]
        );
        assert!(
            (c[0] - r[0]).abs() < 0.01 * r[0],
            "{name} onset {c:?} {r:?}"
        );
        assert!((c[1] - r[1]).abs() < 0.01 * r[1], "{name} 50 ms");
        assert!((c[2] - r[2]).abs() <= 2.0, "{name} −20 dB");
        assert!((c[3] - r[3]).abs() <= 2.0, "{name} −40 dB");
        assert!((c[4] - r[4]).abs() < 0.3, "{name} 1 s");
        assert!(c[0] > 1500.0 && c[1] < c[0], "{name}: bright, then darker");
        assert!(
            c[3] > 200.0 && c[3] < 1600.0,
            "{name}: shuts in about a second"
        );
    }
}

/// LPG darkens as the gain falls, DECAY lengthens the fall, STRIKE adds
/// the ping to the routes.
#[test]
fn each_knob_does_what_it_says() {
    let n = (2.5 * FS) as usize;
    let x = tone_saw(220.0, n);
    let p = perc(n);
    // LPG: the same hit's 50 ms centroid falls as LPG rises.
    let c50 = |a: f32| centroid(&gate(a, 0.5, 0.0, &x, Some(&p), 57), 45 * 48);
    let cs = [0.0, 0.5, 1.0].map(c50);
    assert!(cs[0] > cs[1] && cs[1] > cs[2], "{cs:?}");
    // DECAY: the time to −40 dB grows.
    let t40 = |d: f32| checks(&gate(1.0, d, 0.0, &x, Some(&p), 57))[3];
    let ts = [0.1, 0.4, 0.7].map(t40);
    assert!(ts[0] < ts[1] && ts[1] < ts[2], "{ts:?}");
    // DECAY 0 at LPG 0: the gain is the routes' level, sample for sample.
    let y = gate(0.0, 0.0, 0.0, &x, Some(&p), 57);
    let vca: Vec<f32> = x.iter().zip(&p).map(|(a, b)| a * b).collect();
    let vca64: Vec<f64> = vca.iter().map(|&v| f64::from(v)).collect();
    let null = null_db(&y, &vca64);
    assert!(null < -100.0, "DECAY 0 against the VCA: {null:.1} dB");
    // STRIKE ½ on a held level of ½: the strike's ½ sums with the level
    // to 1 − ½·½, then falls back to the level. The level is no longer
    // × (1 − STRIKE) under the routes: STRIKE is an accent on it (LxD).
    let half = vec![0.5f32; n];
    let y = gate(0.0, 0.5, 0.5, &vec![1.0; n], Some(&half), 60);
    let peak = y[..4800].iter().fold(0.0f32, |m, v| m.max(*v));
    let held = y[n - 1];
    assert!((peak - 0.75).abs() < 0.01, "{peak}");
    assert!((held - 0.5).abs() < 0.01, "{held}");
}

/// A held level `l` struck at `s` and velocity `v` (LPG 0, DECAY 0.3, a
/// DC input): the gain, a sample at a time.
fn struck(l: f32, s: f32, v: u8) -> Vec<f32> {
    let n = 2 * FS as usize;
    let hit = Velocity::new(v).unwrap();
    gate_hit(0.0, 0.3, s, &vec![1.0; n], Some(&vec![l; n]), 60, hit)
}

/// A held note with sustain stays at the envelope's level after its
/// strike, at every STRIKE up to 1, and never falls below it.
#[test]
fn a_held_note_stays_at_its_envelope_after_the_strike() {
    for s in [0.25, 0.5, 1.0] {
        let y = struck(0.5, s, 127);
        let top = y
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
        let after = &y[top.0..];
        let low = after.iter().fold(1.0f32, |m, v| m.min(*v));
        let end = y[y.len() - 1];
        assert!((end - 0.5).abs() < 1e-3, "STRIKE {s}: ends at {end}");
        assert!(low > 0.5 - 1e-3, "STRIKE {s}: falls to {low}");
    }
}

/// The strike is an accent above the envelope: it peaks at the soft sum
/// `1 − (1 − l)(1 − STRIKE·vel)`, rising with STRIKE and velocity, over
/// the envelope's level `l`; at STRIKE 0 there is none.
#[test]
fn the_strike_is_an_accent_above_the_envelope() {
    let peak = |l: f32, s: f32, v: u8| struck(l, s, v).iter().fold(0.0f32, |m, v| m.max(*v));
    for l in [0.25, 0.5, 0.8] {
        for v in [127u8, 64] {
            let x = Velocity::new(v).unwrap().unit();
            let p = [0.0, 0.5, 1.0].map(|s| peak(l, s, v));
            assert!((p[0] - l).abs() < 1e-3, "level {l}: no accent at 0, {p:?}");
            for (k, s) in [(1, 0.5), (2, 1.0)] {
                let want = 1.0 - (1.0 - l) * (1.0 - s * x);
                assert!(
                    (p[k] - want).abs() < 0.01,
                    "level {l} vel {v} STRIKE {s}: {p:?}"
                );
            }
            assert!(p[0] < p[1] && p[1] < p[2], "{p:?}");
        }
    }
}

/// Nothing holding the gate open still pings: a routed envelope at 0
/// (sustain 0, or finished) is the no-route ping at STRIKE 1, to the
/// sample, and both shut.
#[test]
fn sustain_0_or_no_route_still_pings() {
    let n = FS as usize;
    let x = tone_saw(220.0, n);
    let ping = gate(1.0, 0.3, 1.0, &x, None, 60);
    let shut = gate(1.0, 0.3, 1.0, &x, Some(&vec![0.0; n]), 60);
    assert_eq!(ping, shut);
    assert!(ping[..4800].iter().any(|v| v.abs() > 0.05), "it pings");
    assert!(ping[n - 4800..].iter().all(|v| v.abs() < 1e-3), "and shuts");
}

/// STRIKE 1 no longer ignores the envelope: a held VA INIT note under the
/// gate at STRIKE 1 sounds on at STRIKE 0's level (ENV 1's sustain), its
/// voice held, where it once pinged and ended.
#[test]
fn strike_at_one_keeps_the_envelope() {
    let tail = |k: f32| {
        let mut b = bench(gated(Sound::init(EngineType::Va), 0.5, 0.4, k));
        let t = hold(&mut b, None, BPS * 4);
        assert_eq!(b.sounding(), 1, "STRIKE {k}: the held note sounds on");
        rms_db(t.pair(0, BPS * 3, BPS * 4))
    };
    let (none, full) = (tail(0.0), tail(1.0));
    assert!(none > -60.0, "{none}");
    assert!((full - none).abs() < 0.1, "STRIKE 1 {full} vs 0 {none} dB");
}

/// #798: ALGO's carrier envelopes hold the gate like VA's key: a held note
/// under LPG 1, DECAY 0, STRIKE 1 sounds on instead of pinging shut.
#[test]
fn algo_held_note_holds_the_gate_at_strike_one() {
    let mut b = bench(gated(saw_sound(), 1.0, 0.0, 1.0));
    let t = hold(&mut b, None, BPS * 4);
    assert_eq!(b.sounding(), 1, "the held note sounds on");
    let tail = rms_db(t.pair(0, BPS * 3, BPS * 4));
    assert!(tail > -60.0, "{tail}");
}

/// #798, released: the ALGO note held under STRIKE 1 ends by its carriers'
/// envelopes after note-off, and the voice frees.
#[test]
fn algo_released_note_frees_through_its_carriers() {
    let mut b = bench(gated(saw_sound(), 1.0, 0.0, 1.0));
    let off = BPS * 2;
    let t = hold(&mut b, Some(off), BPS * 12);
    assert!(rms_db(t.pair(0, BPS, BPS * 2)) > -60.0, "held note sounds");
    assert!(b.all_free(), "freed after the key");
    assert!(t.freed(off).is_some());
}

/// #806: with no route the ALGO gate follows its carrier envelopes. On a
/// slow attack STRIKE plucks ahead of the envelope (louder at the start
/// the higher it goes), and DECAY lengthens the pluck.
#[test]
fn algo_strike_plucks_over_the_carrier_level() {
    let run = |d: f32, k: f32| {
        let mut s = saw_sound();
        for o in &mut s.params.algo.ops {
            (o.ar, o.d1l) = (8, 15);
        }
        let mut b = bench(gated(s, 0.5, d, k));
        hold(&mut b, None, BPS)
    };
    let early = |d: f32, k: f32, a: usize, z: usize| rms_db(run(d, k).pair(0, a, z));
    let (e0, e5, e1) = (
        early(0.3, 0.0, 0, BPS / 20),
        early(0.3, 0.5, 0, BPS / 20),
        early(0.3, 1.0, 0, BPS / 20),
    );
    assert!(e5 > e0 + 1.0 && e1 > e5 + 1.0, "{e0} {e5} {e1}");
    let (short, long) = (
        early(0.1, 1.0, BPS / 10, BPS / 5),
        early(0.9, 1.0, BPS / 10, BPS / 5),
    );
    assert!(long > short + 1.0, "{short} {long}");
}

// ============================================================ the voice

fn saw_sound() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = AlgoParams::single(WaveId::SAW);
    s
}

fn gated(mut s: Sound, a: f32, d: f32, k: f32) -> Sound {
    s.params.out.gate = [a, d, k];
    s
}

fn bench(s: Sound) -> Bench {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::OFF);
    b.edit_sound(|x| *x = s);
    b
}

/// `blocks` with a note held from 0 to `off` (`None`: held throughout).
fn hold(b: &mut Bench, off: Option<usize>, blocks: usize) -> Tape {
    let mut t = Tape::default();
    for i in 0..blocks {
        if i == 0 {
            b.note(48, Some(100));
        }
        if Some(i) == off {
            b.note(48, None);
        }
        b.render(&mut t);
    }
    t
}

/// VA INIT plays ENV 1 into VCA: under the gate that route drives it and
/// the VCA leaves the gain to it, so nothing is gated twice. At LPG 1/128,
/// DECAY 0 the gate is a VCA: it plays INIT's level within 0.2 dB, and
/// ENV 1's SUSTAIN ½ is 6 dB under SUSTAIN 1, not 12.
#[test]
fn the_gate_never_gates_twice() {
    let held = |sustain: f32, on: bool| {
        let mut s = Sound::init(EngineType::Va);
        s.params.envelopes[0].sustain = sustain;
        if on {
            s.params.out.gate[0] = 1.0 / 128.0;
        }
        let mut b = bench(s);
        let t = hold(&mut b, None, BPS * 2);
        rms_db(t.pair(0, BPS, BPS * 2))
    };
    let (plain, gate) = (held(1.0, false), held(1.0, true));
    assert!((plain - gate).abs() < 0.2, "{plain} vs {gate}");
    let d = held(0.5, true) - gate;
    assert!((d + 6.02).abs() < 0.3, "{d:.2} dB");
}

/// A voice under the gate ends when it shuts. A struck MODAL note (STRIKE
/// 1, no route into VCA) frees once its ping has shut, the key still held;
/// an ALGO one holds on its carriers' envelopes (#798);
/// VA INIT's note rings past ENV 1's release on DECAY's tail, then frees;
/// a VA note with no route falls from its key up and frees.
#[test]
fn voices_end_when_the_gate_shuts() {
    let mut b = bench(gated(Sound::init(EngineType::Modal), 1.0, 0.3, 1.0));
    let t = hold(&mut b, None, BPS * 8);
    assert!(t.rms(0, 0, BPS / 10) > 1e-3, "the ping is heard");
    assert_eq!(b.sounding(), 0, "the shut gate ended it, the key held");
    assert!(t.rms(0, BPS * 7, BPS * 8) < 1e-5);
    let mut c = bench(saw_sound());
    hold(&mut c, None, BPS * 8);
    assert_eq!(c.sounding(), 1, "the plain VCA's held saw sounds on");
    let freed = |s: Sound| {
        let mut b = bench(s);
        let off = BPS / 2;
        let t = hold(&mut b, Some(off), BPS * 12);
        assert!(b.all_free(), "freed");
        t.freed(off).expect("freed after the key")
    };
    let va = Sound::init(EngineType::Va);
    let (plain, gate) = (freed(va.clone()), freed(gated(va.clone(), 0.5, 0.5, 0.0)));
    assert!(gate > plain, "gate {gate} vs VCA {plain}");
    let mut bare = va;
    bare.mod_state
        .clear_fixed(ModSource::Env1.index(), FixedCol::Vca);
    let (plain, gate) = (freed(bare.clone()), freed(gated(bare, 0.5, 0.5, 0.0)));
    assert!(gate >= plain, "gate {gate} vs key {plain}");
}

/// Each knob swept from 0 to 1 over a second on a held VA INIT note never
/// steps: no edge in the output's second difference over the sweep's
/// steady neighbours (owner rule), and turning the gate on from 0 is no
/// click.
#[test]
fn the_knobs_never_zipper() {
    for which in 0..3 {
        let mut b = bench(Sound::init(EngineType::Va));
        let mut t = Tape::default();
        b.note(48, Some(100));
        for _ in 0..BPS / 2 {
            b.render(&mut t);
        }
        for k in 0..BPS {
            let v = (k + 1) as f32 / BPS as f32;
            b.turn_sound(|s| match which {
                0 => s.params.out.gate[0] = v,
                1 => s.params.out.gate[1] = v,
                _ => s.params.out.gate[2] = v,
            });
            b.render(&mut t);
        }
        for _ in 0..BPS / 2 {
            b.render(&mut t);
        }
        assert!(t.finite());
        let s = BPS / 2;
        assert!(
            t.click(0, s, (s / 2, s + 8), true).is_none(),
            "knob {which}"
        );
    }
}

/// Each knob swept a block at a time over a second on a 110 Hz sine, with
/// the routes' level at ½ and a strike: no energy in the fourth difference
/// beyond the sine's own (owner rule: never steppy).
#[test]
fn the_gate_sweeps_smoothly() {
    let n = 48_000;
    let half = [0.5f32; BLOCK_SIZE];
    for which in 0..3 {
        let mut g = Lpg::NEW;
        g.strike(Onset::Fresh);
        let mut y = Vec::new();
        for b in 0..n / BLOCK_SIZE {
            let t = b as f32 * BLOCK_SIZE as f32 / n as f32;
            let mut k = [0.5f32, 0.5, 0.3];
            k[which] = t;
            let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                let j = (b * BLOCK_SIZE + i) as f32;
                0.3 * (core::f32::consts::TAU * 110.0 * j / FS as f32).sin()
            });
            let d = Drive {
                level: Level::Routes(&half, Unit::new(1.0)),
                knobs: Knobs::of(k),
                hit: Velocity::MAX,
                note: MidiNote::new(60).unwrap(),
                fs: FS as f32,
            };
            g.block(&mut buf, &d);
            y.extend_from_slice(&buf);
        }
        let hi: Vec<f32> = y[9600..]
            .windows(5)
            .map(|w| w[0] - 4.0 * w[1] + 6.0 * w[2] - 4.0 * w[3] + w[4])
            .collect();
        let r = rms_db(&hi) - rms_db(&y[9600..]);
        println!("sweep {which}: {r:.1} dB");
        assert!(r < -70.0, "sweep {which}: {r:.1} dB");
    }
}

/// A NaN or infinite block in, or a NaN level: the block after is finite
/// and the gate plays on.
#[test]
fn the_gate_recovers_from_nan() {
    let mut g = Lpg::NEW;
    let one = [1.0f32; BLOCK_SIZE];
    let nan = [f32::NAN; BLOCK_SIZE];
    let run = |g: &mut Lpg, x: f32, level: &[f32; BLOCK_SIZE]| {
        let mut buf = [x; BLOCK_SIZE];
        let d = Drive {
            level: Level::Routes(level, Unit::new(1.0)),
            knobs: Knobs::of([0.7, 0.5, 0.3]),
            hit: Velocity::MAX,
            note: MidiNote::new(60).unwrap(),
            fs: FS as f32,
        };
        g.block(&mut buf, &d);
        buf
    };
    for _ in 0..4 {
        run(&mut g, 0.3, &one);
    }
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        run(&mut g, bad, &one);
        for _ in 0..2 {
            assert!(
                run(&mut g, 0.3, &one).iter().all(|v| v.is_finite()),
                "{bad}"
            );
        }
    }
    let y = run(&mut g, 0.3, &nan);
    assert!(y.iter().all(|v| v.is_finite()), "a NaN level");
    for _ in 0..4 {
        run(&mut g, 0.3, &one);
    }
    assert!(run(&mut g, 0.3, &one).iter().any(|v| *v != 0.0), "plays on");
}

// ============================================================ Sounds

/// An old Sound (no LPG, DECAY or STRIKE record) loads with all three at
/// 0, a plain VCA; a Sound with them round-trips; INIT stores none of
/// them; the sends keep their idents and ids.
#[test]
fn old_sounds_load_as_plain_vcas() {
    use chimera_core::block::{Block, DiskCode};
    use chimera_core::params::OutParams;
    use chimera_core::storage::RecordTag;
    let engine = (RecordTag::Engine, vec![EngineType::Va.disk_code()]);
    let old = decode(&file(&[engine])).unwrap();
    assert!(!old.params.out.gated());
    let mut s = Sound::init(EngineType::Va);
    gated_into(&mut s);
    let d = decode(&encode(&s)).unwrap();
    assert!(d.bits_eq(&s));
    assert_eq!(d.params.out.gate, [0.75, 0.5, 0.25]);
    let init = encode(&Sound::init(EngineType::Va));
    assert!(init.len() < encode(&s).len());
    let specs = s.params.out.specs();
    for (id, ident) in [
        (4, "SEND1"),
        (5, "SEND2"),
        (6, "SEND3"),
        (7, "LPG"),
        (8, "DECAY"),
        (9, "STRIKE"),
    ] {
        let sp = specs.iter().find(|p| p.id.0 == id).unwrap();
        assert_eq!(sp.ident, ident);
    }
    for id in [OutParams::LPG, OutParams::DECAY, OutParams::STRIKE] {
        let sp = specs.iter().find(|p| p.id == id).unwrap();
        assert!(sp.modulatable && sp.stored && !sp.patching, "{}", sp.label);
        assert_eq!(sp.default, 0.0);
    }
}

fn gated_into(s: &mut Sound) {
    s.params.out.gate = [0.75, 0.5, 0.25];
}

/// Every strike is Plaits' ping at the note's velocity: on a voice at rest
/// (MODAL with no route, VA INIT's ENV 1) as on a held one, a soft note
/// strikes softer.
#[test]
fn a_fresh_voice_strikes_at_its_velocity() {
    for s in [Sound::init(EngineType::Modal), Sound::init(EngineType::Va)] {
        let first = |vel: u8| {
            let mut b = bench(gated(s.clone(), 0.5, 0.5, 1.0));
            let mut t = Tape::default();
            b.note(48, Some(vel));
            for _ in 0..BPS / 4 {
                b.render(&mut t);
            }
            t.rms(0, 0, BPS / 10)
        };
        let (loud, soft) = (first(127), first(30));
        assert!(soft < 0.5 * loud, "{:?}: {soft} vs {loud}", s.engine());
    }
}

/// A softer strike on a ringing gate never drops it to the new peak: the
/// fall takes it from where it is.
#[test]
fn a_softer_strike_never_drops_the_gate() {
    let open = [1.0f32; BLOCK_SIZE];
    let drive = |vel: u8| Drive {
        level: Level::Routes(&open, Unit::new(1.0)),
        knobs: Knobs::of([0.5, 0.5, 1.0]),
        hit: Velocity::new(vel).unwrap(),
        note: MidiNote::new(60).unwrap(),
        fs: FS as f32,
    };
    let mut g = Lpg::NEW;
    let mut buf = [0.3f32; BLOCK_SIZE];
    g.strike(Onset::Fresh);
    for _ in 0..20 {
        g.block(&mut buf, &drive(127));
    }
    let was = g.level().get();
    g.strike(Onset::Again);
    g.block(&mut buf, &drive(20));
    assert!(was > 0.5, "{was}");
    assert!(
        g.level().get() > 0.95 * was,
        "{} from {was}",
        g.level().get()
    );
}
