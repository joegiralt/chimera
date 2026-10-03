//! `OrbitRunner` (Task 5): the transport on edges, the working copy and its
//! drift, and a project load, through `Engine::block`'s order.

mod common;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::orbit::euclid::{Fill, Steps};
use chimera_core::orbit::ring::{Octave, Ring};
use chimera_core::orbit::rng::Pct;
use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
use chimera_core::orbit::scene::Scene;
use chimera_core::project::{LoadGate, LoadLink};
use chimera_core::scope::ScopeWriter;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use common::{SR, scope_writer};

/// `Engine::block`'s order: the gate, the runner, the render.
struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    shared: Box<AudioShared>,
    gate: LoadGate,
    link: LoadLink,
    runner: Box<OrbitRunner>,
}

impl Rig {
    fn new(scene: Scene) -> Rig {
        let mut shared = Box::<AudioShared>::default();
        shared.orbit.settings = scene;
        Rig {
            inst: Box::new(Instrument::new(SR, SampleBudget::for_cpu(CPU_HZ_REV_V))),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: scope_writer(),
            shared,
            gate: LoadGate::new(),
            link: LoadLink::new(),
            runner: Box::new(OrbitRunner::new(core::num::NonZeroU32::new(SR).unwrap())),
        }
    }

    fn play(&mut self, on: bool) {
        self.shared.orbit.run = on;
    }

    fn block(&mut self) {
        if self
            .gate
            .before_block(&self.link, &mut self.inst, &self.shared)
        {
            self.runner.block(&mut self.inst, &self.shared);
        }
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
    }

    fn run(&mut self, blocks: usize) {
        for _ in 0..blocks {
            self.block();
        }
    }

    /// The held notes, sorted: what is sounding by key.
    fn held(&self) -> Vec<String> {
        let mut h: Vec<String> = self
            .inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| s.held())
            .map(|s| format!("{:?}", s.note()))
            .collect();
        h.sort();
        h
    }

    fn view(&self) -> OrbitView {
        self.runner.view()
    }

    fn live(&mut self, ch: u8, note: u8, on: bool) {
        let ev = NoteEvent {
            channel: MidiChannel::new(ch).unwrap(),
            note: MidiNote::new(note).unwrap(),
            kind: if on {
                NoteKind::On(Velocity::DEFAULT)
            } else {
                NoteKind::Off
            },
        };
        self.inst.handle(ev, &self.shared);
    }
}

fn fill(steps: u8, hits: u8, rotate: u8) -> Fill {
    Fill::new(Steps::new(steps).unwrap(), hits, rotate).unwrap()
}

/// Ring `i` of `s`: `steps` steps with `hits` hits, unmuted, gate `gate` %.
fn voice(s: &mut Scene, i: usize, steps: u8, hits: u8, gate: u8) {
    let r = &mut s.rings[i];
    r.ring = Ring::new(fill(steps, hits, 0), Octave::new(4).unwrap());
    r.ring.mute = false;
    r.ring.gate = Pct::clamped(gate);
}

#[test]
fn stop_releases_only_orbit() {
    let mut s = Scene::neutral();
    for i in 0..3 {
        voice(&mut s, i, 1, 1, 100);
    }
    let mut r = Rig::new(s);
    r.play(true);
    r.live(4, 40, true);
    r.live(5, 41, true);
    r.run(10);
    assert_eq!(
        r.held().len(),
        5,
        "three ORBIT notes and two keys: {:?}",
        r.held()
    );
    r.play(false);
    r.block();
    let held = r.held();
    assert_eq!(
        held.len(),
        2,
        "STOP released exactly ORBIT's three: {held:?}"
    );
    assert!(held.iter().all(|h| h.contains("40") || h.contains("41")));
    assert!(!r.view().running);
}

#[test]
fn play_starts_at_the_loop_start() {
    let mut r = Rig::new(Scene::init());
    let take = |r: &mut Rig| {
        r.play(true);
        let mut sig = Vec::new();
        for _ in 0..1400 {
            r.block();
            sig.push(r.held());
        }
        sig
    };
    let first = take(&mut r);
    r.run(850); // 1.5 loops in all
    r.play(false);
    r.run(2000); // tails out
    let second = take(&mut r);
    assert!(first.iter().any(|h| !h.is_empty()), "something played");
    assert_eq!(first, second);
}

#[test]
fn edits_keep_the_drift() {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 16, 5, 50);
    s.rings[0].ring.mutate = Pct::FULL;
    s.rings[0].ring.mute = true;
    let mut r = Rig::new(s);
    r.shared.orbit.bpm = chimera_core::orbit::clock::Bpm::new(300).unwrap();
    r.play(true);
    // 300 BPM: a bar is 600 blocks.
    r.run(6005);
    let fresh = fill(16, 5, 0).mask();
    let drifted = r.view().rings[0].hits;
    assert_ne!(drifted, fresh, "ten loops of MUTATE moved a hit");

    r.shared.orbit.settings.rings[0].ring.chance = Pct::clamped(50);
    r.run(2);
    assert_eq!(r.view().rings[0].hits, drifted, "CHANCE keeps the drift");

    let sr = &mut r.shared.orbit.settings.rings[0];
    sr.ring = sr.ring.with_fill(fill(12, 4, 0));
    sr.rhythm_rev = sr.rhythm_rev.wrapping_add(1);
    r.run(2);
    assert_eq!(
        r.view().rings[0].hits,
        fill(12, 4, 0).mask(),
        "STEPS re-lays it"
    );
}

#[test]
fn no_knob_copy_while_cued() {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 4, 4, 20);
    let mut r = Rig::new(s);
    r.play(true);
    r.run(100);
    let fired = r.view().rings[0].fired;
    r.shared.orbit.scene_rev += 1;
    r.shared.orbit.settings.rings[0].ring.mute = true;
    r.run(1);
    assert!(r.view().queued.is_some());
    // A bar is 1500 blocks at 120 BPM; the steps at 0.25, 0.5, 0.75 still sound.
    r.run(1300);
    assert_eq!(
        r.view().rings[0].fired.wrapping_sub(fired),
        3,
        "knob unchanged"
    );
    assert!(r.view().queued.is_some());
    r.run(200);
    assert!(r.view().queued.is_none(), "adopted at the boundary");
    let at = r.view().rings[0].fired;
    r.run(1400);
    assert_eq!(
        r.view().rings[0].fired,
        at,
        "the muted scene is the whole of settings"
    );
}

#[test]
fn a_project_load_stops_orbit_cleanly() {
    let mut r = Rig::new(Scene::init());
    r.play(true);
    r.run(300);
    assert!(!r.held().is_empty());
    let _swap = r.link.bump_for_test();
    r.shared.epoch = r.link.epoch();
    let mut quiet_at = None;
    for i in 0..100 {
        r.block();
        if quiet_at.is_none() && r.inst.quiet() {
            quiet_at = Some(i);
        }
        if quiet_at.is_some() {
            assert!(r.inst.quiet(), "quiet stays quiet, block {i}");
            assert!(r.held().is_empty(), "no ORBIT On, block {i}");
        }
        assert!(!r.view().running);
    }
    assert!(
        quiet_at.is_some_and(|i| i < 40),
        "quiet within the fade: {quiet_at:?}"
    );
    // A fresh false -> true edge starts it again.
    r.play(false);
    r.block();
    r.play(true);
    r.run(3);
    assert!(r.view().running);
}
