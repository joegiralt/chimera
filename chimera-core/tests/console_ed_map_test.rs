//! `ed` on MIDI MAP (ADR 0289, protocol 4's slice 4): `ed describe MAP`
//! from the binding model's own tables, `since`'s MAP section (the Sound's
//! rows, the controller map's, learn, the card's maps), the `ed map`
//! commands through the box's own paths, and `ed learn`, which takes the
//! box to the cell and arms learn there.

#![cfg(feature = "midi-map")]

mod common;
mod screen;
use common::console_fake::{Fake, ask};

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::console::{ANSWER_BYTES_MAX, Schema, ed_schema_pages};
use chimera_core::midi_in::{Cc, CcEvent};
use chimera_core::midi_map::ctl::{self, ControllerMap};
use chimera_core::midi_map::{Input, LearnCc, MAP_LEN, Mappable, MidiMap};
use chimera_core::name::SoundName;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::{LoadLink, PartId};
use chimera_core::storage::{Card, Ready, SystemSettings, SystemSync};
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::learn::Learn;
use chimera_core::ui::settings::CardCx;
use chimera_hal::Ms;
use chimera_hal::store::StoreError;
use chimera_hal::testkit::MemStore;

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);

fn apply(f: &mut Fake) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

fn queued(f: &mut Fake, line: &str) {
    assert_eq!(ask(f, &format!("{line}\n")), "OK queued\n", "{line}");
}

/// `ed since <n>`: the answer and its new number.
fn since(f: &mut Fake, n: u32) -> (String, u32) {
    let a = ask(f, &format!("ed since {n}\n"));
    let last = a.trim_end().rsplit('\n').next().unwrap();
    let m = last
        .strip_prefix("OK ")
        .unwrap_or_else(|| panic!("{a}"))
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (a, m)
}

/// The MAP section's lines of a whole answer: its header to the line
/// before MIXER's.
fn map_section(a: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut on = false;
    for l in a.lines() {
        if l.starts_with("map ") {
            on = true;
        }
        if l == "mixer" {
            break;
        }
        if on {
            out.push(l.to_string());
        }
    }
    assert!(on, "no MAP section: {a}");
    out
}

fn rows(a: &str, head: &str) -> Vec<String> {
    map_section(a)
        .into_iter()
        .filter(|l| l.starts_with(head))
        .collect()
}

fn whole(f: &mut Fake) -> String {
    since(f, 0).0
}

fn cc(n: u8) -> Input {
    Input::cc(n).unwrap()
}

fn name(s: &str) -> SoundName {
    SoundName::new(s).unwrap()
}

/// The fake unit with a card, as the shell's card work runs it.
struct Rig {
    f: Fake,
    s: MemStore,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl Rig {
    fn new() -> Rig {
        let mut s = MemStore::new(1);
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        Rig {
            f: Fake::new(),
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
        }
    }

    /// The UI pass, then its card work.
    fn pass(&mut self) {
        apply(&mut self.f);
        self.work();
    }

    fn work(&mut self) {
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.s,
            sync: &mut self.sync,
            settings: &mut self.set,
        };
        self.f
            .ui
            .card_work(chimera_core::errand::Errand::HOST, cx, link, |swap, _| {
                let _ = swap.settle(link, || false);
            });
    }

    /// `rows` (CC, target) saved as map `n`, by a Part on ALGO.
    fn put_map(&mut self, n: &str, rows: &[(u8, ParamAddr)]) {
        let mut own = MidiMap::EMPTY;
        for &(c, a) in rows {
            own.learn(cc(c), Mappable::new(a).unwrap()).unwrap();
        }
        let mut m = ControllerMap::EMPTY;
        m.absorb(&own, EngineType::Algo).unwrap();
        let mut card = Card::new();
        let out = card
            .run(&mut self.s, |s, r: &Ready| {
                Ok::<_, StoreError>(ctl::save(s, r, &name(n), &m))
            })
            .unwrap();
        out.result.unwrap().unwrap();
    }

    fn map_file(&mut self, n: &str) -> Option<ControllerMap> {
        let mut back = ControllerMap::EMPTY;
        let mut card = Card::new();
        let out = card
            .run(&mut self.s, |s, r: &Ready| {
                Ok::<_, StoreError>(ctl::load(s, r, &name(n), &mut back))
            })
            .unwrap();
        out.result.unwrap().ok().map(|_| back)
    }
}

