//! TUNING picks a Scala file by name, MAPPING lays it on the keys (#907,
//! ADR 0259). `TUNINGS` is global: every test here holds `LOCK`.

#![cfg(feature = "tuning")]

mod screen;

use std::sync::Mutex;

use chimera_core::part::{PartParams, TUNING_MTS, TUNING_SCALA};
use chimera_core::project::{PartId, Project};
use chimera_core::scala_name::name_id;
use chimera_core::tuning::{ScalaName, TUNINGS};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::tuning_walk::{MARQUEE_HOLD, MARQUEE_STEP, SHOWN, marquee};
use chimera_hal::EncoderId;
use chimera_hal::store::{Dir, FileName, Store};
use chimera_hal::testkit::MemStore;
use screen::{Input, Rig};

static LOCK: Mutex<()> = Mutex::new(());

const MAJOR: &[u8] = b"! major.scl\nJust major, 7 notes\n7\n9/8\n5/4\n4/3\n3/2\n5/3\n15/8\n2/1\n";
const QUARTER: &[u8] = b"quarter\n2\n50.\n2/1\n";

fn file(stem: &[u8], ext: &[u8]) -> FileName {
    FileName::new(Dir::Tunings, stem, ext).unwrap()
}

/// File `s` as the walk picks it: its stem and its shown name's id.
fn name(s: &str) -> Option<ScalaName> {
    ScalaName::new(s.as_bytes()).map(|n| n.with_id(name_id(s.as_bytes())))
}

/// A file copied as `long`, kept under short name `stem`.
fn long(s: &mut MemStore, stem: &[u8], ext: &[u8], long: &str, text: &[u8]) {
    s.plant_long(file(stem, ext), long, text);
}

/// What the walk picks for a file kept as `stem`, showing `shows`.
fn picked(stem: &str, shows: &str) -> Option<ScalaName> {
    ScalaName::new(stem.as_bytes()).map(|n| n.with_id(name_id(shows.as_bytes())))
}

/// A UI on SETTINGS › AUDIO ROUTING › TUNING with `files` on the card.
fn on_page<'u>(ui: &'u mut UiState, files: &[(&[u8], &[u8])]) -> Rig<'u> {
    let mut s = MemStore::new(1);
    let vol = s.mount().unwrap();
    let _ = s.make_dir(vol, Dir::Chimera);
    s.make_dir(vol, Dir::Tunings).unwrap();
    for (stem, text) in files {
        s.plant(file(stem, b"SCL"), text);
    }
    let mut r = Rig::new(ui, s);
    r.to(&["AUDIO ROUTING", "TUNING"]);
    r
}

fn now(r: &Rig) -> (u8, Option<ScalaName>) {
    let p = r.ui.project().part(PartId::ALL[0]);
    (p.mix.tuning, p.scala())
}

fn turn(r: &mut Rig, e: EncoderId, d: i8) {
    r.feed(Input::turn(e, d));
    // The table lands on the pass after the turn.
    r.work();
}

fn toast(r: &mut Rig) -> Option<String> {
    match r.ui.step_toast(0) {
        ToastStep::Show(l) => Some(l.as_str().into()),
        _ => None,
    }
}

#[test]
fn past_mts_the_turn_walks_the_files_by_name_and_back() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[(b"QUARTER", QUARTER), (b"MAJOR", MAJOR)]);
    turn(&mut r, EncoderId::B, 5);
    assert_eq!(now(&r), (TUNING_MTS, None));
    turn(&mut r, EncoderId::B, 1);
    assert_eq!(now(&r), (TUNING_SCALA, name("MAJOR")), "the first by name");
    turn(&mut r, EncoderId::B, 1);
    assert_eq!(now(&r), (TUNING_SCALA, name("QUARTER")));
    assert_eq!(TUNINGS.offset(0, 61), -0.5, "QUARTER plays");
    turn(&mut r, EncoderId::B, 3);
    assert_eq!(
        now(&r),
        (TUNING_SCALA, name("QUARTER")),
        "stops at the last"
    );
    turn(&mut r, EncoderId::B, -2);
    assert_eq!(now(&r), (TUNING_MTS, None));
    turn(&mut r, EncoderId::B, -9);
    assert_eq!(now(&r), (0, None));
    assert_eq!(TUNINGS.offset(0, 61), 0.0);
}

#[test]
fn with_no_files_the_turn_stops_at_mts() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    turn(&mut r, EncoderId::B, 9);
    assert_eq!(now(&r), (TUNING_MTS, None));
}

