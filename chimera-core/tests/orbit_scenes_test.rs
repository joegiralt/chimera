//! Scenes (Task 16, spec § 4.1, § 6.3): SCN's SCENE queues one, which
//! lands on the loop; SEQ saves the playing one with its drift; SCN's SAVE
//! over another saved scene asks, and only its pill overwrites.

// ORBIT's UI: SEQ hold opens it only with the feature.
#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::block::Block;
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::channel::{SceneCmd, Seq as LogSeq};
use chimera_core::orbit::chords::harmony;
use chimera_core::orbit::clock::Bpm;
use chimera_core::orbit::engine::{OrbitEvent, OrbitKind};
use chimera_core::orbit::rng::Rng;
use chimera_core::orbit::runner::OrbitView;
use chimera_core::orbit::scene::{Scene, SceneIdx, SceneRing};
use chimera_core::part::{Held, LiveSnap};
use chimera_core::project::{Confirmed, PartId, SceneSlot};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::orbit::{ORBIT_ORB, ORBIT_SCN, OrbitCell};
use chimera_core::ui::region::{RegionData, RegionKind};
use chimera_hal::ButtonId::{B2, B6, Edit, Menu, Minus, Plus, Seq};
use chimera_hal::EncoderId::{A, B, E};
use chimera_theory::{Note, PitchClass, SnapTo, snap};
use common::orbit::{OrbitRig, secs};
use screen::{Input, ShellRig, feed, hold, tap};

const S: [SceneIdx; 8] = SceneIdx::ALL;

/// A fresh UI, inside ORBIT on ORB.
fn in_orbit() -> Box<UiState> {
    let mut ui = Box::new(crate::screen::stack_ui());
    hold(&mut ui, Seq);
    assert!(ui.location().orbit().is_some());
    ui
}

/// ORB → RING → CHRD → SCN.
fn to_scn(ui: &mut UiState) {
    for _ in 0..3 {
        feed(ui, Input::press(Plus));
    }
    assert_eq!(ui.page_def().id, ORBIT_SCN.id);
}

/// SCN's cells, in order: SCENE, SAVE, SWITCH.
#[test]
fn scn_holds_scene_save_switch() {
    use chimera_core::ui::block_def::SlotBinding;
    let cells: Vec<_> = ORBIT_SCN
        .params
        .iter()
        .filter_map(|p| match p.binding {
            SlotBinding::Orbit(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(
        cells,
        [OrbitCell::Scene, OrbitCell::Save, OrbitCell::Switch]
    );
    assert_eq!(ORBIT_SCN.id, 84);
}

fn toast(ui: &mut UiState) -> String {
    match ui.step_toast(0) {
        ToastStep::Show(l) => l.as_str().into(),
        s => panic!("no toast: {s:?}"),
    }
}

fn slot(ui: &UiState, n: usize) -> Scene {
    *ui.project().orbit().scene(S[n])
}

/// SCN's SAVE turned to slot `n` (one-based), then EDIT.
fn save_to(ui: &mut UiState, n: usize) {
    let from = ui.orbit().save_slot(ui.project().orbit()).get() as i8;
    feed(ui, Input::turn(B, n as i8 - 1 - from));
    tap(ui, Edit);
}

/// The live scene, edited so it differs from every slot.
fn edit_live(ui: &mut UiState, tonic: PitchClass) {
    ui.project_mut()
        .orbit_mut()
        .live_mut_for_test()
        .chords
        .tonic = tonic;
}

/// The question the open prompt asks.
fn question(ui: &UiState) -> String {
    ui.prompt_words_for_test()
        .map(|(q, _)| q.as_str().into())
        .unwrap_or_default()
}

/// Each ring's drift as a view shows it.
fn drift(v: &OrbitView) -> Vec<(u32, Vec<u8>, Vec<u8>)> {
    v.rings
        .iter()
        .map(|r| {
            (
                r.hits.bits(),
                r.pool.notes().iter().map(|n| n.get()).collect(),
                r.notes.iter().map(|n| n.get()).collect(),
            )
        })
        .collect()
}

/// Each ring's drift as a scene holds it.
fn scene_drift(s: &Scene) -> Vec<(u32, Vec<u8>, Vec<u8>)> {
    s.rings
        .iter()
        .map(|r| {
            (
                r.ring.hits_mask().bits(),
                r.ring.pool.notes().iter().map(|n| n.get()).collect(),
                r.ring.notes.iter().map(|n| n.get()).collect(),
            )
        })
        .collect()
}

/// Slot 2 as scene 1 in C, every ring MUTATE 100: written through the
/// test-only overwrite, `live` left as scene 1.
fn with_c_in_slot_2(rig: &mut OrbitRig) -> Scene {
    let a = *rig.project.orbit().live();
    let mut b = a;
    b.chords.tonic = PitchClass::C;
    for r in &mut b.rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    *rig.song().live_mut_for_test() = b;
    let c = Confirmed::<SceneSlot>::unasked(rig.project.orbit(), S[1]);
    rig.song().overwrite_scene(c).expect("nothing queued");
    *rig.song().live_mut_for_test() = a;
    b
}

/// A switch queued mid-loop: the old scene plays to the boundary, the new
/// one's step 0s fire at it, in its key, and its first loop is unmutated;
/// the song takes it as playing once the view reports it. Stopped, a
/// switch lands at once.
#[test]
fn scene_switch_lands_on_the_loop() {
    let mut rig = OrbitRig::new();
    let b = with_c_in_slot_2(&mut rig);
    rig.play(true);
    rig.run_loops(1);
    rig.run(500);
    let loops = rig.view().loops;
    rig.ui.queue(S[1], rig.project.orbit_mut());
    assert_eq!(
        rig.project.orbit().live().chords.tonic,
        PitchClass::A,
        "live waits"
    );
    rig.block();
    assert_eq!(rig.view().playing, S[0]);
    assert_eq!(rig.view().cue, Some(S[1]));
    assert_eq!(
        rig.view().chord.root(),
        PitchClass::A,
        "the old scene plays"
    );
    assert_eq!(rig.project.orbit().current(), S[0]);
    assert_eq!(rig.project.orbit().queued(), Some(S[1]));
    while rig.view().loops == loops {
        assert_eq!(rig.view().playing, S[0], "to the boundary");
        assert_ne!(rig.view().chord.root(), PitchClass::C);
        rig.block();
    }
    // The boundary block: the new scene's step 0s, snapped to C.
    assert_eq!((rig.view().playing, rig.view().cue), (S[1], None));
    assert_eq!(rig.view().chord.root(), PitchClass::C);
    let ons: Vec<_> = rig
        .sent
        .iter()
        .filter(|e| matches!(e.kind, OrbitKind::On(_)))
        .collect();
    assert!(!ons.is_empty(), "step 0s fire at the boundary");
    let bass = ons.iter().find(|e| e.ring.get() == 0).expect("ring 1's");
    assert!(
        [0, 3, 7].contains(&(bass.note.get() % 12)),
        "ring 1 snaps to Cm"
    );
    assert_eq!(rig.project.orbit().current(), S[1], "the song took it");
    assert_eq!(*rig.project.orbit().live(), b, "live is the slot's copy");
    assert_eq!(rig.project.orbit().queued(), None);
    // Its first loop plays as saved, MUTATE 100 or not.
    let landed = rig.view().loops;
    while rig.view().loops == landed {
        assert_eq!(drift(rig.view()), scene_drift(&b), "unmutated");
        rig.block();
    }
    assert_ne!(drift(rig.view()), scene_drift(&b), "then it drifts");

    // Stopped: at once.
    rig.play(false);
    rig.run(4);
    rig.ui.queue(S[0], rig.project.orbit_mut());
    rig.run(2);
    assert_eq!((rig.view().playing, rig.view().cue), (S[0], None));
    assert_eq!(rig.project.orbit().current(), S[0]);
}

/// A held key on a RE-SNAP Part re-snaps when a switch lands: a landed
/// scene is a harmony change like any other.
#[test]
fn a_landed_scene_resnaps_held_keys() {
    let mut rig = OrbitRig::on_init();
    for ring in &mut rig.song().live_mut_for_test().rings {
        ring.ring.mute = true;
    }
    let mix = &mut rig.project.edit_part(PartId::ALL[0]).mix;
    mix.snap = LiveSnap::Chord;
    mix.held = Held::Resnap;
    let b = with_c_in_slot_2(&mut rig);
    assert!(b.rings.iter().all(|r| r.ring.mute));
    rig.play(true);
    rig.run(4);
    rig.key(0, 66, true);
    rig.run(2);
    let am = snap(Note::new(66).unwrap(), SnapTo::Chord(rig.view().chord)).get();
    let held = |rig: &OrbitRig| -> Vec<u8> {
        rig.inst()
            .allocator()
            .slots()
            .iter()
            .filter(|s| s.held() && s.part() == Some(PartId::ALL[0]))
            .filter_map(|s| s.note().map(|n| n.get()))
            .collect()
    };
    assert_eq!(held(&rig), [am]);
    rig.ui.queue(S[1], rig.project.orbit_mut());
    let loops = rig.view().loops;
    while rig.view().loops == loops {
        rig.block();
    }
    rig.run(2);
    let cm = snap(Note::new(66).unwrap(), SnapTo::Chord(rig.view().chord)).get();
    assert_eq!(rig.view().chord.root(), PitchClass::C);
    assert_ne!(cm, am, "C's chord moves F# elsewhere");
    assert_eq!(held(&rig), [cm], "the held key re-snapped");
}

/// An edit while a switch waits goes to the playing scene, and is heard
/// at once; the switch then lands its slot as saved, and `live` becomes a
/// copy of it (interim ruling, ADR 0077).
#[test]
fn edits_while_queued_play_in_the_playing_scene() {
    let run = |edit: bool| {
        let mut rig = OrbitRig::new();
        with_c_in_slot_2(&mut rig);
        rig.play(true);
        rig.run_loops(1);
        rig.run(300);
        rig.ui.queue(S[1], rig.project.orbit_mut());
        rig.run(2);
        if edit {
            for r in &mut rig.song().live_mut_for_test().rings {
                r.ring.mute = true;
            }
        }
        let loops = rig.view().loops;
        let mut old = Vec::new();
        while rig.view().loops == loops {
            rig.block();
            if rig.view().loops == loops {
                old.push(rig.sent.clone());
            }
        }
        (rig, old)
    };
    let (control, old_control) = run(false);
    let (edited, old_edited) = run(true);
    let ons = |old: &[Vec<OrbitEvent>]| {
        old.iter()
            .flatten()
            .filter(|e| matches!(e.kind, OrbitKind::On(_)))
            .count()
    };
    assert!(ons(&old_control) > 0);
    assert_eq!(ons(&old_edited), 0, "the mute is heard before the boundary");
    for rig in [&control, &edited] {
        let song = rig.project.orbit();
        assert_eq!(rig.view().chord.root(), PitchClass::C, "the slot lands");
        assert_eq!(song.current(), S[1]);
        assert_eq!(*song.live(), *song.scene(S[1]), "live is the slot's copy");
    }
}

/// Queuing leaves `live` alone; turning SCENE back to the playing scene
/// before the boundary cancels the switch: nothing is lost, and the loop
/// goes on, mutating, with no relaunch.
#[test]
fn turning_back_cancels_the_switch() {
    let mut rig = OrbitRig::new();
    for r in &mut rig.song().live_mut_for_test().rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    with_c_in_slot_2(&mut rig);
    rig.play(true);
    rig.run_loops(1);
    rig.run(300);
    rig.song().live_mut_for_test().chords.tonic = PitchClass::E;
    rig.run(2);
    let before = *rig.project.orbit().live();
    rig.ui.queue(S[1], rig.project.orbit_mut());
    assert_eq!(
        *rig.project.orbit().live(),
        before,
        "queuing copies nothing"
    );
    rig.run(2);
    assert_eq!(rig.view().cue, Some(S[1]));
    rig.ui.queue(S[0], rig.project.orbit_mut());
    assert_eq!(rig.project.orbit().queued(), None, "cancelled");
    rig.run(2);
    assert_eq!((rig.view().playing, rig.view().cue), (S[0], None));
    let loops = rig.view().loops;
    let mut last = drift(rig.view());
    while rig.view().loops == loops {
        last = drift(rig.view());
        rig.block();
    }
    assert_eq!(rig.view().playing, S[0]);
    assert_eq!(rig.view().chord.root(), PitchClass::E, "the edit stands");
    assert_ne!(drift(rig.view()), last, "mutated on: no relaunch");
    let song = rig.project.orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));
    assert_eq!(song.live().chords.tonic, PitchClass::E);
    assert_eq!(
        scene_drift(song.live()),
        drift(rig.view()),
        "drift absorbed"
    );
}

/// A view from before a switch lands nothing: away to 2 and landed, back
/// to 1, a stale view playing 1 (one that hasn't applied the switch back)
/// leaves 1 queued; only a view that has applied it lands it. Landing on
/// `playing == queued` would not.
#[test]
fn a_stale_view_lands_nothing() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let view = |playing: usize, acked: LogSeq| OrbitView {
        playing: S[playing],
        acked,
        ..OrbitView::default()
    };
    feed(&mut ui, Input::turn(A, 1));
    let stale = ui.orbit().log().head();
    ui.set_orbit_view(&view(1, ui.orbit().log().head()));
    assert_eq!(ui.project().orbit().current(), S[1]);
    feed(&mut ui, Input::turn(A, -1));
    assert_eq!(ui.project().orbit().queued(), Some(S[0]));
    ui.set_orbit_view(&view(0, stale));
    assert_eq!(
        ui.project().orbit().queued(),
        Some(S[0]),
        "stale: not landed"
    );
    assert_eq!(ui.project().orbit().current(), S[1]);
    ui.set_orbit_view(&view(0, ui.orbit().log().head()));
    assert_eq!(ui.project().orbit().current(), S[0]);
}