/// `ed schema MAP` is one page, and `ed describe MAP 0` is the binding
/// grammar from the tables: every input form with its legal kinds in E's
/// cycle, each kind's words, the CCs a binding may take.
#[test]
fn the_map_schema_is_the_binding_model() {
    let mut f = Fake::new();
    assert_eq!(ed_schema_pages(Schema::Map), 1);
    let s = ask(&mut f, "ed schema MAP\n");
    assert!(s.starts_with("schema MAP 1 "), "{s}");
    let p = ask(&mut f, "ed describe MAP 0\n");
    let lines: Vec<&str> = p.lines().collect();
    assert_eq!(lines[0], "page 0 MAP of 1");
    assert!(
        lines.contains(
            &"input CC JUMP PICKUP SCALE TWOS-COMP OFFSET SIGNED SIGNED-2 MOMENTARY TOGGLE"
        )
    );
    assert!(lines.contains(&"input CC14 JUMP PICKUP SCALE"));
    assert!(lines.contains(
        &"input NRPN JUMP PICKUP SCALE TWOS-COMP OFFSET SIGNED SIGNED-2 INC-DEC MOMENTARY TOGGLE"
    ));
    assert!(lines.contains(&"input PRESS JUMP PICKUP SCALE"));
    assert!(lines.contains(&"input SYSEX JUMP"));
    assert!(lines.contains(&"kind JUMP \"ABSOLUTE · JUMPS TO THE KNOB\" \"\""));
    assert!(lines.contains(&"kind TWOS-COMP \"RELATIVE · TWOS COMPLEMENT\" \"+1 = 1  -1 = 127\""));
    let ccs: Vec<u8> = lines
        .iter()
        .find_map(|l| l.strip_prefix("ccs "))
        .unwrap()
        .split(' ')
        .map(|n| n.parse().unwrap())
        .collect();
    let want: Vec<u8> = (0..=127).filter(|&n| LearnCc::new(n).is_some()).collect();
    assert_eq!(ccs, want);
    assert!(lines.contains(&"span 0 255"));
    assert!(lines.contains(&format!("rows {MAP_LEN}").as_str()));
    assert!(lines.contains(&format!("maprows {MAP_LEN}").as_str()));
    assert_eq!(*lines.last().unwrap(), "OK");
    assert!(ask(&mut f, "ed describe MAP 1\n").starts_with("ERR"));
}

/// The schema's text, as the editor's fixture holds it.
#[test]
fn map_fixture() {
    let mut f = Fake::new();
    let text = ask(&mut f, "ed describe MAP 0\n");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../editor/tests/fixtures/map-schema.txt");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == text,
        "the schema changed: BLESS=1 rewrites {}",
        path.display()
    );
}

/// Whole: the header, each of the Sound's rows (INIT's default map), the
/// learn line; then nothing moved is nothing said.
#[test]
fn since_answers_map_whole_then_nothing() {
    let mut f = Fake::new();
    let (a, n) = since(&mut f, 0);
    let s = map_section(&a);
    assert_eq!(s[0], "map 1 none keep off");
    assert!(
        s.contains(&"row S1 CC74 JUMP FILTER.CUTOFF 0 255 off live".to_string()),
        "{s:?}"
    );
    assert!(
        s.contains(&"row S3 CC7 JUMP PART1.LEVEL 0 255 off live".to_string()),
        "{s:?}"
    );
    assert!(s.contains(&"learn off \"\"".to_string()), "{s:?}");
    let ordered: Vec<usize> = ["map ", "row S", "learn "]
        .iter()
        .map(|h| s.iter().position(|l| l.starts_with(h)).unwrap())
        .collect();
    assert!(ordered.windows(2).all(|w| w[0] < w[1]), "{s:?}");
    let (d, _) = since(&mut f, n);
    assert_eq!(d, format!("OK {}\n", n + 1));
}

