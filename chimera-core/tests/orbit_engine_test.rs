use chimera_core::MidiChannel;
use chimera_core::hw::{BLOCK_SIZE, MAX_PARTS};
use chimera_core::orbit::chords::{Bars, Segs, harmony};
use chimera_core::orbit::clock::{Bpm, InternalClock, Position, step_start};
use chimera_core::orbit::engine::{Events, OrbitEvent, OrbitKind, OrbitState, Span, block};
use chimera_core::orbit::euclid::{Fill, Steps, euclid};
use chimera_core::orbit::ring::SnapTo;
use chimera_core::orbit::rng::Pct;
use chimera_core::orbit::scene::Scene;
use chimera_core::orbit::walk::{Pool, Walk};
use chimera_core::orbit::{RINGS, RingIdx};
use chimera_theory::{Degree, Note, snap};
use core::num::NonZeroU32;
use std::collections::HashSet;

const SR: NonZeroU32 = match NonZeroU32::new(48_000) {
    Some(n) => n,
    None => panic!(),
};

const _: Scene = Scene::init();
const _: Scene = Scene::neutral();

#[derive(Clone, Copy)]
struct Rec {
    t: u64,
    span: Span,
    ev: OrbitEvent,
}

/// Drives `block` as the runner will: half a block either side of `t`.
struct Rig {
    scene: Scene,
    st: OrbitState,
    clock: InternalClock,
    edge: Position,
    t: u64,
    channels: [MidiChannel; MAX_PARTS],
    bpm: Option<Bpm>,
    log: Vec<Rec>,
    spans: Vec<Span>,
}

impl Rig {
    fn new(scene: Scene, bpm: u16) -> Rig {
        Rig {
            st: OrbitState::start(&scene),
            scene,
            clock: InternalClock::start(0, Bpm::new(bpm).unwrap(), SR),
            edge: Position::ZERO,
            t: 0,
            channels: core::array::from_fn(|i| MidiChannel::new(i as u8 + 1).unwrap()),
            bpm: None,
            log: Vec::new(),
            spans: Vec::new(),
        }
    }

    fn set_bpm(&mut self, b: u16) {
        self.bpm = Some(Bpm::new(b).unwrap());
    }

    fn block(&mut self, cue: Option<&Scene>) -> Events {
        let from_sample = self.t.saturating_sub(32).max(self.clock.anchor());
        if let Some(b) = self.bpm.take() {
            self.clock.set_bpm(from_sample, self.edge, b);
        }
        let to = self.clock.position(self.t + 32);
        let span = Span {
            from: self.edge,
            to,
            from_sample,
            to_sample: self.t + 32,
            bar_len: self.clock.bar_len(),
        };
        let ev = block(&mut self.st, &mut self.scene, cue, span, &self.channels);
        for e in ev.iter() {
            self.log.push(Rec {
                t: self.t,
                span,
                ev: e,
            });
        }
        self.spans.push(span);
        self.edge = to;
        self.t += 64;
        ev
    }

    fn run_bars(&mut self, bars: u32) {
        let end = Position {
            bar: bars,
            phase: chimera_core::orbit::clock::Phase::ZERO,
        };
        while self.edge < end {
            self.block(None);
        }
    }

    fn events(&self, ring: usize) -> Vec<(u64, OrbitEvent)> {
        self.log
            .iter()
            .filter(|r| r.ev.ring.get() as usize == ring)
            .map(|r| (r.t, r.ev))
            .collect()
    }

    fn ons(&self, ring: usize) -> Vec<Rec> {
        self.log
            .iter()
            .filter(|r| r.ev.ring.get() as usize == ring && matches!(r.ev.kind, OrbitKind::On(_)))
            .copied()
            .collect()
    }
}

fn is_on(e: &OrbitEvent) -> bool {
    matches!(e.kind, OrbitKind::On(_))
}

