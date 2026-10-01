//! The Part button toggles sound and mixer; the mixer opens on SENDS (ADR 0057).

mod screen;

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::part::DacPair;
use chimera_core::project::{PartFrom, PartId, PartSource};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{ALGO_ALG, ALGO_WAVE, CHORUS, FILTER, PART, SENDS};
use chimera_core::ui::chain::{ChainId, next_on_part_button};
use chimera_core::ui::components::header_text;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, settle};

fn mix(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::chord(ButtonId::Mix, b));
}

fn press(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::press(b));
}

fn at(ui: &UiState) -> (ChainId, u16) {
    (ui.nav.chain_id, ui.nav.active_block_def().id)
}

/// The owner's repro: MIX+B1, C +1 turned REV, not OUT.
#[test]
fn mix_b1_then_c_turns_the_reverb_send_not_out() {
    let mut ui = UiState::new();
    mix(&mut ui, ButtonId::B1);
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    settle(&mut ui);
    let p = &ui.project().part(PartId::ALL[0]).mix;
    assert!(p.sends[2] > 0.0, "REV moved");
    assert_eq!(p.output, DacPair::P1, "OUT stays P1");
}

#[test]
fn the_toggle_is_a_pure_function_of_where_you_are() {
    use ChainId::*;
    for n in 0..6 {
        assert_eq!(next_on_part_button(Part(n), n), Mixer(n));
        assert_eq!(next_on_part_button(Mixer(n), n), Part(n));
        assert_eq!(next_on_part_button(System, n), Part(n));
        assert_eq!(next_on_part_button(Demo, n), Part(n));
        let other = (n + 1) % 6;
        assert_eq!(next_on_part_button(Part(other), n), Part(n));
        assert_eq!(next_on_part_button(Mixer(other), n), Part(n));
    }
}

#[test]
fn b_n_toggles_sound_and_mixer_and_returns_to_the_page_left() {
    let mut ui = UiState::new();
    for _ in 0..3 {
        press(&mut ui, ButtonId::Plus); // → FLT
    }
    assert_eq!(at(&ui), (ChainId::Part(0), FILTER.id));
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (ChainId::Mixer(0), SENDS.id));
    assert_eq!(ui.active_part, PartId::ALL[0]);
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (ChainId::Part(0), FILTER.id), "the page left");
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (ChainId::Mixer(0), SENDS.id));
}

#[test]
fn another_parts_button_lands_on_its_home() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::B2);
    assert_eq!(at(&ui), (ChainId::Part(1), ALGO_ALG.id));
    press(&mut ui, ButtonId::Plus);
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        at(&ui),
        (ChainId::Part(0), ALGO_ALG.id),
        "Part 2's pages → Part 1's"
    );
    assert_eq!(ui.active_part, PartId::ALL[0]);
    mix(&mut ui, ButtonId::B2);
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        at(&ui),
        (ChainId::Part(0), ALGO_ALG.id),
        "Part 2's mixer → Part 1's"
    );
    press(&mut ui, ButtonId::Menu);
    press(&mut ui, ButtonId::B3);
    assert_eq!(
        at(&ui),
        (ChainId::Part(2), ALGO_ALG.id),
        "System → Part 3's"
    );
}

/// The mixer page is remembered for the session, for every Part.
#[test]
fn the_mixer_reopens_on_the_page_last_used() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::Plus); // SENDS → CHORUS
    assert_eq!(at(&ui), (ChainId::Mixer(0), CHORUS.id));
    press(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (ChainId::Mixer(0), CHORUS.id));
    mix(&mut ui, ButtonId::B2);
    assert_eq!(at(&ui), (ChainId::Mixer(1), CHORUS.id), "global");
}

/// PART is remembered only from mixer to mixer (balancing LEVEL and PAN);
/// arriving from outside, a remembered PART opens SENDS, where C is REV.
#[test]
fn the_mixer_never_reopens_on_part_from_outside() {
    let mut ui = UiState::new();
    mix(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::Minus);
    assert_eq!(at(&ui), (ChainId::Mixer(0), PART.id));
    mix(&mut ui, ButtonId::B2);
    assert_eq!(at(&ui), (ChainId::Mixer(1), PART.id), "mixer to mixer");
    for (leave, back) in [
        (ButtonId::Menu, Input::chord(ButtonId::Mix, ButtonId::B1)),
        (ButtonId::B2, Input::press(ButtonId::B2)), // its sound pages, and back
        (ButtonId::B1, Input::chord(ButtonId::Mix, ButtonId::B1)),
    ] {
        mix(&mut ui, ButtonId::B2);
        press(&mut ui, ButtonId::Minus);
        press(&mut ui, ButtonId::Minus);
        assert_eq!(ui.nav.active_block_def().id, PART.id);
        press(&mut ui, leave);
        feed(&mut ui, back);
        assert_eq!(ui.nav.active_block_def().id, SENDS.id, "{leave:?}");
    }
    mix(&mut ui, ButtonId::B6);
    mix(&mut ui, ButtonId::B1);
    assert_eq!(ui.nav.active_block_def().id, SENDS.id, "from Demo");
}

