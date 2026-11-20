//! KEEP MAP (#517, ADR 0253): MIX + PLUS on MIDI MAP's row 0 keeps the
//! active Part's own rows through Sound loads, so a controller layout
//! survives preset browsing. A kept map is no edit: the Part reads Clean
//! and the next load asks nothing.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::midi_in::CcEvent;
use chimera_core::midi_map::{Input, Mappable, MidiMap};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::project::{PartFrom, PartId, PartSource, PartStatus, SlotId, sound_status};
use chimera_core::ui::UiState;
use chimera_core::ui::hint::{self, Gesture};
use chimera_core::ui::theme;
use chimera_hal::ButtonId::{B1, Edit, Mix, Plus};
use chimera_hal::testkit::MemStore;
use chimera_hal::{EncoderId, Ms};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use screen::{Fb, Input as Key, Rig, feed, render_ui};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const MINE: SlotId = SlotId::ALL[100];
const VA: SlotId = SlotId::ALL[101];
const MODAL: SlotId = SlotId::ALL[102];

/// CC 21 on the cutoff, alone.
fn rows() -> MidiMap {
    let mut m = MidiMap::EMPTY;
    m.learn(Input::cc(21).unwrap(), Mappable::new(CUTOFF).unwrap())
        .unwrap();
    m
}

/// Part 1 on a Sound holding `rows`; INIT VA and MODAL (their own default
/// rows) in two more slots. One MIDI pass run.
fn ui() -> Box<UiState> {
    let mut ui = Box::new(screen::stack_ui());
    let mut mine = Sound::init(EngineType::Algo);
    mine.map = rows();
    let p = ui.project_mut();
    p.pool_store(MINE, &mine).unwrap();
    p.pool_store(VA, &Sound::init(EngineType::Va)).unwrap();
    p.pool_store(MODAL, &Sound::init(EngineType::Modal))
        .unwrap();
    load(&mut ui, MINE);
    ui
}

/// Slot `s` into Part 1 through the guard, then the MIDI pass after it.
fn load(ui: &mut UiState, s: SlotId) {
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Slot(s),
    });
    pass(ui);
}

/// One pass of the MIDI UI, nothing heard: the shells run one a loop.
fn pass(ui: &mut UiState) {
    ui.hear_ccs(core::iter::empty::<CcEvent>(), Ms(1000), true);
}

/// SETTINGS › PART › MIDI MAP, the bar on row 0.
fn to_map(ui: &mut UiState) {
    feed(ui, Key::press(B1));
    screen::to_leaf(ui, &["PART"]);
    for _ in 1..chimera_core::ui::settings::tree::PART.len() {
        feed(ui, Key::press(Plus));
    }
    feed(ui, Key::press(Edit));
    assert_eq!(ui.location().settings().unwrap().row(), 0);
}

/// MIX + PLUS on the page.
fn keep(ui: &mut UiState) {
    feed(ui, Key::chord(Mix, Plus));
    pass(ui);
}

fn map(ui: &UiState) -> MidiMap {
    ui.project().part(P1).sound.map
}

fn status(ui: &UiState) -> PartStatus {
    sound_status(ui.project().part(P1), ui.project().pool())
}

/// Row 0's KEEP MAP word.
fn kept_word(ui: &UiState) -> String {
    let t = ui.map_band().text(0);
    let v = t.values();
    assert_eq!(v[0].0, "MIX + PLUS KEEP MAP");
    v[0].1.into()
}

#[test]
fn off_a_load_takes_the_sound_s_rows() {
    let mut ui = ui();
    assert_eq!(map(&ui), rows());
    load(&mut ui, VA);
    assert_eq!(map(&ui), Sound::init(EngineType::Va).map);
}

#[test]
fn kept_rows_survive_loads_and_ask_nothing() {
    let mut ui = ui();
    to_map(&mut ui);
    assert_eq!(kept_word(&ui), "OFF");
    keep(&mut ui);
    assert_eq!(kept_word(&ui), "ON");
    for s in [VA, MODAL, MINE, VA] {
        load(&mut ui, s);
        assert!(ui.prompt_words_for_test().is_none(), "{s:?} asked");
        assert_eq!(map(&ui), rows(), "{s:?}");
        assert_eq!(status(&ui), PartStatus::Clean, "{s:?}");
    }
    // INIT too.
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Modal),
    });
    pass(&mut ui);
    assert!(ui.prompt_words_for_test().is_none());
    assert_eq!(map(&ui), rows());
    assert_eq!(status(&ui), PartStatus::Clean);
}

/// Only the kept rows are forgiven: a real edit still asks.
#[test]
fn an_edit_still_asks() {
    let mut ui = ui();
    to_map(&mut ui);
    keep(&mut ui);
    load(&mut ui, VA);
    ui.project_mut().edit_part(P1).sound.params.filter.cutoff *= 0.5;
    pass(&mut ui);
    assert_eq!(status(&ui), PartStatus::Edited);
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Slot(MODAL),
    });
    assert!(ui.prompt_words_for_test().is_some());
}

/// A load that lands edited (a program change's recall copy, ADR 0224)
/// keeps its `*` and its prompt: only a Clean landing is marked.
#[test]
fn an_edited_landing_stays_edited() {
    let mut ui = ui();
    to_map(&mut ui);
    keep(&mut ui);
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Slot(VA),
    });
    // The recall copy's edit, landed with the load, before the pass.
    ui.project_mut().edit_part(P1).sound.params.filter.cutoff *= 0.5;
    pass(&mut ui);
    assert_eq!(map(&ui), rows());
    assert_eq!(status(&ui), PartStatus::Edited);
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Slot(MODAL),
    });
    assert!(ui.prompt_words_for_test().is_some());
}

