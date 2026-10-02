//! ORBIT's settings in the project (Task 7): the ring, chord and tempo
//! blocks, their frozen codes, and the drift the UI absorbs from the view.

mod screen;

use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, DiskCode};
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::chords::ScaleKind;
use chimera_core::orbit::clock::Bpm;
use chimera_core::orbit::ring::{NoteMode, Octave, Range, SnapTo};
use chimera_core::orbit::rng::Pct;
use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
use chimera_core::orbit::scene::{OrbitSong, Scene, SceneRing, TargetKind};
use chimera_core::orbit::walk::{Pool, Walk};
use chimera_core::project::PartId;
use chimera_core::storage::Card;
use chimera_core::triple::{Reader, TripleBuffer, Writer};
use chimera_core::ui::UiState;
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use chimera_theory::{Degree, Note, PitchClass};
use screen::Input;

const P: [PartId; 6] = PartId::ALL;
const R: [RingIdx; 5] = RingIdx::ALL;

/// A fresh UI, inside ORBIT on ORB.
fn in_orbit() -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    screen::hold(&mut ui, ButtonId::Seq);
    assert!(ui.location().orbit().is_some());
    ui
}

fn steps(r: &SceneRing) -> u8 {
    r.ring.fill().steps().get()
}

#[test]
fn ring_cells_edit_the_selected_ring() {
    let mut ui = in_orbit();
    screen::tap(&mut ui, ButtonId::B3);
    assert_eq!(ui.location().orbit().map(|o| o.ring()), Some(R[2]));
    let before = ui.project().orbit().live;
    screen::feed(&mut ui, Input::turn(EncoderId::A, 1));
    let after = ui.project().orbit().live;
    for i in 0..5 {
        let (b, a) = (&before.rings[i], &after.rings[i]);
        if i == 2 {
            assert_eq!(steps(a), steps(b) + 1, "ring 3's STEPS");
        } else {
            assert_eq!(a, b, "ring {} untouched", i + 1);
        }
    }
    assert_eq!(after.chords, before.chords);
}

#[test]
fn rhythm_edits_bump_the_rev_and_knobs_do_not() {
    let mut r = Scene::init().rings[2];
    let revs = |r: &SceneRing| (r.rhythm_rev, r.notes_rev);
    let (rhythm, notes) = revs(&r);
    r.nudge(SceneRing::STEPS, 1);
    r.nudge(SceneRing::HITS, 1);
    r.nudge(SceneRing::ROTATE, 1);
    assert_eq!(revs(&r), (rhythm.wrapping_add(3), notes));
    r.nudge(SceneRing::CHANCE, -1);
    assert_eq!(r.ring.chance, Pct::clamped(99));
    assert_eq!(
        revs(&r),
        (rhythm.wrapping_add(3), notes),
        "CHANCE is a knob"
    );

    // A turn that changes nothing re-lays nothing.
    r.set(SceneRing::STEPS, 31.0);
    let at_max = revs(&r);
    r.nudge(SceneRing::STEPS, 1);
    assert_eq!(revs(&r), at_max);

    // HITS never exceeds STEPS, whichever moves.
    r.set(SceneRing::STEPS, 3.0);
    assert_eq!(steps(&r), 4);
    r.set(SceneRing::HITS, 32.0);
    assert_eq!(r.ring.fill().hits(), 4);
    r.set(SceneRing::STEPS, 1.0);
    assert_eq!((steps(&r), r.ring.fill().hits()), (2, 2));
    assert_eq!(r.ring.hits_mask().hits(), 2, "the mask is re-laid");

    // ROTATE wraps at STEPS.
    r.set(SceneRing::STEPS, 4.0);
    r.set(SceneRing::ROTATE, 4.0);
    assert_eq!(r.ring.fill().rotate(), 4);
    r.nudge(SceneRing::ROTATE, 1);
    assert_eq!(r.ring.fill().rotate(), 0);
}

