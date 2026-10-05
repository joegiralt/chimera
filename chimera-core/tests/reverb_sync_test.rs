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
use common::rev::max_d2;

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

#[test]
fn tempo_quantiser_holds_through_jitter() {
    let held = tempo(120.0);
    for b in [119.9, 120.1, 119.95] {
        assert_eq!(tempo(b).quantised(held), held);
    }
    // Jitter across a step's edge holds too.
    for b in [120.13, 119.87] {
        assert_eq!(tempo(b).quantised(held), held);
    }
    assert_eq!(tempo(120.4).quantised(held), tempo(120.5));
    assert_eq!(tempo(119.8).quantised(held), tempo(119.75));
}

#[test]
fn decay_in_beats_clamps_and_room_has_no_all() {
    let mut f = FoldState(0);
    let mut hall = synced(ReverbType::Hall, RevSync::All);
    hall.time = 1.0; // 32 beats
    assert_eq!(DECAY_BEATS[index_of(hall.time, DECAY_BEATS.len())], 32);
    let k = knobs(&hall, tempo(120.0), &mut f);
    assert!(
        (k.rt60_s - 10.0).abs() < 0.01,
        "16 s clamps to 10: {}",
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
    // PRE 18.75 ms → 1/64T (20.8); DECAY 2.49 s → 4 beats (2 s) over 6 (3 s).
    let h = synced(ReverbType::Hall, RevSync::Off);
    let all = h.with_sync(RevSync::All, t);
    assert_eq!(all.predelay, at(HALL_PRE, 64, DivKind::Triplet));
    assert_eq!(all.time, p_of(3, DECAY_BEATS.len()));
    let back = all.with_sync(RevSync::Off, t);
    let k = knobs(&back, t, &mut FoldState(0));
    assert!((k.predelay_ms - 20.83).abs() < 0.1, "{}", k.predelay_ms);
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

/// `g` fed a 220 Hz sine at −12 dBFS, block by block at `tempos`, from
/// 2 s: its L.
fn replay(g: &ReverbParams, tempos: &[Tempo]) -> Vec<f32> {
    use chimera_core::dsp::Stereo;
    use chimera_core::dsp::rev::Reverb;
    let mut rv = Box::new(Reverb::of_type(g.kind));
    let sine = common::rev::tone(220.0, 0.25);
    let mut out = vec![];
    for (b, &t) in tempos.iter().enumerate() {
        let send = core::array::from_fn(|i| sine(b * BLOCK_SIZE + i));
        let mut w = Stereo::SILENT;
        rv.process(&send, g, g.mix, t, SR, &mut w);
        out.extend_from_slice(&w.l);
    }
    out.split_off(2 * SR as usize)
}

/// Samples between MIDI clocks at `bpm`, 24 a beat.
fn tick_every(bpm: f32) -> f64 {
    SR as f64 * 60.0 / (bpm as f64 * 24.0)
}

/// Review Focus 5. ORBIT on SYNC EXT follows a 100 BPM MIDI clock; GATE
/// plays 1/4 synced (600 ms, its maximum: the fold's boundary). The ticks
/// stop, and 500 ms later the internal 120 BPM takes over. The LENGTH
/// target changes exactly once, the taps reach it by crossfades of at most
/// one step, the return is no rougher than held at either tempo, and the
/// fold never flips.
#[test]
fn clock_dropout_moves_synced_length_once() {
    let run = |drop_at: Option<f64>| {
        let mut r = InstRig::with_budget(SampleBudget::for_cpu(480_000_000));
        let mut g = synced(ReverbType::Gate, RevSync::Time);
        (g.size, g.mix) = (1.0, 1.0); // 1/4
        r.shared.fx.reverb = g;
        let mut runner = Box::new(OrbitRunner::new(core::num::NonZeroU32::new(SR).unwrap()));
        let (mut next, every) = (0.0f64, tick_every(100.0));
        let (mut tempos, mut targets, mut folds) = (vec![], vec![], vec![]);
        let (mut steps, mut taps) = (vec![], vec![]);
        for b in 0..(4.0 * SR as f64 / BLOCK_SIZE as f64) as usize {
            let now = runner.now();
            let end = (now + BLOCK_SIZE as u64) as f64;
            while next < end && drop_at.is_none_or(|d| next < d) {
                runner.realtime(0, RealTime::Clock, now);
                next += every;
            }
            runner.block(&mut r.inst, &r.shared);
            r.render(1);
            let rv = r.fx.reverb();
            let mut f = rv.fold();
            tempos.push(r.inst.tempo());
            // What the cells draw is what played, the 100 BPM too.
            assert_eq!(rev_clock(), (r.inst.tempo(), rv.fold()));
            targets.push(knobs(&g, r.inst.tempo(), &mut f).length_ms);
            folds.push(rv.fold());
            // QUAD switches to GATE over the first blocks.
            if let Some(e) = rv.burst() {
                steps.extend(e.crossfades().map(|(_, a, b)| a.abs_diff(b)));
                taps.push(e.taps_at());
            }
            let _ = b;
        }
        (g, tempos, targets, folds, steps, taps)
    };
    // Just past a tick: the clock's last.
    let drop = 2.0 * SR as f64 + 1.0;
    let (g, tempos, targets, folds, steps, taps) = run(Some(drop));
    let changes: Vec<_> = targets
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] != w[1])
        .map(|(b, w)| (b, w[0], w[1]))
        .collect();
    println!("LENGTH target changes: {changes:?}");
    // The first second settles the clock's average: judged after it.
    let settled = SR as usize / BLOCK_SIZE;
    let after: Vec<_> = changes.iter().filter(|c| c.0 >= settled).collect();
    assert_eq!(after.len(), 1, "{after:?}");
    let (b, from, to) = *after[0];
    assert!((from - 600.0).abs() < 3.0 && to == 500.0, "{from} → {to}");
    let fell = (drop + SR as f64 / 2.0) / BLOCK_SIZE as f64;
    assert!((b as f64 - fell).abs() <= 3.0, "at block {b}, not {fell}");
    assert!(folds.iter().all(|&f| f == FoldState(0)), "the fold flipped");
    assert!(
        !steps.is_empty() && steps.iter().all(|&s| s <= 120),
        "{steps:?}"
    );
    // The engine plays the target: the taps on it before the drop and
    // after the move.
    let before = (drop / BLOCK_SIZE as f64) as usize;
    let at = |b: usize| taps[b - (targets.len() - taps.len())];
    assert_eq!(at(before), tap_targets(targets[before]));
    assert_eq!(at(targets.len() - 1), tap_targets(500.0));
    // That tempo sequence replayed into GATE fed a sine, which shows a
    // click the chain's own source would hide: no rougher through the
    // move than held at either tempo.
    let held = |bpm| max_d2(&replay(&g, &vec![tempo(bpm); tempos.len()]));
    let held = held(100.0).max(held(120.0));
    let moved = max_d2(&replay(&g, &tempos));
    println!("d2: moved {moved}, held {held}");
    assert!(moved <= 1.25 * held, "d2 {moved} against held {held}");
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