/// Through the real triple buffers, playing: SCENE away and back before
/// the loop ends cancels the switch. The runner never leaves the playing
/// scene, and nothing lands: the song keeps it, and `live` as it was.
#[test]
fn a_switch_turned_back_lands_nothing() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running && v.loops >= 1);
    rig.idle(1);
    let live = *rig.ui.project().orbit().live();
    let (loops, from) = (rig.view().loops, rig.views.len());
    rig.frame(Input::turn(A, 1), 1);
    for _ in 0..20 {
        rig.idle(1);
    }
    assert_eq!(rig.view().cue, Some(S[1]), "the runner holds it");
    rig.frame(Input::turn(A, -1), 1);
    assert_eq!(rig.view().loops, loops, "back before the boundary");
    assert_eq!(rig.ui.project().orbit().queued(), None);
    // Taken at once as a cancel, not cued as a re-landing of 1: a `Keep`,
    // applied with nothing left to adopt.
    assert_eq!(rig.ui.orbit().awaiting(), Some(SceneCmd::Keep(S[0])));
    let head = rig.ui.orbit().log().head();
    assert_eq!((rig.view().cue, rig.view().acked), (None, head));
    while rig.view().loops < loops + 2 {
        rig.idle(1);
        let song = rig.ui.project().orbit();
        assert_eq!((song.current(), song.queued()), (S[0], None));
        // No relaunch at a boundary: nothing is cued there.
        assert_eq!(rig.view().cue, None);
    }
    assert!(rig.views[from..].iter().all(|v| v.playing == S[0]));
    assert_eq!(*rig.ui.project().orbit().live(), live, "nothing landed");
}

/// Through the real triple buffers, stopped: the runner adopts 2 between
/// the UI's view read and its keys, which turn SCENE back to 1 (a cancel,
/// as the UI sees it) and then on to 2 again, a newer command. The views
/// that follow read `playing` 2 with the newer commands unapplied: stale,
/// they land nothing. Only a view that has applied them lands 2.
#[test]
fn a_stale_view_through_the_buffers_lands_nothing() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let mut rig = ShellRig::new(ui);
    rig.frame(Input::turn(A, 1), 0);
    let r1 = rig.ui.orbit().log().head();
    rig.late(1, Input::turn(A, -1), 0);
    assert_eq!((rig.view().playing, rig.view().acked), (S[1], r1));
    assert_eq!(rig.ui.project().orbit().current(), S[0], "unseen");
    assert_eq!(rig.ui.project().orbit().queued(), None, "cancelled");
    rig.frame(Input::turn(A, 1), 0);
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[1]));
    assert_ne!(rig.ui.orbit().log().head(), r1);
    for _ in 0..3 {
        rig.idle(0);
        let song = rig.ui.project().orbit();
        assert_eq!((song.current(), song.queued()), (S[0], Some(S[1])), "stale");
    }
    rig.idle(1);
    rig.idle(0);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[1], None), "its own rev");
    assert_eq!(rig.view().acked, rig.ui.orbit().log().head());
}

