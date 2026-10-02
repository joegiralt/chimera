//! SETTINGS › PART and the Sound rung (settings spec § The PART branch):
//! each action from `part_actions`, each prompt's answers, and no replace
//! without REPLACE (#257, #258).

mod screen;

use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{Line, PartId, PartStatus, SlotId, part_status, test_support};
use chimera_core::storage::sound_crc;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::nav::Location;
use chimera_core::ui::settings::part::{Offer, PartCmd};
use chimera_core::ui::settings::view::{PartMark, RowLook, draw_part_strip};
use chimera_core::ui::settings::{PART_ROW, Screen, screen_path};
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Fb, Input, feed, tap, to_leaf};

const P: [PartId; 6] = PartId::ALL;
const B: [ButtonId; 6] = [
    ButtonId::B1,
    ButtonId::B2,
    ButtonId::B3,
    ButtonId::B4,
    ButtonId::B5,
    ButtonId::B6,
];
/// SLOT 03.
const S3: SlotId = SlotId::ALL[2];

fn edit(ui: &mut UiState, p: PartId) {
    ui.project_mut().edit_part(p).sound.params.filter.cutoff *= 0.5;
}

fn status(ui: &UiState, p: PartId) -> PartStatus {
    part_status(ui.project().part(p), ui.project().pool())
}

fn crc(ui: &UiState, p: PartId) -> u32 {
    sound_crc(&ui.project().part(p).sound)
}

/// Part `p`'s Sound rung, the cursor on entry `entry`.
fn rung(ui: &mut UiState, p: PartId, entry: usize) {
    feed(ui, Input::chord(ButtonId::Edit, B[p.index()]));
    feed(ui, Input::turn(EncoderId::A, entry as i8));
    assert_eq!(ui.location().browse().map(|(_, b)| b.cursor()), Some(entry));
}

/// Slot `s` into a Part that doesn't ask, through the Sound rung.
fn load(ui: &mut UiState, p: PartId, s: SlotId) {
    rung(ui, p, s.index());
    feed(ui, Input::press(ButtonId::Edit));
    assert!(!ui.prompt_open());
    assert!(
        ui.project()
            .part(p)
            .sound
            .bits_eq(ui.project().pool().get(s).unwrap())
    );
}

/// SETTINGS › PART for Part `p`, the bar on row `row`.
fn part_row(ui: &mut UiState, p: PartId, row: usize) {
    feed(ui, Input::press(B[p.index()]));
    to_leaf(ui, &["PART"]);
    for _ in 0..row {
        feed(ui, Input::press(ButtonId::Plus));
    }
    assert_eq!(ui.active_part, p);
}

/// SAVE TO PROJ for Part `p`, the bar on `c`'s row.
fn save_row(ui: &mut UiState, p: PartId, c: PartCmd) {
    part_row(ui, p, 2);
    feed(ui, Input::press(ButtonId::Edit));
    let i = chimera_core::ui::settings::part::SAVE_ROWS
        .iter()
        .position(|&r| r == c)
        .unwrap();
    for _ in 0..i {
        feed(ui, Input::press(ButtonId::Plus));
    }
    assert_eq!(
        ui.location(),
        Location::settings_at(screen_path(Screen::SaveToProj), i as u8)
    );
}

fn question(ui: &UiState) -> String {
    ui.prompt_words_for_test()
        .expect("a prompt")
        .0
        .as_str()
        .to_string()
}

/// Option `i` of the open prompt.
fn answer(ui: &mut UiState, i: i8) {
    feed(ui, Input::turn(EncoderId::A, i));
    tap(ui, ButtonId::Seq);
    assert!(!ui.prompt_open());
}

fn mark(ui: &UiState, p: PartId) -> String {
    PartMark::of(ui.project(), p).to_string()
}

