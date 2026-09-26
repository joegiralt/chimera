//! CPU costs (ADR 0013): cycles/sample per voice, measured on the bench
//! (`bench-results.md`, rev V at 480 MHz) and rounded up to the next 10.

use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, Cost, MAX_VOICES, SampleBudget};
use chimera_core::params::EngineType;

/// `bench-results.md`: Pizza 244, FM 6,599, Modal 379, VA (the chain) 6
/// cycles/sample per voice, each minus the chain and rounded up to the
/// next 10.
#[test]
fn voice_costs_are_the_bench_measurements() {
    assert_eq!(Voice::CHAIN_COST, Cost(10));
    assert_eq!(Voice::cost(EngineType::Pizza), Cost(250));
    assert_eq!(Voice::cost(EngineType::Fm), Cost(6_600));
    assert_eq!(Voice::cost(EngineType::Modal), Cost(380));
    assert_eq!(Voice::cost(EngineType::Va), Cost(310));
    for e in EngineType::ALL {
        assert_eq!(
            Voice::cost(e),
            Engines::cost(e) + Voice::CHAIN_COST,
            "{e:?}"
        );
    }
}

/// What the 7,000-cycle budget allows with the FX bus (MidiVerb, the
/// costliest reverb) running: six of Pizza, Modal and VA; FM (6,600/voice)
/// doesn't fit even one, so every FM note is refused
/// (https://github.com/joegiralt/chimera/issues/26).
#[test]
fn budget_capacity_per_engine() {
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost();
    let fits = |e: EngineType, n: u32| FxBus::COST.0 + n * Voice::cost(e).0 <= budget.0;
    for e in EngineType::ALL {
        let k = ((budget.0 - FxBus::COST.0) / Voice::cost(e).0).min(MAX_VOICES as u32);
        assert!(fits(e, k), "{e:?} should fit {k}");
        if k < MAX_VOICES as u32 {
            assert!(!fits(e, k + 1), "{e:?} should not fit {}", k + 1);
        }
    }
}
