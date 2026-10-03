//! Scenes (Task 16, spec § 4.1, § 6.3): SCN's SCENE queues one, which
//! lands on the loop; SEQ saves the playing one with its drift; SCN's SAVE
//! over another saved scene asks, and only its pill overwrites.

mod common;
mod screen;

use chimera_core::block::Block;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::orbit::engine::{OrbitEvent, OrbitKind};
use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
use chimera_core::orbit::scene::{Scene, SceneIdx, SceneRing};
use chimera_core::part::{Held, LiveSnap};
use chimera_core::project::{Confirmed, PartId, SceneSlot};
use chimera_core::triple::{Reader, TripleBuffer, Writer};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::orbit::{ORBIT_SCN, OrbitCell};
use chimera_core::ui::region::{RegionData, RegionKind};
use chimera_hal::ButtonId::{B2, Edit, Menu, Plus, Seq};
use chimera_hal::EncoderId::{A, B};
use chimera_theory::{Note, PitchClass, SnapTo, snap};
use common::orbit::OrbitRig;
use screen::{Input, feed, hold, tap};

const S: [SceneIdx; 8] = SceneIdx::ALL;

/// A fresh UI, inside ORBIT on ORB.
fn in_orbit() -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
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
    ui.project_mut().orbit_mut().live.chords.tonic = tonic;
}

/// The question the open prompt asks.
fn question(ui: &UiState) -> String {
    ui.prompt_words_for_test()
        .map(|(q, _)| q.as_str().into())
        .unwrap_or_default()
}

/// The UI and the runner through the triple buffer, as the shells run
/// them: the view in, the keys, the publish; then the blocks.
struct Rig {
    ui: Box<UiState>,
    w: Writer<AudioShared>,
    r: Reader<AudioShared>,
    inst: Box<Instrument>,
    runner: Box<OrbitRunner>,
    /// Each block's events.
    sent: Vec<Vec<OrbitEvent>>,
    views: Vec<OrbitView>,
}