#[test]
fn part_strip_marks() {
    let mut ui = UiState::new();
    assert_eq!(mark(&ui, P[0]), "CLEAN");
    edit(&mut ui, P[0]);
    assert_eq!(mark(&ui, P[0]), "* EDITED · FROM INIT");

    load(&mut ui, P[1], S3);
    load(&mut ui, P[2], S3);
    edit(&mut ui, P[1]);
    assert_eq!(mark(&ui, P[1]), "* EDITED · FROM SLOT 03");
    let _ = test_support::save_part_to(ui.project_mut(), P[1], S3);
    assert_eq!(mark(&ui, P[2]), "◦ SLOT 03 MOVED");

    let strip = |m: PartMark| {
        let mut fb = Fb::new();
        draw_part_strip(&mut fb, P[1], "WARM BASS", m);
        assert_eq!(fb.oob, 0);
        (0..240).any(|x| (28..48).any(|y| fb.at(x, y) == theme::WARN))
    };
    assert!(strip(PartMark::of(ui.project(), P[0])), "edited in WARN");
    assert!(!strip(PartMark::of(ui.project(), P[1])), "clean");

    // The moved mark leaves the name room.
    let mut fb = Fb::new();
    draw_part_strip(
        &mut fb,
        P[2],
        "ABCDEFGHIJKLMNOP",
        PartMark::of(ui.project(), P[2]),
    );
    assert_eq!(fb.oob, 0);
    assert!((0..240).any(|x| (28..48).any(|y| fb.at(x, y) == theme::WARN)));
    let name_px = (100..120).any(|x| (28..48).any(|y| fb.at(x, y) == theme::INK));
    assert!(name_px, "the name runs past 100 px");
}

/// P1 and P4 on SLOT 03, P1 edited and saved over it: the prompt names P4.
fn saved_over() -> UiState {
    let mut ui = UiState::new();
    load(&mut ui, P[0], S3);
    load(&mut ui, P[3], S3);
    edit(&mut ui, P[0]);
    save_row(&mut ui, P[0], PartCmd::OverSlot);
    assert_eq!(
        Offer::of(ui.project(), P[0])
            .label(PartCmd::OverSlot)
            .as_str(),
        "OVER SLOT 03"
    );
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(question(&ui), "P4 ALSO USES SLOT 03");
    assert_eq!(
        ui.step_toast(0),
        ToastStep::Show(Line::new("SAVED TO SLOT 03"))
    );
    ui
}

/// UPDATE applies the reverts the prompt showed: a Part edited since, or
/// one it didn't name, keeps its edit.
#[test]
fn update_applies_only_what_it_showed() {
    let mut ui = UiState::new();
    for p in [P[0], P[3], P[4]] {
        load(&mut ui, p, S3);
    }
    edit(&mut ui, P[4]); // Edited, so never named; not as P1 is
    edit(&mut ui, P[4]);
    edit(&mut ui, P[0]);
    save_row(&mut ui, P[0], PartCmd::OverSlot);
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(question(&ui), "P4 ALSO USES SLOT 03");
    let p5 = crc(&ui, P[4]);
    // While the prompt is open, and not to the sound P1 saved.
    let r = &mut ui
        .project_mut()
        .edit_part(P[3])
        .sound
        .params
        .filter
        .resonance;
    *r = if *r > 0.5 { 0.1 } else { 0.9 };
    assert_eq!(status(&ui, P[3]), PartStatus::Edited);
    let p4 = crc(&ui, P[3]);
    answer(&mut ui, 0); // UPDATE P4
    assert_eq!(crc(&ui, P[3]), p4, "edited since: refused");
    assert_eq!(crc(&ui, P[4]), p5, "not named: untouched");
    assert_eq!(status(&ui, P[4]), PartStatus::Edited);
}

#[test]
fn save_over_then_update_stale() {
    let mut ui = saved_over();
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);
    answer(&mut ui, 0); // UPDATE P4
    assert_eq!(status(&ui, P[3]), PartStatus::Clean);
    assert_eq!(crc(&ui, P[3]), crc(&ui, P[0]), "the new sound");

    let mut ui = saved_over();
    answer(&mut ui, 1); // LEAVE
    assert_eq!(status(&ui, P[3]), PartStatus::Stale(S3));
    assert_ne!(crc(&ui, P[3]), crc(&ui, P[0]));
}

#[test]
fn stale_part_offers_no_over_slot() {
    let mut ui = saved_over();
    answer(&mut ui, 1); // LEAVE
    let pool = sound_crc(ui.project().pool().get(S3).unwrap());
    let was = crc(&ui, P[3]);
    save_row(&mut ui, P[3], PartCmd::OverSlot);
    let o = Offer::of(ui.project(), P[3]);
    assert_eq!(o.look(PartCmd::OverSlot), RowLook::Dimmed);
    assert_eq!(o.look(PartCmd::NewSlot), RowLook::Normal);
    assert_eq!(o.look(PartCmd::Reload), RowLook::Normal, "UPDATE");
    tap(&mut ui, ButtonId::Seq);
    assert!(!ui.prompt_open());
    assert_eq!(status(&ui, P[3]), PartStatus::Stale(S3));
    assert_eq!(crc(&ui, P[3]), was);
    assert_eq!(sound_crc(ui.project().pool().get(S3).unwrap()), pool);
}

