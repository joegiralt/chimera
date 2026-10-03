//! ORBIT's settings in the project (Task 7): the ring, chord and tempo
//! blocks, their frozen codes, and the drift the UI absorbs from the view.

// ORBIT's UI: SEQ hold opens it only with the feature.
#![cfg(feature = "orbit")]

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
use chimera_core::ui::orbit::OrbitUi;
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
fn rhythm_edits_touch_and_knobs_do_not() {
    use chimera_core::orbit::channel::RingMask;
    let mut s = Scene::init();
    let none = (RingMask::EMPTY, RingMask::EMPTY);
    s.rings[2].nudge(SceneRing::STEPS, 1);
    s.rings[2].nudge(SceneRing::HITS, 1);
    s.rings[2].nudge(SceneRing::ROTATE, 1);
    assert_eq!(s.harvest(), (RingMask::EMPTY.with(R[2]), RingMask::EMPTY));
    s.rings[2].nudge(SceneRing::CHANCE, -1);
    assert_eq!(s.rings[2].ring.chance, Pct::clamped(99));
    assert_eq!(s.harvest(), none, "CHANCE is a knob");

    // A turn that changes nothing re-lays nothing.
    s.rings[2].set(SceneRing::STEPS, 31.0);
    s.harvest();
    s.rings[2].nudge(SceneRing::STEPS, 1);
    assert_eq!(s.harvest(), none);
    let mut r = s.rings[2];

    // HITS never exceeds STEPS, whichever moves.
    r.set(SceneRing::STEPS, 3.0);
    assert_eq!(steps(&r), 4);
    r.set(SceneRing::HITS, 32.0);
    assert_eq!(r.ring.fill().hits(), 4);
    r.set(SceneRing::STEPS, 1.0);
    assert_eq!((steps(&r), r.ring.fill().hits()), (2, 2));
    assert_eq!(r.ring.hits_mask().hits(), 2, "the mask is re-laid");

    // ROTATE wraps both ways at every STEPS.
    for n in 1..=32u8 {
        r.set(SceneRing::STEPS, f32::from(n - 1));
        r.set(SceneRing::ROTATE, 0.0);
        r.nudge(SceneRing::ROTATE, -1);
        assert_eq!(r.ring.fill().rotate(), n - 1, "{n} steps, down from 0");
        r.nudge(SceneRing::ROTATE, 1);
        assert_eq!(r.ring.fill().rotate(), 0, "{n} steps, up from the last");
    }

    // VEL is 1..=127: no velocity is a note-off.
    r.set(SceneRing::VEL, 0.0);
    assert_eq!(r.ring.velocity.get(), 1);
}

