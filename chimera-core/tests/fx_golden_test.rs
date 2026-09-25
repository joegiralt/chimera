//! FX refactor lock (ADR 0011, ADR 0014): chorus, delay and reverb output
//! frozen bit-for-bit before their buffers were trimmed to what their ranges
//! need. Re-record only for an intended sound change:
//!
//!     GOLDEN_RECORD=1 cargo test -p chimera-core --test fx_golden_test -- --nocapture

mod common;

use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::reverb::{Reverb, ReverbParams};
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

fn cases() -> [(&'static str, Fx); 6] {
    let reverb = |reverb_type: u8, size: f32| ReverbParams {
        reverb_type,
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
        ("reverb_plate", Fx::Reverb(reverb(0, 0.5))),
        ("reverb_fdn_max_size", Fx::Reverb(reverb(1, 1.0))),
        ("reverb_midiverb", Fx::Reverb(reverb(2, 0.5))),
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

fn render(fx: Fx) -> Vec<f32> {
    let mut chorus = Box::new(JunoChorus::new());
    let mut delay = Box::new(TapeDelay::new());
    let mut reverb = Box::new(Reverb::new());
    let mut out = Vec::with_capacity(FX_BLOCKS * BLOCK_SIZE);
    let mut phase = 0.0f32;
    for b in 0..FX_BLOCKS {
        let mut block = input(b, &mut phase);
        match fx {
            Fx::Chorus(p) => chorus.process(&mut block, &p, SR),
            Fx::Delay(p) => delay.process(&mut block, &p, SR),
            Fx::Reverb(p) => reverb.process(&mut block, &p),
        }
        out.extend_from_slice(&block);
    }
    out
}

const GOLDENS: &[(&str, u64)] = &[
    ("chorus_both", 0x0ad6a4dbd5636552),
    ("delay_375ms", 0x4ed2b23c577884bf),
    ("delay_500ms", 0xc1f7798ede6627fd),
    ("reverb_plate", 0x543857e5bed9848d),
    ("reverb_fdn_max_size", 0xcd5bdb2e4f83a478),
    ("reverb_midiverb", 0xef55c35ea73dc552),
];

#[test]
fn fx_goldens_match() {
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let mut failures = Vec::new();
    for (name, fx) in cases() {
        let hash = fnv1a(&render(fx));
        if record {
            println!("    (\"{name}\", 0x{hash:016x}),");
            continue;
        }
        match GOLDENS.iter().find(|g| g.0 == name) {
            Some(&(_, want)) if want == hash => {}
            Some(&(_, want)) => {
                failures.push(format!("{name}: 0x{hash:016x} (want 0x{want:016x})"))
            }
            None => failures.push(format!("{name}: no golden recorded")),
        }
    }
    assert!(
        failures.is_empty(),
        "fx golden mismatch:\n{}",
        failures.join("\n")
    );
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
