//! Tempo sync on ORBIT's clock (reverb TYPEs spec § Tempo sync): the
//! division lists, the fold, the tempo quantiser, SYNC's conversion, and
//! the clock reaching the reverb through `OrbitRunner` and `Instrument`.

mod common;

use chimera_core::dsp::rev::burst::tap_targets;
use chimera_core::dsp::rev::sync::{
    DECAY_BEATS, DivKind, Division, FoldState, GATE_LENGTH, HALL_PRE, REVERSE_LENGTH, ROOM_PRE,
    Tempo, fold, index_of, p_of,
};
use chimera_core::dsp::reverb::{KnobMemory, RevSync, ReverbParams, ReverbType, knobs};
use chimera_core::hw::SampleBudget;
use chimera_core::meter::rev_clock;
use chimera_core::note_queue::RealTime;
use chimera_core::orbit::runner::OrbitRunner;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::inst_rig::InstRig;
use common::rev::{blocks, max_d2};
use common::sweep::{CLICK_FLOOR, CLICK_RATIO};

/// Each list with its knob's ms range (spec § Each TYPE's knobs).
const LISTS: [(&str, &[Division], f32, f32); 4] = [
    ("ROOM PRE-DLY", ROOM_PRE, 0.0, 50.0),
    ("HALL PRE-DLY", HALL_PRE, 0.0, 125.0),
    ("GATE LENGTH", GATE_LENGTH, 40.0, 600.0),
    ("REVERSE LENGTH", REVERSE_LENGTH, 150.0, 800.0),
];

fn tempo(bpm: f32) -> Tempo {
    Tempo::from_bpm(bpm)
}

/// `t` on its starting knobs, SYNC `s`.
fn synced(t: ReverbType, s: RevSync) -> ReverbParams {
    ReverbParams {
        sync: s,
        ..ReverbParams::default().with_type(t, &KnobMemory::new())
    }
}

fn at(list: &[Division], den: u8, kind: DivKind) -> f32 {
    let i = list
        .iter()
        .position(|d| d.num == 1 && d.den == den && d.kind == kind)
        .expect("in the list");
    p_of(i, list.len())
}

#[test]
fn division_lists_sit_in_range_at_120() {
    let t = tempo(120.0);
    for (name, list, lo, hi) in LISTS {
        for d in list {
            let ms = d.ms(t);
            assert!((lo..=hi).contains(&ms), "{name} {d:?}: {ms} ms");
        }
    }
    let (first, last) = (GATE_LENGTH[0], GATE_LENGTH[GATE_LENGTH.len() - 1]);
    assert_eq!((first.den, first.kind), (32, DivKind::Triplet));
    assert!((first.ms(t) - 41.67).abs() < 0.01);
    assert_eq!((last.den, last.kind), (4, DivKind::Straight));
    assert_eq!(last.ms(t), 500.0);
}

#[test]
fn fold_halves_doubles_and_holds() {
    let mut st = FoldState(0);
    // GATE 1/4 at 90 BPM plays 1/8.
    assert_eq!(fold(667.0, 40.0, 600.0, &mut st), 333.5);
    // Doubled once.
    assert_eq!(fold(20.0, 40.0, 600.0, &mut st), 40.0);
    let mut st = FoldState(0);
    for ms in [600.0, 606.0, 594.0, 611.0] {
        let _ = fold(ms, 40.0, 600.0, &mut st);
    }
    // Within 2 %: no flip.
    assert_eq!(st, FoldState(0));
    // Past 2 %: one halving.
    assert_eq!(fold(613.0, 40.0, 600.0, &mut st), 306.5);
    assert_eq!(st, FoldState(1));
    // Back in only 2 % inside.
    assert_eq!(fold(1_190.0, 40.0, 600.0, &mut st), 595.0);
    assert_eq!(st, FoldState(1));
    assert_eq!(fold(1_150.0 / 2.0, 40.0, 600.0, &mut st), 575.0);
    assert_eq!(st, FoldState(0));
}

