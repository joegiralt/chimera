//! Real-time MIDI and the MIDI clock (Task 12, spec § 6.1–6.3): the parser,
//! the queue's order, and the runner following a leading clock, through the
//! QA rig's `Engine::block` order (the drain inside the load gate).

mod common;

use chimera_core::instrument::NoteOrigin;
use chimera_core::note_queue::{InEvent, NoteEvent, NoteKind, NoteSources, RealTime, SourceId};
use chimera_core::orbit::clock::{
    ClockSource, MAX_BPM_EXT, Phase, Position, SyncMode, step_at, step_start,
};
use chimera_core::orbit::engine::{EVENTS_PER_RING, OrbitKind};
use chimera_core::orbit::euclid::{Fill, Steps};
use chimera_core::orbit::ring::{Octave, Ring};
use chimera_core::orbit::rng::Pct;
use chimera_core::orbit::scene::Scene;
use chimera_core::orbit::{RINGS, RingIdx};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::midi::MidiParser;
use chimera_hal::{BLOCK_SIZE, MidiMessage};
use common::SR;
use common::orbit::OrbitRig;

const B: u64 = BLOCK_SIZE as u64;
/// 500 ms of silence, in samples.
const SILENT: u64 = SR as u64 / 2;
const BAR: u64 = 1 << 32;

#[test]
fn midi_parser_real_time() {
    let mut p = MidiParser::new();
    let got: Vec<MidiMessage> = [
        0x90, 60, 0xF8, 100, 0xFA, 62, 0xFB, 0, 0xFC, 0xFE, 0xFF, 64, 0xFE, 90,
    ]
    .iter()
    .filter_map(|&b| p.feed(b))
    .collect();
    let ch = MidiChannel::new(0).unwrap();
    let n = |n| MidiNote::new(n).unwrap();
    assert_eq!(
        got,
        [
            MidiMessage::Clock,
            MidiMessage::NoteOn {
                channel: ch,
                note: n(60),
                velocity: Velocity::new(100).unwrap()
            },
            MidiMessage::Start,
            MidiMessage::Continue,
            MidiMessage::NoteOff {
                channel: ch,
                note: n(62),
                velocity: 0
            },
            MidiMessage::Stop,
            MidiMessage::NoteOn {
                channel: ch,
                note: n(64),
                velocity: Velocity::new(90).unwrap()
            },
        ],
        "real-time bytes come out where they fall; running status holds; 0xFE and 0xFF drop"
    );
}

fn note(on: bool, n: u8) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(3).unwrap(),
        note: MidiNote::new(n).unwrap(),
        kind: if on {
            NoteKind::On(Velocity::new(77).unwrap())
        } else {
            NoteKind::Off
        },
    }
}

#[test]
fn real_time_keeps_its_order_in_the_queue() {
    const SRC: SourceId<1> = SourceId::new(0);
    let q: NoteSources<1> = NoteSources::new();
    let (mut producers, mut drain) = q.split().unwrap();
    let mut tx = producers.take(SRC).unwrap();
    let sent = [
        InEvent::Note(note(true, 60)),
        InEvent::Rt(RealTime::Clock),
        InEvent::Note(note(false, 60)),
        InEvent::Rt(RealTime::Start),
    ];
    for e in sent {
        assert!(tx.push(e));
    }
    let mut got = Vec::new();
    drain.drain(|s, e| got.push((s, e)));
    assert_eq!(got, sent.map(|e| (SRC, e)));
    for m in [
        MidiMessage::Clock,
        MidiMessage::Start,
        MidiMessage::Continue,
        MidiMessage::Stop,
    ] {
        let rt = InEvent::from_midi(m).expect("real-time is queued");
        assert!(tx.push(rt));
        drain.drain(|_, e| assert_eq!(e, rt));
    }
}

fn fill(n: u8) -> Fill {
    Fill::new(Steps::new(n).unwrap(), n, 0).unwrap()
}

/// NEW with ring `i` playing every one of `steps[i]` steps, gate `gate` %,
/// at an internal `bpm`; ORBIT not started.
fn rig(steps: [u8; RINGS], gate: u8, bpm: u16) -> OrbitRig {
    let mut r = OrbitRig::new();
    let song = r.song();
    *song.live_mut_for_test() = Scene::neutral();
    song.bpm = chimera_core::orbit::clock::Bpm::new(bpm).unwrap();
    for (i, &n) in steps.iter().enumerate() {
        let s = &mut song.live_mut_for_test().rings[i];
        s.ring = Ring::new(fill(n), Octave::new(4).unwrap());
        s.ring.gate = Pct::clamped(gate);
    }
    r
}

/// The step of an `n`-step ring that starts in `[from, to)`: the first at or
/// after `from`.
fn step_in(from: Position, n: u8) -> (u32, u8) {
    let st = Steps::new(n).unwrap();
    let k = step_at(from.phase, st);
    let k = k + (step_start(k, st) < from.phase) as u8;
    if k == n {
        (from.bar + 1, 0)
    } else {
        (from.bar, k)
    }
}

