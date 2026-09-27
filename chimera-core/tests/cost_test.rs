//! CPU costs (ADR 0013): cycles/sample per voice, measured on the bench
//! (`bench-results.md`, rev V at 480 MHz) and rounded up to the next 10.

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, Cost, MAX_VOICES, SampleBudget};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};

const UNROUTED: [bool; OPS] = [false; OPS];

fn cost(p: &AlgoParams) -> u32 {
    AlgoEngine::cost(p, &UNROUTED).0
}

fn voice_cost(e: EngineType) -> Cost {
    Voice::cost(&ParamSnapshot::for_engine(e), &ModState::new())
}

/// The bench's worst case: A14 and A22 mid-MORPH, six operators, all with
/// feedback.
fn worst() -> AlgoParams {
    let mut p = AlgoParams::default();
    (p.alg_a, p.alg_b, p.morph) = (AlgoId::A14.get(), AlgoId::A22.get(), 64);
    for op in p.ops.iter_mut() {
        (op.level, op.feedback) = (99, 7);
    }
    p
}

/// `bench-results.md`: Modal 379 cycles/sample per voice, minus the chain
/// and rounded up to the next 10. Algo is priced from its patch.
#[test]
fn voice_costs_are_the_bench_measurements() {
    assert_eq!(Voice::CHAIN_COST, Cost(10));
    assert_eq!(voice_cost(EngineType::Modal), Cost(380));
    let mods = ModState::new();
    for e in EngineType::ALL {
        let p = ParamSnapshot::for_engine(e);
        assert_eq!(
            Voice::cost(&p, &mods),
            Engines::cost(&p, &mods) + Voice::CHAIN_COST,
            "{e:?}"
        );
    }
}

/// What the 7,000-cycle budget allows with the FX bus (MidiVerb, the
/// costliest reverb) running: as many voices of each engine's default
/// Sound as fit, up to `MAX_VOICES`.
#[test]
fn budget_capacity_per_engine() {
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost();
    let fits = |e: EngineType, n: u32| FxBus::COST.0 + n * voice_cost(e).0 <= budget.0;
    for e in EngineType::ALL {
        let k = ((budget.0 - FxBus::COST.0) / voice_cost(e).0).min(MAX_VOICES as u32);
        assert!(fits(e, k), "{e:?} should fit {k}");
        if k < MAX_VOICES as u32 {
            assert!(!fits(e, k + 1), "{e:?} should not fit {}", k + 1);
        }
    }
}

/// A14 ∪ A22 is eight links.
#[test]
fn the_worst_case_is_the_sum_of_its_terms() {
    let sum = AlgoEngine::COST_BASE.0
        + 6 * AlgoEngine::COST_OP.0
        + 8 * AlgoEngine::COST_LINK.0
        + 6 * AlgoEngine::COST_FEEDBACK.0;
    assert_eq!(cost(&worst()), sum);
    assert_eq!(sum, 560, "the provisional terms keep the old flat estimate");
}

#[test]
fn a_single_operator_is_much_cheaper_than_the_worst_case() {
    let one = AlgoParams::single(chimera_core::dsp::algo::waves::WaveId::W1);
    let (one, worst) = (cost(&one), cost(&worst()));
    assert!(3 * one < worst, "{one} vs {worst}");
}

/// MORPH does not matter: the plan runs the union's links at any MORPH.
#[test]
fn morph_does_not_change_the_cost() {
    let mut p = worst();
    let mid = cost(&p);
    for m in [0, 127] {
        p.morph = m;
        assert_eq!(cost(&p), mid);
    }
}

/// Every single step towards a bigger patch (an operator, feedback, a link)
/// never lowers the cost, from every algorithm pair.
#[test]
fn cost_is_monotonic_in_operators_links_and_feedback() {
    let base = AlgoParams::default();
    for a in 0..32u8 {
        for b in 0..32u8 {
            let mut p = base;
            (p.alg_a, p.alg_b) = (a, b);
            for op in p.ops.iter_mut() {
                (op.level, op.feedback) = (0, 0);
            }
            for i in 0..OPS {
                let before = cost(&p);
                p.ops[i].level = 1;
                let lit = cost(&p);
                assert!(lit > before, "op {i} on {a}/{b}");
                p.ops[i].feedback = 1;
                assert!(cost(&p) > lit, "fb {i} on {a}/{b}");
            }
        }
    }
    // A link: A1 has none, A2 adds 6→1, A3 adds 6→2 as well; with B held,
    // a superset of links in A never costs less.
    let mut p = worst();
    for (from, to) in [(AlgoId::A1, AlgoId::A2), (AlgoId::A2, AlgoId::A3)] {
        (p.alg_a, p.alg_b) = (from.get(), from.get());
        let fewer = cost(&p);
        p.alg_a = to.get();
        assert!(cost(&p) > fewer, "{from:?} to {to:?}");
    }
}

/// Links are priced by their target: A17 is 6→5→4→3→2→1, so silencing
/// operator 1 drops the link 2→1, and silencing operator 6 (a source only)
/// drops none.
#[test]
fn links_are_priced_by_their_active_target() {
    let mut all = worst();
    (all.alg_a, all.alg_b) = (AlgoId::A17.get(), AlgoId::A17.get());
    for op in all.ops.iter_mut() {
        op.feedback = 0;
    }
    let (op, link) = (AlgoEngine::COST_OP.0, AlgoEngine::COST_LINK.0);
    let full = cost(&all);
    assert_eq!(full, AlgoEngine::COST_BASE.0 + 6 * op + 5 * link);
    let mut no_1 = all;
    no_1.ops[0].level = 0;
    assert_eq!(full - cost(&no_1), op + link, "4 links");
    let mut no_6 = all;
    no_6.ops[5].level = 0;
    assert_eq!(full - cost(&no_6), op, "5 links");
}

/// A route on a silent operator's LEVEL may lift it, so it is priced.
#[test]
fn a_routed_silent_operator_is_priced() {
    let mut p = worst();
    p.ops[5].level = 0;
    let mut routed = UNROUTED;
    let bare = AlgoEngine::cost(&p, &routed).0;
    routed[5] = true;
    assert_eq!(AlgoEngine::cost(&p, &routed).0, cost(&worst()));
    assert!(bare < cost(&worst()));
}
