//! Capturing notes and chords (spec § 5.3, plan Task 14): what the keys
//! play while LEARN or PLAY is armed becomes a ring's notes or a
//! segment's chord. Through the QA rig: the drain's `live` hook, the
//! runner's view, and the UI taking it into the song.

mod common;
mod screen;

use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::euclid::{Fill, Steps};
use chimera_core::orbit::ring::NoteMode;
use chimera_core::orbit::runner::{Capture, OrbitRunner};
use chimera_core::triple::TripleBuffer;
use chimera_core::ui::UiState;
use chimera_core::ui::orbit::OrbitCell;
use chimera_core::ui::orbit::draw::{OrbitFrame, cell_text};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::ButtonId;
use chimera_hal::EncoderId::{D, F};
use chimera_theory::{Chord, Note, PitchClass};
use common::orbit::{OrbitRig, secs};
use screen::{Input, feed};

const R: [RingIdx; 5] = RingIdx::ALL;

fn notes(ns: &[u8]) -> Vec<Note> {
    ns.iter().map(|&n| Note::new(n).unwrap()).collect()
}

fn pool(rig: &mut OrbitRig, r: usize) -> Vec<u8> {
    let p = rig.song().live.rings[r].ring.pool;
    p.notes().iter().map(|n| n.get()).collect()
}

/// Keys held one after another, `gap` blocks apart, then all released.
fn chord(rig: &mut OrbitRig, keys: &[u8], gap: usize) {
    for &k in keys {
        rig.key(0, k, true);
        rig.run(gap);
    }
    for &k in keys {
        rig.key(0, k, false);
    }
    rig.run(2);
}

/// E4, C4 and A3 pressed over 300 ms, the lowest last, and released: the
/// chord over its lowest note, A minor, overrides segment 2; the PLAY
/// that armed it disarms.
#[test]
fn capture_chord_over_300_ms() {
    let mut rig = OrbitRig::new();
    rig.ui.arm(Capture::Segment(1));
    rig.run(1);
    chord(&mut rig, &[64, 60, 57], secs(0.1));
    let want = Chord::from_notes(&notes(&[57, 60, 64])).unwrap();
    let c = rig.song().live.chords;
    assert_eq!(c.played[1], Some(want));
    assert_eq!(want.bass(), PitchClass::A);
    assert_eq!(want.to_string(), "Am");
    assert!(
        c.played
            .iter()
            .enumerate()
            .all(|(i, p)| i == 1 || p.is_none())
    );
    assert_eq!(rig.ui.capture, Capture::Off, "disarmed");
    assert_eq!(rig.view().captured.count, 1);
}

/// One note, or one class in two octaves, is no chord: nothing is taken
/// and PLAY stays armed for the next one.
#[test]
fn capture_single_note_ignored() {
    let mut rig = OrbitRig::new();
    rig.ui.arm(Capture::Segment(0));
    rig.run(1);
    chord(&mut rig, &[60], 4);
    chord(&mut rig, &[60, 72], 4);
    assert_eq!(rig.song().live.chords.played[0], None);
    assert_eq!(rig.ui.capture, Capture::Segment(0), "still armed");
    assert_eq!(rig.view().captured.count, 0);
    chord(&mut rig, &[60, 64, 67], 4);
    let c = rig.song().live.chords.played[0].expect("the next chord taken");
    assert_eq!(c.to_string(), "C");
}

/// Nine keys held: the first eight pressed make the pool; the ninth is
/// dropped, though it sounds.
#[test]
fn capture_ninth_note_dropped() {
    let mut rig = OrbitRig::new();
    rig.ui.arm(Capture::Ring(R[1]));
    rig.run(1);
    let rev = rig.song().live.rings[1].notes_rev;
    chord(&mut rig, &[67, 60, 61, 62, 63, 64, 65, 66, 68], 2);
    assert_eq!(pool(&mut rig, 1), [60, 61, 62, 63, 64, 65, 66, 67]);
    let r = rig.song().live.rings[1];
    assert_eq!(r.notes_rev, rev.wrapping_add(1), "notes_rev bumped");
    let cap = rig.view().captured;
    assert_eq!((cap.len, cap.target), (8, Capture::Ring(R[1])));
}

