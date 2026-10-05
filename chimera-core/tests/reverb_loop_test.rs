//! ROOM on the LOOP engine (reverb TYPEs spec § LOOP engine, § Level
//! plan, § Moving a length, § Pitch bounds, § Tests).

mod common;

use std::cell::RefCell;

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::halfband::{Decimator, HALF, Interpolator};
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::arena::ReverbArena;
use chimera_core::dsp::rev::loop_engine::{Layout, PER_MS, Room, STEP, THETA, rotate, rotation};
use chimera_core::dsp::rev::parts::{Wander, wander_seeds};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType, knobs};
use chimera_core::dsp::ring::RING_LEN;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{
    MS, Noise, blocks, check_knobs_ease, check_level_plan, check_no_slap, check_noise_floor,
    check_settles_on_pitch, check_sustained_loudness, onset_ms, pink, rt60_t20, run,
    settled_peak_hz, tone,
};

type Blk = [f32; BLOCK_SIZE];
type L = Room;
const LENS: chimera_core::dsp::rev::loop_engine::Lens<2, 2> = <L as Layout<2, 2>>::LENS;

/// ROOM's starting knobs at MIX `mix`.
fn params(mix: f32) -> ReverbParams {
    ReverbParams {
        mix,
        ..ReverbParams::default()
    }
    .with_type(ReverbType::Room, &KnobMemory::new())
}

fn tick(rv: &mut Reverb, send: &Blk, p: &ReverbParams) -> (Blk, Blk) {
    let mut out = Stereo::SILENT;
    rv.process(send, p, p.mix, Tempo::DEFAULT, SR, &mut out);
    (out.l, out.r)
}

fn bus<'a>(
    rv: &'a RefCell<Box<Reverb>>,
    p: &'a RefCell<ReverbParams>,
) -> impl FnMut(&Blk) -> (Blk, Blk) + 'a {
    move |send| tick(&mut rv.borrow_mut(), send, &p.borrow())
}

fn fresh(p: &ReverbParams) -> RefCell<Box<Reverb>> {
    RefCell::new(Box::new(Reverb::of_type(p.kind)))
}

fn is_prime(n: usize) -> bool {
    n >= 2
        && (2..)
            .take_while(|d| d * d <= n)
            .all(|d| !n.is_multiple_of(d))
}

/// `ms` at 24 kHz to the nearest prime not in `taken` (the lower on a tie).
fn nearest_prime(ms: f32, taken: &[usize]) -> usize {
    let want = (ms * 24.0).round() as usize;
    (0..)
        .flat_map(|d| [want - d, want + d])
        .find(|&n| is_prime(n) && !taken.contains(&n))
        .unwrap()
}

#[test]
fn room_layout_fits_and_lengths_are_distinct_primes() {
    let spec_ms: [f32; 12] = [
        2.3, 3.7, 5.9, 8.9, // diffuser
        59.0, 101.0, 73.0, 95.0, // delays A1 A2 B1 B2
        17.0, 23.0, 13.0, 19.0, // stage APs A1 A2 B1 B2
    ];
    let got: Vec<usize> = LENS
        .diffuser
        .iter()
        .chain(LENS.delay.as_flattened())
        .chain(LENS.ap.as_flattened())
        .copied()
        .collect();
    let mut taken = Vec::new();
    for (&ms, &n) in spec_ms.iter().zip(&got) {
        assert!(is_prime(n), "{ms} ms → {n}");
        assert_eq!(n, nearest_prime(ms, &taken), "{ms} ms");
        taken.push(n);
    }
    assert_eq!(LENS.pre, 1_200, "PRE-DLY's 50 ms");
    let i16_slots: usize = [LENS.pre]
        .iter()
        .chain(LENS.delay.as_flattened())
        .chain(LENS.ap.as_flattened())
        .map(|n| n + 2)
        .sum();
    let f32_slots: usize = LENS.diffuser.iter().map(|n| 2 * (n + 2)).sum();
    assert!(i16_slots + f32_slots <= RING_LEN);
    const { assert!(<L as Layout<2, 2>>::SLOTS + 2 * <L as Layout<2, 2>>::FSLOTS <= RING_LEN) };
}

