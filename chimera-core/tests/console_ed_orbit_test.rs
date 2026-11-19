//! `ed` on ORBIT (ADR 0289, protocol 4's slice 5): `ed describe ORBIT`
//! from `ORBIT_CHAIN` with RHYTHM, VOICE and NOTES once a ring and the
//! cells no param as `act` lines, `ORBIT`, `RING<k>` and `TEMPO` set
//! through `OrbitSong::edit`, since's ORBIT section, and the transport,
//! scene, chord, scale, step and pool commands through the box's paths.

// ORBIT's pages and song: only with the feature.
#![cfg(feature = "orbit")]

mod common;
mod screen;
use common::console_fake::{Fake, ask};
use screen::hold;

use std::collections::BTreeSet;

use chimera_core::block::Block;
use chimera_core::console::{ANSWER_BYTES_MAX, Schema, ed_schema_pages};
use chimera_core::orbit::runner::OrbitView;
use chimera_core::orbit::scene::{SceneIdx, SceneRing};
use chimera_core::orbit::{RINGS, RingIdx};
use chimera_core::ui::block_def::SlotBinding;
use chimera_core::ui::orbit::{ORBIT_CHAIN, OrbitCell};
use chimera_hal::ButtonId::Seq;

const S: [SceneIdx; 8] = SceneIdx::ALL;
const R: [RingIdx; RINGS] = RingIdx::ALL;

fn apply(f: &mut Fake) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

fn ok(f: &mut Fake, line: &str) {
    assert_eq!(ask(f, &format!("{line}\n")), "OK queued\n", "{line}");
    apply(f);
}

fn pages(f: &mut Fake) -> Vec<String> {
    (0..ed_schema_pages(Schema::Orbit))
        .map(|n| ask(f, &format!("ed describe ORBIT {n}\n")))
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

/// SYNC INT: the transport is ORBIT's own (ADR 0080).
fn sync_int(f: &mut Fake) {
    ok(f, "ed set MIDI.SYNC INT");
}

/// `ed since <n>`'s lines (the `OK` line apart), its new number and
/// whether more is owed.
fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32, bool) {
    let got = ask(f, &format!("ed since {n}\n"));
    let mut lines: Vec<String> = got.lines().map(str::to_string).collect();
    let last = lines.pop().unwrap();
    let rest = last.strip_prefix("OK ").unwrap_or_else(|| panic!("{got}"));
    let (m, more) = match rest.strip_suffix(" more") {
        Some(m) => (m, true),
        None => (rest, false),
    };
    (lines, m.parse().unwrap(), more)
}

/// ORBIT's section of an answer's lines: from its header to SETTINGS'.
fn orbit_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .skip_while(|l| *l != "orbit")
        .take_while(|l| *l != "settings")
        .cloned()
        .collect()
}

fn rev(f: &Fake, n: SceneIdx) -> u16 {
    f.ui.project().orbit().slot_rev(n)
}

// ---- describe ----

/// ORB, CUSTOM, CHRD and SCN shared, then each ring's RHYTHM, VOICE and
/// NOTES, `RING2_RHYTHM`: 19 pages, every one a page of the chain.
#[test]
fn orbit_walks_its_chain_shared_then_each_ring() {
    let mut f = Fake::new();
    let texts = pages(&mut f);
    assert_eq!(ed_schema_pages(Schema::Orbit), 19);
    let labels: Vec<String> = texts.iter().map(|t| label(t)).collect();
    let mut want: Vec<String> = ["ORB", "CUSTOM", "CHRD", "SCN"].map(String::from).to_vec();
    for k in 1..=RINGS {
        for p in ["RHYTHM", "VOICE", "NOTES"] {
            want.push(format!("RING{k}_{p}"));
        }
    }
    assert_eq!(labels, want);
    for (i, t) in texts.iter().enumerate() {
        assert!(t.ends_with("OK\n"), "{t}");
        assert!(t.starts_with(&format!("page {i} ")), "{t}");
        assert!(t.lines().nth(1).unwrap().starts_with("panel "), "{t}");
    }
    let schema = ask(&mut f, "ed schema ORBIT\n");
    assert!(schema.starts_with("schema ORBIT 19 "), "{schema}");
}