/// The (bar, step) of an On of a ring with `steps` steps.
fn locate(r: &Rec, steps: Steps) -> (u32, u8) {
    for bar in r.span.from.bar..=r.span.to.bar {
        for k in 0..steps.get() {
            let p = Position {
                bar,
                phase: step_start(k, steps),
            };
            if p >= r.span.from && p < r.span.to {
                return (bar, k);
            }
        }
    }
    panic!("no step in the window");
}

fn steps(n: u8) -> Steps {
    Steps::new(n).unwrap()
}

fn fill(n: u8, hits: u8) -> Fill {
    Fill::new(steps(n), hits, 0).unwrap()
}

/// Ring `i` unmuted: `hits` in `n`, one pool note.
fn voice(s: &mut Scene, i: usize, n: u8, hits: u8, note: u8) {
    let r = &mut s.rings[i].ring;
    *r = r.with_fill(fill(n, hits));
    r.mute = false;
    r.pool = Pool::one(Note::new(note).unwrap());
}

const C_MAJOR: [u8; RINGS] = [60, 62, 64, 65, 67];

/// Every ring full, on a C major note of its own.
fn full(n: [u8; RINGS], notes: [u8; RINGS]) -> Scene {
    let mut s = Scene::neutral();
    for i in 0..RINGS {
        voice(&mut s, i, n[i], n[i], notes[i]);
    }
    s
}

fn lcg(x: &mut u32) -> u32 {
    *x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *x >> 8
}

/// The init scene, every ring on and a varied dice.
fn lively() -> Scene {
    let mut s = Scene::init();
    for r in s.rings.iter_mut() {
        r.ring.mute = false;
        r.ring.chance = Pct::clamped(70);
        r.ring.mutate = Pct::clamped(60);
        r.ring.walk = Walk::Random;
    }
    s
}

#[test]
fn start_block_fires_step_zero() {
    let mut rig = Rig::new(full([4, 3, 5, 8, 16], C_MAJOR), 120);
    let ev = rig.block(None);
    let rings: Vec<usize> = ev
        .iter()
        .filter(is_on)
        .map(|e| e.ring.get() as usize)
        .collect();
    assert_eq!(rings, [0, 1, 2, 3, 4]);
    let mut init = Rig::new(Scene::init(), 120);
    let first: Vec<usize> = init
        .block(None)
        .iter()
        .map(|e| e.ring.get() as usize)
        .collect();
    assert!(first.contains(&0) && first.contains(&1));
    assert!(!first.contains(&4), "ring 5 is muted");
}

#[test]
fn set_bpm_keeps_windows_contiguous() {
    let mut rig = Rig::new(Scene::init(), 120);
    let mut x = 7;
    for _ in 0..1000 {
        rig.set_bpm(30 + (lcg(&mut x) % 271) as u16);
        rig.block(None);
    }
    for w in rig.spans.windows(2) {
        assert_eq!(w[1].from, w[0].to);
    }
}

#[test]
fn bpm_sweep_fires_every_step_once() {
    let n = [4, 3, 5, 8, 16];
    let mut rig = Rig::new(full(n, C_MAJOR), 60);
    let end = Position {
        bar: 50,
        phase: chimera_core::orbit::clock::Phase::ZERO,
    };
    let mut blocks = 0u32;
    let mut peak = 0;
    while rig.edge < end {
        if blocks.is_multiple_of(7) {
            let up = if rig.edge.bar < 25 {
                rig.edge.bar
            } else {
                50 - rig.edge.bar
            };
            rig.set_bpm(60 + (240 * up / 25) as u16);
            peak = peak.max(60 + (240 * up / 25) as u16);
        }
        rig.block(None);
        blocks += 1;
    }
    assert_eq!(peak, 300);
    for (i, &steps_i) in n.iter().enumerate() {
        let seen: Vec<(u32, u8)> = rig
            .ons(i)
            .iter()
            .filter(|r| r.span.to <= end)
            .map(|r| locate(r, steps(steps_i)))
            .collect();
        let set: HashSet<_> = seen.iter().copied().collect();
        assert_eq!(seen.len(), 50 * steps_i as usize, "ring {i}");
        assert_eq!(set.len(), seen.len(), "ring {i} fired a step twice");
    }
}

