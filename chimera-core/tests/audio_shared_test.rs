//! What crosses from the UI to the audio thread (spec § Threading).

use chimera_core::instrument::AudioShared;
use chimera_core::part::PartMode;
use chimera_core::preset::{ChainType, Performance};

#[test]
fn snapshot_copies_every_part_and_the_fx() {
    let mut perf = Performance::new();
    perf.parts[4].load_init(ChainType::Fm);
    perf.parts[4].mix.mode = PartMode::Mono;
    perf.parts[4].mix.pan = -0.5;
    perf.fx.reverb.mix = 0.4;
    let shared = AudioShared::from_performance(&perf);
    assert_eq!(shared.parts[4].params.engine(), perf.parts[4].sound.params.engine());
    assert_eq!(shared.parts[4].mix, perf.parts[4].mix);
    assert_eq!(shared.fx.reverb.mix, 0.4);
    for (i, p) in shared.parts.iter().enumerate() {
        assert_eq!(p.mix.channel.get() as usize, i);
    }
}

/// The UI refreshes the back buffer in place each frame.
#[test]
fn update_from_overwrites_in_place() {
    let mut shared = AudioShared::default();
    let mut perf = Performance::new();
    perf.parts[0].sound.params.filter.cutoff = 440.0;
    perf.parts[0].mix.level = 0.1;
    shared.update_from(&perf);
    assert_eq!(shared.parts[0].params.filter.cutoff, 440.0);
    assert_eq!(shared.parts[0].mix.level, 0.1);
}
