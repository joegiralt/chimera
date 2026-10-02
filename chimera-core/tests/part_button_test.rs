//! The Part button toggles sound and mixer; the mixer opens on SENDS (ADR 0057).

mod screen;

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::part::DacPair;
use chimera_core::project::{PartFrom, PartId, PartSource, ReplaceGuard};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{ALGO_ALG, ALGO_WAVE, CHORUS, FILTER, PART, SENDS};
use chimera_core::ui::components::{Head, header_text};
use chimera_core::ui::nav::{Location, chain_def_for};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, settle, tap};

fn mix(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::chord(ButtonId::Mix, b));
}

fn press(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::press(b));
}

/// Whose page is shown, by 0-based Part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum On {
    Sound(usize),
    Mix(usize),
    Settings,
}

fn on(l: Location) -> On {
    match (l.settings(), l.part()) {
        (None, Some(p)) if l.on_mixer() => On::Mix(p.index()),
        (None, Some(p)) => On::Sound(p.index()),
        _ => On::Settings,
    }
}

fn head(l: Location) -> Head {
    match on(l) {
        On::Sound(n) => Head::Sound(PartId::ALL[n]),
        On::Mix(n) => Head::Mix(PartId::ALL[n]),
        On::Settings => Head::Settings,
    }
}

fn at(ui: &UiState) -> (On, u16) {
    (on(ui.location()), ui.page_def().id)
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
fn b_n_toggles_sound_and_mixer_and_returns_to_the_page_left() {
    let mut ui = UiState::new();
    for _ in 0..3 {
        press(&mut ui, ButtonId::Plus); // → FLT
    }
    assert_eq!(at(&ui), (On::Sound(0), FILTER.id));
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (On::Mix(0), SENDS.id));
    assert_eq!(ui.active_part, PartId::ALL[0]);
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (On::Sound(0), FILTER.id), "the page left");
    press(&mut ui, ButtonId::B1);
    assert_eq!(at(&ui), (On::Mix(0), SENDS.id));
}

#[test]
fn another_parts_button_lands_on_its_home() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::B2);
    assert_eq!(at(&ui), (On::Sound(1), ALGO_ALG.id));
    press(&mut ui, ButtonId::Plus);
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        at(&ui),
        (On::Sound(0), ALGO_ALG.id),
        "Part 2's pages → Part 1's"
    );
    assert_eq!(ui.active_part, PartId::ALL[0]);
    mix(&mut ui, ButtonId::B2);
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        at(&ui),
        (On::Sound(0), ALGO_ALG.id),
        "Part 2's mixer → Part 1's"
    );
    tap(&mut ui, ButtonId::Menu);
    assert_eq!(at(&ui).0, On::Settings);
    press(&mut ui, ButtonId::B3);
    assert_eq!(at(&ui), (On::Sound(2), ALGO_ALG.id), "SETTINGS → Part 3's");
}

/// The FX page left is remembered for the session, for every Part.
#[test]
fn the_mixer_reopens_on_the_page_last_used() {
    use chimera_core::ui::block_registry::MIXER_CHANNEL_CHAIN;
    let mut ui = UiState::new();
    let chorus = MIXER_CHANNEL_CHAIN
        .blocks
        .iter()
        .position(|b| b.def.id == CHORUS.id)
        .unwrap();
    screen::to_fx(&mut ui, chorus); // Part 6's FX
    assert_eq!(at(&ui), (On::Mix(5), CHORUS.id));
    press(&mut ui, ButtonId::B6);
    press(&mut ui, ButtonId::B6);
    assert_eq!(at(&ui), (On::Mix(5), CHORUS.id));
    mix(&mut ui, ButtonId::B2);
    assert_eq!(at(&ui), (On::Mix(1), CHORUS.id), "global");
}

/// PART is remembered only from mixer to mixer (balancing LEVEL and PAN);
/// arriving from outside, a remembered PART opens SENDS, where C is REV.
#[test]
fn the_mixer_never_reopens_on_part_from_outside() {
    let mut ui = UiState::new();
    mix(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::Minus);
    assert_eq!(at(&ui), (On::Mix(0), PART.id));
    mix(&mut ui, ButtonId::B2);
    assert_eq!(at(&ui), (On::Mix(1), PART.id), "mixer to mixer");
    for (leave, back) in [
        (ButtonId::Menu, Input::chord(ButtonId::Mix, ButtonId::B1)),
        (ButtonId::B2, Input::press(ButtonId::B2)), // its sound pages, and back
        (ButtonId::B1, Input::chord(ButtonId::Mix, ButtonId::B1)),
    ] {
        mix(&mut ui, ButtonId::B2);
        if ui.page_def().id == SENDS.id {
            press(&mut ui, ButtonId::Minus); // Part 2's SENDS → its PART
        }
        assert_eq!(at(&ui), (On::Mix(1), PART.id));
        if leave == ButtonId::Menu {
            tap(&mut ui, leave);
        } else {
            press(&mut ui, leave);
        }
        feed(&mut ui, back);
        assert_eq!(ui.page_def().id, SENDS.id, "{leave:?}");
    }
    #[cfg(debug_assertions)]
    {
        screen::to_demo(&mut ui, 0);
        mix(&mut ui, ButtonId::B1);
        assert_eq!(ui.page_def().id, SENDS.id, "from DEMO");
    }
}

/// The page left is kept with its engine: a changed engine lands home.
#[test]
fn a_changed_engine_returns_home_not_to_the_page_left() {
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::MODAL_1;
    let mut ui = UiState::new();
    for _ in 0..3 {
        press(&mut ui, ButtonId::Plus); // → FLT
    }
    press(&mut ui, ButtonId::B1); // mixer
    let src = PartSource {
        part: PartId::ALL[0],
        from: PartFrom::Init(EngineType::Modal),
    };
    let c = ReplaceGuard::check(ui.project(), ui.template(), src).unwrap();
    ui.project_mut().replace_part(c).unwrap();
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        ui.location(),
        Location::pages(PartId::ALL[0], chain_def_for(EngineType::Modal).home())
    );
    assert_eq!(at(&ui), (On::Sound(0), MODAL_1.id), "Modal's home is RES");
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
    assert_eq!(at(&ui), (On::Mix(2), SENDS.id));
    assert_eq!(ui.active_part, PartId::ALL[2]);
    mix(&mut ui, ButtonId::B3);
    assert_eq!(at(&ui), (On::Mix(2), SENDS.id), "again: stays");
    mix(&mut ui, ButtonId::B6);
    assert_eq!(at(&ui), (On::Mix(5), SENDS.id), "MIX+B6 is Part 6's");
    mix(&mut ui, ButtonId::B1);
    press(&mut ui, ButtonId::B1);
    assert_eq!(
        at(&ui),
        (On::Sound(0), ALGO_WAVE.id),
        "the Part 1 page left, through Part 6's mixer"
    );
}

fn header(ui: &UiState) -> (String, String, Option<&'static str>) {
    let out = ui.project().part(ui.active_part).mix.output;
    let h = header_text(
        head(ui.location()),
        ui.page_def(),
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
    tap(&mut ui, ButtonId::Menu);
    assert_eq!(header(&ui).2, None, "SETTINGS has no OUT");
    mix(&mut ui, ButtonId::B2); // SENDS: PART isn't kept from outside
    press(&mut ui, ButtonId::Minus);
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // → P3
    settle(&mut ui);
    assert_eq!(header(&ui).2, Some("OUT P3"));
    feed(&mut ui, Input::turn(EncoderId::C, -2)); // → P1
    settle(&mut ui);
    assert_eq!(header(&ui).2, None, "gone back on P1");
}