#[test]
fn steps_fire_within_half_a_block() {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 13, 13, 60);
    let mut rig = Rig::new(s, 137);
    rig.run_bars(100);
    let ons = rig.ons(0);
    assert!(ons.len() >= 1300);
    for r in &ons {
        let (bar, k) = locate(r, steps(13));
        let at = rig.clock.sample_of(Position {
            bar,
            phase: step_start(k, steps(13)),
        });
        assert!(
            r.t.abs_diff(at) <= 32,
            "bar {bar} step {k}: block {} step {at}",
            r.t
        );
    }
}

#[test]
fn gate_off_precedes_the_next_on() {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 16, 16, 60);
    s.rings[0].ring.gate = Pct::FULL;
    let mut rig = Rig::new(s, 120);
    rig.run_bars(8);
    let ev = rig.events(0);
    assert!(ev.len() > 100);
    for (i, (_, e)) in ev.iter().enumerate() {
        assert_eq!(
            is_on(e),
            i % 2 == 0,
            "On and Off alternate, On first: event {i}"
        );
    }
}

#[test]
fn gate_zero_never_sticks() {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 32, 32, 60);
    s.rings[0].ring.gate = Pct::ZERO;
    let mut rig = Rig::new(s, 300);
    rig.run_bars(20);
    let ev = rig.events(0);
    assert!(ev.len() >= 1200);
    for pair in ev.chunks(2).filter(|p| p.len() == 2) {
        assert!(is_on(&pair[0].1) && !is_on(&pair[1].1));
        assert!(pair[1].0 > pair[0].0, "an Off in its On's block");
    }
    // The 96-sample floor, quantised: 64 or 128, both seen.
    let gaps: HashSet<u64> = ev
        .chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| p[1].0 - p[0].0)
        .collect();
    assert_eq!(gaps, HashSet::from([64, 128]));
    let ons = ev.iter().filter(|(_, e)| is_on(e)).count();
    assert!(ons - (ev.len() - ons) <= 1);
}

#[test]
fn pending_off_keeps_its_channel() {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 4, 4, 60);
    let mut rig = Rig::new(s, 120);
    while rig.log.is_empty() {
        rig.block(None);
    }
    let on = rig.log[0].ev;
    assert_eq!(on.channel, MidiChannel::new(1).unwrap());
    rig.channels[0] = MidiChannel::new(5).unwrap();
    while rig.log.len() < 2 {
        rig.block(None);
    }
    let off = rig.log[1].ev;
    assert_eq!(off.kind, OrbitKind::Off);
    assert_eq!(off.channel, MidiChannel::new(1).unwrap());
    assert_eq!(off.note, on.note);
}

fn mid_gate(edit: impl FnOnce(&mut Scene)) {
    let mut s = Scene::neutral();
    voice(&mut s, 0, 13, 13, 60);
    let mut rig = Rig::new(s, 120);
    while rig.ons(0).len() < 13 {
        rig.block(None);
    }
    let last = *rig.ons(0).last().unwrap();
    assert_eq!(locate(&last, steps(13)), (0, 12));
    let s12 = rig.clock.sample_of(Position {
        bar: 0,
        phase: step_start(12, steps(13)),
    });
    let at = s12 + (rig.clock.bar_len() as u64 / 13) / 2;
    edit(&mut rig.scene);
    let n = rig.log.len();
    while rig.log.len() == n {
        rig.block(None);
    }
    let r = rig.log[n];
    assert_eq!(r.ev.kind, OrbitKind::Off);
    assert!(
        at + 32 >= r.t && at < r.t + 32,
        "off due {at}, sent in block {}",
        r.t
    );
}