impl Rig {
    fn new(ui: Box<UiState>) -> Rig {
        let buf = Box::leak(Box::new(TripleBuffer::new(
            AudioShared::default(),
            AudioShared::default(),
            AudioShared::default(),
        )));
        let (w, r) = buf.split();
        let sr = core::num::NonZeroU32::new(chimera_hal::SAMPLE_RATE).unwrap();
        Rig {
            ui,
            w,
            r,
            inst: Box::new(Instrument::new(
                chimera_hal::SAMPLE_RATE,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            runner: Box::new(OrbitRunner::new(sr)),
            sent: Vec::new(),
            views: Vec::new(),
        }
    }

    fn frame(&mut self, input: Input, blocks: usize) {
        self.ui.set_orbit_view(&self.runner.view());
        feed(&mut self.ui, input);
        let ui = &self.ui;
        self.w.publish(|b| {
            b.update_from(ui.project().perf(), 0);
            ui.publish_orbit(&mut b.orbit);
        });
        for _ in 0..blocks {
            let ev = self.runner.block(&mut self.inst, self.r.read());
            self.sent.push(ev.iter().collect());
            self.views.push(self.runner.view());
        }
    }

    fn idle(&mut self, blocks: usize) {
        self.frame(Input::default(), blocks);
    }

    fn b6(&mut self) {
        self.frame(Input::press(chimera_hal::ButtonId::B6), 1);
        self.frame(Input::release(chimera_hal::ButtonId::B6), 1);
    }

    fn tap(&mut self, b: chimera_hal::ButtonId) {
        self.frame(Input::press(b).at(0), 1);
        self.frame(Input::release(b).at(100), 1);
    }

    fn view(&self) -> OrbitView {
        self.runner.view()
    }

    /// Frames of one block until `f` holds of the view.
    fn until(&mut self, f: impl Fn(&OrbitView) -> bool) {
        for _ in 0..200_000 {
            if f(&self.view()) {
                return;
            }
            self.idle(1);
        }
        panic!("never");
    }
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
    let a = rig.project.orbit().live;
    let mut b = a;
    b.chords.tonic = PitchClass::C;
    for r in &mut b.rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    rig.song().live = b;
    let c = Confirmed::<SceneSlot>::unasked(rig.project.orbit(), S[1]);
    rig.song().overwrite_scene(c).expect("nothing queued");
    rig.song().live = a;
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
        rig.project.orbit().live.chords.tonic,
        PitchClass::A,
        "live waits"
    );
    rig.block();
    assert_eq!(rig.view().playing, S[0]);
    assert_eq!(rig.view().queued, Some(S[1]));
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
    assert_eq!((rig.view().playing, rig.view().queued), (S[1], None));
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
    assert_eq!(rig.project.orbit().live, b, "live is the slot's copy");
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
    assert_eq!((rig.view().playing, rig.view().queued), (S[0], None));
    assert_eq!(rig.project.orbit().current(), S[0]);
}

/// A held key on a RE-SNAP Part re-snaps when a switch lands: a landed
/// scene is a harmony change like any other.
#[test]
fn a_landed_scene_resnaps_held_keys() {
    let mut rig = OrbitRig::on_init();
    for ring in &mut rig.song().live.rings {
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
            .filter(|s| s.held() && s.part() == Some(0))
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
            for r in &mut rig.song().live.rings {
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
        assert_eq!(song.live, *song.scene(S[1]), "live is the slot's copy");
    }
}

/// Queuing leaves `live` alone; turning SCENE back to the playing scene
/// before the boundary cancels the switch: nothing is lost, and the loop
/// goes on, mutating, with no relaunch.
#[test]
fn turning_back_cancels_the_switch() {
    let mut rig = OrbitRig::new();
    for r in &mut rig.song().live.rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    with_c_in_slot_2(&mut rig);
    rig.play(true);
    rig.run_loops(1);
    rig.run(300);
    rig.song().live.chords.tonic = PitchClass::E;
    rig.run(2);
    let before = rig.project.orbit().live;
    rig.ui.queue(S[1], rig.project.orbit_mut());
    assert_eq!(rig.project.orbit().live, before, "queuing copies nothing");
    rig.run(2);
    assert_eq!(rig.view().queued, Some(S[1]));
    rig.ui.queue(S[0], rig.project.orbit_mut());
    assert_eq!(rig.project.orbit().queued(), None, "cancelled");
    rig.run(2);
    assert_eq!((rig.view().playing, rig.view().queued), (S[0], None));
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
    assert_eq!(song.live.chords.tonic, PitchClass::E);
}

/// A view from before a switch lands nothing: away to 2 and landed, back
/// to 1, a stale view playing 1 (an old rev) leaves 1 queued; only the
/// rev's own view lands it. Landing on `playing == queued` would not.
#[test]
fn a_stale_view_lands_nothing() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let view = |playing: usize, rev: u16| OrbitView {
        playing: S[playing],
        scene_rev: rev,
        ..OrbitView::default()
    };
    let stale = ui.orbit().scene_rev;
    feed(&mut ui, Input::turn(A, 1));
    ui.set_orbit_view(&view(1, ui.orbit().scene_rev));
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
    ui.set_orbit_view(&view(0, ui.orbit().scene_rev));
    assert_eq!(ui.project().orbit().current(), S[0]);
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
        scene_rev: ui.orbit().scene_rev,
        ..OrbitView::default()
    };
    ui.set_orbit_view(&v);
    assert_eq!(ui.orbit().save_slot(ui.project().orbit()), S[2]);

    let mut o = OrbitUi::new();
    o.save = Some(S[4]);
    o.loaded(1);
    assert_eq!(o.save, None);
}