#[test]
fn octave_moves_the_notes() {
    // Ring 2 of the init scene: OCTAVE 3, a pool of C3, E3, A3.
    let mut r = Scene::init().rings[1];
    assert_eq!(r.ring.octave, Octave::new(3).unwrap());
    let pool: Vec<u8> = r.ring.pool.notes().iter().map(|n| n.get()).collect();
    let notes = r.ring.notes;
    let rev = r.notes_rev;
    r.set(SceneRing::OCTAVE, 5.0);
    assert_eq!(r.ring.octave, Octave::new(5).unwrap());
    let up: Vec<u8> = r.ring.pool.notes().iter().map(|n| n.get()).collect();
    assert_eq!(up, pool.iter().map(|n| n + 24).collect::<Vec<_>>());
    for (a, b) in r.ring.notes.iter().zip(notes) {
        assert_eq!(a.get(), b.get() + 24);
    }
    assert_eq!(r.notes_rev, rev.wrapping_add(1));
    assert_eq!(r.rhythm_rev, Scene::init().rings[1].rhythm_rev);

    // A note pushed past 127 folds back down by octaves.
    r.set(SceneRing::OCTAVE, 7.0);
    r.ring.pool = Pool::new(&[Note::new(96).unwrap(), Note::new(125).unwrap()]).unwrap();
    r.set(SceneRing::OCTAVE, 8.0);
    let top: Vec<u8> = r.ring.pool.notes().iter().map(|n| n.get()).collect();
    assert_eq!(top, [108, 125]);
}

#[test]
fn target_steers_the_cross() {
    let mut ui = in_orbit();
    ui.project_mut().orbit_mut().live.rings[1].set(SceneRing::TARGET, 4.0);
    screen::hold(&mut ui, ButtonId::B2);
    assert_eq!(ui.location().part(), Some(P[4]));
    assert_eq!(ui.active_part, P[4]);
}

/// The fixture's frozen keys.
fn fixture() -> Vec<&'static str> {
    include_str!("fixtures/disk_codes_v1.txt")
        .lines()
        .map(|l| l.split(" # ").next().unwrap())
        .collect()
}

/// `(code, ident)` of each value.
fn codes<T: DiskCode>(all: &[T]) -> Vec<(u8, &'static str)> {
    all.iter()
        .map(|v| (v.disk_code(), v.disk_ident()))
        .collect()
}

#[test]
fn orbit_codes_pinned() {
    let f = fixture();
    let has = |k: String| assert!(f.contains(&k.as_str()), "not pinned: {k}");
    has("K 27 ORBIT".into());
    for (r, ring) in R.into_iter().enumerate() {
        let code = BlockRef::OrbitRing(ring).disk_code().unwrap();
        assert_eq!(code, 28 + r as u8);
        has(format!("K {code} RING{}", r + 1));
    }
    assert_eq!(BlockRef::Tempo.disk_code(), None);

    let scales = codes(&ScaleKind::ALL);
    assert_eq!(
        scales,
        [
            (0, "MAJOR"),
            (1, "MINOR"),
            (2, "DORIAN"),
            (3, "PHRYGIAN"),
            (4, "LYDIAN"),
            (5, "MIXOLYDIAN"),
            (6, "LOCRIAN"),
            (7, "HARMONIC_MINOR"),
            (8, "PENT_MAJOR"),
            (9, "PENT_MINOR"),
            (10, "CUSTOM"),
        ]
    );
    let pinned = |block: u8, id: u8, all: Vec<(u8, &str)>| {
        for (c, i) in all {
            has(format!("E {block} {id} {c} {i}"));
        }
    };
    pinned(27, 0, codes(&PitchClass::ALL));
    pinned(27, 1, scales);
    pinned(
        28,
        10,
        codes(&[Range::Zero, Range::One, Range::Two, Range::Three]),
    );
    pinned(28, 11, codes(&[SnapTo::Scale, SnapTo::Chord]));
    pinned(28, 12, codes(&[NoteMode::Step, NoteMode::Pool]));
    pinned(
        28,
        13,
        codes(&[Walk::Up, Walk::Down, Walk::Random, Walk::PingPong]),
    );
    for (c, i) in codes(&Degree::ALL) {
        has(format!("C DEGREE {c} {i}"));
    }
    for (c, i) in codes(&[TargetKind::Part]) {
        has(format!("C TARGET_KIND {c} {i}"));
    }
}

#[test]
fn new_project_orbit_is_init_and_pristine() {
    assert_eq!(OrbitSong::NEW.live, Scene::init());
    assert_eq!(OrbitSong::NEW.bpm, Bpm::DEFAULT);
    let mut ui = Box::new(UiState::new());
    assert_eq!(*ui.project().orbit(), OrbitSong::NEW);
    let song = ui.project_mut().orbit_mut();
    song.live.rings[0].set(SceneRing::STEPS, 6.0);
    song.bpm = Bpm::new(90).unwrap();
    assert_ne!(*ui.project().orbit(), OrbitSong::NEW);
    // No card file: boot falls back to NEW through `reset_new`.
    ui.boot_project(&mut Card::new(), &mut MemStore::new(1), None);
    assert_eq!(*ui.project().orbit(), OrbitSong::NEW);
}

