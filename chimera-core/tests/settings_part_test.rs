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
use chimera_core::ui::nav::{Location, MixPage};
use chimera_core::ui::settings::part::{Offer, PartCmd};
use chimera_core::ui::settings::view::{PartMark, RowLook, draw_part_strip};
use chimera_core::ui::settings::{PART_ROW, Screen, screen_path};
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Fb, Input, feed, tap, to_leaf, to_row};

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

/// Part `p`'s Sound rung, the cursor on pool slot `slot` (0-based).
fn rung(ui: &mut UiState, p: PartId, slot: usize) {
    feed(ui, Input::chord(ButtonId::Edit, B[p.index()]));
    to_row(ui, chimera_core::ui::browser::HEAD + slot);
    let at = ui.location().browse().map(|(_, b)| b.cursor());
    assert_eq!(at, Some(chimera_core::ui::browser::HEAD + slot));
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
            .bits_eq(&ui.project().pool().get(s).unwrap().to_sound())
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
    feed(ui, Input::turn(EncoderId::A, -9));
    feed(ui, Input::turn(EncoderId::A, i));
    tap(ui, ButtonId::Edit);
    assert!(!ui.prompt_open());
}

fn mark(ui: &UiState, p: PartId) -> String {
    PartMark::of(ui.project(), p).to_string()
}

#[test]
fn part_strip_marks() {
    let mut ui = UiState::new();
    // Part 6: NEW's one INIT Part.
    assert_eq!(mark(&ui, P[5]), "CLEAN");
    edit(&mut ui, P[5]);
    assert_eq!(mark(&ui, P[5]), "* EDITED · FROM INIT");

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
    assert!(strip(PartMark::of(ui.project(), P[5])), "edited in WARN");
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
    tap(&mut ui, ButtonId::Edit);
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
    tap(&mut ui, ButtonId::Edit);
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
    let pool = ui.project().pool().get(S3).unwrap().crc();
    let was = crc(&ui, P[3]);
    save_row(&mut ui, P[3], PartCmd::OverSlot);
    let o = Offer::of(ui.project(), P[3]);
    assert_eq!(o.look(PartCmd::OverSlot), RowLook::Dimmed);
    assert_eq!(o.look(PartCmd::NewSlot), RowLook::Normal);
    assert_eq!(o.look(PartCmd::Reload), RowLook::Normal, "UPDATE");
    tap(&mut ui, ButtonId::Edit);
    assert!(!ui.prompt_open());
    assert_eq!(status(&ui, P[3]), PartStatus::Stale(S3));
    assert_eq!(crc(&ui, P[3]), was);
    assert_eq!(ui.project().pool().get(S3).unwrap().crc(), pool);
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
    tap(&mut ui, ButtonId::Edit);
    answer(&mut ui, 1); // REPLACE
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);
}

#[test]
fn reload_over_edits_asks_and_save_first_goes_to_a_new_slot() {
    let mut ui = UiState::new();
    load(&mut ui, P[1], S3);
    let slot = ui.project().pool().get(S3).unwrap().crc();
    edit(&mut ui, P[1]);
    let edited = crc(&ui, P[1]);
    part_row(&mut ui, P[1], 3); // RELOAD FROM PROJ
    tap(&mut ui, ButtonId::Edit);
    assert_eq!(question(&ui), "RELOAD P2 FROM SLOT 03?");
    answer(&mut ui, 2); // CANCEL
    assert_eq!(crc(&ui, P[1]), edited);

    let free = ui.project().pool().first_free().unwrap();
    tap(&mut ui, ButtonId::Edit);
    answer(&mut ui, 0); // SAVE PART FIRST
    assert_eq!(ui.project().pool().get(free).unwrap().crc(), edited);
    assert_eq!(ui.project().pool().get(S3).unwrap().crc(), slot);
    assert_eq!(crc(&ui, P[1]), slot, "reloaded");
    assert_eq!(status(&ui, P[1]), PartStatus::Clean);
}

#[test]
fn a_stale_part_reloads_without_asking() {
    let mut ui = saved_over();
    answer(&mut ui, 1); // LEAVE
    part_row(&mut ui, P[3], 3);
    tap(&mut ui, ButtonId::Edit);
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
    screen::init_part(ui, P[0]);
    edit(ui, P[0]);
    part_row(ui, P[0], 1); // CLEAR
    tap(ui, ButtonId::Edit);
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
    tap(&mut ui, ButtonId::Edit);
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
    assert_eq!(ui.project().pool().get(free).unwrap().crc(), edited);
    assert_eq!(crc(&ui, P[0]), init);
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);
}

