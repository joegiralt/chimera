//! CPU costs (ADR 0013): cycles/sample per voice, measured on the bench
//! (`bench-results.md`, rev V at 480 MHz) and rounded up to the next 10.

use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, Cost, MAX_VOICES, SampleBudget};
use chimera_core::params::EngineType;

/// `bench-results.md`: Modal 379 cycles/sample per voice, minus the chain
/// and rounded up to the next 10. Algo is an estimate (`AlgoEngine::COST` plus the chain) until Task 13
/// measures the worst case.
#[test]
fn voice_costs_are_the_bench_measurements() {
    assert_eq!(Voice::CHAIN_COST, Cost(10));
    assert_eq!(Voice::cost(EngineType::Algo), Cost(570));
    assert_eq!(Voice::cost(EngineType::Modal), Cost(380));
    for e in EngineType::ALL {
        assert_eq!(
            Voice::cost(e),
            Engines::cost(e) + Voice::CHAIN_COST,
            "{e:?}"
        );
    }
}

/// What the 7,000-cycle budget allows with the FX bus (MidiVerb, the
/// costliest reverb) running: as many voices of each engine as fit, up to
/// `MAX_VOICES`.
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
