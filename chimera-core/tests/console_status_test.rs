mod screen;

use chimera_core::console::{state_word, write_status};
use chimera_core::part::DacPair;
use chimera_core::project::{PartId, ProjectStatus};
use chimera_core::ui::UiState;
use chimera_core::ui::about_page::{BUILD, VERSION};
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::nav::{MixPage, Rung};
use chimera_hal::ButtonId;
use screen::{Input, feed, modify, naming_save_as, tap, to_leaf};

fn status(ui: &UiState) -> String {
    let mut s = String::new();
    write_status(ui, &mut s).unwrap();
    s
}

fn line(s: &str, key: &str) -> String {
    s.lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("no {key} in {s}"))
        .to_string()
}

fn at(ui: &UiState) -> String {
    line(&status(ui), "at")
}

#[test]
fn status_has_its_lines_in_order() {
    let ui = Box::new(UiState::new());
    let s = status(&ui);
    let keys: Vec<_> = s.lines().map(|l| l.split(' ').next().unwrap()).collect();
    assert_eq!(
        keys,
        ["firmware", "protocol", "project", "state", "part", "at"]
    );
    assert_eq!(
        line(&s, "firmware"),
        format!("{VERSION} {}", BUILD.to_ascii_lowercase())
    );
    assert_eq!(line(&s, "protocol"), "1");
    assert_eq!(line(&s, "project"), ui.project().meta().name().as_str());
    assert_eq!(line(&s, "part"), "1");
    assert!(
        s.ends_with('\n') && !s.contains('\r') && s.is_ascii(),
        "{s}"
    );
}

#[test]
fn each_state_has_its_word() {
    assert_eq!(state_word(ProjectStatus::Pristine), "NEW");
    assert_eq!(state_word(ProjectStatus::Saved), "SAVED");
    assert_eq!(state_word(ProjectStatus::Modified), "MODIFIED");
}

#[test]
fn at_names_a_parts_page_by_its_header() {
    let mut ui = Box::new(UiState::new());
    ui.update(UiTick::for_test());
    assert!(matches!(ui.location().rung(), Rung::Pages(..)));
    let name = ui.page_name().expect("a page has a name");
    assert_eq!(at(&ui), format!("PART 1 > {}", name.as_str()));
    tap(&mut ui, ButtonId::B3);
    assert_eq!(line(&status(&ui), "part"), "3");
    let name = ui.page_name().expect("a page has a name");
    assert_eq!(at(&ui), format!("PART 3 > {}", name.as_str()));
}

#[test]
fn at_names_the_page_short_where_the_header_does() {
    let mut ui = Box::new(UiState::new());
    ui.update(UiTick::for_test());
    assert_eq!(at(&ui), "PART 1 > ALGORITHM");
    ui.project_mut().edit_part(PartId::ALL[0]).mix.output = DacPair::P2;
    assert_eq!(at(&ui), "PART 1 > ALG"); // beside OUT P2
}

#[test]
fn at_names_the_mixer_and_the_fx() {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B2));
    assert_eq!(
        ui.location().rung(),
        Rung::Mixer(PartId::ALL[1], MixPage::Sends)
    );
    assert_eq!(at(&ui), "MIXER 2 > SENDS");
    feed(&mut ui, Input::press(ButtonId::Minus));
    assert_eq!(at(&ui), "MIXER 2 > PART");
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert!(matches!(ui.location().rung(), Rung::Fx(..)));
    assert_eq!(at(&ui), "FX > CHORUS");
}

#[test]
fn at_names_the_sound_rung() {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B4));
    feed(&mut ui, Input::press(ButtonId::Edit)); // EDIT on the mixer: the Sound rung
    assert_eq!(ui.location().rung(), Rung::Sound(PartId::ALL[3]));
    assert_eq!(at(&ui), "SOUND 4");
    assert!(ui.page_name().is_none());
}

#[test]
fn at_in_settings_is_the_breadcrumb_as_drawn() {
    let mut ui = Box::new(UiState::new());
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "AUDIO LOAD"]);
    assert!(matches!(ui.location().rung(), Rung::Settings(_)));
    assert_eq!(at(&ui), "SETTINGS > SYSTEM > DIAG > AUD LOAD");
    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    assert_eq!(at(&ui), "SETTINGS > PERSONAL > THEME");
}

#[test]
fn at_includes_namings_crumb() {
    let mut ui = Box::new(UiState::new());
    naming_save_as(&mut ui);
    assert_eq!(at(&ui), "SETTINGS > PROJECT > SAVE AS");
}

#[test]
fn a_breadcrumb_drawn_short_is_written_in_full() {
    let mut ui = Box::new(UiState::new());
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "DEMO", "LVL"]);
    let drawn = format!("{}", ui.crumbs().unwrap());
    assert!(drawn.starts_with(".."), "{drawn}");
    assert_eq!(at(&ui), "SETTINGS > SYSTEM > DIAG > DEMO > LVL");
}

#[test]
fn state_follows_an_edit() {
    let mut ui = Box::new(UiState::new());
    ui.update(UiTick::for_test());
    assert_eq!(line(&status(&ui), "state"), "NEW");
    modify(&mut ui);
    assert_eq!(line(&status(&ui), "state"), "MODIFIED");
}

#[test]
fn page_name_is_the_upper_case_page() {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    assert_eq!(ui.page_name().unwrap().as_str(), "SENDS");
    feed(&mut ui, Input::press(ButtonId::Minus));
    assert_eq!(ui.page_name().unwrap().as_str(), "PART");
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS"]); // a list
    assert!(ui.page_name().is_none());
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "AUDIO LOAD"]); // a leaf: the breadcrumb, no header
    assert!(ui.page_name().is_none());
}
