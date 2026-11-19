//! A Part's controller map on the unit (#807, ADR 0226): read from the card
//! when picked, played under the Sound's own rows, picked and saved on
//! MIDI MAP's row 0, its rows shown and dropped there.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::midi_in::{Cc, CcEvent};
use chimera_core::midi_map::ctl::{self, ControllerMap};
use chimera_core::midi_map::{Input, Mappable, MidiMap};
use chimera_core::name::SoundName;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::PartId;
use chimera_core::storage::{Card, Ready};
use chimera_core::ui::UiState;
use chimera_core::ui::settings::view::RowLook;
use chimera_hal::store::{Store as _, StoreError};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId, Ms};
use screen::{Input as Key, Rig};

const P1: PartId = PartId::ALL[0];

/// `map_put` is global: a test that moves it, or that reads a map gone
/// from its card (which a move would take to NONE), holds this.
static PUTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn puts() -> std::sync::MutexGuard<'static, ()> {
    PUTS.lock().unwrap_or_else(|e| e.into_inner())
}
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);

fn name(s: &str) -> SoundName {
    SoundName::new(s).unwrap()
}

fn on_card<R>(s: &mut MemStore, f: impl FnOnce(&mut MemStore, &Ready) -> R) -> R {
    let mut card = Card::new();
    let out = card.run(s, |s, r| Ok::<_, StoreError>(f(s, r))).unwrap();
    out.result.unwrap()
}

/// `rows` (CC, target) saved as map `n`, by a Part on ALGO.
fn put_map(s: &mut MemStore, n: &str, rows: &[(u8, ParamAddr)]) {
    let mut own = MidiMap::EMPTY;
    for &(cc, a) in rows {
        own.learn(Input::cc(cc).unwrap(), Mappable::new(a).unwrap())
            .unwrap();
    }
    let mut m = ControllerMap::EMPTY;
    m.absorb(&own, EngineType::Algo).unwrap();
    on_card(s, |s, r| ctl::save(s, r, &name(n), &m)).unwrap();
}

/// Part 1 on INIT ALGO, its map the default's.
fn ui() -> Box<UiState> {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    ui
}

fn cc(n: u8, v: u8) -> CcEvent {
    CcEvent {
        channel: MidiChannel::new(0).unwrap(),
        cc: Cc::new(n).unwrap(),
        value: v,
    }
}

/// `ev` heard, then the ramps run out.
fn hear(ui: &mut UiState, now: &mut u32, ev: CcEvent) {
    *now += 4;
    ui.hear_ccs(core::iter::once(ev), Ms(*now), true);
    for _ in 0..60 {
        *now += 4;
        ui.hear_ccs(core::iter::empty::<CcEvent>(), Ms(*now), true);
    }
}

fn place(ui: &UiState, a: ParamAddr) -> f32 {
    let v = ui
        .project()
        .read_part(P1)
        .block(a.block)
        .unwrap()
        .get(a.param);
    a.spec().unwrap().normalize(v)
}

/// A picked map is read from the card and plays under the Sound's rows:
/// its CC 21 moves the cutoff, but CC 74 is the Sound's (cutoff), not the
/// map's (resonance).
#[test]
fn a_picked_map_plays_under_the_sound_s_rows() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF), (74, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.work();
    let mut now = 100;
    hear(rig.ui, &mut now, cc(21, 0));
    assert!(place(rig.ui, CUTOFF) < 0.01, "CC 21 is the map's cutoff");
    let reso = place(rig.ui, RESO);
    hear(rig.ui, &mut now, cc(74, 127));
    assert!(place(rig.ui, CUTOFF) > 0.99, "CC 74 is the Sound's cutoff");
    assert_eq!(place(rig.ui, RESO), reso, "the map's CC 74 is shadowed");
    // NONE: CC 21 does nothing.
    rig.ui.project_mut().set_controller(P1, None);
    rig.work();
    hear(rig.ui, &mut now, cc(21, 0));
    assert!(place(rig.ui, CUTOFF) > 0.99);
}

