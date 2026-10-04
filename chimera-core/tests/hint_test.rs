//! Gesture hints (ADR 0086): each MIX gesture's hint in the focus band of
//! the cell it acts on, there while that cell is the touched one and gone
//! when another is; the matrix's by its cursor's route, the browser's by
//! its row. One component draws them all, each within the screen.

#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::addr::Op;
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::scene::Target;
use chimera_core::project::PartId;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::hint::{self, Gesture, Hint};
use chimera_core::ui::mod_grid::{self, MatrixState};
use chimera_core::ui::orbit::draw::{
    CHIPS_SHOWN, NOTE_H, NOTE_TOP, OrbitFrame, chip_window, focus,
};
use chimera_core::ui::orbit::{ORBIT_CHRD, ORBIT_NOTES, ORBIT_RHYTHM, OrbitCell};
use chimera_core::ui::view::{SlotCtx, view};
use chimera_core::ui::{UiState, browser, theme};
use chimera_hal::ButtonId::{Edit, Seq};
use chimera_hal::EncoderId::{A, B, E, F};
use screen::{Input, feed, render_ui, tap, ui_for};

/// `def`'s slot `slot` as its focus band reads it on the selected ring:
/// the label and the hint.
fn band(ui: &UiState, def: &BlockDef, slot: usize) -> (String, Hint) {
    let r = ui.location().orbit().unwrap().ring();
    let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
    let ctx = SlotCtx::read(&ui.project().part(ui.active_part).sound.params, Op::A).on_ring(r);
    match focus(def, slot, &ctx, &f, |_| 0.0) {
        Some(fo) => (fo.label.as_str().into(), fo.hint),
        None => (
            view(def, slot, &ctx).label().into(),
            view(def, slot, &ctx).hint(),
        ),
    }
}

#[test]
fn note_on_a_step_ring_names_both_its_turns() {
    let ui = ui_for("orbit_notes_step");
    assert_eq!(
        band(&ui, &ORBIT_NOTES, 5),
        (
            "STEP 4 OF 8".into(),
            &[
                Gesture("MIX + NOTE", "◂▸ STEP"),
                Gesture("NOTE", "▴▾ PITCH")
            ][..]
        )
    );
}

#[test]
fn note_in_a_pool_picks_a_note() {
    let ui = ui_for("orbit_notes");
    let (label, h) = band(&ui, &ORBIT_NOTES, 5);
    assert_eq!(label, "NOTE 6 OF 8");
    assert_eq!(h[0], Gesture("MIX + NOTE", "◂▸ PICK"));
}

#[test]
fn clear_says_what_each_way_does() {
    let ui = ui_for("orbit_notes");
    assert_eq!(
        band(&ui, &ORBIT_NOTES, 4),
        (
            "CLEAR".into(),
            &[Gesture("MIX + CLEAR", "▴ RESET  ▾ RANDOM")][..]
        )
    );
}

#[test]
fn chrd_cells_name_their_mix_turns() {
    let ui = ui_for("orbit_chrd");
    assert_eq!(
        band(&ui, &ORBIT_CHRD, 1).1[0],
        Gesture("MIX + DEGREE", "◂▸ SEGMENT")
    );
    assert_eq!(
        band(&ui, &ORBIT_CHRD, 2).1,
        &[Gesture("MIX + PLAY", "▾ DROPS THE CHORD")][..]
    );
    assert_eq!(band(&ui, &ORBIT_CHRD, 0).1, hint::NONE, "SEGS: no gesture");
}

#[test]
fn mute_names_the_ring_buttons() {
    let ui = ui_for("orbit_rhythm");
    assert_eq!(
        band(&ui, &ORBIT_RHYTHM, 5),
        (
            "MUTE".into(),
            &[Gesture("MIX + B1-5", "MUTES THAT RING")][..]
        )
    );
    assert_eq!(band(&ui, &ORBIT_RHYTHM, 3).1, hint::NONE, "CHANCE");
}

