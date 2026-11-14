//! METAL's squares a block at a time null against their old render, a
//! sample at a time (`render_ticked`): HAT closed and open, CYMBAL,
//! COWBELL and JINGLE, at their defaults, their knobs' ends and the keys'
//! ends. Bit-identical, or under −120 dB.

use chimera_core::Velocity;
use chimera_core::dsp::drum::kernel::KernelState;
use chimera_core::dsp::drum::laws::resolve;
use chimera_core::dsp::drum::types::{
    HitSeed, Knob, KnobSet, KnobValue, Machine, SampleRate, Transpose,
};
use chimera_core::hw::{BLOCK_SIZE, SAMPLE_RATE};
use chimera_midi::value::U7;

fn with(mut s: KnobSet, k: Knob, v: u8) -> KnobSet {
    s.set(k, KnobValue::new(U7::new(v).unwrap()));
    s
}

fn all(v: u8) -> KnobSet {
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
        s = with(s, k, v);
    }
    s
}

/// The worst null over `m`'s sets, transposes and seeds, in dB under the
/// old render (−inf: bit-identical).
fn null(m: Machine, sets: &[KnobSet]) -> f32 {
    let limit = 10 * SAMPLE_RATE as usize / BLOCK_SIZE;
    let mut worst = f32::NEG_INFINITY;
    for ks in sets {
        for (up, seed) in [(0, 7), (67, 0xC0FFEE), (-60, 7)] {
            let p = resolve(m, ks).transposed(Transpose::new(up).unwrap());
            let vel = Velocity::new(127).unwrap();
            let strike = || KernelState::strike(p, vel, HitSeed::new(seed), SampleRate::HW);
            let (mut new, mut old) = (strike(), strike());
            let (mut e_ref, mut e_diff) = (0.0f64, 0.0f64);
            for _ in 0..limit {
                if new.finished() && old.finished() {
                    break;
                }
                let (mut a, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
                new.render(&mut a);
                old.render_ticked(&mut b);
                for (x, y) in a.iter().zip(&b) {
                    e_ref += f64::from(*y) * f64::from(*y);
                    e_diff += f64::from(x - y) * f64::from(x - y);
                }
            }
            assert!(e_ref > 0.0, "{m:?} at {up}: silent");
            worst = worst.max(10.0 * (e_diff / e_ref).log10() as f32);
        }
    }
    worst
}

#[test]
fn metal_block_squares_null_against_ticked() {
    let hat = KnobSet::defaults(Machine::MetalHat);
    let cases = [
        (
            "HAT closed",
            Machine::MetalHat,
            vec![with(hat, Knob::B, 20)],
        ),
        ("HAT open", Machine::MetalHat, vec![with(hat, Knob::B, 80)]),
        ("HAT ends", Machine::MetalHat, vec![all(0), all(127)]),
        (
            "CYMBAL",
            Machine::ToneCymbal,
            vec![KnobSet::defaults(Machine::ToneCymbal), all(0), all(127)],
        ),
        (
            "COWBELL",
            Machine::ToneCowbell,
            vec![KnobSet::defaults(Machine::ToneCowbell), all(0), all(127)],
        ),
        (
            "JINGLE",
            Machine::MetalJingle,
            vec![KnobSet::defaults(Machine::MetalJingle), all(0), all(127)],
        ),
    ];
    for (name, m, sets) in cases {
        let db = null(m, &sets);
        eprintln!("{name}: null {db} dB");
        assert!(db < -120.0, "{name}: null {db} dB");
    }
}
