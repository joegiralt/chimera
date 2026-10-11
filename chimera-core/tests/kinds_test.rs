//! Operator kinds and the formant budget (formant-operators plan, Task 3).

use chimera_core::budget::{FORMANT_BUDGET, FORMANT_N, HEAVY_OVERHEAD};
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::kinds::{OP_KIND_IDENTS, OpKind, fits};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{CLEAN_MOVE_COST, FILTER_LEVEL_COST};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, FLOOR_VOICES, SampleBudget};
use chimera_core::modulation::ModRouting;

#[test]
fn op_kind_codes_are_frozen() {
    assert_eq!(OpKind::from_code(15), Some(OpKind::Table(WaveId::SSAW)));
    assert_eq!(OpKind::from_code(16), Some(OpKind::Formant));
    assert_eq!(OpKind::from_code(17), Some(OpKind::Noise));
    assert_eq!(OpKind::from_code(18), None);
    assert_eq!(OpKind::clamped(200), OpKind::Noise);
    assert_eq!(&OP_KIND_IDENTS[16..], &["FRMT", "NOIS"]);
    for c in 0..18 {
        assert_eq!(OpKind::from_code(c).unwrap().code(), c);
    }
}

#[test]
fn the_budget_is_derived_from_the_cost_model() {
    let share =
        (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - FxBus::COST_TAPE.0) / FLOOR_VOICES;
    let table = AlgoEngine::COST_BASE.0
        + 12 * AlgoEngine::COST_LINK.0
        + 6 * (AlgoEngine::COST_OP.0 + AlgoEngine::COST_FEEDBACK.0);
    let heavy = ModRouting::HEAVY.0 + CLEAN_MOVE_COST.0 + FILTER_LEVEL_COST.0 + Voice::CHAIN_COST.0;
    assert_eq!(HEAVY_OVERHEAD.0, heavy);
    assert_eq!(FORMANT_BUDGET.0, share - heavy - table);
    assert_eq!(
        OpKind::Formant.charge(),
        AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_OP.0
    );
    assert_eq!(FORMANT_N, FORMANT_BUDGET.0 / OpKind::Formant.charge());
}

#[test]
fn the_budget_rule_is_exact() {
    use OpKind::*;
    let t = Table(WaveId::W1);
    let n = FORMANT_N as usize;
    let mut k = [t; OPS];
    for slot in k.iter_mut().take(n) {
        *slot = Formant;
    }
    assert!(fits(&k));
    k[n] = Formant;
    assert!(!fits(&k));
    assert_eq!(
        fits(&[Noise; OPS]),
        6 * OpKind::Noise.charge() <= FORMANT_BUDGET.0
    );
}