#[test]
fn keyboard_starts_each_c_and_silences_the_spare_keys() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[(b"MAJOR", MAJOR)]);
    turn(&mut r, EncoderId::B, 6);
    assert_eq!(now(&r), (TUNING_SCALA, name("MAJOR")));
    // CONTINUOUS: key 67 is degree 7, an octave up.
    assert!((TUNINGS.offset(0, 67) - 5.0).abs() < 1e-4);
    assert!(!TUNINGS.silent(0, 67));
    turn(&mut r, EncoderId::C, 1);
    assert_eq!(r.ui.project().part(PartId::ALL[0]).mix.mapping, 1);
    // KEYBOARD: 67 (G) is spare, silent; 72 is the next C.
    assert!(TUNINGS.silent(0, 67));
    assert!(TUNINGS.offset(0, 72).abs() < 1e-4);
    assert!(TUNINGS.silent(0, 55) && !TUNINGS.silent(0, 48));
    // A .KBM of the same name wins: nothing silent.
    r.s.plant(
        file(b"MAJOR", b"KBM"),
        b"0\n0\n127\n60\n60\n261.6255653\n7\n",
    );
    chimera_core::tuning::file_put();
    r.work();
    assert!(!TUNINGS.silent(0, 67));
    turn(&mut r, EncoderId::B, -9);
    assert!(!TUNINGS.silent(0, 67));
}

#[test]
fn a_missing_file_plays_twelve_tet_and_says_so() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    r.ui.project_mut().set_scala(PartId::ALL[0], name("PELOG"));
    r.ui.project_mut().edit_part(PartId::ALL[0]).mix.tuning = TUNING_SCALA;
    r.work();
    assert_eq!(toast(&mut r).as_deref(), Some("PELOG.SCL NOT FOUND"));
    assert!((0..128).all(|k| TUNINGS.offset(0, k) == 0.0));
    r.ui.project_mut().edit_part(PartId::ALL[0]).mix.tuning = 0;
    r.work();
}

#[test]
fn the_project_keeps_the_name_not_a_place() {
    let (mut p, _) = Project::boxed();
    p.set_scala(PartId::ALL[2], name("PELOG"));
    p.edit_part(PartId::ALL[2]).mix.tuning = TUNING_SCALA;
    p.edit_part(PartId::ALL[2]).mix.mapping = 1;
    let mut s = MemStore::new(1);
    let f = screen::put_with(&mut s, "TUNED", |q| {
        q.set_scala(PartId::ALL[2], name("PELOG"));
        q.edit_part(PartId::ALL[2]).mix.tuning = TUNING_SCALA;
        q.edit_part(PartId::ALL[2]).mix.mapping = 1;
    });
    let mut ui = UiState::new();
    let mut r = Rig::new(&mut ui, s);
    r.load(f);
    let part = r.ui.project().part(PartId::ALL[2]);
    assert_eq!(part.scala(), name("PELOG"));
    assert_eq!((part.mix.tuning, part.mix.mapping), (TUNING_SCALA, 1));
    assert_eq!(r.ui.project().part(PartId::ALL[1]).scala(), None);
}

#[test]
fn mapping_is_saved_with_the_part() {
    assert_eq!(PartParams::MAPPING.0, 12);
    assert_eq!(ScalaName::new(b"pelog"), None, "upper case only");
    assert_eq!(ScalaName::new(b"TOOLONGNAME"), None);
    assert_eq!(ScalaName::new(b""), None);
}

/// The page with a file picked: TUNING names it, the band describes it;
/// MAPPING beside it. `SCREEN_DUMP` keeps the guide's picture.
#[test]
fn the_page_names_the_file_and_describes_it() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[(b"MAJOR", MAJOR)]);
    let before = screen::render_ui(r.ui).hash();
    turn(&mut r, EncoderId::B, 6);
    r.ui.update(chimera_core::ui::animation::UiTick::for_test());
    let named = screen::render_ui(r.ui);
    named.dump("tuning");
    assert_ne!(named.hash(), before);
    turn(&mut r, EncoderId::C, 1);
    r.ui.update(chimera_core::ui::animation::UiTick::for_test());
    screen::render_ui(r.ui).dump("tuning-mapping");
    turn(&mut r, EncoderId::B, -9);
    turn(&mut r, EncoderId::C, -1);
}

