// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SMEAR's depth and RATE's law (ADR 0121): at SMEAR 1 every TYPE's held
//! tone moves by a chorus's few cents at the default RATE and by no more
//! than 15 at RATE's top; SMEAR 0 sounds the same at any RATE.
//!
//! The pitch a held tone plays is judged by its power-weighted centroid
//! within ±60 Hz, 100 ms Hann windows a 50 ms hop: a reverb's output is a
//! mixture of tones each shifted its own way, which zero crossings read
//! as phase slips, the centroid as the mixture's mean shift. Four probes
//! (350, 700, 1 000 and 1 400 Hz: a smeared allpass detunes most near its
//! own resonances) pool their windows over 60 s, TIME 1, from 2 s; the
//! deviation is their 95th percentile, in cents from the probe.

mod common;

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, RATE_DEFAULT, ReverbParams, ReverbType};
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{Noise, check_knobs_ease, knobs_120, run, tone};
use std::cell::RefCell;
use std::f64::consts::TAU;

/// `kind` on its starting knobs, TIME 1 (the longest tail), MIX 1.
fn params(kind: ReverbType, smear: f32, rate: f32) -> ReverbParams {
    let mut p = ReverbParams::default().with_type(kind, &KnobMemory::new());
    (p.time, p.mix, p.smear, p.rate) = (1.0, 1.0, smear, rate);
    p
}

fn render(p: ReverbParams, seconds: f32, input: &mut dyn FnMut(usize) -> f32) -> Vec<f32> {
    let mut rv = Box::new(Reverb::new());
    let (l, _) = run(
        &mut |s| {
            let mut out = Stereo::SILENT;
            rv.process(s, &p, p.mix, Tempo::DEFAULT, SR, &mut out);
            (out.l, out.r)
        },
        input,
        seconds,
    );
    l
}

