//! `UiState` on one `Location`: MENU opens SETTINGS, taps act on release,
//! and a leaf edits what it names (ADR 0066).

mod screen;

use chimera_core::block::Block;
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::project::PartId;
use chimera_core::storage::{Card, SystemSync};
use chimera_core::ui::block_registry::{FILTER, MIXER_CHANNEL_CHAIN, MIXER_PART};
use chimera_core::ui::nav::{Location, MixPage, PageAt, home};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::region::RegionKind;
use chimera_core::ui::theme_settings::ThemeSettings;
use chimera_core::ui::{UiState, block_def::SlotBinding, page::PageKey};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const P: [PartId; 6] = PartId::ALL;

fn top() -> Location {
    Location::settings_at(&[], 0)
}

fn part_home(p: PartId) -> Location {
    Location::pages(p, home(EngineType::Algo))
}

/// From Part 2's home, PLUS to FILTER.
fn filter_on_part_2(ui: &mut UiState) -> Location {
    feed(ui, Input::press(ButtonId::B2));
    for _ in 0..3 {
        feed(ui, Input::press(ButtonId::Plus));
    }
    assert!(matches!(ui.page(), PageKey::Part { def, .. } if def == FILTER.id));
    ui.location()
}

#[test]
fn menu_tap_opens_settings_and_backs_out() {
    let mut ui = UiState::new();
    let filter = filter_on_part_2(&mut ui);
    assert_eq!(filter, Location::pages(P[1], PageAt { node: 3, sub: 0 }));
    tap(&mut ui, ButtonId::Menu);
    assert_eq!(ui.location(), top());
    assert!(ui.in_settings());
    tap(&mut ui, ButtonId::Menu);
    assert_eq!(ui.location(), filter);
    assert!(!ui.in_settings());
}

#[test]
fn menu_press_without_release_does_nothing() {
    let mut ui = UiState::new();
    let at = ui.location();
    feed(&mut ui, Input::press(ButtonId::Menu));
    assert_eq!(ui.location(), at);
    feed(&mut ui, Input::held(ButtonId::Menu).at(100));
    assert_eq!(ui.location(), at);
}

#[test]
fn seq_tap_is_sub_page_up_on_release() {
    let mut ui = UiState::new();
    filter_on_part_2(&mut ui);
    feed(&mut ui, Input::press(ButtonId::Edit));
    let down = Location::pages(P[1], PageAt { node: 3, sub: 1 });
    assert_eq!(ui.location(), down);
    feed(&mut ui, Input::press(ButtonId::Seq));
    assert_eq!(ui.location(), down);
    feed(&mut ui, Input::release(ButtonId::Seq).at(100));
    assert_eq!(
        ui.location(),
        Location::pages(P[1], PageAt { node: 3, sub: 0 })
    );
}

#[test]
fn bn_from_a_settings_leaf_lands_on_part_pages_and_syncs_system() {
    let mut s = chimera_hal::testkit::MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    let mut ui = UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut set);
    assert!(ui.in_settings());
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    ui.sync_system(&mut sync, &mut card, &mut s, &mut set);
    let edited = ui.theme();
    assert_ne!(edited, ThemeSettings::DEFAULT);
    feed(&mut ui, Input::press(ButtonId::B2));
    ui.sync_system(&mut sync, &mut card, &mut s, &mut set);
    assert!(!ui.in_settings());
    assert_eq!(ui.location(), part_home(P[1]));
    let (_, saved, _) = SystemSync::boot(&mut Card::new(), &mut s);
    assert_eq!(saved.theme, edited);
}

#[test]
fn theme_leaf_edits_ease() {
    let mut ui = UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    settle(&mut ui);
    let from = ui.renderer.anim[0].current();
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    ui.update();
    let to = ui.theme().normalized(ThemeSettings::BRIGHT);
    let now = ui.renderer.anim[0].current();
    assert_ne!(from, to);
    assert!(now != to && now != from, "{from} → {now} → {to}");
}

#[test]
fn channels_mirror_edits_the_mixer_value() {
    let mut ui = UiState::new();
    let ch = |ui: &UiState| ui.project().part(P[2]).mix.get(PartParams::CHANNEL);
    let before = ch(&ui);
    to_leaf(&mut ui, &["MIDI CONFIG", "CHANNELS"]);
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    assert_eq!(ch(&ui), before + 1.0);
    assert_eq!(ui.project().part(P[0]).mix.get(PartParams::CHANNEL), 0.0);

    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B3));
    feed(&mut ui, Input::press(ButtonId::Minus));
    assert_eq!(ui.location(), Location::mixer(P[2], MixPage::Part));
    settle(&mut ui);
    let part = MIXER_CHANNEL_CHAIN.blocks[MIXER_PART].def;
    let slot = part
        .params
        .iter()
        .position(|s| matches!(s.binding, SlotBinding::Param(a) if a.param == PartParams::CHANNEL))
        .unwrap();
    let shown = ui.renderer.anim[slot].current();
    let stored = ui.project().part(P[2]).mix.normalized(PartParams::CHANNEL);
    assert!((shown - stored).abs() < 1e-6, "{shown} vs {stored}");
    assert_eq!(ch(&ui), 3.0, "CHANNEL 4");
}

#[test]
fn mix_b6_is_part_6_mixer() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B6));
    assert_eq!(ui.location(), Location::mixer(P[5], MixPage::Sends));
    assert_eq!(ui.active_part, P[5]);
}

#[test]
fn edit_on_mixer_opens_sound_and_seq_opens_part_settings() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.location(), Location::sound(P[1]));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B2));
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.location(), Location::settings_at(&[1], 0));
    assert_eq!(ui.active_part, P[1]);
}

#[test]
fn no_press_lost_in_a_stalled_frame() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::tap_in_frame(ButtonId::B3));
    assert_eq!(ui.location(), part_home(P[2]));
    feed(&mut ui, Input::tap_in_frame(ButtonId::Menu));
    assert_eq!(ui.location(), top());
}

#[test]
fn mix_menu_does_not_open_settings() {
    let mut ui = UiState::new();
    let at = ui.location();
    feed(
        &mut ui,
        Input::tap_in_frame(ButtonId::Menu).and_held(ButtonId::Mix),
    );
    assert_eq!(ui.location(), at);
}

#[test]
fn settings_keys_follow_the_bar_and_the_status() {
    let mut ui = UiState::new();
    let mut fb = Fb::new();
    let mut draw = |ui: &mut UiState| {
        ui.update();
        ui.render_dirty_with_audio(&mut fb, &PerfStats::zero(), None, &scope_fixture());
    };
    tap(&mut ui, ButtonId::Menu);
    draw(&mut ui);
    let key = |ui: &UiState, k| ui.drawn_key(k).unwrap();
    let (list, footer) = (key(&ui, RegionKind::List), key(&ui, RegionKind::Footer));

    feed(&mut ui, Input::turn(EncoderId::A, 1));
    draw(&mut ui);
    assert_ne!(key(&ui, RegionKind::List), list, "bar");
    assert_eq!(key(&ui, RegionKind::Footer), footer);

    ui.project_mut()
        .edit_part(P[0])
        .mix
        .set(PartParams::LEVEL, 0.25);
    draw(&mut ui);
    assert_ne!(key(&ui, RegionKind::Footer), footer, "status");
}