/// A POOL capture sorts the notes, and OCTAVE becomes the lowest's: A2
/// (45) is octave 2, MIDI 36-47. Repeats count once.
#[test]
fn capture_pool_sets_octave() {
    let mut rig = OrbitRig::new();
    assert_eq!(rig.song().live.rings[1].ring.octave.get(), 4);
    rig.ui.arm(Capture::Ring(R[1]));
    rig.run(1);
    rig.key(0, 50, true);
    rig.run(2);
    rig.key(0, 45, true);
    rig.key(0, 57, true);
    rig.run(2);
    // 50 again while 45 and 57 hold: a repeat.
    rig.key(0, 50, false);
    rig.run(1);
    rig.key(0, 50, true);
    rig.run(1);
    for k in [45, 50, 57] {
        rig.key(0, k, false);
    }
    rig.run(2);
    assert_eq!(pool(&mut rig, 1), [45, 50, 57]);
    assert_eq!(rig.song().live.rings[1].ring.octave.get(), 2);
    // The other rings are untouched.
    let init = chimera_core::orbit::scene::Scene::init();
    for i in [0, 2, 3, 4] {
        assert_eq!(rig.song().live.rings[i], init.rings[i], "ring {}", i + 1);
    }
}

/// A STEP ring of E(3,8): three notes, each its own key-down and key-up,
/// fill the hits in the order played, and the rests after each hit take
/// its note. The third note ends it: a fourth changes nothing. Rotated so
/// step 1 rests, the steps before the first hit take the last hit's note.
#[test]
fn capture_step_fills_hits_in_order() {
    for rotate in [0, 1] {
        let mut rig = OrbitRig::new();
        {
            let s = &mut rig.song().live.rings[1];
            s.ring.mode = NoteMode::Step;
            let fill = Fill::new(Steps::new(8).unwrap(), 3, rotate).unwrap();
            s.ring = s.ring.with_fill(fill);
            s.rhythm_rev = s.rhythm_rev.wrapping_add(1);
        }
        let mask = rig.song().live.rings[1].ring.hits_mask();
        rig.ui.arm(Capture::Ring(R[1]));
        rig.run(1);
        for k in [64, 60, 67, 71] {
            rig.key(0, k, true);
            rig.run(2);
            rig.key(0, k, false);
            rig.run(2);
        }
        let played = [64u8, 60, 67];
        let last = (0..8).rev().find(|&i| mask.is_hit(i)).unwrap();
        let want: Vec<u8> = (0..8)
            .map(|i| {
                let at = (0..=i).rev().find(|&j| mask.is_hit(j)).unwrap_or(last);
                played[mask.hit_index(at).unwrap()]
            })
            .collect();
        if rotate == 0 {
            assert_eq!(want, [64, 64, 64, 60, 60, 60, 67, 67]);
        } else {
            assert!(!mask.is_hit(0), "step 1 rests");
        }
        let got: Vec<u8> = rig.song().live.rings[1].ring.notes[..8]
            .iter()
            .map(|n| n.get())
            .collect();
        assert_eq!(got, want, "ROTATE {rotate}");
        assert_eq!(rig.song().live.rings[1].ring.octave.get(), 4, "C4's octave");
        assert_eq!(rig.view().captured.count, 1, "the fourth note took nothing");
        assert_eq!(rig.ui.capture, Capture::Off);
    }
}