#[test]
fn edits_mid_gate_never_strand_a_note() {
    mid_gate(|s| s.rings[0].ring = s.rings[0].ring.with_fill(fill(5, 5)));
    mid_gate(|s| s.rings[0].ring.mute = true);
}

#[test]
fn mute_changes_nothing_else() {
    let base = lively();
    let mut a = Rig::new(base, 120);
    a.run_bars(16);
    let mut muted = base;
    muted.rings[2].ring.mute = true;
    let mut b = Rig::new(muted, 120);
    b.run_bars(16);
    for i in [0, 1, 3, 4] {
        assert_eq!(a.events(i), b.events(i), "ring {i}");
    }
    assert!(b.events(2).is_empty());

    let mut c = Rig::new(muted, 120);
    let mut switched = None;
    while c.edge.bar < 16 {
        if switched.is_none() && c.edge.bar >= 8 {
            c.scene.rings[2].ring.mute = false;
            switched = Some(c.t);
        }
        c.block(None);
    }
    let from = switched.unwrap();
    let want: Vec<_> = a
        .events(2)
        .into_iter()
        .filter(|(t, e)| *t >= from && is_on(e))
        .collect();
    let got: Vec<_> = c.events(2).into_iter().filter(|(_, e)| is_on(e)).collect();
    assert!(!want.is_empty());
    assert_eq!(got, want);
}

#[test]
fn rings_have_their_own_dice() {
    let base = lively();
    let mut a = Rig::new(base, 120);
    a.run_bars(16);
    let mut other = base;
    other.rings[1].ring.chance = Pct::clamped(30);
    let mut b = Rig::new(other, 120);
    b.run_bars(16);
    assert_eq!(a.events(3), b.events(3));
    assert_ne!(a.events(1), b.events(1));
}

#[test]
fn loop_boundary_mutates_after_the_first_loop() {
    let mut s = Scene::neutral();
    for i in 0..RINGS {
        voice(&mut s, i, 13, 5, 60);
        let r = &mut s.rings[i].ring;
        r.pool = Pool::new(&[60, 64, 67].map(|n| Note::new(n).unwrap())).unwrap();
        r.mutate = Pct::FULL;
    }
    let start = s;
    let mut rig = Rig::new(s, 120);
    let mut snaps = Vec::new();
    while snaps.len() < 52 {
        if rig.block(None).boundary() {
            snaps.push(rig.scene.rings.map(|r| (r.ring, r.rhythm_rev, r.notes_rev)));
        }
    }
    for i in 0..RINGS {
        assert_eq!(
            snaps[0][i].0, start.rings[i].ring,
            "the first boundary is the start"
        );
    }
    for i in 0..RINGS {
        let changed = snaps
            .windows(2)
            .skip(1)
            .filter(|w| w[0][i].0 != w[1][i].0)
            .count();
        assert!(changed >= 40, "ring {i} changed on {changed} of 50");
        // A nudge bumps rhythm_rev, a swap notes_rev, and nothing else does.
        for w in snaps.windows(2).skip(1) {
            let ((a, ar, an), (b, br, bn)) = (w[0][i], w[1][i]);
            let nudged = a.hits_mask() != b.hits_mask();
            assert_eq!(br.wrapping_sub(ar), nudged as u8);
            // A swap may redraw the same note: then only the rev moves.
            let swapped = bn.wrapping_sub(an);
            assert!(
                swapped <= 1 && (!nudged || swapped == 0) && (a == b || nudged || swapped == 1)
            );
        }
        assert!(snaps.iter().all(|w| w[i].0.hits_mask().hits() == 5));
    }
}