fn units(p: Position) -> u64 {
    ((p.bar as u64) << 32) + p.phase.get() as u64
}

/// Where the `n`-th Clock after Start falls: `(n − 1)/96` of a bar.
fn tick_pos(n: u64) -> u64 {
    ((n - 1) * BAR).div_ceil(96)
}

/// One block, delivering first every tick of `src` due by its start.
struct Ticks {
    at: Vec<u64>,
    next: usize,
}

impl Ticks {
    fn new(at: Vec<u64>) -> Ticks {
        Ticks { at, next: 0 }
    }

    /// Queues the ticks due by the next block's start: block resolution, as
    /// a shell drains them.
    fn feed(&mut self, r: &mut OrbitRig, src: usize) -> usize {
        let n = self.due(r.blocks() as u64 * B);
        for _ in 0..n {
            r.rt(src, RealTime::Clock);
        }
        n
    }

    /// Takes the ticks due by sample `t`: how many.
    fn due(&mut self, t: u64) -> usize {
        let from = self.next;
        while self.next < self.at.len() && self.at[self.next] <= t {
            self.next += 1;
        }
        self.next - from
    }

    fn sent(&self) -> u64 {
        self.next as u64
    }

    fn done(&self) -> bool {
        self.next == self.at.len()
    }
}

/// 24-PPQN ticks from `start`, `bpm(i)` setting the i-th interval.
fn ticks(start: u64, count: usize, bpm: impl Fn(usize) -> f64) -> Vec<f64> {
    let mut t = start as f64;
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        v.push(t);
        t += SR as f64 * 60.0 / (24.0 * bpm(i));
    }
    v
}

/// What fired: `(ring, bar, step, block)` per On.
type Fired = Vec<(usize, u32, u8, usize)>;

/// One block on the rig, its Ons as steps of `steps`, after checking the
/// block's events: at most `EVENTS_PER_RING` per ring, offs first.
fn step(r: &mut OrbitRig, steps: [u8; RINGS], fired: &mut Fired) {
    let from = r.view().pos;
    let b = r.blocks();
    r.block();
    let sent = r.sent.clone();
    let first_on = sent.iter().position(|e| e.kind != OrbitKind::Off);
    assert!(
        first_on.is_none_or(|i| sent[i..].iter().all(|e| e.kind != OrbitKind::Off)),
        "block {b}: offs precede ons: {sent:?}"
    );
    for ring in RingIdx::ALL {
        let mine: Vec<_> = sent.iter().filter(|e| e.ring == ring).collect();
        assert!(
            mine.len() <= EVENTS_PER_RING,
            "block {b}: ring {ring:?} sent {mine:?}"
        );
        let i = ring.get() as usize;
        if mine.iter().any(|e| e.kind != OrbitKind::Off) {
            let (bar, k) = step_in(from, steps[i]);
            fired.push((i, bar, k, b));
        }
    }
}

/// Every ring's steps fired once each, in order from (0, 0); returns how
/// many each fired.
fn each_once(fired: &Fired, steps: [u8; RINGS]) -> [usize; RINGS] {
    core::array::from_fn(|i| {
        let mine: Vec<(u32, u8)> = fired
            .iter()
            .filter(|f| f.0 == i)
            .map(|f| (f.1, f.2))
            .collect();
        for (j, &(bar, k)) in mine.iter().enumerate() {
            let n = steps[i] as usize;
            let want = ((j / n) as u32, (j % n) as u8);
            assert_eq!((bar, k), want, "ring {} ({n} steps): the {j}th On", i + 1);
        }
        mine.len()
    })
}

#[test]
fn start_waits_for_the_downbeat() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.play(true);
    r.run(40);
    assert!(r.view().running, "B6 under the internal clock plays");
    r.rt(0, RealTime::Start);
    r.block();
    assert!(
        r.sent.iter().all(|e| e.kind == OrbitKind::Off),
        "Start sends only the held notes' offs: {:?}",
        r.sent
    );
    assert!(r.runner_pending().iter().all(Option::is_none));
    for _ in 0..30 {
        r.block();
        assert!(r.sent.is_empty(), "no step before the first Clock");
        assert_eq!(r.view().pos, Position::ZERO, "the phase is held at 0");
        assert_eq!(r.view().source, ClockSource::Midi);
    }
    r.rt(0, RealTime::Clock);
    r.block();
    let ons: Vec<_> = r
        .sent
        .iter()
        .filter(|e| e.kind != OrbitKind::Off)
        .map(|e| e.ring)
        .collect();
    assert_eq!(
        ons,
        RingIdx::ALL,
        "every ring's step 0 is the first Clock's"
    );
    assert_eq!(r.view().pos.bar, 0);
}