#[test]
fn fold_keeps_every_division_in_range_from_30_to_300_bpm() {
    for (name, list, lo, hi) in LISTS {
        for d in list {
            // Fresh at each tempo, and carried through a sweep up and down.
            let mut carried = FoldState(0);
            for bpm in (30..=300).chain((30..=300).rev()) {
                let ms = d.ms(tempo(bpm as f32));
                for st in [&mut FoldState(0), &mut carried] {
                    let got = fold(ms, lo, hi, st);
                    assert!((lo..=hi).contains(&got), "{name} {d:?} at {bpm}: {got}");
                    // On the grid: the division over a power of two, or
                    // within the 2 % held at a limit.
                    let k = libm::ldexpf(ms, -(st.0 as i32));
                    assert!(
                        k == 0.0 || (got / k - 1.0).abs() <= 0.021,
                        "{name} {d:?} at {bpm}: {got} of {k}"
                    );
                }
            }
        }
    }
}

/// A clock at a fold's boundary, jittering ±1 % (100 BPM, GATE 1/4 is
/// 600 ms), never flips it.
#[test]
fn jitter_at_a_fold_boundary_never_flips_it() {
    let mut st = FoldState(0);
    for k in 0..1_000 {
        let bpm = 100.0 * (1.0 + 0.01 * ((k * 7 % 21) as f32 - 10.0) / 10.0);
        let _ = fold(GATE_LENGTH[10].ms(tempo(bpm)), 40.0, 600.0, &mut st);
        assert_eq!(st, FoldState(0), "flipped at {bpm}");
    }
}

/// The tempo is a beat's samples on a 0.25 BPM grid.
#[test]
fn a_beat_maps_to_the_grid() {
    let at = |samples| Tempo::from_beat(samples, SR);
    assert_eq!(at(24_000), tempo(120.0));
    assert_eq!(at(22_500), tempo(128.0));
    // ±1 block of drain at 128 BPM: 127.75 or 128.25 at most.
    assert_eq!(at(22_564), tempo(127.75));
    assert_eq!(at(22_436), tempo(128.25));
    assert_eq!(at(0), tempo(1_200.0));
}

#[test]
fn decay_in_beats_clamps_and_room_has_no_all() {
    let mut f = FoldState(0);
    let mut hall = synced(ReverbType::Hall, RevSync::All);
    hall.time = 1.0; // 32 beats
    assert_eq!(DECAY_BEATS[index_of(hall.time, DECAY_BEATS.len())], 32);
    let k = knobs(&hall, tempo(60.0), &mut f);
    assert!(
        (k.rt60_s - 16.0).abs() < 0.01,
        "32 s clamps to 16: {}",
        k.rt60_s
    );
    hall.time = 0.0; // 1 beat
    let k = knobs(&hall, tempo(300.0), &mut f);
    assert!(
        (k.rt60_s - 0.8).abs() < 0.01,
        "0.2 s clamps to 0.8: {}",
        k.rt60_s
    );
    hall.time = p_of(3, DECAY_BEATS.len()); // 4 beats at 120: 2 s
    let k = knobs(&hall, tempo(120.0), &mut f);
    assert!((k.rt60_s - 2.0).abs() < 0.01, "{}", k.rt60_s);
    // BLOOM's DECAY in beats clamps to its 2–12 s.
    let mut bloom = synced(ReverbType::Bloom, RevSync::All);
    for (beats, bpm, want) in [(32, 120.0, 12.0), (1, 300.0, 2.0), (4, 60.0, 4.0)] {
        bloom.time = p_of(
            DECAY_BEATS.iter().position(|&b| b == beats).unwrap(),
            DECAY_BEATS.len(),
        );
        let k = knobs(&bloom, tempo(bpm), &mut f);
        assert!(
            (k.rt60_s - want).abs() < 0.01,
            "BLOOM {beats} beats at {bpm}: {}",
            k.rt60_s
        );
    }
    // HALL under TIME: DECAY in seconds.
    let time = ReverbParams {
        sync: RevSync::Time,
        ..hall
    };
    let off = ReverbParams {
        sync: RevSync::Off,
        ..hall
    };
    let k = |p: &ReverbParams| knobs(p, tempo(120.0), &mut FoldState(0));
    assert_eq!(k(&time).rt60_s, k(&off).rt60_s);
    // ROOM with a stored ALL: PRE-DLY as TIME, DECAY in seconds.
    let mut room = synced(ReverbType::Room, RevSync::All);
    room.predelay = p_of(3, ROOM_PRE.len()); // 1/64: 31.25 ms
    let as_time = ReverbParams {
        sync: RevSync::Time,
        ..room
    };
    let unsynced = ReverbParams {
        sync: RevSync::Off,
        ..room
    };
    assert_eq!(k(&room), k(&as_time));
    assert_eq!(k(&room).predelay_ms, 31.25);
    assert_eq!(k(&room).rt60_s, k(&unsynced).rt60_s);
    // ROOM's SYNC turned to ALL stores TIME.
    assert_eq!(
        room.with_sync(RevSync::All, tempo(120.0)).sync,
        RevSync::Time
    );
}

