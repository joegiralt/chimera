//! SETTINGS › SYSTEM › OS UPGRADE: a prompt, then one yes for the shell.

mod screen;
use chimera_core::project::Line;
use chimera_core::ui::UiState;
use chimera_core::ui::settings::prompt::{Answers, DfuAnswer, EnterDfu, Prompt};
use chimera_core::ui::settings::{Act, AskKind, Kind};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, tap, to_leaf};

/// SETTINGS › SYSTEM, the cursor on OS UPGRADE (row 0), SEQ.
fn ask(ui: &mut UiState) {
    to_leaf(ui, &["SYSTEM"]);
    tap(ui, ButtonId::Seq);
}

#[test]
fn os_upgrade_is_an_action_row() {
    use chimera_core::ui::settings::rows;
    let top = rows(&[]).iter().position(|r| r.label == "SYSTEM").unwrap() as u8;
    let r = &rows(&[top])[0];
    assert_eq!((r.label, r.crumb), ("OS UPGRADE", "OS"));
    assert!(matches!(r.kind, Kind::Act(Act::EnterDfu)), "{:?}", r.kind);
}

#[test]
fn os_upgrade_asks_before_dfu() {
    let mut ui = Box::new(UiState::new());
    ask(&mut ui);
    assert_eq!(ui.prompt_kind_for_test(), Some(AskKind::EnterDfu));
    assert!(ui.take_dfu().is_none(), "asking is not a yes");
}

#[test]
fn the_prompt_says_what_it_does() {
    let (mut q, mut r) = (Line::new(""), Line::new(""));
    EnterDfu.words(&mut q, &mut r);
    assert_eq!(
        (q.as_str(), r.as_str()),
        ("ENTER DFU?", "PLAY STOPS UNTIL FLASHED OR POWER-CYCLED")
    );
    let pills: Vec<_> = DfuAnswer::ALL
        .as_slice()
        .iter()
        .map(|a| a.label())
        .collect();
    assert_eq!(pills, ["ENTER DFU", "CANCEL"]);
}

#[test]
fn enter_dfu_gives_one_yes() {
    let mut ui = Box::new(UiState::new());
    ask(&mut ui);
    tap(&mut ui, ButtonId::Seq); // the first pill: ENTER DFU
    assert!(ui.take_dfu().is_some());
    assert!(ui.take_dfu().is_none(), "taken once");
    assert_eq!(ui.prompt_kind_for_test(), None);
}

#[test]
fn cancel_and_menu_give_none() {
    let mut ui = Box::new(UiState::new());
    ask(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::A, 1)); // CANCEL
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.take_dfu().is_none());
    ask(&mut ui);
    tap(&mut ui, ButtonId::Menu);
    assert!(ui.take_dfu().is_none());
    assert_eq!(ui.prompt_kind_for_test(), None);
}