/// Ticks of a 90 → 140 BPM ramp over `bars`, jittered ±1 ms from a seed:
/// `(ideal, arrival)`.
fn ramp(bars: u64, start: u64, seed: u32) -> (Vec<f64>, Vec<u64>) {
    let count = (bars * 96) as usize;
    let ideal = ticks(start, count, |i| 90.0 + 50.0 * i as f64 / count as f64);
    let mut rng = seed;
    let jitter = SR as f64 / 1000.0;
    let arrive = ideal
        .iter()
        .map(|&t| {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let j = ((rng >> 8) as f64 / (1u64 << 24) as f64 * 2.0 - 1.0) * jitter;
            (t + j).round() as u64
        })
        .collect();
    (ideal, arrive)
}

/// The sample `(bar, k)` of an `n`-step ring falls on, by the ticks' times.
fn when(times: &[f64], bar: u32, k: u8, n: u8) -> f64 {
    let x = (bar as f64 + k as f64 / n as f64) * 96.0;
    let j = x.floor() as usize;
    let f = x - j as f64;
    if f == 0.0 {
        times[j]
    } else {
        times[j] + f * (times[j + 1] - times[j])
    }
}

#[test]
fn clock_locks_to_midi() {
    const BARS: u64 = 4;
    let counts: Vec<u8> = (1..=32).collect();
    for (run, group) in counts.chunks(RINGS).enumerate() {
        let steps: [u8; RINGS] = core::array::from_fn(|i| group[i % group.len()]);
        let mut r = rig(steps, 50, 120);
        r.play(true);
        r.block();
        r.rt(0, RealTime::Start);
        r.block();
        let start = r.blocks() as u64 * B + 300;
        let (ideal, arrive) = ramp(BARS, start, 0x5EED + run as u32);
        let mut feed = Ticks::new(arrive.clone());
        let mut fired = Fired::new();
        // The arrival time of the block each tick was drained in.
        let mut heard: Vec<u64> = Vec::new();
        while !feed.done() {
            let t = r.blocks() as u64 * B;
            for _ in 0..feed.feed(&mut r, 0) {
                heard.push(t);
            }
            let before = r.view().pos;
            step(&mut r, steps, &mut fired);
            let now = r.view().pos;
            assert!(now >= before, "run {run}: the phase moved back");
            if feed.sent() > 0 {
                assert!(
                    units(now) < tick_pos(feed.sent() + 1),
                    "run {run}, block {}: past tick {} unheard",
                    r.blocks(),
                    feed.sent() + 1
                );
            }
        }
        let n = each_once(&fired, steps);
        for (i, &s) in steps.iter().enumerate() {
            assert!(
                n[i] as u64 >= (BARS - 1) * s as u64,
                "run {run}: ring {} fired {} of {s}-step bars",
                i + 1,
                n[i]
            );
        }
        // A step on a tick fires in the block that drains the tick. Every
        // step lands within ±1 block of its ideal time, give or take the
        // tick's own ±1 ms and the drain's block: ±2 blocks in all.
        //
        // A step between ticks is the estimator's: from the tick's arrival
        // at the tempo's rate. Against that it lands within ±1.5 blocks: a
        // half block from dating the drain, a half from the block it fires
        // in, a half for an average lagging the ramp. On average within a
        // third of a block; dated at the drain, a step is 0.6 late.
        let mut between = Vec::new();
        for &(i, bar, k, b) in &fired {
            let n = steps[i] as u64;
            let x = (bar as u64 * n + k as u64) * 96;
            let at = b as f64 * B as f64;
            if x.is_multiple_of(n) {
                assert_eq!(
                    b as u64 * B,
                    heard[(x / n) as usize],
                    "run {run}: ring {} ({n} steps) bar {bar} step {k} is on a tick",
                    i + 1
                );
            } else {
                let (j, f) = ((x / n) as usize, (x % n) as f64 / n as f64);
                let due = arrive[j] as f64 + f * (ideal[j + 1] - ideal[j]);
                let d = (at - due) / B as f64;
                assert!(
                    d.abs() <= 1.5,
                    "run {run}: ring {} ({n} steps) bar {bar} step {k}: {d:.2} blocks from its tick's arrival",
                    i + 1
                );
                between.push(d);
            }
            let d = (at - when(&ideal, bar, k, steps[i])) / B as f64;
            assert!(
                d.abs() <= 2.0,
                "run {run}: ring {} ({n} steps) bar {bar} step {k}: block {b} is {d:.2} blocks off",
                i + 1
            );
        }
        let mean = between.iter().sum::<f64>() / between.len() as f64;
        assert!(
            mean.abs() <= 0.35,
            "run {run}: steps between ticks {mean:.2} blocks off on average"
        );
    }
}