/// BLOOM syncs as HALL does: under TIME its PRE-DLY walks HALL's list,
/// folded into its 0–125 ms, and DECAY stays in seconds; under ALL its
/// DECAY is in beats, clamped to its 2–12 s.
#[test]
fn bloom_syncs_predelay_and_decay_in_beats() {
    let mut p = synced(ReverbType::Bloom, RevSync::Time);
    let k = |p: &ReverbParams, bpm: f32| knobs(p, tempo(bpm), &mut FoldState(0));
    for (i, d) in HALL_PRE.iter().enumerate() {
        p.predelay = p_of(i, HALL_PRE.len());
        assert_eq!(k(&p, 120.0).predelay_ms, d.ms(tempo(120.0)), "{d:?}");
    }
    // 1/16 at 60 BPM is 250 ms: folded an octave down.
    p.predelay = at(HALL_PRE, 16, DivKind::Straight);
    assert_eq!(k(&p, 60.0).predelay_ms, 125.0);
    let off = ReverbParams {
        sync: RevSync::Off,
        ..p
    };
    assert_eq!(
        k(&p, 120.0).rt60_s,
        k(&off, 120.0).rt60_s,
        "TIME: DECAY in s"
    );
    let all = ReverbParams {
        sync: RevSync::All,
        ..p
    };
    let beats = |n: u8| {
        p_of(
            DECAY_BEATS.iter().position(|&b| b == n).unwrap(),
            DECAY_BEATS.len(),
        )
    };
    for (n, bpm, want) in [
        (32, 120.0, 12.0),
        (4, 120.0, 2.0),
        (1, 300.0, 2.0),
        (12, 120.0, 6.0),
    ] {
        let q = ReverbParams {
            time: beats(n),
            ..all
        };
        let rt = k(&q, bpm).rt60_s;
        assert!((rt - want).abs() < 0.01, "{n} beats at {bpm}: {rt}");
    }
    // ALL keeps PRE-DLY's note.
    assert_eq!(k(&all, 120.0).predelay_ms, k(&p, 120.0).predelay_ms);
}

/// Spec: toggling SYNC on and off keeps the length within one division.
/// LENGTH .55 is 177.4 ms; the nearest division at 120 BPM is 1/16D
/// (187.5 ms, 10.1 away) over 1/8T (166.7, 10.7 away).
#[test]
fn sync_toggle_keeps_the_length_within_one_division() {
    let t = tempo(120.0);
    let g = synced(ReverbType::Gate, RevSync::Off);
    assert!((g.size - 0.55).abs() < 1e-6);
    let ms = |p: &ReverbParams| knobs(p, t, &mut FoldState(0)).length_ms;
    assert!((ms(&g) - 177.4).abs() < 0.1, "{}", ms(&g));
    let on = g.with_sync(RevSync::Time, t);
    assert_eq!(on.sync, RevSync::Time);
    assert_eq!(on.size, at(GATE_LENGTH, 16, DivKind::Dotted));
    assert_eq!(ms(&on), 187.5);
    let off = on.with_sync(RevSync::Off, t);
    assert!((ms(&off) - 187.5).abs() <= 1.0, "{}", ms(&off));
    // Converted at the tempo: at 90 BPM, 1/16 (166.7 ms, 10.7 away) over
    // 1/8T (222.2, 44.8 away).
    let on90 = g.with_sync(RevSync::Time, tempo(90.0));
    assert_eq!(on90.size, at(GATE_LENGTH, 16, DivKind::Straight));
    // HALL: PRE-DLY in note values, and under ALL DECAY in beats too.
    // PRE 7.3 ms → none (0, 7.3 away, over 1/128's 15.6); DECAY 1.97 s →
    // 4 beats (2 s).
    let h = synced(ReverbType::Hall, RevSync::Off);
    let all = h.with_sync(RevSync::All, t);
    assert_eq!(all.predelay, 0.0);
    assert_eq!(all.time, p_of(3, DECAY_BEATS.len()));
    let back = all.with_sync(RevSync::Off, t);
    let k = knobs(&back, t, &mut FoldState(0));
    assert!(k.predelay_ms.abs() < 0.1, "{}", k.predelay_ms);
    assert!((k.rt60_s - 2.0).abs() < 0.01, "{}", k.rt60_s);
    // TIME → ALL converts DECAY alone; ALL → TIME back to seconds.
    let time = h.with_sync(RevSync::Time, t);
    assert_eq!((time.time, time.predelay), (h.time, all.predelay));
    assert_eq!(time.with_sync(RevSync::All, t), all);
}

