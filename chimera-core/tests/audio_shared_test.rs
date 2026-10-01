//! What crosses from the UI to the audio thread (spec § Threading).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::instrument::{AudioShared, PartAudio};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::ModState;
use chimera_core::params::EngineType;
use chimera_core::params::FilterParams;
use chimera_core::part::PartMode;
use chimera_core::preset::{Performance, Sound};
use chimera_core::project::PartId;

fn non_default_performance() -> Performance {
    let mut perf = Performance::new();
    *perf.edit(PartId::ALL[4]).sound = Sound::init(EngineType::Algo);
    perf.edit(PartId::ALL[4]).mix.mode = PartMode::Mono;
    perf.edit(PartId::ALL[4]).mix.pan = -0.5;
    perf.fx.reverb.mix = 0.4;

    // A non-default ModState: one modulatable destination with a non-zero amount.
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut registry = ModDestRegistry::new();
    registry.add(cutoff, *b"CUTOFF\0\0").unwrap();
    let mut mod_state = ModState::from_registry(&registry, 1);
    mod_state.set_amount(0, 0, 42);
    perf.edit(PartId::ALL[4]).sound.mod_state = mod_state;
    perf
}

#[test]
fn snapshot_copies_every_part_and_the_fx() {
    let perf = non_default_performance();
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let shared = AudioShared::from_performance(&perf);
    assert_eq!(
        shared.parts[4].params.engine(),
        perf.parts()[4].sound.params.engine()
    );
    assert_eq!(shared.parts[4].mix, perf.parts()[4].mix);
    assert_eq!(shared.fx.reverb.mix, 0.4);
    assert_eq!(shared.parts[4].mod_state.num_dests(), 1);
    assert_eq!(shared.parts[4].mod_state.dest(0), cutoff);
    assert_eq!(shared.parts[4].mod_state.amount(0, 0), 42);
    for (i, p) in shared.parts.iter().enumerate() {
        assert_eq!(p.mix.channel.get() as usize, i);
    }
}

/// The UI refreshes the back buffer in place each frame: `update_from` must
/// replace stale routing (from a previous Sound), not just leave it mixed in.
#[test]
fn update_from_overwrites_in_place() {
    let mut shared = AudioShared::default();

    // Give the back buffer stale routing: two destinations, amount 99.
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let resonance = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
    let mut stale_registry = ModDestRegistry::new();
    stale_registry.add(cutoff, *b"CUTOFF\0\0").unwrap();
    stale_registry.add(resonance, *b"RESO\0\0\0\0").unwrap();
    let mut stale_mod_state = ModState::from_registry(&stale_registry, 1);
    stale_mod_state.set_amount(0, 0, 99);
    stale_mod_state.set_amount(0, 1, 99);
    shared.parts[0].mod_state = stale_mod_state;

    let mut perf = Performance::new();
    perf.edit(PartId::ALL[0]).sound.params.filter.cutoff = 440.0;
    perf.edit(PartId::ALL[0]).mix.level = 0.1;

    // The Performance's actual routing: one destination, a different amount.
    let mut fresh_registry = ModDestRegistry::new();
    fresh_registry.add(cutoff, *b"CUTOFF\0\0").unwrap();
    let mut fresh_mod_state = ModState::from_registry(&fresh_registry, 1);
    fresh_mod_state.set_amount(0, 0, 17);
    perf.edit(PartId::ALL[0]).sound.mod_state = fresh_mod_state;

    shared.update_from(&perf, 0);

    assert_eq!(shared.parts[0].params.filter.cutoff, 440.0);
    assert_eq!(shared.parts[0].mix.level, 0.1);
    assert_eq!(shared.parts[0].mod_state.num_dests(), 1);
    assert_eq!(shared.parts[0].mod_state.dest(0), cutoff);
    assert_eq!(shared.parts[0].mod_state.amount(0, 0), 17);
}

/// Built in a slot full of garbage, `init_in_place` matches both
/// `from_performance` and a field-by-field copy of the Performance.
#[test]
fn init_in_place_matches_from_performance() {
    let perf = non_default_performance();
    let mut slot = Box::new(core::mem::MaybeUninit::<AudioShared>::uninit());
    // SAFETY: filling a `MaybeUninit`'s bytes is always sound.
    unsafe { slot.as_mut_ptr().write_bytes(0xA5, 1) };
    let placed = AudioShared::init_in_place(&mut slot, &perf);

    let expected = AudioShared {
        parts: core::array::from_fn(|i| {
            let p = &perf.parts()[i];
            PartAudio {
                params: p.sound.params.clone(),
                mod_state: p.sound.mod_state.clone(),
                mix: p.mix,
            }
        }),
        fx: perf.fx,
        epoch: 0,
    };
    let placed = format!("{placed:?}");
    assert_eq!(placed, format!("{expected:?}"));
    assert_eq!(
        placed,
        format!("{:?}", AudioShared::from_performance(&perf))
    );
    assert_ne!(placed, format!("{:?}", AudioShared::default()));
}
