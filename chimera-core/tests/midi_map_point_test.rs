//! MIDI MAP's pointing (ADR 0288, chimera-releases#18): `+ NEW BINDING`
//! ends the list; EDIT on it walks the box's own pages, a turn points at a
//! cell and EDIT binds it, on the first CC no row uses; EDIT on a Sound row
//! changes only its target. MENU cancels. No controller is plugged in.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::midi_map::{
    Binding, CcKind, Feedback, Input, LearnCc, MAP_LEN, Mappable, MidiMap, Span,
};
use chimera_core::params::FilterParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::hint::{self, Gesture, Hint};
use chimera_core::ui::learn::{Aim, Learn, Pointer};
use chimera_core::ui::settings::midi_map::MapRow;
use chimera_core::ui::settings::view::RowLook;
use chimera_core::ui::theme;
use chimera_hal::ButtonId::{B2, Edit, Menu, Mix, Plus, Seq};
use chimera_hal::EncoderId::{self, A};
use chimera_midi::rel::ByteEncoding;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use screen::{Fb, Input as Key, feed, render_ui, tap};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);

const BINDS: Hint = &[Gesture("EDIT", "BINDS"), Gesture("MENU", "CANCELS")];
const WALKS: Hint = &[Gesture("A-F", "POINT"), Gesture("MENU", "CANCELS")];

fn at(a: ParamAddr) -> Mappable {
    Mappable::new(a).unwrap()
}

fn bind(input: Input, target: ParamAddr) -> Binding {
    Binding {
        input,
        target: at(target),
        span: Span::FULL,
        feedback: Feedback::Off,
    }
}

fn to_filter(ui: &mut UiState) {
    for _ in 0..12 {
        if ui.page_def().id == FILTER.id {
            break;
        }
        feed(ui, Key::press(Plus));
    }
    assert_eq!(ui.page_def().id, FILTER.id);
}

/// Back to Part 1's first node, then on and down to PIT.
fn to_pit(ui: &mut UiState) {
    for _ in 0..12 {
        feed(ui, Key::press(chimera_hal::ButtonId::Minus));
    }
    screen::to_pitch(ui, chimera_core::params::EngineType::Algo);
}

/// Part 1 on INIT Algo holding `entries`, SETTINGS opened from FILTER,
/// MIDI MAP open, the bar on row `row`.
fn page(entries: &[Binding], row: u8) -> Box<UiState> {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    let mut m = MidiMap::EMPTY;
    for e in entries {
        m.learn(e.input, e.target).unwrap();
        let n = m.len() - 1;
        m.set(n, *e);
    }
    ui.project_mut().edit_part(P1).sound.map = m;
    to_filter(&mut ui);
    screen::to_leaf(&mut ui, &["PART"]);
    for _ in 1..chimera_core::ui::settings::tree::PART.len() {
        feed(&mut ui, Key::press(Plus));
    }
    feed(&mut ui, Key::press(Edit));
    feed(&mut ui, Key::turn(A, row as i8));
    assert_eq!(bar(&ui), row);
    ui
}

fn bar(ui: &UiState) -> u8 {
    ui.location().settings().expect("on MIDI MAP").row()
}

fn map(ui: &UiState) -> MidiMap {
    ui.project().part(P1).sound.map
}

fn edit(ui: &mut UiState) {
    feed(ui, Key::press(Edit));
}

fn pointing(ui: &UiState) -> Option<(Aim, Pointer)> {
    match ui.learn() {
        Learn::Pointing(p) => Some((p.aim, p.at)),
        _ => None,
    }
}

fn status(ui: &UiState) -> String {
    ui.status_text()
        .map_or(String::new(), |s| s.as_str().into())
}