/// A map the card doesn't hold plays nothing; another engine's rows play
/// only on that engine.
#[test]
fn a_missing_map_plays_nothing() {
    let _g = puts();
    let s = MemStore::new(1);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("GONE")));
    rig.work();
    let mut now = 100;
    let was = place(rig.ui, CUTOFF);
    hear(rig.ui, &mut now, cc(21, 0));
    assert_eq!(place(rig.ui, CUTOFF), was);
}

/// MIDI MAP: row 0 names the map, the layer's rows follow (a shadowed one
/// dimmed), then the Sound's; B on row 0 picks along NONE and the card's
/// maps.
#[test]
fn row_0_picks_and_the_layer_s_rows_follow() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF), (74, RESO)]);
    put_map(&mut s, "LCXL3", &[(30, RESO)]);
    let mut ui = ui();
    let own = ui.project().part(P1).sound.map.len();
    let mut rig = Rig::new(&mut ui, s);
    rig.to(&["PART", "MIDI MAP"]);
    let b = rig.ui.map_band();
    // The pick, the Sound's rows, `+ NEW BINDING` (ADR 0288).
    assert_eq!(b.rows, 2 + own);
    assert_eq!(b.text(0).label.as_str(), "CONTROLLER");
    assert_eq!(b.text(0).word.as_str(), "NONE");
    assert_eq!(rig.ui.legend_for_test().unwrap(), "B PICK · EDIT SAVE");
    rig.feed(Key::turn(EncoderId::B, 1));
    assert_eq!(rig.ui.project().controller(P1), Some(name("EC4")));
    rig.work();
    let b = rig.ui.map_band();
    assert_eq!(b.rows, 4 + own);
    assert_eq!(b.text(0).word.as_str(), "EC4");
    assert_eq!(b.text(1).label.as_str(), "CC 21  CUTOFF");
    assert_eq!(b.text(1).tail.as_str(), "IN EC4");
    assert_eq!(b.look(1), RowLook::Normal);
    // The Sound's CC 74 holds the control: the map's row is dimmed.
    assert_eq!(b.look(2), RowLook::Dimmed);
    rig.feed(Key::turn(EncoderId::B, 1));
    assert_eq!(rig.ui.project().controller(P1), Some(name("LCXL3")));
    rig.feed(Key::turn(EncoderId::B, 5));
    assert_eq!(rig.ui.project().controller(P1), Some(name("LCXL3")));
    rig.feed(Key::turn(EncoderId::B, -9));
    assert_eq!(rig.ui.project().controller(P1), None);
}

/// EDIT on row 0 names a map and saves the Part's rows into it: the Part
/// picks it, its Sound's rows moved into the map (its Sound edited).
#[test]
fn edit_on_row_0_saves_the_part_s_rows() {
    let s = MemStore::new(1);
    let mut ui = ui();
    let own = ui.project().part(P1).sound.map;
    let mut rig = Rig::new(&mut ui, s);
    rig.to(&["PART", "MIDI MAP"]);
    rig.tap(ButtonId::Edit);
    assert_eq!(rig.ui.naming().unwrap().text(), "CONTROLLER");
    rig.tap(ButtonId::Edit);
    rig.work();
    assert_eq!(rig.ui.project().controller(P1), Some(name("CONTROLLER")));
    assert!(rig.ui.project().part(P1).sound.map.is_empty());
    let mut back = ControllerMap::EMPTY;
    on_card(&mut rig.s, |s, r| {
        ctl::load(s, r, &name("CONTROLLER"), &mut back)
    })
    .unwrap();
    let mut saved = back.layer(EngineType::Algo).0.entries().to_vec();
    let mut was = own.entries().to_vec();
    let key = |b: &chimera_core::midi_map::Binding| format!("{b:?}");
    saved.sort_by_key(key);
    was.sort_by_key(key);
    assert_eq!(saved, was);
    // The page shows the same rows, now the map's.
    rig.work();
    assert_eq!(rig.ui.map_band().rows, 2 + own.len());
    assert_eq!(rig.ui.location().settings().unwrap().row(), 0);
}