#[test]
fn a_cued_scene_lands_on_the_boundary() {
    let mut a = full([4, 4, 4, 4, 16], [48, 50, 52, 53, 55]);
    a.chords.bars = Bars::new(2).unwrap();
    a.rings[0].ring.gate = Pct::FULL;
    let mut b = full([4, 4, 4, 4, 16], C_MAJOR);
    b.chords.bars = Bars::new(2).unwrap();
    b.seed = 99;
    for r in b.rings.iter_mut() {
        r.ring.mutate = Pct::FULL;
    }
    let solo: Vec<(usize, OrbitKind, Note)> = Rig::new(b, 120)
        .block(None)
        .iter()
        .map(|e| (e.ring.get() as usize, e.kind, e.note))
        .collect();
    assert_eq!(solo.len(), RINGS);

    let mut rig = Rig::new(a, 120);
    while rig.edge.bar < 1 {
        rig.block(None);
    }
    let old = rig.ons(0).last().unwrap().ev.note;
    assert!(old.get() < 56);
    loop {
        let ev = rig.block(Some(&b));
        if !ev.adopted() {
            assert!(
                rig.scene.rings[0].ring.pool.get(0).get() < 56,
                "the old scene plays to the boundary"
            );
            continue;
        }
        assert!(ev.boundary());
        assert_eq!(
            rig.scene.rings.map(|r| r.ring),
            b.rings.map(|r| r.ring),
            "unmutated"
        );
        let offs: Vec<_> = ev.iter().filter(|e| !is_on(e)).collect();
        assert!(
            offs.iter().any(|e| e.ring.get() == 0 && e.note == old),
            "the old pending off fires"
        );
        let ons: Vec<_> = ev
            .iter()
            .filter(is_on)
            .map(|e| (e.ring.get() as usize, e.kind, e.note))
            .collect();
        assert_eq!(ons, solo);
        break;
    }
}

#[test]
fn a_segment_boundary_inside_a_window() {
    let mut s = Scene::neutral();
    s.chords.bars = Bars::ONE;
    s.chords.segs = Segs::new(2).unwrap();
    s.chords.degrees[0] = Degree::I;
    s.chords.degrees[1] = Degree::V;
    voice(&mut s, 0, 31, 31, 62);
    voice(&mut s, 1, 2, 2, 62);
    for r in s.rings.iter_mut().take(2) {
        r.ring.snap = SnapTo::Chord;
    }
    let span = Span {
        from: Position {
            bar: 0,
            phase: step_start(15, steps(31)),
        },
        to: Position {
            bar: 0,
            phase: chimera_core::orbit::clock::Phase::new(step_start(1, steps(2)).get() + 1),
        },
        from_sample: 1000,
        to_sample: 1100,
        bar_len: 96_000,
    };
    let mut st = OrbitState::start(&s);
    let ch = core::array::from_fn(|i| MidiChannel::new(i as u8).unwrap());
    let ev = block(&mut st, &mut s, None, span, &ch);
    let notes: Vec<_> = ev.iter().map(|e| (e.ring.get(), e.note)).collect();
    let (c, g) = (harmony(&s.chords, 0).chord, harmony(&s.chords, 1).chord);
    let n62 = Note::new(62).unwrap();
    let want0 = snap(n62, chimera_theory::SnapTo::Chord(c));
    let want1 = snap(n62, chimera_theory::SnapTo::Chord(g));
    assert_ne!(want0, want1, "the chords differ under this note");
    assert_eq!(notes, [(0, want0), (1, want1)]);
    assert!(ev.seg_changed() && st.seg() == 1);
}

#[test]
fn with_mask_keeps_steps_and_count() {
    let ring = Scene::init().rings[1].ring;
    let m = ring.hits_mask();
    let from = (0..8).find(|&i| m.is_hit(i)).unwrap();
    let to = (0..8).find(|&i| !m.is_hit(i)).unwrap();
    let nudged = m.moved(from, to).unwrap();
    assert_eq!(ring.with_mask(nudged).unwrap().hits_mask(), nudged);
    assert!(ring.with_mask(euclid(steps(8), 4, 0)).is_none());
    assert!(ring.with_mask(euclid(steps(7), 3, 0)).is_none());
}