/// A file copied with a long name walks by it, plays, and the page holds
/// the name it shows (#907 owner decision, ADR 0259).
#[test]
fn long_names_walk_in_their_order_and_play() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[(b"MAJOR", MAJOR)]);
    long(&mut r.s, b"ZEBRA~1", b"SCL", "bohlen-pierce.scl", QUARTER);
    turn(&mut r, EncoderId::B, 6);
    let bp = picked("ZEBRA~1", "BOHLEN-PIERCE");
    assert_eq!(now(&r), (TUNING_SCALA, bp), "BOHLEN-PIERCE before MAJOR");
    assert_eq!(TUNINGS.offset(0, 61), -0.5);
    let mut shown = [0; 32];
    let n = SHOWN.name(bp.unwrap(), &mut shown);
    assert_eq!(&shown[..n], b"BOHLEN-PIERCE");
    turn(&mut r, EncoderId::B, 1);
    assert_eq!(now(&r), (TUNING_SCALA, name("MAJOR")));
    turn(&mut r, EncoderId::B, -1);
    assert_eq!(now(&r), (TUNING_SCALA, bp));
    turn(&mut r, EncoderId::B, -9);
}

/// A copy that gave the file another short name, and its map under the
/// same long name: still found, the map still pairs.
#[test]
fn a_file_copied_again_is_found_by_its_long_name() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    long(&mut r.s, b"BOHLEN~2", b"SCL", "bohlen-pierce.scl", MAJOR);
    long(
        &mut r.s,
        b"BOHLEN~1",
        b"SCL",
        "bohlen-pierce-old.scl",
        QUARTER,
    );
    let p = PartId::ALL[0];
    r.ui.project_mut()
        .set_scala(p, picked("BOHLEN~1", "BOHLEN-PIERCE"));
    r.ui.project_mut().edit_part(p).mix.tuning = TUNING_SCALA;
    r.ui.project_mut().edit_part(p).mix.mapping = 1;
    r.work();
    assert!(
        (TUNINGS.offset(0, 61) - 1.0391).abs() < 1e-3,
        "MAJOR plays (9/8 on C#)"
    );
    assert!(TUNINGS.silent(0, 67), "KEYBOARD, no map yet");
    long(
        &mut r.s,
        b"BOHLEN~3",
        b"KBM",
        "bohlen-pierce.kbm",
        b"0\n0\n127\n60\n60\n261.6255653\n7\n",
    );
    chimera_core::tuning::file_put();
    r.work();
    assert!(!TUNINGS.silent(0, 67), "the map of the same long name wins");
    r.ui.project_mut().edit_part(p).mix.tuning = 0;
    r.work();
}

/// A project from before long names names its file by stem alone.
#[test]
fn a_stem_alone_still_names_its_file() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[(b"QUARTER", QUARTER)]);
    let p = PartId::ALL[0];
    r.ui.project_mut().set_scala(p, ScalaName::new(b"QUARTER"));
    r.ui.project_mut().edit_part(p).mix.tuning = TUNING_SCALA;
    r.work();
    assert_eq!(TUNINGS.offset(0, 61), -0.5);
    r.ui.project_mut().edit_part(p).mix.tuning = 0;
    r.work();
}

#[test]
fn a_marquee_holds_scrolls_holds_and_loops() {
    let (h, s) = (MARQUEE_HOLD, MARQUEE_STEP);
    assert_eq!(marquee(8, 8, 100), 0, "a name that fits stays");
    assert_eq!(marquee(13, 8, 0), 0);
    assert_eq!(marquee(13, 8, h - 1), 0);
    assert_eq!(marquee(13, 8, h + s), 1);
    assert_eq!(marquee(13, 8, h + 5 * s), 5, "the end");
    assert_eq!(marquee(13, 8, 2 * h + 5 * s - 1), 5, "held there");
    assert_eq!(marquee(13, 8, 2 * h + 5 * s), 0, "and again");
}

/// The TUNING cell shows a long name's first 8 characters and scrolls it
/// while it has focus; `SCREEN_DUMP` keeps the guide's picture.
#[test]
fn the_cell_scrolls_a_long_name_while_focused() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    long(&mut r.s, b"BOHLEN~1", b"SCL", "bohlen-pierce.scl", MAJOR);
    turn(&mut r, EncoderId::B, 6);
    let tick = |r: &mut Rig| r.ui.update(chimera_core::ui::animation::UiTick::for_test());
    tick(&mut r);
    let start = screen::render_ui(r.ui);
    start.dump("tuning-long");
    for _ in 0..MARQUEE_HOLD + 2 * MARQUEE_STEP {
        tick(&mut r);
    }
    assert_ne!(screen::render_ui(r.ui).hash(), start.hash(), "it scrolls");
    // Focus on MAPPING: the name holds still.
    turn(&mut r, EncoderId::C, 1);
    turn(&mut r, EncoderId::C, -1);
    tick(&mut r);
    let still = screen::render_ui(r.ui).hash();
    for _ in 0..MARQUEE_HOLD + 2 * MARQUEE_STEP {
        tick(&mut r);
    }
    assert_eq!(screen::render_ui(r.ui).hash(), still, "not focused: still");
    turn(&mut r, EncoderId::B, -9);
}

