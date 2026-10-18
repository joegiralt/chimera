//! Any encoder moves a list's bar, one row per detent, and edits nothing
//! (#561). MIDI MAP keeps B, C and D for the row's values; the browser keeps
//! B for its tabs.

mod screen;

use chimera_core::ui::UiState;
use chimera_core::ui::browser;
use chimera_core::ui::nav::Browse;
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

#[test]
fn every_encoder_picks_the_prompt_pill() {
    let pick = |e: Option<EncoderId>| {
        let mut ui = UiState::new();
        screen::prompt_load(&mut ui);
        if let Some(e) = e {
            feed(&mut ui, Input::turn(e, 2));
        }
        feed(&mut ui, Input::press(ButtonId::Seq));
        feed(&mut ui, Input::release(ButtonId::Seq).at(100));
        (ui.prompt_open(), ui.card_pending(), ui.naming().is_some())
    };
    let none = pick(None);
    let a = pick(Some(EncoderId::A));
    assert_ne!(a, none, "two detents reach CANCEL");
    for e in ALL_ENCODERS {
        assert_eq!(pick(Some(e)), a, "{e:?}");
    }
}