/// `ed map add` is learn's own write: a new row (the full range unless a
/// span is given, echo off), a row on the same target replaced, MAP FULL
/// past sixteen. Illegal pairs and targets refuse at once.
#[test]
fn add_writes_a_sound_row_through_learn() {
    let mut f = Fake::new();
    f.ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    queued(&mut f, "ed map add CC20 PICKUP FILTER.RESO 10 200");
    apply(&mut f);
    let m = f.ui.project().part(P1).sound.map;
    assert_eq!(m.len(), 1);
    assert_eq!(m.entries()[0].target, Mappable::new(RESO).unwrap());
    let a = whole(&mut f);
    assert_eq!(
        rows(&a, "row "),
        vec!["row S1 CC20 PICKUP FILTER.RESO 10 200 off live".to_string()]
    );
    // The same target again: its row replaced, as learn does.
    queued(&mut f, "ed map add NRPN:1234 INC-DEC FILTER.RESO");
    apply(&mut f);
    assert_eq!(
        rows(&whole(&mut f), "row "),
        vec!["row S1 NRPN:1234 INC-DEC FILTER.RESO 0 255 off live".to_string()]
    );
    // Illegal pairs can't be said.
    for bad in [
        "ed map add CC20 INC-DEC FILTER.CUTOFF",
        "ed map add CC14:2 TWOS-COMP FILTER.CUTOFF",
        "ed map add PRESS TOGGLE FILTER.CUTOFF",
        "ed map add SYSEX JUMP FILTER.CUTOFF",
        "ed map add CC0 JUMP FILTER.CUTOFF",
        "ed map add CC14:6 JUMP FILTER.CUTOFF",
        "ed map add CC20 FAST FILTER.CUTOFF",
        "ed map add CC20 JUMP FILTER.CUTOFF 0",
        "ed map add CC20 JUMP FILTER.CUTOFF 0 256",
    ] {
        let got = ask(&mut f, &format!("{bad}\n"));
        assert!(got.starts_with("ERR ed map add takes "), "{bad}: {got}");
    }
    // Not a knob a control may drive; another Part's LEVEL.
    for bad in ["PART1.CH", "PART2.LEVEL", "THEME.ACCENT"] {
        let got = ask(&mut f, &format!("ed map add CC20 JUMP {bad}\n"));
        assert_eq!(got, format!("ERR ed map can't bind {bad}\n"));
    }
    // Sixteen rows: a seventeenth target is refused, nothing queued.
    let mut m = MidiMap::EMPTY;
    let free: Vec<u8> = (2..=127).filter(|&n| LearnCc::new(n).is_some()).collect();
    let targets: Vec<Mappable> = [BlockRef::Filter, BlockRef::Out, BlockRef::VoiceFx]
        .iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(*b, s.id)))
        .filter_map(Mappable::new)
        .collect();
    for (t, &n) in targets.iter().take(MAP_LEN).zip(&free) {
        m.learn(cc(n), *t).unwrap();
    }
    assert_eq!(m.len(), MAP_LEN);
    f.ui.project_mut().edit_part(P1).sound.map = m;
    let got = ask(&mut f, "ed map add CC110 JUMP ALGO.MORPH\n");
    assert_eq!(got, "ERR ed map add MAP FULL\n");
}