#[test]
fn reload_is_dimmed_until_there_is_something_to_revert() {
    let mut ui = UiState::new();
    load(&mut ui, P[0], S3);
    assert_eq!(
        Offer::of(ui.project(), P[0]).look(PartCmd::Reload),
        RowLook::Dimmed
    );
    edit(&mut ui, P[0]);
    assert_eq!(
        Offer::of(ui.project(), P[0]).look(PartCmd::Reload),
        RowLook::Normal
    );
    part_row(&mut ui, P[0], 3); // RELOAD FROM PROJ
    tap(&mut ui, ButtonId::Seq);
    answer(&mut ui, 1); // REPLACE
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);
}

#[test]
fn reload_over_edits_asks_and_save_first_goes_to_a_new_slot() {
    let mut ui = UiState::new();
    load(&mut ui, P[1], S3);
    let slot = sound_crc(ui.project().pool().get(S3).unwrap());
    edit(&mut ui, P[1]);
    let edited = crc(&ui, P[1]);
    part_row(&mut ui, P[1], 3); // RELOAD FROM PROJ
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(question(&ui), "RELOAD P2 FROM SLOT 03?");
    answer(&mut ui, 2); // CANCEL
    assert_eq!(crc(&ui, P[1]), edited);

    let free = ui.project().pool().first_free().unwrap();
    tap(&mut ui, ButtonId::Seq);
    answer(&mut ui, 0); // SAVE PART FIRST
    assert_eq!(sound_crc(ui.project().pool().get(free).unwrap()), edited);
    assert_eq!(sound_crc(ui.project().pool().get(S3).unwrap()), slot);
    assert_eq!(crc(&ui, P[1]), slot, "reloaded");
    assert_eq!(status(&ui, P[1]), PartStatus::Clean);
}

#[test]
fn a_stale_part_reloads_without_asking() {
    let mut ui = saved_over();
    answer(&mut ui, 1); // LEAVE
    part_row(&mut ui, P[3], 3);
    tap(&mut ui, ButtonId::Seq);
    assert!(!ui.prompt_open());
    assert_eq!(status(&ui, P[3]), PartStatus::Clean);
    assert_eq!(crc(&ui, P[3]), crc(&ui, P[0]));
}

/// A replace refused at the answer says why and stays.
#[test]
fn a_refused_replace_says_why() {
    let mut ui = UiState::new();
    edit(&mut ui, P[0]);
    let edited = crc(&ui, P[0]);
    rung(&mut ui, P[0], S3.index());
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert!(ui.prompt_open());
    ui.project_mut().pool_clear(S3).unwrap();
    answer(&mut ui, 1); // REPLACE
    assert_eq!(
        ui.step_toast(0),
        ToastStep::Show(Line::new("SLOT IS EMPTY"))
    );
    assert_eq!(crc(&ui, P[0]), edited);
    assert!(ui.location().browse().is_some());
}

/// CLEAR on Part 1, edited from INIT: its prompt.
fn clear_asks(ui: &mut UiState) -> u32 {
    edit(ui, P[0]);
    part_row(ui, P[0], 1); // CLEAR
    tap(ui, ButtonId::Seq);
    assert_eq!(question(ui), "CLEAR P1 TO INIT?");
    crc(ui, P[0])
}

#[test]
fn clear_edited_part_asks_and_each_answer() {
    let init = sound_crc(&Sound::init(EngineType::Algo));

    let mut ui = UiState::new();
    let edited = clear_asks(&mut ui);
    answer(&mut ui, 2); // CANCEL
    assert_eq!(crc(&ui, P[0]), edited);
    tap(&mut ui, ButtonId::Seq);
    tap(&mut ui, ButtonId::Menu); // CANCEL too
    assert!(!ui.prompt_open());
    assert_eq!(crc(&ui, P[0]), edited);

    let mut ui = UiState::new();
    clear_asks(&mut ui);
    answer(&mut ui, 1); // REPLACE
    assert_eq!(crc(&ui, P[0]), init);
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);

    let mut ui = UiState::new();
    let free = ui.project().pool().first_free().unwrap();
    let edited = clear_asks(&mut ui);
    answer(&mut ui, 0); // SAVE PART FIRST
    assert_eq!(sound_crc(ui.project().pool().get(free).unwrap()), edited);
    assert_eq!(crc(&ui, P[0]), init);
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);
}

