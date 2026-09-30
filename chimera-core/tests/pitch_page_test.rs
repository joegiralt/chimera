//! The PIT page (#162): PITCH on A, FINE on B, C–F free for GLIDE and SLEW.

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::params::{EngineType, PitchParams};
use chimera_core::ui::block_def::SlotBinding;
use chimera_core::ui::block_registry::PITCH;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

#[test]
fn pitch_is_on_a_and_fine_on_b_and_the_rest_is_free() {
    let at = |i: usize| PITCH.params[i].binding;
    assert_eq!(
        at(0),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH))
    );
    assert_eq!(
        at(1),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::FINE))
    );
    for i in 2..6 {
        assert_eq!(at(i), SlotBinding::Empty, "slot {i}");
    }
}

#[test]
fn both_engines_reach_the_pitch_page_and_turn_it() {
    for engine in EngineType::ALL {
        let mut ui = chimera_core::ui::UiState::new();
        load_init(&mut ui, engine);
        to_pitch(&mut ui, engine);
        feed(&mut ui, Input::turn(EncoderId::A, 12));
        feed(&mut ui, Input::turn(EncoderId::B, -30));
        let p = &ui.params().pitch;
        assert_eq!((p.pitch, p.fine), (12.0, -30.0), "{engine:?}");
    }
}

#[test]
fn mix_plus_primes_pitch_for_the_matrix() {
    let mut ui = chimera_core::ui::UiState::new();
    load_init(&mut ui, EngineType::Modal);
    to_pitch(&mut ui, EngineType::Modal);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
    assert!(ui.mod_state().find(pitch).is_some());
}