/// The cancel race (ADR 0077): playing, the runner adopts 2 at the
/// boundary between the UI's view read and its turn back to 1. The UI
/// never saw 2 land, so the turn is a cancel to it, and its song never
/// leaves 1; the runner plays 2 for one loop, then takes 1 back at the
/// next boundary. Nothing is lost: `live` is 1's as it was.
#[test]
fn the_cancel_race_plays_the_switch_one_loop() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running && v.loops >= 1);
    // A loop's length in blocks, boundary to boundary.
    let at = |rig: &ShellRig| rig.views.len();
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let start = at(&rig);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let len = at(&rig) - start;
    let live = *rig.ui.project().orbit().live();

    let start = at(&rig);
    rig.frame(Input::turn(A, 1), 1);
    while at(&rig) < start + len - 10 {
        rig.idle(1);
    }
    assert_eq!(rig.view().playing, S[0]);
    rig.late(20, Input::turn(A, -1), 0);
    assert_eq!(rig.view().playing, S[1], "adopted, unseen");
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));

    let loops = rig.view().loops;
    let from = at(&rig);
    while rig.view().loops < loops + 1 {
        rig.idle(1);
        assert_eq!(rig.ui.project().orbit().current(), S[0]);
    }
    assert!(
        rig.views[from..at(&rig) - 1]
            .iter()
            .all(|v| v.playing == S[1]),
        "one loop of 2"
    );
    assert_eq!(rig.view().playing, S[0], "then 1 again");
    rig.idle(1);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));
    assert_eq!(*song.live(), live, "nothing lost");
}

/// The cancel race's second boundary, where the runner adopts `live`
/// back (a `Live` cue), falls between a frame's view read and its publish
/// of a NOTE edit. The edit reaches the runner, and no drift takes it back
/// out of `live` once the UI lands its turn back. (A HITS edit hides the
/// hole: a new fill re-lays the hits with or without the edit.)
#[test]
fn an_edit_across_the_live_adoption_is_kept() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    // Slot 2 loops as long as slot 1: scene 1 in C.
    let song = ui.project_mut().orbit_mut();
    let one = *song.live();
    song.live_mut_for_test().chords.tonic = PitchClass::C;
    let c = Confirmed::<SceneSlot>::unasked(song, S[1]);
    song.overwrite_scene(c).expect("nothing queued");
    *song.live_mut_for_test() = one;
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running && v.loops >= 1);
    let at = |rig: &ShellRig| rig.views.len();
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let start = at(&rig);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let len = at(&rig) - start;

    // The cancel race: 2 adopted unseen, SCENE back to 1 cues `live`.
    let start = at(&rig);
    rig.frame(Input::turn(A, 1), 1);
    while at(&rig) < start + len - 10 {
        rig.idle(1);
    }
    rig.late(20, Input::turn(A, -1), 0);
    assert_eq!((rig.view().playing, rig.view().cue), (S[1], None));
    rig.idle(1);
    assert_eq!(rig.view().cue, Some(S[0]), "`live` cued back");

    // A NOTE edit read before the boundary, published after it.
    let loops = rig.view().loops;
    let start = rig.views.iter().position(|v| v.loops == loops).unwrap();
    while at(&rig) < start + len - 10 {
        rig.idle(1);
    }
    assert_eq!(rig.view().cue, Some(S[0]), "still cued");
    let ring = &mut rig.ui.project_mut().orbit_mut().live_mut_for_test().rings[1];
    ring.set_chip(0, Note::new(50).unwrap());
    let edited = ring.ring.pool;
    rig.late(20, Input::default(), 0);
    assert_ne!(rig.view().loops, loops, "the boundary fell in the frame");
    assert_eq!(rig.view().playing, S[0]);
    for _ in 0..4 {
        rig.idle(1);
    }
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));
    assert_eq!(
        rig.view().rings[1].pool,
        edited,
        "the runner plays the edit"
    );
    assert_eq!(song.live().rings[1].ring.pool, edited, "`live` keeps it");
}

/// Through the real triple buffers, playing: in the cancel race's window
/// (2 adopted unseen, SCENE back to 1), SCENE on to 2 again queues 2 under
/// a new rev. The views still read 2 under the old rev for the rest of
/// that loop, and land nothing; at the next boundary the runner adopts the
/// new rev, 2 again from its slot, and its view lands it (ADR 0077).
#[test]
fn requeued_in_the_cancel_window_lands_on_the_next_loop() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running && v.loops >= 1);
    let at = |rig: &ShellRig| rig.views.len();
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let start = at(&rig);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let len = at(&rig) - start;

    let start = at(&rig);
    rig.frame(Input::turn(A, 1), 1);
    let r1 = rig.ui.orbit().log().head();
    while at(&rig) < start + len - 10 {
        rig.idle(1);
    }
    rig.late(20, Input::turn(A, -1), 0);
    assert_eq!((rig.view().playing, rig.view().acked), (S[1], r1));
    rig.frame(Input::turn(A, 1), 1);
    let r3 = rig.ui.orbit().log().head();
    assert_ne!(r3, r1);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[0], Some(S[1])));

    let loops = rig.view().loops;
    while rig.view().loops == loops {
        let v = rig.view();
        assert_eq!((v.playing, v.acked, v.cue), (S[1], r3, Some(S[1])));
        let song = rig.ui.project().orbit();
        assert_eq!((song.current(), song.queued()), (S[0], Some(S[1])), "stale");
        rig.idle(1);
    }
    let v = rig.view();
    assert_eq!((v.playing, v.acked, v.cue), (S[1], r3, None), "adopted");
    rig.idle(1);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[1], None));
    assert_eq!(*song.live(), *song.scene(S[1]), "landed from its slot");
}

/// SAVE defaults to the playing scene again after a switch lands, and
/// after a load.
#[test]
fn save_follows_the_playing_scene() {
    use chimera_core::ui::orbit::OrbitUi;
    let mut ui = in_orbit();
    to_scn(&mut ui);
    feed(&mut ui, Input::turn(B, 4));
    assert_eq!(ui.orbit().save_slot(ui.project().orbit()), S[4]);
    feed(&mut ui, Input::turn(A, 2));
    let v = OrbitView {
        playing: S[2],
        acked: ui.orbit().log().head(),
        ..OrbitView::default()
    };
    ui.set_orbit_view(&v);
    assert_eq!(ui.orbit().save_slot(ui.project().orbit()), S[2]);

    let mut o = OrbitUi::new();
    o.save = Some(S[4]);
    let mut song = chimera_core::orbit::scene::OrbitSong::NEW;
    o.loaded(1, &mut song);
    assert_eq!(o.save, None);
}

/// 20 loops of MUTATE, SCN's SAVE on the playing slot, a switch away and
/// back: the slot plays the drift as it was saved.
#[test]
fn save_captures_the_drift() {
    let mut ui = in_orbit();
    for r in &mut ui.project_mut().orbit_mut().live_mut_for_test().rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    let init = scene_drift(ui.project().orbit().live());
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.loops >= 20);
    rig.idle(1);
    to_scn(&mut rig.ui);
    save_to(&mut rig.ui, 1);
    assert_eq!(toast(&mut rig.ui), "SCENE 1 SAVED");
    let saved = slot(&rig.ui, 0);
    assert_ne!(scene_drift(&saved), init, "20 loops drift");
    assert_eq!(saved.chords, rig.ui.project().orbit().live().chords);

    to_scn(&mut rig.ui);
    rig.frame(Input::turn(A, 1), 1);
    rig.until(|v| v.playing == S[1] && v.cue.is_none());
    rig.frame(Input::turn(A, -1), 1);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let v = rig.view();
    assert_eq!((v.playing, v.cue), (S[0], None));
    assert_eq!(drift(&v), scene_drift(&saved), "the slot plays its drift");
}