#[test]
fn division_encoding_round_trips() {
    for (name, list, ..) in LISTS {
        let n = list.len();
        for i in 0..n {
            assert_eq!(index_of(p_of(i, n), n), i, "{name} {i}");
        }
    }
    let n = DECAY_BEATS.len();
    for i in 0..n {
        assert_eq!(index_of(p_of(i, n), n), i);
    }
    assert_eq!(index_of(f32::NAN, 11), 0);
}

#[test]
fn gate_sync_eighth_at_120_is_250_ms() {
    let mut g = synced(ReverbType::Gate, RevSync::Time);
    g.size = at(GATE_LENGTH, 8, DivKind::Straight);
    let k = knobs(&g, tempo(120.0), &mut FoldState(0));
    assert!((k.length_ms - 250.0).abs() <= 0.1, "{}", k.length_ms);
    // Unsynced, the same knob is its ms law.
    g.sync = RevSync::Off;
    assert!((knobs(&g, tempo(120.0), &mut FoldState(0)).length_ms - 250.0).abs() > 5.0);
}

/// REVERSE's REGEN rings for its synced LENGTH: RT60 follows the burst.
#[test]
fn regen_rt60_follows_the_synced_length() {
    let mut r = synced(ReverbType::Reverse, RevSync::Time);
    r.time = 0.7;
    r.size = at(REVERSE_LENGTH, 4, DivKind::Straight);
    let k = |bpm| knobs(&r, tempo(bpm), &mut FoldState(0));
    let (a, b) = (k(120.0), k(100.0));
    assert_eq!((a.length_ms, b.length_ms), (500.0, 600.0));
    assert!(
        (b.rt60_s / a.rt60_s - 1.2).abs() < 1e-3,
        "{} {}",
        a.rt60_s,
        b.rt60_s
    );
}

/// Samples between MIDI clocks at `bpm`, 24 a beat.
fn tick_every(bpm: f32) -> f64 {
    SR as f64 * 60.0 / (bpm as f64 * 24.0)
}

/// ORBIT's runner handing an `Instrument` its tempo a block at a time, fed
/// a MIDI clock whose ticks are drained at each block's start, as the
/// shells drain them.
struct Clocked {
    r: InstRig,
    runner: Box<OrbitRunner>,
    /// The next tick, and when it lands, jittered.
    next: f64,
    due: f64,
    seed: u32,
}

impl Clocked {
    fn new(g: ReverbParams) -> Self {
        let mut r = InstRig::with_budget(SampleBudget::for_cpu(480_000_000));
        r.shared.fx.reverb = g;
        let sr = core::num::NonZeroU32::new(SR).unwrap();
        Clocked {
            r,
            runner: Box::new(OrbitRunner::new(sr)),
            next: 0.0,
            due: 0.0,
            seed: 0x2545_f491,
        }
    }

    fn rt(&mut self, rt: RealTime) {
        let now = self.runner.now();
        self.runner.realtime(0, rt, now);
    }