#[test]
fn max_tempo_32_steps_fits_events() {
    let steps = [32; RINGS];
    let mut r = rig(steps, 100, 120);
    r.play(true);
    r.block();
    r.rt(0, RealTime::Start);
    r.block();
    let start = r.blocks() as u64 * B + 10;
    let mut at: Vec<u64> = ticks(start, 2 * 96, |_| MAX_BPM_EXT as f64)
        .iter()
        .map(|&t| t as u64)
        .collect();
    let burst = *at.last().unwrap() + 100;
    at.extend((0..50).map(|_| burst));
    at.extend(
        ticks(burst + 100, 4 * 96, |_| MAX_BPM_EXT as f64)
            .iter()
            .map(|&t| t as u64),
    );
    let mut feed = Ticks::new(at);
    let mut fired = Fired::new();
    let mut max_units = 0;
    // The ticks go through a real note queue, the burst too.
    const SRC: SourceId<1> = SourceId::new(0);
    let q: NoteSources<1> = NoteSources::new();
    let (mut producers, mut drain) = q.split().unwrap();
    let mut tx = producers.take(SRC).unwrap();
    while !feed.done() {
        for _ in 0..feed.due(r.blocks() as u64 * B) {
            assert!(tx.push(RealTime::Clock), "the queue holds the burst");
        }
        drain.drain(|s, e| match e {
            InEvent::Rt(rt) => r.rt(s.index(), rt),
            InEvent::Note(n) => panic!("only clock was sent: {n:?}"),
            InEvent::Bend(_) | InEvent::Sustain(_) => panic!("only clock was sent"),
        });
        let before = r.view().pos;
        step(&mut r, steps, &mut fired);
        let d = units(r.view().pos) - units(before);
        max_units = max_units.max(d);
    }
    let n = each_once(&fired, steps);
    assert!(
        n.iter().all(|&k| k >= 5 * 32),
        "a burst is caught up, never skipped: {n:?}"
    );
    assert!(
        max_units < BAR / 32,
        "a window never spans a step: {max_units}"
    );
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
    assert_eq!(q.drops(), [0]);
}

/// Ticks at 120 BPM from Start for `bars`; returns the last tick's block.
fn play_ticks(r: &mut OrbitRig, steps: [u8; RINGS], bars: u64, fired: &mut Fired) -> u64 {
    r.rt(0, RealTime::Start);
    r.block();
    let start = r.blocks() as u64 * B + 20;
    let at: Vec<u64> = ticks(start, (bars * 96) as usize, |_| 120.0)
        .iter()
        .map(|&t| t as u64)
        .collect();
    let last = *at.last().unwrap();
    let mut feed = Ticks::new(at);
    while !feed.done() {
        feed.feed(r, 0);
        step(r, steps, fired);
    }
    // The block the last tick was drained in.
    last.div_ceil(B) * B
}

#[test]
fn clock_falls_back_after_500_ms() {
    let steps = [4, 3, 5, 7, 16];
    // Playing: the internal 90 BPM takes over from where MIDI left it.
    let mut r = rig(steps, 50, 90);
    r.play(true);
    r.block();
    let mut fired = Fired::new();
    let heard = play_ticks(&mut r, steps, 2, &mut fired);
    let mut fell = None;
    let mut prev = r.view().pos;
    while r.blocks() as u64 * B <= heard + 2 * SILENT {
        let t = r.blocks() as u64 * B;
        step(&mut r, steps, &mut fired);
        let v = r.view();
        assert!(v.pos >= prev, "the fallback never moves the phase back");
        prev = v.pos;
        if fell.is_none() && v.source == ClockSource::Internal {
            fell = Some(t);
        }
        assert!(v.running, "the fallback keeps the transport");
    }
    let fell = fell.expect("the clock fell back");
    // Silence counts from the tick's arrival, taken as half a block before
    // the drain that heard it.
    let arrived = heard - B / 2;
    assert!(
        fell + 32 >= arrived + SILENT && fell + 32 < arrived + SILENT + B,
        "falls back at 500 ms: block {fell}, last tick {heard}"
    );
    assert_eq!(r.view().bpm.get(), 90);
    // A second of internal clock: 90 BPM is 3/8 of a bar.
    let from = units(r.view().pos);
    r.run((SR as u64 / B) as usize);
    let moved = (units(r.view().pos) - from) as f64 / BAR as f64;
    assert!(
        (moved - 0.375).abs() < 0.01,
        "the internal BPM drives it: {moved}"
    );
    each_once(&fired, steps);

    // Stopped by the DAW: the fallback starts nothing.
    let mut r = rig(steps, 50, 90);
    let mut fired = Fired::new();
    let heard = play_ticks(&mut r, steps, 1, &mut fired);
    r.rt(0, RealTime::Stop);
    r.block();
    fired.clear();
    while r.blocks() as u64 * B <= heard + 2 * SILENT {
        step(&mut r, steps, &mut fired);
        assert!(!r.view().running, "a stopped transport stays stopped");
    }
    assert_eq!(r.view().source, ClockSource::Internal);
    assert!(fired.is_empty(), "{fired:?}");
}