/// While a switch is queued, SCN's SAVE is refused, and says so:
/// `live` is the queued scene's.
#[test]
fn save_while_queued_is_refused() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let before: Vec<Scene> = (0..8).map(|n| slot(&ui, n)).collect();
    feed(&mut ui, Input::turn(A, 1));
    assert_eq!(ui.project().orbit().queued(), Some(S[1]));
    edit_live(&mut ui, PitchClass::E);
    save_to(&mut ui, 1);
    assert_eq!(toast(&mut ui), "SWITCH PENDING");
    save_to(&mut ui, 6);
    assert_eq!(toast(&mut ui), "SWITCH PENDING");
    assert!(!ui.prompt_open());
    let after: Vec<Scene> = (0..8).map(|n| slot(&ui, n)).collect();
    assert_eq!(after, before);
}

/// SAVE → 5 over a saved slot 5 asks; CANCEL and MENU leave it, SAVE OVER
/// writes it; B2 drops the prompt and opens ring 2. Over a neutral slot,
/// or the playing one, it saves at once.
#[test]
fn save_over_another_scene_asks() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    // Neutral: at once.
    edit_live(&mut ui, PitchClass::E);
    save_to(&mut ui, 5);
    assert!(!ui.prompt_open());
    assert_eq!(toast(&mut ui), "SCENE 5 SAVED");
    let five = slot(&ui, 4);
    assert_eq!(five, *ui.project().orbit().live());
    assert_ne!(five, Scene::neutral());

    edit_live(&mut ui, PitchClass::F);
    for cancel in [Menu, Seq] {
        save_to(&mut ui, 5);
        assert_eq!(question(&ui), "SAVE OVER SCENE 5?");
        tap(&mut ui, cancel);
        assert!(!ui.prompt_open());
        assert_eq!(slot(&ui, 4), five, "{cancel:?} leaves it");
    }
    // B2 drops it, and is a ring key.
    save_to(&mut ui, 5);
    assert!(ui.prompt_open());
    tap(&mut ui, B2);
    assert!(!ui.prompt_open());
    assert_eq!(ui.location().orbit().unwrap().ring().get(), 1);
    assert_eq!(slot(&ui, 4), five);
    to_scn(&mut ui);

    save_to(&mut ui, 5);
    feed(&mut ui, Input::turn(A, -9));
    tap(&mut ui, Edit); // SAVE OVER
    assert!(!ui.prompt_open());
    assert_eq!(slot(&ui, 4), *ui.project().orbit().live());
    assert_eq!(slot(&ui, 4).chords.tonic, PitchClass::F);
    assert_eq!(toast(&mut ui), "SCENE 5 SAVED");

    // Neutral slot 6: at once.
    save_to(&mut ui, 6);
    assert!(!ui.prompt_open());
    assert_eq!(slot(&ui, 5), *ui.project().orbit().live());
    // The playing slot: at once.
    edit_live(&mut ui, PitchClass::G);
    save_to(&mut ui, 1);
    assert!(!ui.prompt_open());
    assert_eq!(slot(&ui, 0).chords.tonic, PitchClass::G);
}

/// SAVE OVER on slot 5 refused when slot 5 was written while the prompt
/// was open: the slot keeps that write.
#[test]
fn a_changed_slot_refuses_the_prompt() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    edit_live(&mut ui, PitchClass::E);
    save_to(&mut ui, 5);
    let _ = ui.step_toast(0);
    edit_live(&mut ui, PitchClass::F);
    save_to(&mut ui, 5);
    assert!(ui.prompt_open());
    // Meanwhile slot 5 is written.
    edit_live(&mut ui, PitchClass::G);
    let c = Confirmed::<SceneSlot>::unasked(ui.project().orbit(), S[4]);
    ui.project_mut()
        .orbit_mut()
        .overwrite_scene(c)
        .expect("nothing queued");
    let meanwhile = slot(&ui, 4);
    edit_live(&mut ui, PitchClass::B);
    feed(&mut ui, Input::turn(A, -9));
    tap(&mut ui, Edit); // SAVE OVER
    assert!(!ui.prompt_open());
    assert_eq!(slot(&ui, 4), meanwhile, "refused");
    assert_eq!(toast(&mut ui), "SCENE CHANGED");
}

/// The header reads the playing scene, on CHRD and SCN, and redraws when a
/// switch lands: its region key holds the scene.
#[test]
fn the_header_reads_the_playing_scene() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let header = |ui: &UiState| ui.drawn_key(RegionKind::Header);
    let mut fb = screen::Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    ui.render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    let before = header(&ui);
    feed(&mut ui, Input::turn(A, 2));
    // Stopped: the runner adopts at once; its view lands it.
    let v = OrbitView {
        playing: S[2],
        cue: None,
        acked: ui.orbit().log().head(),
        ..OrbitView::default()
    };
    ui.set_orbit_view(&v);
    assert_eq!(ui.project().orbit().current(), S[2]);
    ui.render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    let after = header(&ui);
    assert_ne!(after, before, "the header redraws");
    assert!(matches!(after, Some(RegionData::Header { names: 3, .. })));
}

/// SEQ saves nothing on any ORBIT page (ADR 0081): scene saving is SCN's
/// SAVE and SETTINGS › ORBIT's.
#[test]
fn seq_saves_nothing_on_any_orbit_page() {
    use chimera_core::orbit::chords::ScaleKind;
    let mut ui = in_orbit();
    ui.project_mut()
        .orbit_mut()
        .live_mut_for_test()
        .chords
        .scale = ScaleKind::Custom;
    // SCALE in focus on ORB, CUSTOM its top: EDIT there opens CUSTOM.
    feed(&mut ui, Input::turn(chimera_hal::EncoderId::C, 1));
    let mut seen = Vec::new();
    for _ in 0..4 {
        for _ in 0..3 {
            let at = ui.location();
            let saved = slot(&ui, 0).chords.tonic;
            edit_live(
                &mut ui,
                *PitchClass::ALL.iter().find(|&&t| t != saved).unwrap(),
            );
            tap(&mut ui, Seq);
            assert_ne!(slot(&ui, 0), *ui.project().orbit().live(), "not saved");
            assert!(matches!(ui.step_toast(0), ToastStep::Idle), "no toast");
            // SEQ is up a sub-page: back to where it was pressed.
            ui.go_for_test(at);
            seen.push(ui.page_def().id);
            // ORB's and RING's sub-pages; EDIT does nothing elsewhere.
            if ORBIT_SCN.id != ui.page_def().id {
                tap(&mut ui, Edit);
            }
        }
        feed(&mut ui, Input::press(Plus));
    }
    use chimera_core::ui::orbit::{
        ORBIT_CHRD, ORBIT_CUSTOM, ORBIT_NOTES, ORBIT_ORB, ORBIT_RHYTHM, ORBIT_VOICE,
    };
    for def in [
        &ORBIT_ORB,
        &ORBIT_RHYTHM,
        &ORBIT_VOICE,
        &ORBIT_NOTES,
        &ORBIT_CHRD,
        &ORBIT_CUSTOM,
        &ORBIT_SCN,
    ] {
        assert!(seen.contains(&def.id), "{}", def.short);
    }
}

/// NEW plays the owner's pick (V4, ADR 0073) as scene 1; the other seven
/// slots are neutral, and nothing waits.
#[test]
fn new_plays_v4_as_scene_1() {
    use chimera_core::orbit::scene::OrbitSong;
    let song = OrbitSong::NEW;
    assert_eq!(*song.scene(S[0]), Scene::init());
    assert_eq!(*song.live(), Scene::init());
    assert!(S[1..].iter().all(|&n| *song.scene(n) == Scene::neutral()));
    assert_eq!((song.current(), song.queued()), (S[0], None));
    let ui = crate::screen::stack_ui();
    assert_eq!(*ui.project().orbit(), song);
}

