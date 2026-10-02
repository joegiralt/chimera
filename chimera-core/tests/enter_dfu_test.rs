//! SETTINGS › SYSTEM › OS UPGRADE: a prompt, then one yes for the shell.

mod screen;
use chimera_core::boot::RomDfu;
use chimera_core::project::Line;
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::settings::prompt::{Answers, DfuAnswer, EnterDfu, Prompt};
use chimera_core::ui::settings::replace::said::Said;
use chimera_core::ui::settings::{Act, AskKind, Kind};
use chimera_core::ui::theme_settings::{Accent, Black, Bright, Gamma, ThemeSettings};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, tap, to_leaf};

/// The shell's SYSTEM side: a card, its sync and settings.
struct Sys {
    s: MemStore,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
}

impl Sys {
    fn new() -> Sys {
        let mut s = MemStore::new(1);
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        Sys { s, card, sync, set }
    }

    /// The shell's once-a-frame sync.
    fn frame(&mut self, ui: &mut UiState) {
        ui.sync_system(&mut self.sync, &mut self.card, &mut self.s, &mut self.set);
    }

    fn take(&mut self, ui: &mut UiState) -> Option<Said<RomDfu>> {
        ui.take_dfu(&mut self.sync, &mut self.card, &mut self.s, &mut self.set)
    }
}

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
    let mut sys = Sys::new();
    ask(&mut ui);
    assert_eq!(ui.prompt_kind_for_test(), Some(AskKind::EnterDfu));
    assert!(sys.take(&mut ui).is_none(), "asking is not a yes");
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
    let mut sys = Sys::new();
    ask(&mut ui);
    tap(&mut ui, ButtonId::Seq); // the first pill: ENTER DFU
    assert!(sys.take(&mut ui).is_some());
    assert!(sys.take(&mut ui).is_none(), "taken once");
    assert_eq!(ui.prompt_kind_for_test(), None);
}

#[test]
fn cancel_and_menu_give_none() {
    let mut ui = Box::new(UiState::new());
    let mut sys = Sys::new();
    ask(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::A, 1)); // CANCEL
    tap(&mut ui, ButtonId::Seq);
    assert!(sys.take(&mut ui).is_none());
    ask(&mut ui);
    tap(&mut ui, ButtonId::Menu);
    assert!(sys.take(&mut ui).is_none());
    assert_eq!(ui.prompt_kind_for_test(), None);
}

/// A THEME change made in the same SETTINGS visit reaches the card before
/// the restart, as leaving SETTINGS would have saved it.
#[test]
fn the_theme_is_saved_before_the_restart() {
    let t = ThemeSettings {
        bright: Bright::new(40),
        gamma: Gamma::Soft,
        accent: Accent::Lime,
        black: Black::new(1),
    };
    let mut ui = Box::new(UiState::new());
    let mut sys = Sys::new();
    ask(&mut ui);
    sys.frame(&mut ui);
    ui.set_theme(t);
    sys.frame(&mut ui);
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.in_settings(), "still in SETTINGS: no exit sync ran");
    assert!(sys.take(&mut ui).is_some());
    let (_, after, _) = SystemSync::boot(&mut Card::new(), &mut sys.s);
    assert_eq!(after.theme, t);
}
