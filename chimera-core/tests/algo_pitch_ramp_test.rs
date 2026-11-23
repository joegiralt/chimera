// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! #164: under a fast route into PITCH (an LFO or an ENV), ALGO's table
//! operators ramp their increment across the block instead of stepping it
//! once a block. A steady pitch keeps the old arithmetic, bit for bit.
mod common;
use common::{Rig, SR};

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::params::{EngineType, ParamSnapshot, PitchParams};
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// Operator 1's sine alone at C6, `blocks` blocks, `mods` its routes,
/// LFO 1 at 8 Hz and `depth`.
fn play(mods: &ModState, depth: f32, blocks: usize) -> Vec<f32> {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::W1);
    (p.lfos[0].rate, p.lfos[0].depth) = (8.0, depth);
    let mut v = Rig::new(SR);
    v.note_on(MidiNote::new(84).unwrap(), Velocity::DEFAULT, &p);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK_SIZE];
    for _ in 0..blocks {
        v.render(&mut block, &p, mods);
        out.extend_from_slice(&block);
    }
    out
}

/// ALGO INIT's routes.
fn init_mods() -> ModState {
    Sound::init(EngineType::Algo).mod_state
}

fn lfo_on_pitch(amount: i8) -> ModState {
    let mut m = init_mods();
    let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
    assert!(m.set_route_addr(ModSource::Lfo1.index(), pitch, amount));
    m
}

/// The sine's frequency at sample `n`, radians a sample, from its
/// three-term recurrence `y[n+1] + y[n−1] = 2·cos ω·y[n]`; `None` near a
/// zero crossing, where the ratio is noise. A host test: f64's clamp
/// links no firmware.
#[allow(clippy::disallowed_methods)]
fn omega(y: &[f32], n: usize, floor: f32) -> Option<f64> {
    let (a, b, c) = (y[n - 1] as f64, y[n] as f64, y[n + 1] as f64);
    (b.abs() > floor as f64).then(|| ((a + c) / (2.0 * b)).clamp(-1.0, 1.0).acos())
}

/// Least-squares slope of `(n, w)`.
fn slope(pts: &[(f64, f64)]) -> f64 {
    let k = pts.len() as f64;
    let (mx, my) = (
        pts.iter().map(|p| p.0).sum::<f64>() / k,
        pts.iter().map(|p| p.1).sum::<f64>() / k,
    );
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    sxy / sxx
}

/// Where the LFO sweeps fastest, the frequency inside a block moves as
/// far as it moves from one block to the next: a ramp, not a step.
#[test]
fn a_fast_pitch_route_ramps_inside_the_block() {
    let y = play(&lfo_on_pitch(127), 1.0, 300);
    let floor = 0.5
        * y[100 * BLOCK_SIZE..]
            .iter()
            .fold(0.0f32, |m, x| m.max(x.abs()));
    // Per block from block 100: (mean ω, fitted in-block slope × 64).
    let blocks: Vec<(f64, f64)> = (100..299)
        .map(|b| {
            let pts: Vec<(f64, f64)> = (b * BLOCK_SIZE + 2..(b + 1) * BLOCK_SIZE - 2)
                .filter_map(|n| omega(&y, n, floor).map(|w| (n as f64, w)))
                .collect();
            let mean = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
            (mean, slope(&pts) * BLOCK_SIZE as f64)
        })
        .collect();
    let steps: Vec<f64> = blocks.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let top = steps.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    assert!(top > 0.01, "the LFO moves the pitch: {top}");
    let mut checked = 0;
    for (b, s) in steps.windows(2).enumerate() {
        // Block b+1's in-block slope against the motion either side of it.
        let around = 0.5 * (s[0] + s[1]);
        if around.abs() < 0.5 * top {
            continue;
        }
        let inside = blocks[b + 1].1;
        let r = inside / around;
        assert!(
            (0.6..1.4).contains(&r),
            "block {}: inside {inside:.5} vs around {around:.5}",
            b + 101
        );
        checked += 1;
    }
    assert!(checked > 10, "{checked} blocks checked");
}

/// A fast route whose LFO is still (DEPTH 0) takes the ramp's path with
/// nothing to ramp: the same samples as a route that asks nothing.
#[test]
fn a_still_pitch_under_a_fast_route_plays_as_before() {
    let (a, b) = (
        play(&lfo_on_pitch(0), 0.0, 100),
        play(&lfo_on_pitch(127), 0.0, 100),
    );
    assert!(a.iter().any(|x| x.abs() > 0.01));
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}

/// The ramp's bill (#164): `COST_RAMP`, 2, for every table operator (an
/// upper bound), on a Sound with a fast route into PITCH; none with a slow
/// one.
#[test]
fn a_fast_pitch_route_bills_its_ramp() {
    use chimera_core::dsp::algo::engine::AlgoEngine;
    use chimera_core::dsp::engines::{EngineSlot, pitch_ramps};
    use chimera_core::hw::Cost;
    assert_eq!(AlgoEngine::COST_RAMP, Cost(2));
    // Six table operators, ALGO INIT's and a lone sine's alike.
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    assert_eq!(AlgoEngine::ramp_cost(&p.algo), Cost(12));
    p.algo = AlgoParams::single(WaveId::W1);
    assert_eq!(AlgoEngine::ramp_cost(&p.algo), Cost(12));
    // A fast route past a semitone ramps; the Instrument adds the bill,
    // so the engine's own stays as it was.
    assert!(pitch_ramps(&lfo_on_pitch(127)));
    assert_eq!(
        EngineSlot::cost(&p, &lfo_on_pitch(127)),
        EngineSlot::cost(&p, &init_mods())
    );
    // INIT's wheel vibrato (38 cents) and VEL (latched) don't.
    assert!(!pitch_ramps(&init_mods()));
    let mut m = init_mods();
    let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
    assert!(m.set_route_addr(ModSource::Vel.index(), pitch, 127));
    assert!(!pitch_ramps(&m));
}