#[test]
fn orbit_edits_bump_the_project_rev() {
    let mut ui = in_orbit();
    screen::tap(&mut ui, ButtonId::B1);
    let rev = ui.project().rev();
    screen::feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_ne!(ui.project().rev(), rev, "STEPS marks the project");
    let rev = ui.project().rev();
    screen::tap(&mut ui, ButtonId::B6);
    assert!(ui.orbit().run);
    assert_eq!(
        ui.project().rev(),
        rev,
        "the transport is not the project's"
    );
}

/// The UI and the runner joined as the desktop joins them: the UI
/// publishes into a real triple buffer and the runner reads one slot a
/// block, so a field left unwritten shows up stale.
struct Rig {
    ui: Box<UiState>,
    w: Writer<AudioShared>,
    r: Reader<AudioShared>,
    inst: Box<Instrument>,
    runner: Box<OrbitRunner>,
    epoch: u32,
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
            epoch: 0,
            views: Vec::new(),
        }
    }

    /// One UI loop as the shells run it: the latest view, the keys, the
    /// publish; then `blocks` audio blocks, each reading the newest slot.
    fn frame(&mut self, input: Input, blocks: usize) {
        self.ui.set_orbit_view(&self.runner.view());
        screen::feed(&mut self.ui, input);
        let (ui, epoch) = (&self.ui, self.epoch);
        self.w.publish(|b| {
            b.update_from(ui.project().perf(), epoch);
            ui.publish_orbit(&mut b.orbit);
        });
        for _ in 0..blocks {
            self.runner.block(&mut self.inst, self.r.read());
            self.views.push(self.runner.view());
        }
    }

    fn idle(&mut self, blocks: usize) {
        self.frame(Input::default(), blocks);
    }

    fn b6(&mut self) {
        self.frame(Input::press(ButtonId::B6), 1);
        self.frame(Input::release(ButtonId::B6), 1);
    }

    fn running(&self) -> bool {
        self.runner.view().running
    }

    /// Times the transport started, and stopped, block by block.
    fn edges(&self) -> (usize, usize) {
        let runs: Vec<bool> = self.views.iter().map(|v| v.running).collect();
        let starts = runs.windows(2).filter(|w| !w[0] && w[1]).count();
        let stops = runs.windows(2).filter(|w| w[0] && !w[1]).count();
        (
            starts + runs.first().copied().unwrap_or(false) as usize,
            stops,
        )
    }
}

/// Task 5's carry: `publish` hands back a slot two publishes stale, so
/// every publish writes all of ORBIT's fields. One B6 and then frames
/// with nothing new are one start and no stop.
#[test]
fn one_press_is_one_start_through_the_triple_buffer() {
    let mut rig = Rig::new(in_orbit());
    rig.b6();
    for _ in 0..8 {
        rig.idle(1);
    }
    assert!(rig.running());
    assert_eq!(rig.edges(), (1, 0));
}

/// The rings' drift as the view shows it, and as `live` holds it.
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

fn live_drift(s: &Scene) -> Vec<(u32, Vec<u8>, Vec<u8>)> {
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

#[test]
fn stop_play_keeps_the_drift() {
    let mut ui = in_orbit();
    for r in &mut ui.project_mut().orbit_mut().live.rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    let init = live_drift(&ui.project().orbit().live);
    let mut rig = Rig::new(ui);
    rig.b6();
    while rig.runner.view().loops < 10 {
        rig.idle(100);
    }
    rig.b6();
    rig.idle(1);
    assert!(!rig.running());
    let stopped = rig.runner.view();
    let live = live_drift(&rig.ui.project().orbit().live);
    assert_eq!(live, drift(&stopped), "live holds the view's drift");
    assert_ne!(live, init, "10 loops at MUTATE 100 % drift");

    rig.b6();
    assert!(rig.running());
    let first = rig.runner.view();
    assert_eq!(first.pos.bar, 0, "still the first loop");
    assert_eq!(drift(&first), live, "PLAY plays the drift");
}

#[test]
fn a_self_stop_clears_run() {
    let mut rig = Rig::new(in_orbit());
    rig.b6();
    rig.idle(4);
    assert!(rig.running());
    rig.idle(1);
    // A load: the runner stops itself, with no edge from the UI.
    rig.epoch += 1;
    rig.idle(1);
    assert!(!rig.running());
    assert!(rig.ui.orbit().run, "the UI hasn't seen the stop yet");
    rig.idle(1);
    assert!(!rig.ui.orbit().run, "the view's stop clears run");
    rig.b6();
    assert!(rig.running(), "one B6 starts it again");
    assert!(rig.ui.orbit().run);
}