/// CLEAR on the playing slot adopted by the runner, unseen, then SCENE
/// away and back: the CLEAR stays due (ADR 0083; it used to cancel, and
/// left `live` on its old notes over a neutral slot). It lands, `live`
/// is the cleared slot, and the runner plays it.
#[test]
fn a_clear_adopted_then_turned_back_still_lands() {
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run_loops(1);
    let yes = Confirmed::<SceneSlot>::unasked(rig.project.orbit(), S[0]);
    rig.song().clear_scene(yes).expect("nothing queued");
    rig.ui.issue_scene(rig.project.orbit());
    rig.card_read(1);
    while !(rig.view().acked == rig.ui.log().head() && rig.view().cue.is_none()) {
        rig.card_read(1);
    }
    rig.ui.queue(S[1], rig.project.orbit_mut());
    rig.ui.queue(S[0], rig.project.orbit_mut());
    let song = rig.project.orbit();
    assert_eq!(
        (song.current(), song.queued()),
        (S[0], Some(S[0])),
        "still due"
    );
    rig.run(secs(10.0));
    let song = rig.project.orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));
    assert_eq!(*song.live(), Scene::neutral(), "the cleared slot");
    assert_eq!(*song.scene(S[0]), Scene::neutral());
    assert_eq!(drift(rig.view()), scene_drift(&Scene::neutral()), "played");
}

/// `s` with `base`'s hits, notes and pools: `s` apart from its drift.
fn sans_drift(s: &Scene, base: &Scene) -> Scene {
    let mut s = *s;
    for (r, b) in s.rings.iter_mut().zip(&base.rings) {
        r.ring = r.ring.with_mask(b.ring.hits_mask()).unwrap_or(r.ring);
        r.ring.notes = b.ring.notes;
        r.ring.pool = b.ring.pool;
    }
    s
}

/// A frame's blocks in the property tests: a few, so the runner keeps up.
fn some_blocks(rng: &mut Rng) -> usize {
    rng.below(4) as usize
}

/// SETTINGS › ORBIT's `row` (CLEAR 1, RELOAD 3) answered yes, if it asks,
/// through the rig's frames; then back into ORBIT on ORB.
fn settings_scene_act(rig: &mut ShellRig, row: usize) {
    while rig.ui.location().orbit().is_some() {
        tap(&mut rig.ui, Menu);
    }
    screen::to_leaf(&mut rig.ui, &["ORBIT"]);
    screen::plus(&mut rig.ui, row);
    rig.tap(Edit);
    if rig.ui.prompt_open() {
        rig.frame(Input::turn(A, -9), 1);
        rig.tap(Edit);
    }
    let _ = rig.ui.step_toast(0);
    while rig.ui.in_settings() {
        tap(&mut rig.ui, Menu);
    }
    hold(&mut rig.ui, Seq);
    assert_eq!(rig.ui.page_def().id, ORBIT_ORB.id);
}

/// § 4.3: random SCENE, KEY, CLEAR, RELOAD and B6, at random
/// frame-to-block ratios and with the audio interrupting frames, every
/// edit through the page. With no drift (MUTATE 0), `live` changes only by
/// the UI's own acts; after two quiet loops the song plays what the runner
/// plays, nothing waits, the runner is in sync (a knob reaches it),
/// `live`'s rings are the runner's, and the playing slot is `live` but for
/// drift (ADR 0083).
#[test]
fn scenes_converge_after_any_interleaving() {
    const STEPS: usize = 300;
    for seed in 1..=4u32 {
        let mut ui = in_orbit();
        {
            // Slot 2: scene 1 in C, ring 1 of 8 steps, saved by SCN's
            // SAVE's own call; then scene 1's edits undone.
            let song = ui.project_mut().orbit_mut();
            song.bpm = Bpm::clamped(Bpm::MAX);
            let a = *song.live();
            song.edit(&mut |s| {
                s.chords.tonic = PitchClass::C;
                s.rings[0].set(SceneRing::STEPS, 7.0);
            });
            song.save_to(S[1]).expect("nothing queued");
            song.edit(&mut |s| *s = a);
        }
        let mut rig = ShellRig::new(ui);
        let mut rng = Rng::new(seed);
        for step in 0..STEPS {
            let (first, then) = (some_blocks(&mut rng), some_blocks(&mut rng));
            match rng.below(10) {
                0..=4 => {
                    let d = [-2, -1, 1, 2][rng.below(4) as usize];
                    // SCENE, or KEY: an edit through the page.
                    let enc = if rng.below(3) == 0 { B } else { E };
                    rig.late(first, Input::turn(enc, d), then);
                }
                5 => {
                    rig.late(first, Input::press(B6), then);
                    rig.frame(Input::release(B6), some_blocks(&mut rng));
                }
                6 => settings_scene_act(&mut rig, 1),
                7 => settings_scene_act(&mut rig, 3),
                _ => {
                    let song = rig.ui.project().orbit();
                    let (live, at) = (*song.live(), (song.current(), song.queued()));
                    rig.late(first, Input::default(), then);
                    let song = rig.ui.project().orbit();
                    if (song.current(), song.queued()) == at {
                        assert_eq!(
                            *song.live(),
                            live,
                            "seed {seed} step {step}: no act, no change"
                        );
                    }
                }
            }
        }
        // Two quiet loops, or a few frames stopped.
        if rig.view().running {
            let loops = rig.view().loops;
            while rig.view().loops < loops + 2 {
                rig.idle(25);
            }
        }
        for _ in 0..4 {
            rig.idle(1);
        }
        let v = rig.view();
        let song = rig.ui.project().orbit();
        assert_eq!(song.current(), v.playing, "seed {seed}");
        assert_eq!((song.queued(), v.cue), (None, None), "seed {seed}");
        assert_eq!(rig.ui.orbit().awaiting(), None, "seed {seed}");
        assert_eq!(scene_drift(song.live()), drift(&v), "seed {seed}");
        let slot = song.scene(song.current());
        assert_eq!(sans_drift(song.live(), slot), *slot, "seed {seed}");
        // In sync: a knob, KEY, reaches the runner.
        let d = if song.live().chords.tonic == PitchClass::B {
            -1
        } else {
            1
        };
        rig.frame(Input::turn(B, d), 1);
        rig.idle(1);
        rig.idle(1);
        let v = rig.view();
        let chords = rig.ui.project().orbit().live().chords;
        assert_eq!(
            v.chord,
            harmony(&chords, v.seg as usize).chord,
            "seed {seed}"
        );
    }
}

/// Review Focus 1: the loop boundary falls between the UI's view read and
/// its SCENE turn, so the switch waits a loop on a drift the UI has not
/// absorbed. RHYTHM and ORB draw the runner's hits all the while, never
/// the scene's bare ones.
#[test]
fn rhythm_draws_the_drift_while_a_switch_waits() {
    use chimera_core::ui::orbit::draw::OrbitFrame;
    let mut ui = in_orbit();
    for r in &mut ui.project_mut().orbit_mut().live_mut_for_test().rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running && v.loops >= 2);
    let at = |rig: &ShellRig| rig.views.len();
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let start = at(&rig);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let len = at(&rig) - start;
    let start = at(&rig);
    while at(&rig) < start + len - 10 {
        rig.idle(1);
    }
    let loops = rig.view().loops;
    rig.late(20, Input::turn(E, 1), 1);
    assert_ne!(rig.view().loops, loops, "the boundary fell in the frame");
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[1]));
    let loops = rig.view().loops;
    let mut drifted = false;
    while rig.view().loops == loops {
        let (song, o) = (rig.ui.project().orbit(), rig.ui.orbit());
        let f = OrbitFrame::new(song, o);
        for r in chimera_core::orbit::RingIdx::ALL {
            let seen = o.status.rings[r.get() as usize].hits;
            assert_eq!(f.hits(r), seen, "ring {} draws the drift", r.get() + 1);
            drifted |= song.live().rings[r.get() as usize].ring.hits_mask() != seen;
        }
        rig.idle(1);
    }
    assert!(drifted, "the boundary moved a hit `live` hasn't taken");
}

