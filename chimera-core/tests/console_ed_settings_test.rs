//! `ed` on SETTINGS (ADR 0289, protocol 4's slice 3): `ed describe
//! SETTINGS` from the tree's leaves with their crumbs, THEME, MIDI CONFIG
//! and RANDOMIZE addressed, `since`'s SETTINGS section, and the project
//! commands through the box's own card work.
mod common;
mod screen;
use common::console_fake::{Fake, ask};
use screen::{Input, feed};

use std::collections::BTreeSet;

use chimera_core::console::{
    ANSWER_BYTES_MAX, ED_SETTINGS_SLOTS, EdAddr, EdBlock, GlobalBlock, Schema, ed_schema_pages,
    ed_slots_used,
};
use chimera_core::name::is_name_byte;
use chimera_core::project::{LoadLink, PartId, ProjectStatus};
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::block_def::SlotBinding;
use chimera_core::ui::settings::CardCx;
use chimera_core::ui::settings::tree::{Kind, ROOT, Row};
use chimera_core::ui::theme_settings::palette;
use chimera_core::ui::view;
use chimera_hal::testkit::MemStore;

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn apply(f: &mut Fake) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

fn pages(f: &mut Fake) -> Vec<String> {
    (0..ed_schema_pages(Schema::Settings))
        .map(|n| ask(f, &format!("ed describe SETTINGS {n}\n")))
        .collect()
}

fn label(page: &str) -> String {
    page.lines()
        .next()
        .unwrap()
        .split(' ')
        .nth(2)
        .unwrap()
        .to_string()
}

/// Every `param` line's address.
fn listed(texts: &[String]) -> BTreeSet<String> {
    texts
        .iter()
        .flat_map(|t| t.lines())
        .filter_map(|l| l.strip_prefix("param "))
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect()
}

