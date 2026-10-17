//! Gesture hints (ADR 0086): each MIX gesture's hint in the focus band of
//! the cell it acts on, there while that cell is the touched one and gone
//! when another is; the matrix's by its cursor's route, the browser's by
//! its row. One component draws them all, each within the screen.

#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::chorus::{self, ChorusMode};
use chimera_core::dsp::reverb::ReverbType;
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::scene::Target;
use chimera_core::params::ParamSnapshot;
use chimera_core::params::{FolderParams, OutParams};
use chimera_core::project::PartId;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
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
    match focus(def, slot, &ctx, &f, true, |_| 0.0) {
        Some(fo) => (fo.label.as_str().into(), fo.hint),
        None => (
            view(def, slot, &ctx).label().into(),
            view(def, slot, &ctx).hint(&ctx),
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
    let mut m = MatrixState::new();
    m.rebuild_sources(&["SRC"; 8]);
    assert_eq!(m.hint(), mod_grid::PRIME, "no route on the fixed FREQ");
    m.set(m.sel_source(), 0, 20);
    assert_eq!(m.hint(), mod_grid::DELETE);
    m.delete_selected();
    assert_eq!(m.hint(), mod_grid::PRIME);
}

/// MIX + MINUS on every route knob of every type: the band names the
/// delete (ADR 0086; FM1, FM2, FM3, QM on CLEAN; ENV AMT, KEYBD on 2040).
#[test]
fn route_knobs_name_the_delete() {
    use chimera_core::dsp::filter::FilterKind;
    use chimera_core::ui::block_registry::{FILTER, FILTER_MOD};
    let mut ui = ui_for("flt_mod_clean");
    // A knob: its page, its slot and its label.
    type Knob = (&'static BlockDef, usize, &'static str);
    let knobs: [(FilterKind, &[Knob]); 2] = [
        (
            FilterKind::Svf,
            &[
                (&FILTER, 4, "FM1"),
                (&FILTER, 5, "FM2"),
                (&FILTER_MOD, 1, "FM3"),
                (&FILTER_MOD, 3, "QM"),
            ],
        ),
        (
            FilterKind::K2040,
            &[
                (&FILTER, 3, "ENV AMT"),
                (&FILTER, 4, "KEYBD"),
                (&FILTER_MOD, 1, "FM3"),
                (&FILTER_MOD, 3, "QM"),
            ],
        ),
    ];
    for (kind, cells) in knobs {
        ui.params_mut().filter.set_kind(kind);
        let ctx = SlotCtx::read(ui.params(), Op::A);
        for &(def, slot, label) in cells {
            let v = view(def, slot, &ctx);
            assert_eq!(
                (v.label(), v.hint(&ctx)),
                (label, mod_grid::DELETE),
                "{kind:?}"
            );
        }
    }
}

/// The cursor on OP5's X cell (row 8, a registry column): the hint says
/// what a tap can reach, and it fits.
#[test]
fn x_cell_names_what_a_tap_reaches() {
    let ui = ui_for("mod_matrix_algo_taps");
    assert!(matrix_hint_is(&ui, mod_grid::X_OP5));
    for h in [mod_grid::X_OP5, mod_grid::X_OP6] {
        assert!(
            hint::width(h) <= theme::CONTENT_W,
            "{h:?}: {}",
            hint::width(h)
        );
    }
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
        browser::draw(
            &mut fb,
            ui.project(),
            browser::BrowserView::Project,
            PartId::ALL[0],
            cursor,
            0,
            true,
            None,
        );
        (0..120)
            .flat_map(|x| (browser::CLEAR_Y - 7..=browser::CLEAR_Y).map(move |y| (x, y)))
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
            mod_grid::X_OP5,
            mod_grid::X_OP6,
        ]);
    for h in all {
        println!("{:3} {h:?}", hint::width(h));
        assert!(
            hint::width(h) <= chimera_core::ui::theme::CONTENT_W,
            "{h:?}: {}",
            hint::width(h)
        );
    }
}