/// `ed map set`: the box's B to F on a Sound row, each a value; what the
/// row's control can't take is refused, nothing queued.
#[test]
fn set_edits_a_sound_row() {
    let mut f = Fake::new();
    f.ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    queued(&mut f, "ed map add CC20 JUMP FILTER.CUTOFF");
    apply(&mut f);
    queued(&mut f, "ed map set S1 control 21");
    queued(&mut f, "ed map set S1 min 5");
    apply(&mut f);
    // Each set was taken against the row as since saw it: the second is
    // stale once the first lands, so it writes nothing.
    assert_eq!(
        rows(&whole(&mut f), "row "),
        vec!["row S1 CC21 JUMP FILTER.CUTOFF 0 255 off live".to_string()]
    );
    for (cmd, want) in [
        ("min 5", "row S1 CC21 JUMP FILTER.CUTOFF 5 255 off live"),
        ("max 7", "row S1 CC21 JUMP FILTER.CUTOFF 5 7 off live"),
        (
            "input TWOS-COMP",
            "row S1 CC21 TWOS-COMP FILTER.CUTOFF 5 7 off live",
        ),
        ("echo on", "row S1 CC21 TWOS-COMP FILTER.CUTOFF 5 7 on live"),
    ] {
        queued(&mut f, &format!("ed map set S1 {cmd}"));
        apply(&mut f);
        assert_eq!(
            rows(&whole(&mut f), "row "),
            vec![want.to_string()],
            "{cmd}"
        );
    }
    // A drag: each set of the same row and field takes the one act, more
    // than the queue holds.
    for v in 0..20 {
        queued(&mut f, &format!("ed map set S1 max {v}"));
    }
    apply(&mut f);
    assert_eq!(
        rows(&whole(&mut f), "row "),
        vec!["row S1 CC21 TWOS-COMP FILTER.CUTOFF 5 19 on live".to_string()]
    );
    assert_eq!(
        ask(&mut f, "ed map set S1 input INC-DEC\n"),
        "ERR ed map set S1 can't read INC-DEC\n"
    );
    assert_eq!(
        ask(&mut f, "ed map set S1 control 0\n"),
        "ERR ed map set S1 can't take control 0\n"
    );
    assert_eq!(
        ask(&mut f, "ed map set S2 min 1\n"),
        "ERR ed map has no S2 here\n"
    );
    assert!(ask(&mut f, "ed map set S1 min 256\n").starts_with("ERR ed map takes "));
    assert!(ask(&mut f, "ed map set S1 volume 2\n").starts_with("ERR ed map takes "));
    // A fixed control (pressure): B does nothing on the box, so no control.
    queued(&mut f, "ed map add PRESS JUMP FILTER.RESO");
    apply(&mut f);
    assert_eq!(
        ask(&mut f, "ed map set S2 control 21\n"),
        "ERR ed map set S2 has a fixed control\n"
    );
}

/// `ed map rm` is witnessed by the row's input and target as `since` said
/// them: a row that moved or changed refuses, and the right one goes.
#[test]
fn rm_takes_the_witnessed_row() {
    let mut f = Fake::new();
    f.ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    queued(&mut f, "ed map add CC20 JUMP FILTER.CUTOFF");
    queued(&mut f, "ed map add CC21 JUMP FILTER.RESO");
    apply(&mut f);
    assert_eq!(
        ask(&mut f, "ed map rm S1 CC21 FILTER.RESO\n"),
        "ERR ed map rm S1 is not that row: ed since\n"
    );
    assert_eq!(
        ask(&mut f, "ed map rm S1 CC20 FILTER.RESO\n"),
        "ERR ed map rm S1 is not that row: ed since\n"
    );
    queued(&mut f, "ed map rm S1 CC20 FILTER.CUTOFF");
    apply(&mut f);
    assert_eq!(
        rows(&whole(&mut f), "row "),
        vec!["row S1 CC21 JUMP FILTER.RESO 0 255 off live".to_string()]
    );
    // Queued, then the row moved to another control: nothing goes.
    queued(&mut f, "ed map rm S1 CC21 FILTER.RESO");
    let mut m = f.ui.project().part(P1).sound.map;
    let e = m.entries()[0];
    m.set(0, chimera_core::midi_map::Binding { input: cc(22), ..e });
    f.ui.project_mut().edit_part(P1).sound.map = m;
    apply(&mut f);
    assert_eq!(f.ui.project().part(P1).sound.map.len(), 1);
}

