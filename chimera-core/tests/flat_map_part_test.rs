// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A Part on a flat controller map (ADR 0323, #1103, #1099): up to 90
//! rows played through 16 seats, any map on any engine (rows the engine
//! lacks dimmed and dead), one shared copy a map, `+ NEW BINDING` into the
//! picked map, EDIT retargeting a map row, and MIX's fine step.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::midi_in::{Cc, CcEvent};
use chimera_core::midi_map::ctl::{self, ControllerMap, MAP_ROWS};
use chimera_core::midi_map::{Binding, Feedback, Input, Mappable, Span};
use chimera_core::name::SoundName;
use chimera_core::params::{EngineType, EnvParams, FilterParams};
use chimera_core::project::PartId;
use chimera_core::storage::{Card, Ready};
use chimera_core::ui::UiState;
use chimera_core::ui::settings::view::RowLook;
use chimera_hal::store::StoreError;
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId, Ms};
use screen::{Input as Key, Rig};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn name(s: &str) -> SoundName {
    SoundName::new(s).unwrap()
}

fn on_card<R>(s: &mut MemStore, f: impl FnOnce(&mut MemStore, &Ready) -> R) -> R {
    let mut card = Card::new();
    let out = card.run(s, |s, r| Ok::<_, StoreError>(f(s, r))).unwrap();
    out.result.unwrap()
}

fn row(cc: u8, a: ParamAddr) -> Binding {
    Binding {
        input: Input::cc(cc).unwrap(),
        target: Mappable::new(a).unwrap(),
        span: Span::FULL,
        feedback: Feedback::Off,
    }
}

fn save(s: &mut MemStore, n: &str, m: &ControllerMap) {
    on_card(s, |s, r| {
        ctl::save(s, r, &name(n), &mut ctl::FileBuf::new(), m)
    })
    .unwrap();
}

fn load(s: &mut MemStore, n: &str) -> ControllerMap {
    let mut back = ControllerMap::EMPTY;
    on_card(s, |s, r| {
        ctl::load(s, r, &name(n), &mut ctl::FileBuf::new(), &mut back)
    })
    .unwrap();
    back
}

/// The CCs no INIT Sound row is on (`midi_map::default_for`), nor the
/// built-in layer's.
fn free_ccs() -> impl Iterator<Item = u8> {
    (0..=127u8).filter(|c| {
        Input::cc(*c).is_some() && !matches!(c, 7 | 10 | 11 | 16..=19 | 71..=74 | 91 | 93 | 94)
    })
}

/// Every cell a VA Part reads in its own Sound, in table order.
fn va_cells() -> Vec<ParamAddr> {
    BlockRef::ALL
        .into_iter()
        .filter(|b| b.in_sound())
        .flat_map(|b| (0..64).map(move |p| ParamAddr::new(b, ParamId(p))))
        .filter(|&a| Mappable::new(a).is_some_and(|m| m.live(EngineType::Va)))
        .collect()
}

/// A 90-row map of `va_cells` on `free_ccs`.
fn ninety() -> ControllerMap {
    let mut m = ControllerMap::EMPTY;
    for (a, c) in va_cells().into_iter().zip(free_ccs()).take(MAP_ROWS) {
        m.push(row(c, a)).unwrap();
    }
    assert_eq!(m.len(), MAP_ROWS);
    m
}

fn ui(e: EngineType) -> Box<UiState> {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part_engine(&mut ui, P1, e);
    screen::init_part_engine(&mut ui, P2, e);
    ui
}