#[test]
fn one_clock_leads() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.play(true);
    r.block();
    r.rt(0, RealTime::Start);
    r.block();
    let start = r.blocks() as u64 * B + 20;
    // The leader at 120 BPM for two bars, then silent.
    let lead: Vec<u64> = ticks(start, 192, |_| 120.0)
        .iter()
        .map(|&t| t as u64)
        .collect();
    let lead_end = *lead.last().unwrap();
    // The other at 100 BPM, from a bar in, with a Start of its own.
    let other: Vec<u64> = ticks(start + SR as u64 * 2, 1000, |_| 100.0)
        .iter()
        .map(|&t| t as u64)
        .collect();
    let (mut a, mut b) = (Ticks::new(lead), Ticks::new(other));
    let mut fired = Fired::new();
    let mut other_started = false;
    let mut took_over = None;
    while !b.done() {
        let t = r.blocks() as u64 * B;
        a.feed(&mut r, 0);
        if !other_started && b.sent() == 0 && b.at[0] <= t {
            r.rt(1, RealTime::Start);
            other_started = true;
        }
        b.feed(&mut r, 1);
        let before = r.view().pos;
        step(&mut r, steps, &mut fired);
        let v = r.view();
        assert!(
            v.pos >= before,
            "never moved back: the other's Start is ignored"
        );
        if t <= lead_end + SILENT - B {
            // The leader's clock only: never past its next tick, never reset.
            assert!(
                a.sent() == 0 && v.pos == Position::ZERO || units(v.pos) < tick_pos(a.sent() + 1),
                "block {}: past the leader's tick {}",
                r.blocks(),
                a.sent() + 1
            );
        } else if took_over.is_none() && v.source == ClockSource::Midi && v.pos > before {
            took_over = Some(t);
        }
        if b.sent() > 400 {
            break;
        }
    }
    let took = took_over.expect("the other source leads once the first is silent");
    assert!(took >= lead_end + SILENT - B, "{took} {lead_end}");
    // It leads at its own rate: 100 BPM measured.
    assert_eq!(r.view().source, ClockSource::Midi);
    assert!(
        r.view().bpm.get().abs_diff(100) <= 1,
        "the second clock's tempo: {}",
        r.view().bpm.get()
    );
    each_once(&fired, steps);
}

fn ons(r: &OrbitRig) -> usize {
    r.sent.iter().filter(|e| e.kind != OrbitKind::Off).count()
}

/// Stop, then the cable is pulled: the internal clock takes over, and B6
/// plays ORBIT on it. Nothing outside is left to wait for.
#[test]
fn stop_then_silence_b6_plays() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.play(true);
    r.block();
    let mut fired = Fired::new();
    let heard = play_ticks(&mut r, steps, 1, &mut fired);
    r.rt(0, RealTime::Stop);
    r.block();
    assert!(!r.view().running);
    while r.blocks() as u64 * B <= heard + 2 * SILENT {
        r.block();
        assert_eq!(ons(&r), 0, "stopped");
    }
    assert_eq!(r.view().source, ClockSource::Internal);
    r.play(true);
    let mut n = 0;
    for _ in 0..200 {
        r.block();
        n += ons(&r);
    }
    assert!(r.view().running);
    assert!(n > 0, "B6 plays on the internal clock");
}

/// One source's Stop, then another clocks with no Start: once the first is
/// silent, the second leads and moves, and B6 plays on it.
#[test]
fn new_leader_without_start_moves() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.play(true);
    r.block();
    let mut fired = Fired::new();
    let heard = play_ticks(&mut r, steps, 1, &mut fired);
    r.rt(0, RealTime::Stop);
    r.block();
    let start = r.blocks() as u64 * B;
    let mut other = Ticks::new(
        ticks(start, 400, |_| 120.0)
            .iter()
            .map(|&t| t as u64)
            .collect(),
    );
    let held = r.view().pos;
    while r.blocks() as u64 * B <= heard + SILENT - B {
        other.feed(&mut r, 1);
        r.block();
        assert_eq!(r.view().pos, held, "the first leader's Stop holds it");
    }
    for _ in 0..(SR as u64 / B) {
        other.feed(&mut r, 1);
        r.block();
    }
    assert_eq!(r.view().source, ClockSource::Midi);
    assert!(r.view().pos > held, "the new leader moves the clock");
    r.play(true);
    let mut n = 0;
    for _ in 0..200 {
        other.feed(&mut r, 1);
        r.block();
        n += ons(&r);
    }
    assert!(n > 0, "B6 plays on the new leader");
}