#[test]
fn rotation_preserves_energy() {
    let (c, s) = rotation(THETA);
    let e = (c as f64).powi(2) + (s as f64).powi(2);
    assert!((e - 1.0).abs() <= 1e-7, "c² + s² = {e}");
    assert!((c - THETA.cos()).abs() < 1e-6 && (s - THETA.sin()).abs() < 1e-6);
    // Two lossless loops (gains 1, dampers off) of ROOM's SIZE-1 round
    // trips, coupled by the engine's own `rotate` in f32: their stored
    // energy holds.
    let trip = |l: usize| LENS.delay[l].iter().chain(&LENS.ap[l]).sum::<usize>();
    let mut n = Noise(7);
    let mut lines: [Vec<f32>; 2] =
        core::array::from_fn(|l| (0..trip(l)).map(|_| n.next()).collect());
    let energy = |x: &[Vec<f32>; 2]| {
        x.iter()
            .flatten()
            .map(|&v| v as f64 * v as f64)
            .sum::<f64>()
    };
    let e0 = energy(&lines);
    let mut at = [0usize; 2];
    for _ in 0..10 * trip(0).max(trip(1)) {
        (lines[0][at[0]], lines[1][at[1]]) = rotate(lines[0][at[0]], lines[1][at[1]]);
        at = [(at[0] + 1) % trip(0), (at[1] + 1) % trip(1)];
    }
    let drift = energy(&lines) / e0 - 1.0;
    assert!(drift.abs() <= 1e-6, "energy drifts {drift}");
}