/// The keys sound through a capture exactly as with nothing armed, voice
/// for voice and sample for sample.
#[test]
fn capture_notes_still_sound() {
    let render = |arm: Capture| {
        let mut rig = OrbitRig::new();
        rig.ui.arm(arm);
        rig.run(1);
        for k in [57, 60, 64] {
            rig.key(0, k, true);
        }
        rig.run(secs(0.3));
        let held = *rig.tape.busy.last().unwrap();
        for k in [57, 60, 64] {
            rig.key(0, k, false);
        }
        rig.run(secs(0.2));
        assert!(rig.stuck_notes().is_empty(), "{:?}", rig.stuck_notes());
        (held, rig.sum.clone(), rig.song().live.chords.played[0])
    };
    let (held, armed, took) = render(Capture::Segment(0));
    let (_, plain, none) = render(Capture::Off);
    assert!(held >= 3, "three voices held, got {held}");
    assert!(took.is_some() && none.is_none());
    let peak = armed.iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.01, "the chord sounds: peak {peak}");
    assert!(armed == plain, "a capture leaves the sound alone");
}

/// The runner on its own: a key held before arming plays no part, and
/// a capture is taken once, whatever the slot it reads says after.
#[test]
fn runner_takes_one_capture_per_arming() {
    let sr = core::num::NonZeroU32::new(chimera_hal::SAMPLE_RATE).unwrap();
    let mut runner = OrbitRunner::new(sr);
    let mut inst = Box::new(Instrument::new(
        chimera_hal::SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let mut shared = Box::new(AudioShared::default());
    let ev = |n: u8, on: bool| NoteEvent {
        channel: chimera_core::MidiChannel::new(1).unwrap(),
        note: MidiNote::new(n).unwrap(),
        kind: if on {
            NoteKind::On(Velocity::DEFAULT)
        } else {
            NoteKind::Off
        },
    };
    // Held before arming: never part of it, its key-up ends nothing.
    runner.live(ev(48, true));
    shared.orbit.capture = Capture::Segment(2);
    shared.orbit.arming = 1;
    runner.block(&mut inst, &shared);
    runner.live(ev(60, true));
    runner.live(ev(48, false));
    runner.live(ev(64, true));
    assert_eq!(runner.view().captured.count, 0, "60 and 64 still held");
    runner.live(ev(60, false));
    runner.live(ev(64, false));
    let cap = runner.view().captured;
    assert_eq!(
        (cap.count, cap.target, cap.len),
        (1, Capture::Segment(2), 2)
    );
    assert_eq!(&cap.notes[..2], &notes(&[60, 64])[..]);
    // Still armed in the slot, the same arming: no second capture.
    for _ in 0..4 {
        runner.block(&mut inst, &shared);
        runner.live(ev(62, true));
        runner.live(ev(65, true));
        runner.live(ev(62, false));
        runner.live(ev(65, false));
    }
    assert_eq!(runner.view().captured.count, 1);
    let chord = |runner: &mut OrbitRunner| {
        for (n, on) in [(62, true), (65, true), (62, false), (65, false)] {
            runner.live(ev(n, on));
        }
        runner.view().captured.count
    };
    // The same target armed anew, with no Off between: a new arming.
    shared.orbit.arming = 2;
    runner.block(&mut inst, &shared);
    assert_eq!(chord(&mut runner), 2, "re-armed");
    // Off, the arming unchanged, disarms; armed again takes again.
    shared.orbit.arming = 3;
    runner.block(&mut inst, &shared);
    shared.orbit.capture = Capture::Off;
    runner.block(&mut inst, &shared);
    assert_eq!(chord(&mut runner), 2, "disarmed");
    (shared.orbit.capture, shared.orbit.arming) = (Capture::Segment(2), 4);
    runner.block(&mut inst, &shared);
    assert_eq!(chord(&mut runner), 3);
}

/// The UI and runner joined through a real triple buffer, as the desktop
/// joins them: PLAY armed on CHRD, a chord, then frame after frame with
/// nothing new. The chord is taken once; `capture` is written on every
/// publish, so a stale slot never re-arms it, and a later chord is no
/// capture.
#[test]
fn capture_survives_the_triple_buffer() {
    let buf = Box::leak(Box::new(TripleBuffer::new(
        AudioShared::default(),
        AudioShared::default(),
        AudioShared::default(),
    )));
    let (mut w, mut r) = buf.split();
    let sr = core::num::NonZeroU32::new(chimera_hal::SAMPLE_RATE).unwrap();
    let mut runner = OrbitRunner::new(sr);
    let mut inst = Box::new(Instrument::new(
        chimera_hal::SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let mut ui = Box::new(UiState::new());
    screen::to_ring(&mut ui, 1);
    screen::plus(&mut ui, 1);
    let mut frame = |ui: &mut UiState, input: Input, keys: &[(u8, bool)]| {
        ui.set_orbit_view(&runner.view());
        feed(ui, input);
        w.publish(|b| {
            b.update_from(ui.project().perf(), 0);
            ui.publish_orbit(&mut b.orbit);
        });
        let shared = r.read();
        runner.block(&mut inst, shared);
        for &(n, on) in keys {
            runner.live(NoteEvent {
                channel: chimera_core::MidiChannel::new(1).unwrap(),
                note: MidiNote::new(n).unwrap(),
                kind: if on {
                    NoteKind::On(Velocity::DEFAULT)
                } else {
                    NoteKind::Off
                },
            });
        }
        runner.view().captured.count
    };
    let text = |ui: &UiState| {
        let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
        cell_text(OrbitCell::Play, &f, R[0]).as_str().to_string()
    };
    assert_eq!(text(&ui), "-");
    frame(&mut ui, Input::turn(F, 1), &[]);
    assert_eq!(ui.orbit().capture, Capture::Segment(0));
    assert_eq!(text(&ui), "HOLD");
    frame(&mut ui, Input::default(), &[]);
    let count = frame(
        &mut ui,
        Input::default(),
        &[
            (57, true),
            (60, true),
            (64, true),
            (57, false),
            (60, false),
            (64, false),
        ],
    );
    assert_eq!(count, 1);
    for _ in 0..8 {
        assert_eq!(
            frame(&mut ui, Input::default(), &[]),
            1,
            "no spurious capture"
        );
    }
    assert_eq!(ui.orbit().capture, Capture::Off);
    assert_eq!(text(&ui), "Am");
    let am = ui.project().orbit().live.chords.played[0];
    let count = frame(
        &mut ui,
        Input::default(),
        &[
            (60, true),
            (64, true),
            (67, true),
            (60, false),
            (64, false),
            (67, false),
        ],
    );
    assert_eq!(count, 1, "disarmed: C major is no capture");
    frame(&mut ui, Input::default(), &[]);
    assert_eq!(ui.project().orbit().live.chords.played[0], am);
    // PLAY armed over the played chord, then a quick two-detent flick
    // down over two frames: disarmed, the chord kept.
    frame(&mut ui, Input::turn(F, 1), &[]);
    assert_eq!(text(&ui), "HOLD");
    frame(&mut ui, Input::turn(F, -1), &[]);
    frame(&mut ui, Input::turn(F, -1), &[]);
    assert_eq!(ui.orbit().capture, Capture::Off);
    assert_eq!(ui.project().orbit().live.chords.played[0], am);
    assert_eq!(text(&ui), "Am");
    // MIX + down clears it: the degree plays again.
    frame(&mut ui, Input::turn(F, -1).and_held(ButtonId::Mix), &[]);
    assert_eq!(ui.project().orbit().live.chords.played[0], None);
    assert_eq!(text(&ui), "-");

    // A capture taken in the frame PLAY is turned up again: the take's
    // disarm and the new arming land together, and the next chord is
    // still captured.
    frame(&mut ui, Input::turn(F, 1), &[]);
    let n = frame(
        &mut ui,
        Input::default(),
        &[
            (57, true),
            (60, true),
            (64, true),
            (57, false),
            (60, false),
            (64, false),
        ],
    );
    assert_eq!(n, 2);
    frame(&mut ui, Input::turn(F, 1), &[]);
    assert_eq!(
        ui.orbit().capture,
        Capture::Segment(0),
        "re-armed in that frame"
    );
    assert_eq!(text(&ui), "HOLD");
    let n = frame(
        &mut ui,
        Input::default(),
        &[
            (60, true),
            (64, true),
            (67, true),
            (60, false),
            (64, false),
            (67, false),
        ],
    );
    assert_eq!(n, 3, "the next chord is captured");
    frame(&mut ui, Input::default(), &[]);
    let c = ui.project().orbit().live.chords.played[0].unwrap();
    assert_eq!(c.to_string(), "C");
}

/// SEGS turned below an armed segment drops the capture: nothing is
/// written to a hidden segment.
#[test]
fn segs_shrinking_disarms_a_segment() {
    use chimera_hal::EncoderId::E;
    let mut ui = Box::new(UiState::new());
    screen::to_ring(&mut ui, 1);
    screen::plus(&mut ui, 1);
    feed(&mut ui, Input::turn(E, 3).and_held(ButtonId::Mix));
    feed(&mut ui, Input::turn(F, 1));
    assert_eq!(ui.orbit().capture, Capture::Segment(3));
    // SEGS (D) from 4 to 3.
    feed(&mut ui, Input::turn(D, -1));
    assert_eq!(ui.project().orbit().live.chords.segs.get(), 3);
    assert_eq!(ui.orbit().capture, Capture::Off);
    // A capture already reported into it is not written past SEGS.
    let mut s = ui.project().orbit().live;
    let mut c = chimera_core::orbit::runner::OrbitView::default().captured;
    c.target = Capture::Segment(3);
    c.len = 3;
    c.notes[..3].copy_from_slice(&notes(&[57, 60, 64]));
    s.take(&c);
    assert_eq!(s.chords.played, ui.project().orbit().live.chords.played);
}

/// LEARN arms the ring it is turned on, shows ON there and OFF on the
/// others; turned down it disarms. Arming one target drops the other.
#[test]
fn learn_arms_the_selected_ring() {
    let mut ui = Box::new(UiState::new());
    screen::to_ring(&mut ui, 2);
    screen::tap(&mut ui, ButtonId::Edit);
    screen::tap(&mut ui, ButtonId::Edit);
    let text = |ui: &UiState, r: RingIdx| {
        let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
        cell_text(OrbitCell::Learn, &f, r).as_str().to_string()
    };
    let before = ui.project().orbit().live;
    feed(&mut ui, Input::turn(D, 1));
    assert_eq!(ui.orbit().capture, Capture::Ring(R[1]));
    assert_eq!(
        (text(&ui, R[1]), text(&ui, R[2])),
        ("ON".into(), "OFF".into())
    );
    assert_eq!(ui.project().orbit().live, before, "arming edits nothing");
    feed(&mut ui, Input::turn(D, -1));
    assert_eq!(ui.orbit().capture, Capture::Off);
    feed(&mut ui, Input::turn(D, 2));
    assert_eq!(ui.orbit().capture, Capture::Ring(R[1]));
}

/// A load drops an armed capture, and one the runner reported from before
/// it: the new project is not written with the old one's keys.
#[test]
fn a_load_drops_the_capture() {
    use chimera_core::orbit::runner::OrbitView;
    use chimera_core::ui::orbit::OrbitUi;
    let mut ui = OrbitUi::new();
    ui.arm(Capture::Ring(R[0]));
    ui.loaded(1);
    assert_eq!(ui.capture, Capture::Off);
    let mut v = OrbitView::default();
    v.captured.count = 1;
    v.captured.target = Capture::Ring(R[0]);
    assert!(ui.take(&v).is_none(), "from before the load");
    v.epoch = 1;
    assert!(ui.take(&v).is_none(), "taken once, never again");
    v.captured.count = 2;
    assert!(ui.take(&v).is_some());
}