    /// One block: Clocks at `bpm` (`None`: silent), each ±`jit_ms` off its
    /// beat, then the runner, then (with `render`) the instrument.
    fn block(&mut self, bpm: Option<f32>, jit_ms: f32, render: bool) {
        let now = self.runner.now();
        let end = (now + BLOCK_SIZE as u64) as f64;
        while let Some(b) = bpm
            && self.due < end
        {
            self.rt(RealTime::Clock);
            self.next += tick_every(b);
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 17;
            self.seed ^= self.seed << 5;
            let j = (self.seed as f64 / u32::MAX as f64 * 2.0 - 1.0) * jit_ms as f64;
            self.due = self.next + j * SR as f64 / 1000.0;
        }
        if bpm.is_none() {
            // Silence: the next clock lands where it would have.
            (self.next, self.due) = (end, end);
        }
        self.runner.block(&mut self.r.inst, &self.r.shared);
        if render {
            self.r.render(1);
        }
    }

    fn tempo(&self) -> Tempo {
        self.r.inst.tempo()
    }
}

/// The blocks at which `tempos` changes.
fn retargets(tempos: &[Tempo]) -> Vec<(usize, f32, f32)> {
    tempos
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] != w[1])
        .map(|(b, w)| (b + 1, w[0].0, w[1].0))
        .collect()
}

/// A steady clock, drained a block at a time, never retargets once its
/// average is full: one move from the internal 120 BPM, at a beat, and
/// none after, at 110, 128, 140 and 174 BPM and at 120 BPM with ±0.5 ms of
/// jitter a tick.
#[test]
fn steady_clocks_never_retarget() {
    let g = synced(ReverbType::Gate, RevSync::Time);
    for (bpm, jit) in [
        (110.0, 0.0),
        (128.0, 0.0),
        (140.0, 0.0),
        (174.0, 0.0),
        (120.0, 0.5),
    ] {
        let mut c = Clocked::new(g);
        // The clock comes in a second after power-up.
        let tempos: Vec<_> = (0..blocks(9.0))
            .map(|b| {
                c.block((b >= blocks(1.0)).then_some(bpm), jit, false);
                c.tempo()
            })
            .collect();
        let moves = retargets(&tempos);
        println!("{bpm} BPM ±{jit} ms: {} retargets {moves:?}", moves.len());
        // The settled average taken once (at 120 ± jitter it may land a
        // grid step off), then nothing.
        assert!(moves.len() <= 1, "{bpm}: {moves:?}");
        assert!(bpm == 120.0 || moves.len() == 1, "{bpm}: {moves:?}");
        // Held until the average spans a beat.
        let beat = (SR as f32 * 60.0 / bpm) as usize / BLOCK_SIZE;
        assert!(
            moves.iter().all(|m| m.0 + 2 >= blocks(1.0) + beat),
            "{bpm}: {moves:?}"
        );
        // Within a block's drain over the beat, and the grid's half step.
        let last = tempos.last().unwrap().0;
        let within = bpm * bpm * BLOCK_SIZE as f32 / (60.0 * SR as f32) + 0.125;
        assert!(
            (last - bpm).abs() <= within,
            "{bpm}: {last} off by > {within}"
        );
    }
}

/// A DAW stopped with its Clock still running is followed: a tempo it
/// changes to while stopped (100 → 125 BPM) moves the reverb. The clock
/// falling silent while ORBIT is stopped keeps the last external tempo, as
/// an Elektron box's does; a BPM edit takes the internal one back.
#[test]
fn a_stopped_or_silent_daw_keeps_its_tempo() {
    let mut c = Clocked::new(synced(ReverbType::Gate, RevSync::Time));
    c.rt(RealTime::Start);
    let mut tempos = vec![];
    for b in 0..blocks(10.0) {
        if b == blocks(3.0) {
            c.rt(RealTime::Stop);
        }
        // Stopped, the clock runs on at 125 for 3 s, then stops.
        let bpm = match b {
            _ if b < blocks(3.0) => Some(100.0),
            _ if b < blocks(6.0) => Some(125.0),
            _ => None,
        };
        c.block(bpm, 0.0, false);
        tempos.push(c.tempo());
    }
    let moves = retargets(&tempos);
    assert_eq!(moves.len(), 2, "{moves:?}");
    assert!(
        moves[1].0 > blocks(3.0) && moves[1].0 < blocks(6.0),
        "{moves:?}"
    );
    assert_eq!(c.tempo(), tempo(125.0));
    c.r.shared.orbit.bpm = chimera_core::orbit::clock::Bpm::new(90).unwrap();
    c.block(None, 0.0, false);
    assert_eq!(c.tempo(), tempo(90.0));
}

