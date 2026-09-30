use chimera_core::Velocity;
use chimera_core::orbit::euclid::{Fill, HitMask, Steps, accents, euclid};
use chimera_core::orbit::mutate::{Mutation, mutate};
use chimera_core::orbit::ring::{
    ACCENT_BOOST, Harmony, NoteMode, NoteOut, Octave, Range, Ring, SnapTo,
};
use chimera_core::orbit::rng::{Pct, Rng};
use chimera_core::orbit::walk::{Pool, Walk, WalkState};
use chimera_core::orbit::{RingIdx, STATE_BYTES};
use chimera_theory::{Chord, Key, Note, PitchClass, Quality, Scale};

fn n(x: u8) -> Note {
    Note::new(x).unwrap()
}

fn steps(s: u8) -> Steps {
    Steps::new(s).unwrap()
}

fn fill(s: u8, hits: u8, rotate: u8) -> Fill {
    Fill::new(steps(s), hits, rotate).unwrap()
}

fn show(m: HitMask) -> String {
    (0..m.steps().get() as usize)
        .map(|i| if m.is_hit(i) { 'x' } else { '.' })
        .collect()
}

fn e(s: u8, hits: u8, rotate: u8) -> String {
    show(euclid(steps(s), hits, rotate))
}

fn pct(p: u8) -> Pct {
    Pct::new(p).unwrap()
}

fn a_minor() -> Harmony {
    Harmony {
        key: Key::new(PitchClass::A, Scale::MINOR),
        chord: Chord::new(PitchClass::D, Quality::Min),
    }
}

fn ring(s: u8, hits: u8) -> Ring {
    Ring::new(fill(s, hits, 0), Octave::new(3).unwrap())
}

/// Plays `loops` loops of one bar, mutating at each loop's end.
fn run(mut r: Ring, h: &Harmony, seed: u32, loops: usize) -> (Vec<Option<NoteOut>>, Ring) {
    let mut rng = Rng::for_ring(seed, RingIdx::new(0).unwrap());
    let mut walk = WalkState::START;
    let mut out = Vec::new();
    let s = r.fill().steps().get() as usize;
    for _ in 0..loops {
        for i in 0..s {
            out.push(r.play(i, &mut walk, h, &mut rng));
        }
        if let Some(m) = mutate(&r, h, &mut rng) {
            r = r.apply(m);
        }
    }
    (out, r)
}

fn set(mut r: Ring, f: impl FnOnce(&mut Ring)) -> Ring {
    f(&mut r);
    r
}

fn notes(out: &[Option<NoteOut>]) -> Vec<u8> {
    out.iter().flatten().map(|o| o.note.get()).collect()
}

// Euclid

#[test]
fn ranges_are_types() {
    assert!(Steps::new(0).is_none());
    assert!(Steps::new(1).is_some());
    assert!(Steps::new(32).is_some());
    assert!(Steps::new(33).is_none());
    assert!(
        Fill::new(steps(8), 9, 0).is_none(),
        "hits never exceed steps"
    );
    assert_eq!(fill(8, 3, 10).rotate(), 2, "rotate wraps");
    assert_eq!(fill(8, 7, 0).with_steps(steps(4)).hits(), 4);
    assert!(Pct::new(101).is_none());
    assert_eq!(Pct::clamped(200), Pct::FULL);
    assert!(Octave::new(9).is_none());
    assert_eq!(Octave::new(3).unwrap().low(), n(48));
    assert!(RingIdx::new(5).is_none());
}

#[test]
fn known_euclidean_rhythms() {
    assert_eq!(e(8, 3, 0), "x..x..x.", "tresillo");
    assert_eq!(e(8, 5, 0), "x.xx.xx.", "cinquillo");
    assert_eq!(e(16, 4, 0), "x...x...x...x...");
    assert_eq!(e(12, 5, 0), "x..x.x..x.x.");
    assert_eq!(e(13, 5, 0), "x..x.x..x.x..");
    assert_eq!(e(4, 0, 0), "....");
    assert_eq!(e(4, 4, 0), "xxxx");
    assert_eq!(e(1, 1, 0), "x");
    assert_eq!(e(32, 32, 0), "x".repeat(32));
    assert_eq!(e(4, 9, 0), "xxxx", "hits clamp to steps");
}