#[test]
fn stop_returns_every_pending_off() {
    let mut s = full([4, 3, 5, 8, 16], C_MAJOR);
    for r in s.rings.iter_mut() {
        r.ring.gate = Pct::FULL;
    }
    let mut rig = Rig::new(s, 120);
    for _ in 0..3 {
        rig.block(None);
    }
    let sounding = (0..RINGS)
        .filter(|&i| rig.events(i).last().is_some_and(|(_, e)| is_on(e)))
        .count();
    assert!(sounding > 0);
    let ev = rig.st.stop();
    assert_eq!(ev.len(), sounding);
    assert!(ev.iter().all(|e| e.kind == OrbitKind::Off));
    assert_eq!(rig.st.stop().len(), 0);
}

#[test]
fn scene_replays_identically() {
    let run = || {
        let mut rig = Rig::new(lively(), 150);
        rig.run_bars(64);
        rig.log.iter().map(|r| (r.t, r.ev)).collect::<Vec<_>>()
    };
    let (x, y) = (run(), run());
    assert!(x.len() > 500);
    assert_eq!(x, y);
}

#[test]
fn init_scene_is_the_design_choice() {
    let s = Scene::init();
    assert_eq!(s.seed, 0x0B17_0001);
    // The owner's pick at UAT (V4, sparse groove): ring 5 muted, to
    // bring in; rings 3 and 4 both the pad's, on Part 3.
    let muted: Vec<bool> = s.rings.iter().map(|r| r.ring.mute).collect();
    assert_eq!(muted, [false, false, false, false, true]);
    assert_eq!(s.chords.bars, Bars::new(4).unwrap());
    assert_eq!(s.chords.segs, Segs::new(4).unwrap());
    let parts: Vec<usize> = s.rings.iter().map(|r| r.target.part().index()).collect();
    assert_eq!(parts, [0, 1, 2, 2, 4]);
    for r in &s.rings {
        assert_eq!((r.ring.chance, r.ring.mutate), (Pct::FULL, Pct::ZERO));
    }
    let _ = RingIdx::ALL;
}

/// A load killed the voices: `forget` leaves `stop` nothing to release.
#[test]
fn forget_leaves_stop_no_offs() {
    let mut rig = Rig::new(full([1; RINGS], C_MAJOR), 120);
    rig.block(None);
    assert!(rig.log.iter().any(|r| is_on(&r.ev)), "notes are pending");
    let mut held = rig.st;
    assert!(!held.stop().is_empty(), "control: stop releases them");
    rig.st.forget();
    assert!(rig.st.stop().is_empty());
}

/// The init scene's first 8 bars, pinned: `block ring channel note kind`
/// per event, on NEW's channels. Re-record an intended change with
/// `GOLDEN_RECORD=1`, which writes the fixture.
#[test]
fn init_scene_events_golden() {
    let mut rig = Rig::new(Scene::init(), 120);
    rig.channels = core::array::from_fn(|i| MidiChannel::new(i as u8).unwrap());
    rig.run_bars(8);
    // Sorted within each block: the order of a block's events is not heard.
    let mut lines: Vec<(u64, String)> = rig
        .log
        .iter()
        .map(|r| {
            let kind = match r.ev.kind {
                OrbitKind::On(v) => format!("on {}", v.get()),
                OrbitKind::Off => "off".into(),
            };
            let block = r.t / BLOCK_SIZE as u64;
            let line = format!(
                "{block} {} {} {} {kind}\n",
                r.ev.ring.get() + 1,
                r.ev.channel.get() + 1,
                r.ev.note.get()
            );
            (block, line)
        })
        .collect();
    lines.sort();
    let got: String = lines.into_iter().map(|l| l.1).collect();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/orbit_init_events.txt"
    );
    if std::env::var_os("GOLDEN_RECORD").is_some() {
        std::fs::write(path, &got).expect("the fixture");
        return;
    }
    let want = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        got == want,
        "the init scene's events moved (GOLDEN_RECORD=1 to re-record); now:\n{got}"
    );
}