/// Two long names alike in their first 32 characters show one name and
/// share an id: each is still picked, played, saved and walked past by
/// its stem (ADR 0259).
#[test]
fn two_names_alike_for_32_characters_stay_two_files() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let a = "gamelan slendro ten-tone version a.scl";
    let b = "gamelan slendro ten-tone version b.scl";
    let shows = "GAMELAN SLENDRO TEN-TONE VERSION";
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    long(&mut r.s, b"GAMELA~1", b"SCL", a, MAJOR);
    long(&mut r.s, b"GAMELA~2", b"SCL", b, QUARTER);
    let (one, two) = (picked("GAMELA~1", shows), picked("GAMELA~2", shows));
    turn(&mut r, EncoderId::B, 6);
    assert_eq!(now(&r), (TUNING_SCALA, one));
    assert!(
        (TUNINGS.offset(0, 61) - 1.0391).abs() < 1e-3,
        "A: 9/8 on C#"
    );
    turn(&mut r, EncoderId::B, 1);
    assert_eq!(now(&r), (TUNING_SCALA, two), "past the first");
    assert_eq!(TUNINGS.offset(0, 61), -0.5, "B plays, not A");
    turn(&mut r, EncoderId::B, 1);
    assert_eq!(now(&r), (TUNING_SCALA, two), "the last");
    turn(&mut r, EncoderId::B, -1);
    assert_eq!(now(&r), (TUNING_SCALA, one), "and back");
    turn(&mut r, EncoderId::B, 1);
    turn(&mut r, EncoderId::B, -9);
    drop(r);
    // Saved and loaded on a fresh unit: still B.
    let mut st = MemStore::new(1);
    let f = screen::put_with(&mut st, "TWO", |q| {
        q.set_scala(PartId::ALL[0], two);
        q.edit_part(PartId::ALL[0]).mix.tuning = TUNING_SCALA;
    });
    long(&mut st, b"GAMELA~1", b"SCL", a, MAJOR);
    long(&mut st, b"GAMELA~2", b"SCL", b, QUARTER);
    let mut ui = UiState::new();
    let mut r = Rig::new(&mut ui, st);
    r.load(f);
    r.work();
    assert_eq!(now(&r), (TUNING_SCALA, two));
    assert_eq!(TUNINGS.offset(0, 61), -0.5);
    r.ui.project_mut().edit_part(PartId::ALL[0]).mix.tuning = 0;
    r.work();
}

/// Past the first `MAX_FILES` the walk stops and says so.
#[test]
fn the_walk_reaches_the_first_128_files_and_says_so() {
    use chimera_core::ui::tuning_walk::MAX_FILES;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    for i in 0..MAX_FILES + 2 {
        r.s.plant(file(format!("S{i:03}").as_bytes(), b"SCL"), QUARTER);
    }
    turn(&mut r, EncoderId::B, 6);
    for _ in 0..MAX_FILES + 5 {
        turn(&mut r, EncoderId::B, 1);
    }
    assert_eq!(now(&r), (TUNING_SCALA, name("S127")));
    assert_eq!(toast(&mut r).as_deref(), Some("FIRST 128 FILES ONLY"));
    turn(&mut r, EncoderId::B, -9);
    turn(&mut r, EncoderId::B, -9);
}

/// The hardware's path draws only what changed: the focused cell still
/// repaints as its name scrolls, frame by frame like a full render.
#[test]
fn the_scrolling_cell_repaints_on_the_dirty_path() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = UiState::new();
    let mut r = on_page(&mut ui, &[]);
    long(&mut r.s, b"BOHLEN~1", b"SCL", "bohlen-pierce.scl", MAJOR);
    turn(&mut r, EncoderId::B, 6);
    let mut fb = screen::Fb::new();
    let mut changed = 0;
    let mut last = 0;
    for _ in 0..2 * MARQUEE_HOLD + 8 * MARQUEE_STEP {
        r.ui.update(chimera_core::ui::animation::UiTick::for_test());
        r.ui.render_dirty_with_audio(
            &mut fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            Some(&screen::audio_fixture()),
            &screen::scope_fixture(),
        );
        let full = screen::render_ui(r.ui);
        let h = full.hash();
        changed += usize::from(h != last);
        last = h;
        assert!(fb.px == full.px, "the dirty path matches a full render");
    }
    assert!(changed > 3, "it scrolled ({changed} changes)");
    turn(&mut r, EncoderId::B, -9);
}