/// The page left is kept with its engine: a changed engine lands home.
#[test]
fn a_changed_engine_returns_home_not_to_the_page_left() {
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::MODAL_EXC;
    let mut ui = UiState::new();
    for _ in 0..3 {
        press(&mut ui, ButtonId::Plus); // → FLT
    }
    press(&mut ui, ButtonId::B1); // mixer
    ui.project_mut()
        .load_part(PartSource {
            part: PartId::ALL[0],
            from: PartFrom::Init(EngineType::Modal),
        })
        .unwrap();
    press(&mut ui, ButtonId::B1);
    assert_eq!(ui.nav.engine, EngineType::Modal);
    assert_eq!(at(&ui), (ChainId::Part(0), MODAL_EXC.id));
    assert_eq!((ui.nav.node, ui.nav.sub_page), (0, 0));
}

/// Turning OUT redraws the header on the dirty-render path.
#[test]
fn the_out_warning_redraws_on_the_dirty_path() {
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme;
    use screen::{Fb, scope_fixture};
    let mut ui = UiState::new();
    mix(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::Minus); // PART
    settle(&mut ui);
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_with_scope(&mut fb, &perf, &scope);
    ui.prime_regions(&perf, None, &scope);
    let band = ..theme::HEADER_BOTTOM as usize * 240;
    let before = fb.px[band].to_vec();
    for delta in [1, 1, -2] {
        feed(&mut ui, Input::turn(EncoderId::C, delta));
        settle(&mut ui);
        ui.render_dirty_with_scope(&mut fb, &perf, &scope);
        let mut full = Fb::new();
        ui.render_with_scope(&mut full, &perf, &scope);
        assert!(fb.px[band] == full.px[band], "stale header at {delta}");
    }
    assert!(fb.px[band] == before[..], "the warning is gone back on P1");
}

#[test]
fn mix_b_is_still_a_shortcut_to_the_mixer() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::Plus);
    mix(&mut ui, ButtonId::B3);
    assert_eq!(at(&ui), (ChainId::Mixer(2), SENDS.id));
    assert_eq!(ui.active_part, PartId::ALL[2]);
    mix(&mut ui, ButtonId::B3);
    assert_eq!(at(&ui), (ChainId::Mixer(2), SENDS.id), "again: stays");
    mix(&mut ui, ButtonId::B6);
    assert_eq!(ui.nav.chain_id, ChainId::Demo);
    assert_eq!(ui.nav.node, 0);
    mix(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        at(&ui),
        (ChainId::Part(0), ALGO_WAVE.id),
        "the Part 1 page left, through the demo"
    );
}

fn header(ui: &UiState) -> (String, String, Option<&'static str>) {
    let out = ui.project().part(ui.active_part).mix.output;
    let h = header_text(
        &ui.nav,
        ui.nav.active_block_def(),
        ResonatorMode::String,
        "",
        out,
    );
    (h.context.as_str().into(), h.name.as_str().into(), h.warn)
}

#[test]
fn the_header_says_sound_or_mix_and_warns_off_p1() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::B2);
    assert_eq!(
        header(&ui),
        ("PART 2 · SOUND".into(), "ALGORITHM".into(), None)
    );
    press(&mut ui, ButtonId::B2);
    assert_eq!(header(&ui), ("PART 2 · MIX".into(), "SENDS".into(), None));
    press(&mut ui, ButtonId::Minus); // PART
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // OUT → P2
    settle(&mut ui);
    assert_eq!(ui.project().part(PartId::ALL[1]).mix.output, DacPair::P2);
    assert_eq!(
        header(&ui),
        ("PART 2 · MIX".into(), "PART".into(), Some("OUT P2"))
    );
    press(&mut ui, ButtonId::B2);
    assert_eq!(header(&ui).2, Some("OUT P2"), "on the sound pages too");
    assert_eq!(header(&ui).0, "PART 2 · SOUND");
    press(&mut ui, ButtonId::B1);
    assert_eq!(header(&ui).2, None, "Part 1 is on P1");
    press(&mut ui, ButtonId::Menu);
    assert_eq!(header(&ui).2, None, "System has no OUT");
    mix(&mut ui, ButtonId::B2); // SENDS: PART isn't kept from outside
    press(&mut ui, ButtonId::Minus);
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // → P3
    settle(&mut ui);
    assert_eq!(header(&ui).2, Some("OUT P3"));
    feed(&mut ui, Input::turn(EncoderId::C, -2)); // → P1
    settle(&mut ui);
    assert_eq!(header(&ui).2, None, "gone back on P1");
}
