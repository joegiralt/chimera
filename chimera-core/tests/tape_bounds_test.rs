// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Review Focus 3: whatever the params, source and `held`, no model reads
//! outside the row (`SourceView::strict` panics on a tap past it) and every
//! head stays within a clamp's step of it. And whatever changes mid-note,
//! every machine's output on a sine stays finite and near the sine's peak.

// TAPE's DSP as bank 1 builds it: under `bank2` the engine is the door's
// proxy (tape_proxy_test), and none of this links (#811).
#![cfg(not(feature = "bank2"))]

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::dsp::tape_engine::Tron;
use chimera_core::dsp::tape_engine::params::Drift;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape, SourceView};
use chimera_core::dsp::tape_engine::wave;
use chimera_core::dsp::tape_engine::{
    Direction, INC_MAX, Retrig, SliceCode, StretchType, TapeEngine, TapeEngineParams, TapeModel,
    Wow,
};
use chimera_core::storage::twelve::pack12;
use chimera_hal::BLOCK_SIZE;
use common::XorShift;
use common::tape::{held_plain, held_transwave};

/// The models these runs play (TRANSWAVE on a `Plain` is silent).
/// Every machine; START and END are drawn in either order.
const MODELS: [TapeModel; 4] = TapeModel::ALL;
const MODES: [Direction; 4] = Direction::ALL;
const TRONS: [Tron; 2] = [Tron::Off, Tron::On];

const MAX_FRAMES: usize = 100_000;

fn unit(rng: &mut XorShift) -> f32 {
    rng.next_u32() as f32 / u32::MAX as f32
}

#[test]
fn reads_stay_in_row() {
    let mut rng = XorShift(0x7A9E_0301);
    let s: Vec<i16> = (0..MAX_FRAMES)
        .map(|_| ((rng.next_u32() & 0xFFF) as i16) << 4 >> 4)
        .collect();
    let mut packed = vec![0u8; 3 * MAX_FRAMES / 2];
    pack12(&s, &mut packed);
    let rates = [4_000u16, 8_000, 24_000, 48_000];
    for run in 0..2_000 {
        let frames = 64 + rng.below(MAX_FRAMES - 64);
        let rate = rates[rng.below(rates.len())];
        let d = Descriptor::whole(Shape::Plain, 1, rate).expect("a descriptor");
        let mut held = d.to_bytes().to_vec();
        held.extend_from_slice(&packed[..3 * frames.div_ceil(2)]);
        // Held cut short, as a head-only hold is: the shortest source at
        // least (`MIN_SOURCE_FRAMES`, 64).
        let keep = DESC_LEN + 96 + rng.below(held.len() - DESC_LEN - 95);
        held.truncate(keep);
        let v = SourceView::strict(&held).unwrap();
        let p = TapeEngineParams {
            machine: TapeMachine::of(MODELS[rng.below(MODELS.len())]),
            direction: MODES[rng.below(4)],
            tron: TRONS[rng.below(2)],
            drift: Drift::of(unit(&mut rng)),
            start: unit(&mut rng),
            end: unit(&mut rng),
            position: unit(&mut rng),
            motion: 2.0 * unit(&mut rng) - 1.0,
            tape: unit(&mut rng),
            age: unit(&mut rng),
            size: unit(&mut rng),
            xfade: 0.5 * unit(&mut rng),
            drag: unit(&mut rng),
            kind: [StretchType::Mono, StretchType::Poly, StretchType::Grain][rng.below(3)],
            root: 12_700.0 * unit(&mut rng),
            period: [0.0, 1.0 + 60.0 * unit(&mut rng), 1_000.0 * unit(&mut rng)][rng.below(3)],
            slice: SliceCode::new(rng.below(18) as u8),
            wow: [Wow::Capstan, Wow::Warble][rng.below(2)],
            retrig: Retrig::Stack,
            ..TapeEngineParams::default()
        };
        let mut slot = MaybeUninit::uninit();
        let e = TapeEngine::init_in_place(&mut slot);
        e.set_pitch(4_800.0 * unit(&mut rng) - 2_400.0);
        e.note_on(rng.below(128) as u8, &p, &v, 48_000);
        let mut b = [0.0; BLOCK_SIZE];
        let reach = v.frames() + INC_MAX as u32;
        assert!(p.model() != TapeModel::Transwave || b.iter().all(|&y| y == 0.0));
        for _ in 0..4 {
            e.render(&mut b, &p, &v, 48_000);
            assert!(b.iter().all(|y| y.is_finite()), "run {run}: {p:?}");
            let pr = e.probe();
            assert!(
                pr.head <= reach,
                "run {run}: head {} of {}",
                pr.head,
                v.frames()
            );
            assert!(pr.other.is_none_or(|o| o <= reach), "run {run}: {pr:?}");
        }
    }
}