/// Review Focus 3: SCENE spun over seven slots and back while the runner
/// stalls, more commands than the log holds. Only the last slot lands,
/// and none between ever plays.
#[test]
fn a_scene_spin_during_a_stall_lands_only_the_last() {
    let mut rig = ShellRig::new(in_orbit());
    rig.b6();
    rig.until(|v| v.running && v.loops >= 1);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let from = rig.views.len();
    for d in [1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1] {
        rig.frame(Input::turn(E, d), 0);
    }
    for _ in 0..8 {
        rig.idle(0);
    }
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[2]));
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    rig.idle(1);
    assert!(
        rig.views[from..]
            .iter()
            .all(|v| v.playing == S[0] || v.playing == S[2]),
        "no slot between plays"
    );
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[2], None));
}

/// Review Focus 4: stopped under SYNC EXT, SCENE → 2 is published and a
/// MIDI Start reaches the block that reads it: ORBIT starts in scene 2,
/// its slot whole.
#[test]
fn a_daw_start_lands_the_waiting_switch() {
    use chimera_core::note_queue::RealTime;
    let mut rig = ShellRig::new(in_orbit());
    rig.idle(2);
    assert!(!rig.view().running);
    assert_eq!(
        rig.ui.midi().sync,
        chimera_core::orbit::clock::SyncMode::Ext
    );
    rig.frame(Input::turn(E, 1), 0);
    let now = rig.runner.now();
    rig.runner.realtime(0, RealTime::Start, now);
    rig.blocks(1);
    let v = rig.view();
    assert!(v.running, "the DAW's Start plays");
    assert_eq!(v.playing, S[1]);
    assert_eq!(drift(&v), scene_drift(&slot(&rig.ui, 1)), "slot 2 whole");
}

/// The newest edit of one ring's rhythm or notes, as `live` holds it.
struct Pending {
    ring: usize,
    rhythm: bool,
    want: SceneRing,
    /// The log's head once it was flushed.
    target: LogSeq,
    /// The first view after it.
    from: usize,
}

impl Pending {
    fn holds(&self, r: &SceneRing) -> bool {
        match self.rhythm {
            true => r.ring.hits_mask() == self.want.ring.hits_mask(),
            false => r.ring.pool == self.want.ring.pool && r.ring.notes == self.want.ring.notes,
        }
    }

    fn played(&self, v: &OrbitView) -> bool {
        let r = &v.rings[self.ring];
        match self.rhythm {
            true => r.hits == self.want.ring.hits_mask(),
            false => r.pool == self.want.ring.pool && r.notes == self.want.ring.notes,
        }
    }

    /// The first view at or past its seq.
    fn acked(&self, views: &[OrbitView]) -> Option<usize> {
        (self.from..views.len()).find(|&i| views[i].acked.since(self.target) < 128)
    }
}

/// § 4.3: MUTATE 100 and random NOTE, STEPS and CLEAR edits, the audio
/// interrupting frames. The block that applies an edit plays it (unless a
/// boundary moved it there), and until the UI sees it applied, no drift
/// overwrites it in `live`.
#[test]
fn edits_never_lost_to_drift() {
    const STEPS: usize = 1500;
    for seed in 1..=4u32 {
        let mut ui = in_orbit();
        {
            let song = ui.project_mut().orbit_mut();
            song.bpm = Bpm::clamped(Bpm::MAX);
            song.live_mut_for_test().chords.bars = chimera_core::orbit::chords::Bars::ONE;
            for r in &mut song.live_mut_for_test().rings {
                r.set(SceneRing::MUTATE, 100.0);
            }
        }
        let mut rig = ShellRig::new(ui);
        rig.b6();
        let mut rng = Rng::new(seed);
        let mut edits: Vec<Pending> = Vec::new();
        let mut checked = 0;
        for step in 0..STEPS {
            let edited = (rng.below(3) == 0).then(|| {
                let r = rng.below(5) as usize;
                let song = rig.ui.project_mut().orbit_mut();
                let kind = rng.below(4);
                match kind {
                    0 => song.live_mut_for_test().rings[r]
                        .set(SceneRing::STEPS, rng.below(32) as f32),
                    1 => {
                        let n = Note::new(36 + rng.below(60) as u8).unwrap();
                        song.live_mut_for_test().rings[r].set_chip(0, n);
                    }
                    2 => song.live_mut_for_test().rings[r].clear(),
                    _ => song
                        .live_mut_for_test()
                        .scatter(RingIdx::ALL[r], rng.below(256) as u8),
                }
                (r, kind == 0, song.live().rings[r])
            });
            let seen = rig.views.len();
            let (first, then) = (rng.below(3) as usize, 1 + rng.below(12) as usize);
            rig.late(first, Input::default(), then);
            if let Some((ring, rhythm, want)) = edited {
                edits.retain(|e| (e.ring, e.rhythm) != (ring, rhythm));
                edits.push(Pending {
                    ring,
                    rhythm,
                    want,
                    target: rig.ui.orbit().log().head(),
                    from: seen,
                });
            }
            let live = *rig.ui.project().orbit().live();
            edits.retain(|e| {
                let acked = e.acked(&rig.views);
                // The UI read the newest view at its frame's start.
                let seen_by_ui = acked.is_some_and(|i| i < seen);
                if !seen_by_ui {
                    assert!(
                        e.holds(&live.rings[e.ring]),
                        "seed {seed} step {step}: ring {} lost its edit",
                        e.ring + 1
                    );
                }
                if let Some(i) = acked
                    && i > 0
                    && rig.views[i].loops == rig.views[i - 1].loops
                {
                    assert!(
                        e.played(&rig.views[i]),
                        "seed {seed} step {step}: ring {} applied without its edit",
                        e.ring + 1
                    );
                    checked += 1;
                }
                !seen_by_ui
            });
        }
        assert!(checked > 100, "seed {seed}: {checked} edits checked");
    }
}

/// § 4.3: random acts (LEARN, keys, NOTE edits, B6) and loads of NEW, with
/// blocks the UI never sees before a load (its card read). After each
/// load: no capture or drift from before it reaches `live`, ORBIT is
/// stopped, and once a view from the load arrives both sides are in sync
/// (nothing awaits; a knob reaches the runner).
#[test]
fn loads_void_everything_before() {
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    use chimera_core::orbit::chords::Bars;
    use chimera_core::{MidiChannel, MidiNote, Velocity};
    use chimera_hal::EncoderId::{D, F};
    let key = |rig: &mut ShellRig, n: u8, on: bool| {
        rig.runner.live(NoteEvent {
            channel: MidiChannel::new(1).unwrap(),
            note: MidiNote::new(n).unwrap(),
            kind: if on {
                NoteKind::On(Velocity::DEFAULT)
            } else {
                NoteKind::Off
            },
        });
    };
    for seed in 1..=4u32 {
        let mut ui = Box::new(crate::screen::stack_ui());
        screen::to_ring(&mut ui, 2);
        tap(&mut ui, Edit);
        tap(&mut ui, Edit);
        let mut rig = ShellRig::new(ui);
        let mut rng = Rng::new(seed);
        for round in 0..12u32 {
            {
                let song = rig.ui.project_mut().orbit_mut();
                song.bpm = Bpm::clamped(Bpm::MAX);
                song.live_mut_for_test().chords.bars = Bars::ONE;
                for r in &mut song.live_mut_for_test().rings {
                    r.set(SceneRing::MUTATE, 100.0);
                }
            }
            if !rig.view().running {
                rig.b6();
            }
            let mut held = Vec::new();
            for _ in 0..rng.below(60) {
                let blocks = some_blocks(&mut rng);
                match rng.below(6) {
                    0 => rig.frame(Input::turn(D, 1), blocks),
                    1 => rig.frame(Input::turn(F, 1 + rng.below(3) as i8), blocks),
                    2 => {
                        let n = 48 + rng.below(24) as u8;
                        key(&mut rig, n, true);
                        held.push(n);
                        rig.idle(blocks);
                    }
                    3 => {
                        for n in held.drain(..) {
                            key(&mut rig, n, false);
                        }
                        rig.idle(blocks);
                    }
                    _ => rig.idle(blocks),
                }
            }
            // The card read: blocks the UI doesn't see, keys ending in it.
            rig.blocks(rng.below(700) as usize);
            if rng.below(2) == 0 {
                for n in held.drain(..) {
                    key(&mut rig, n, false);
                }
                rig.blocks(rng.below(4) as usize);
            }
            rig.load_new();
            for n in held.drain(..) {
                key(&mut rig, n, false);
            }
            let new = Scene::init();
            let views = rig.views.len();
            for _ in 0..2 + rng.below(20) {
                rig.late(
                    some_blocks(&mut rng),
                    Input::default(),
                    some_blocks(&mut rng),
                );
                assert_eq!(
                    *rig.ui.project().orbit().live(),
                    new,
                    "seed {seed} round {round}: nothing from before the load"
                );
                // Once the runner has taken the load: one load a round.
                let v = rig.view();
                if v.epoch == round + 1 {
                    assert!(!v.running, "seed {seed} round {round}: stopped");
                }
            }
            while rig.views.len() == views {
                rig.idle(1);
            }
            rig.idle(1);
            assert_eq!(rig.ui.orbit().awaiting(), None, "seed {seed} round {round}");
            assert!(rig.ui.orbit().idle(), "seed {seed} round {round}");
            assert_eq!(
                drift(&rig.view()),
                scene_drift(&new),
                "seed {seed} round {round}"
            );
            rig.ui
                .project_mut()
                .orbit_mut()
                .live_mut_for_test()
                .chords
                .tonic = PitchClass::E;
            rig.idle(1);
            rig.idle(1);
            let v = rig.view();
            let chords = rig.ui.project().orbit().live().chords;
            assert_eq!(
                v.chord,
                harmony(&chords, v.seg as usize).chord,
                "seed {seed} round {round}: a knob reaches the runner"
            );
        }
    }
}