/// A controller map's rows: `M<i>`, dimmed where the Sound's rows shadow
/// them; an edit or a removal is a card job, one waiting at a time: a
/// second while one waits says CARD BUSY. Pick, keep, save and rename go
/// through row 0's own paths; `ed maps` lists the card.
#[test]
fn map_rows_edit_through_the_card() {
    let mut r = Rig::new();
    r.put_map("EC4", &[(21, CUTOFF), (74, RESO)]);
    r.put_map("LCXL3", &[(30, CUTOFF)]);
    // The Part's own rows: only CC 74 (cutoff), which shadows the map's.
    let mut own = MidiMap::EMPTY;
    own.learn(cc(74), Mappable::new(CUTOFF).unwrap()).unwrap();
    r.f.ui.project_mut().edit_part(P1).sound.map = own;
    queued(&mut r.f, "ed maps");
    r.pass();
    r.pass();
    let a = whole(&mut r.f);
    let files = rows(&a, "mapfile ");
    assert_eq!(
        files,
        vec![
            "mapfile \"EC4\"".to_string(),
            "mapfile \"LCXL3\"".to_string()
        ]
    );
    assert_eq!(
        ask(&mut r.f, "ed map pick NOPE\n"),
        "ERR ed map pick has no such map: ed maps\n"
    );
    queued(&mut r.f, "ed map pick EC4");
    r.pass();
    r.pass();
    let a = whole(&mut r.f);
    assert_eq!(map_section(&a)[0], "map 1 \"EC4\" keep off");
    let m = rows(&a, "row M");
    assert!(
        m.contains(&"row M1 CC21 JUMP FILTER.CUTOFF 0 255 off live".to_string()),
        "{m:?}"
    );
    assert!(
        m.contains(&"row M2 CC74 JUMP FILTER.RESO 0 255 off shadowed".to_string()),
        "{m:?}"
    );
    // An edit of M1 waits for the card: another row's edit is CARD BUSY.
    queued(&mut r.f, "ed map set M1 min 9");
    apply(&mut r.f);
    assert_eq!(
        ask(&mut r.f, "ed map set M2 min 1\n"),
        "ERR ed map CARD BUSY: a map job waits\n"
    );
    assert_eq!(
        ask(&mut r.f, "ed map rm M2 CC74 FILTER.RESO\n"),
        "ERR ed map CARD BUSY: a map job waits\n"
    );
    // The same row's edit takes the job on.
    queued(&mut r.f, "ed map set M1 max 99");
    apply(&mut r.f);
    r.work();
    let file = r.map_file("EC4").unwrap();
    let row = file
        .section(None)
        .entries()
        .iter()
        .find(|b| b.target == Mappable::new(CUTOFF).unwrap())
        .copied()
        .unwrap();
    assert_eq!((row.span.min, row.span.max), (9, 99));
    // A map row removed from the file, witnessed.
    r.work();
    queued(&mut r.f, "ed map rm M2 CC74 FILTER.RESO");
    r.pass();
    r.pass();
    let file = r.map_file("EC4").unwrap();
    assert!(
        file.section(None)
            .entries()
            .iter()
            .all(|b| b.target != Mappable::new(RESO).unwrap())
    );
    // KEEP MAP, idempotent.
    queued(&mut r.f, "ed map keep on");
    queued(&mut r.f, "ed map keep on");
    r.pass();
    assert_eq!(map_section(&whole(&mut r.f))[0], "map 1 \"EC4\" keep on");
    // Save the Part's rows into the picked map: they move into it.
    queued(&mut r.f, "ed map save");
    r.pass();
    r.pass();
    assert!(r.f.ui.project().part(P1).sound.map.is_empty());
    // Save as another name: the Part picks it.
    queued(&mut r.f, "ed map save MY_MAP");
    r.pass();
    r.pass();
    assert_eq!(
        r.f.ui.project().controller(P1).map(|n| n.padded()),
        Some(name("MY MAP").padded())
    );
    assert!(r.map_file("MY MAP").is_some());
    // Rename it: the file follows.
    queued(&mut r.f, "ed map rename YOURS");
    r.pass();
    r.pass();
    assert!(r.map_file("YOURS").is_some());
    assert!(r.map_file("MY MAP").is_none());
    // None picked: save and rename have nothing to name.
    queued(&mut r.f, "ed map pick none");
    r.pass();
    assert_eq!(r.f.ui.project().controller(P1), None);
    assert_eq!(
        ask(&mut r.f, "ed map save\n"),
        "ERR ed map has no map picked: ed map save <name>\n"
    );
}

fn hear_cc(f: &mut Fake, n: u8, v: u8, at: u32) {
    let ev = CcEvent {
        channel: MidiChannel::new(0).unwrap(),
        cc: Cc::new(n).unwrap(),
        value: v,
    };
    f.ui.hear_ccs(core::iter::once(ev), Ms(at), true);
}