#[test]
fn every_fill_is_even() {
    for s in 1..=32u8 {
        for k in 0..=s {
            let m = euclid(steps(s), k, 0);
            assert_eq!(m.hits(), k);
            if k == 0 {
                continue;
            }
            let at: Vec<usize> = (0..s as usize).filter(|&i| m.is_hit(i)).collect();
            let gaps: Vec<usize> = at
                .iter()
                .zip(at.iter().cycle().skip(1))
                .map(|(a, b)| (b + s as usize - a - 1) % s as usize + 1)
                .collect();
            let (lo, hi) = (gaps.iter().min().unwrap(), gaps.iter().max().unwrap());
            assert!(hi - lo <= 1, "{s}/{k}: {gaps:?}");
            assert!(m.is_hit(0), "{s}/{k} starts on a hit");
        }
    }
}

#[test]
fn rotate_shifts_like_the_spec_formula() {
    // Step i plays step i + ROTATE of the unrotated fill.
    assert_eq!(e(8, 3, 1), "..x..x.x");
    assert_eq!(e(8, 3, 2), ".x..x.x.");
    assert_eq!(e(8, 3, 8), "x..x..x.");
    assert_eq!(e(32, 1, 31), format!(".x{}", ".".repeat(30)));
    for r in 0..8u8 {
        let a = euclid(steps(8), 3, 0);
        let b = euclid(steps(8), 3, r);
        assert!((0..8).all(|i| b.is_hit(i) == a.is_hit(i + r as usize)));
    }
}

#[test]
fn steps_past_the_bar_wrap() {
    let m = euclid(steps(3), 1, 0);
    assert!(m.is_hit(0) && m.is_hit(3) && m.is_hit(6));
    assert!(!m.is_hit(4));
}

#[test]
fn hit_index_counts_hits() {
    let m = euclid(steps(8), 5, 0); // x.xx.xx.
    let idx: Vec<_> = (0..8).map(|i| m.hit_index(i)).collect();
    assert_eq!(
        idx,
        [
            Some(0),
            None,
            Some(1),
            Some(2),
            None,
            Some(3),
            Some(4),
            None
        ]
    );
}

#[test]
fn accents_ride_the_hits() {
    let m = euclid(steps(8), 5, 0); // hits at 0 2 3 5 6
    // 2 accents over 5 hits: x.x.. -> hits 0 and 2 -> steps 0 and 3.
    assert_eq!(show(accents(m, 2)), "x..x....");
    // Whatever the nudges: move the hit on 3 to 4; hits 0 and 2 are now
    // steps 0 and 4.
    let nudged = m.moved(3, 4).unwrap();
    assert_eq!(show(accents(nudged, 2)), "x...x...");
    // More accents than hits: every hit. None: none.
    assert_eq!(accents(m, 32), m);
    assert_eq!(accents(m, 0).hits(), 0);
    assert_eq!(accents(HitMask::empty(steps(8)), 3).hits(), 0);
}

#[test]
fn a_hit_moves_only_into_a_free_step() {
    let m = euclid(steps(8), 3, 0); // x..x..x.
    assert!(m.moved(0, 1).is_some());
    assert!(m.moved(1, 2).is_none(), "1 is not a hit");
    assert!(m.moved(0, 3).is_none(), "3 is taken");
    assert_eq!(show(m.moved(0, 7).unwrap()), "...x..xx");
}

// Rng

#[test]
fn rng_is_seeded() {
    let draws = |mut r: Rng| (0..64).map(|_| r.next_u32()).collect::<Vec<_>>();
    assert_eq!(draws(Rng::new(7)), draws(Rng::new(7)));
    assert_ne!(draws(Rng::new(7)), draws(Rng::new(8)));
    assert_ne!(Rng::new(0).next_u32(), 0, "seed 0 does not stick");
    let [a, b, ..] = RingIdx::ALL.map(|i| draws(Rng::for_ring(42, i)));
    assert_ne!(a, b, "each ring has its own dice");
    assert_eq!(
        draws(Rng::for_ring(42, RingIdx::ALL[1])),
        b,
        "and they replay"
    );
}