/// The cached targets follow every move: DECAY, TONE, then SIZE turned
/// mid-run, and once the lengths land the targets equal a fresh engine's
/// primed at the final knobs. Without `retarget` the stage gains would
/// keep the lengths DECAY last saw.
#[test]
fn room_targets_follow_decay_tone_and_size() {
    let mut p = params(1.0);
    let (mut e, mut arena) = engine(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    for b in 0..1_200 {
        match b {
            10 => p.time = 1.0,
            20 => p.tone = 0.9,
            30 => p.size = 1.0,
            _ => {}
        }
        e.run(&mut arena, &u, &knobs(&p), &mut l, &mut r);
    }
    assert!(!e.moving(), "the move lands");
    let (fresh, _) = engine(&p);
    assert_eq!(e.delays(), fresh.delays());
    let ((ta, da, g), (fta, fda, fg)) = (e.targets(), fresh.targets());
    assert_eq!((ta, da), (fta, fda));
    assert_eq!(g, fg, "stage-gain targets stale");
}

/// ROOM alone at the ring's rate: the engine, primed on `p`.
fn engine(p: &ReverbParams) -> (L, Box<ReverbArena>) {
    let mut e = L::default();
    e.prime(&knobs(p));
    (e, Box::new(ReverbArena::new()))
}

#[test]
fn round_trip_moves_at_most_5_ms_per_crossfade() {
    let mut p = params(1.0);
    (p.size, p.predelay) = (0.0, 0.0);
    let (mut e, mut arena) = engine(&p);
    let trip = |d: [[u16; 2]; 2], l: usize| d[l].iter().map(|&x| x as i32).sum::<i32>();
    let k0 = e.delays();
    for (l, ap) in LENS.ap.iter().enumerate() {
        let ms = (trip(k0, l) as usize + ap.iter().sum::<usize>()) as f32 / 24.0;
        println!("loop {l} at SIZE 0: {ms:.1} ms");
    }
    (p.size, p.predelay) = (1.0, 1.0);
    let k = knobs(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    let mut crossfades = 0;
    let mut last = None;
    for b in 0..2_000 {
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        if let Some((from, to, pre)) = e.crossfade() {
            if last != Some((from, to, pre)) {
                crossfades += 1;
                for l in 0..2 {
                    let d = (trip(to, l) - trip(from, l)).abs();
                    assert!(d <= 120, "block {b}: loop {l} moves {d}");
                }
                assert!(pre.0.abs_diff(pre.1) <= 120, "block {b}: PRE {pre:?}");
                // One slice a loop, every loop still moving in the same
                // crossfade: the round trips move together.
                for l in 0..2 {
                    let moved = (0..2).filter(|&s| from[l][s] != to[l][s]).count();
                    let left = (0..2).any(|s| to[l][s] as usize != LENS.delay[l][s]);
                    assert!(moved <= 1, "block {b}: loop {l} moves {moved} delays");
                    assert!(moved == 1 || !left || from[l] == to[l] && from[1 - l] == to[1 - l]);
                }
            }
            last = Some((from, to, pre));
        } else if e.delays() == LENS.delay.map(|l| l.map(|x| x as u16))
            && e.predelay() == LENS.pre as u16
        {
            println!("{crossfades} crossfades; landed at block {b}");
            assert!((18..=26).contains(&crossfades), "{crossfades}");
            return;
        }
    }
    panic!("never landed");
}

#[test]
fn room_settles_on_its_input_pitch() {
    let mut p = params(1.0);
    (p.time, p.smear, p.size, p.predelay) = (1.0, 0.0, 0.0, 0.0);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let start = blocks(0.25);
    // SIZE and PRE-DLY in the same crossfades (§ Moving a length): a
    // sweep after a sweep would judge a tail 70 dB down, at its LSBs.
    let mut step = |b: usize| {
        if b == start {
            let mut p = pc.borrow_mut();
            (p.size, p.predelay) = (1.0, 1.0);
        }
        b >= start && rv.borrow().room().expect("ROOM runs").moving()
    };
    check_settles_on_pitch(&mut bus(&rv, &pc), &mut step, 0.2);
}

/// SMEAR's cap (§ The SMEAR bound) where ROOM keeps it: each loop's one
/// smeared allpass reads within [M − 2D, M], never under 2, and 2D is at
/// most 1.45 % of that loop's round trip at every SIZE, however deep the
/// offsets asked. The 25 cents follow from it. `check_smear_bound`'s zero
/// crossings can't judge it here: two coupled loops 100 and 95.4 ms long
/// beat, so a still ROOM at SMEAR 0 already reads ±45 cents pass to pass.
#[test]
fn room_smear_reads_stay_within_the_cap() {
    let mut p = params(1.0);
    (p.smear, p.rate) = (1.0, 1.0);
    for i in 0..=16 {
        p.size = i as f32 / 16.0;
        let k = knobs(&p);
        assert!(k.smear_ms <= 0.0072 * k.length_ms + 1e-6, "SIZE {}", p.size);
        let (mut e, mut arena) = engine(&p);
        let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
        e.set_smear([(1e9, -1e9), (-5.0, 1e9)]);
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let held = e.smear();
        for (m, &(lp, st)) in <L as Layout<2, 2>>::SMEARED.iter().enumerate() {
            let trip: usize = e.delays()[lp].iter().map(|&d| d as usize).sum::<usize>()
                + LENS.ap[lp].iter().sum::<usize>();
            for x in [held[m].0, held[m].1] {
                assert!(
                    (0.0..=2.0 * 0.0072 * trip as f32 + 1e-3).contains(&x),
                    "{x} of {trip}"
                );
                assert!(LENS.ap[lp][st] as f32 - x >= 2.0);
            }
            // E ≤ 1.45 % of L: 1200·log2(1/(1 − E/L)) ≈ 25 cents.
            let e_share = held[m].0.max(held[m].1) / trip as f32;
            assert!(e_share <= 0.0145, "SIZE {}: loop {lp} E {e_share}", p.size);
        }
    }
}

/// ROOM alone at `p`, its two wanders run as `Reverb` runs them, and its
/// decay held long: RT60 20 s holds every stage at its 0.985 cap (22 s at
/// SIZE 0), so a tail lasts the 4 s windows a wander's drift needs.
fn smeared(p: ReverbParams) -> impl FnMut(&Blk) -> (Blk, Blk) {
    let mut k = knobs(&p);
    k.rt60_s = 20.0;
    let full = k.smear_ms * PER_MS;
    let seeds = wander_seeds::<2>(0x5EED_0343);
    let mut w: [Wander; 2] = core::array::from_fn(|i| {
        let mut w = Wander::new(seeds[i], i as f32 / 2.0);
        w.set(Wander::rate(k.rate_hz, 24_000.0));
        w
    });
    let (mut e, mut arena) = (L::default(), Box::new(ReverbArena::new()));
    let (mut dec, mut up) = (Decimator::new(), [Interpolator::new(), Interpolator::new()]);
    move |send| {
        let (mut u, mut l, mut r) = ([0.0; HALF], [0.0; HALF], [0.0; HALF]);
        dec.process(send, &mut u);
        let x0 = w.map(|w| w.value());
        let x1: [f32; 2] = core::array::from_fn(|i| w[i].block());
        e.set_smear(core::array::from_fn(|i| {
            (full * (1.0 + x0[i]), full * (1.0 + x1[i]))
        }));
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let (mut ol, mut or) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        up[0].process(&l, &mut ol);
        up[1].process(&r, &mut or);
        (ol.map(|x| x / 16_384.0), or.map(|x| x / 16_384.0))
    }
}

/// SMEAR leaves a held tone's tail where it was: a 1 kHz sine's tail,
/// held long, has its spectral peak in two 4 s windows (1.5–5.5 s and
/// 5.5–9.5 s, above −60 dB); at SMEAR 1 and RATE 1 the peak hops between
/// them by no more than 25 cents beyond SMEAR 0's own hop, at SIZE 0.
#[test]
fn room_smear_does_not_move_the_tails_peak() {
    let hop = |smear: f32| {
        let mut p = params(1.0);
        (p.smear, p.rate, p.size) = (smear, 1.0, 0.0);
        let sine = tone(1_000.0, 0.25);
        let (l, _) = run(
            &mut smeared(p),
            &mut |i| if i < SR as usize / 2 { sine(i) } else { 0.0 },
            9.5,
        );
        let at = |a: f32, b: f32| {
            settled_peak_hz(&l[(a * SR as f32) as usize..(b * SR as f32) as usize])
        };
        let (f1, f2) = (at(1.5, 5.5), at(5.5, 9.5));
        println!("SMEAR {smear}: {f1:.2} → {f2:.2} Hz");
        (1200.0 * (f2 / f1).log2()).abs()
    };
    let (still, moving) = (hop(0.0), hop(1.0));
    assert!(
        moving <= still + 25.0,
        "SMEAR 1 hops {moving} cents, SMEAR 0 {still}"
    );
}

/// ROOM at DECAY 1: noise for 2 s, SIZE from `from` to 1 from `start`
/// seconds, so the sweep lands as the noise ends or after; then the tail
/// from `LATE` after the noise to 6 s. `coarse` steps 50 ms a crossfade.
fn size_tail(from: f32, start: f32, coarse: u16) -> Vec<f32> {
    let mut p = params(1.0);
    (p.time, p.size) = (1.0, from);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    if coarse > 0 {
        rv.borrow_mut()
            .room_mut()
            .expect("ROOM")
            .coarse_steps(coarse);
    }
    let mut n = Noise(0xface);
    let mut render = bus(&rv, &pc);
    let mut out = Vec::new();
    for b in 0..blocks(6.0) {
        if b == blocks(start) {
            pc.borrow_mut().size = 1.0;
        }
        let send = core::array::from_fn(|i| {
            if (b * BLOCK_SIZE + i) < 2 * SR as usize {
                0.25 * n.next()
            } else {
                0.0
            }
        });
        out.extend_from_slice(&render(&send).0);
    }
    out.split_off(2 * SR as usize + LATE)
}

/// Where a tail is judged from: once the longest fixed tap (0.83 of A2's
/// line, 84 ms) has heard the send's end, and the diffuser's allpasses
/// (2.2–8.9 ms, an echo train at their own lengths while they ring out)
/// have rung down. Before it, those are the tail.
const LATE: usize = 100 * MS;

/// The 0 → 1 sweep (22 crossfades, 0.66 s) from 1.4 s lands at 2.06 s,
/// as the noise ends: the judged tail is the settled one. And the check
/// has teeth: 50 ms steps (test-only), crossfading inside the judged tail
/// from 2.1 s, slap. (It hears 10 ms steps too, not 20 or 100 ms; and a
/// 5 ms sweep still running in the tail reads up to −10.7 dB, the
/// crossfade's own comb, § Moving a length.)
#[test]
fn no_slap_after_a_size_sweep() {
    let still = size_tail(1.0, 1.4, 0);
    check_no_slap(&size_tail(0.0, 1.4, 0), &still);
    let coarse = size_tail(0.0, 2.1, 10 * STEP);
    let heard = std::panic::catch_unwind(|| check_no_slap(&coarse, &still));
    assert!(heard.is_err(), "50 ms steps pass the slap check");
}

/// A 0.5 s noise burst into ROOM on `p`, MIX 1, `seconds` long: (L, R).
fn noise_burst(p: &ReverbParams, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let (rv, pc) = (fresh(p), RefCell::new(*p));
    let mut n = Noise(0x5eed);
    run(
        &mut bus(&rv, &pc),
        &mut |i| {
            if i < SR as usize / 2 {
                0.5 * n.next()
            } else {
                0.0
            }
        },
        seconds,
    )
}

/// RT60 by T20 on `p`'s tail, from `LATE` after a 0.5 s noise burst.
fn rt60_of(p: &ReverbParams) -> f32 {
    let (l, r) = noise_burst(p, 0.5 + 3.0 * knobs(p).rt60_s.max(0.3));
    let tail = |x: &[f32]| x[SR as usize / 2 + LATE..].to_vec();
    rt60_t20(&tail(&l), &tail(&r))
}

#[test]
fn room_window_and_rt60() {
    let p = params(1.0);
    let (l, r) = noise_burst(&p, 1.0);
    let onset = onset_ms(&l).min(onset_ms(&r));
    let rt = rt60_of(&p);
    println!("defaults: RT60 {rt:.3} s, onset {onset} ms");
    assert!((rt / 0.548 - 1.0).abs() <= 0.2, "RT60 {rt}");
    assert!(onset <= 34.0, "onset {onset} ms");
    // DECAY 0 checks the rotation. No `t_min` floors it: the stage
    // allpasses' g is held to their own ring, DIFFUSE 1's too.
    for (decay, diffuse) in [
        (0.0, 0.4),
        (0.25, 0.4),
        (0.5, 0.4),
        (0.75, 0.4),
        (1.0, 0.4),
        (0.0, 1.0),
    ] {
        let mut q = p;
        (q.time, q.diffuse) = (decay, diffuse);
        let want = knobs(&q).rt60_s;
        let law = 0.2 * 7.5f32.powf(decay);
        assert!(
            (want / law - 1.0).abs() < 1e-3,
            "{decay}: {want}, the law {law}"
        );
        let rt = rt60_of(&q);
        println!("DECAY {decay} DIFFUSE {diffuse}: RT60 {rt:.3} s, target {want:.3} s");
        assert!(
            (rt / want - 1.0).abs() <= 0.2,
            "DECAY {decay}: {rt} vs {want}"
        );
    }
}

/// ROOM alone at the ring's rate, behind the bus's half-bands: the i16
/// engine, or its unclipped or f32-line references.
struct Alone {
    e: L,
    arena: Box<ReverbArena>,
    f32_lines: Option<Vec<f32>>,
    clipped: bool,
    dec: Decimator,
    up: [Interpolator; 2],
}

impl Alone {
    fn new(f32_lines: bool, clipped: bool) -> Self {
        Self {
            e: L::default(),
            arena: Box::new(ReverbArena::new()),
            f32_lines: f32_lines.then(|| vec![0.0; <L as Layout<2, 2>>::SLOTS]),
            clipped,
            dec: Decimator::new(),
            up: [Interpolator::new(), Interpolator::new()],
        }
    }

    fn block(&mut self, send: &Blk, p: &ReverbParams) -> (Blk, Blk) {
        let (mut u, mut l, mut r) = ([0.0; HALF], [0.0; HALF], [0.0; HALF]);
        self.dec.process(send, &mut u);
        let k = knobs(p);
        self.e.run_reference(
            &mut self.arena,
            self.f32_lines.as_deref_mut(),
            self.clipped,
            &u,
            &k,
            &mut l,
            &mut r,
        );
        let (mut ol, mut or) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        self.up[0].process(&l, &mut ol);
        self.up[1].process(&r, &mut or);
        let g = 1.0 / 16_384.0;
        (ol.map(|x| x * g), or.map(|x| x * g))
    }

    /// The largest |value| each stored line region holds now, in line
    /// units (1.0 full scale): PRE, the stage delays, the stage APs.
    fn peaks(&self) -> [f32; 3] {
        let lines = self.f32_lines.as_deref().expect("f32 lines");
        <L as Layout<2, 2>>::regions()
            .map(|r| lines[r].iter().fold(0.0f32, |m, x| m.max(x.abs())) / 16_384.0)
    }
}

#[test]
fn level_plan_holds_for_room() {
    let mut p = params(1.0);
    (p.size, p.diffuse) = (0.0, 1.0);
    let pc = RefCell::new(p);
    let mut a = Alone::new(false, true);
    let mut b = Alone::new(false, false);
    let mut c = Alone::new(true, false);
    let mode = 1_000.0 / knobs(&p).length_ms;
    {
        let mut ra = |s: &Blk| a.block(s, &pc.borrow());
        let mut rb = |s: &Blk| b.block(s, &pc.borrow());
        let mut rc = |s: &Blk| c.block(s, &pc.borrow());
        check_level_plan(
            &mut ra,
            &mut || pc.borrow_mut().time = 1.0,
            &mut rb,
            &mut rc,
            mode,
        );
    }
    // § Level plan's table: the clipped engine on unquantised lines, at
    // maximum DECAY, under the pad and a held loop mode. PRE holds the
    // send (≤ 0.5, the half-band's overshoot aside); every loop line ≤ 2.0.
    let mut d = Alone::new(true, true);
    let pad = pink(0x91ab, 10.0, -12.0);
    let f = (500.0 / mode).round() * mode;
    let held = tone(f, 1.0);
    let mut most = [0.0f32; 3];
    for i in 0..blocks(10.0) {
        let send = core::array::from_fn(|j| pad[(i * BLOCK_SIZE + j) % pad.len()]);
        d.block(&send, &pc.borrow());
        most = core::array::from_fn(|k| most[k].max(d.peaks()[k]));
    }
    for i in 0..blocks(10.0) {
        let send = core::array::from_fn(|j| held(i * BLOCK_SIZE + j));
        d.block(&send, &pc.borrow());
        most = core::array::from_fn(|k| most[k].max(d.peaks()[k]));
    }
    println!(
        "peaks: PRE {:.3}, delays {:.3}, APs {:.3}",
        most[0], most[1], most[2]
    );
    assert!(most[0] <= 0.52, "PRE {}", most[0]);
    assert!(most[1] <= 2.0 && most[2] <= 2.0, "{most:?}");
}

#[test]
fn room_knobs_ease() {
    let p = params(1.0);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let ids = [1, 3, 7, 8, 9, 11, 12].map(ParamId);
    check_knobs_ease(&mut bus(&rv, &pc), &ids, &mut |id, v| {
        pc.borrow_mut().write(id, v)
    });
}

#[test]
fn room_noise_and_loudness() {
    let (p, q) = (
        params(0.5),
        ReverbParams {
            mix: 0.5,
            ..ReverbParams::default()
        },
    );
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let (qv, qc) = (fresh(&q), RefCell::new(q));
    check_sustained_loudness(&mut bus(&rv, &pc), &mut bus(&qv, &qc));
    check_noise_floor(&mut bus(&rv, &pc), &mut bus(&qv, &qc), 4.0);
    let mut dense = p;
    dense.diffuse = 1.0;
    let (rv, pc) = (fresh(&dense), RefCell::new(dense));
    let (qv, qc) = (fresh(&q), RefCell::new(q));
    check_noise_floor(&mut bus(&rv, &pc), &mut bus(&qv, &qc), 6.0);
}

/// The return reaches exact zero within 1.5 × its longest RT60, from a
/// noise burst and from a held 1 kHz tone at SIZE 0 (where a ±1 LSB
/// cycle once rang on at 1080 Hz).
#[test]
fn room_falls_silent() {
    for (smear, size, noise) in [(0.0, 0.5, true), (1.0, 0.5, true), (0.0, 0.0, false)] {
        let mut p = params(1.0);
        (p.time, p.smear, p.size) = (1.0, smear, size);
        let seconds = 0.5 + 1.5 * knobs(&p).rt60_s + 1.0;
        let (l, r) = if noise {
            noise_burst(&p, seconds)
        } else {
            let (rv, pc) = (fresh(&p), RefCell::new(p));
            let sine = tone(1_000.0, 0.25);
            run(
                &mut bus(&rv, &pc),
                &mut |i| if i < SR as usize / 2 { sine(i) } else { 0.0 },
                seconds,
            )
        };
        let last = 50 * MS;
        assert!(
            l[l.len() - last..]
                .iter()
                .chain(&r[r.len() - last..])
                .all(|&s| s == 0.0),
            "SMEAR {smear} SIZE {size}: still ringing"
        );
    }
}

/// ROOM's TAIL draws a pre-delay gap, then the fall (spec § Glyph).
#[test]
fn room_tail_draws_its_predelay_gap() {
    use chimera_core::ui::glyph::tail_shape;
    let mut p = params(0.5);
    p.predelay = 0.0;
    let open = tail_shape(ReverbType::Room, &knobs(&p));
    p.predelay = 1.0;
    let gap = tail_shape(ReverbType::Room, &knobs(&p));
    // 1.5 s over 60 columns: 50 ms is the first two.
    assert!(gap[..2].iter().all(|&c| c == 0) && open[0] > 0, "{gap:?}");
    assert!(gap[2] > 0);
}