/// `ed learn` takes the box to the cell's page, focuses it and arms learn
/// there, as MIX + SEQ would; a CC then learns it. `since` says so.
/// ORBIT's and SETTINGS' cells refuse, as on the box.
#[test]
fn learn_takes_the_box_to_the_cell() {
    let mut f = Fake::new();
    f.ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    assert_ne!(f.ui.page_def().id, FILTER.id);
    queued(&mut f, "ed learn FILTER.RESO");
    apply(&mut f);
    assert_eq!(f.ui.page_def().id, FILTER.id);
    assert!(matches!(f.ui.learn(), Learn::Armed { part: P1, .. }));
    let a = whole(&mut f);
    let l = rows(&a, "learn ");
    assert_eq!(l.len(), 1);
    assert!(l[0].starts_with("learn armed FILTER.RESO \""), "{l:?}");
    hear_cc(&mut f, 33, 64, 1000);
    let m = f.ui.project().part(P1).sound.map;
    assert_eq!(m.len(), 1);
    assert_eq!(m.entries()[0].target, Mappable::new(RESO).unwrap());
    assert_eq!(
        ask(&mut f, "ed learn RING1.HITS\n"),
        "ERR ed learn RING1.HITS is no Part page's cell\n"
    );
    assert_eq!(
        ask(&mut f, "ed learn THEME.ACCENT\n"),
        "ERR ed learn THEME.ACCENT is no Part page's cell\n"
    );
    assert_eq!(
        ask(&mut f, "ed learn PART1.CH\n"),
        "ERR ed learn PART1.CH is not mappable\n"
    );
    queued(&mut f, "ed learn off");
    apply(&mut f);
    assert_eq!(f.ui.learn(), Learn::Off);
    let a = whole(&mut f);
    assert_eq!(rows(&a, "learn "), vec!["learn off \"\"".to_string()]);
}

/// `ed learn` is for the Part its answer checked: a box on another Part
/// by the pass arms nothing there.
#[test]
fn learn_asked_on_a_part_the_box_left_arms_nothing() {
    let mut f = Fake::new();
    queued(&mut f, "ed learn FILTER.RESO");
    screen::feed(&mut f.ui, screen::Input::press(chimera_hal::ButtonId::B2));
    assert_eq!(f.ui.active_part, PartId::ALL[1]);
    apply(&mut f);
    assert_eq!(f.ui.learn(), Learn::Off);
    // Asked again, on the Part the box shows: armed there.
    queued(&mut f, "ed learn FILTER.RESO");
    apply(&mut f);
    assert!(matches!(f.ui.learn(), Learn::Armed { part, .. } if part == PartId::ALL[1]));
}

/// A diff after learn moved is its `learn` line alone.
#[test]
fn a_learn_change_is_its_line() {
    let mut f = Fake::new();
    let (_, n) = since(&mut f, 0);
    queued(&mut f, "ed learn FILTER.CUTOFF");
    apply(&mut f);
    let (d, _) = since(&mut f, n);
    let mapped: Vec<&str> = d
        .lines()
        .filter(|l| l.starts_with("map ") || l.starts_with("row ") || l.starts_with("learn "))
        .collect();
    assert_eq!(mapped.len(), 1, "{d}");
    assert!(mapped[0].starts_with("learn armed FILTER.CUTOFF \""), "{d}");
}

/// The worst MAP section fits the bound by itself: sixteen Sound rows and
/// a full controller map's, each at its longest, and room for 106 rows
/// (ADR 0266's 90 a map) besides.
#[test]
fn a_whole_map_section_fits_the_bound() {
    let mut f = Fake::new();
    let mut m = MidiMap::EMPTY;
    let targets: Vec<Mappable> = BlockRef::ALL
        .iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(*b, s.id)))
        .filter_map(Mappable::new)
        .collect();
    for (i, t) in targets.iter().take(MAP_LEN).enumerate() {
        let n = chimera_core::midi_map::Input::Nrpn(
            chimera_midi::value::U14::new(16383 - i as u16).unwrap(),
            chimera_core::midi_map::NrpnKind::Relative(chimera_midi::rel::ByteEncoding::SignBitUp),
        );
        m.learn(n, *t).unwrap();
    }
    f.ui.project_mut().edit_part(P1).sound.map = m;
    let a = whole(&mut f);
    let s = map_section(&a);
    let longest = s.iter().map(|l| l.len() + 1).max().unwrap();
    let size: usize = s.iter().map(|l| l.len() + 1).sum();
    assert!(size < ANSWER_BYTES_MAX, "{size}");
    assert!(106 * longest + 16 * 26 < ANSWER_BYTES_MAX, "{longest}");
}
