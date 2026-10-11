//! The PIT page (#162): PITCH on A, FINE on B, STEAL on C and its GLIDE
//! TIME on D (#254), SNAP on E (TAPE Task 12b), F free.

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::params::{EngineType, PitchParams};
use chimera_core::ui::block_def::SlotBinding;
use chimera_core::ui::block_registry::PITCH;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

#[test]
fn pitch_fine_steal_time_and_snap_on_a_to_e_and_f_free() {
    let at = |i: usize| PITCH.params[i].binding;
    assert_eq!(
        at(0),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH))
    );
    assert_eq!(
        at(1),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::FINE))
    );
    assert_eq!(
        at(2),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::STEAL))
    );
    assert_eq!(
        at(3),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::GLIDE_TIME))
    );
    assert_eq!(
        at(4),
        SlotBinding::Param(ParamAddr::new(BlockRef::Pitch, PitchParams::SNAP))
    );
    assert_eq!(at(5), SlotBinding::Empty);
}

/// SNAP MUSICAL on E: PITCH's encoder steps fifths and fourths, on every
/// engine.
#[test]
fn snap_musical_steps_pitch_on_every_engine() {
    use chimera_core::params::Snap;
    for engine in EngineType::ALL {
        let mut ui = chimera_core::ui::UiState::new();
        load_init(&mut ui, engine);
        to_pitch(&mut ui, engine);
        assert_eq!(ui.params().pitch.snap, Snap::Chromatic);
        feed(&mut ui, Input::turn(EncoderId::E, 1));
        assert_eq!(ui.params().pitch.snap, Snap::Musical, "{engine:?}");
        feed(&mut ui, Input::turn(EncoderId::A, 1));
        assert_eq!(ui.params().pitch.pitch, 7.0, "{engine:?}");
        feed(&mut ui, Input::turn(EncoderId::A, 1));
        assert_eq!(ui.params().pitch.pitch, 12.0, "{engine:?}");
        feed(&mut ui, Input::turn(EncoderId::A, -3));
        assert_eq!(ui.params().pitch.pitch, -7.0, "{engine:?}");
    }
}

/// STEAL turns CUT to GLIDE; TIME reads 150 ms at INIT and runs 1 ms to 2 s.
#[test]
fn steal_and_glide_time_turn() {
    use chimera_core::params::Steal;
    for engine in EngineType::ALL {
        let mut ui = chimera_core::ui::UiState::new();
        load_init(&mut ui, engine);
        to_pitch(&mut ui, engine);
        assert_eq!(ui.params().pitch.steal, Steal::Cut);
        assert!((ui.params().pitch.glide_secs() - 0.150).abs() < 1e-4);
        feed(&mut ui, Input::turn(EncoderId::C, 1));
        assert_eq!(ui.params().pitch.steal, Steal::Glide, "{engine:?}");
        feed(&mut ui, Input::turn(EncoderId::D, 127));
        assert!((ui.params().pitch.glide_secs() - 2.0).abs() < 1e-3);
        feed(&mut ui, Input::turn(EncoderId::D, -127));
        feed(&mut ui, Input::turn(EncoderId::D, -127));
        assert!((ui.params().pitch.glide_secs() - 0.001).abs() < 1e-5);
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