/// 20 loops of MUTATE, a SEQ tap, a switch away and back: the slot plays
/// the drift as it was saved.
#[test]
fn save_captures_the_drift() {
    let mut ui = in_orbit();
    for r in &mut ui.project_mut().orbit_mut().live.rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    let init = scene_drift(&ui.project().orbit().live);
    let mut rig = Rig::new(ui);
    rig.b6();
    rig.until(|v| v.loops >= 20);
    rig.idle(1);
    rig.tap(Seq);
    assert_eq!(toast(&mut rig.ui), "SCENE 1 SAVED");
    let saved = slot(&rig.ui, 0);
    assert_ne!(scene_drift(&saved), init, "20 loops drift");
    assert_eq!(saved.chords, rig.ui.project().orbit().live.chords);

    to_scn(&mut rig.ui);
    rig.frame(Input::turn(A, 1), 1);
    rig.until(|v| v.playing == S[1] && v.queued.is_none());
    rig.frame(Input::turn(A, -1), 1);
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    let v = rig.view();
    assert_eq!((v.playing, v.queued), (S[0], None));
    assert_eq!(drift(&v), scene_drift(&saved), "the slot plays its drift");
}

/// While a switch is queued, SEQ and SCN's SAVE are refused, and say so:
/// `live` is the queued scene's.
#[test]
fn save_while_queued_is_refused() {
    let mut ui = in_orbit();
    to_scn(&mut ui);
    let before: Vec<Scene> = (0..8).map(|n| slot(&ui, n)).collect();
    feed(&mut ui, Input::turn(A, 1));
    assert_eq!(ui.project().orbit().queued(), Some(S[1]));
    edit_live(&mut ui, PitchClass::E);
    tap(&mut ui, Seq);
    assert_eq!(toast(&mut ui), "SWITCH PENDING");
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
    assert_eq!(five, ui.project().orbit().live);
    assert_ne!(five, Scene::neutral());

    edit_live(&mut ui, PitchClass::F);
    for cancel in [Menu, Seq] {
        save_to(&mut ui, 5);
        assert_eq!(question(&ui), "SAVE OVER SCENE 5?");
        if cancel == Seq {
            feed(&mut ui, Input::turn(A, 1)); // CANCEL
        }
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
    tap(&mut ui, Seq); // SAVE OVER
    assert!(!ui.prompt_open());
    assert_eq!(slot(&ui, 4), ui.project().orbit().live);
    assert_eq!(slot(&ui, 4).chords.tonic, PitchClass::F);
    assert_eq!(toast(&mut ui), "SCENE 5 SAVED");

    // Neutral slot 6: at once.
    save_to(&mut ui, 6);
    assert!(!ui.prompt_open());
    assert_eq!(slot(&ui, 5), ui.project().orbit().live);
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
    tap(&mut ui, Seq); // SAVE OVER
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
        queued: None,
        scene_rev: ui.orbit().scene_rev,
        ..OrbitView::default()
    };
    ui.set_orbit_view(&v);
    assert_eq!(ui.project().orbit().current(), S[2]);
    ui.render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    let after = header(&ui);
    assert_ne!(after, before, "the header redraws");
    assert!(matches!(after, Some(RegionData::Header { scene: 3, .. })));
}

/// SEQ saves the scene on every ORBIT page, and moves none.
#[test]
fn seq_saves_on_every_orbit_page() {
    use chimera_core::orbit::chords::ScaleKind;
    let mut ui = in_orbit();
    ui.project_mut().orbit_mut().live.chords.scale = ScaleKind::Custom;
    let mut seen = Vec::new();
    for _ in 0..4 {
        for _ in 0..3 {
            let at = ui.location();
            edit_live(&mut ui, PitchClass::ALL[seen.len() % 12]);
            tap(&mut ui, Seq);
            assert_eq!(ui.location(), at, "SEQ moves no page");
            assert_eq!(slot(&ui, 0), ui.project().orbit().live);
            assert_eq!(toast(&mut ui), "SCENE 1 SAVED");
            seen.push(ui.page_def().id);
            // RING's and CHRD's sub-pages; EDIT does nothing elsewhere.
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
    assert_eq!(song.live, Scene::init());
    assert!(S[1..].iter().all(|&n| *song.scene(n) == Scene::neutral()));
    assert_eq!((song.current(), song.queued()), (S[0], None));
    let ui = UiState::new();
    assert_eq!(*ui.project().orbit(), song);
}