/// `p_at(block)` fed a sine of `hz` at −6 dBFS: its L, and the blocks a
/// length move ran (its first crossfade to its last), from the event at
/// block `e`.
fn replay(
    hz: f32,
    e: usize,
    p_at: impl Fn(usize) -> (ReverbParams, Tempo),
) -> (Vec<f32>, std::ops::Range<usize>) {
    use chimera_core::dsp::Stereo;
    use chimera_core::dsp::rev::Reverb;
    let mut rv = Box::new(Reverb::of_type(p_at(0).0.kind));
    let sine = common::rev::tone(hz, 0.5);
    let (mut out, mut last) = (vec![], e);
    for b in 0..blocks(4.0) {
        let (p, t) = p_at(b);
        let send = core::array::from_fn(|i| sine(b * BLOCK_SIZE + i));
        let mut w = Stereo::SILENT;
        rv.process(&send, &p, p.mix, t, SR, &mut w);
        out.extend_from_slice(&w.l);
        let mut f = rv.fold();
        let e_ = rv.burst().unwrap();
        if e_.crossfades().next().is_some()
            || e_.taps_at() != tap_targets(knobs(&p, t, &mut f).length_ms)
        {
            last = b;
        }
    }
    (out, e..last + 1)
}

/// `common::sweep`'s click: the move's largest second difference over
/// `CLICK_RATIO` × the steady state either side, and over `CLICK_FLOOR`.
/// Returns the ratio.
fn click(x: &[f32], mv: &std::ops::Range<usize>) -> (bool, f32) {
    let d2 = |a: usize, b: usize| max_d2(&x[a * BLOCK_SIZE..b * BLOCK_SIZE]);
    let trans = d2(mv.start, mv.end + 2);
    let steady = d2(mv.start - 40, mv.start - 1).max(d2(mv.end + 20, mv.end + 60));
    let r = trans / steady.max(1e-9);
    (trans > CLICK_FLOOR && r > CLICK_RATIO, r)
}

const FREQS: [f32; 5] = [110.0, 220.0, 440.0, 1_000.0, 3_000.0];

/// GATE's synced LENGTH moved by the tempo (100 → 120 BPM: 1/4 600 →
/// 500 ms) and by SYNC's note (1/8 → 1/4 mid-tail, spec § Sound) clicks at
/// no frequency.
#[test]
fn synced_length_moves_do_not_click() {
    let mut g = synced(ReverbType::Gate, RevSync::Time);
    (g.size, g.mix) = (1.0, 1.0);
    let e = blocks(2.0);
    let eighth = ReverbParams {
        size: at(GATE_LENGTH, 8, DivKind::Straight),
        ..g
    };
    for hz in FREQS {
        let (x, mv) = replay(hz, e, |b| (g, tempo(if b < e { 100.0 } else { 120.0 })));
        let (bad, r) = click(&x, &mv);
        println!("tempo move at {hz} Hz: {:?} blocks, ×{r:.2}", mv.len());
        assert!(!bad && mv.len() > 4, "tempo move clicks at {hz} Hz: ×{r}");
        let (x, mv) = replay(hz, e, |b| (if b < e { eighth } else { g }, tempo(120.0)));
        let (bad, r) = click(&x, &mv);
        println!("1/8 → 1/4 at {hz} Hz: {:?} blocks, ×{r:.2}", mv.len());
        assert!(!bad && mv.len() > 4, "1/8 → 1/4 clicks at {hz} Hz: ×{r}");
    }
}

