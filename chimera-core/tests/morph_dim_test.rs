//! #188: MORPH is inert while ALG A = ALG B, so it draws dimmed, refuses
//! priming, and a route into it draws inert in the matrix (ADR 0049).

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::algorithms::ALGO_COUNT;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::CUTOFF;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::ui::mod_grid::{MatrixState, inert_dests};
use chimera_core::ui::{PrimeStatus, UiState, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const MORPH: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);

#[test]
fn morph_is_dimmed_iff_alg_a_equals_alg_b() {
    let mut s = Sound::init(EngineType::Algo);
    assert!(
        !view::dimmed(MORPH, &s),
        "INIT's ALG B differs from its ALG A"
    );
    for a in 0..ALGO_COUNT as u8 {
        for b in 0..ALGO_COUNT as u8 {
            (s.params.algo.alg_a, s.params.algo.alg_b) = (a, b);
            assert_eq!(view::dimmed(MORPH, &s), a == b, "{a} {b}");
        }
    }
}

/// Part 1's home is the ALG page: ALG A, ALG B, MORPH on encoders A–C.
#[test]
fn priming_morph_is_refused_while_it_is_dimmed() {
    let mut ui = UiState::new();
    let (a, b) = (ui.params().algo.alg_a, ui.params().algo.alg_b);
    feed(&mut ui, Input::turn(EncoderId::B, a as i8 - b as i8)); // ALG B = ALG A
    assert_eq!(ui.params().algo.alg_b, a);
    feed(&mut ui, Input::turn(EncoderId::C, 10)); // focus MORPH; ignored
    assert_eq!(ui.params().algo.morph, 0);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::NotModulatable));
    assert!(ui.mod_state().find(MORPH).is_none());

    feed(&mut ui, Input::turn(EncoderId::B, 1)); // A ≠ B: MORPH is live
    feed(&mut ui, Input::turn(EncoderId::C, 10));
    assert_eq!(ui.params().algo.morph, 10);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));
}

/// A route primed while MORPH was live stays, drawn inert once A = B.
#[test]
fn a_route_into_morph_is_inert_while_a_equals_b() {
    let mut s = Sound::init(EngineType::Algo);
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF, *b"CUTOFF\0\0").unwrap();
    reg.add(MORPH, *b"ALGMORPH").unwrap();
    let mut m = MatrixState::new();
    m.rebuild_dests_from_registry(&reg);
    assert_eq!(inert_dests(&m, &s), 0);
    s.params.algo.alg_b = s.params.algo.alg_a;
    assert_eq!(inert_dests(&m, &s), 0b10, "MORPH, column 2");
}