/// Each 100 ms Hann window's power-weighted centroid over `f0` ± 60 Hz
/// (1 Hz steps), a 50 ms hop, in cents from `f0`.
fn centroid_cents(x: &[f32], f0: f64) -> Vec<f32> {
    let n = SR as usize / 10;
    let w: Vec<f64> = (0..n)
        .map(|k| 0.5 - 0.5 * (TAU * k as f64 / n as f64).cos())
        .collect();
    let rot: Vec<(f64, f64, f64)> = (-60..=60)
        .map(|d| {
            let f = f0 + d as f64;
            let a = TAU * f / SR as f64;
            (f, a.cos(), a.sin())
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= x.len() {
        let xw: Vec<f64> = (0..n).map(|k| x[i + k] as f64 * w[k]).collect();
        let (mut num, mut den) = (0.0, 0.0);
        for &(f, c, s) in &rot {
            let (mut cr, mut ci, mut re, mut im) = (1.0f64, 0.0f64, 0.0, 0.0);
            for &v in &xw {
                re += v * cr;
                im += v * ci;
                (cr, ci) = (cr * c - ci * s, cr * s + ci * c);
            }
            let pw = re * re + im * im;
            num += f * pw;
            den += pw;
        }
        out.push((1200.0 * (num / den / f0).log2()) as f32);
        i += n / 2;
    }
    out
}

const PROBES: [f32; 4] = [350.0, 700.0, 1_000.0, 1_400.0];

/// The held probes' pitch deviation at `p` (the module's judge): the 95th
/// percentile of every window's |cents|.
fn deviation_p95(p: ReverbParams) -> f32 {
    let mut all: Vec<f32> = PROBES
        .iter()
        .flat_map(|&f| {
            let t = tone(f, 0.25);
            let x = render(p, 62.0, &mut |i| t(i));
            centroid_cents(&x[2 * SR as usize..], f as f64)
        })
        .map(f32::abs)
        .collect();
    all.sort_by(f32::total_cmp);
    all[all.len() * 95 / 100]
}

/// At SMEAR 1 and the default RATE (0.95 Hz) the held probes move by at
/// least 5 cents (P95), a chorus; at RATE's top (2 Hz) by at most 15,
/// never seasick. Before ADR 0121 (RATE 0.13–0.45 Hz by default, SMEAR's
/// old depths) QUAD read 4.8, ROOM 1.8, BLOOM 3.3, GATE 4.1 and REVERSE
/// 2.7 cents at their default RATE, and HALL 22 and GATE 15.4 at their
/// top: each TYPE failed one of the two.
fn check_depth(kind: ReverbType) {
    assert!((knobs_120(&params(kind, 1.0, RATE_DEFAULT)).rate_hz - 0.946).abs() < 1e-3);
    let at_default = deviation_p95(params(kind, 1.0, RATE_DEFAULT));
    let at_top = deviation_p95(params(kind, 1.0, 1.0));
    println!("{kind:?}: {at_default:.1} cents at the default RATE, {at_top:.1} at the top");
    assert!(at_default >= 5.0, "{kind:?}: {at_default} cents, still");
    assert!(at_top <= 15.0, "{kind:?}: {at_top} cents, seasick");
    assert!(at_top >= at_default, "{kind:?}: RATE's top moves less");
}

#[test]
fn quad_smear_moves_a_chorus_and_stays_in_tune() {
    check_depth(ReverbType::Quad);
}

#[test]
fn room_smear_moves_a_chorus_and_stays_in_tune() {
    check_depth(ReverbType::Room);
}

#[test]
fn hall_smear_moves_a_chorus_and_stays_in_tune() {
    check_depth(ReverbType::Hall);
}

#[test]
fn bloom_smear_moves_a_chorus_and_stays_in_tune() {
    check_depth(ReverbType::Bloom);
}

#[test]
fn gate_smear_moves_a_chorus_and_stays_in_tune() {
    check_depth(ReverbType::Gate);
}

#[test]
fn reverse_smear_moves_a_chorus_and_stays_in_tune() {
    check_depth(ReverbType::Reverse);
}

/// The share of the power within 40 Hz of the 1 kHz probe's peak that
/// lies more than 1.5 Hz off it, dB: a held tone's spread.
fn spread_db(p: ReverbParams) -> f32 {
    let t = tone(1_000.0, 0.25);
    let x = render(p, 9.5, &mut |i| t(i));
    let n = 1 << 17;
    let pw = common::sweep::power(&x[3 * SR as usize / 2..], n);
    let hz = SR as f32 / n as f32;
    let k = (1_000.0 / hz) as usize;
    let peak = (k - 100..k + 100)
        .max_by(|&a, &b| pw[a].total_cmp(&pw[b]))
        .unwrap();
    let (a, b) = ((1.5 / hz) as usize, (40.0 / hz) as usize);
    let all: f64 = pw[peak - b..=peak + b].iter().sum();
    let side: f64 = (a..=b).map(|d| pw[peak - d] + pw[peak + d]).sum();
    10.0 * (side / all).log10() as f32
}

/// SMEAR 1 at the default RATE spreads a held 1 kHz tone on every TYPE:
/// over a tenth of its power lies off the peak, where SMEAR 0 leaves
/// under 1 %. Before ADR 0121 every TYPE failed it: QUAD −18.5 dB, ROOM
/// −17.4, HALL −16.5, BLOOM −20.1, GATE −10.9, REVERSE −14.7.
#[test]
fn smear_spreads_a_held_tone_on_every_type() {
    for kind in ReverbType::ALL {
        let (still, smeared) = (
            spread_db(params(kind, 0.0, RATE_DEFAULT)),
            spread_db(params(kind, 1.0, RATE_DEFAULT)),
        );
        println!("{kind:?}: {still:.1} dB off the peak at SMEAR 0, {smeared:.1} at 1");
        assert!(smeared >= -10.0, "{kind:?}: {smeared} dB");
        assert!(still <= -20.0, "{kind:?} at SMEAR 0: {still} dB");
    }
}

/// SMEAR 0 is the MidiVerb II preset at any RATE: RATE 0 and RATE 1 render
/// a noise burst's tail bit for bit alike on every TYPE.
#[test]
fn smear_0_is_deaf_to_rate() {
    for kind in ReverbType::ALL {
        let burst = |rate: f32| {
            let mut n = Noise(0x5eed);
            render(params(kind, 0.0, rate), 3.0, &mut |i| {
                if i < SR as usize / 4 {
                    0.5 * n.next()
                } else {
                    0.0
                }
            })
        };
        let (slow, fast) = (burst(0.0), burst(1.0));
        assert!(
            slow.iter()
                .zip(&fast)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{kind:?}: RATE moved SMEAR 0"
        );
    }
}

/// SMEAR then RATE stepped 0 → 1 mid-tail on QUAD (the other TYPEs' knob
/// ease tests step both): no click.
#[test]
fn quad_smear_and_rate_steps_do_not_click() {
    let p = params(ReverbType::Quad, 0.0, 0.0);
    let (rv, pc) = (RefCell::new(Box::new(Reverb::new())), RefCell::new(p));
    let mut render = |send: &[f32; BLOCK_SIZE]| {
        let p = *pc.borrow();
        let mut out = Stereo::SILENT;
        rv.borrow_mut()
            .process(send, &p, p.mix, Tempo::DEFAULT, SR, &mut out);
        (out.l, out.r)
    };
    let ids = [ReverbParams::SMEAR, ReverbParams::RATE];
    check_knobs_ease(&mut render, &ids, &mut |id: ParamId, v| {
        pc.borrow_mut().write(id, v)
    });
}

/// The takes the ledger keeps (`SMEAR_TAKES` names its directory): per
/// TYPE, a 220/277/330 Hz sine chord held 2 s of 4, TIME 1, wet, at SMEAR 0,
/// SMEAR 1 at the default RATE and SMEAR 1 at RATE's top, each TYPE's three
/// at one gain (its loudest to −3 dBFS), 48 kHz 16-bit stereo WAV.
#[test]
#[ignore]
fn smear_takes() {
    let Ok(dir) = std::env::var("SMEAR_TAKES") else {
        return;
    };
    for kind in ReverbType::ALL {
        let takes: Vec<(&str, Vec<f32>, Vec<f32>)> = [
            ("smear0", 0.0, RATE_DEFAULT),
            ("smear1-rate-default", 1.0, RATE_DEFAULT),
            ("smear1-rate-max", 1.0, 1.0),
        ]
        .into_iter()
        .map(|(name, smear, rate)| {
            let p = params(kind, smear, rate);
            let mut rv = Box::new(Reverb::new());
            let (t1, t2, t3) = (tone(220.0, 0.15), tone(277.2, 0.15), tone(329.6, 0.15));
            let (l, r) = run(
                &mut |s| {
                    let mut out = Stereo::SILENT;
                    rv.process(s, &p, p.mix, Tempo::DEFAULT, SR, &mut out);
                    (out.l, out.r)
                },
                &mut |i| {
                    if i < 2 * SR as usize {
                        t1(i) + t2(i) + t3(i)
                    } else {
                        0.0
                    }
                },
                4.0,
            );
            (name, l, r)
        })
        .collect();
        let peak = takes
            .iter()
            .flat_map(|t| t.1.iter().chain(&t.2))
            .fold(0f32, |a, &b| a.max(b.abs()));
        let gain = 0.708 / peak;
        for (name, l, r) in takes {
            let n = (l.len() * 4) as u32;
            let mut b = Vec::new();
            b.extend(b"RIFF");
            b.extend((36 + n).to_le_bytes());
            b.extend(b"WAVEfmt ");
            b.extend(16u32.to_le_bytes());
            b.extend(1u16.to_le_bytes());
            b.extend(2u16.to_le_bytes());
            b.extend(SR.to_le_bytes());
            b.extend((SR * 4).to_le_bytes());
            b.extend(4u16.to_le_bytes());
            b.extend(16u16.to_le_bytes());
            b.extend(b"data");
            b.extend(n.to_le_bytes());
            for (x, y) in l.iter().zip(&r) {
                b.extend(((x * gain * 32767.0) as i16).to_le_bytes());
                b.extend(((y * gain * 32767.0) as i16).to_le_bytes());
            }
            let kind_name = ReverbType::NAMES[kind.index()];
            std::fs::write(format!("{dir}/{kind_name}-{name}.wav"), b).unwrap();
        }
    }
}