#[test]
fn rename_marks_the_part_edited() {
    let mut ui = UiState::new();
    load(&mut ui, P[0], S3);
    let name = ui.project().part(P[0]).sound.name;
    part_row(&mut ui, P[0], 0); // RENAME
    tap(&mut ui, ButtonId::Edit);
    assert_eq!(ui.naming().expect("NAMING").text(), name.as_str());
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    tap(&mut ui, ButtonId::Edit);
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
            .bits_eq(&ui.project().pool().get(S3).unwrap().to_sound())
    );
    assert_eq!(
        ui.location(),
        Location::part_home(P[0], ui.project().part(P[0]).sound.engine())
    );
}

#[test]
fn sound_rung_seq_goes_up_and_menu_reaches_part_settings() {
    let mut ui = UiState::new();
    rung(&mut ui, P[2], 0);
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.location(), Location::mixer(P[2], MixPage::Sends));
    tap(&mut ui, ButtonId::Menu);
    for _ in 0..PART_ROW {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
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
            .bits_eq(&ui.project().pool().get(S3).unwrap().to_sound())
    );
}

/// SETTINGS › PART › MIDI MAP (#247, spec § 8).
#[cfg(feature = "midi-map")]
mod midi_map {
    use super::*;
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modal::ModalParams;
    use chimera_core::midi_in::Cc;
    use chimera_core::midi_map::{Binding, Feedback, Input as MapInput, Mappable, MidiMap, Span};
    use chimera_core::params::FilterParams;

    /// The last row of PART.
    const MIDI_MAP_ROW: usize = chimera_core::ui::settings::tree::PART.len() - 1;

    fn entry(cc: u8, block: BlockRef, id: chimera_core::block::ParamId) -> Binding {
        Binding {
            input: MapInput::cc(cc).unwrap(),
            target: Mappable::new(ParamAddr::new(block, id)).unwrap(),
            span: Span::FULL,
            feedback: Feedback::Off,
        }
    }

    fn cc_num(b: &Binding) -> u8 {
        match b.input {
            MapInput::Cc(c, _) => c.get(),
            _ => panic!("not a CC binding"),
        }
    }

    fn cutoff(cc: u8) -> Binding {
        entry(cc, BlockRef::Filter, FilterParams::CUTOFF)
    }

    /// Part 1 on INIT with `entries`, and the page open.
    fn page(entries: &[Binding]) -> UiState {
        let mut ui = UiState::new();
        let mut m = MidiMap::EMPTY;
        for e in entries {
            m.learn(e.input, e.target).unwrap();
        }
        ui.project_mut().edit_part(P[0]).sound.map = m;
        part_row(&mut ui, P[0], MIDI_MAP_ROW);
        feed(&mut ui, Input::press(ButtonId::Edit));
        assert_eq!(
            ui.location(),
            Location::settings_at(screen_path(Screen::MidiMap), 0)
        );
        ui
    }

    fn map(ui: &UiState) -> MidiMap {
        ui.project().part(P[0]).sound.map
    }

    fn turn(ui: &mut UiState, e: EncoderId, d: i8) {
        feed(ui, Input::turn(e, d));
    }

    /// Row `i`'s label, its range as the bar's lines give it, its look.
    fn text(ui: &UiState, i: usize) -> (String, String, RowLook) {
        let b = ui.map_band();
        let t = b.text(i);
        let range = format!("{}-{}", t.min.as_str(), t.max.as_str());
        (t.label.as_str().into(), range, b.look(i))
    }

    #[test]
    fn midi_map_lists_rows() {
        let structure = entry(16, BlockRef::Modal, ModalParams::STRUCTURE);
        let ui = page(&[cutoff(74), structure]);
        assert_eq!(
            ui.project().part(P[0]).sound.params.engine(),
            EngineType::Algo
        );
        assert_eq!(
            text(&ui, 0),
            ("CC 74  CUTOFF".into(), "0-255".into(), RowLook::Normal)
        );
        let (label, _, look) = text(&ui, 1);
        assert_eq!(label, "CC 16  - STRUCT");
        assert_eq!(look, RowLook::Dimmed);
    }