/// SCENE turned by `by` on ORB, playing: frames until the runner plays
/// the new scene and the song has landed it.
fn switch(rig: &mut ShellRig, by: i8) {
    let to = S[(rig.ui.project().orbit().current().get() as i8 + by) as usize];
    rig.frame(Input::turn(E, by), 1);
    rig.until(|v| v.playing == to && v.cue.is_none());
    rig.idle(1);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (to, None));
}

/// The owner's report (ADR 0083): KEY and SCALE turned on scene 1 (ring
/// 2's harmony with every other ring's), a switch to scene 2 and back
/// while playing: scene 1 comes back with its edit, and plays it.
#[test]
fn a_scene_left_and_returned_to_keeps_its_edits() {
    use chimera_core::orbit::chords::ScaleKind;
    let ui = in_orbit();
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running);
    // A minor → C major: B is KEY, C SCALE.
    rig.frame(Input::turn(B, -9), 1);
    rig.frame(Input::turn(chimera_hal::EncoderId::C, -1), 1);
    let edited = rig.ui.project().orbit().live().chords;
    assert_eq!(
        (edited.tonic, edited.scale),
        (PitchClass::C, ScaleKind::Major)
    );
    assert_eq!(slot(&rig.ui, 0).chords, edited, "the slot has it");

    switch(&mut rig, 1);
    assert_eq!(*rig.ui.project().orbit().live(), Scene::neutral());
    switch(&mut rig, -1);
    let song = rig.ui.project().orbit();
    assert_eq!(song.live().chords, edited, "scene 1 keeps its edit");
    let v = rig.view();
    assert_eq!(
        v.chord,
        harmony(&edited, v.seg as usize).chord,
        "and plays it"
    );
}

/// MUTATE drifts scene 1 (MUTATE itself an edit, so kept); away and back,
/// it restarts from its edited base, not the drifted notes.
#[test]
fn a_returning_scene_drops_its_drift() {
    let mut ui = in_orbit();
    feed(&mut ui, Input::press(Plus));
    assert_eq!(ui.page_def().id, chimera_core::ui::orbit::ORBIT_RHYTHM.id);
    // E is ring 1's MUTATE.
    feed(&mut ui, Input::turn(E, 100));
    feed(&mut ui, Input::press(Minus));
    assert_eq!(ui.page_def().id, ORBIT_ORB.id);
    let base = slot(&ui, 0);
    assert_eq!(base.rings[0].ring.mutate.get(), 100, "the slot has MUTATE");
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.loops >= 20);
    rig.idle(1);
    let song = rig.ui.project().orbit();
    assert_ne!(scene_drift(song.live()), scene_drift(&base), "drifted");
    assert_eq!(*song.scene(S[0]), base, "the slot never drifts");

    switch(&mut rig, 1);
    switch(&mut rig, -1);
    assert_eq!(*rig.ui.project().orbit().live(), base, "the edited base");
    assert_eq!(
        drift(&rig.view()),
        scene_drift(&base),
        "plays it, drift restarted"
    );
}

/// SCN's SAVE is the deliberate act that locks the drift in: away and
/// back, the scene plays the drift as saved.
#[test]
fn a_save_locks_the_drift_in() {
    let mut ui = in_orbit();
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::turn(E, 100));
    feed(&mut ui, Input::press(Minus));
    let base = slot(&ui, 0);
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.loops >= 20);
    rig.idle(1);
    to_scn(&mut rig.ui);
    save_to(&mut rig.ui, 1);
    assert_eq!(toast(&mut rig.ui), "SCENE 1 SAVED");
    let locked = slot(&rig.ui, 0);
    assert_ne!(scene_drift(&locked), scene_drift(&base), "the drift saved");
    // SCN's A is SCENE.
    rig.frame(Input::turn(A, 1), 1);
    rig.until(|v| v.playing == S[1] && v.cue.is_none());
    rig.idle(1);
    rig.frame(Input::turn(A, -1), 1);
    rig.until(|v| v.playing == S[0] && v.cue.is_none());
    rig.idle(1);
    assert_eq!(*rig.ui.project().orbit().live(), locked);
    assert_eq!(drift(&rig.view()), scene_drift(&locked));
}

/// The cells' edits and MIX + B*n*'s MUTE reach the slot as `live` takes
/// them; drift, never.
#[test]
fn cell_edits_reach_the_slot() {
    use chimera_core::ui::orbit::{ORBIT_CHRD, OrbitUi};
    let mut ui = in_orbit();
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::press(Plus));
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id);
    feed(&mut ui, Input::turn(B, 2)); // DEGREE
    OrbitUi::mute(RingIdx::ALL[4], ui.project_mut());
    let song = ui.project().orbit();
    assert!(!song.live().rings[4].ring.mute, "ring 5 unmuted");
    assert_eq!(*song.live(), *song.scene(S[0]));
    assert_ne!(*song.live(), Scene::init(), "edited");
    assert!(!song.edited(), "nothing for RELOAD to restart");
}

/// An edit while a switch waits, through the page: it goes to the playing
/// scene's slot, and comes back with it.
#[test]
fn an_edit_while_queued_reaches_the_playing_slot() {
    let ui = in_orbit();
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running);
    rig.frame(Input::turn(E, 1), 1); // SCENE → 2, queued
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[1]));
    rig.frame(Input::turn(B, -9), 1); // KEY A → C
    let song = rig.ui.project().orbit();
    assert_eq!(song.queued(), Some(S[1]), "still waiting");
    assert_eq!(
        slot(&rig.ui, 0).chords.tonic,
        PitchClass::C,
        "scene 1's slot"
    );
    assert_eq!(slot(&rig.ui, 1), Scene::neutral(), "not the queued one");
    rig.until(|v| v.playing == S[1] && v.cue.is_none());
    rig.idle(1);
    switch(&mut rig, -1);
    assert_eq!(rig.ui.project().orbit().live().chords.tonic, PitchClass::C);
}