/// Whether `h` is drawn with its left edge at the margin, baseline `y`:
/// every pixel it lights, lit the same in `shot`.
fn drawn_at(shot: &Fb, h: &[Gesture<'_>], y: i32) -> bool {
    let mut want = Fb::new();
    hint::draw(&mut want, h, theme::MARGIN_X, y);
    let mut any = false;
    for py in (y - 12).max(0)..(y + 4).min(320) {
        for px in 0..240 {
            let w = want.at(px, py);
            if w != Rgb565::BLACK {
                any = true;
                if shot.at(px, py) != w {
                    return false;
                }
            }
        }
    }
    any
}

/// `+ NEW BINDING` is the last row, after the pick and the Sound's rows;
/// with the map full it is dimmed.
#[test]
fn new_binding_ends_the_list() {
    let ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)], 0);
    let b = ui.map_band();
    assert_eq!(b.rows, 3);
    assert!(!b.empty);
    let t = b.text(2);
    assert_eq!(t.row, MapRow::New);
    assert_eq!(t.label.as_str(), "+ NEW BINDING");
    assert_eq!(t.look, RowLook::Normal);
    let v = t.values();
    assert_eq!((v[0].0, v[0].1), ("EDIT", "POINT AT A CELL"));

    let full: Vec<Binding> = (0..MAP_LEN as u8)
        .map(|i| {
            let a = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
            let mut b = bind(Input::cc(40 + i).unwrap(), a);
            b.target = full_target(i);
            b
        })
        .collect();
    let mut ui = page(&full, 0);
    let t = ui.map_band().text(MAP_LEN + 1);
    assert_eq!((t.row, t.look), (MapRow::New, RowLook::Dimmed));
    assert_eq!(t.head.as_str(), "MAP FULL");
    feed(&mut ui, Key::turn(A, MAP_LEN as i8 + 1));
    edit(&mut ui);
    assert_eq!(pointing(&ui), None, "a full map points nowhere");
    assert!(ui.in_settings());
}

/// Sixteen distinct mappable targets: Algo's own and the filter's.
fn full_target(i: u8) -> Mappable {
    use chimera_core::dsp::algo::params::AlgoParams;
    let all = [
        CUTOFF,
        RESO,
        ParamAddr::new(BlockRef::Filter, FilterParams::STAB),
        ParamAddr::new(BlockRef::Part, chimera_core::part::PartParams::LEVEL),
        ParamAddr::new(BlockRef::Part, chimera_core::part::PartParams::PAN),
        ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH),
    ];
    if let Some(&a) = all.get(i as usize) {
        return at(a);
    }
    // The rest: any mappable param, in spec order.
    let mut n = 0;
    for b in [BlockRef::Algo, BlockRef::Filter, BlockRef::Out] {
        for p in 0..64 {
            let a = ParamAddr::new(b, chimera_core::block::ParamId(p));
            if all.contains(&a) {
                continue;
            }
            if let Some(m) = Mappable::new(a) {
                if n == i as usize - all.len() {
                    return m;
                }
                n += 1;
            }
        }
    }
    panic!("not enough targets")
}

/// EDIT on `+ NEW BINDING` goes to the page SETTINGS was opened from,
/// pointing; a turn points at a cell and moves nothing; EDIT binds it on
/// the first CC no row uses, read as absolute JUMP over the full range,
/// echo off, and MIDI MAP opens on the new row.
#[test]
fn new_binding_by_pointing() {
    let mut ui = page(&[bind(Input::cc(1).unwrap(), RESO)], 2);
    edit(&mut ui);
    assert!(!ui.in_settings());
    assert_eq!(ui.page_def().id, FILTER.id, "where SETTINGS was opened");
    assert_eq!(pointing(&ui), Some((Aim::New, Pointer::Walking)));
    assert_eq!(status(&ui), "NEW BINDING");
    let before = ui.project().rev();
    let e_cut = enc_of(&ui, CUTOFF);
    feed(&mut ui, Key::turn(e_cut, 3));
    assert_eq!(ui.project().rev(), before, "a turn points, never edits");
    assert!(matches!(
        pointing(&ui),
        Some((Aim::New, Pointer::At(c))) if c.page == FILTER.id
    ));
    edit(&mut ui);
    assert_eq!(pointing(&ui), None);
    let m = map(&ui);
    assert_eq!(m.len(), 2);
    // CC 1 is RESO's: the next free number is 2.
    assert_eq!(m.entries()[1], bind(Input::cc(2).unwrap(), CUTOFF));
    assert!(ui.in_settings());
    assert_eq!(bar(&ui), 2, "on the new row");
    assert_eq!(ui.map_band().text(2).row, MapRow::Own);
    assert_eq!(ui.map_band().text(3).row, MapRow::New);
}