/// MIX+MINUS on a layer row asks, then drops the row from the map file.
#[test]
fn mix_minus_drops_a_layer_row_from_the_map() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF), (22, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.bar_to(1);
    rig.feed(Key::chord(ButtonId::Mix, ButtonId::Minus));
    let (q, r) = rig.ui.prompt_words_for_test().expect("a prompt");
    assert_eq!((q.as_str(), r.as_str()), ("DELETE CC 21?", "CUTOFF"));
    rig.feed(Key::turn(EncoderId::A, -9));
    rig.tap(ButtonId::Edit);
    rig.work();
    let mut back = ControllerMap::EMPTY;
    on_card(&mut rig.s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
    let rows = back.layer(EngineType::Algo).0;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.entries()[0].target.addr(), RESO);
}

/// No card: the Part plays no map, and the card work stops asking till
/// the pick changes; another card in the slot reads its maps again.
#[test]
fn no_card_reads_nothing_and_stops_asking() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    s.eject();
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.work();
    let mut now = 100;
    let was = place(rig.ui, CUTOFF);
    hear(rig.ui, &mut now, cc(21, 0));
    assert_eq!(place(rig.ui, CUTOFF), was);
    // Picked again with the card back: read.
    rig.s.insert();
    rig.ui.project_mut().set_controller(P1, None);
    rig.work();
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.work();
    hear(rig.ui, &mut now, cc(21, 0));
    assert!(place(rig.ui, CUTOFF) < 0.01);
}

/// A project load reads its maps from the card again: a map copied in
/// under the same name since plays its new rows.
#[test]
fn a_project_load_reads_its_map_again() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    let f = screen::put_with(&mut s, "LIVE", |p| p.set_controller(P1, Some(name("EC4"))));
    let mut ui = Box::new(screen::stack_ui());
    let mut rig = Rig::new(&mut ui, s);
    let row = |rig: &mut Rig| {
        rig.load(f);
        rig.work();
        rig.to(&["PART", "MIDI MAP"]);
        rig.ui.map_band().text(1).label.as_str().to_owned()
    };
    assert_eq!(row(&mut rig), "CC 21  CUTOFF");
    put_map(&mut rig.s, "EC4", &[(21, RESO)]);
    assert_eq!(row(&mut rig), "CC 21  RESO");
}

/// A layer row whose map has left the card since it was read: the delete
/// is refused, and nothing is written in the map's place.
#[test]
fn a_row_of_a_map_gone_from_the_card_is_not_dropped() {
    let _g = puts();
    use chimera_hal::store::{Dir, FileName, Store};
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF), (22, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    let (st, n) = ctl::stem(&name("EC4"));
    let vol = rig.s.mount().unwrap();
    for side in [b"A", b"B"] {
        let _ = rig
            .s
            .delete(vol, FileName::new(Dir::Maps, &st[..n], side).unwrap());
    }
    rig.bar_to(1);
    rig.feed(Key::chord(ButtonId::Mix, ButtonId::Minus));
    rig.feed(Key::turn(EncoderId::A, -9));
    rig.tap(ButtonId::Edit);
    rig.work();
    let mut back = ControllerMap::EMPTY;
    let read = on_card(&mut rig.s, |s, r| ctl::load(s, r, &name("EC4"), &mut back));
    assert_eq!(read, Err(chimera_core::storage::LoadError::Missing));
    match rig.ui.step_toast(0) {
        chimera_core::ui::busy::ToastStep::Show(l) => assert_eq!(l.as_str(), "FILE NOT FOUND"),
        s => panic!("no toast: {s:?}"),
    }
}

/// More maps than row 0 can list: it says so under the pick.
#[test]
fn row_0_says_when_the_card_holds_more_maps() {
    let mut s = MemStore::new(1);
    for i in 0..=ctl::MAX_MAPS {
        put_map(&mut s, &format!("M{i:02}"), &[(21, CUTOFF)]);
    }
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.to(&["PART", "MIDI MAP"]);
    assert_eq!(
        rig.ui.map_band().text(0).tail.as_str(),
        "CARD HOLDS MORE: 16 SHOWN"
    );
}