/// The reland window (ADR 0083): the runner adopts the playing slot again
/// (stopped: at once), then a NOTE edit lands in the slot and `live`
/// before the UI lands the reland. Landing re-sends every ring, so the
/// runner plays the edit too.
#[test]
fn an_edit_in_the_reland_window_reaches_the_runner() {
    let mut rig = OrbitRig::new();
    rig.run(2);
    let yes = Confirmed::<SceneSlot>::unasked(rig.project.orbit(), S[0]);
    rig.song().reload_scene(yes).expect("nothing queued");
    rig.ui.issue_scene(rig.project.orbit());
    rig.card_read(1);
    assert_eq!(rig.view().cue, None, "the runner adopted it");
    assert_eq!(
        rig.project.orbit().queued(),
        Some(S[0]),
        "the UI has not landed it"
    );
    let mut ring = RingIdx::ALL[1];
    OrbitCell::Note.turn(1, false, rig.project.orbit_mut(), &mut rig.ui, &mut ring);
    let pool = rig.project.orbit().live().rings[1].ring.pool;
    assert_eq!(rig.project.orbit().scene(S[0]).rings[1].ring.pool, pool);
    rig.run(4);
    assert_eq!(rig.project.orbit().queued(), None, "landed");
    assert_eq!(rig.project.orbit().live().rings[1].ring.pool, pool);
    assert_eq!(rig.view().rings[1].pool, pool, "the runner plays the edit");
}

/// Each note edit reaches the playing slot as `live` takes it: NOTE,
/// CUSTOM, CLEAR up and down, a capture (the cells' and `sync`'s own
/// paths), and OCTAVE turned on VOICE.
#[test]
fn every_note_edit_reaches_the_slot() {
    use chimera_core::orbit::runner::{Capture, Captured};
    use chimera_core::project::Project;
    use chimera_core::ui::orbit::OrbitUi;
    type Case = (&'static str, fn(&mut Project, &mut OrbitUi));
    let cases: [Case; 5] = [
        ("NOTE", |p, ui| {
            let mut ring = RingIdx::ALL[1];
            OrbitCell::Note.turn(3, false, p.orbit_mut(), ui, &mut ring);
        }),
        ("CUSTOM", |p, ui| {
            ui.cursor = 1;
            let mut ring = RingIdx::ALL[1];
            OrbitCell::Set.turn(1, false, p.orbit_mut(), ui, &mut ring);
        }),
        ("CLEAR up", |p, ui| {
            let mut ring = RingIdx::ALL[1];
            OrbitCell::Clear.turn(1, true, p.orbit_mut(), ui, &mut ring);
        }),
        ("CLEAR down", |p, ui| {
            let mut ring = RingIdx::ALL[1];
            OrbitCell::Clear.turn(-1, true, p.orbit_mut(), ui, &mut ring);
        }),
        ("capture", |p, ui| {
            let mut notes = [Note::new(0).unwrap(); 8];
            for (i, n) in [50u8, 55, 62].into_iter().enumerate() {
                notes[i] = Note::new(n).unwrap();
            }
            let v = OrbitView {
                captured: Captured {
                    count: 1,
                    target: Capture::Ring(RingIdx::ALL[1]),
                    scene: chimera_core::orbit::scene::SceneIdx::ALL[0],
                    notes,
                    len: 3,
                },
                acked: ui.log().head(),
                ..OrbitView::default()
            };
            ui.sync(&v, p);
        }),
    ];
    for (name, edit) in cases {
        let (mut p, _) = Project::boxed();
        let mut ui = OrbitUi::new();
        edit(&mut p, &mut ui);
        let song = p.orbit();
        assert_ne!(*song.live(), Scene::init(), "{name}: an edit");
        assert_eq!(*song.live(), *song.scene(S[0]), "{name}: in the slot");
    }

    // OCTAVE on VOICE, through the page's encoder.
    let mut ui = in_orbit();
    feed(&mut ui, Input::press(Plus)); // RHYTHM
    tap(&mut ui, Edit); // VOICE
    assert_eq!(ui.page_def().id, chimera_core::ui::orbit::ORBIT_VOICE.id);
    feed(&mut ui, Input::turn(chimera_hal::EncoderId::D, 1));
    let song = ui.project().orbit();
    assert_ne!(
        song.live().rings[0],
        Scene::init().rings[0],
        "OCTAVE: an edit"
    );
    assert_eq!(*song.live(), *song.scene(S[0]), "OCTAVE: in the slot");
}

/// A capture the runner finishes in scene 2, before the UI has landed the
/// switch, goes into scene 2, where it was played: its slot, then `live`
/// as it lands, and the runner plays it. Scene 1 is untouched.
#[test]
fn a_capture_across_a_switch_goes_to_the_scene_it_was_played_in() {
    use chimera_core::orbit::channel::ArmTarget;
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run_loops(1);
    let one = *rig.project.orbit().scene(S[0]);
    rig.ui.queue(S[1], rig.project.orbit_mut());
    rig.ui.arm(Some(ArmTarget::Ring(RingIdx::ALL[1])));
    rig.block();
    while rig.view().playing != S[1] {
        rig.card_read(1);
    }
    for n in [50, 55, 62] {
        rig.key(0, n, true);
    }
    rig.card_read(1);
    for n in [50, 55, 62] {
        rig.key(0, n, false);
    }
    rig.card_read(1);
    assert_eq!(rig.project.orbit().current(), S[0], "not landed yet");
    rig.run(4);
    let want = [50, 55, 62].map(|n| Note::new(n).unwrap());
    let song = rig.project.orbit();
    assert_eq!(song.current(), S[1]);
    assert_eq!(song.scene(S[1]).rings[1].ring.pool.notes(), want);
    assert_eq!(song.live().rings[1].ring.pool.notes(), want);
    assert_eq!(*song.scene(S[0]), one, "scene 1 untouched");
    assert_eq!(
        rig.view().rings[1].pool.notes(),
        want,
        "the runner plays it"
    );
}

/// A CLEAR or RELOAD waiting on the loop survives SCENE turned away and
/// back: the reland stays queued, so `live` and the slot meet again.
#[test]
fn turning_back_keeps_a_pending_reland() {
    use chimera_core::orbit::scene::OrbitSong;
    for clear in [false, true] {
        let mut song = OrbitSong::NEW;
        // A drift's stand-in: `live` off its slot.
        song.live_mut_for_test().rings[0].ring.mute = true;
        let yes = Confirmed::<SceneSlot>::unasked(&song, S[0]);
        if clear {
            song.clear_scene(yes)
        } else {
            song.reload_scene(yes)
        }
        .expect("nothing queued");
        song.queue(S[1]);
        song.queue(S[0]);
        assert_eq!(song.queued(), Some(S[0]), "clear {clear}: still due");
    }
}

/// ACCENT marks no RHYTHM: the runner's knob copy lays the kept hits'
/// accents again itself, and plays the new ACCENT.
#[test]
fn accent_alone_reaches_the_runner() {
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run_loops(1);
    rig.song()
        .edit(&mut |s| s.rings[1].set(SceneRing::ACCENT, 2.0));
    rig.run(4);
    let w = rig.working().rings[1].ring;
    assert_eq!(w.accent(), 2);
    assert_eq!(w, w.with_accent(2), "the accents laid over the hits");
}

/// TARGET reaches the runner with no edit mask: a knob.
#[test]
fn target_reaches_the_runner() {
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run_loops(1);
    rig.song()
        .edit(&mut |s| s.rings[1].set(SceneRing::TARGET, 3.0));
    rig.run(4);
    assert_eq!(rig.working().rings[1].target.part(), PartId::ALL[3]);
}

/// An edit's touched marks are `live`'s to send: the slot it also wrote
/// keeps none, so landing it later sends nothing twice.
#[test]
fn an_edit_leaves_its_slot_untouched() {
    let mut song = chimera_core::orbit::scene::OrbitSong::NEW;
    song.edit(&mut |s| s.rings[0].set(SceneRing::STEPS, 7.0));
    assert!(!song.live().untouched());
    assert!(song.scene(S[0]).untouched());
}

/// A project's ORBIT compares as it is kept: `live` counts, apart from the
/// slots; the slots' revs don't.
#[test]
fn songs_differ_by_live_alone() {
    use chimera_core::orbit::scene::OrbitSong;
    let a = OrbitSong::NEW;
    let mut b = a;
    assert_eq!(a, b);
    b.live_mut_for_test().chords.tonic = PitchClass::E;
    assert_ne!(a, b, "live alone differs");
    let mut c = a;
    c.edit(&mut |s| s.chords.tonic = PitchClass::E);
    assert_ne!(a, c, "an edit, slot and live");
    let mut d = c;
    d.live_mut_for_test().chords.tonic = a.live().chords.tonic;
    assert_ne!(c, d, "the slot matches, live does not");
}