/// TRANSWAVE on random transwaves, whole or cut short (then silent): every
/// frame and level it reads lies inside the row (`read_cycle` panics on a
/// frame past it when strict), and the output stays finite.
#[test]
fn transwave_reads_stay_in_row() {
    let mut rng = XorShift(0x7A9E_0702);
    for run in 0..300 {
        let top = [512u32, 1_024][rng.below(2)];
        let count = 1 + rng.below(32) as u8;
        let n = count as usize * wave::frame_len(top) as usize;
        let s: Vec<i16> = (0..n)
            .map(|_| ((rng.next_u32() & 0xFFF) as i16) << 4 >> 4)
            .collect();
        let d = Descriptor::whole(Shape::Transwave, count, 48_000).expect("a descriptor");
        let mut held = d.to_bytes().to_vec();
        held.resize(DESC_LEN + 3 * n / 2, 0);
        pack12(&s, &mut held[DESC_LEN..]);
        if rng.below(4) == 0 {
            let keep = DESC_LEN + 3 + rng.below(held.len() - DESC_LEN - 3);
            held.truncate(keep);
        }
        let v = SourceView::strict(&held).unwrap();
        let p = TapeEngineParams {
            machine: TapeMachine::Wave,
            position: unit(&mut rng),
            motion: 2.0 * unit(&mut rng) - 1.0,
            size: unit(&mut rng),
            ..TapeEngineParams::default()
        };
        let mut slot = MaybeUninit::uninit();
        let e = TapeEngine::init_in_place(&mut slot);
        e.set_pitch(4_800.0 * unit(&mut rng) - 2_400.0);
        e.note_on(rng.below(128) as u8, &p, &v, 48_000);
        let mut b = [0.0; BLOCK_SIZE];
        for k in 0..8 {
            // A pitch route anywhere in ±24 st, block by block.
            e.set_pitch(4_800.0 * unit(&mut rng) - 2_400.0);
            e.render(&mut b, &p, &v, 48_000);
            assert!(b.iter().all(|y| y.is_finite()), "run {run}, block {k}");
            let pr = e.probe();
            assert!(pr.head < count as u32, "run {run}: {pr:?}");
            assert!(
                pr.other.is_some_and(|o| o < count as u32),
                "run {run}: {pr:?}"
            );
        }
    }
}

/// SCRUB at a crawl (an 8 kHz source, a low note under a high ROOT: `inc`
/// near 10⁻⁴) with MOTION flipping sign around ±10⁻⁷: a reversal lands
/// just after a wrap's fade began, at a share of 0, with the fade's way
/// back under 2⁻²⁴ of a frame. Its slope was 0/0, and the note NaN.
#[test]
fn scrub_reversing_at_a_crawl_stays_finite() {
    let mut rng = XorShift(3);
    for run in 0..40 {
        let frames = 64 + rng.below(2_000);
        let s: Vec<f32> = (0..frames)
            .map(|i| 0.7 * libm::sinf(i as f32 * 0.3))
            .collect();
        let held = held_plain(&s, [4_000, 8_000][rng.below(2)]);
        let v = SourceView::strict(&held).unwrap();
        let mut p = TapeEngineParams {
            machine: TapeMachine::Scrub,
            size: unit(&mut rng),
            xfade: 0.5 * unit(&mut rng),
            position: unit(&mut rng),
            drag: unit(&mut rng),
            root: 12_700.0,
            motion: 1e-6,
            ..TapeEngineParams::default()
        };
        let mut slot = MaybeUninit::uninit();
        let e = TapeEngine::init_in_place(&mut slot);
        e.without_transport();
        e.set_pitch(-4_800.0 * unit(&mut rng));
        e.note_on(rng.below(12) as u8, &p, &v, 48_000);
        let mut b = [0.0; BLOCK_SIZE];
        for k in 0..400 {
            match rng.below(6) {
                0 => p.motion = -p.motion,
                1 => p.motion = [1e-6, -1e-6, 1e-7, -1e-7, 0.5, -0.5][rng.below(6)],
                2 => p.position = unit(&mut rng),
                3 => {
                    p.size = unit(&mut rng);
                    p.xfade = 0.5 * unit(&mut rng);
                }
                _ => {}
            }
            e.render(&mut b, &p, &v, 48_000);
            assert!(b.iter().all(|y| y.is_finite()), "run {run}, block {k}");
        }
    }
}

/// A param in [0, 1], at an edge a third of the time.
fn edge(rng: &mut XorShift) -> f32 {
    match rng.below(9) {
        0 => 0.0,
        1 => 1.0,
        2 => 0.5,
        3 => f32::EPSILON,
        4 => 1.0 - f32::EPSILON,
        _ => unit(rng),
    }
}