/// Every cell of every page in every ring round: a ring's cells are
/// `RING<k>.*` on its own pages, ORBIT's own cells `act` lines, the chord
/// ring `ORBIT.*` and the tempo `TEMPO.BPM`; nothing dropped but EMPTY.
#[test]
fn every_cell_in_every_ring_round_is_described() {
    let mut f = Fake::new();
    let texts = pages(&mut f);
    let mut acts = BTreeSet::new();
    let mut params = BTreeSet::new();
    for t in &texts {
        let page = label(t);
        let ring = page
            .strip_prefix("RING")
            .map(|r| r.split('_').next().unwrap().to_string());
        for l in t.lines() {
            if let Some(a) = l.strip_prefix("param ") {
                let addr = a.split(' ').next().unwrap();
                if let Some(k) = &ring {
                    assert!(addr.starts_with(&format!("RING{k}.")), "{page}: {addr}");
                } else {
                    assert!(
                        addr.starts_with("ORBIT.") || addr == "TEMPO.BPM",
                        "{page}: {addr}"
                    );
                }
                params.insert(addr.to_string());
            }
            if let Some(a) = l.strip_prefix("act ") {
                acts.insert(a.to_string());
            }
        }
    }
    // Each ring's fifteen, the chords' four, the tempo.
    assert_eq!(params.len(), RINGS * 15 + 4 + 1, "{params:?}");
    for k in 1..=RINGS {
        for id in ["STEPS", "HITS", "MUTE", "GATE", "OCTAVE", "TARGET", "WALK"] {
            assert!(params.contains(&format!("RING{k}.{id}")), "RING{k}.{id}");
        }
    }
    // Each cell the chain binds, as its label and token.
    let mut bound = Vec::new();
    for b in ORBIT_CHAIN.blocks {
        for def in std::iter::once(b.def).chain(b.sub_pages.iter().copied()) {
            for s in &def.params {
                if let SlotBinding::Orbit(c) = s.binding
                    && !bound.contains(&c)
                {
                    bound.push(c);
                }
            }
        }
    }
    let token = |c: OrbitCell| match c {
        OrbitCell::Ring => "RING",
        OrbitCell::Degree => "DEGREE",
        OrbitCell::Cursor => "CURSOR",
        OrbitCell::Set => "SET",
        OrbitCell::Note => "NOTE",
        OrbitCell::Learn => "LEARN",
        OrbitCell::Clear => "CLEAR",
        OrbitCell::Play => "PLAY",
        OrbitCell::Scene => "SCENE",
        OrbitCell::Save => "SAVE",
        OrbitCell::Switch => "SWITCH",
    };
    let want: BTreeSet<String> = bound
        .iter()
        .map(|&c| format!("\"{}\" {}", c.label(), token(c)))
        .collect();
    assert_eq!(acts, want);
    assert_eq!(bound.len(), 11, "every OrbitCell is on a page");
}