/// Before a turn points, EDIT walks the pages as ever (down a sub-page
/// under STACK) and binds nothing.
#[test]
fn edit_walks_until_a_turn_points() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    let def = ui.page_def().id;
    edit(&mut ui);
    assert_ne!(ui.page_def().id, def, "EDIT went down");
    assert_eq!(pointing(&ui), Some((Aim::New, Pointer::Walking)));
    assert!(map(&ui).is_empty());
}

/// MENU cancels: MIDI MAP again, the bar on the row pointing began from,
/// the map untouched.
#[test]
fn menu_cancels() {
    let e = bind(Input::cc(74).unwrap(), CUTOFF);
    for row in [1u8, 2] {
        let mut ui = page(&[e], row);
        edit(&mut ui);
        assert!(pointing(&ui).is_some());
        feed(&mut ui, Key::turn(EncoderId::B, 1));
        tap(&mut ui, Menu);
        assert_eq!(pointing(&ui), None);
        assert!(ui.in_settings());
        assert_eq!(bar(&ui), row);
        assert_eq!(map(&ui).entries(), &[e]);
    }
}

/// EDIT on a Sound row points at its new target: only the target changes,
/// its control, kind, range and echo kept.
#[test]
fn retarget_keeps_the_rest() {
    let cc21 = LearnCc::new(21).unwrap();
    let e = Binding {
        input: Input::Cc(cc21, CcKind::Relative(ByteEncoding::TwosComplement)),
        target: at(CUTOFF),
        span: Span { min: 40, max: 200 },
        feedback: Feedback::Value,
    };
    let mut ui = page(&[bind(Input::cc(74).unwrap(), RESO), e], 2);
    edit(&mut ui);
    assert_eq!(
        pointing(&ui),
        Some((Aim::Retarget(at(CUTOFF)), Pointer::Walking))
    );
    assert_eq!(status(&ui), "NEW TARGET");
    // FILTER's FREQ is CUTOFF itself: its own target binds again.
    let e_cut = enc_of(&ui, CUTOFF);
    feed(&mut ui, Key::turn(e_cut, 1));
    assert!(ui.status_text().is_some());
    // To PIT's first mappable cell.
    to_pit(&mut ui);
    assert_eq!(pointing(&ui).map(|p| p.1), Some(Pointer::Walking));
    let (slot, a) = first_mappable(&ui);
    feed(&mut ui, Key::turn(chimera_hal::ALL_ENCODERS[slot], 1));
    edit(&mut ui);
    assert_eq!(pointing(&ui), None);
    let m = map(&ui);
    assert_eq!(m.len(), 2);
    assert_eq!(m.entries()[1], Binding { target: at(a), ..e });
    assert_eq!(bar(&ui), 2);
}

/// A cell another row holds is said, `ALREADY BOUND`, and EDIT there walks.
#[test]
fn a_bound_cell_is_refused() {
    let mut ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)], 2);
    edit(&mut ui);
    let e_cut = enc_of(&ui, CUTOFF);
    feed(&mut ui, Key::turn(e_cut, 1));
    assert_eq!(status(&ui), "ALREADY BOUND");
    let def = ui.page_def().id;
    edit(&mut ui);
    assert_ne!(ui.page_def().id, def, "EDIT walked");
    assert_eq!(map(&ui).len(), 1);
}

/// Another Part's key ends pointing; MIX + SEQ leaves it be (learn is not
/// armed); a layer row's EDIT points nowhere.
#[test]
fn other_keys() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    feed(&mut ui, Key::chord(Mix, Seq).at(10_000));
    feed(&mut ui, Key::release(Seq).and_held(Mix).at(10_100));
    feed(&mut ui, Key::release(Mix).at(10_133));
    assert_eq!(pointing(&ui), Some((Aim::New, Pointer::Walking)));
    feed(&mut ui, Key::press(B2));
    assert_eq!(pointing(&ui), None);
    assert!(!ui.in_settings());
}