fn drawn(rng: &mut XorShift, model: TapeModel, period: f32) -> TapeEngineParams {
    let start = edge(rng);
    TapeEngineParams {
        machine: TapeMachine::of(model),
        direction: MODES[rng.below(4)],
        tron: TRONS[rng.below(2)],
        drift: Drift::of(edge(rng)),
        start,
        end: if rng.below(5) == 0 { start } else { edge(rng) },
        position: edge(rng),
        motion: [2.0 * edge(rng) - 1.0, -1.0, 1.0, 0.0, -1e-6, 1e-6][rng.below(6)],
        tape: edge(rng),
        age: edge(rng),
        size: edge(rng),
        xfade: 0.5 * edge(rng),
        drag: edge(rng),
        kind: [StretchType::Mono, StretchType::Poly, StretchType::Grain][rng.below(3)],
        root: [0.0, 6_000.0, 12_700.0, 12_700.0 * unit(rng)][rng.below(4)],
        period: [0.0, period][rng.below(2)],
        slice: SliceCode::new(rng.below(18) as u8),
        wow: [Wow::Capstan, Wow::Warble][rng.below(2)],
        retrig: Retrig::Stack,
        ..TapeEngineParams::default()
    }
}

/// The sources' sine peak.
const PEAK: f32 = 0.9;

/// The peak a sine at `PEAK` may reach through any machine: an eighth past
/// it (a STRETCH hold's most, ADR 0103) times the Hermite's own ~1.07×
/// overshoot on a short period read at `INC_MAX`, 1.125 × 1.07 ≈ 1.2.
const BOUND: f32 = 1.22 * PEAK;

/// Every machine on a sine (a transwave of sines for TRANSWAVE), with its
/// params, pitch and gate changed block by block mid-note: a fresh draw,
/// END, START onto END, MOTION negated, a pitch jump (to ±120 st), a
/// note-off, a glide, a retrigger, SIZE and XFADE, POSITION; edges a third
/// of the time. Every sample stays finite and within `BOUND`. The
/// held-still bounds test above missed SCRUB's NaN and PING-PONG's
/// retargeted lift (+6.6 dB).
#[test]
fn mid_note_changes_stay_finite_and_bounded() {
    let mut rng = XorShift(0x7A9E_1003);
    let rates = [4_000u16, 8_000, 24_000, 48_000];
    let pitch = |rng: &mut XorShift| {
        [9_600.0 * edge(rng) - 4_800.0, 12_000.0, -12_000.0, 0.0][rng.below(4)]
    };
    for model in TapeModel::ALL {
        for run in 0..300 {
            let (held, period) = if model == TapeModel::Transwave {
                let top = [512u32, 1_024][rng.below(2)];
                let count = 1 + rng.below(16) as u8;
                let amps: Vec<f32> = (0..count).map(|_| 0.2 + 0.75 * unit(&mut rng)).collect();
                let held = held_transwave(top, count, |f, h| if h == 1 { amps[f] } else { 0.0 });
                (held, 0.0)
            } else {
                let most = [300, 5_000, 40_000][rng.below(3)];
                let frames = 64 + rng.below(most);
                let period = 4.0 + 400.0 * unit(&mut rng);
                let s: Vec<f32> = (0..frames)
                    .map(|i| PEAK * libm::sinf(core::f32::consts::TAU * i as f32 / period))
                    .collect();
                (held_plain(&s, rates[rng.below(rates.len())]), period)
            };
            let v = SourceView::strict(&held).unwrap();
            let mut p = drawn(&mut rng, model, period);
            let mut slot = MaybeUninit::uninit();
            let e = TapeEngine::init_in_place(&mut slot);
            e.seed(run);
            e.set_pitch(pitch(&mut rng));
            e.note_on(rng.below(128) as u8, &p, &v, 48_000);
            let mut b = [0.0; BLOCK_SIZE];
            for k in 0..150 {
                let op = rng.below(12);
                match op {
                    0 => p = drawn(&mut rng, model, period),
                    1 => p.end = edge(&mut rng),
                    2 => {
                        p.start = edge(&mut rng);
                        p.end = p.start;
                    }
                    3 => p.motion = -p.motion,
                    4 => e.set_pitch(pitch(&mut rng)),
                    5 => e.note_off(),
                    6 => e.glide_to(rng.below(128) as u8, (48_000, 0.01 * unit(&mut rng))),
                    7 => {
                        e.set_pitch(pitch(&mut rng));
                        e.note_on(rng.below(128) as u8, &p, &v, 48_000);
                    }
                    8 => {
                        p.size = edge(&mut rng);
                        p.xfade = 0.5 * edge(&mut rng);
                    }
                    9 => p.position = edge(&mut rng),
                    _ => {}
                }
                e.render(&mut b, &p, &v, 48_000);
                let worst = b.iter().fold(0.0f32, |m, y| m.max(y.abs()));
                assert!(
                    worst.is_finite() && worst <= BOUND,
                    "{model:?} run {run} block {k} (op {op}): {worst}, {p:?}"
                );
            }
        }
    }
}