#[test]
fn midi_stop_stays_stopped() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 100, 120);
    r.play(true);
    r.block();
    let mut fired = Fired::new();
    play_ticks(&mut r, steps, 1, &mut fired);
    assert!(!fired.is_empty());
    r.rt(0, RealTime::Stop);
    r.block();
    assert!(
        r.sent.iter().all(|e| e.kind == OrbitKind::Off) && !r.sent.is_empty(),
        "Stop sends the held notes' offs: {:?}",
        r.sent
    );
    assert!(r.runner_pending().iter().all(Option::is_none));
    assert!(!r.view().running);
    // The clock goes on while stopped, as many DAWs send it.
    let start = r.blocks() as u64 * B;
    let mut feed = Ticks::new(
        ticks(start, 400, |_| 120.0)
            .iter()
            .map(|&t| t as u64)
            .collect(),
    );
    let pos = r.view().pos;
    for _ in 0..100 {
        feed.feed(&mut r, 0);
        r.block();
        assert!(
            r.sent.iter().all(|e| e.kind == OrbitKind::Off),
            "no ORBIT On after Stop: {:?}",
            r.sent
        );
        assert!(!r.view().running, "no B6, no start");
        assert_eq!(r.view().pos, pos, "a stopped clock doesn't move");
    }
    // B6 under the stopped DAW: ORBIT plays on its own clock, from bar 0.
    r.play(true);
    let mut n = 0;
    for _ in 0..100 {
        feed.feed(&mut r, 0);
        r.block();
        n += ons(&r);
        assert_eq!(r.view().source, ClockSource::Internal);
    }
    assert!(r.view().running && n > 0, "B6 plays ORBIT locally");
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// The DAW plays, B6 has stopped ORBIT: the clock runs on without it, and
/// B6 joins it in phase.
#[test]
fn b6_gates_orbit_under_an_external_clock() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.rt(0, RealTime::Start);
    r.block();
    assert!(r.view().running, "the DAW's Start plays ORBIT");
    r.ui.play_stop();
    r.block();
    assert!(!r.view().running, "B6 stops it");
    let start = r.blocks() as u64 * B + 20;
    let at: Vec<u64> = ticks(start, 8 * 96, |_| 120.0)
        .iter()
        .map(|&t| t as u64)
        .collect();
    let mut feed = Ticks::new(at);
    let mut fired = Fired::new();
    // B6 off: the clock runs, ORBIT is silent.
    while feed.sent() < 3 * 96 + 48 + 1 {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
        assert!(r.sent.is_empty(), "silent until B6");
    }
    let at = r.view().pos;
    assert_eq!(at.bar, 3, "the clock ran while ORBIT was off");
    assert!(at.phase >= Phase::new(1 << 31));
    // B6 at bar 3.5: each ring joins at its next step, in phase.
    r.play(true);
    while !feed.done() {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
        assert_eq!(
            units(r.view().pos) >> 32,
            (feed.sent() - 1) / 96,
            "the bar is the external clock's"
        );
    }
    for (i, &n) in steps.iter().enumerate() {
        let first = fired.iter().find(|f| f.0 == i).expect("ring plays");
        assert_eq!(
            (first.1, first.2),
            step_in(at, n),
            "ring {}: first On at the next step of bar 3",
            i + 1
        );
        let mine: Vec<_> = fired.iter().filter(|f| f.0 == i).collect();
        for w in mine.windows(2) {
            let next = if w[0].2 + 1 == n {
                (w[0].1 + 1, 0)
            } else {
                (w[0].1, w[0].2 + 1)
            };
            assert_eq!((w[1].1, w[1].2), next, "ring {}: every step once", i + 1);
        }
    }
    assert!(r.view().running);
}

/// 120 BPM ticks from the next block, for `n` ticks.
fn clock_from(r: &OrbitRig, n: usize, bpm: f64) -> Ticks {
    let start = r.blocks() as u64 * B + 20;
    Ticks::new(ticks(start, n, |_| bpm).iter().map(|&t| t as u64).collect())
}

/// SYNC INT (ADR 0080): ORBIT runs on its own clock whatever comes in.
/// Clock, Start, Continue and Stop move nothing.
#[test]
fn int_ignores_clock_and_transport() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 90);
    r.sync = SyncMode::Int;
    // Stopped: a DAW's Start and clock start nothing.
    r.rt(0, RealTime::Start);
    let mut feed = clock_from(&r, 400, 140.0);
    for _ in 0..100 {
        feed.feed(&mut r, 0);
        r.block();
        assert!(!r.view().running, "INT: Start doesn't play ORBIT");
        assert_eq!(ons(&r), 0);
    }
    // Playing on B6: the clock and every transport message are ignored.
    r.play(true);
    r.run(4);
    let mut prev = r.view().pos;
    let from = units(prev);
    let blocks = (SR as u64 / B) as usize;
    for i in 0..blocks {
        if i == 100 {
            r.rt(0, RealTime::Stop);
        }
        if i == 200 {
            r.rt(0, RealTime::Start);
        }
        if i == 300 {
            r.rt(0, RealTime::Continue);
        }
        feed.feed(&mut r, 0);
        r.block();
        let v = r.view();
        assert!(v.running, "INT: Stop doesn't stop ORBIT (block {i})");
        assert_eq!(v.source, ClockSource::Internal);
        assert_eq!(v.bpm.get(), 90);
        assert!(v.pos >= prev, "INT: Start doesn't send ORBIT back");
        prev = v.pos;
    }
    // A second at the internal 90 BPM is 3/8 of a bar.
    let moved = (units(r.view().pos) - from) as f64 / BAR as f64;
    assert!(
        (moved - 0.375).abs() < 0.01,
        "the internal BPM drives it: {moved}"
    );
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// The Live voices held, by note.
fn live_held(r: &OrbitRig) -> Vec<u8> {
    let slots = r.inst().allocator().slots();
    let origins = r.inst().origins();
    (0..slots.len())
        .filter(|&v| slots[v].held() && origins[v] == NoteOrigin::Live)
        .filter_map(|v| slots[v].note().map(|n| n.get()))
        .collect()
}