#[test]
fn rng_below_and_chance() {
    let mut r = Rng::new(1);
    let mut seen = [false; 5];
    for _ in 0..1000 {
        let x = r.below(5);
        assert!(x < 5);
        seen[x as usize] = true;
    }
    assert!(seen.iter().all(|&s| s));
    assert!((0..10_000).all(|_| r.chance(Pct::FULL)));
    assert!((0..10_000).all(|_| !r.chance(Pct::ZERO)));
    let k = (0..10_000).filter(|_| r.chance(pct(50))).count();
    assert!((4_500..5_500).contains(&k), "{k}");
}

// Walk

fn pool_ring(pool: &[u8], walk: Walk, range: Range, hits: u8) -> Ring {
    let mut r = ring(hits, hits);
    r.pool = Pool::new(&pool.iter().map(|&x| n(x)).collect::<Vec<_>>()).unwrap();
    r.walk = walk;
    r.range = range;
    r.mode = NoteMode::Pool;
    r
}

fn walk(r: &Ring, count: usize, seed: u32) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut st = WalkState::START;
    (0..count)
        .map(|_| chimera_core::orbit::walk::walk_next(r, &mut st, &mut rng).get())
        .collect()
}

#[test]
fn pool_is_one_to_eight_sorted_notes() {
    assert!(Pool::new(&[]).is_none());
    assert!(Pool::new(&[n(60); 8]).is_some());
    assert!(Pool::new(&[n(60); 9]).is_none());
    let p = Pool::new(&[n(67), n(60), n(64)]).unwrap();
    assert_eq!(p.notes(), [n(60), n(64), n(67)]);
    assert_eq!(
        p.with(4, n(50)).notes(),
        [n(50), n(60), n(67)],
        "wraps, re-sorts"
    );
}

#[test]
fn walk_modes() {
    let r = pool_ring(&[60, 64, 67, 71], Walk::Up, Range::Zero, 4);
    assert_eq!(walk(&r, 6, 1), [60, 64, 67, 71, 60, 64]);
    let r = set(r, |r| {
        r.walk = Walk::Down;
    });
    assert_eq!(walk(&r, 6, 1), [60, 71, 67, 64, 60, 71]);
    let r = set(r, |r| {
        r.walk = Walk::PingPong;
    });
    assert_eq!(walk(&r, 9, 1), [60, 64, 67, 71, 67, 64, 60, 64, 67]);
    let r = set(r, |r| {
        r.walk = Walk::Random;
    });
    let rand = walk(&r, 200, 9);
    for x in [60, 64, 67, 71] {
        assert!(rand.contains(&x));
    }
    assert!(rand.iter().all(|x| [60, 64, 67, 71].contains(x)));
    assert_eq!(rand, walk(&r, 200, 9));
    for m in [Walk::Up, Walk::Down, Walk::PingPong, Walk::Random] {
        let one = pool_ring(&[50], m, Range::Zero, 3);
        assert!(walk(&one, 5, 1).iter().all(|&x| x == 50));
    }
}

#[test]
fn range_walks_the_pool_up_octaves() {
    let r = pool_ring(&[60, 64, 67], Walk::Up, Range::One, 3);
    assert_eq!(walk(&r, 7, 1), [60, 64, 67, 72, 76, 79, 60]);
    let r = set(r, |r| {
        r.walk = Walk::PingPong;
    });
    assert_eq!(
        walk(&r, 11, 1),
        [60, 64, 67, 72, 76, 79, 76, 72, 67, 64, 60]
    );
    // Past 127 folds back an octave.
    let r = pool_ring(&[120], Walk::Up, Range::Three, 3);
    assert!(walk(&r, 8, 1).iter().all(|&x| x <= 127));
}

fn period(xs: &[(usize, u8)]) -> Option<usize> {
    (1..=xs.len() / 2).find(|&t| (0..xs.len() - t).all(|i| xs[i] == xs[i + t]))
}

#[test]
fn pool_walks_phase() {
    // A 3-note pool UP on 5 hits comes round in 15 hits; RANGE 1 in 30.
    let h = a_minor();
    for (range, want) in [(Range::Zero, 15), (Range::One, 30)] {
        let r = pool_ring(&[57, 60, 64], Walk::Up, range, 5);
        let (out, _) = run(r, &h, 1, 24);
        let pairs: Vec<_> = notes(&out)
            .into_iter()
            .enumerate()
            .map(|(i, x)| (i % 5, x))
            .collect();
        assert_eq!(period(&pairs), Some(want));
    }
}

