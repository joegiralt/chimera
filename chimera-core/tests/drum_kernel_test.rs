// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's kernels at their knobs' extremes (plan Review Focus 1): every
//! slice-1 machine at its defaults, all knobs 0, all 127, and each knob
//! alone at 0 and 127, as tuned and transposed to both ends of the keys
//! (−60 and +67 semitones, ADR 0270), at velocity 1 and 127: finite, never past +6 dBFS,
//! finished within 20 s.

use chimera_core::Velocity;
use chimera_core::dsp::drum::kernel::KernelState;
use chimera_core::dsp::drum::laws::resolve;
use chimera_core::dsp::drum::types::{
    HitSeed, Knob, KnobSet, KnobValue, Machine, SampleRate, Transpose,
};
use chimera_core::hw::{BLOCK_SIZE, SAMPLE_RATE};
use chimera_midi::value::U7;

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

fn all(v: u8) -> KnobSet {
    let mut s = KnobSet::ZERO;
    for k in KNOBS {
        s.set(k, KnobValue::new(U7::new(v).unwrap()));
    }
    s
}

/// The sets each machine is played at, and a name for each.
fn sets(m: Machine) -> Vec<(String, KnobSet)> {
    let mut v = vec![
        ("defaults".into(), KnobSet::defaults(m)),
        ("all 0".into(), all(0)),
        ("all 127".into(), all(127)),
    ];
    for k in KNOBS {
        for x in [0, 127] {
            let mut s = KnobSet::defaults(m);
            s.set(k, KnobValue::new(U7::new(x).unwrap()));
            v.push((format!("{k:?} {x}"), s));
        }
    }
    v
}

#[test]
fn kernels_stay_bounded_and_finish() {
    let limit = 20 * SAMPLE_RATE as usize / BLOCK_SIZE;
    let mut fails = Vec::new();
    for m in Machine::ALL {
        let mut loudest = 0.0f32;
        let mut longest = 0;
        // TONAL's reach (ADR 0270): MIDI 0 and 127 over middle C.
        assert_eq!(Transpose::new(68), None, "+67 is the top of the keys");
        assert_eq!(Transpose::new(-61), None, "-60 is the bottom");
        for (name, ks) in sets(m) {
            for (v, seed, up) in [
                (1, 7, 0),
                (127, 7, 0),
                (1, 0xC0FFEE, 0),
                (127, 0xC0FFEE, 0),
                (1, 7, 67),
                (127, 0xC0FFEE, 67),
                (1, 7, -60),
                (127, 0xC0FFEE, -60),
            ] {
                let vel = Velocity::new(v).unwrap();
                let by = Transpose::new(up).unwrap();
                let p = resolve(m, &ks).transposed(by);
                let mut k = KernelState::strike(p, vel, HitSeed::new(seed), SampleRate::HW);
                let mut blocks = 0;
                let mut peak = 0.0f32;
                let mut finite = true;
                while !k.finished() && blocks < limit {
                    let mut out = [0.0; BLOCK_SIZE];
                    k.render(&mut out);
                    finite &= out.iter().all(|s| s.is_finite());
                    peak = out.iter().fold(peak, |a, s| a.max(s.abs()));
                    blocks += 1;
                }
                loudest = loudest.max(peak);
                longest = longest.max(blocks);
                if !finite || peak > 2.0 || !k.finished() {
                    fails.push(format!(
                        "{m:?} {name} {up:+} vel {v} seed {seed}: finite {finite}, peak {peak:.3}, finished {}",
                        k.finished()
                    ));
                }
            }
        }
        println!(
            "{m:?}: loudest {:.2} dBFS, longest {:.2} s",
            20.0 * loudest.log10(),
            (longest * BLOCK_SIZE) as f32 / SAMPLE_RATE as f32
        );
    }
    assert!(fails.is_empty(), "{fails:#?}");
}