/// Every leaf of the tree, its list's crumb and its own, as the box's
/// breadcrumb names them.
fn leaves() -> Vec<(&'static str, &'static Row)> {
    fn walk(rows: &'static [Row], list: &'static str, out: &mut Vec<(&'static str, &'static Row)>) {
        for r in rows {
            match r.kind {
                Kind::List(sub) => walk(sub, r.crumb, out),
                Kind::Leaf(_) => out.push((list, r)),
                Kind::Screen(_) | Kind::Act(_) | Kind::Later(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    let Kind::List(top) = ROOT.kind else {
        panic!("ROOT is a list");
    };
    walk(top, ROOT.crumb, &mut out);
    out
}

// ---- describe ----

/// The pages are the tree's editable leaves in tree order, but DEMO's: the
/// shared ones, then TUNING and LIVE SNAP once a Part; each page names
/// its crumbs; a read-only leaf (`OnePage::read_only`) has no page.
#[test]
fn settings_walks_the_trees_editable_leaves_with_their_crumbs() {
    let mut f = Fake::new();
    let texts = pages(&mut f);
    let n = ed_schema_pages(Schema::Settings);
    assert_eq!(texts.len(), usize::from(n));
    let editable: Vec<(&str, &Row)> = leaves()
        .into_iter()
        .filter(|(list, r)| {
            let Kind::Leaf(p) = r.kind else {
                unreachable!()
            };
            !p.read_only() && *list != "DEMO"
        })
        .collect();
    let per_part = |r: &Row| {
        let Kind::Leaf(p) = r.kind else {
            unreachable!()
        };
        p.def().params.iter().any(|s| {
            matches!(s.binding, SlotBinding::Param(a) if a.block == chimera_engine::addr::BlockRef::Part)
        })
    };
    let shared: Vec<_> = editable.iter().filter(|(_, r)| !per_part(r)).collect();
    let rounds: Vec<_> = editable.iter().filter(|(_, r)| per_part(r)).collect();
    assert_eq!(texts.len(), shared.len() + rounds.len() * PartId::ALL.len());
    let mut i = 0;
    for (list, r) in &shared {
        let Kind::Leaf(p) = r.kind else {
            unreachable!()
        };
        assert_eq!(label(&texts[i]), p.def().short, "{}", texts[i]);
        assert_eq!(
            texts[i].lines().nth(1).unwrap(),
            format!("crumb \"{list}\" \"{}\"", r.crumb)
        );
        i += 1;
    }
    for part in PartId::ALL {
        for (list, r) in &rounds {
            let Kind::Leaf(p) = r.kind else {
                unreachable!()
            };
            assert_eq!(
                label(&texts[i]),
                format!("PART{}_{}", part.number(), p.def().short)
            );
            assert_eq!(
                texts[i].lines().nth(1).unwrap(),
                format!("crumb \"{list}\" \"{}\"", r.crumb)
            );
            i += 1;
        }
    }
    // The leaves the editor never sees are read-only ones (or DEMO's).
    for (list, r) in leaves() {
        let Kind::Leaf(p) = r.kind else {
            unreachable!()
        };
        let shown = texts
            .iter()
            .any(|t| t.lines().nth(1) == Some(&format!("crumb \"{list}\" \"{}\"", r.crumb)));
        assert_eq!(
            shown,
            !p.read_only() && list != "DEMO",
            "{list} {}",
            r.crumb
        );
    }
    assert!(texts[0].starts_with(&format!("page 0 {} of {n}\n", label(&texts[0]))));
    assert_eq!(
        ask(&mut f, &format!("ed describe SETTINGS {n}\n")),
        format!("ERR ed describe SETTINGS has {n} pages\n")
    );
}

/// Every cell of every leaf is described, at the address the box's own
/// view gives it on its round: CHANNELS and OUTPUTS as the mixer's
/// `PART<n>.CH`/`.OUT`, TUNING's and LIVE SNAP's Part cells as `PART<n>.*`
/// on their round, the rest THEME, MIDI or RANDOM.
#[test]
fn every_leaf_cell_is_described() {
    let mut f = Fake::new();
    let texts = pages(&mut f);
    let got = listed(&texts);
    let ctx = f.ui.slot_ctx_for_test();
    let mut want = BTreeSet::new();
    for (list, r) in leaves() {
        let Kind::Leaf(p) = r.kind else {
            unreachable!()
        };
        if p.read_only() || list == "DEMO" {
            continue;
        }
        let def = p.def();
        for i in 0..def.params.len() {
            let Some(a) = view::view(def, i, &ctx).addr() else {
                continue;
            };
            let rounds: Vec<chimera_core::console::Round> =
                if a.block == chimera_engine::addr::BlockRef::Part {
                    PartId::ALL.map(chimera_core::console::Round::Part).to_vec()
                } else {
                    vec![chimera_core::console::Round::Shared]
                };
            for round in rounds {
                let e = EdAddr::shown(a, round).unwrap_or_else(|| panic!("{a:?}"));
                want.insert(e.to_string());
            }
        }
    }
    assert_eq!(got, want);
    for a in [
        "PART3.CH",
        "PART6.OUT",
        "PART2.TUNING",
        "PART4.SNAP",
        "THEME.ACCENT",
        "MIDI.TUNE",
        "RANDOM.OSC",
    ] {
        assert!(got.contains(a), "{a}");
    }
}

#[test]
fn global_addresses_parse() {
    assert_eq!(
        EdAddr::parse(b"THEME.ACCENT").map(|a| a.at()),
        Some(EdBlock::Global(GlobalBlock::Theme))
    );
    assert_eq!(
        EdAddr::parse(b"RANDOM.SEED").map(|a| a.at()),
        Some(EdBlock::Global(GlobalBlock::Random))
    );
    assert_eq!(ed_slots_used(Schema::Settings), ED_SETTINGS_SLOTS);
}

#[test]
fn ed_schema_settings_has_its_pages_and_hash() {
    let mut f = Fake::new();
    let n = ed_schema_pages(Schema::Settings);
    let a = ask(&mut f, "ed schema SETTINGS\n");
    let line = a.lines().next().unwrap();
    let w: Vec<&str> = line.split(' ').collect();
    assert_eq!(&w[..3], &["schema", "SETTINGS", &n.to_string()]);
    assert_eq!(w[3].len(), 16);
    assert!(a.ends_with("OK\n"));
}

// ---- sets ----

fn set(f: &mut Fake, addr: &str, v: &str) {
    assert_eq!(
        ask(f, &format!("ed set {addr} {v}\n")),
        "OK queued\n",
        "{addr} {v}"
    );
}

/// THEME's set lands through the box's turn path: the panel's colours the
/// shell draws the next frame with are the new accent's.
#[test]
fn a_theme_set_reaches_the_panels_colours_on_the_next_frame() {
    let mut f = Fake::new();
    let before = palette(&f.ui.theme());
    set(&mut f, "THEME.ACCENT", "AMBER");
    apply(&mut f);
    assert_ne!(palette(&f.ui.theme()), before);
    assert!(ask(&mut f, "ed get THEME.ACCENT\n").contains("\"AMBER\""));
}

#[test]
fn a_midi_and_a_parts_settings_cell_set_round_trip() {
    let mut f = Fake::new();
    set(&mut f, "PART3.SNAP", "#1");
    set(&mut f, "MIDI.ECHO", "#0");
    apply(&mut f);
    let snap = ask(&mut f, "ed get PART3.SNAP\n");
    assert!(snap.starts_with("val PART3.SNAP 0.5 1 "), "{snap}");
    let echo = ask(&mut f, "ed get MIDI.ECHO\n");
    assert!(echo.starts_with("val MIDI.ECHO 0 0 "), "{echo}");
}

/// A RANDOM set rolls the followed Part's Sound, as a turn does; taken
/// while Part 1 was the box's, it writes nothing once the box is on
/// another Part.
#[test]
fn a_random_set_on_a_part_the_box_left_writes_nothing() {
    let mut f = Fake::new();
    let sound = |f: &Fake, p: PartId| format!("{:?}", f.ui.project().part(p).sound.params);
    let (p1, p2) = (sound(&f, P1), sound(&f, P2));
    set(&mut f, "RANDOM.SEED", "7");
    // The box moves to Part 2 before the pass.
    feed(&mut f.ui, Input::press(chimera_hal::ButtonId::B2));
    assert_eq!(f.ui.active_part, P2);
    apply(&mut f);
    assert_eq!(sound(&f, P1), p1);
    assert_eq!(sound(&f, P2), p2);
    assert!(ask(&mut f, "ed get RANDOM.SEED\n").starts_with("val RANDOM.SEED 0 0 "));
    // On the followed Part, it rolls.
    set(&mut f, "RANDOM.SEED", "7");
    apply(&mut f);
    assert_ne!(sound(&f, P2), p2);
    assert_eq!(sound(&f, P1), p1);
}

// ---- since ----

fn since(f: &mut Fake, n: u32) -> String {
    ask(f, &format!("ed since {n}\n"))
}

fn section<'a>(answer: &'a str, header: &str) -> &'a str {
    let at = answer
        .find(&format!("\n{header}\n"))
        .unwrap_or_else(|| panic!("no {header}: {answer}"));
    let rest = &answer[at + 1..];
    let end = rest.find("\nOK ").map_or(rest.len(), |e| e + 1);
    &rest[..end]
}

/// Whole: the header, every listed value, the project, each Part's Sound,
/// the listing (none read yet); then a THEME set is its one line.
#[test]
fn since_answers_settings_whole_then_its_diff() {
    let mut f = Fake::new();
    let a = since(&mut f, 0);
    let s = section(&a, "settings");
    assert!(s.contains("\nval THEME.ACCENT "), "{s}");
    assert!(s.contains("\nval MIDI.TUNE "), "{s}");
    assert!(s.contains("\nval RANDOM.OSC "), "{s}");
    assert!(s.contains("\nproject \"NEW PROJECT\" new "), "{s}");
    for p in PartId::ALL {
        assert!(s.contains(&format!("\nsound {} \"", p.number())), "{s}");
    }
    assert!(s.contains("\nprojects 0\n"), "{s}");
    // A Part's SETTINGS cells are the mixer's values.
    let m = section(&a, "mixer");
    assert!(m.contains("\nval PART3.SNAP "), "{m}");
    assert!(!s.contains("PART3."), "{s}");
    let n: u32 = a.trim_end().rsplit(' ').next().unwrap().parse().unwrap();
    assert_eq!(since(&mut f, n), format!("OK {}\n", n + 1));
    set(&mut f, "THEME.ACCENT", "ROSE");
    apply(&mut f);
    let d = since(&mut f, n + 1);
    assert!(d.starts_with("val THEME.ACCENT "), "{d}");
    assert_eq!(d.lines().count(), 2, "{d}");
}

/// An edit marks the project and the Sound edited within a few answers:
/// their CRCs are hashed at most once every eight answers while the
/// project moves (1.7 ms on the chip), never on every poll.
#[test]
fn an_edit_marks_the_project_and_its_sound_within_eight_answers() {
    let mut f = Fake::new();
    let a = since(&mut f, 0);
    assert!(
        a.contains("\nproject \"NEW PROJECT\" new clean gen -\n"),
        "{a}"
    );
    assert!(a.contains("\nsound 1 \""), "{a}");
    let mut n: u32 = a.trim_end().rsplit(' ').next().unwrap().parse().unwrap();
    set(&mut f, "FILTER.CUTOFF", "500");
    apply(&mut f);
    let mut said = String::new();
    for _ in 0..8 {
        let d = since(&mut f, n);
        n = d.trim_end().rsplit(' ').next().unwrap().parse().unwrap();
        said.push_str(&d);
    }
    assert!(
        said.contains("project \"NEW PROJECT\" new edited gen -\n"),
        "{said}"
    );
    assert!(
        said.contains(" edited\n") && said.contains("sound 1 \""),
        "{said}"
    );
    assert_eq!(said.matches("project ").count(), 1, "{said}");
}

/// A toast is said once, when it goes up.
#[test]
fn since_says_a_new_toast_once() {
    let mut f = Fake::new();
    let a = since(&mut f, 0);
    let n: u32 = a.trim_end().rsplit(' ').next().unwrap().parse().unwrap();
    f.ui.show_note(chimera_core::project::ProjectNote::NoIds);
    let d = since(&mut f, n);
    assert!(d.starts_with("toast \""), "{d}");
    assert_eq!(since(&mut f, n + 1), format!("OK {}\n", n + 2));
}

/// SETTINGS whole, every Part edited and a full listing's worth, fits one
/// answer with room.
#[test]
fn a_whole_settings_answer_fits_the_bound() {
    let mut f = Fake::new();
    let a = since(&mut f, 0);
    let s = section(&a, "settings");
    // 48 listed projects at their longest line, past what the rest says.
    let listing = 48 * "proj 9999999 \"ABCDEFGHIJKLMNOP\" newer gen 4294967295\n".len();
    assert!(s.len() + listing < ANSWER_BYTES_MAX / 4, "{}", s.len());
}

// ---- project commands ----

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

    fn ask(&mut self, line: &str) -> String {
        ask(&mut self.f, line)
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

    /// Each listed project's id and generation, by name.
    fn listed(&mut self, name: &str) -> (u32, u32) {
        let projs = self.projs();
        let l = projs
            .iter()
            .find(|l| l.contains(&format!(" \"{name}\" ")))
            .unwrap_or_else(|| panic!("no {name}: {projs:?}"));
        let w: Vec<&str> = l.split(' ').collect();
        (w[1].parse().unwrap(), w.last().unwrap().parse().unwrap())
    }

    fn loaded(&self) -> u32 {
        self.f.ui.project().meta().id().unwrap().get()
    }

    /// The `proj` lines of a whole answer.
    fn projs(&mut self) -> Vec<String> {
        let a = since(&mut self.f, 0);
        a.lines()
            .filter(|l| l.starts_with("proj "))
            .map(str::to_string)
            .collect()
    }

    fn project_line(&mut self) -> String {
        let a = since(&mut self.f, 0);
        a.lines()
            .find(|l| l.starts_with("project "))
            .unwrap()
            .to_string()
    }
}

#[test]
fn a_space_on_the_wire_is_no_name_byte() {
    assert!(!is_name_byte(b'_'));
}

#[test]
fn project_commands_parse_and_refuse() {
    let mut f = Fake::new();
    for bad in [
        "ed project",
        "ed project save now",
        "ed project load",
        "ed project load 3",
        "ed project load 0 1",
        "ed project load 3 1 keep",
        "ed project rename X",
        "ed project delete 3",
        "ed project format",
    ] {
        assert_eq!(
            ask(&mut f, &format!("{bad}\n")),
            "ERR ed project takes save, saveas <name> or load <id> <gen> [discard]\n",
            "{bad}"
        );
    }
    for bad in ["MY SONG", "_SONG", "A.B", "ABCDEFGHIJKLMNOPQ"] {
        let a = ask(&mut f, &format!("ed project saveas {bad}\n"));
        assert!(a.starts_with("ERR ed project"), "{bad}: {a}");
    }
    assert_eq!(
        ask(&mut f, "ed project save\n"),
        "ERR ed project save has no file of its own: saveas\n"
    );
    assert_eq!(
        ask(&mut f, "ed project load 3 1\n"),
        "ERR ed project load has no such id at that gen: ed projects\n"
    );
}

/// `ed projects` while the card is BUSY waits for the job, and the answer
/// never does; save-as makes a new file, save goes over it, load takes the
/// listing's generation as its witness and refuses over edits unless
/// `discard`.
#[test]
fn projects_save_saveas_and_load_go_through_the_boxs_card_work() {
    let mut r = Rig::new();
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    // Asked twice before the pass: one re-list.
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    // The pass queues the job; the card is BUSY until `card_work`.
    apply(&mut r.f);
    assert!(r.f.ui.card_pending());
    assert_eq!(r.ask("ed project saveas MY_SONG\n"), "OK queued\n");
    apply(&mut r.f);
    // Still BUSY: the save-as waits for the listing's job.
    r.work();
    assert!(r.projs().is_empty());
    r.pass();
    assert_eq!(r.f.ui.project().meta().name().as_str(), "MY SONG");
    let song = r.loaded();
    let line = r.project_line();
    assert!(
        line.starts_with(&format!("project \"MY SONG\" {song} clean gen ")),
        "{line}"
    );
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    r.pass();
    assert_eq!(r.projs().len(), 1);
    let (id, gen1) = r.listed("MY SONG");
    assert_eq!(id, song);
    // The project's line reads its file's generation from the listing.
    assert!(r.project_line().ends_with(&format!(" gen {gen1}")));

    // An edit, then save over the project's own file.
    set(&mut r.f, "PART3.LEVEL", "0.25");
    apply(&mut r.f);
    assert_eq!(r.f.ui.project_status(), ProjectStatus::Modified);
    assert!(r.project_line().contains(" edited gen "));
    assert_eq!(r.ask("ed project save\n"), "OK queued\n");
    r.pass();
    assert_ne!(r.f.ui.project_status(), ProjectStatus::Modified);
    assert_eq!(r.loaded(), song);
    // The save moved the file's generation: the old one is no witness.
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    r.pass();
    let (_, gen2) = r.listed("MY SONG");
    assert_ne!(gen1, gen2);
    assert_eq!(
        r.ask(&format!("ed project load {song} {gen1}\n")),
        "ERR ed project load has no such id at that gen: ed projects\n"
    );

    // A second project, then edits: a load over them refuses unless
    // `discard`.
    assert_eq!(r.ask("ed project saveas OTHER\n"), "OK queued\n");
    r.pass();
    let other = r.loaded();
    assert_ne!(other, song);
    set(&mut r.f, "PART3.LEVEL", "0.5");
    apply(&mut r.f);
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    r.pass();
    assert_eq!(r.projs().len(), 2);
    assert_eq!(
        r.ask(&format!("ed project load {song} {gen2}\n")),
        "ERR ed project load over unsaved edits: discard\n"
    );
    assert_eq!(r.loaded(), other);
    assert_eq!(
        r.ask(&format!("ed project load {song} {gen2} discard\n")),
        "OK queued\n"
    );
    r.pass();
    assert_eq!(r.loaded(), song);
    assert_eq!(r.f.ui.project().meta().name().as_str(), "MY SONG");
}

/// A load whose listing moved between the answer and its pass writes
/// nothing: a save queued ahead of it marks the listing stale, as every
/// card job does.
#[test]
fn a_stale_load_witness_writes_nothing() {
    let mut r = Rig::new();
    assert_eq!(r.ask("ed project saveas FIRST\n"), "OK queued\n");
    r.pass();
    assert_eq!(r.ask("ed project saveas SECOND\n"), "OK queued\n");
    r.pass();
    let second = r.loaded();
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    r.pass();
    let (first, g) = r.listed("FIRST");
    assert_eq!(r.ask("ed project save\n"), "OK queued\n");
    assert_eq!(
        r.ask(&format!("ed project load {first} {g}\n")),
        "OK queued\n"
    );
    // The save runs; the load waits for the card, then finds its listing
    // stale.
    r.pass();
    r.pass();
    assert_eq!(r.loaded(), second);
    assert!(!r.f.ui.card_pending());
}

/// A project loaded while a route glides (I1): the act was for the
/// project that left, and the loaded one's cell is never stepped.
#[test]
fn a_project_load_drops_a_route_in_progress() {
    fn cutoff(r: &mut Rig) -> Option<i32> {
        since(&mut r.f, 0)
            .lines()
            .find_map(|l| l.strip_prefix("route ENV2:FILTER.CUTOFF "))
            .map(|l| l.split(' ').next().unwrap().parse().unwrap())
    }
    let mut r = Rig::new();
    assert_eq!(r.ask("ed project saveas PLAIN\n"), "OK queued\n");
    r.pass();
    assert_eq!(r.ask("ed projects\n"), "OK queued\n");
    r.pass();
    let (id, g) = r.listed("PLAIN");
    assert_eq!(r.ask("ed route ENV2:FILTER.CUTOFF 127\n"), "OK queued\n");
    apply(&mut r.f);
    assert_eq!(cutoff(&mut r), Some(10));
    assert_eq!(
        r.ask(&format!("ed project load {id} {g} discard\n")),
        "OK queued\n"
    );
    // The route steps once more, the load is queued behind it and runs.
    r.pass();
    assert_eq!(cutoff(&mut r), None, "PLAIN has no route");
    for _ in 0..30 {
        r.pass();
    }
    assert_eq!(cutoff(&mut r), None);
    assert_ne!(r.f.ui.project_status(), ProjectStatus::Modified);
}

/// The editor's fixture: `ed describe SETTINGS` as the default build
/// describes it (ORBIT's SYNC, USB AUDIO and PUSH ENCODERS are features).
#[cfg(all(feature = "orbit", feature = "usb-audio", feature = "push-encoders"))]
#[test]
fn settings_fixture() {
    let mut f = Fake::new();
    let text = pages(&mut f).join("");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../editor/tests/fixtures/settings-schema.txt");
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