/// The pool's lines at their longest (999K) fit too: the refusal, the
/// clear's bytes, the save's cost and its WON'T FIT.
#[test]
fn every_pool_hint_fits() {
    use chimera_core::project::PoolFull;
    use chimera_core::project::arena::Cost;
    use chimera_core::ui::settings::prompt;
    let k999 = Cost::of(999 * 1024);
    assert_eq!(k999.kib_label().as_str(), "999K");
    let w = theme::CONTENT_W;
    let needs = prompt::pool_needs(PoolFull {
        needs: k999,
        free: k999,
    });
    assert_eq!(needs.as_str(), "NEEDS 999K, 999K FREE");
    for line in [prompt::POOL_FULL, needs.as_str()] {
        let text =
            chimera_core::ui::draw::text_width(&theme::FONT_LABEL, line, theme::LABEL_TRACKING);
        assert!(text <= w, "{line}: {text}");
    }
    let (clear, save, wont) = (
        browser::clear_words(k999),
        browser::save_words(Some(k999)),
        browser::save_words(None),
    );
    let lines = [
        vec![Gesture(browser::CLEAR_KEYS, clear.as_str())],
        vec![Gesture(browser::SAVE_KEYS, save.as_str())],
        vec![Gesture(browser::SAVE_KEYS, wont.as_str())],
    ];
    for l in &lines {
        assert!(hint::width(l) <= w, "{l:?}: {}", hint::width(l));
    }
    // A clear that orphans a source: its own line left of the meter's
    // percentage, the longest name cut to fit; then the purge's question.
    let pct = chimera_core::ui::draw::text_width(&theme::FONT_LABEL, "100%", 0);
    assert!(theme::MARGIN_X + browser::ORPHAN_W + 6 <= theme::VIZ_RIGHT - pct);
    let name = chimera_core::name::SoundName::new("WWWWWWWWWWWWWWWW").unwrap();
    let orphan = browser::orphan_words(name, k999);
    assert!(
        orphan.as_str().ends_with("W 999K UNUSED"),
        "{}",
        orphan.as_str()
    );
    let text = chimera_core::ui::draw::text_width(
        &theme::FONT_LABEL,
        orphan.as_str(),
        theme::LABEL_TRACKING,
    );
    assert!(text <= browser::ORPHAN_W, "{}: {text}", orphan.as_str());
    // The toast after such a clear: the name cut so it fits.
    let after = prompt::unused_after_clear(name, k999);
    assert!(
        after.as_str().ends_with("W 999K UNUSED · PURGE FREES IT"),
        "{}",
        after.as_str()
    );
    assert!(
        chimera_core::ui::busy::toast_fits(after.as_str()),
        "{}",
        after.as_str()
    );
    let choir = prompt::unused_after_clear(
        chimera_core::name::SoundName::new("CHOIR").unwrap(),
        Cost::of(41 * 1024),
    );
    assert_eq!(choir.as_str(), "CHOIR 41K UNUSED · PURGE FREES IT");
    let purge = prompt::purge_question(chimera_core::project::Unused {
        count: 32,
        cost: k999,
    });
    assert_eq!(purge.as_str(), "PURGE 32 SOURCES? FREES 999K");
    // A load over the arena, at its most.
    let most = chimera_core::project::ProjectNote::DidntFit {
        sounds: 128,
        sources: 32,
    }
    .line();
    assert_eq!(most.as_str(), "LOADED · 128 SOUNDS, 32 SRC DIDN'T FIT");
    assert!(
        chimera_core::ui::busy::toast_fits(most.as_str()),
        "{}",
        most.as_str()
    );
    let text = chimera_core::ui::draw::text_width(
        &theme::FONT_LABEL,
        purge.as_str(),
        theme::LABEL_TRACKING,
    );
    assert!(text <= w, "{}: {text}", purge.as_str());
}

/// RING's value, its redraw key, is one of RINGS × 6 within 0..=1: ring 5
/// on Part 6 is 1, not past it.
#[test]
fn ring_value_stays_within_one() {
    let mut ui = Box::new(crate::screen::stack_ui());
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
    let mut ui = Box::new(crate::screen::stack_ui());
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
    browser::draw(
        &mut a,
        ui.project(),
        browser::BrowserView::Project,
        PartId::ALL[0],
        0,
        0,
        true,
        None,
    );
    browser::draw(
        &mut b,
        ui.project(),
        browser::BrowserView::Project,
        PartId::ALL[0],
        0,
        0,
        false,
        None,
    );
    assert_ne!(a.hash(), b.hash(), "the browser's go too");
}

/// HINTS turned off between frames, through the dirty-region path: the
/// focus band redraws at once, as a full render draws it; NOTE's hint goes,
/// and CLEAR's label keeps `MIX +`.
#[test]
fn hints_off_redraws_through_the_dirty_regions() {
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme_settings::ThemeSettings;
    let mut ui = Box::new(ui_for("orbit_notes_step"));
    let mut fb = screen::Fb::new();
    let frame = |ui: &mut UiState, fb: &mut screen::Fb| {
        ui.render_dirty_with_scope(fb, &PerfStats::zero(), &screen::scope_fixture());
    };
    frame(&mut ui, &mut fb);
    assert_eq!(fb.hash(), render_ui(&ui).hash(), "hints on");
    ui.set_theme(ThemeSettings {
        hints: false,
        ..ThemeSettings::DEFAULT
    });
    frame(&mut ui, &mut fb);
    assert_eq!(fb.hash(), render_ui(&ui).hash(), "NOTE's hint gone at once");
    feed(&mut ui, Input::turn(E, 1)); // CLEAR touched; no MIX, no change
    frame(&mut ui, &mut fb);
    assert_eq!(fb.hash(), render_ui(&ui).hash(), "CLEAR");
    let r = ui.location().orbit().unwrap().ring();
    let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
    let ctx = SlotCtx::read(&ui.project().part(ui.active_part).sound.params, Op::A).on_ring(r);
    let fo = focus(&ORBIT_NOTES, 4, &ctx, &f, false, |_| 0.0).unwrap();
    assert_eq!(fo.label.as_str(), "MIX + CLEAR");
}