/// SYNC INT: a key played while clock and every transport message flood
/// in sounds, holds and releases as with nothing coming in.
#[test]
fn int_plays_live_notes_under_a_flood() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 90);
    r.sync = SyncMode::Int;
    let flood = [
        RealTime::Start,
        RealTime::Stop,
        RealTime::Continue,
        RealTime::Stop,
    ];
    let mut feed = clock_from(&r, 2000, 140.0);
    let mut flood_block = |r: &mut OrbitRig, i: usize| {
        for _ in 0..4 {
            r.rt(0, RealTime::Clock);
        }
        r.rt(0, flood[i % flood.len()]);
        feed.feed(r, 0);
        r.block();
    };
    for i in 0..20 {
        flood_block(&mut r, i);
    }
    r.key(0, 60, true);
    flood_block(&mut r, 20);
    assert_eq!(live_held(&r), [60], "the key sounds");
    for i in 21..200 {
        flood_block(&mut r, i);
        assert_eq!(live_held(&r), [60], "held through the flood (block {i})");
        assert!(!r.view().running, "INT: nothing starts ORBIT");
    }
    r.key(0, 60, false);
    flood_block(&mut r, 200);
    assert!(live_held(&r).is_empty(), "the key releases");
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// SYNC EXT: the DAW's Start plays ORBIT, from bar 0, with no B6, also
/// right after its own Stop; B6 still stops it locally.
#[test]
fn ext_start_plays_without_b6_after_a_stop() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    assert_eq!(r.sync, SyncMode::Ext, "EXT is the default");
    for round in 0..2 {
        r.rt(0, RealTime::Start);
        r.block();
        assert!(r.view().running, "round {round}: Start plays ORBIT");
        assert_eq!(r.view().pos, Position::ZERO, "round {round}: from bar 0");
        let mut feed = clock_from(&r, 2 * 96, 120.0);
        let mut fired = Fired::new();
        while !feed.done() {
            feed.feed(&mut r, 0);
            step(&mut r, steps, &mut fired);
        }
        assert_eq!(
            each_once(&fired, steps),
            [8, 6, 10, 14, 32],
            "round {round}"
        );
        r.rt(0, RealTime::Stop);
        r.block();
        assert!(!r.view().running, "round {round}: Stop stops");
        r.run(20);
        assert!(!r.view().running);
    }
    // B6 stops what the DAW started.
    r.rt(0, RealTime::Start);
    let mut feed = clock_from(&r, 96, 120.0);
    for _ in 0..50 {
        feed.feed(&mut r, 0);
        r.block();
    }
    assert!(r.view().running);
    r.ui.play_stop();
    feed.feed(&mut r, 0);
    r.block();
    assert!(!r.view().running, "B6 stops ORBIT under the DAW");
    for _ in 0..50 {
        feed.feed(&mut r, 0);
        r.block();
        assert_eq!(ons(&r), 0);
    }
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// SYNC EXT: B6 and the DAW's Stop in one block both say stop. B6
/// toggles the transport as it was before the DAW's word, so ORBIT
/// stops and stays stopped, never restarted from bar 0.
#[test]
fn b6_and_daw_stop_in_one_block_stop() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.rt(0, RealTime::Start);
    let mut feed = clock_from(&r, 4 * 96, 120.0);
    for _ in 0..200 {
        feed.feed(&mut r, 0);
        r.block();
    }
    assert!(r.view().running);
    r.rt(0, RealTime::Stop);
    r.ui.play_stop();
    feed.feed(&mut r, 0);
    r.block();
    assert!(!r.view().running, "B6 + Stop: stopped");
    for _ in 0..100 {
        feed.feed(&mut r, 0);
        r.block();
        assert!(!r.view().running, "stays stopped");
        assert_eq!(ons(&r), 0);
    }
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// SYNC EXT: B6 and the DAW's Start in one block. B6 toggles the
/// transport as it was before the Start: a stopped ORBIT plays, a
/// playing one stops.
#[test]
fn b6_and_daw_start_in_one_block_toggle_the_transport_before() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    r.rt(0, RealTime::Start);
    r.ui.play_stop();
    let mut feed = clock_from(&r, 4 * 96, 120.0);
    r.block();
    assert!(r.view().running, "stopped, B6 + Start: plays");
    assert_eq!(r.view().pos, Position::ZERO, "from bar 0");
    for _ in 0..200 {
        feed.feed(&mut r, 0);
        r.block();
        assert!(r.view().running);
    }
    r.rt(0, RealTime::Start);
    r.ui.play_stop();
    feed.feed(&mut r, 0);
    r.block();
    assert!(!r.view().running, "playing, B6 + Start: stops");
    for _ in 0..100 {
        feed.feed(&mut r, 0);
        r.block();
        assert!(!r.view().running, "stays stopped");
        assert_eq!(ons(&r), 0);
    }
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// SYNC EXT: a project load is the player's newer act, so the DAW's
/// Start, Continue or Stop drained with it is dropped: the new project waits stopped
/// for the DAW's next Start (ADR 0080).
#[test]
fn a_load_drops_a_pending_daw_start() {
    let steps = [4, 3, 5, 7, 16];
    for rt in [RealTime::Start, RealTime::Continue, RealTime::Stop] {
        let mut r = rig(steps, 50, 120);
        let mut feed = clock_from(&r, 4 * 96, 120.0);
        r.rt(0, rt);
        r.load();
        for _ in 0..100 {
            feed.feed(&mut r, 0);
            r.block();
            assert!(!r.view().running, "{rt:?} with a load: stopped");
            assert_eq!(ons(&r), 0);
        }
        r.rt(0, RealTime::Start);
        r.block();
        assert!(r.view().running, "the DAW's next Start plays");
        assert_eq!(r.view().pos, Position::ZERO);
        assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
    }
}