#[test]
fn octave_moves_the_notes() {
    // Ring 2 of the init scene: OCTAVE 4, a pool of A4, C5, E5.
    let r = Scene::init().rings[1];
    assert_eq!(r.ring.octave, Octave::new(4).unwrap());
    let pool: Vec<u8> = r.ring.pool.notes().iter().map(|n| n.get()).collect();
    let notes = r.ring.notes;
    let mut s = Scene::init();
    s.rings[1].set(SceneRing::OCTAVE, 6.0);
    let touched = s.harvest();
    let mut r = s.rings[1];
    assert_eq!(r.ring.octave, Octave::new(6).unwrap());
    let up: Vec<u8> = r.ring.pool.notes().iter().map(|n| n.get()).collect();
    assert_eq!(up, pool.iter().map(|n| n + 24).collect::<Vec<_>>());
    for (a, b) in r.ring.notes.iter().zip(notes) {
        assert_eq!(a.get(), b.get() + 24);
    }
    let ring = chimera_core::orbit::channel::RingMask::EMPTY;
    assert_eq!(
        touched,
        (ring, ring.with(R[1])),
        "the notes, not the rhythm"
    );

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
fn new_project_orbit_is_init_and_reset_restores_it() {
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
    let at = ui.orbit().log().head();
    screen::tap(&mut ui, ButtonId::B6);
    let sent = chimera_core::orbit::channel::receive(ui.orbit().log(), &mut at.clone());
    assert!(sent.flip, "one TogglePlay");
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
            views: Vec::new(),
        }
    }

    /// One UI loop as the shells run it: the latest view, the keys, the
    /// publish; then `blocks` audio blocks, each reading the newest slot.
    fn frame(&mut self, input: Input, blocks: usize) {
        self.ui.set_orbit_view(&self.runner.view());
        screen::feed(&mut self.ui, input);
        let (ui, epoch) = (&self.ui, 0);
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

/// B6 presses published with no block between: the runner reads only the
/// newest slot, and every press still counts, past the log's wrap. Two
/// are a stop and a play, 257 a stop. Once the log is full, presses fold
/// by parity in the outbox: two leave the log as it is, three put one
/// `TogglePlay` in it once the runner acks.
#[test]
fn presses_between_runner_reads_all_count() {
    use chimera_core::orbit::channel::{LOG, Seq, receive};
    let mut rig = Rig::new(in_orbit());
    rig.b6();
    rig.idle(4);
    assert!(rig.running());
    let press = |rig: &mut Rig| {
        rig.frame(Input::press(ButtonId::B6), 0);
        rig.frame(Input::release(ButtonId::B6), 0);
    };
    let head = |rig: &Rig| rig.ui.orbit().log().head();
    let from = head(&rig);
    press(&mut rig);
    press(&mut rig);
    assert_eq!(head(&rig).since(from), 2);
    let sent = receive(rig.ui.orbit().log(), &mut from.clone());
    assert!(!sent.flip, "two toggles: no flip");
    rig.idle(4);
    assert!(rig.running(), "two presses: stopped and played again");
    let from = head(&rig);
    for _ in 0..257 {
        press(&mut rig);
    }
    assert_eq!(head(&rig).since(from) as usize, LOG, "the log full");
    rig.idle(4);
    rig.idle(4);
    assert!(!rig.running(), "257 presses: stopped");
    rig.b6();
    assert!(rig.running(), "the next press plays");

    // The log full again: two presses fold away, three leave one.
    let full = |rig: &mut Rig| {
        for _ in 0..LOG {
            press(rig);
        }
    };
    full(&mut rig);
    let at = head(&rig);
    press(&mut rig);
    press(&mut rig);
    assert_eq!(head(&rig), at, "two presses folded in the outbox");
    rig.idle(1);
    rig.idle(1);
    assert_eq!(head(&rig), at, "and cancelled");
    assert!(rig.running());
    full(&mut rig);
    let at = head(&rig);
    for _ in 0..3 {
        press(&mut rig);
    }
    assert_eq!(head(&rig), at);
    rig.idle(1);
    rig.idle(1);
    assert_eq!(head(&rig).since(at), 1, "one TogglePlay");
    let mut a: Seq = at;
    assert!(receive(rig.ui.orbit().log(), &mut a).flip);
    rig.idle(1);
    assert!(!rig.running(), "three presses: stopped");
}

/// Review Focus 5: ORBIT stopped, the runner stalls for 12 frames, and
/// B6 is pressed in 11 of them, twice in some. When it resumes, ORBIT
/// runs iff the presses were odd, once they are all acked, and stays so.
#[test]
fn b6_presses_during_a_stall_toggle_by_parity() {
    use chimera_core::orbit::rng::Rng;
    for seed in 1..=8u32 {
        let mut rng = Rng::new(seed);
        let mut rig = Rig::new(in_orbit());
        rig.idle(2);
        assert!(!rig.running());
        let skip = rng.below(12);
        let mut presses = 0;
        for f in 0..12 {
            if f != skip {
                if rng.below(3) == 0 {
                    screen::feed(&mut rig.ui, Input::press(ButtonId::B6));
                    screen::feed(&mut rig.ui, Input::release(ButtonId::B6));
                    presses += 1;
                }
                screen::feed(&mut rig.ui, Input::press(ButtonId::B6));
                presses += 1;
            }
            rig.frame(Input::release(ButtonId::B6), 0);
        }
        while !rig.ui.orbit().idle() {
            rig.idle(1);
        }
        rig.idle(1);
        for _ in 0..20 {
            assert_eq!(
                rig.running(),
                presses % 2 == 1,
                "seed {seed}: {presses} presses"
            );
            rig.idle(1 + rng.below(4) as usize);
        }
    }
}

/// B6 pressed before a load, read by the runner only with it: the load
/// voids the press, so the runner takes the load and never starts, and
/// the next B6 starts.
#[test]
fn b6_with_a_load_never_sticks() {
    let mut rig = screen::ShellRig::new(in_orbit());
    rig.frame(Input::press(ButtonId::B6), 0);
    rig.load_new();
    rig.frame(Input::release(ButtonId::B6), 1);
    assert!(!rig.view().running, "the load wins the block");
    rig.idle(1);
    rig.b6();
    assert!(rig.view().running, "one B6 starts it");
}

/// B6 pressed after a load, before the runner has read it: the press is
/// the player's first act on the loaded project, so the block that takes
/// the load plays (spec § 3: a post-load command is applied).
#[test]
fn b6_after_a_load_read_with_it_plays() {
    let mut rig = screen::ShellRig::new(in_orbit());
    rig.idle(2);
    rig.load_new();
    rig.frame(Input::press(ButtonId::B6), 1);
    let v = rig.view();
    assert_eq!(v.epoch, 1, "the load taken");
    assert!(v.running, "the press after it plays");
    rig.frame(Input::release(ButtonId::B6), 4);
    assert!(rig.view().running);
}

/// A view from before a load is the old project's: its drift stays out
/// of the new one.
#[test]
fn a_view_from_before_a_load_brings_no_drift() {
    let (mut p, _) = chimera_core::project::Project::boxed();
    let mut o = OrbitUi::new();
    o.loaded(1, p.orbit_mut());
    let live = p.orbit().live;
    let drift = |n: u8| Pool::one(Note::new(n).unwrap());
    let mut v = OrbitView {
        loops: 5,
        ..OrbitView::default()
    };
    v.rings[1].pool = drift(70);
    let absorbed = |p: &chimera_core::project::Project| p.orbit().live.rings[1].ring.pool;
    // The old project's epoch.
    o.sync(&v, &mut p);
    assert_eq!(p.orbit().live, live, "epoch 0, the old project's");
    v.epoch = 1;
    v.acked = o.log().head();
    o.sync(&v, &mut p);
    assert_eq!(absorbed(&p), drift(70), "the new project's");
    v.rings[1].pool = drift(72);
    o.sync(&v, &mut p);
    assert_eq!(absorbed(&p), drift(70), "once a loop");
}

/// A view on the load's epoch with `acked` before the load's seq can't
/// arise (the load publish carries both): the epoch is the gate, and
/// debug builds say so.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a view from before the load")]
fn a_view_on_the_epoch_acked_before_the_load_is_a_bug() {
    let (mut p, _) = chimera_core::project::Project::boxed();
    let mut o = OrbitUi::new();
    o.loaded(1, p.orbit_mut());
    let v = OrbitView {
        epoch: 1,
        ..OrbitView::default()
    };
    o.sync(&v, &mut p);
}

/// A load stops the runner with no press from the UI: B6 is a press, so
/// one starts it again (ADR 0080).
#[test]
fn one_b6_starts_after_a_self_stop() {
    let mut rig = screen::ShellRig::new(in_orbit());
    let running = |rig: &screen::ShellRig| rig.view().running;
    rig.b6();
    rig.idle(4);
    assert!(running(&rig));
    rig.idle(1);
    // A load: the runner stops itself, with no edge from the UI.
    rig.load_new();
    rig.idle(1);
    assert!(!running(&rig));
    rig.idle(1);
    rig.b6();
    assert!(running(&rig), "one B6 starts it again");
}

/// Each ring draws the runner's hits, drift and all, unless an edit to
/// that ring's rhythm is still unacked: then it draws `live`'s. A command
/// for no ring (B6), or an edit to another ring, changes nothing.
#[test]
fn only_a_ring_with_an_unacked_edit_draws_live() {
    use chimera_core::orbit::euclid::HitMask;
    use chimera_core::ui::orbit::draw::OrbitFrame;
    let mut ui = in_orbit();
    let mut v = OrbitView::default();
    for r in &mut v.rings {
        r.hits = HitMask::empty(r.hits.steps());
    }
    v.acked = ui.orbit().log().head();
    ui.set_orbit_view(&v);
    let drawn =
        |ui: &UiState, r: RingIdx| OrbitFrame::new(ui.project().orbit(), ui.orbit()).hits(r);
    let runner = |r: RingIdx| v.rings[r.get() as usize].hits;
    for r in R {
        assert_eq!(drawn(&ui, r), runner(r), "ring {}: all acked", r.get() + 1);
    }
    screen::feed(&mut ui, Input::press(ButtonId::B6));
    assert!(!ui.orbit().idle(), "B6 unacked");
    for r in R {
        assert_eq!(
            drawn(&ui, r),
            runner(r),
            "ring {}: B6 is no ring's",
            r.get() + 1
        );
    }
    let ring = &mut ui.project_mut().orbit_mut().live.rings[1];
    let h = ring.ring.fill().hits();
    ring.set(
        SceneRing::HITS,
        f32::from(if h >= 2 { h - 1 } else { h + 1 }),
    );
    screen::feed(&mut ui, Input::default());
    let live = ui.project().orbit().live.rings[1].ring.hits_mask();
    assert_ne!(live, runner(R[1]));
    for r in R {
        let want = if r == R[1] { live } else { runner(r) };
        assert_eq!(
            drawn(&ui, r),
            want,
            "ring {}: ring 2's HITS unacked",
            r.get() + 1
        );
    }
    v.acked = ui.orbit().log().head();
    ui.set_orbit_view(&v);
    for r in R {
        assert_eq!(drawn(&ui, r), runner(r), "ring {}: acked", r.get() + 1);
    }
}