    #[test]
    fn midi_map_edits() {
        let mut ui = page(&[cutoff(74), cutoff_b(63)]);
        let c0 = crc(&ui, P[0]);
        turn(&mut ui, EncoderId::B, 1);
        assert_eq!(cc_num(&map(&ui).entries()[0]), 75);
        assert_ne!(crc(&ui, P[0]), c0);
        assert_eq!(status(&ui, P[0]), PartStatus::Edited);

        // Row 1, CC 63: PLUS skips the reserved 64.
        feed(&mut ui, Input::press(ButtonId::Plus));
        turn(&mut ui, EncoderId::B, 1);
        assert_eq!(cc_num(&map(&ui).entries()[1]), 65);
        turn(&mut ui, EncoderId::B, -1);
        assert_eq!(cc_num(&map(&ui).entries()[1]), 63);
        // A duplicate is allowed: row 1 onto row 0's 75.
        turn(&mut ui, EncoderId::B, 11);
        assert_eq!(cc_num(&map(&ui).entries()[1]), 75);
        // Range edits dirty it too.
        let c1 = crc(&ui, P[0]);
        turn(&mut ui, EncoderId::C, 5);
        turn(&mut ui, EncoderId::D, -5);
        assert_eq!(
            (
                map(&ui).entries()[1].span.min,
                map(&ui).entries()[1].span.max
            ),
            (5, 250)
        );
        assert_ne!(crc(&ui, P[0]), c1);
        assert_eq!(status(&ui, P[0]), PartStatus::Edited);
    }

    fn cutoff_b(cc: u8) -> Binding {
        entry(cc, BlockRef::Filter, FilterParams::RESONANCE)
    }

    #[test]
    fn midi_map_cc_clamps_at_the_ends() {
        let mut ui = page(&[cutoff(1)]);
        turn(&mut ui, EncoderId::B, -3); // 0 is bank select
        assert_eq!(cc_num(&map(&ui).entries()[0]), 1);
        let mut ui = page(&[cutoff(119)]);
        turn(&mut ui, EncoderId::B, 3); // 120-127 are channel mode
        assert_eq!(cc_num(&map(&ui).entries()[0]), 119);
    }

    #[test]
    fn midi_map_range_clamps_and_inverts() {
        let mut ui = page(&[cutoff(74)]);
        let c0 = crc(&ui, P[0]);
        turn(&mut ui, EncoderId::D, -10);
        assert_eq!(map(&ui).entries()[0].span.max, 245);
        assert_eq!(text(&ui, 0).1, "0-245");
        assert_ne!(crc(&ui, P[0]), c0);
        // C up to 250: min above max is an inverted range.
        turn(&mut ui, EncoderId::C, 125);
        turn(&mut ui, EncoderId::C, 125);
        assert_eq!(map(&ui).entries()[0].span.min, 250);
        assert_eq!(text(&ui, 0).1, "250-245");
        assert_eq!(status(&ui, P[0]), PartStatus::Edited);
        // Clamps, never wraps.
        turn(&mut ui, EncoderId::C, 127);
        assert_eq!(map(&ui).entries()[0].span.min, 255);
        turn(&mut ui, EncoderId::D, -127);
        turn(&mut ui, EncoderId::D, -127);
        assert_eq!(map(&ui).entries()[0].span.max, 0);
    }

    #[test]
    fn midi_map_delete_confirms() {
        let mut ui = page(&[cutoff(74), cutoff_b(20)]);
        feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
        let (q, r) = ui.prompt_words_for_test().expect("a prompt");
        assert_eq!((q.as_str(), r.as_str()), ("DELETE CC 74?", "CUTOFF"));
        answer(&mut ui, 1); // CANCEL
        assert_eq!(map(&ui).len(), 2);
        feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
        answer(&mut ui, 0); // DELETE
        assert_eq!(map(&ui).len(), 1);
        assert_eq!(cc_num(&map(&ui).entries()[0]), 20);
        assert_eq!(status(&ui, P[0]), PartStatus::Edited);
    }

    #[test]
    fn midi_map_empty() {
        let mut ui = page(&[]);
        assert_eq!(map(&ui).len(), 0);
        // Nothing to edit or delete.
        turn(&mut ui, EncoderId::B, 1);
        feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
        assert!(!ui.prompt_open());
        assert_eq!(map(&ui).len(), 0);
    }

    #[test]
    fn midi_map_edit_stops_a_moving_ramp() {
        use chimera_core::MidiChannel;
        use chimera_core::midi_in::CcEvent;
        use chimera_hal::Ms;
        let mut ui = page(&[cutoff(74)]);
        let ev = |v| CcEvent {
            channel: MidiChannel::new(0).unwrap(),
            cc: Cc::new(74).unwrap(),
            value: v,
        };
        ui.hear_ccs(core::iter::once(ev(0)), Ms(100), true);
        ui.hear_ccs(core::iter::once(ev(127)), Ms(200), true);
        turn(&mut ui, EncoderId::D, -50);
        let was = ui.project().part(P[0]).sound.params.filter.cutoff;
        ui.hear_ccs(
            core::iter::empty::<chimera_core::midi_in::CcEvent>(),
            Ms(300),
            true,
        );
        assert_eq!(ui.project().part(P[0]).sound.params.filter.cutoff, was);
    }
}