/// The hint says what EDIT and MENU do: on PIT's focus band, and in place
/// of the status line on FILTER's BigViz.
#[test]
fn the_hint_is_drawn() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    assert!(drawn_at(&render_ui(&ui), WALKS, theme::BIGVIZ_STATUS_Y));
    let e_cut = enc_of(&ui, CUTOFF);
    feed(&mut ui, Key::turn(e_cut, 1));
    assert!(drawn_at(&render_ui(&ui), BINDS, theme::BIGVIZ_STATUS_Y));
    to_pit(&mut ui);
    assert!(drawn_at(&render_ui(&ui), WALKS, hint::FOCUS_Y));
    let (slot, _) = first_mappable(&ui);
    feed(&mut ui, Key::turn(chimera_hal::ALL_ENCODERS[slot], 1));
    assert_eq!(status(&ui), "NEW BINDING");
    assert!(drawn_at(&render_ui(&ui), BINDS, hint::FOCUS_Y));
}

/// The words fit their lines.
#[test]
fn words_fit() {
    for h in [BINDS, WALKS] {
        assert!(hint::width(h) <= theme::CONTENT_W, "{h:?}");
    }
    let ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)], 1);
    let t = ui.map_band().text(1);
    assert!(hint::width(t.hint()) <= theme::CONTENT_W, "{:?}", t.hint());
    assert_eq!(t.hint()[2], Gesture("EDIT", "TARGET"));
    use chimera_core::ui::settings::midi_map::{FIXED, KEYS};
    for h in [KEYS, FIXED] {
        assert!(
            hint::width(h) <= theme::CONTENT_W,
            "{h:?}: {}",
            hint::width(h)
        );
    }
    assert_eq!(
        ui.legend_for_test(),
        Some("A MOVE · MIX+MINUS DELETES"),
        "a Sound row's legend stays"
    );
    let ui = page(&[], 1);
    assert_eq!(ui.legend_for_test(), Some("EDIT RUN · MENU BACK"));
}

/// The encoder of the slot showing `a` on the page shown.
fn enc_of(ui: &UiState, a: ParamAddr) -> EncoderId {
    use chimera_core::addr::Op;
    use chimera_core::ui::view::{SlotCtx, view};
    let ctx = SlotCtx::read(&ui.project().part(P1).sound.params, Op::A);
    let def = ui.page_def();
    let i = (0..def.params.len())
        .find(|&i| view(def, i, &ctx).addr() == Some(a))
        .expect("on the page");
    chimera_hal::ALL_ENCODERS[i]
}

/// The page's first mappable slot and its param.
fn first_mappable(ui: &UiState) -> (usize, ParamAddr) {
    use chimera_core::addr::Op;
    use chimera_core::ui::view::{SlotCtx, view};
    let ctx = SlotCtx::read(&ui.project().part(P1).sound.params, Op::A);
    let def = ui.page_def();
    (0..def.params.len())
        .find_map(|i| {
            let a = view(def, i, &ctx).addr()?;
            Mappable::new(a).map(|_| (i, a))
        })
        .expect("a mappable cell")
}

