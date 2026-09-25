//! CPU cost estimates (ADR 0013): cycles/sample per voice from the design
//! doc's budget table, until measured with the DWT cycle counter.

use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{AUDIO_CYCLE_BUDGET, Cost, MAX_VOICES};
use chimera_core::params::EngineType;

/// `docs/chimera-synth-design.md` § CPU Budget: FM ~610, Modal ~1,210 and
/// VA ~710 per voice including the chain (~410).
#[test]
fn voice_costs_follow_the_design_table() {
    assert_eq!(Voice::CHAIN_COST, Cost(410));
    assert_eq!(Voice::cost(EngineType::Fm), Cost(610));
    assert_eq!(Voice::cost(EngineType::Modal), Cost(1_210));
    assert_eq!(Voice::cost(EngineType::Va), Cost(710));
    assert_eq!(Voice::cost(EngineType::Pizza), Cost(710));
    for e in EngineType::ALL {
        assert_eq!(
            Voice::cost(e),
            Engines::cost(e) + Voice::CHAIN_COST,
            "{e:?}"
        );
    }
}

/// What the budget allows with the FX bus running: six of every engine but
/// Modal, five Modal (the design doc planned for four).
#[test]
fn budget_capacity_per_engine() {
    let fits = |e: EngineType, n: u32| FxBus::COST.0 + n * Voice::cost(e).0 <= AUDIO_CYCLE_BUDGET.0;
    for e in [EngineType::Pizza, EngineType::Fm, EngineType::Va] {
        assert!(fits(e, MAX_VOICES as u32), "{e:?}");
    }
    assert!(fits(EngineType::Modal, 5));
    assert!(!fits(EngineType::Modal, 6));
}