#[test]
fn rename_marks_the_part_edited() {
    let mut ui = UiState::new();
    load(&mut ui, P[0], S3);
    let name = ui.project().part(P[0]).sound.name;
    part_row(&mut ui, P[0], 0); // RENAME
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.naming().expect("NAMING").text(), name.as_str());
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.naming().is_none());
    assert_ne!(ui.project().part(P[0]).sound.name, name);
    assert_eq!(status(&ui, P[0]), PartStatus::Edited);
}

#[test]
fn edit_runs_an_action_row() {
    let mut ui = UiState::new();
    part_row(&mut ui, P[0], 0); // RENAME
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert!(ui.naming().is_some(), "EDIT on PART > RENAME");
    tap(&mut ui, ButtonId::Menu);
    assert!(ui.naming().is_none());
}

#[test]
fn sound_rung_load_asks_when_edited() {
    let mut ui = UiState::new();
    edit(&mut ui, P[0]);
    rung(&mut ui, P[0], S3.index());
    feed(&mut ui, Input::press(ButtonId::Edit));
    let (q, r) = ui.prompt_words_for_test().expect("a prompt");
    assert_eq!(
        (q.as_str(), r.as_str()),
        ("REPLACE P1 SOUND?", "P1 IS EDITED")
    );
    assert!(ui.location().browse().is_some(), "still on the rung");
    answer(&mut ui, 1); // REPLACE
    assert!(
        ui.project()
            .part(P[0])
            .sound
            .bits_eq(ui.project().pool().get(S3).unwrap())
    );
    assert_eq!(
        ui.location(),
        Location::part_home(P[0], ui.project().part(P[0]).sound.engine())
    );
}

#[test]
fn sound_rung_seq_opens_part_settings() {
    let mut ui = UiState::new();
    rung(&mut ui, P[2], 0);
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.location(), Location::settings_at(&[PART_ROW], 0));
    assert_eq!(ui.active_part, P[2]);
    assert_eq!(ui.crumbs().unwrap().to_string(), "SETTINGS › PART 3");
}

#[test]
fn sound_rung_mix_minus_clears_an_unused_slot_after_confirm() {
    let mut ui = UiState::new();
    assert!(ui.project().pool().get(S3).is_some());
    rung(&mut ui, P[0], S3.index());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    let (q, r) = ui.prompt_words_for_test().expect("a prompt");
    assert_eq!(
        (q.as_str(), r.as_str()),
        ("CLEAR SLOT 03?", "NO PART USES IT")
    );
    answer(&mut ui, 1); // CANCEL
    assert!(ui.project().pool().get(S3).is_some());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    answer(&mut ui, 0); // CLEAR
    assert!(ui.project().pool().get(S3).is_none());

    let mut ui = UiState::new();
    load(&mut ui, P[0], S3);
    rung(&mut ui, P[0], S3.index());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert!(!ui.prompt_open());
    assert_eq!(
        ui.step_toast(0),
        ToastStep::Show(Line::new("SLOT IN USE: P1"))
    );
    assert!(ui.project().pool().get(S3).is_some());
}

/// #258: the browser's load never replaces without REPLACE.
#[test]
fn sound_rung_load_never_replaces_without_an_answer() {
    let mut ui = UiState::new();
    edit(&mut ui, P[0]);
    let edited = crc(&ui, P[0]);
    let ask = |ui: &mut UiState| {
        rung(ui, P[0], S3.index());
        feed(ui, Input::press(ButtonId::Edit));
        assert!(ui.prompt_open());
    };

    ask(&mut ui);
    for _ in 0..30 {
        feed(&mut ui, Input::default());
        ui.update(UiTick::for_test());
    }
    assert!(ui.prompt_open());
    assert_eq!(crc(&ui, P[0]), edited, "no answer");

    tap(&mut ui, ButtonId::Menu);
    assert!(!ui.prompt_open());
    assert_eq!(crc(&ui, P[0]), edited, "MENU");

    ask(&mut ui);
    feed(&mut ui, Input::press(ButtonId::B2));
    assert!(!ui.prompt_open());
    assert_eq!(crc(&ui, P[0]), edited, "B2 drops it");

    ask(&mut ui);
    answer(&mut ui, 1); // REPLACE
    assert_ne!(crc(&ui, P[0]), edited);
    assert!(
        ui.project()
            .part(P[0])
            .sound
            .bits_eq(ui.project().pool().get(S3).unwrap())
    );
}