#[test]
fn the_walk_advances_on_a_missed_hit() {
    let h = a_minor();
    let all = pool_ring(&[57, 60, 64], Walk::Up, Range::Zero, 5);
    let some = set(all, |r| {
        r.chance = pct(50);
    });
    let (full, _) = run(all, &h, 3, 6);
    let (half, _) = run(some, &h, 3, 6);
    assert!(half.iter().any(|o| o.is_none()));
    for (a, b) in full.iter().zip(&half) {
        if let Some(b) = b {
            assert_eq!(Some(b.note), a.map(|a| a.note));
        }
    }
}

#[test]
fn walk_survives_a_shrinking_pool() {
    let big = pool_ring(&[60, 62, 64, 65, 67], Walk::PingPong, Range::Zero, 5);
    let mut rng = Rng::new(3);
    let mut st = WalkState::START;
    for _ in 0..4 {
        chimera_core::orbit::walk::walk_next(&big, &mut st, &mut rng);
    }
    for m in [Walk::Up, Walk::Down, Walk::PingPong] {
        let small = pool_ring(&[70, 72], m, Range::Zero, 5);
        let mut s = st;
        let x = chimera_core::orbit::walk::walk_next(&small, &mut s, &mut rng);
        assert!([70, 72].contains(&x.get()));
    }
}

// Ring

#[test]
fn chance_full_always_fires_and_zero_never() {
    let h = a_minor();
    let r = ring(13, 5);
    let (out, _) = run(r, &h, 5, 50);
    assert_eq!(out.iter().flatten().count(), 5 * 50);
    let (out, _) = run(
        set(r, |r| {
            r.chance = Pct::ZERO;
        }),
        &h,
        5,
        50,
    );
    assert_eq!(out.iter().flatten().count(), 0);
}

#[test]
fn mute_sounds_nothing_and_keeps_the_dice() {
    let h = a_minor();
    let r = set(
        pool_ring(&[57, 60, 64, 67], Walk::Random, Range::One, 7),
        |r| {
            r.chance = pct(70);
            r.walk = Walk::Random;
        },
    );
    let mut rng_a = Rng::new(9);
    let mut rng_b = Rng::new(9);
    let (mut wa, mut wb) = (WalkState::START, WalkState::START);
    let muted = set(r, |r| {
        r.mute = true;
    });
    for i in 0..70 {
        let _ = r.play(i, &mut wa, &h, &mut rng_a);
        assert_eq!(muted.play(i, &mut wb, &h, &mut rng_b), None);
    }
    assert_eq!((rng_a, wa), (rng_b, wb));
}

#[test]
fn step_rings_play_each_steps_note() {
    let h = a_minor();
    let mut r = ring(4, 4);
    r.mode = NoteMode::Step;
    r.notes[..4].copy_from_slice(&[n(57), n(60), n(64), n(67)]);
    let (out, _) = run(r, &h, 1, 2);
    assert_eq!(notes(&out), [57, 60, 64, 67, 57, 60, 64, 67]);
}

#[test]
fn notes_snap_to_the_ring_target() {
    let h = a_minor(); // A minor, chord Dm (D F A)
    let r = pool_ring(&[58, 61, 63, 66, 68], Walk::Up, Range::Zero, 5);
    let (out, _) = run(r, &h, 1, 1);
    // A# B | C# C D | D# D E | F# F G | G# G A: ties go down.
    assert_eq!(notes(&out), [57, 60, 62, 65, 67]);
    let r = set(r, |r| {
        r.snap = SnapTo::Chord;
    });
    let (out, _) = run(r, &h, 1, 1);
    // To D F A: A#->A, C#->D, D#->D, F#->F, G#->A.
    assert_eq!(notes(&out), [57, 62, 62, 65, 69]);
}

#[test]
fn accents_play_louder() {
    let h = a_minor();
    let mut r = ring(8, 5);
    r.accent = 2;
    r.velocity = Velocity::new(80).unwrap();
    let (out, _) = run(r, &h, 1, 1);
    let acc: Vec<_> = out
        .iter()
        .enumerate()
        .filter_map(|(i, o)| o.map(|o| (i, o.accent, o.velocity.get())))
        .collect();
    assert_eq!(
        acc,
        [
            (0, true, 80 + ACCENT_BOOST),
            (2, false, 80),
            (3, true, 80 + ACCENT_BOOST),
            (5, false, 80),
            (6, false, 80)
        ]
    );
    r.velocity = Velocity::new(120).unwrap();
    let (out, _) = run(r, &h, 1, 1);
    assert_eq!(out[0].unwrap().velocity, Velocity::MAX);
}