/// HINTS turned either way between frames redraws every page's focus band
/// and hint through the dirty-region path, as a full render draws them:
/// no page has to key its band on HINTS itself.
#[test]
fn a_hints_change_redraws_every_page() {
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme_settings::ThemeSettings;
    let frame = |ui: &mut UiState, fb: &mut screen::Fb| {
        let audio = screen::audio_fixture();
        ui.render_dirty_with_audio(
            fb,
            &PerfStats::zero(),
            Some(&audio),
            &screen::scope_fixture(),
        );
    };
    for &(name, _) in screen::CASES {
        let mut ui = Box::new(ui_for(name));
        let mut fb = screen::Fb::new();
        frame(&mut ui, &mut fb);
        for hints in [false, true] {
            ui.set_theme(ThemeSettings {
                hints,
                ..ThemeSettings::DEFAULT
            });
            frame(&mut ui, &mut fb);
            assert_eq!(fb.hash(), render_ui(&ui).hash(), "{name}: hints {hints}");
        }
    }
}

/// TAPE Task 17: SOURCE's hints at their longest (999K) fit 216 px:
/// `MIX + PLUS LOADS · 999K`, and `WON'T FIT · NEEDS 999K, 999K FREE`.
#[test]
fn source_hints_fit() {
    use chimera_core::project::arena::Cost;
    use chimera_core::ui::source_cell::{LOAD_KEYS, loads_words, wont_fit_words};
    let k999 = Cost::of(999 * 1024);
    let (loads, wont) = (loads_words(k999), wont_fit_words(k999, k999));
    assert_eq!(loads.as_str(), "LOADS · 999K");
    assert_eq!(wont.as_str(), "WON'T FIT · NEEDS 999K, 999K FREE");
    let w = theme::CONTENT_W;
    let g = [Gesture(LOAD_KEYS, loads.as_str())];
    assert!(hint::width(&g) <= w, "{}", hint::width(&g));
    // Past the measure, the line closes its tracking (a 4-digit need).
    for (n, f) in [(999, 999), (1024, 150)] {
        let wont = wont_fit_words(Cost::of(n * 1024), Cost::of(f * 1024));
        let t = chimera_core::ui::draw::text_width(
            &theme::FONT_LABEL,
            wont.as_str(),
            chimera_core::ui::source_cell::tracking(wont.as_str()),
        );
        assert!(t <= w, "{}: {t}", wont.as_str());
    }
}

fn out_hint(block: BlockRef, id: ParamId, ctx: &SlotCtx) -> Hint {
    let def = &reg::FOLDER;
    let i = (0..def.params.len())
        .find(|&i| view(def, i, ctx).addr() == Some(ParamAddr::new(block, id)))
        .expect("cell on AMP");
    view(def, i, ctx).hint(ctx)
}

fn ctx0() -> SlotCtx {
    SlotCtx::read(&ParamSnapshot::default(), Op::A)
}

#[test]
fn reverb_hint_names_the_type() {
    for t in ReverbType::ALL {
        let c = SlotCtx { rev: t, ..ctx0() };
        assert_eq!(
            out_hint(BlockRef::Out, OutParams::SEND3, &c),
            &[Gesture("REVERB ▸", ReverbType::NAMES[t.index()])][..]
        );
    }
}

#[test]
fn chorus_hint_names_the_mode() {
    for (i, name) in chorus::MODE_NAMES.iter().enumerate() {
        let c = SlotCtx {
            chorus: ChorusMode::from_u8(i as u8),
            ..ctx0()
        };
        assert_eq!(
            out_hint(BlockRef::Out, OutParams::SEND1, &c),
            &[Gesture("CHORUS ▸", name)][..]
        );
    }
    assert_eq!(
        out_hint(BlockRef::Out, OutParams::SEND2, &ctx0()),
        &[LEARN][..]
    );
}

/// SYM is AMP's next page: EDIT ▾ under STACK, SEQ ▸ under ROW (ADR 0145).
#[test]
fn fold_vel_mix_hint_sym() {
    use chimera_core::ui::theme_settings::Pages;
    for (pages, want) in [
        (Pages::Stack, &[Gesture("EDIT ▾", "SYM")]),
        (Pages::Row, &[Gesture("SEQ ▸", "SYM")]),
    ] {
        let c = ctx0().on_pages(pages);
        for (b, p) in [
            (BlockRef::Folder, FolderParams::FOLD),
            (BlockRef::Out, OutParams::VCA_VEL),
            (BlockRef::Folder, FolderParams::MIX),
        ] {
            assert_eq!(out_hint(b, p, &c), &want[..], "{pages:?}");
        }
    }
}