/// The focus band's hint line: INK pixels in it.
fn hint_ink(ui: &UiState) -> usize {
    let fb = render_ui(ui);
    (theme::MARGIN_X..theme::SCREEN_W - theme::MARGIN_X)
        .flat_map(|x| (hint::FOCUS_Y - 7..=hint::FOCUS_Y).map(move |y| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == theme::INK)
        .count()
}

/// The hint is the touched cell's: NOTE's shows, WALK's touch takes it
/// away, CLEAR's brings its own, and NOTE's comes back with NOTE.
#[test]
fn a_hint_shows_only_while_its_cell_is_touched() {
    let mut ui = Box::new(ui_for("orbit_notes_step"));
    let note = hint_ink(&ui);
    assert!(note > 0, "NOTE touched: its hint");
    feed(&mut ui, Input::turn(B, 1));
    assert_eq!(hint_ink(&ui), 0, "WALK touched: none");
    feed(&mut ui, Input::turn(E, 1));
    let clear = hint_ink(&ui);
    assert!(clear > 0 && clear != note, "CLEAR touched: CLEAR's");
    feed(&mut ui, Input::turn(F, 1));
    assert_eq!(hint_ink(&ui), note, "NOTE again");
}

#[test]
fn the_window_always_holds_the_selected_chip() {
    assert_eq!(chip_window(31, 32), 24..32);
    assert_eq!(chip_window(0, 32), 0..8);
    assert_eq!(chip_window(9, 32), 8..16);
    assert_eq!(chip_window(10, 12), 4..12);
    assert_eq!(chip_window(5, 8), 0..8);
    assert_eq!(chip_window(0, 1), 0..1);
    for n in 1..=32 {
        for sel in 0..n {
            let w = chip_window(sel, n);
            assert!(w.contains(&sel) && w.end <= n, "{sel} of {n}");
            assert_eq!(w.len(), n.min(CHIPS_SHOWN));
        }
    }
}

/// Step 32 of 32: the row scrolled to its last eight, the last chip the
/// one outlined, and the band's label says which.
#[test]
fn step_32_of_32_is_on_screen() {
    let ui = ui_for("orbit_notes_step32");
    assert_eq!(band(&ui, &ORBIT_NOTES, 5).0, "STEP 32 OF 32");
    let fb = render_ui(&ui);
    assert_eq!(outlined(&fb), [7], "the last chip outlined");
}

/// Which of the eight chips on show are outlined in INK2, by their left
/// edge at mid height.
fn outlined(fb: &screen::Fb) -> Vec<i32> {
    let slot = (theme::VIZ_RIGHT - theme::VIZ_LEFT + 4) as f32 / CHIPS_SHOWN as f32;
    let y = NOTE_TOP + NOTE_H / 2;
    let x = |i: i32| theme::VIZ_LEFT + (slot * i as f32).round() as i32;
    (0..CHIPS_SHOWN as i32)
        .filter(|&i| fb.at(x(i), y) == theme::INK2)
        .collect()
}

/// Step 32 picked, then STEPS cut to 8: the pick clamps to step 8, the
/// window to the eight there are, and the stored pick waits for STEPS to
/// grow back.
#[test]
fn fewer_steps_clamp_the_pick_and_the_window() {
    let mut ui = Box::new(ui_for("orbit_notes_step32"));
    while ui.page_def().id != ORBIT_RHYTHM.id {
        tap(&mut ui, Seq);
    }
    feed(&mut ui, Input::turn(A, -24)); // STEPS 8
    tap(&mut ui, Edit);
    tap(&mut ui, Edit);
    screen::settle(&mut ui);
    let r = &ui.project().orbit().live().rings[1];
    assert_eq!(r.chips(), 8);
    assert_eq!(ui.orbit().chip, 31, "the pick kept");
    assert_eq!(ui.orbit().chip_of(r), 7, "read as the last step");
    assert_eq!(chip_window(ui.orbit().chip_of(r), r.chips()), 0..8);
    assert_eq!(band(&ui, &ORBIT_NOTES, 5).0, "STEP 8 OF 8");
    assert_eq!(outlined(&render_ui(&ui)), [7]);
}

/// The matrix's hint line as drawn: its INK pixels, against `want`
/// drawn alone at the same place.
fn matrix_hint_is(ui: &UiState, want: Hint) -> bool {
    let ink = |fb: &screen::Fb| -> Vec<(i32, i32)> {
        (0..theme::SCREEN_W)
            .flat_map(|x| (mod_grid::HINT_Y - 8..=mod_grid::HINT_Y + 2).map(move |y| (x, y)))
            .filter(|&(x, y)| fb.at(x, y) == theme::INK)
            .collect()
    };
    let mut alone = screen::Fb::new();
    hint::draw(&mut alone, want, theme::MARGIN_X, mod_grid::HINT_Y);
    ink(&render_ui(ui)) == ink(&alone)
}

/// On a route the matrix draws DELETE; the cursor moved off it, PRIME.
#[test]
fn the_matrix_draws_the_hint_of_its_cursor() {
    let mut ui = Box::new(ui_for("mod_matrix"));
    assert!(matrix_hint_is(&ui, mod_grid::DELETE), "LFO 1 → CUTOFF");
    feed(&mut ui, Input::turn(A, 1));
    screen::settle(&mut ui);
    assert!(matrix_hint_is(&ui, mod_grid::PRIME), "LFO 2: no route");
    assert!(!matrix_hint_is(&ui, mod_grid::DELETE));
}

#[test]
fn the_matrix_hint_follows_the_cursors_route() {
    use chimera_core::mod_path::ModDestRegistry;
    use chimera_core::modulation::CUTOFF;
    let mut m = MatrixState::new();
    m.rebuild_sources(&["SRC"; 8]);
    assert_eq!(m.hint(), mod_grid::PRIME, "no destinations");
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF, *b"CUTOFF\0\0").unwrap();
    m.rebuild_dests_from_registry(&reg);
    assert_eq!(m.hint(), mod_grid::PRIME, "no route here");
    m.set(m.sel_source(), 0, 20);
    assert_eq!(m.hint(), mod_grid::DELETE);
    m.delete_selected();
    assert_eq!(m.hint(), mod_grid::PRIME);
}

