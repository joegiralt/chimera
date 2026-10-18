//! Any encoder moves a list's bar, one row per detent, and edits nothing
//! (#561). MIDI MAP keeps B, C and D for the row's values; the browser keeps
//! B for its tabs.

mod screen;

use chimera_core::ui::UiState;
use chimera_core::ui::browser;
use chimera_core::ui::nav::Browse;
use chimera_core::ui::settings::{Kind, rows};
use chimera_hal::{ALL_ENCODERS, ButtonId, EncoderId};
use screen::{Input, Rig, feed, three};

fn row(ui: &UiState) -> u8 {
    ui.location().settings().unwrap().row()
}

/// Each encoder in `moving` takes the bar down a row and back; the project
/// stays as it was.
fn bar_follows(ui: &mut UiState, moving: &[EncoderId]) {
    let rev = ui.project().rev();
    for &e in moving {
        let at = row(ui);
        feed(ui, Input::turn(e, 1));
        assert_eq!(row(ui), at + 1, "{e:?} down");
        feed(ui, Input::turn(e, -1));
        assert_eq!(row(ui), at, "{e:?} up");
    }
    assert_eq!(ui.project().rev(), rev);
}

#[test]
fn every_encoder_moves_the_settings_rows() {
    let mut ui = UiState::new();
    screen::tap(&mut ui, ButtonId::Menu);
    bar_follows(&mut ui, &ALL_ENCODERS);
}

#[test]
fn every_encoder_moves_the_project_lists() {
    for labels in [
        &["PROJECT"][..],
        &["PROJECT", "LOAD PROJECT"],
        &["PROJECT", "MANAGE PROJECTS"],
        &["PART", "SAVE TO PROJ"],
    ] {
        let (s, _) = three();
        let mut ui = UiState::new();
        let mut r = Rig::new(&mut ui, s);
        r.to(labels);
        bar_follows(r.ui, &ALL_ENCODERS);
    }
}

/// Every SETTINGS list with three rows or more, as label paths.
fn lists(path: &mut Vec<u8>, labels: &mut Vec<&'static str>, out: &mut Vec<Vec<&'static str>>) {
    let rs = rows(path);
    if rs.len() >= 3 {
        out.push(labels.clone());
    }
    for (i, r) in rs.iter().enumerate() {
        if let Kind::List(_) = r.kind {
            path.push(i as u8);
            labels.push(r.label);
            lists(path, labels, out);
            labels.pop();
            path.pop();
        }
    }
}

#[test]
fn every_encoder_moves_every_settings_sub_list() {
    let mut all = Vec::new();
    lists(&mut Vec::new(), &mut Vec::new(), &mut all);
    assert!(all.len() > 3);
    for labels in all.iter().filter(|l| !l.is_empty()) {
        let mut ui = UiState::new();
        let mut r = Rig::new(&mut ui, three().0);
        r.to(labels);
        bar_follows(r.ui, &ALL_ENCODERS);
    }
}

#[cfg(feature = "orbit")]
#[test]
fn every_encoder_moves_scene_save_to() {
    let mut ui = UiState::new();
    let mut r = Rig::new(&mut ui, three().0);
    r.to(&["ORBIT", "SAVE TO PROJ"]);
    bar_follows(r.ui, &ALL_ENCODERS);
}

/// The bar moves by the biggest turn of the frame: brushed encoders move
/// it once, and opposite nudges do not cancel.
#[test]
fn the_biggest_turn_wins() {
    let mut ui = UiState::new();
    screen::tap(&mut ui, ButtonId::Menu);
    let go = |ui: &mut UiState, i: Input, by: i32| {
        let at = row(ui) as i32;
        feed(ui, i);
        assert_eq!(row(ui) as i32 - at, by);
    };
    go(
        &mut ui,
        Input::turn(EncoderId::A, 1).and_turn(EncoderId::C, 1),
        1,
    );
    go(
        &mut ui,
        Input::turn(EncoderId::B, 1).and_turn(EncoderId::F, 3),
        3,
    );
    go(
        &mut ui,
        Input::turn(EncoderId::A, -1).and_turn(EncoderId::E, 1),
        -1,
    );
    go(
        &mut ui,
        Input::turn(EncoderId::D, 1).and_turn(EncoderId::E, -2),
        -2,
    );
}

#[test]
fn the_browser_moves_on_every_encoder_but_b() {
    let at = Browse::of(4, 0);
    for e in ALL_ENCODERS {
        let down = browser::input(&Input::turn(e, 1), at);
        let up = browser::input(&Input::turn(e, -1), at);
        let lib = browser::input_library(&Input::turn(e, 1), Browse::default(), 8);
        if e == EncoderId::B {
            assert_eq!(
                (down, up, lib),
                (at, at, Browse::default()),
                "B turns the tabs"
            );
        } else {
            assert_eq!(down, Browse::of(5, 0), "{e:?}");
            assert_eq!(up, Browse::of(3, 0), "{e:?}");
            assert_eq!(lib.lib_cursor(), 1, "{e:?}");
        }
    }
}