/// MIDI MAP's rows follow A; B to F edit the row (#561, spec § 8.2).
#[cfg(feature = "midi-map")]
mod midi_map_bar {
    use super::*;
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::midi_map::{Mappable, MidiMap};
    use chimera_core::params::FilterParams;

    #[test]
    fn a_moves_the_bar_b_to_f_edit() {
        let mut ui = UiState::new();
        let (cc, target) = (
            chimera_core::midi_map::Input::cc(74).unwrap(),
            Mappable::new(ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)).unwrap(),
        );
        let mut m = MidiMap::EMPTY;
        m.learn(cc, target).unwrap();
        ui.project_mut().edit_part(P[0]).sound.map = m;
        part_row(
            &mut ui,
            P[0],
            chimera_core::ui::settings::tree::PART.len() - 1,
        );
        feed(&mut ui, Input::press(ButtonId::Edit));
        let row = |ui: &UiState| ui.location().settings().unwrap().row();
        let rev = ui.project().rev();
        feed(&mut ui, Input::turn(EncoderId::A, 1));
        assert_eq!(row(&ui), 0, "one row: it wraps");
        assert_eq!(ui.project().rev(), rev, "the bar edits nothing");
        // MIN sits at 0 and MAX at 255: C turns up, D down; E steps the
        // input, F turns the echo on.
        for (id, d) in [
            (EncoderId::B, 1),
            (EncoderId::C, 1),
            (EncoderId::D, -1),
            (EncoderId::E, 1),
            (EncoderId::F, 1),
        ] {
            let before = ui.project().rev();
            feed(&mut ui, Input::turn(id, d));
            assert_ne!(ui.project().rev(), before, "{id:?} edits");
        }
    }
}

/// PART › RANDOMIZE (ADR 0232): a turn of SEED rolls the active Part's
/// Sound, as `kern::roll` rolls it; the Part reads Edited, turning back to
/// a seed rolls its Sound again, and RELOAD FROM PROJ undoes it.
#[test]
fn randomize_rolls_the_part_and_reload_undoes_it() {
    use chimera_core::randomize::kern::roll;
    use chimera_core::randomize::{Randomize, Seed};
    use chimera_hal::EncoderId;
    let mut ui = UiState::new();
    load(&mut ui, P[0], S3);
    let slot = crc(&ui, P[0]);
    let seeded = |n| {
        let mut s: Sound = (*ui.project().part(P[0]).sound).clone();
        let r = Randomize {
            seed: Seed::new(n),
            ..Randomize::DEFAULT
        };
        roll(&mut s, &r);
        s
    };
    let (one, two) = (seeded(1), seeded(2));
    feed(&mut ui, Input::press(B[0]));
    to_leaf(&mut ui, &["PART", "RANDOMIZE"]);
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert!(ui.project().part(P[0]).sound.bits_eq(&one), "SEED 1");
    assert_eq!(status(&ui, P[0]), PartStatus::Edited);
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert!(ui.project().part(P[0]).sound.bits_eq(&two), "SEED 2");

    part_row(&mut ui, P[0], 3); // RELOAD FROM PROJ
    tap(&mut ui, ButtonId::Edit);
    answer(&mut ui, 1); // REPLACE
    assert_eq!(status(&ui, P[0]), PartStatus::Clean);
    assert_eq!(crc(&ui, P[0]), slot);
}

/// PART's list scrolls past the seven rows under the strip: the bar on
/// its last row stays above the footer.
#[test]
fn the_part_list_scrolls_to_its_last_row() {
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::settings::tree::PART;
    let mut ui = UiState::new();
    part_row(&mut ui, P[0], PART.len() - 1);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &screen::scope_fixture());
    assert_eq!(fb.oob, 0);
    let bar = (0..320).filter(|&y| (0..240).any(|x| fb.at(x, y) == theme::ACCENT_SOFT));
    let (top, bottom) = (bar.clone().min().unwrap(), bar.max().unwrap());
    assert!(
        top > 54 && bottom < theme::MAP_TOP,
        "bar at {top}..{bottom}"
    );
}