#[test]
fn hints_draw() {
    let mut c = ctx0();
    let mut all = vec![];
    for t in ReverbType::ALL {
        c.rev = t;
        all.push(out_hint(BlockRef::Out, OutParams::SEND3, &c));
    }
    for i in 0..4 {
        c.chorus = ChorusMode::from_u8(i);
        all.push(out_hint(BlockRef::Out, OutParams::SEND1, &c));
    }
    all.push(out_hint(BlockRef::Folder, FolderParams::FOLD, &c));
    for h in all {
        for g in h {
            assert!(
                (g.0.to_owned() + g.1)
                    .chars()
                    .all(|ch| ch.is_ascii() || ch == '▸' || ch == '▾'),
                "{h:?}"
            );
        }
        assert!(hint::width(h) <= theme::CONTENT_W, "{h:?}");
    }
}

/// The focus band draws `want`: every pixel of `want` drawn alone sits in
/// it, in its colour (the band holds other ink besides).
fn focus_hint_is(ui: &UiState, want: Hint) -> bool {
    let mut alone = screen::Fb::new();
    hint::draw(&mut alone, want, theme::MARGIN_X, hint::FOCUS_Y);
    let (fb, ground) = (render_ui(ui), alone.at(theme::SCREEN_W - 1, hint::FOCUS_Y));
    (0..theme::SCREEN_W)
        .flat_map(|x| (hint::FOCUS_Y - 7..=hint::FOCUS_Y).map(move |y| (x, y)))
        .filter(|&(x, y)| alone.at(x, y) != ground)
        .all(|(x, y)| fb.at(x, y) == alone.at(x, y))
}

/// The CHORUS send hints the project's chorus MODE as the UI draws it, on
/// AMP and on the mixer's SENDS (a Sound's params hold no FX).
#[test]
fn chorus_send_hint_follows_the_project_chorus() {
    let amp = |ui: &mut UiState| {
        screen::plus(ui, 4);
        feed(ui, Input::turn(A, 1)); // the CHORUS cell
    };
    let sends = |ui: &mut UiState| {
        feed(
            ui,
            Input::chord(chimera_hal::ButtonId::Mix, chimera_hal::ButtonId::B1),
        );
        feed(ui, Input::turn(A, 1));
    };
    for go in [amp, sends] {
        let mut ui = Box::new(crate::screen::stack_ui());
        go(&mut ui);
        screen::settle(&mut ui);
        assert!(
            focus_hint_is(&ui, &[Gesture("CHORUS ▸", "I")]),
            "NEW: mode I"
        );
        ui.project_mut().edit_fx().chorus.mode = ChorusMode::JunoII as u8;
        screen::settle(&mut ui);
        assert!(focus_hint_is(&ui, &[Gesture("CHORUS ▸", "II")]), "mode II");
    }
}

const LEARN: Gesture = Gesture("MIX + SEQ", "LEARNS A CC");

/// Every mappable cell's hint offers MIX + SEQ's learn (MIDI map spec
/// § 4) but where the cell's own gesture leaves it no room (REVERB ▸,
/// CHORUS ▸, EDIT ▾ SYM), every other cell's never, and each fits, under
/// every reverb TYPE and chorus MODE.
#[test]
fn mappable_cells_hint_learn() {
    use chimera_core::midi_map::Mappable;
    let mut seen = 0;
    for chain in reg::ALL_CHAINS {
        for b in chain.blocks {
            for def in core::iter::once(b.def).chain(b.sub_pages.iter().copied()) {
                for t in ReverbType::ALL {
                    for m in 0..4 {
                        let c = SlotCtx {
                            rev: t,
                            chorus: ChorusMode::from_u8(m),
                            ..ctx0()
                        };
                        for i in 0..def.params.len() {
                            let v = view(def, i, &c);
                            let h = v.hint(&c);
                            let mappable = v.addr().and_then(Mappable::new).is_some();
                            let crowded = h.len() == 1
                                && h[0] != LEARN
                                && hint::width(&[h[0], LEARN]) > theme::CONTENT_W;
                            assert_eq!(
                                h.contains(&LEARN),
                                mappable && !crowded,
                                "{} {i}: {h:?}",
                                def.name
                            );
                            assert!(
                                hint::width(h) <= theme::CONTENT_W,
                                "{h:?}: {}",
                                hint::width(h)
                            );
                            seen += usize::from(mappable);
                        }
                    }
                }
            }
        }
    }
    assert!(seen > 0);
}
