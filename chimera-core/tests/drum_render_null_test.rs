// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every DRUM machine renders as it did before the cost work (origin/0.4
//! `262039d9`), bit for bit: its defaults, every knob at 0 and at 127 and
//! each INIT pad's knobs, at three transposes and two seeds, up to 10 s a
//! hit. A machine's print is FNV-1a over its renders' bits, a hit's end
//! marked, so lengths must match too.

use chimera_core::Velocity;
use chimera_core::dsp::drum::kernel::KernelState;
use chimera_core::dsp::drum::laws::resolve;
use chimera_core::dsp::drum::types::{
    HitSeed, Kit, Knob, KnobSet, KnobValue, Machine, PadId, SampleRate, Transpose,
};
use chimera_core::hw::{BLOCK_SIZE, SAMPLE_RATE};
use chimera_midi::value::U7;

/// Every knob at `v`.
fn all(v: U7) -> KnobSet {
    let mut s = KnobSet::ZERO;
    for k in [
        Knob::A,
        Knob::B,
        Knob::C,
        Knob::D,
        Knob::E,
        Knob::F,
        Knob::G,
        Knob::H,
    ] {
        s.set(k, KnobValue::new(v));
    }
    s
}

/// A running FNV-1a print.
struct Print(u64);

impl Print {
    fn add(&mut self, bits: u32) {
        self.0 = (self.0 ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// `m`'s print over its knob sets.
fn print(m: Machine, kit: &Kit) -> u64 {
    let mut sets = vec![
        KnobSet::defaults(m),
        all(U7::new(0).unwrap()),
        all(U7::new(127).unwrap()),
    ];
    sets.extend(
        PadId::ALL
            .into_iter()
            .map(|p| kit.pad(p))
            .filter(|p| p.machine() == m)
            .map(|p| *p.knobs()),
    );
    let limit = 10 * SAMPLE_RATE as usize / BLOCK_SIZE;
    let mut h = Print(0xcbf2_9ce4_8422_2325);
    for ks in &sets {
        for up in [0, 67, -60] {
            for seed in [7, 0xC0_FFEE] {
                let p = resolve(m, ks).transposed(Transpose::new(up).unwrap());
                let mut k = KernelState::strike(
                    p,
                    Velocity::new(127).unwrap(),
                    HitSeed::new(seed),
                    SampleRate::HW,
                );
                for _ in 0..limit {
                    if k.finished() {
                        break;
                    }
                    let mut b = [0.0; BLOCK_SIZE];
                    k.render(&mut b);
                    b.iter().for_each(|v| h.add(v.to_bits()));
                }
                h.add(f32::NAN.to_bits());
            }
        }
    }
    h.0
}

/// The prints at origin/0.4 `262039d9`.
const BEFORE: &[(Machine, u64)] = &[
    (Machine::ToneKick, 0x3ba825dfc53b88d6),
    (Machine::ToneSnare, 0x3119c7d44a1111dc),
    (Machine::ToneTom, 0xd30fe2554ff4c1d7),
    (Machine::ToneClap, 0x212596044d49ee6f),
    (Machine::ToneCowbell, 0xf7e5811c1bee77d5),
    (Machine::ToneCymbal, 0x8b83f2791425ec73),
    (Machine::MetalHat, 0x500d483d2cb749f5),
    (Machine::MetalJingle, 0x07b2ddc005faf480),
    (Machine::BodyShaker, 0xfd6404ebac8b05c9),
    (Machine::PlainSine, 0x07c7be547ee549e5),
    (Machine::PlainNoise, 0x553d2303796b2416),
    (Machine::PlainImpulse, 0xbd84e35bf1b5c38f),
];

#[test]
fn every_machine_renders_as_before() {
    let kit = Kit::init();
    let now: Vec<(Machine, u64)> = Machine::ALL
        .into_iter()
        .map(|m| (m, print(m, &kit)))
        .collect();
    assert_eq!(now, BEFORE);
}
