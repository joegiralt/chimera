//! A Part's mix settings (instrument-core spec § Data model).

use chimera_core::block::Block;
use chimera_core::hw::MAX_PARTS;
use chimera_core::part::{DacPair, PartMode, PartParams};
use chimera_core::preset::{ChainType, Part, Performance};
use chimera_core::MidiChannel;

#[test]
fn midi_channel_accepts_0_to_15_only() {
    assert_eq!(MidiChannel::new(0).map(MidiChannel::get), Some(0));
    assert_eq!(MidiChannel::new(15).map(MidiChannel::get), Some(15));
    assert_eq!(MidiChannel::new(16), None);
    assert_eq!(MidiChannel::clamped(200).get(), 15);
}

/// Part n listens on channel n (0-based), Poly, output P1, level 0.8,
/// centre pan, no sends.
#[test]
fn performance_defaults_per_part() {
    let perf = Performance::new();
    for (n, part) in perf.parts.iter().enumerate() {
        let m = &part.mix;
        assert_eq!(m.channel.get() as usize, n);
        assert_eq!((m.mode, m.output), (PartMode::Poly, DacPair::P1));
        assert_eq!((m.level, m.pan, m.sends), (0.8, 0.0, [0.0; 3]));
    }
    assert_eq!(perf.parts.len(), MAX_PARTS);
}

#[test]
fn enum_params_round_trip_through_the_block() {
    let mut m = PartParams::for_part(0);
    m.set(PartParams::MODE, 0.0);
    assert_eq!(m.mode, PartMode::Mono);
    m.set(PartParams::OUTPUT, 2.0);
    assert_eq!(m.output, DacPair::P3);
    assert_eq!(m.output.index(), 2);
    m.set(PartParams::CHANNEL, 99.0); // clamps to the spec's max
    assert_eq!(m.channel.get(), 15);
    m.nudge(PartParams::SEND_REVERB, 3);
    assert_eq!(m.sends[2], 3.0 / 128.0);
}

/// ADR 0010: level, pan and sends are applied by the mixer, not read by the
/// voice, so none of them is modulatable.
#[test]
fn no_part_param_is_modulatable() {
    assert!(PartParams::default().specs().iter().all(|s| !s.modulatable));
}

/// Loading a Sound replaces only the Sound: channel, mode and mix stay.
#[test]
fn loading_a_sound_keeps_the_mix() {
    let mut part = Part::new(ChainType::PizzaPoly);
    part.mix.channel = MidiChannel::new(9).unwrap();
    part.mix.level = 0.25;
    part.load_init(ChainType::Fm);
    assert_eq!(part.sound.chain_type, ChainType::Fm);
    assert_eq!((part.mix.channel.get(), part.mix.level), (9, 0.25));
    assert_eq!(part.loaded_from, None);
}