/// The editor's fixture: `ed describe ORBIT`, every page.
#[test]
fn orbit_fixture() {
    let mut f = Fake::new();
    let text = pages(&mut f).join("");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../editor/tests/fixtures/orbit-schema.txt");
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

// ---- sets ----

/// A set on RING3 lands on ring 3 while the box shows ring 1, on `live`
/// and the playing slot alike (ADR 0083); `get` reads it back.
#[test]
fn a_set_on_ring3_lands_on_ring3_while_the_box_shows_ring1() {
    let mut f = Fake::new();
    hold(&mut f.ui, Seq);
    assert_eq!(f.ui.location().orbit().unwrap().ring(), R[0]);
    let before = f.ui.project().orbit().live().rings[0];
    ok(&mut f, "ed set RING3.GATE 40");
    let song = f.ui.project().orbit();
    assert_eq!(song.live().rings[2].get(SceneRing::GATE), 40.0);
    assert_eq!(
        song.scene(song.current()).rings[2].get(SceneRing::GATE),
        40.0
    );
    assert_eq!(song.live().rings[0], before, "ring 1 untouched");
    assert_eq!(
        f.ui.location().orbit().unwrap().ring(),
        R[0],
        "the box stays"
    );
    let got = ask(&mut f, "ed get RING3.GATE\n");
    assert!(got.starts_with("val RING3.GATE "), "{got}");
    assert!(got.contains(" 40 "), "{got}");
    // The chord ring and the tempo.
    // SEGS stores its count less one, as the box shows it one-based.
    ok(&mut f, "ed set ORBIT.SEGS 3");
    assert_eq!(f.ui.project().orbit().live().chords.segs.get(), 4);
    ok(&mut f, "ed set TEMPO.BPM 97");
    assert_eq!(f.ui.project().orbit().bpm.get(), 97);
}

/// A set marks the ring touched as a turn does, and the frame's flush
/// sends it to the runner: it hears it on the next loop (ADR 0082).
#[test]
fn a_set_marks_the_ring_touched_and_the_runner_hears_it() {
    let mut f = Fake::new();
    let head = f.ui.orbit().log().head();
    ok(&mut f, "ed set RING2.HITS 1");
    assert!(!f.ui.project().orbit().live().untouched(), "touched");
    f.ui.flush_orbit();
    assert!(f.ui.project().orbit().live().untouched(), "harvested");
    assert_ne!(f.ui.orbit().log().head(), head, "the edit is in the log");
    assert!(!f.ui.orbit().idle());
}

/// BPM under a MIDI clock is dimmed on the box, and refused here.
#[test]
fn bpm_under_a_midi_clock_is_refused() {
    let mut f = Fake::new();
    let v = OrbitView {
        source: chimera_core::orbit::clock::ClockSource::Midi,
        ..OrbitView::default()
    };
    f.ui.set_orbit_view(&v);
    assert_eq!(
        ask(&mut f, "ed set TEMPO.BPM 90\n"),
        "ERR ed set TEMPO.BPM is dimmed: a MIDI clock leads\n"
    );
    // A ring's cells aren't.
    ok(&mut f, "ed set RING1.CHANCE 50");
}

// ---- transport ----

/// SYNC EXT: the DAW drives the transport (ADR 0080), and both refuse.
#[test]
fn transport_under_ext_refuses() {
    let mut f = Fake::new();
    for c in ["play", "stop"] {
        assert_eq!(
            ask(&mut f, &format!("ed orbit {c}\n")),
            "ERR ed orbit: SYNC EXT, the DAW drives the transport\n"
        );
    }
    assert_eq!(
        ask(&mut f, "ed orbit toggle\n"),
        "ERR ed orbit takes play or stop\n"
    );
}

/// `play` flips the transport once; `play` while playing does nothing;
/// `stop` stops.
#[test]
fn play_while_playing_does_nothing() {
    let mut f = Fake::new();
    sync_int(&mut f);
    f.ui.flush_orbit();
    let head = f.ui.orbit().log().head();
    ok(&mut f, "ed orbit play");
    f.ui.flush_orbit();
    let flipped = f.ui.orbit().log().head();
    assert_ne!(flipped, head, "B6's press is in the log");
    // The runner plays and acks it.
    f.ui.set_orbit_view(&OrbitView {
        running: true,
        acked: flipped,
        ..OrbitView::default()
    });
    ok(&mut f, "ed orbit play");
    f.ui.flush_orbit();
    assert_eq!(f.ui.orbit().log().head(), flipped, "nothing sent");
    ok(&mut f, "ed orbit stop");
    f.ui.flush_orbit();
    assert_ne!(f.ui.orbit().log().head(), flipped, "stop is a press");
}

/// A transport asked while a press is still unapplied waits for it, so
/// it reads the runner's transport as it is: two `play`s are one press.
#[test]
fn play_twice_before_the_runner_answers_is_one_press() {
    let mut f = Fake::new();
    sync_int(&mut f);
    f.ui.flush_orbit();
    let head = f.ui.orbit().log().head();
    ok(&mut f, "ed orbit play");
    f.ui.flush_orbit();
    let one = f.ui.orbit().log().head();
    ok(&mut f, "ed orbit play");
    f.ui.flush_orbit();
    assert_eq!(f.ui.orbit().log().head(), one, "it waits");
    f.ui.set_orbit_view(&OrbitView {
        running: true,
        acked: one,
        ..OrbitView::default()
    });
    apply(&mut f);
    f.ui.flush_orbit();
    assert_eq!(f.ui.orbit().log().head(), one, "already playing");
    assert_ne!(head, one);
}

// ---- scenes ----

/// `ed scene queue <n>` queues as SCENE's turn does; back to the playing
/// one cancels it.
#[test]
fn scene_queue_queues_and_cancels() {
    let mut f = Fake::new();
    ok(&mut f, "ed scene queue 3");
    assert_eq!(f.ui.project().orbit().queued(), Some(S[2]));
    ok(&mut f, "ed scene queue 1");
    assert_eq!(f.ui.project().orbit().queued(), None);
    assert_eq!(
        ask(&mut f, "ed scene queue 9\n").lines().next().unwrap(),
        "ERR ed scene takes queue <n>, save <n> [over <rev>], clear <n> <rev>, reload <n> <rev> or move <a> <b> <rev a> <rev b>"
    );
}

/// SAVE into a neutral slot at once; over a saved one it needs the
/// slot's rev, and a stale rev writes nothing.
#[test]
fn save_over_with_a_stale_rev_writes_nothing() {
    let mut f = Fake::new();
    ok(&mut f, "ed scene save 2");
    assert!(f.ui.project().orbit().saved().contains(S[1]));
    let r = rev(&f, S[1]);
    assert_eq!(
        ask(&mut f, "ed scene save 2\n"),
        "ERR ed scene save: a saved scene: over <rev>\n"
    );
    // An edit since, then the stale rev.
    ok(&mut f, "ed set RING1.GATE 10");
    let stale = r.wrapping_sub(1);
    assert_eq!(
        ask(&mut f, &format!("ed scene save 2 over {stale}\n")),
        "ERR ed scene: the slot changed since: ed since\n"
    );
    // The right rev writes `live` over it; the same rev again, queued in
    // the same pass, is stale by then and writes nothing.
    for _ in 0..2 {
        assert_eq!(
            ask(&mut f, &format!("ed scene save 2 over {r}\n")),
            "OK queued\n"
        );
    }
    apply(&mut f);
    assert_eq!(rev(&f, S[1]), r.wrapping_add(1), "one write");
    assert_eq!(
        f.ui.project().orbit().scene(S[1]).rings[0].get(SceneRing::GATE),
        10.0
    );
}

/// Saving while a switch waits refuses, as on the box.
#[test]
fn save_while_a_switch_waits_refuses() {
    let mut f = Fake::new();
    ok(&mut f, "ed scene queue 4");
    assert_eq!(
        ask(&mut f, "ed scene save 2\n"),
        "ERR ed scene: a switch is pending\n"
    );
}

/// CLEAR and RELOAD are the playing scene's, witnessed.
#[test]
fn clear_and_reload_take_the_playing_scene_witnessed() {
    let mut f = Fake::new();
    let r = rev(&f, S[0]);
    assert_eq!(
        ask(&mut f, &format!("ed scene clear 2 {r}\n")),
        "ERR ed scene: not the playing scene\n"
    );
    assert_eq!(
        ask(&mut f, &format!("ed scene reload 1 {r}\n")),
        "ERR ed scene reload: nothing to reload\n"
    );
    assert_eq!(
        ask(&mut f, &format!("ed scene clear 1 {}\n", r.wrapping_add(1))),
        "ERR ed scene: the slot changed since: ed since\n"
    );
    ok(&mut f, &format!("ed scene clear 1 {r}"));
    let song = f.ui.project().orbit();
    assert!(!song.saved().contains(S[0]), "neutral");
    assert_eq!(song.queued(), Some(S[0]), "it lands again on the loop");
}

/// MOVE swaps two slots, both witnessed; a stale rev writes nothing.
/// The playing scene moves with its slot.
#[test]
fn move_swaps_with_both_revs_and_refuses_a_stale_one() {
    let mut f = Fake::new();
    ok(&mut f, "ed scene save 3");
    ok(&mut f, "ed set RING1.GATE 20");
    let (a, b) = (rev(&f, S[0]), rev(&f, S[4]));
    let one = *f.ui.project().orbit().scene(S[0]);
    assert_eq!(
        ask(
            &mut f,
            &format!("ed scene move 1 5 {} {b}\n", a.wrapping_add(7))
        ),
        "ERR ed scene: the slot changed since: ed since\n"
    );
    assert_eq!(
        ask(&mut f, &format!("ed scene move 1 1 {a} {a}\n"))
            .lines()
            .next()
            .unwrap(),
        "ERR ed scene takes queue <n>, save <n> [over <rev>], clear <n> <rev>, reload <n> <rev> or move <a> <b> <rev a> <rev b>"
    );
    ok(&mut f, &format!("ed scene move 1 5 {a} {b}"));
    let song = f.ui.project().orbit();
    assert_eq!(*song.scene(S[4]), one);
    assert!(!song.saved().contains(S[0]), "slot 1 took 5's neutral");
    assert_eq!(song.current(), S[4], "the playing scene moved with it");
    // Again with the old revs: stale.
    assert_eq!(
        ask(&mut f, &format!("ed scene move 1 5 {a} {b}\n")),
        "ERR ed scene: the slot changed since: ed since\n"
    );
}

// ---- notes and chords ----

/// `chord`, `scale`, `step` and `pool` round-trip through since.
#[test]
fn chord_scale_step_and_pool_round_trip_through_since() {
    let mut f = Fake::new();
    let (lines, m, _) = since(&mut f, 0);
    let orbit = orbit_lines(&lines);
    assert!(orbit.contains(&"chord 2 6".to_string()), "{orbit:?}");
    assert!(orbit.contains(&"pool 1 33 33 41 41 43 43 45 45".to_string()));
    assert_eq!(orbit.iter().filter(|l| l.starts_with("step ")).count(), 160);
    assert_eq!(orbit.iter().filter(|l| l.starts_with("chord ")).count(), 8);
    assert_eq!(orbit.iter().filter(|l| l.starts_with("slot ")).count(), 8);
    ok(&mut f, "ed chord 3 5");
    ok(&mut f, "ed scale 661");
    ok(&mut f, "ed step 4 7 61");
    ok(&mut f, "ed pool 2 72 60 67");
    let (lines, _, _) = since(&mut f, m);
    for l in ["chord 3 5", "scale 661", "step 4 7 61", "pool 2 60 67 72"] {
        assert!(lines.contains(&l.to_string()), "{l}: {lines:?}");
    }
    assert!(!lines.contains(&"orbit".to_string()), "a diff: {lines:?}");
    let c = &f.ui.project().orbit().live().chords;
    assert_eq!(c.scale, chimera_core::orbit::chords::ScaleKind::Custom);
    assert_eq!(c.degrees[2].index(), 4);
    for bad in [
        "ed chord 9 1",
        "ed chord 1 13",
        "ed scale 2",
        "ed scale 4096",
        "ed step 6 1 60",
        "ed step 1 33 60",
        "ed step 1 1 128",
        "ed pool 1",
        "ed pool 1 1 2 3 4 5 6 7 8 9",
    ] {
        assert!(
            ask(&mut f, &format!("{bad}\n")).starts_with("ERR ed "),
            "{bad}"
        );
    }
}

/// A pool or step set marks its ring's notes touched, for the runner.
#[test]
fn a_pool_marks_the_ring_touched() {
    let mut f = Fake::new();
    ok(&mut f, "ed pool 5 81");
    assert!(!f.ui.project().orbit().live().untouched());
}

// ---- since ----

/// Whole: the header, the song line, eight slots, every value; then a
/// diff of what moved. A SCENE queued moves the song line alone.
#[test]
fn since_answers_orbit_whole_then_its_diffs() {
    let mut f = Fake::new();
    let (lines, m, more) = since(&mut f, 0);
    assert!(!more);
    let orbit = orbit_lines(&lines);
    assert_eq!(orbit[0], "orbit");
    assert_eq!(
        orbit[1],
        "song stopped clock INT scene 1 queued - edited no"
    );
    assert_eq!(orbit[2], "slot 1 saved rev 0");
    assert_eq!(orbit[3], "slot 2 neutral rev 0");
    let vals = orbit.iter().filter(|l| l.starts_with("val ")).count();
    assert_eq!(vals, 80, "{orbit:?}");
    assert!(
        orbit.iter().any(|l| l.starts_with("val TEMPO.BPM 0")),
        "{orbit:?}"
    );
    let (lines, m2, _) = since(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");
    ok(&mut f, "ed scene queue 2");
    let (lines, m3, _) = since(&mut f, m2);
    assert_eq!(
        lines,
        vec!["song stopped clock INT scene 1 queued 2 edited no".to_string()]
    );
    ok(&mut f, "ed set RING5.MUTE OFF");
    let (lines, _, _) = since(&mut f, m3);
    assert!(
        lines.iter().any(|l| l.starts_with("val RING5.MUTE 0 0 ")),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"slot 1 saved rev 1".to_string()),
        "{lines:?}"
    );
}

/// A MIDI clock taking the lead moves BPM's text: ORBIT whole again.
#[test]
fn the_clock_taking_the_lead_answers_orbit_whole() {
    let mut f = Fake::new();
    let (_, m, _) = since(&mut f, 0);
    f.ui.set_orbit_view(&OrbitView {
        source: chimera_core::orbit::clock::ClockSource::Midi,
        ..OrbitView::default()
    });
    let (lines, _, _) = since(&mut f, m);
    assert_eq!(lines[0], "");
    let orbit = orbit_lines(&lines);
    assert_eq!(
        orbit[1],
        "song stopped clock EXT scene 1 queued - edited no"
    );
    assert!(!lines.iter().any(|l| l.starts_with("part ")));
}

/// The worst ORBIT section fits the bound by itself, every value at its
/// longest.
#[test]
fn a_whole_orbit_section_fits_the_bound() {
    let mut f = Fake::new();
    for k in 1..=RINGS {
        for (id, v) in [
            ("STEPS", "#31"),
            ("HITS", "32"),
            ("CHANCE", "100"),
            ("MUTATE", "100"),
            ("GATE", "100"),
            ("VEL", "127"),
            ("OCTAVE", "8"),
            ("TARGET", "#5"),
        ] {
            let _ = ask(&mut f, &format!("ed set RING{k}.{id} {v}\n"));
        }
        apply(&mut f);
        let _ = ask(
            &mut f,
            &format!("ed pool {k} 127 127 127 127 127 127 127 127\n"),
        );
        apply(&mut f);
    }
    let (lines, _, more) = since(&mut f, 0);
    assert!(!more);
    let size: usize = orbit_lines(&lines).iter().map(|l| l.len() + 1).sum();
    println!("the ORBIT section whole is {size} B");
    assert!(size > 4000 && size < ANSWER_BYTES_MAX / 2, "{size}");
}

/// Every `param` of `texts`: its address, its max, and its last option.
fn maxes(texts: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for t in texts {
        for l in t.lines() {
            if let Some(rest) = l.strip_prefix("param ") {
                let addr = rest.split(' ').next().unwrap().to_string();
                let tail = rest.rsplit('"').next().unwrap();
                let max = tail.split_whitespace().nth(3).unwrap().to_string();
                out.push((addr, max));
            } else if let Some(rest) = l.strip_prefix("opt ") {
                let mut w = rest.split(' ');
                let addr = w.next().unwrap();
                let i = w.next().unwrap();
                if let Some(last) = out.iter_mut().rev().find(|(a, _)| a == addr) {
                    last.1 = format!("#{i}");
                }
            }
        }
    }
    out
}

/// A messy DRUM Part with every section full answers whole in pages,
/// each under the bound, every section said once and nothing lost; then
/// an empty diff (the design's "two pages").
#[test]
fn the_worst_whole_answer_pages_with_every_section() {
    use chimera_core::params::EngineType;
    let mut f = Fake::new();
    screen::init_part_engine(
        &mut f.ui,
        chimera_core::project::PartId::ALL[0],
        EngineType::Drum,
    );
    let drum: Vec<String> = (0..chimera_core::console::ed_pages(EngineType::Drum))
        .map(|n| ask(&mut f, &format!("ed describe DRUM {n}\n")))
        .collect();
    let mixer: Vec<String> = (0..ed_schema_pages(Schema::Mixer))
        .map(|n| ask(&mut f, &format!("ed describe MIXER {n}\n")))
        .collect();
    let orbit = pages(&mut f);
    let mut all = maxes(&drum);
    all.extend(maxes(&mixer));
    all.extend(maxes(&orbit));
    for (addr, max) in &all {
        if addr.ends_with(".RESERVE") || addr.ends_with(".MAX_VOICES") || addr.ends_with(".MODE") {
            continue;
        }
        let _ = ask(&mut f, &format!("ed set {addr} {max}\n"));
        apply(&mut f);
    }
    for k in 1..=RINGS {
        let _ = ask(
            &mut f,
            &format!("ed pool {k} 127 127 127 127 127 127 127 127\n"),
        );
        apply(&mut f);
    }
    let mut heads = Vec::new();
    let (mut n, mut answers) = (0, 0);
    loop {
        let got = ask(&mut f, &format!("ed since {n}\n"));
        assert!(got.len() <= ANSWER_BYTES_MAX, "{} B", got.len());
        println!("an answer of {} B", got.len());
        answers += 1;
        for l in got.lines() {
            if l.starts_with("part ")
                || l.starts_with("routes ")
                || l.starts_with("map ")
                || ["mixer", "orbit", "settings"].contains(&l)
            {
                heads.push(l.split(' ').next().unwrap().to_string());
            }
        }
        let last = got.lines().last().unwrap();
        let rest = last.strip_prefix("OK ").unwrap();
        match rest.strip_suffix(" more") {
            Some(m) => n = m.parse().unwrap(),
            None => {
                n = rest.parse().unwrap();
                break;
            }
        }
        assert!(answers < 4, "it ends");
    }
    assert!(answers <= 2, "{answers} answers");
    let mut want = vec!["part", "routes", "mixer", "orbit", "settings"];
    if cfg!(feature = "midi-map") {
        want.insert(2, "map");
    }
    assert_eq!(heads, want);
    let (lines, _, more) = since(&mut f, n);
    assert!(!more);
    assert!(lines.is_empty(), "{lines:?}");
}
