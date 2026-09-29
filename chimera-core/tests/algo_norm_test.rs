//! ADR 0049: the Algo output scale is a property of the patch, read from
//! the stored LEVELs. A matrix route into one carrier's LEVEL (VEL, an LFO)
//! never moves another carrier.
//!
//! The route runs as the voice's matrix pass does (`ModState::sum_for`
//! into `AlgoLive::offset`); the engine's output is measured before the SVF,
//! whose integrator states saturate at the levels a live scale needs
//! (a carrier power above 1 means a peak above 1).

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

/// INIT on A1: operator 1 at f0 (A4), operator 2 a carrier at 2 × f0 and
/// LEVEL 90, so the carrier power is above 1 and the scale is live.
fn two_carriers() -> AlgoParams {
    let mut p = AlgoParams::default();
    (p.alg_a, p.alg_b) = (AlgoId::A1.get(), AlgoId::A1.get());
    for o in &mut p.ops[2..] {
        o.level = 0;
    }
    (p.ops[1].coarse, p.ops[1].level) = (8, 90);
    p
}

/// `source` → operator 2's LEVEL at `amount`.
fn route(source: ModSource, amount: i8) -> ModState {
    let level = ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::LEVEL);
    let mut reg = ModDestRegistry::new();
    reg.add(level, *b"OP2 LVL\0").unwrap();
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    ms.set_amount(source.index(), 0, amount);
    ms
}

/// Plays A4 at `vel`; block `b`'s source values come from `sources(b)`.
fn play(
    p: &AlgoParams,
    ms: &ModState,
    vel: u8,
    blocks: usize,
    sources: impl Fn(usize) -> [f32; MAX_MOD_SOURCES],
) -> Vec<f32> {
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::A4, Velocity::new(vel).unwrap(), p, SAMPLE_RATE);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..blocks {
        let mut live = AlgoLive::from_params(p);
        live.routed = ms.algo_levels_routed();
        for d in 0..ms.num_dests() {
            live.offset(ms.dest(d), ms.sum_for(d, &sources(b)));
        }
        e.render(&mut blk, p, &live, SAMPLE_RATE);
        out.extend_from_slice(&blk);
    }
    out
}

/// Operator 1's amplitude: the 440 Hz component of 20 periods (2182
/// samples) under a Hann window, whose sidelobes keep operator 2's 880 Hz,
/// swept by a route, out.
fn f0_amp(s: &[f32]) -> f64 {
    let n = 2182;
    let w = core::f64::consts::TAU * 440.0 / SAMPLE_RATE as f64;
    let (mut re, mut im, mut sw) = (0.0, 0.0, 0.0);
    for (i, &x) in s[..n].iter().enumerate() {
        let h = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
        re += x as f64 * h * (w * i as f64).cos();
        im += x as f64 * h * (w * i as f64).sin();
        sw += h;
    }
    2.0 * re.hypot(im) / sw
}

#[test]
fn a_vel_route_on_a_soft_note_never_makes_another_carrier_louder() {
    let (p, ms) = (two_carriers(), route(ModSource::Vel, 127));
    let tail = |vel: u8| {
        let mut v = [0.0; MAX_MOD_SOURCES];
        v[ModSource::Vel.index()] = Velocity::new(vel).unwrap().unit();
        f0_amp(&play(&p, &ms, vel, 80, |_| v)[40 * BLOCK_SIZE..])
    };
    let (loud, soft) = (tail(127), tail(1));
    assert!(soft <= loud * (1.0 + 1e-4), "soft {soft}, loud {loud}");
}

#[test]
fn an_lfo_route_on_a_carrier_leaves_the_others_constant() {
    let p = two_carriers();
    // A 3 Hz sine as LFO 1, stepped per block as the voice's LFO is.
    let lfo = |b: usize| {
        let mut v = [0.0; MAX_MOD_SOURCES];
        let t = (b * BLOCK_SIZE) as f32 / SAMPLE_RATE as f32;
        v[ModSource::Lfo1.index()] = (core::f32::consts::TAU * 3.0 * t).sin();
        v
    };
    let out = play(&p, &route(ModSource::Lfo1, 127), 100, 800, lfo);
    let amps: Vec<f64> = out[20 * BLOCK_SIZE..]
        .as_chunks::<2182>()
        .0
        .iter()
        .map(|w| 20.0 * f0_amp(w).log10())
        .collect();
    let (lo, hi) = amps
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), &a| (l.min(a), h.max(a)));
    assert!(hi - lo < 0.02, "operator 1 moves {:.3} dB", hi - lo);
}