/// A kept Part whose slot moved on reads Stale, as any other.
#[test]
fn a_kept_part_still_reads_stale() {
    let mut ui = ui();
    to_map(&mut ui);
    keep(&mut ui);
    load(&mut ui, VA);
    assert_eq!(status(&ui), PartStatus::Clean);
    ui.project_mut()
        .pool_store(VA, &Sound::init(EngineType::Algo))
        .unwrap();
    pass(&mut ui);
    assert_eq!(status(&ui), PartStatus::Stale(VA));
}

/// MIX + PLUS again lets go: the Part holds rows its Sound doesn't, so it
/// reads Edited, and the next load takes the Sound's.
#[test]
fn off_again_the_part_is_edited() {
    let mut ui = ui();
    to_map(&mut ui);
    keep(&mut ui);
    load(&mut ui, VA);
    to_map(&mut ui);
    keep(&mut ui);
    assert_eq!(kept_word(&ui), "OFF");
    assert_eq!(status(&ui), PartStatus::Edited);
    assert_eq!(map(&ui), rows());
}

/// Rows learned while kept are an edit until the next load, which asks
/// once; REPLACE keeps them.
#[test]
fn rows_learned_while_kept_ask_once() {
    let mut ui = ui();
    to_map(&mut ui);
    keep(&mut ui);
    load(&mut ui, VA);
    let mut m = rows();
    m.learn(
        Input::cc(22).unwrap(),
        Mappable::new(ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE)).unwrap(),
    )
    .unwrap();
    ui.project_mut().edit_part(P1).sound.map = m;
    pass(&mut ui);
    assert_eq!(status(&ui), PartStatus::Edited);
    assert!(ui.ask_replace_for_test(PartSource {
        part: P1,
        from: PartFrom::Slot(MODAL),
    }));
    // REPLACE.
    feed(&mut ui, Key::turn(EncoderId::A, -9));
    feed(&mut ui, Key::turn(EncoderId::A, 1));
    screen::tap(&mut ui, Edit);
    assert!(!ui.prompt_open());
    pass(&mut ui);
    assert_eq!(map(&ui), m);
    assert_eq!(status(&ui), PartStatus::Clean);
}

/// A project load forgets every keep.
#[test]
fn a_project_load_forgets() {
    let mut s = MemStore::new(1);
    let f = screen::put(&mut s, "TWO");
    // A pristine project, so LOAD asks nothing.
    let mut ui = Box::new(screen::stack_ui());
    let mut rig = Rig::new(&mut ui, s);
    rig.to(&["PART", "MIDI MAP"]);
    keep(rig.ui);
    assert_eq!(kept_word(rig.ui), "ON");
    rig.load(f);
    pass(rig.ui);
    rig.to(&["PART", "MIDI MAP"]);
    assert_eq!(kept_word(rig.ui), "OFF");
}

/// Row 0's focus band names MIX + PLUS and the state (ADR 0086).
#[test]
fn row_0_hints_mix_plus() {
    let mut ui = ui();
    to_map(&mut ui);
    let on = |ui: &UiState, word| {
        let shot = render_ui(ui);
        let h = [Gesture("MIX + PLUS KEEP MAP", word)];
        (0..320).any(|y| drawn_at(&shot, &h, theme::MARGIN_X, y))
    };
    assert!(on(&ui, "OFF"));
    keep(&mut ui);
    assert!(on(&ui, "ON"));
}

/// Whether `h` is drawn with its left edge at `x`, baseline `y`.
fn drawn_at(shot: &Fb, h: &[Gesture<'_>], x: i32, y: i32) -> bool {
    let mut want = Fb::new();
    hint::draw(&mut want, h, x, y);
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

/// KEEP MAP and a program change (ADR 0224): to another Sound and back to
/// the edited one, the Part brings its edits back and still reads Edited,
/// its kept rows in place.
#[test]
fn a_program_change_back_to_an_edit_stays_edited() {
    use chimera_core::midi_in::{CtrlEvent, Port};
    use chimera_core::ui::midi_settings::MidiSettings;
    use chimera_midi::ctl::Ctl;
    use chimera_midi::value::U7;
    let mut ui = ui();
    to_map(&mut ui);
    keep(&mut ui);
    ui.set_midi(MidiSettings::DEFAULT.with_program_in(true));
    let ch = ui.project().part(P1).mix.channel;
    let pc = |ui: &mut UiState, s: SlotId| {
        let e = CtrlEvent::Ctl(
            ch,
            Ctl::Program {
                bank: None,
                program: U7::new(s.index() as u8).unwrap(),
            },
        );
        ui.hear_ccs([(Port::Din, e)].into_iter(), Ms(1000), true);
        pass(ui);
    };
    pc(&mut ui, VA);
    assert_eq!(status(&ui), PartStatus::Clean);
    ui.project_mut().edit_part(P1).sound.params.filter.cutoff *= 0.5;
    let edited = ui.project().part(P1).sound.params.filter.cutoff;
    pass(&mut ui);
    assert_eq!(status(&ui), PartStatus::Edited);
    pc(&mut ui, MODAL);
    assert_eq!(status(&ui), PartStatus::Clean, "MODAL, kept rows: Clean");
    assert_eq!(map(&ui), rows());
    pc(&mut ui, VA);
    assert_eq!(ui.project().part(P1).sound.params.filter.cutoff, edited);
    assert_eq!(status(&ui), PartStatus::Edited);
    assert_eq!(map(&ui), rows());
}
