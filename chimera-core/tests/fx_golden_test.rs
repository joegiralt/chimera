//! FX refactor lock (ADR 0011, ADR 0014): chorus, delay and reverb output
//! frozen bit-for-bit before their buffers were trimmed to what their ranges
//! need. Re-record only for an intended sound change (`common::golden`).

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::dsp::ring::RingReverb;
use chimera_hal::BLOCK_SIZE;
use common::{SR, fnv1a};

/// 50 blocks of a 110 Hz saw at 0.5, then silence; 450 blocks in all so a
/// 500 ms delay repeats at least once.
const BURST_BLOCKS: usize = 50;
const FX_BLOCKS: usize = 450;

#[derive(Clone, Copy)]
enum Fx {
    Chorus(ChorusParams),
    Delay(DelayParams),
    Reverb(ReverbParams),
}

fn cases() -> [(&'static str, Fx); 7] {
    let reverb = |grit: f32, size: f32| ReverbParams {
        grit,
        time: 0.7,
        damping: 0.3,
        size,
        mix: 0.5,
    };
    [
        (
            "chorus_both",
            Fx::Chorus(ChorusParams {
                mode: 3,
                rate: 0.5,
                depth: 0.5,
                mix: 0.5,
            }),
        ),
        (
            "delay_375ms",
            Fx::Delay(DelayParams {
                feedback: 0.6,
                mix: 0.5,
                ..DelayParams::default()
            }),
        ),
        (
            "delay_500ms",
            Fx::Delay(DelayParams {
                time_ms: 500.0,
                feedback: 0.6,
                wow_flutter: 1.0,
                mix: 0.5,
                ..DelayParams::default()
            }),
        ),
        // Five saturated repeats in the render: the feedback loop's lock.
        (
            "delay_feedback_100ms",
            Fx::Delay(DelayParams {
                time_ms: 100.0,
                feedback: 0.6,
                mix: 0.5,
                ..DelayParams::default()
            }),
        ),
        ("reverb_ring", Fx::Reverb(reverb(0.3, 0.5))),
        ("reverb_ring_max_size", Fx::Reverb(reverb(0.3, 1.0))),
        ("reverb_ring_full_grit", Fx::Reverb(reverb(1.0, 0.5))),
    ]
}

/// Block `b` of the input signal; `phase` carries the saw between blocks.
fn input(b: usize, phase: &mut f32) -> [f32; BLOCK_SIZE] {
    let mut block = [0.0f32; BLOCK_SIZE];
    if b < BURST_BLOCKS {
        for s in block.iter_mut() {
            *s = *phase - 0.5;
            *phase = (*phase + 110.0 / SR as f32).fract();
        }
    }
    block
}

/// Stereo effects add L then R per block; the delay its insert output.
fn render(fx: Fx) -> Vec<f32> {
    let mut chorus = Box::new(JunoChorus::new());
    let mut delay = Box::new(TapeDelay::new());
    let mut reverb = Box::new(RingReverb::new());
    let mut out = Vec::with_capacity(2 * FX_BLOCKS * BLOCK_SIZE);
    let mut phase = 0.0f32;
    for b in 0..FX_BLOCKS {
        let mut block = input(b, &mut phase);
        match fx {
            Fx::Chorus(p) => {
                let mut wet = Stereo::SILENT;
                chorus.process_wet(&block, &p, SR, &mut wet);
                out.extend_from_slice(&wet.l);
                out.extend_from_slice(&wet.r);
            }
            Fx::Delay(p) => {
                delay.process(&mut block, &p, SR);
                out.extend_from_slice(&block);
            }
            Fx::Reverb(p) => {
                let mut wet = Stereo::SILENT;
                reverb.process(&block, &p.controls(), p.mix, SR, &mut wet);
                out.extend_from_slice(&wet.l);
                out.extend_from_slice(&wet.r);
            }
        }
    }
    out
}

const GOLDENS: &[(&str, u64)] = &[
    ("chorus_both", 0x666b43969f8ed31a),           // u32 LFO phase
    ("delay_375ms", 0x036a3a3f29f429c6),           // MECHANICS (ADR 0053)
    ("delay_500ms", 0x40eb96efc0f4fefe),           // MECHANICS (ADR 0053)
    ("delay_feedback_100ms", 0x07e67081e5f28449),  // SAT × 1/gain, not ÷ gain (ADR 0053)
    ("reverb_ring", 0x2a4709f629cb4cd8),           // FX diet
    ("reverb_ring_max_size", 0x6eee15a3372d88b5),  // FX diet
    ("reverb_ring_full_grit", 0xe8b2bff63402edec), // FX diet
];

#[test]
fn fx_goldens_match() {
    let got: Vec<_> = cases()
        .into_iter()
        .map(|(name, fx)| (name, fnv1a(&render(fx))))
        .collect();
    common::golden::check(GOLDENS, &got);
}

/// ADR 0011's gate for the FX cases, run before their goldens are
/// recorded: finite, within ±1.0, and audible.
#[test]
fn fx_cases_pass_the_sanity_gate() {
    for (name, fx) in cases() {
        let out = render(fx);
        assert!(out.iter().all(|s| s.is_finite()), "{name}: non-finite");
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((1e-3..=1.0).contains(&peak), "{name}: peak {peak}");
    }
}

/// A case whose effect is bypassed would lock only the dry input.
#[test]
fn fx_cases_are_not_dry() {
    let mut phase = 0.0f32;
    let dry: Vec<f32> = (0..FX_BLOCKS).flat_map(|b| input(b, &mut phase)).collect();
    for (name, fx) in cases() {
        assert_ne!(fnv1a(&render(fx)), fnv1a(&dry), "{name} is bypassed");
    }
}