fn cc_on(ui: &UiState, p: PartId, n: u8, v: u8) -> CcEvent {
    CcEvent {
        channel: ui.project().part(p).mix.channel,
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

fn place(ui: &UiState, p: PartId, a: ParamAddr) -> f32 {
    let v = ui
        .project()
        .read_part(p)
        .block(a.block)
        .unwrap()
        .get(a.param);
    a.spec().unwrap().normalize(v)
}

/// Ninety rows: the first, a middle and the last each move their cell,
/// past the 16 seats.
#[test]
fn ninety_rows_each_move_their_cell() {
    let mut s = MemStore::new(1);
    let m = ninety();
    save(&mut s, "BIG", &m);
    let mut ui = ui(EngineType::Va);
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("BIG")));
    rig.work();
    let mut now = 100;
    for i in [0, 44, 89, 1, 60] {
        let b = m.get(i).unwrap();
        let Input::Cc(c, _) = b.input else { panic!() };
        let a = b.target.addr();
        let ev = cc_on(rig.ui, P1, c.get(), 127);
        hear(rig.ui, &mut now, ev);
        let hi = place(rig.ui, P1, a);
        let ev = cc_on(rig.ui, P1, c.get(), 0);
        hear(rig.ui, &mut now, ev);
        let lo = place(rig.ui, P1, a);
        assert!(hi > lo, "row {i} ({a:?}): {lo} .. {hi}");
    }
}

/// The ALGO cells of a map do nothing on a VA Part and dim on MIDI MAP;
/// its shared rows play.
#[test]
fn rows_the_engine_lacks_dim_and_do_nothing() {
    let mut s = MemStore::new(1);
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let attack = ParamAddr::new(
        BlockRef::Env(chimera_core::dsp::modulator::EnvSlot::Env1),
        EnvParams::ATTACK,
    );
    let mut m = ControllerMap::EMPTY;
    m.push(row(20, morph)).unwrap();
    m.push(row(21, attack)).unwrap();
    save(&mut s, "ALGO", &m);
    let mut ui = ui(EngineType::Va);
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("ALGO")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    let b = rig.ui.map_band();
    assert_eq!(b.look(1), RowLook::Dimmed, "MORPH on VA");
    assert_eq!(b.look(2), RowLook::Normal, "ENV 1 ATTACK");
    let algo = rig.ui.project().part(P1).sound.params.algo;
    let mut now = 100;
    let ev = cc_on(rig.ui, P1, 20, 0);
    hear(rig.ui, &mut now, ev);
    assert!(rig.ui.project().part(P1).sound.params.algo == algo);
    let was = place(rig.ui, P1, attack);
    let ev = cc_on(rig.ui, P1, 21, if was > 0.5 { 0 } else { 127 });
    hear(rig.ui, &mut now, ev);
    assert_ne!(place(rig.ui, P1, attack), was);
}