// Mutate

#[test]
fn mutate_zero_locks() {
    let h = a_minor();
    let r = set(
        pool_ring(&[57, 60, 64], Walk::Random, Range::Two, 13),
        |r| {
            r.chance = pct(70);
        },
    );
    let mut rng = Rng::new(11);
    for _ in 0..10_000 {
        let before = rng;
        assert_eq!(mutate(&r, &h, &mut rng), None);
        assert_eq!(rng, before, "draws nothing");
    }
    let (_, after) = run(r, &h, 11, 100);
    assert_eq!(after, r);
}

#[test]
fn mutate_keeps_count_and_register() {
    let h = a_minor();
    for (mode, range, oct) in [
        (NoteMode::Pool, Range::Zero, 3),
        (NoteMode::Pool, Range::Three, 0),
        (NoteMode::Step, Range::One, 8),
    ] {
        let mut r = pool_ring(&[57, 60, 64], Walk::Up, range, 13);
        r = r.with_fill(fill(13, 5, 2));
        r.mode = mode;
        r.octave = Octave::new(oct).unwrap();
        r.mutate = Pct::FULL;
        let lo = r.octave.low().get();
        let hi = (lo as u16 + 12 * (range.octaves() as u16 + 1)).min(128) as u8;
        let orig = r;
        let mut rng = Rng::new(21);
        let mut walk = WalkState::START;
        let mut swaps = 0;
        let mut nudges = 0;
        for _ in 0..1000 {
            for i in 0..13 {
                if let Some(o) = r.play(i, &mut walk, &h, &mut rng) {
                    assert!(h.key.contains(o.note.pc()));
                }
            }
            match mutate(&r, &h, &mut rng) {
                Some(m @ Mutation::Swap { note, .. }) => {
                    assert!((lo..hi).contains(&note.get()), "{note} in {lo}..{hi}");
                    assert!(h.key.contains(note.pc()));
                    swaps += 1;
                    r = r.apply(m);
                }
                Some(m @ Mutation::Nudge { from, to }) => {
                    assert!(r.hits_mask().is_hit(from) && !r.hits_mask().is_hit(to));
                    nudges += 1;
                    r = r.apply(m);
                }
                None => {}
            }
            assert_eq!(r.hits_mask().hits(), 5);
        }
        assert!(swaps > 300 && nudges > 300, "{swaps} {nudges}");
        assert_ne!(r, orig);
    }
}

#[test]
fn a_full_ring_has_nowhere_to_nudge() {
    let h = a_minor();
    let mut r = ring(4, 4);
    r.mutate = Pct::FULL;
    let mut rng = Rng::new(2);
    for _ in 0..200 {
        assert!(!matches!(
            mutate(&r, &h, &mut rng),
            Some(Mutation::Nudge { .. })
        ));
    }
}

#[test]
fn mutate_back_to_zero_freezes() {
    let h = a_minor();
    let mut r = pool_ring(&[57, 60, 64], Walk::Up, Range::One, 8);
    r = r.with_fill(fill(8, 3, 0));
    r.mutate = Pct::FULL;
    let (_, drifted) = run(r, &h, 4, 10);
    let frozen = set(drifted, |r| {
        r.mutate = Pct::ZERO;
    });
    let (_, later) = run(frozen, &h, 99, 200);
    assert_eq!(later, frozen);
}

#[test]
fn a_fixed_seed_replays_exactly() {
    let h = a_minor();
    let r = set(
        pool_ring(&[57, 60, 64, 69], Walk::Random, Range::Two, 13),
        |r| {
            r.chance = pct(70);
            r.mutate = pct(60);
            r.accent = 2;
        },
    )
    .with_fill(fill(13, 5, 2));
    let a = run(r, &h, 1234, 64);
    let b = run(r, &h, 1234, 64);
    assert_eq!(a, b);
    let c = run(r, &h, 1235, 64);
    assert_ne!(a.0, c.0);
}

// Size

#[test]
fn state_fits_in_a_kilobyte() {
    const { assert!(STATE_BYTES < 1024) };
    println!("orbit state: {STATE_BYTES} B");
}