/// The Part's engine changes: its layer is read again for the new
/// engine, that engine's rows in place of the old one's.
#[test]
fn an_engine_change_swaps_the_layer() {
    use chimera_core::dsp::algo::params::AlgoParams;
    use chimera_core::dsp::modal::ModalParams;
    let mut s = MemStore::new(1);
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let structure = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);
    put_map(&mut s, "EC4", &[(50, morph), (51, structure), (21, CUTOFF)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    let rows = |rig: &mut Rig| {
        rig.work();
        let b = rig.ui.map_band();
        let own = rig.ui.project().part(P1).sound.map.len();
        (1..b.rows - own - 1)
            .map(|i| b.text(i).label.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    rig.to(&["PART", "MIDI MAP"]);
    assert_eq!(rows(&mut rig), ["CC 50  MORPH", "CC 21  CUTOFF"]);
    screen::init_part_engine(rig.ui, P1, EngineType::Modal);
    assert_eq!(rows(&mut rig), ["CC 51  STRUCT", "CC 21  CUTOFF"]);
}

/// A map put or removed over USB (`map_put`) reaches the Parts playing
/// it: every layer reads the card again.
#[test]
fn a_map_put_over_usb_plays_at_once() {
    let _g = puts();
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.work();
    put_map(&mut rig.s, "EC4", &[(21, RESO)]);
    chimera_core::midi_map::map_put();
    rig.work();
    let mut now = 100;
    let cutoff = place(rig.ui, CUTOFF);
    hear(rig.ui, &mut now, cc(21, 0));
    assert!(
        place(rig.ui, RESO) < 0.01,
        "CC 21 is the put map's resonance"
    );
    assert_eq!(place(rig.ui, CUTOFF), cutoff);
}

/// A map removed over USB (`rm`, which bumps `map_put`) takes every Part
/// on it to NONE, as a delete on the unit does (#905, ADR 0255).
#[test]
fn a_map_removed_over_usb_leaves_its_parts_on_none() {
    let _g = puts();
    use chimera_hal::store::{Dir, FileName, Store};
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    put_map(&mut s, "LCXL3", &[(30, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    let p2 = PartId::ALL[1];
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.ui.project_mut().set_controller(p2, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.work();
    let (st, n) = ctl::stem(&name("EC4"));
    let vol = rig.s.mount().unwrap();
    for side in [b"A", b"B"] {
        let _ = rig
            .s
            .delete(vol, FileName::new(Dir::Maps, &st[..n], side).unwrap());
    }
    chimera_core::midi_map::map_put();
    for _ in 0..4 {
        rig.work();
    }
    assert_eq!(rig.ui.project().controller(P1), None);
    assert_eq!(rig.ui.project().controller(p2), None);
    assert_eq!(rig.ui.map_band().text(0).word.as_str(), "NONE");
}

/// Another card without the map is not a removal: the Part keeps its name
/// (ADR 0226) and plays nothing from it.
#[test]
fn another_card_without_the_map_keeps_the_name() {
    let _g = puts();
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.work();
    // The new card's first read comes with a map put over USB.
    rig.s.swap(2);
    put_map(&mut rig.s, "LCXL3", &[(30, RESO)]);
    chimera_core::midi_map::map_put();
    for _ in 0..4 {
        rig.work();
    }
    assert_eq!(rig.ui.project().controller(P1), Some(name("EC4")));
}

/// A name the card never held (a project's, ADR 0226) is no removal: read
/// again, after a map put over USB or an engine change, it stays.
#[test]
fn a_missing_name_read_again_stays() {
    let _g = puts();
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("GONE")));
    rig.to(&["PART", "MIDI MAP"]);
    for _ in 0..4 {
        rig.work();
    }
    put_map(&mut rig.s, "LCXL3", &[(30, RESO)]);
    chimera_core::midi_map::map_put();
    for _ in 0..4 {
        rig.work();
    }
    assert_eq!(rig.ui.project().controller(P1), Some(name("GONE")), "put");
    screen::init_part_engine(rig.ui, P1, EngineType::Modal);
    for _ in 0..4 {
        rig.work();
    }
    assert_eq!(
        rig.ui.project().controller(P1),
        Some(name("GONE")),
        "engine"
    );
}

/// A name the card doesn't hold (a project's, ADR 0226) stays shown, and
/// B down from it reaches NONE.
#[test]
fn b_down_from_a_missing_map_reaches_none() {
    let _g = puts();
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("GONE")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.work();
    assert_eq!(rig.ui.project().controller(P1), Some(name("GONE")));
    rig.feed(Key::turn(EncoderId::B, -1));
    assert_eq!(rig.ui.project().controller(P1), None);
}

/// The map read back from the card, as a Part on ALGO plays it.
fn card_rows(s: &mut MemStore, n: &str) -> Option<Vec<chimera_core::midi_map::Binding>> {
    let mut back = ControllerMap::EMPTY;
    on_card(s, |s, r| ctl::load(s, r, &name(n), &mut back)).ok()?;
    Some(back.layer(EngineType::Algo).0.entries().to_vec())
}

/// B to F edit a layer row as they edit a Sound's: the row changes in
/// the map on the card, and the Sound is not edited.
#[test]
fn b_to_f_edit_a_layer_row_in_the_map() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF), (22, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    let own = rig.ui.project().part(P1).sound.map;
    rig.bar_to(1);
    assert_eq!(
        rig.ui.legend_for_test().unwrap(),
        "A MOVE · MIX+MINUS DELETES"
    );
    rig.feed(Key::turn(EncoderId::B, 2));
    rig.feed(Key::turn(EncoderId::C, 10));
    rig.feed(Key::turn(EncoderId::F, 1));
    // Shown at once, before the card work.
    assert_eq!(rig.ui.map_band().text(1).label.as_str(), "CC 23  CUTOFF");
    rig.work();
    let rows = card_rows(&mut rig.s, "EC4").unwrap();
    let e = rows.iter().find(|b| b.target.addr() == CUTOFF).unwrap();
    assert_eq!(e.input, Input::cc(23).unwrap());
    assert_eq!(e.span.min, 10);
    assert_eq!(e.feedback, chimera_core::midi_map::Feedback::Value);
    assert_eq!(rows.len(), 2, "RESO's row stays");
    assert_eq!(rig.ui.project().part(P1).sound.map, own, "no Sound edit");
    let b = rig.ui.map_band();
    assert_eq!(b.text(1).label.as_str(), "CC 23  CUTOFF");
    assert_eq!(b.text(1).min.as_str(), "10");
    // The edited row plays.
    let mut now = 100;
    hear(rig.ui, &mut now, cc(23, 0));
    assert!(place(rig.ui, CUTOFF) < 0.05);
}

/// MIX+MINUS on row 0 asks, then deletes the Part's map from the card;
/// the Part plays NONE.
#[test]
fn mix_minus_on_row_0_deletes_the_map() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    put_map(&mut s, "LCXL3", &[(30, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.feed(Key::chord(ButtonId::Mix, ButtonId::Minus));
    let (q, r) = rig.ui.prompt_words_for_test().expect("a prompt");
    assert_eq!((q.as_str(), r.as_str()), ("DELETE MAP?", "EC4"));
    rig.feed(Key::turn(EncoderId::A, -9));
    rig.tap(ButtonId::Edit);
    rig.work();
    assert_eq!(card_rows(&mut rig.s, "EC4"), None);
    assert!(card_rows(&mut rig.s, "LCXL3").is_some());
    assert_eq!(rig.ui.project().controller(P1), None);
    rig.work();
    assert_eq!(rig.ui.map_band().text(0).word.as_str(), "NONE");
    // No map: MIX+MINUS asks nothing.
    rig.feed(Key::chord(ButtonId::Mix, ButtonId::Minus));
    assert!(rig.ui.prompt_words_for_test().is_none());
}

/// MIX+EDIT on row 0 renames the Part's map: NAMING starts from its name,
/// the file moves to the new name's, and every Part on it follows. A name
/// another map holds is refused, nothing moved.
#[test]
fn mix_edit_on_row_0_renames_the_map() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    put_map(&mut s, "EC", &[(30, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    let p2 = PartId::ALL[1];
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.ui.project_mut().set_controller(p2, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    let rename = |rig: &mut Rig| {
        rig.feed(Key::chord(ButtonId::Mix, ButtonId::Edit));
        assert_eq!(rig.ui.naming().unwrap().text(), "EC4");
        rig.feed(Key::turn(EncoderId::E, -1));
        rig.tap(ButtonId::Edit);
        rig.work();
    };
    // `EC` is taken: refused, nothing moved.
    rename(&mut rig);
    assert_eq!(rig.ui.project().controller(P1), Some(name("EC4")));
    assert!(card_rows(&mut rig.s, "EC4").is_some());
    let gone = name("EC");
    on_card(&mut rig.s, |s, r| {
        let (st, n) = ctl::stem(&gone);
        for side in [b"A", b"B"] {
            let f =
                chimera_hal::store::FileName::new(chimera_hal::store::Dir::Maps, &st[..n], side);
            let _ = s.delete(r.volume(), f.unwrap());
        }
    });
    rename(&mut rig);
    assert_eq!(card_rows(&mut rig.s, "EC4"), None);
    let rows = card_rows(&mut rig.s, "EC").unwrap();
    assert_eq!(rows[0].target.addr(), CUTOFF);
    assert_eq!(rig.ui.project().controller(P1), Some(name("EC")));
    assert_eq!(rig.ui.project().controller(p2), Some(name("EC")));
    assert!(
        !rig.ui.project().part(P1).sound.map.is_empty(),
        "the Part's rows stay its own"
    );
}

/// A map job confirmed while another waits for the card is refused on
/// screen (`CARD BUSY`), never dropped silently: the waiting edit still
/// reaches the card, and the map stays.
#[test]
fn a_job_while_another_waits_is_refused_not_lost() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.bar_to(1);
    // Keys alone, no card work between them: the edit waits.
    screen::feed(rig.ui, Key::turn(EncoderId::C, 5));
    screen::feed(rig.ui, Key::turn(EncoderId::A, -1));
    screen::feed(rig.ui, Key::chord(ButtonId::Mix, ButtonId::Minus));
    screen::feed(rig.ui, Key::turn(EncoderId::A, -9));
    screen::tap(rig.ui, ButtonId::Edit);
    match rig.ui.step_toast(0) {
        chimera_core::ui::busy::ToastStep::Show(l) => assert_eq!(l.as_str(), "CARD BUSY"),
        s => panic!("no toast: {s:?}"),
    }
    rig.work();
    let rows = card_rows(&mut rig.s, "EC4").expect("the map stays");
    assert_eq!(rows[0].span.min, 5, "the waiting edit reached the card");
    assert_eq!(rig.ui.project().controller(P1), Some(name("EC4")));
}

/// DELETE deletes the map it named, even if the Part picks another before
/// the card work runs.
#[test]
fn delete_takes_the_map_it_asked_about() {
    let mut s = MemStore::new(1);
    put_map(&mut s, "EC4", &[(21, CUTOFF)]);
    put_map(&mut s, "LCXL3", &[(30, RESO)]);
    let mut ui = ui();
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("EC4")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    // Keys alone: the Part picks LCXL3 before the delete runs.
    screen::feed(rig.ui, Key::chord(ButtonId::Mix, ButtonId::Minus));
    screen::feed(rig.ui, Key::turn(EncoderId::A, -9));
    screen::tap(rig.ui, ButtonId::Edit);
    screen::feed(rig.ui, Key::turn(EncoderId::B, 1));
    assert_eq!(rig.ui.project().controller(P1), Some(name("LCXL3")));
    rig.work();
    assert_eq!(card_rows(&mut rig.s, "EC4"), None);
    assert!(card_rows(&mut rig.s, "LCXL3").is_some());
    assert_eq!(rig.ui.project().controller(P1), Some(name("LCXL3")));
}
