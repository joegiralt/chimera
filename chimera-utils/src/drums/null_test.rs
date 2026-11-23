// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine's kernels (ADR 0265) against the prototypes they port: every
//! slice-1 machine at each of its settings, a second rendered by both, the
//! difference's RMS at least 100 dB under the prototype's.

use super::machines::{Id, build, spec};
use super::parts::{B, Blk, FS};
use super::settings;
use chimera_core::Velocity;
use chimera_core::dsp::drum::kernel::KernelState;
use chimera_core::dsp::drum::laws::resolve;
use chimera_core::dsp::drum::types::{HitSeed, Knob, KnobSet, KnobValue, Machine, SampleRate};
use chimera_midi::value::U7;

const PAIRS: [(Id, Machine); 12] = [
    (Id::ToneKick, Machine::ToneKick),
    (Id::ToneSnare, Machine::ToneSnare),
    (Id::ToneTom, Machine::ToneTom),
    (Id::ToneClap, Machine::ToneClap),
    (Id::ToneCowbell, Machine::ToneCowbell),
    (Id::ToneCymbal, Machine::ToneCymbal),
    (Id::MetalHat, Machine::MetalHat),
    (Id::MetalJingle, Machine::MetalJingle),
    (Id::BodyShaker, Machine::BodyShaker),
    (Id::PlainSine, Machine::PlainSine),
    (Id::PlainNoise, Machine::PlainNoise),
    (Id::PlainImpulse, Machine::PlainImpulse),
];

const KNOBS: [Knob; 8] = [
    Knob::A,
    Knob::B,
    Knob::C,
    Knob::D,
    Knob::E,
    Knob::F,
    Knob::G,
    Knob::H,
];

/// A setting's eight positions, MORE's at the prototype's defaults where
/// it leaves them out (`drums::knobs`).
fn positions(id: Id, k: &[u8]) -> [u8; 8] {
    let more = spec(id).more;
    core::array::from_fn(|i| k.get(i).copied().unwrap_or(more[(i.max(6) - 6).min(1)]))
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// The difference's RMS over the prototype's, dB.
fn null_db(id: Id, m: Machine, pos: [u8; 8], vel: Velocity, seed: u32) -> f64 {
    let n = FS as usize / B;
    let mut proto = build(id, pos.map(|v| v as f32 / 127.0), vel.unit(), seed);
    let mut ks = KnobSet::ZERO;
    for (kn, v) in KNOBS.into_iter().zip(pos) {
        ks.set(kn, KnobValue::new(U7::new(v).unwrap()));
    }
    let mut port = KernelState::strike(resolve(m, &ks), vel, HitSeed::new(seed), SampleRate::HW);
    let (mut a, mut d) = (Vec::with_capacity(n * B), Vec::with_capacity(n * B));
    for _ in 0..n {
        let (mut x, mut y): (Blk, Blk) = ([0.0; B], [0.0; B]);
        proto.block(&mut x, None);
        port.render(&mut y);
        a.extend_from_slice(&x);
        d.extend(x.iter().zip(&y).map(|(p, q)| p - q));
    }
    let (ra, rd) = (rms(&a), rms(&d));
    assert!(ra > 0.0, "{m:?}: the prototype is silent");
    if rd == 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * (rd / ra).log10()
    }
}

#[test]
fn kernels_null_against_the_prototypes() {
    let mut worst = f64::NEG_INFINITY;
    let mut fails = Vec::new();
    for (id, m) in PAIRS {
        for (i, (name, k, _)) in settings(id).into_iter().enumerate() {
            for v in [127, 90] {
                let vel = Velocity::new(v).unwrap();
                let db = null_db(id, m, positions(id, k), vel, 1 + i as u32 * 7919);
                println!("{m:?} {name} vel {v}: {db:.1} dB");
                worst = worst.max(db);
                if db > -100.0 {
                    fails.push(format!("{m:?} {name} vel {v}: {db:.1} dB"));
                }
            }
        }
    }
    println!("worst {worst:.1} dB");
    assert!(fails.is_empty(), "above −100 dB: {fails:#?}");
}