/// SYNC EXT: Continue plays on from where Stop left ORBIT, not bar 0.
#[test]
fn ext_continue_resumes() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 120);
    let mut fired = Fired::new();
    play_ticks(&mut r, steps, 1, &mut fired);
    // Half a bar more, then Stop.
    let mut feed = clock_from(&r, 48, 120.0);
    while !feed.done() {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
    }
    r.rt(0, RealTime::Stop);
    r.block();
    let held = r.view().pos;
    assert!(!r.view().running);
    assert_eq!(held.bar, 1);
    r.run(100);
    assert_eq!(r.view().pos, held, "stopped, ORBIT stays put");
    r.rt(0, RealTime::Continue);
    r.block();
    assert!(r.view().running, "Continue plays ORBIT with no B6");
    assert!(r.view().pos >= held && units(r.view().pos) < units(held) + BAR / 96);
    let mut feed = clock_from(&r, 2 * 96, 120.0);
    while !feed.done() {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
    }
    // Every step once from bar 0, through the Stop and on.
    let n = each_once(&fired, steps);
    for (i, &k) in steps.iter().enumerate() {
        assert!(n[i] >= 3 * k as usize, "ring {}: {} ons", i + 1, n[i]);
    }
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}

/// SYNC turned while ORBIT plays: never a jump back, never a stop, and the
/// new mode's clock takes over from where ORBIT is.
#[test]
fn switching_sync_while_playing() {
    let steps = [4, 3, 5, 7, 16];
    let mut r = rig(steps, 50, 90);
    let mut fired = Fired::new();
    play_ticks(&mut r, steps, 1, &mut fired);
    let mut feed = clock_from(&r, 2000, 140.0);
    for _ in 0..50 {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
    }
    assert_eq!(r.view().source, ClockSource::Midi);
    // EXT → INT: the internal 90 BPM carries on from the edge.
    r.sync = SyncMode::Int;
    let mut prev = r.view().pos;
    for _ in 0..400 {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
        let v = r.view();
        assert!(v.running, "INT keeps ORBIT playing");
        assert_eq!(v.source, ClockSource::Internal);
        assert_eq!(v.bpm.get(), 90);
        assert!(v.pos >= prev, "never back");
        prev = v.pos;
    }
    // A Stop in INT is ignored, so EXT doesn't wake to it.
    r.rt(0, RealTime::Stop);
    r.block();
    assert!(r.view().running);
    // INT → EXT: the running clock leads again, on from the edge.
    r.sync = SyncMode::Ext;
    let mut led = false;
    for _ in 0..400 {
        feed.feed(&mut r, 0);
        step(&mut r, steps, &mut fired);
        let v = r.view();
        assert!(v.running, "EXT keeps ORBIT playing");
        assert!(v.pos >= prev, "never back");
        prev = v.pos;
        led |= v.source == ClockSource::Midi;
    }
    assert!(led, "the DAW's clock leads again");
    assert_eq!(r.view().source, ClockSource::Midi);
    assert!(
        r.view().bpm.get().abs_diff(140) <= 1,
        "{}",
        r.view().bpm.get()
    );
    each_once(&fired, steps);
    assert!(r.stuck_notes().is_empty(), "{:?}", r.stuck_notes());
}