/// MOD MATRIX while pointing: its routes are no cell to bind, said on its
/// hint line, and A and B still move its cursor; E edits no amount.
#[test]
fn mod_matrix_says_not_mappable_and_moves() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    to_mod_matrix(&mut ui);
    assert!(pointing(&ui).is_some());
    const NOT: Hint = &[Gesture("NOT MAPPABLE", ""), Gesture("MENU", "CANCELS")];
    let shot = render_ui(&ui);
    assert!((0..320).any(|y| drawn_at(&shot, NOT, y)), "said");
    let m = &ui.matrix_state;
    let (row, col, scroll) = (m.sel_row, m.sel_col, m.scroll_x);
    let rev = ui.project().rev();
    feed(&mut ui, Key::turn(EncoderId::A, 1));
    feed(&mut ui, Key::turn(EncoderId::B, 1));
    feed(&mut ui, Key::turn(EncoderId::E, 5));
    let m = &ui.matrix_state;
    assert_eq!(
        (m.sel_row, m.sel_col),
        (row + 1, col + 1),
        "A and B move it"
    );
    feed(&mut ui, Key::turn(EncoderId::D, 3));
    assert_ne!(ui.matrix_state.scroll_x, scroll, "D scrolls it");
    // MIX + MINUS on a route deletes nothing.
    let (r, c) = present_route(&ui).expect("INIT has a route");
    (ui.matrix_state.sel_row, ui.matrix_state.sel_col) = (r, c);
    feed(&mut ui, Key::chord(Mix, chimera_hal::ButtonId::Minus));
    assert!(
        ui.matrix_state.is_present(r, c),
        "MIX + MINUS deleted nothing"
    );
    assert_eq!(
        ui.project().rev(),
        rev,
        "no amount edited, no route deleted"
    );
    // EDIT walks down MOD's sub-list, still pointing at nothing.
    edit(&mut ui);
    assert_ne!(
        ui.page_def().id,
        chimera_core::ui::block_registry::MOD_MATRIX.id
    );
    assert_eq!(pointing(&ui), Some((Aim::New, Pointer::Walking)));
    assert!(map(&ui).is_empty());
}

/// On to the MOD node, whose home is the matrix.
fn to_mod_matrix(ui: &mut UiState) {
    use chimera_core::ui::block_registry::MOD_MATRIX;
    for _ in 0..12 {
        if ui.page_def().id == MOD_MATRIX.id {
            break;
        }
        feed(ui, Key::press(Plus));
    }
    assert_eq!(ui.page_def().id, MOD_MATRIX.id);
}

/// The matrix's first route, if any.
fn present_route(ui: &UiState) -> Option<(usize, usize)> {
    let m = &ui.matrix_state;
    (0..m.rows()).find_map(|r| {
        (0..m.num_cols())
            .find(|&c| m.is_present(r, c))
            .map(|c| (r, c))
    })
}

/// On a DRUM Part, MOD MATRIX's cursor under pointing is no pad edit:
/// PAD FOLLOW IDLE still follows the next hit.
#[test]
fn drum_matrix_turns_under_pointing_leave_the_pad_free() {
    use chimera_core::dsp::drum::types::PadId;
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part_engine(&mut ui, P1, chimera_core::params::EngineType::Drum);
    screen::to_leaf(&mut ui, &["PART"]);
    for _ in 1..chimera_core::ui::settings::tree::PART.len() {
        feed(&mut ui, Key::press(Plus));
    }
    feed(&mut ui, Key::press(Edit));
    feed(&mut ui, Key::turn(A, -1));
    edit(&mut ui);
    assert!(pointing(&ui).is_some());
    to_mod_matrix(&mut ui);
    feed(&mut ui, Key::turn(EncoderId::A, 1));
    feed(&mut ui, Key::turn(EncoderId::D, 1));
    let nine = PadId::ALL[8];
    ui.pad_struck(P1, nine);
    assert_eq!(ui.selected_pad(P1), nine, "no edit held the pad");
}

/// Part 1 loaded with INIT over its edits, as a program change's load
/// lands whatever the Part held.
fn load_init(ui: &mut UiState) {
    use chimera_core::project::{PartFrom, PartSource, ReplaceGuard};
    let src = PartSource {
        part: P1,
        from: PartFrom::Init(chimera_core::params::EngineType::Algo),
    };
    let c = match ReplaceGuard::check(ui.project(), ui.template(), src) {
        Ok(c) => c,
        Err(n) => n.into_pending().anyway(ui.project()),
    };
    ui.project_mut().replace_part(c).expect("INIT loads");
}