/// The browser's CLEAR hint on a filled slot alone.
#[test]
fn the_browser_offers_clear_on_a_filled_slot() {
    let ui = ui_for("sound_browser");
    let pool = ui.project().pool();
    let filled = (0..browser::TOTAL_ENTRIES)
        .find(|&e| browser::slot_at(e).is_some_and(|s| pool.get(s).is_some()))
        .expect("a factory Sound");
    let empty = (0..browser::TOTAL_ENTRIES)
        .find(|&e| browser::slot_at(e).is_some_and(|s| pool.get(s).is_none()))
        .expect("an empty slot");
    let ink = |cursor| {
        let mut fb = screen::Fb::new();
        browser::draw(&mut fb, pool, PartId::ALL[0], cursor, 0, true);
        (0..120)
            .flat_map(|x| (277..=284).map(move |y| (x, y)))
            .filter(|&(x, y)| fb.at(x, y) == theme::INK)
            .count()
    };
    assert!(ink(filled) > 0);
    assert_eq!(ink(empty), 0);
}

/// Every hint fits the screen less its margins.
#[test]
fn every_hint_fits() {
    let cells = [
        OrbitCell::Ring,
        OrbitCell::Degree,
        OrbitCell::Cursor,
        OrbitCell::Set,
        OrbitCell::Note,
        OrbitCell::Learn,
        OrbitCell::Clear,
        OrbitCell::Play,
        OrbitCell::Scene,
        OrbitCell::Save,
        OrbitCell::Switch,
    ];
    let mute = {
        let ui = ui_for("orbit_rhythm");
        band(&ui, &ORBIT_RHYTHM, 5).1
    };
    let all = cells
        .iter()
        .flat_map(|c| [c.hint(true), c.hint(false)])
        .chain([
            mute,
            mod_grid::DELETE,
            mod_grid::PRIME,
            browser::HINT,
            browser::CLEAR_HINT,
        ]);
    for h in all {
        println!("{:3} {h:?}", hint::width(h));
        assert!(hint::width(h) <= hint::MAX_W, "{h:?}: {}", hint::width(h));
    }
}

/// RING's value, its redraw key, is one of RINGS × 6 within 0..=1: ring 5
/// on Part 6 is 1, not past it.
#[test]
fn ring_value_stays_within_one() {
    let mut ui = Box::new(UiState::new());
    let mut seen = Vec::new();
    for p in PartId::ALL {
        ui.project_mut()
            .orbit_mut()
            .edit(&mut |s| s.rings.iter_mut().for_each(|r| r.target = Target::Part(p)));
        for r in RingIdx::ALL {
            let v = OrbitCell::Ring.value(ui.project().orbit(), ui.orbit(), r);
            assert!((0.0..=1.0).contains(&v), "ring {r:?} on {p:?}: {v}");
            assert!(!seen.contains(&v), "ring {r:?} on {p:?} shares {v}");
            seen.push(v);
        }
    }
    assert!(seen.contains(&1.0));
}

/// THEME's HINTS, turned OFF on its page: NOTE's band reads its cell
/// alone, as do the matrix's and the browser's; ON, the hint is back.
#[test]
fn hints_off_shows_no_hint() {
    use chimera_core::ui::theme_settings::ThemeSettings;
    let mut ui = Box::new(UiState::new());
    assert!(ui.theme().hints, "ON by default");
    screen::to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    feed(&mut ui, Input::turn(E, -1));
    assert!(!ui.theme().hints, "E turns HINTS");

    let mut ui = Box::new(ui_for("orbit_notes_step"));
    let on = hint_ink(&ui);
    ui.set_theme(ThemeSettings {
        hints: false,
        ..ThemeSettings::DEFAULT
    });
    assert!(on > 0);
    assert_eq!(hint_ink(&ui), 0, "NOTE: no hint");
    assert_eq!(
        band(&ui, &ORBIT_NOTES, 5).0,
        "STEP 4 OF 8",
        "its reading stays"
    );
    let (mut a, mut b) = (screen::Fb::new(), screen::Fb::new());
    let ui = ui_for("sound_browser");
    browser::draw(&mut a, ui.project().pool(), PartId::ALL[0], 0, 0, true);
    browser::draw(&mut b, ui.project().pool(), PartId::ALL[0], 0, 0, false);
    assert_ne!(a.hash(), b.hash(), "the browser's go too");
}