/// Review Focus 5. ORBIT plays on a 100 BPM MIDI clock (SYNC EXT); GATE
/// plays 1/4 synced (600 ms, its maximum: the fold's boundary). The ticks
/// stop, and 500 ms later ORBIT plays on its own 120 BPM, which the reverb
/// follows. The LENGTH target changes exactly once after the clock
/// settles, each tap moves by crossfades of one step, 128 ring samples
/// long, the taps reach the target, the cells read what played, and the
/// fold never flips.
#[test]
fn clock_dropout_moves_synced_length_once() {
    let mut g = synced(ReverbType::Gate, RevSync::Time);
    (g.size, g.mix) = (1.0, 1.0); // 1/4
    let mut c = Clocked::new(g);
    c.rt(RealTime::Start);
    let drop = blocks(3.0);
    let (mut targets, mut folds, mut steps, mut taps, mut fading) =
        (vec![], vec![], vec![], vec![], vec![]);
    for b in 0..blocks(5.0) {
        c.block((b < drop).then_some(100.0), 0.0, true);
        let rv = c.r.fx.reverb();
        assert_eq!(rev_clock(), (c.tempo(), rv.fold()), "the cells' clock");
        targets.push(knobs(&g, c.tempo(), &mut rv.fold()).length_ms);
        folds.push(rv.fold());
        // QUAD switches to GATE over the first blocks.
        if let Some(e) = rv.burst() {
            steps.extend(e.crossfades().map(|(_, a, b)| a.abs_diff(b)));
            fading.push(e.crossfades().next().is_some());
            taps.push(e.taps_at());
        }
    }
    let changes: Vec<_> = targets
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] != w[1])
        .map(|(b, w)| (b + 1, w[0], w[1]))
        .collect();
    println!("LENGTH target changes: {changes:?}");
    // 120 → 100 once settled, then back to 120 once ORBIT's own clock plays.
    assert_eq!(changes.len(), 2, "{changes:?}");
    let (b, from, to) = changes[1];
    assert!((from - 600.0).abs() < 0.01 && to == 500.0, "{from} → {to}");
    let fell = drop + blocks(0.5);
    assert!(b.abs_diff(fell) <= 20, "at block {b}, not about {fell}");
    assert!(folds.iter().all(|&f| f == FoldState(0)), "the fold flipped");
    assert!(
        !steps.is_empty() && steps.iter().all(|&s| s <= 120),
        "{steps:?}"
    );
    // Each crossfade lasts 128 ring samples, 4 blocks of 32: it shows
    // mid-way at 3 blocks' ends, then the next starts.
    let runs: Vec<usize> = fading
        .split(|&f| !f)
        .map(<[bool]>::len)
        .filter(|&n| n > 0)
        .collect();
    assert!(!runs.is_empty() && runs.iter().all(|&n| n == 3), "{runs:?}");
    let off = targets.len() - taps.len();
    assert_eq!(taps[b - 1 - off], tap_targets(from));
    assert_eq!(*taps.last().unwrap(), tap_targets(500.0));
}

/// A switch starts the fold afresh: the fold the cells mark is the running
/// engine's, and QUAD folds nothing.
#[test]
fn a_switch_starts_the_fold_afresh() {
    use chimera_core::dsp::Stereo;
    use chimera_core::dsp::rev::Reverb;
    let mut g = synced(ReverbType::Gate, RevSync::Time);
    (g.size, g.mix) = (1.0, 1.0); // 1/4: 667 ms at 90 BPM plays 1/8
    let mut rv = Box::new(Reverb::of_type(ReverbType::Gate));
    let send = [0.0; BLOCK_SIZE];
    let run = |rv: &mut Reverb, p: &ReverbParams, blocks| {
        for _ in 0..blocks {
            let mut out = Stereo::SILENT;
            rv.process(&send, p, p.mix, tempo(90.0), SR, &mut out);
        }
    };
    run(&mut rv, &g, 4);
    assert_eq!(rv.fold(), FoldState(1));
    let quad = ReverbParams {
        kind: ReverbType::Quad,
        ..g
    };
    run(&mut rv, &quad, 100);
    assert_eq!(rv.running(), ReverbType::Quad);
    assert_eq!(rv.fold(), FoldState(0));
}