/// A load on the pointing Part ends pointing (a program change, a library
/// load): no bind lands in the newly loaded Sound's map. Heard or not
/// before EDIT, the INIT map's CUTOFF row stays.
#[test]
fn a_load_ends_pointing() {
    use chimera_hal::Ms;
    for heard in [false, true] {
        let mut ui = page(&[bind(Input::cc(21).unwrap(), CUTOFF)], 1);
        edit(&mut ui);
        load_init(&mut ui);
        let init = map(&ui);
        if heard {
            ui.hear_ccs(
                core::iter::empty::<chimera_core::midi_in::CtrlEvent>(),
                Ms(10_000),
                true,
            );
            assert_eq!(pointing(&ui), None, "heard: ended");
        }
        // INIT maps CC 74 to CUTOFF: a retarget of CUTOFF's row would
        // move that row to PIT's cell.
        to_pit(&mut ui);
        let (slot, _) = first_mappable(&ui);
        feed(&mut ui, Key::turn(chimera_hal::ALL_ENCODERS[slot], 1));
        edit(&mut ui);
        assert_eq!(pointing(&ui), None);
        assert_eq!(map(&ui), init, "INIT's map untouched (heard {heard})");
    }
}

/// The Sound rung ends pointing: its EDIT loads a Sound, which would
/// replace the map pointing writes.
#[test]
fn the_sound_rung_ends_pointing() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    feed(&mut ui, Key::chord(Edit, chimera_hal::ButtonId::B1));
    assert!(ui.location().browse().is_some(), "on the Sound rung");
    assert_eq!(pointing(&ui), None);
    edit(&mut ui);
    assert!(map(&ui).is_empty());
}

/// MENU held cancels, as tapped: back on MIDI MAP, nothing saved.
#[test]
fn menu_held_cancels() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    screen::hold(&mut ui, Menu);
    assert_eq!(pointing(&ui), None);
    assert!(ui.in_settings());
    assert_eq!(bar(&ui), 1);
}

/// A new row never takes the mod wheel or a built-in control's CC: with
/// 2-5 taken (6 is data entry) it skips 7 (LEVEL) for 8.
#[test]
fn a_new_row_skips_the_built_in_ccs() {
    let taken: Vec<Binding> = (2..=5)
        .map(|n| {
            let mut b = bind(Input::cc(n).unwrap(), CUTOFF);
            b.target = full_target(n - 2);
            b
        })
        .collect();
    let mut ui = page(&taken, 5);
    edit(&mut ui);
    to_pit(&mut ui);
    let (slot, a) = first_mappable(&ui);
    feed(&mut ui, Key::turn(chimera_hal::ALL_ENCODERS[slot], 1));
    edit(&mut ui);
    assert_eq!(map(&ui).entries()[4], bind(Input::cc(8).unwrap(), a));
}

/// A retarget whose row went meanwhile (a load) is said, not silent.
#[test]
fn a_bind_that_cannot_happen_is_said() {
    use chimera_core::ui::busy::ToastStep;
    let mut ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)], 1);
    edit(&mut ui);
    ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    let e_res = enc_of(&ui, RESO);
    feed(&mut ui, Key::turn(e_res, 1));
    edit(&mut ui);
    assert_eq!(pointing(&ui), None);
    match ui.step_toast(0) {
        ToastStep::Show(l) => assert_eq!(l.as_str(), "BINDING GONE"),
        s => panic!("no toast: {s:?}"),
    }
}

/// A pointed cell binds only while it is the one focused: a turn of
/// another cell points there instead, and EDIT binds that one.
#[test]
fn edit_binds_the_cell_pointed_at() {
    let mut ui = page(&[], 1);
    edit(&mut ui);
    let e_cut = enc_of(&ui, CUTOFF);
    feed(&mut ui, Key::turn(e_cut, 1));
    let e_res = enc_of(&ui, RESO);
    feed(&mut ui, Key::turn(e_res, 1));
    edit(&mut ui);
    assert_eq!(map(&ui).entries()[0].target, at(RESO));
}

/// `+ NEW BINDING`'s lines name no key it doesn't answer.
#[test]
fn the_new_row_names_only_its_keys() {
    let ui = page(&[], 1);
    let t = ui.map_band().text(1);
    assert_eq!(t.head.as_str(), "POINT AT A CELL");
    assert_eq!(t.tail.as_str(), "ITS CC IS SET ON ITS ROW");
    assert!(t.hint().is_empty());
}
