//! The UI's door carries more than TAPE (MAX VOICES, CHOKE, the band's
//! notes, the encoders' feel): a host build without `tape` still answers
//! through it (#829).

#[cfg(not(feature = "bank2"))]
#[test]
fn the_ui_door_answers_without_tape() {
    assert!(chimera_core::bank2::ui_door(|_| ()).is_some());
    assert!(!chimera_core::bank2::ui_closed());
}