/// Two Parts on one map share it: a row edited on Part 1's MIDI MAP plays
/// so on Part 2 at once, and reaches the card.
#[test]
fn two_parts_on_one_map_share_an_edit() {
    let mut s = MemStore::new(1);
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut m = ControllerMap::EMPTY;
    m.push(row(21, cutoff)).unwrap();
    save(&mut s, "JOE", &m);
    let mut ui = ui(EngineType::Va);
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("JOE")));
    rig.ui.project_mut().set_controller(P2, Some(name("JOE")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.work();
    rig.bar_to(1);
    // D: MAX all the way down.
    for _ in 0..30 {
        rig.feed(Key::turn(EncoderId::D, -10));
    }
    rig.work();
    assert_eq!(load(&mut rig.s, "JOE").get(0).unwrap().span.max, 0);
    let mut now = 100;
    let ev = cc_on(rig.ui, P2, 21, 127);
    hear(rig.ui, &mut now, ev);
    assert!(place(rig.ui, P2, cutoff) < 0.01, "Part 2 plays the edit");
}

/// MIX held while turning C or D moves MIN or MAX one place a frame,
/// however fast the knob.
#[test]
fn mix_held_is_a_fine_step() {
    let mut s = MemStore::new(1);
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut m = ControllerMap::EMPTY;
    m.push(row(21, cutoff)).unwrap();
    save(&mut s, "FINE", &m);
    let mut ui = ui(EngineType::Va);
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("FINE")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.bar_to(1);
    rig.feed(Key::turn(EncoderId::C, 10).and_held(ButtonId::Mix));
    assert_eq!(rig.ui.map_band().text(1).min.as_str(), "1");
    // MIX held: the row's hint says what MIX does there.
    assert_eq!(
        rig.ui.map_band().text(1).hint(),
        chimera_core::ui::settings::midi_map::MIX_KEYS
    );
    rig.feed(Key::turn(EncoderId::C, 10));
    assert_eq!(rig.ui.map_band().text(1).min.as_str(), "11");
}

/// `+ NEW BINDING` with a map picked adds to the map, not the Sound.
#[test]
fn new_binding_joins_the_picked_map() {
    let mut s = MemStore::new(1);
    let mut m = ControllerMap::EMPTY;
    m.push(row(
        21,
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
    ))
    .unwrap();
    save(&mut s, "NEW", &m);
    let mut ui = ui(EngineType::Algo);
    let own = ui.project().part(P1).sound.map;
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("NEW")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    let last = rig.ui.map_band().rows - 1;
    rig.bar_to(last);
    rig.tap(ButtonId::Edit);
    // Pointing on the Part's pages: a turn points at the focused cell.
    rig.feed(Key::turn(EncoderId::A, 1));
    rig.tap(ButtonId::Edit);
    rig.work();
    assert_eq!(rig.ui.project().part(P1).sound.map, own, "the Sound kept");
    assert_eq!(load(&mut rig.s, "NEW").len(), 2, "the map gained the row");
}

/// No card, a map edited: the edit waits, and no pass mounts the card for
/// it alone; with the card back, the next card work writes it (review of
/// #1122).
#[test]
fn no_card_holds_an_edit_without_mounting() {
    let mut s = MemStore::new(1);
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut m = ControllerMap::EMPTY;
    m.push(row(21, cutoff)).unwrap();
    save(&mut s, "JOE", &m);
    let mut ui = ui(EngineType::Va);
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("JOE")));
    rig.to(&["PART", "MIDI MAP"]);
    rig.work();
    rig.work();
    rig.bar_to(1);
    rig.s.eject();
    rig.feed(Key::turn(EncoderId::C, 5));
    rig.work();
    let was = rig.s.mounts();
    for _ in 0..5 {
        rig.work();
    }
    assert_eq!(rig.s.mounts(), was, "no mount for the edit alone");
    rig.s.insert();
    chimera_core::midi_map::map_put();
    rig.work();
    assert_eq!(load(&mut rig.s, "JOE").get(0).unwrap().span.min, 5);
}

/// Two rows of a long map on one control, one seated (heard least
/// recently) and one not: the message seats the other without taking the
/// first's seat, and moves both (review of #1122).
#[test]
fn two_rows_on_one_control_both_take_seats() {
    let mut s = MemStore::new(1);
    let cells = va_cells();
    let mut ccs = free_ccs();
    let shared = ccs.next().unwrap();
    let (a, b) = (cells[0], cells[30]);
    let mut m = ControllerMap::EMPTY;
    // Row 0, seated first by the map's pick: CC `shared` on `a`.
    m.push(row(shared, a)).unwrap();
    for &c in cells.iter().skip(1).take(20) {
        m.push(row(ccs.next().unwrap(), c)).unwrap();
    }
    // Past the 16 seats: CC `shared` on `b`.
    m.push(row(shared, b)).unwrap();
    save(&mut s, "TWO", &m);
    let mut ui = ui(EngineType::Va);
    let mut rig = Rig::new(&mut ui, s);
    rig.ui.project_mut().set_controller(P1, Some(name("TWO")));
    rig.work();
    // Every other seat heard since: row 0's is the least recent.
    let mut now = 100;
    for i in 1..16 {
        let Input::Cc(c, _) = m.get(i).unwrap().input else {
            panic!()
        };
        let ev = cc_on(rig.ui, P1, c.get(), 64);
        hear(rig.ui, &mut now, ev);
    }
    // One message: both move.
    let was = (place(rig.ui, P1, a), place(rig.ui, P1, b));
    let v = if was.0 < 0.5 && was.1 < 0.5 { 127 } else { 0 };
    let ev = cc_on(rig.ui, P1, shared, v);
    hear(rig.ui, &mut now, ev);
    assert_ne!(place(rig.ui, P1, a), was.0, "{a:?} kept its seat");
    assert_ne!(place(rig.ui, P1, b), was.1, "{b:?} took one");
}
