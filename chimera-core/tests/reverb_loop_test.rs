//! ROOM (ADR 0095 § ROOM v2) and HALL on the LOOP engine (reverb TYPEs
//! spec § LOOP engine, § PLATE is a zone of HALL, § Level plan, § Moving a
//! length, § Pitch bounds, § Tests).

mod common;

use std::cell::RefCell;
use std::ops::Range;

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::halfband::{Decimator, HALF, Interpolator};
use chimera_core::dsp::rev::arena::ReverbArena;
use chimera_core::dsp::rev::loop_engine::{
    self as le, Hall, Layout, LoopEngine, PER_MS, Sign, TapCount,
};
use chimera_core::dsp::rev::parts::{Wander, wander_seeds};
use chimera_core::dsp::rev::room::{self, Room};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::rev::{Reverb, wet_gain};
use chimera_core::dsp::reverb::{EngineKnobs, KnobMemory, ReverbParams, ReverbType};
use chimera_core::dsp::ring::{FULL_SCALE, RING_LEN};
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{
    MS, Noise, blocks, check_knobs_ease, check_level_plan_thd, check_no_slap, check_noise_floor,
    check_settles_on_pitch, check_sustained_loudness, corr, knobs_120, max_d2, onset_ms, pink, run,
    settled_peak_hz, tone,
};
use common::sweep::power;

type Blk = [f32; BLOCK_SIZE];

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

/// A fresh reverb, its tail gate out: these tests read the engines.
fn fresh(p: &ReverbParams) -> RefCell<Box<Reverb>> {
    let mut rv = Box::new(Reverb::of_type(p.kind));
    rv.tail_gate_off();
    RefCell::new(rv)
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

/// A LOOP-family engine as the shared checks drive it: HALL's
/// `LoopEngine`, or ROOM's own.
trait Eng: Default {
    const SLOTS: usize;
    /// Its smeared allpasses, one wander each.
    const SMEARS: usize;
    fn regions() -> [Range<usize>; 2];
    fn head(&self) -> usize;
    fn smear_to(&mut self, s: &[(f32, f32)]);
    #[allow(clippy::too_many_arguments)]
    fn run_ref(
        &mut self,
        arena: &mut ReverbArena,
        lines: Option<&mut [f32]>,
        clipped: bool,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    );
}

impl<const NL: usize, const NS: usize> Eng for LoopEngine<NL, NS>
where
    LoopEngine<NL, NS>: Layout<NL, NS>,
{
    const SLOTS: usize = <Self as Layout<NL, NS>>::SLOTS;
    const SMEARS: usize = 2;
    fn regions() -> [Range<usize>; 2] {
        <Self as Layout<NL, NS>>::regions()
    }
    fn head(&self) -> usize {
        LoopEngine::head(self)
    }
    fn smear_to(&mut self, s: &[(f32, f32)]) {
        self.set_smear([s[0], s[1]]);
    }
    fn run_ref(
        &mut self,
        arena: &mut ReverbArena,
        lines: Option<&mut [f32]>,
        clipped: bool,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        self.run_reference(arena, lines, clipped, u, k, l, r);
    }
}

impl Eng for Room {
    const SLOTS: usize = room::SLOTS;
    const SMEARS: usize = 4;
    fn regions() -> [Range<usize>; 2] {
        Room::regions()
    }
    fn head(&self) -> usize {
        Room::head(self)
    }
    fn smear_to(&mut self, s: &[(f32, f32)]) {
        self.set_smear([s[0], s[1], s[2], s[3]]);
    }
    fn run_ref(
        &mut self,
        arena: &mut ReverbArena,
        lines: Option<&mut [f32]>,
        clipped: bool,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        self.run_reference(arena, lines, clipped, u, k, l, r);
    }
}

// ── ROOM ──

/// ADR 0095's ROOM tables, ms, written out again: the test's own copy.
const DIFFUSER_MS: [f32; 5] = [0.81, 1.92, 3.59, 4.91, 6.24];
const DELAY_MS: [[f32; 2]; 4] = [[37.1, 57.5], [59.4, 42.3], [52.7, 41.9], [41.5, 43.6]];
const AP_MS: [[f32; 2]; 4] = [[29.0, 13.3], [28.9, 18.4], [30.4, 16.3], [27.1, 14.2]];
/// L, then R; tap j reads stage line j (A1 … D2); R's first is fixed.
const TAP_MS: [[f32; 8]; 2] = [
    [5.5, 5.6, 36.7, 11.8, 24.1, 8.9, 18.0, 38.1],
    [0.51, 31.4, 10.8, 20.4, 15.8, 21.3, 30.9, 9.1],
];

fn n24(ms: f32) -> usize {
    (ms * 24.0).round() as usize
}

/// ROOM alone at the ring's rate: the engine, primed on `p`.
fn engine(p: &ReverbParams) -> (Room, Box<ReverbArena>) {
    let mut e = Room::default();
    e.prime(&knobs_120(p));
    (e, Box::new(ReverbArena::new()))
}

#[test]
fn room_layout_matches_adr_0095() {
    assert_eq!(room::DIFFUSER, DIFFUSER_MS.map(n24));
    assert_eq!(room::AP, AP_MS.map(|l| l.map(n24)));
    assert_eq!(room::DELAY, DELAY_MS.map(|l| l.map(n24)));
    assert_eq!(room::PRE, 1_200, "PRE-DLY's 50 ms");
    for (size, k) in [(0.0, 0.4f32), (1.0, 1.0)] {
        let mut p = params(1.0);
        p.size = size;
        let (e, _) = engine(&p);
        let want = DELAY_MS.map(|l| l.map(|ms| (ms * 24.0 * k).round() as u16));
        assert_eq!(e.delays(), want, "k {k}");
        let mut flat = want.as_flattened().to_vec();
        flat.sort();
        flat.dedup();
        assert_eq!(flat.len(), 8, "k {k}: delays not distinct");
        let (taps, to) = e.taps();
        assert_eq!(taps, to, "landed");
        for side in 0..2 {
            for j in 0..8 {
                let line = want[j / 2][j % 2];
                let t = taps[side][j];
                let tap = if side == 1 && j == 0 {
                    n24(TAP_MS[1][0]) as u16
                } else {
                    (TAP_MS[side][j] * 24.0 * k).round() as u16
                };
                // A tap scales with its line's rounded length: within a
                // sample of × k.
                assert!(
                    t.abs_diff(tap) <= (k < 1.0) as u16,
                    "k {k}: side {side} tap {j}: {t}, want {tap}"
                );
                assert!((2..=line).contains(&t), "k {k}: tap {t} past line {line}");
            }
        }
    }
    // Each line: its length, 2 slots, and a block's guard (`room::GUARD`).
    assert_eq!(room::GUARD, HALF);
    let slots = |ms: f32| n24(ms) + 2 + HALF;
    let i16s = slots(50.0)
        + DELAY_MS
            .as_flattened()
            .iter()
            .map(|&ms| slots(ms))
            .sum::<usize>();
    let f32s = DIFFUSER_MS
        .iter()
        .chain(AP_MS.as_flattened())
        .map(|&ms| slots(ms))
        .sum::<usize>();
    assert_eq!(room::SLOTS, i16s + i16s % 2);
    assert_eq!(room::FSLOTS, f32s);
    let used = room::SLOTS + 2 * room::FSLOTS;
    println!(
        "ROOM: {} i16 + {} f32 = {used} of {RING_LEN}",
        room::SLOTS,
        room::FSLOTS
    );
    assert!(used <= RING_LEN);
}

#[test]
fn reflection_preserves_energy_at_every_loops() {
    let trip = |l: usize| room::DELAY[l].iter().chain(&room::AP[l]).sum::<usize>();
    for deg in [0.0, 30.0, 60.0, 90.0] {
        let (c, s) = room::coupling(deg);
        let (c64, s64) = (c as f64, s as f64);
        // H·H = I: a reflection is its own inverse.
        let hh = [
            [c64 * c64 + s64 * s64, c64 * s64 - s64 * c64],
            [s64 * c64 - c64 * s64, s64 * s64 + c64 * c64],
        ];
        for (i, row) in hh.iter().enumerate() {
            for (j, &x) in row.iter().enumerate() {
                let id = (i == j) as u8 as f64;
                assert!((x - id).abs() <= 1e-7, "{deg}°: H·H[{i}][{j}] = {x}");
            }
        }
        // Each pair's two lossless loops (gains 1, dampers open) of their
        // SIZE-1 trips, re-entering through the engine's own `reflect` in
        // f32: their stored energy holds over 10 round trips.
        for (a, b) in [(0, 1), (2, 3)] {
            let mut n = Noise(7);
            let mut lines: [Vec<f32>; 2] = [a, b].map(|l| (0..trip(l)).map(|_| n.next()).collect());
            let energy = |x: &[Vec<f32>; 2]| {
                x.iter()
                    .flatten()
                    .map(|&v| v as f64 * v as f64)
                    .sum::<f64>()
            };
            let e0 = energy(&lines);
            let mut at = [0usize; 2];
            let (ta, tb) = (trip(a), trip(b));
            for _ in 0..10 * ta.max(tb) {
                (lines[0][at[0]], lines[1][at[1]]) =
                    room::reflect(c, s, lines[0][at[0]], lines[1][at[1]]);
                at = [(at[0] + 1) % ta, (at[1] + 1) % tb];
            }
            let drift = energy(&lines) / e0 - 1.0;
            assert!(
                drift.abs() <= 1e-6,
                "{deg}° ({a}, {b}): energy drifts {drift}"
            );
        }
    }
}

/// Each block, which loops' stage lines hold anything: an impulse into
/// loop A alone (diffuser bypassed, stage allpasses at g 0, so it travels
/// as a pulse) at LOOPS `loops`, 0.6 s.
fn loops_holding(loops: f32) -> Vec<[bool; 4]> {
    let mut p = params(1.0);
    (p.shape, p.predelay, p.damping, p.time, p.size) = (loops, 0.0, 0.0, 1.0, 1.0);
    let (mut e, mut arena) = engine(&p);
    e.probe_loop(0);
    let k = knobs_120(&p);
    let (mut l, mut r) = ([0.0f32; HALF], [0.0; HALF]);
    (0..(0.6 * 24_000.0) as usize / HALF)
        .map(|b| {
            let u: [f32; HALF] = core::array::from_fn(|i| (b == 0 && i == 0) as u8 as f32);
            e.run(&mut arena, &u, &k, &mut l, &mut r);
            let (h, lines) = (e.head(), arena.lines());
            core::array::from_fn(|lp| {
                (0..2).any(|s| Room::line(lp, s).any(|i| lines[(h + i) % room::SLOTS] != 0))
            })
        })
        .collect()
}

/// LOOPS 0 (θ 0) keeps four loops: what enters A never reaches B. LOOPS 1
/// (θ 90°) makes (A, B) one loop of four stages: A's output re-enters B
/// alone, so A's lines are silent while the pulse crosses B, every second
/// trip. C and D hear nothing either way.
#[test]
fn loops_0_keeps_four_loops_and_90_makes_two() {
    let four = loops_holding(0.0);
    assert!(
        four.iter().all(|h| !h[1] && !h[2] && !h[3]),
        "θ 0: B, C or D heard A"
    );
    // A's lines hold the pulse but while it crosses A's own allpasses.
    assert!(
        four[four.len() / 2..].iter().any(|h| h[0]),
        "θ 0: A rings on"
    );
    let two = loops_holding(1.0);
    assert!(two.iter().all(|h| !h[2] && !h[3]), "θ 90°: C or D heard A");
    let only = |a: bool, b: bool| two.iter().filter(|h| h[0] == a && h[1] == b).count();
    let (a_only, b_only, both) = (only(true, false), only(false, true), only(true, true));
    println!("θ 90°: A alone {a_only} blocks, B alone {b_only}, both {both}");
    assert!(b_only >= 100, "θ 90°: B never holds the pulse alone");
    assert!(
        a_only >= 100 && both <= a_only / 10,
        "θ 90°: A not silent through B's trips"
    );
    // The pulse comes back to A after B's trip: A holds it again late.
    let first_b = two.iter().position(|h| h[1]).unwrap();
    let back = two[first_b..].iter().position(|h| h[0] && !h[1]);
    assert!(back.is_some(), "θ 90°: never back to A");
}

/// SIZE 0 → 1: each tap's read moves only in its own line's crossfade,
/// by at most 120 samples; R's first tap stays at 12 samples (0.51 ms).
#[test]
fn taps_move_with_their_lines() {
    let mut p = params(1.0);
    (p.size, p.predelay) = (0.0, 0.0);
    let (mut e, mut arena) = engine(&p);
    p.size = 1.0;
    let k = knobs_120(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    let mut moved = [[false; 8]; 2];
    for _ in 0..2_000 {
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let (from, to) = e.taps();
        let lines = e.crossfade().map(|(a, b, _)| (a, b));
        for side in 0..2 {
            for j in 0..8 {
                let (f, t) = (from[side][j], to[side][j]);
                let line_moves = lines.is_some_and(|(a, b)| a[j / 2][j % 2] != b[j / 2][j % 2]);
                if side == 1 && j == 0 {
                    assert_eq!((f, t), (12, 12), "R's first tap moved");
                } else if line_moves {
                    assert!(f != t && f.abs_diff(t) <= 120, "{side} {j}: {f} → {t}");
                    moved[side][j] = true;
                } else {
                    assert_eq!(f, t, "{side} {j}: tap moves without its line");
                }
            }
        }
        if !e.moving() {
            let want: [[u16; 8]; 2] = core::array::from_fn(|s| {
                core::array::from_fn(|j| {
                    if s == 1 && j == 0 {
                        12
                    } else {
                        n24(TAP_MS[s][j]) as u16
                    }
                })
            });
            assert_eq!(from, want, "landed taps");
            assert_eq!(moved.as_flattened().iter().filter(|&&m| m).count(), 15);
            return;
        }
    }
    panic!("never landed");
}

/// The return's first sample within −20 dB of its peak, ms after a click.
fn first_arrival_ms(p: &ReverbParams) -> f32 {
    let (rv, pc) = (fresh(p), RefCell::new(*p));
    let lead = blocks(0.5) * BLOCK_SIZE;
    let (l, r) = run(&mut bus(&rv, &pc), &mut |i| (i == lead) as u8 as f32, 0.8);
    let m: Vec<f32> = l[lead..]
        .iter()
        .zip(&r[lead..])
        .map(|(a, b)| 0.5 * (a + b))
        .collect();
    let peak = m.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    m.iter().position(|x| x.abs() >= 0.1 * peak).unwrap() as f32 * 1000.0 / SR as f32
}

/// The bus's half-bands, 17 samples each way at 48 kHz: ahead of every
/// TYPE alike.
const HALF_BANDS_MS: f32 = 34.0 / 48.0;

/// PRE-DLY 0: ROOM's own first arrival, R's fixed 0.51 ms tap past the
/// diffuser, is under 1 ms at every SIZE (Task 9's ROOM: 2.9 ms on the
/// bus).
#[test]
fn room_first_arrival_is_under_a_millisecond() {
    let mut p = params(1.0);
    p.predelay = 0.0;
    for size in [0.0, 0.5, 1.0] {
        p.size = size;
        let t = first_arrival_ms(&p) - HALF_BANDS_MS;
        println!("SIZE {size}: first arrival {t:.2} ms past the half-bands");
        assert!(t <= 1.0, "SIZE {size}: first arrival {t} ms");
    }
}

/// A 2-pole bandpass at `fc`, an octave wide (RBJ, constant 0 dB peak).
fn bandpass(x: &[f32], fc: f32) -> Vec<f32> {
    let w = std::f64::consts::TAU * fc as f64 / SR as f64;
    let q = 2f64.sqrt();
    let alpha = w.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let (b0, b2, a1, a2) = (
        alpha / a0,
        -alpha / a0,
        -2.0 * w.cos() / a0,
        (1.0 - alpha) / a0,
    );
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    x.iter()
        .map(|&v| {
            let v = v as f64;
            let y = b0 * v + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, v, y1, y);
            y as f32
        })
        .collect()
}

/// RT60 of the `fc` octave by interrupted noise (ISO 3382-2): 1 s of
/// noise, then the decay from the cut, four seeds' band energy averaged,
/// T20 of its Schroeder curve.
fn band_rt60(p: &ReverbParams, fc: f32) -> f32 {
    let tail = 0.3 + 1.5 * knobs_120(p).rt60_s;
    let cut = SR as usize;
    let mut e: Vec<f64> = Vec::new();
    for seed in 0..4 {
        let (rv, pc) = (fresh(p), RefCell::new(*p));
        let mut n = Noise(0x1234 + seed);
        let (l, r) = run(
            &mut bus(&rv, &pc),
            &mut |i| if i < cut { 0.25 * n.next() } else { 0.0 },
            1.0 + tail,
        );
        // Twice through the band: steeper skirts.
        let (bl, br) = (
            bandpass(&bandpass(&l, fc), fc),
            bandpass(&bandpass(&r, fc), fc),
        );
        let sq: Vec<f64> = bl[cut..]
            .iter()
            .zip(&br[cut..])
            .map(|(&a, &b)| a as f64 * a as f64 + b as f64 * b as f64)
            .collect();
        if e.is_empty() {
            e = sq;
        } else {
            e.iter_mut().zip(sq).for_each(|(a, b)| *a += b);
        }
    }
    for i in (0..e.len() - 1).rev() {
        e[i] += e[i + 1];
    }
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &v) in e.iter().enumerate() {
        let db = 10.0 * (v / e[0]).log10();
        if (-25.0..=-5.0).contains(&db) {
            let x = i as f64 / SR as f64;
            (n, sx, sy, sxx, sxy) = (n + 1.0, sx + x, sy + db, sxx + x * x, sxy + x * db);
        }
    }
    (-60.0 * (n * sxx - sx * sx) / (n * sxy - sx * sy)) as f32
}

/// DECAY 0, ½ and 1 ring their 1 kHz octave 0.2, 0.63 and 2.0 s, ±10 %,
/// with DAMP open; no stage's gain passes 0.985 at any SIZE.
#[test]
fn room_rt60_tracks_decay_to_2_s() {
    let mut p = params(1.0);
    (p.damping, p.smear, p.size) = (0.0, 0.0, 0.5);
    for (decay, want) in [(0.0, 0.2), (0.5, 0.632), (1.0, 2.0)] {
        p.time = decay;
        let law = knobs_120(&p).rt60_s;
        assert!(
            (law / want - 1.0).abs() < 2e-3,
            "DECAY {decay}: the law {law}"
        );
        let rt = band_rt60(&p, 1_000.0);
        println!("DECAY {decay}: 1 kHz RT60 {rt:.3} s, target {want}");
        assert!(
            (rt / want - 1.0).abs() <= 0.1,
            "DECAY {decay}: {rt} s against {want}"
        );
    }
    // SIZE's ends: the stage allpasses' share of the trip (Φ's k term)
    // changes most there. (The gains' 0.985 cap is shared machinery and
    // never binds here: the longest stage at 2.0 s is under 0.75.)
    for (size, decay, want) in [
        (0.0, 0.5, 0.632),
        (0.0, 1.0, 2.0),
        (1.0, 0.5, 0.632),
        (1.0, 1.0, 2.0),
    ] {
        (p.size, p.time) = (size, decay);
        let rt = band_rt60(&p, 1_000.0);
        println!("SIZE {size} DECAY {decay}: 1 kHz RT60 {rt:.3} s, target {want}");
        assert!(
            (rt / want - 1.0).abs() <= 0.1,
            "SIZE {size} DECAY {decay}: {rt} s against {want}"
        );
    }
}

/// The tail's spectral centroid, 60 Hz–11 kHz.
fn centroid_hz(x: &[f32]) -> f64 {
    let n = 4_096;
    let p = power(x, n);
    let hz = |k: usize| k as f64 * SR as f64 / n as f64;
    let (num, den) = (1..p.len())
        .filter(|&k| (60.0..=11_000.0).contains(&hz(k)))
        .fold((0.0, 0.0), |(a, b), k| (a + hz(k) * p[k], b + p[k]));
    num / den
}

/// DAMP is in the loop: 1.5 kHz against 10 kHz shortens the 4 kHz octave
/// by 30 % or more and leaves 500 Hz within 5 %. TONE is at the input: 10
/// → 2.5 kHz pulls the tail's centroid down, its 1 kHz RT60 within 5 %.
#[test]
fn damp_darkens_the_loop_and_tone_the_input() {
    let mut p = params(1.0);
    (p.size, p.time, p.smear, p.tone, p.damping) = (1.0, 0.5, 0.0, 0.0, 0.0);
    let open = p;
    let dark = ReverbParams { damping: 1.0, ..p };
    // DAMP's top opens the dampers outright; its next step is a one-pole.
    let da = |damping: f32| engine(&ReverbParams { damping, ..p }).0.targets().1;
    assert_eq!(da(0.0), 1.0, "DAMP 0: the dampers open");
    assert!(da(0.01) < 1.0, "DAMP 0.01: {}", da(0.01));
    let (o4, d4) = (band_rt60(&open, 4_000.0), band_rt60(&dark, 4_000.0));
    let (o5, d5) = (band_rt60(&open, 500.0), band_rt60(&dark, 500.0));
    println!("DAMP 10 → 1.5 kHz: 4 kHz {o4:.3} → {d4:.3} s, 500 Hz {o5:.3} → {d5:.3} s");
    assert!(d4 <= 0.7 * o4, "4 kHz: {o4} → {d4}");
    assert!((d5 / o5 - 1.0).abs() <= 0.05, "500 Hz: {o5} → {d5}");
    // TONE's top opens the input filter, over the knob's last 5 % (no
    // cliff): open at 0, a one-pole from 0.05 on, rising monotonically.
    let ta = |tone: f32| engine(&ReverbParams { tone, ..p }).0.targets().0;
    assert_eq!(ta(0.0), 1.0, "TONE 0: the input open");
    let steps: Vec<f32> = [0.0, 0.01, 0.02, 0.03, 0.04, 0.05, 0.06].map(ta).to_vec();
    assert!(steps.windows(2).all(|w| w[1] < w[0]), "{steps:?}");
    assert!(
        steps.windows(2).all(|w| w[0] - w[1] < 0.03),
        "a cliff: {steps:?}"
    );
    let pole = 1.0 - (-core::f32::consts::TAU * 9_000.0 / 24_000.0).exp();
    assert!(
        ta(0.08) < pole + 0.01,
        "TONE 0.08: a one-pole, {}",
        ta(0.08)
    );
    let toned = ReverbParams { tone: 1.0, ..p };
    let tail = |q: &ReverbParams| {
        let (l, _) = noise_burst(q, 1.5);
        centroid_hz(&l[SR as usize / 2 + 20 * MS..])
    };
    let (bright, darker) = (tail(&open), tail(&toned));
    let (o1, t1) = (band_rt60(&open, 1_000.0), band_rt60(&toned, 1_000.0));
    println!("TONE 10 → 2.5 kHz: centroid {bright:.0} → {darker:.0} Hz, 1 kHz {o1:.3} → {t1:.3} s");
    assert!(darker < 0.8 * bright, "centroid {bright} → {darker}");
    assert!((t1 / o1 - 1.0).abs() <= 0.05, "1 kHz: {o1} → {t1}");
}

/// The cached targets follow every move: DECAY, TONE, DAMP, LOOPS, then
/// SIZE turned mid-run; once the lengths land they equal a fresh engine's
/// primed at the final knobs.
#[test]
fn room_targets_follow_decay_tone_and_size() {
    let mut p = params(1.0);
    let (mut e, mut arena) = engine(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    for b in 0..1_200 {
        match b {
            10 => p.time = 1.0,
            20 => p.tone = 0.9,
            25 => p.damping = 0.8,
            28 => p.shape = 0.7,
            30 => p.size = 1.0,
            _ => {}
        }
        e.run(&mut arena, &u, &knobs_120(&p), &mut l, &mut r);
    }
    assert!(!e.moving(), "the move lands");
    let (fresh, _) = engine(&p);
    assert_eq!(e.delays(), fresh.delays());
    assert_eq!(e.targets(), fresh.targets(), "targets stale");
}

/// SIZE and PRE-DLY 0 → 1: each crossfade moves one stage delay of each
/// loop by at most 120 samples, every loop still moving in the same one.
#[test]
fn round_trip_moves_at_most_5_ms_per_crossfade() {
    let mut p = params(1.0);
    (p.size, p.predelay) = (0.0, 0.0);
    let (mut e, mut arena) = engine(&p);
    let trip = |d: [[u16; 2]; 4], l: usize| d[l].iter().map(|&x| x as i32).sum::<i32>();
    let k0 = e.delays();
    (p.size, p.predelay) = (1.0, 1.0);
    let k = knobs_120(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    // Each delay steps ⌈its move / 120⌉ times, one a loop a crossfade.
    let need = (0..4)
        .map(|l| {
            (0..2)
                .map(|s| (room::DELAY[l][s] - k0[l][s] as usize).div_ceil(120))
                .sum::<usize>()
        })
        .max()
        .unwrap();
    let mut crossfades = 0;
    let mut last = None;
    for b in 0..2_000 {
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        if let Some((from, to, pre)) = e.crossfade() {
            if last != Some((from, to, pre)) {
                crossfades += 1;
                assert!(pre.0.abs_diff(pre.1) <= 120, "block {b}: PRE {pre:?}");
                for l in 0..4 {
                    let d = (trip(to, l) - trip(from, l)).abs();
                    assert!(d <= 120, "block {b}: loop {l} moves {d}");
                    let moved = (0..2).filter(|&s| from[l][s] != to[l][s]).count();
                    let left = (0..2).any(|s| to[l][s] as usize != room::DELAY[l][s]);
                    assert!(moved <= 1, "block {b}: loop {l} moves {moved} delays");
                    // Every loop moves while it has a way to go.
                    assert!(moved == 1 || !left, "block {b}: loop {l} waits");
                }
            }
            last = Some((from, to, pre));
        } else if e.delays() == room::DELAY.map(|l| l.map(|x| x as u16))
            && e.predelay() == room::PRE as u16
        {
            println!("{crossfades} crossfades (need {need}); landed at block {b}");
            assert_eq!(crossfades, need);
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
/// most 1.45 % of that loop's 2-stage round trip at every SIZE (θ 0's,
/// the shortest), however deep the offsets asked.
#[test]
fn room_smear_reads_stay_within_the_cap() {
    let mut p = params(1.0);
    (p.smear, p.rate) = (1.0, 1.0);
    for i in 0..=16 {
        p.size = i as f32 / 16.0;
        let k = knobs_120(&p);
        assert!(k.smear_ms <= 0.0072 * k.length_ms + 1e-6, "SIZE {}", p.size);
        let (mut e, mut arena) = engine(&p);
        let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
        e.set_smear([(1e9, -1e9), (-5.0, 1e9), (1e9, 1e9), (0.0, 1e9)]);
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let held = e.smear();
        for (lp, &st) in room::SMEARED.iter().enumerate() {
            let trip: usize = e.delays()[lp].iter().map(|&d| d as usize).sum::<usize>()
                + room::AP[lp].iter().sum::<usize>();
            for x in [held[lp].0, held[lp].1] {
                assert!(
                    (0.0..=2.0 * 0.0072 * trip as f32 + 1e-3).contains(&x),
                    "{x} of {trip}"
                );
                assert!(room::AP[lp][st] as f32 - x >= 2.0);
            }
            let e_share = held[lp].0.max(held[lp].1) / trip as f32;
            assert!(e_share <= 0.0145, "SIZE {}: loop {lp} E {e_share}", p.size);
        }
    }
    // Through a SIZE move the cap follows the shorter of each moving
    // delay's two lengths.
    p.size = 1.0;
    let (mut e, mut arena) = engine(&p);
    p.size = 0.0;
    let k = knobs_120(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    let mut seen = 0;
    for _ in 0..400 {
        e.set_smear([(1e9, 1e9); 4]);
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let Some((from, to, _)) = e.crossfade() else {
            continue;
        };
        seen += 1;
        let held = e.smear();
        for lp in 0..4 {
            let trip: usize = (0..2)
                .map(|s| from[lp][s].min(to[lp][s]) as usize)
                .sum::<usize>()
                + room::AP[lp].iter().sum::<usize>();
            let top = 2.0 * 0.0072 * trip as f32;
            assert!(
                held[lp].0 <= top + 1e-3 && held[lp].1 <= top + 1e-3,
                "loop {lp}: {:?} over {top}",
                held[lp]
            );
        }
    }
    assert!(seen > 100, "{seen} crossfading blocks");
}

/// A LOOP-family engine alone at `p`, its wanders run as `Reverb` runs
/// them, and its decay held long: RT60 20 s holds every stage at its 0.985
/// cap, so a tail lasts the 4 s windows a wander's drift needs.
fn smeared<E: Eng>(p: ReverbParams) -> impl FnMut(&Blk) -> (Blk, Blk) {
    let mut k = knobs_120(&p);
    k.rt60_s = 20.0;
    let full = k.smear_ms * PER_MS;
    let seeds = wander_seeds::<4>(0x5EED_0343);
    let mut w: Vec<Wander> = (0..E::SMEARS)
        .map(|i| {
            let mut w = Wander::new(seeds[i], i as f32 / E::SMEARS as f32);
            w.set(Wander::rate(k.rate_hz, 24_000.0));
            w
        })
        .collect();
    let (mut e, mut arena) = (E::default(), Box::new(ReverbArena::new()));
    let (mut dec, mut up) = (Decimator::new(), [Interpolator::new(), Interpolator::new()]);
    move |send| {
        let (mut u, mut l, mut r) = ([0.0; HALF], [0.0; HALF], [0.0; HALF]);
        dec.process(send, &mut u);
        let s: Vec<(f32, f32)> = w
            .iter_mut()
            .map(|w| {
                let x0 = w.value();
                (full * (1.0 + x0), full * (1.0 + w.block()))
            })
            .collect();
        e.smear_to(&s);
        e.run_ref(&mut arena, None, true, &u, &k, &mut l, &mut r);
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
            &mut smeared::<Room>(p),
            &mut |i| if i < SR as usize / 2 { sine(i) } else { 0.0 },
            9.5,
        );
        let at = |a: f32, b: f32| {
            settled_peak_hz(&l[(a * SR as f32) as usize..(b * SR as f32) as usize])
        };
        // Two 2 s windows while the tail stands well over the LSB: the
        // stage delays truncate under 3 LSB, so SIZE 0's tail is gone by 8 s.
        let (f1, f2) = (at(1.0, 3.0), at(3.0, 5.0));
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
    // DIFFUSE 0.4 (g 0.55): at the starting 0.45 the 50 ms control's
    // slap passes the check, so it has no teeth there. SMEAR 0.3, where
    // the control was tuned: at 0 its 50 ms slap passes too.
    (p.time, p.size, p.diffuse, p.smear) = (1.0, from, 0.4, 0.3);
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

/// Where a tail is judged from: once the longest tap (L's 38.1 ms, on
/// D2) has heard the send's end and the diffuser has rung down.
const LATE: usize = 100 * MS;

/// The 0 → 1 sweep from 1.4 s lands before the noise ends: the judged
/// tail is the settled one. And the check has teeth: 50 ms steps
/// (test-only), crossfading inside the judged tail from 2.1 s, slap.
#[test]
fn no_slap_after_a_size_sweep() {
    let still = size_tail(1.0, 1.4, 0);
    check_no_slap(&size_tail(0.0, 1.4, 0), &still);
    let coarse = size_tail(0.0, 2.1, 10 * room::STEP);
    let heard = std::panic::catch_unwind(|| check_no_slap(&coarse, &still));
    assert!(heard.is_err(), "50 ms steps pass the slap check");
}

/// A 0.5 s noise burst into the TYPE on `p`, MIX 1, `seconds` long: (L, R).
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

/// A LOOP-family engine alone at the ring's rate, behind the bus's
/// half-bands: the i16 engine, or its unclipped or f32-line references.
struct Alone<E: Eng> {
    e: E,
    arena: Box<ReverbArena>,
    f32_lines: Option<Vec<f32>>,
    clipped: bool,
    dec: Decimator,
    up: [Interpolator; 2],
}

impl<E: Eng> Alone<E> {
    fn new(f32_lines: bool, clipped: bool) -> Self {
        Self {
            e: E::default(),
            arena: Box::new(ReverbArena::new()),
            f32_lines: f32_lines.then(|| vec![0.0; E::SLOTS]),
            clipped,
            dec: Decimator::new(),
            up: [Interpolator::new(), Interpolator::new()],
        }
    }

    fn block(&mut self, send: &Blk, p: &ReverbParams) -> (Blk, Blk) {
        let (mut u, mut l, mut r) = ([0.0; HALF], [0.0; HALF], [0.0; HALF]);
        self.dec.process(send, &mut u);
        let k = knobs_120(p);
        self.e.run_ref(
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

    /// The largest |value| each stored i16 line region holds now, in line
    /// units (1.0 full scale): PRE, the stage delays.
    fn peaks(&self) -> [f32; 2] {
        let lines = self.f32_lines.as_deref().expect("f32 lines");
        // The lines rotate with the shared head: a region's slot i sits
        // at (head + i) mod slots.
        let (h, n) = (self.e.head(), lines.len());
        E::regions().map(|r| r.map(|i| lines[(h + i) % n].abs()).fold(0.0f32, f32::max) / 16_384.0)
    }
}

/// § Level plan on a LOOP-family TYPE at `p` (its DECAY set to 1 for the
/// hot stimuli), then § Level plan's table: the clipped engine on
/// unquantised lines, at maximum DECAY, under the pad and a held loop
/// mode. PRE holds the send (≤ ½ the send's peak, the half-band's
/// overshoot aside); every i16 loop line ≤ 2.0 (the stage allpasses store
/// f32: no bound).
fn check_loop_level_plan<E: Eng>(p: ReverbParams, max_thd: f32, max_alias_db: f32) {
    let pc = RefCell::new(p);
    let mut a = Alone::<E>::new(false, true);
    let mut b = Alone::<E>::new(false, false);
    let mut c = Alone::<E>::new(true, false);
    let mode = 1_000.0 / knobs_120(&p).length_ms;
    {
        let mut ra = |s: &Blk| a.block(s, &pc.borrow());
        let mut rb = |s: &Blk| b.block(s, &pc.borrow());
        let mut rc = |s: &Blk| c.block(s, &pc.borrow());
        check_level_plan_thd(
            &mut ra,
            &mut || pc.borrow_mut().time = 1.0,
            &mut rb,
            &mut rc,
            mode,
            max_thd,
            max_alias_db,
        );
    }
    let mut d = Alone::<E>::new(true, true);
    let pad = pink(0x91ab, 10.0, -12.0);
    let f = (500.0 / mode).round() * mode;
    let held = tone(f, 1.0);
    let mut most = [0.0f32; 2];
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
    // PRE holds the send at ½: the pad's peaks pass 0 dBFS a little, and
    // the half-band overshoots.
    let pad_peak = pad.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    println!(
        "peaks: PRE {:.3} (pad {pad_peak:.3}), delays {:.3}",
        most[0], most[1]
    );
    assert!(most[0] <= 0.55 * pad_peak.max(1.0), "PRE {}", most[0]);
    assert!(most[1] <= 2.0, "{most:?}");
}

#[test]
fn level_plan_holds_for_room() {
    for loops in [0.0, 1.0] {
        let mut p = params(1.0);
        (p.size, p.diffuse, p.shape) = (0.0, 1.0, loops);
        check_loop_level_plan::<Room>(p, 0.03, -40.0);
    }
}

#[test]
fn room_knobs_ease() {
    let p = params(1.0);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let ids = [1, 2, 3, 7, 8, 9, 10, 11, 12].map(ParamId);
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
    // The storage noise floor: GRAIN's is meant, and bounded by its own
    // tests.
    let quiet = |p: &ReverbParams| {
        let rv = fresh(p);
        rv.borrow_mut().room_mut().expect("ROOM").grain_off();
        rv
    };
    let (rv, pc) = (quiet(&p), RefCell::new(p));
    let (qv, qc) = (fresh(&q), RefCell::new(q));
    check_noise_floor(&mut bus(&rv, &pc), &mut bus(&qv, &qc), 4.0);
    let mut dense = p;
    dense.diffuse = 1.0;
    let (rv, pc) = (quiet(&dense), RefCell::new(dense));
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
        let seconds = 0.5 + 1.5 * knobs_120(&p).rt60_s + 1.0;
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

// ── GRAIN (ADR 0095, ROOM as built: GRAIN) ──

/// ROOM at MIX 1 on MidiVerb II program 5's fitted knobs (SIZE, DECAY,
/// TONE, DIFFUSE, PRE-DLY, DAMP, LOOPS), SMEAR 0.
fn mv_room(k: [f32; 7]) -> ReverbParams {
    let mut p = params(1.0);
    (
        p.size, p.time, p.tone, p.diffuse, p.predelay, p.damping, p.shape,
    ) = (k[0], k[1], k[2], k[3], k[4], k[5], k[6]);
    p.smear = 0.0;
    p
}
const MV01: [f32; 7] = [0.42, 0.189, 0.036, 0.0, 0.0, 0.923, 0.0];
const MV05: [f32; 7] = [0.644, 0.718, 0.0, 0.0, 0.085, 0.426, 0.634];
const MV08: [f32; 7] = [0.615, 0.708, 0.91, 0.0, 0.085, 0.402, 0.796];

/// ROOM on `p` fed `input`, with GRAIN and without: (with, without), each
/// (l, r).
#[allow(clippy::type_complexity)]
fn with_and_without_grain(
    p: &ReverbParams,
    input: &dyn Fn(usize) -> f32,
    seconds: f32,
) -> ((Vec<f32>, Vec<f32>), (Vec<f32>, Vec<f32>)) {
    let pc = RefCell::new(*p);
    let on = fresh(p);
    let off = fresh(p);
    off.borrow_mut().room_mut().expect("ROOM").grain_off();
    let a = run(&mut bus(&on, &pc), &mut |i| input(i), seconds);
    let b = run(&mut bus(&off, &pc), &mut |i| input(i), seconds);
    (a, b)
}

/// The harness's noise burst as Chimera hears it: white at −18 dBFS RMS
/// for `burst` samples, then silence.
fn harness_burst(burst: usize) -> impl Fn(usize) -> f32 {
    // Uniform ±a has RMS a/√3.
    let a = 10f32.powf(-18.0 / 20.0) * 3f32.sqrt();
    let x: Vec<f32> = {
        let mut n = Noise(0x9e37_79b9);
        (0..burst).map(|_| a * n.next()).collect()
    };
    move |i| x.get(i).copied().unwrap_or(0.0)
}

fn mean_power(x: &[f32]) -> f64 {
    x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64
}

/// The grain alone, per side: the difference GRAIN makes. Nothing feeds it
/// back, so it is exactly the noise × the gate.
fn grain_of(a: &(Vec<f32>, Vec<f32>), b: &(Vec<f32>, Vec<f32>)) -> (Vec<f32>, Vec<f32>) {
    let d = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p - q).collect();
    (d(&a.0, &b.0), d(&a.1, &b.1))
}

/// Under a held send the gate stands open: each side carries the grain at
/// its level (`room::GRAIN`, for DECAY's RT60), within 1 dB. A short room
/// carries less.
#[test]
fn grain_carries_its_level() {
    for (k, short) in [(MV05, false), (MV01, true)] {
        let p = mv_room(k);
        let (on, off) = with_and_without_grain(&p, &harness_burst(SR as usize * 2), 2.0);
        let g = grain_of(&on, &off);
        let share = room::GRAIN.decay().share(knobs_120(&p).rt60_s);
        assert_eq!(share < 0.9, short, "DECAY's share {share}");
        let want = room::GRAIN.level().get() * share;
        for side in [&g.0, &g.1] {
            let lsb = mean_power(&side[SR as usize..]).sqrt() * 4096.0;
            assert!(
                (20.0 * (lsb / f64::from(want)).log10()).abs() < 1.0,
                "{lsb:.2} LSB13, want {want:.2}"
            );
        }
    }
}

/// After the harness's burst the grain-to-tail ratio follows the unit's
/// (the sizzle report's r/F, all bands): a fixed grain under a falling
/// tail, so the ratio climbs, a small room's furthest. The bands sit under
/// the report's: GRAIN's level is fitted on the notched burst at the A/B's
/// drive (ADR 0095), which drives the unit 3–4 dB harder than the plain
/// burst the r/F was read at, and the gate fades the grain sooner than
/// the unit's dead band takes its tail. Each window lies `GRAIN_UNDER_DB`
/// under to `GRAIN_OVER_DB` over the unit's, and climbs from the first.
#[test]
fn grain_to_tail_follows_the_unit() {
    let burst = SR as usize;
    /// A window after the burst, ms, and the unit's r/F there, dB.
    #[derive(Clone, Copy)]
    struct Window {
        from: f32,
        to: f32,
        unit_db: f64,
    }
    const fn w(from: f32, to: f32, unit_db: f64) -> Window {
        Window { from, to, unit_db }
    }
    struct Case {
        name: &'static str,
        knobs: [f32; 7],
        windows: [Window; 3],
    }
    const CASES: [Case; 3] = [
        Case {
            name: "p01",
            knobs: MV01,
            windows: [
                w(0.0, 50.0, -41.5),
                w(100.0, 200.0, -21.4),
                w(200.0, 400.0, -9.0),
            ],
        },
        Case {
            name: "p05",
            knobs: MV05,
            windows: [
                w(0.0, 50.0, -41.4),
                w(100.0, 200.0, -35.1),
                w(200.0, 400.0, -26.4),
            ],
        },
        Case {
            name: "p08",
            knobs: MV08,
            windows: [
                w(0.0, 50.0, -36.4),
                w(100.0, 200.0, -31.1),
                w(200.0, 400.0, -23.8),
            ],
        },
    ];
    for Case {
        name,
        knobs: k,
        windows,
    } in CASES
    {
        let (on, off) = with_and_without_grain(&mv_room(k), &harness_burst(burst), 2.0);
        let g = grain_of(&on, &off);
        let got = windows.map(
            |Window {
                 from: a,
                 to: b,
                 unit_db: want,
             }| {
                let w = burst + (a * MS as f32) as usize..burst + (b * MS as f32) as usize;
                let grain = mean_power(&g.0[w.clone()]) + mean_power(&g.1[w.clone()]);
                let tail = mean_power(&off.0[w.clone()]) + mean_power(&off.1[w.clone()]);
                let got = 10.0 * (grain / tail).log10();
                assert!(
                    got >= want - GRAIN_UNDER_DB && got <= want + GRAIN_OVER_DB,
                    "{name} {a}-{b} ms: grain/tail {got:.1} dB, the unit's {want:.1}"
                );
                got
            },
        );
        assert!(
            got[0] < got[1] && got[1] < got[2],
            "{name}: {got:?} doesn't climb"
        );
    }
}
/// How far under and over the unit's r/F the grain-to-tail ratio may sit.
const GRAIN_UNDER_DB: f64 = 9.0;
const GRAIN_OVER_DB: f64 = 2.0;

/// A fresh ROOM fed nothing returns exactly nothing: no hiss at idle.
#[test]
fn room_is_silent_at_idle() {
    let p = mv_room(MV05);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let (l, r) = run(&mut bus(&rv, &pc), &mut |_| 0.0, 3.0);
    assert!(l.iter().chain(&r).all(|&s| s == 0.0), "hiss at idle");
}

/// Once the tail is gone the grain is too, exactly: where ROOM without
/// GRAIN reads zero, ROOM with it reads zero, and the envelope sits under
/// the gate's floor (so the gate, not underflow, shuts it).
#[test]
fn no_grain_without_a_tail() {
    let p = mv_room(MV05);
    let (on, off) = with_and_without_grain(&p, &harness_burst(SR as usize / 2), 4.0);
    let last = SR as usize / 2;
    let quiet = |x: &[f32]| x[x.len() - last..].iter().all(|&s| s == 0.0);
    assert!(quiet(&off.0) && quiet(&off.1), "the tail outlasts the test");
    assert!(quiet(&on.0) && quiet(&on.1), "grain after the tail");
    // Block by block: once the tail has been exact zeros for two blocks
    // (so the envelope only falls) and the envelope is under the floor
    // but not yet nothing, the grain is exactly none: the floor, not
    // underflow, shuts the gate.
    let (on, off) = (fresh(&p), fresh(&p));
    off.borrow_mut().room_mut().expect("ROOM").grain_off();
    let x = harness_burst(SR as usize / 2);
    let floor = room::GRAIN.gate().floor().get();
    let (mut under, mut silent) = (0, 0);
    for b in 0..blocks(4.0) {
        let send: Blk = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        let env = on.borrow().room().expect("ROOM").grain_env();
        let (a, z) = (
            tick(&mut on.borrow_mut(), &send, &p),
            tick(&mut off.borrow_mut(), &send, &p),
        );
        if silent >= 2 && env < floor && env > 0.01 * floor {
            under += 1;
            assert!(a == z, "grain with the envelope {env} under the floor");
        }
        let zero = z.0.iter().chain(&z.1).all(|&v| v == 0.0);
        silent = if zero { silent + 1 } else { 0 };
    }
    assert!(under > 10, "{under} blocks under the floor");
}

/// GRAIN's level eases when DECAY moves, as every knob does: under a held
/// send (the gate open), DECAY 1 → 0 takes the level to half (the DECAY
/// law), over the 20 ms ease, not in a block. The same draws with DECAY
/// held are the reference: their ratio is the level's.
#[test]
fn grain_level_eases_with_decay() {
    let mut p = mv_room(MV05);
    p.time = 1.0;
    let x = harness_burst(SR as usize * 2);
    let (on, off) = (fresh(&p), fresh(&p));
    let (on_h, off_h) = (fresh(&p), fresh(&p));
    for rv in [&off, &off_h] {
        rv.borrow_mut().room_mut().expect("ROOM").grain_off();
    }
    let at = blocks(1.0);
    let mut ratio = Vec::new();
    for b in 0..blocks(1.5) {
        let send: Blk = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        let q = ReverbParams {
            time: if b >= at { 0.0 } else { 1.0 },
            ..p
        };
        let g = |a: (Blk, Blk), z: (Blk, Blk)| -> f64 {
            a.0.iter()
                .zip(&z.0)
                .map(|(u, v)| f64::from(u - v).powi(2))
                .sum()
        };
        let moved = g(
            tick(&mut on.borrow_mut(), &send, &q),
            tick(&mut off.borrow_mut(), &send, &q),
        );
        let held = g(
            tick(&mut on_h.borrow_mut(), &send, &p),
            tick(&mut off_h.borrow_mut(), &send, &p),
        );
        if b >= at {
            ratio.push((moved / held).sqrt());
        }
    }
    assert!(
        ratio[0] > 0.9,
        "the level steps: {:.3} in the first block",
        ratio[0]
    );
    let last = ratio[ratio.len() - 1];
    assert!((last - 0.5).abs() < 0.05, "settles at {last:.3}, want 0.5");
}

/// Every fresh ROOM draws the same grain (goldens reproduce), and it is
/// there to draw.
#[test]
fn grain_is_deterministic() {
    let p = mv_room(MV05);
    let x = harness_burst(SR as usize / 2);
    let (a, off) = with_and_without_grain(&p, &x, 1.5);
    let (b, _) = with_and_without_grain(&p, &x, 1.5);
    let bits = |v: &[f32]| v.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&a.0), bits(&b.0));
    assert_eq!(bits(&a.1), bits(&b.1));
    assert_ne!(bits(&a.0), bits(&off.0), "no grain drawn");
}

/// ROOM's TAIL draws a pre-delay gap, then the fall (spec § Glyph).
#[test]
fn room_tail_draws_its_predelay_gap() {
    use chimera_core::ui::glyph::tail_shape;
    let mut p = params(0.5);
    p.predelay = 0.0;
    let open = tail_shape(ReverbType::Room, &knobs_120(&p));
    p.predelay = 1.0;
    let gap = tail_shape(ReverbType::Room, &knobs_120(&p));
    // 1.5 s over 60 columns: 50 ms is the first two.
    assert!(gap[..2].iter().all(|&c| c == 0) && open[0] > 0, "{gap:?}");
    assert!(gap[2] > 0);
}

// ── HALL ──

type H = Hall;
const HLENS: chimera_core::dsp::rev::loop_engine::Lens<1, 4> = <H as Layout<1, 4>>::LENS;

/// HALL's starting knobs at MIX `mix`.
fn hall(mix: f32) -> ReverbParams {
    ReverbParams {
        mix,
        ..ReverbParams::default()
    }
    .with_type(ReverbType::Hall, &KnobMemory::new())
}

/// HALL alone at the ring's rate: the engine, primed on `p`.
fn hall_engine(p: &ReverbParams) -> (H, Box<ReverbArena>) {
    let mut e = H::default();
    e.prime(&knobs_120(p));
    (e, Box::new(ReverbArena::new()))
}

/// The plate corner (spec § PLATE is a zone of HALL).
fn plate_corner() -> ReverbParams {
    let mut p = hall(1.0);
    (
        p.size, p.time, p.tone, p.diffuse, p.damping, p.predelay, p.smear, p.rate,
    ) = (0.0, 0.35, 0.0, 1.0, 0.1, 0.0, 0.5, 0.6);
    p
}

/// Σ x², f64.
fn energy_of(x: &[f32]) -> f64 {
    x.iter().map(|&v| v as f64 * v as f64).sum()
}

/// ADR 0095's diffuser table, kept here to check against: program 17's
/// lengths, ms, at SPREAD × 1.
const HALL_DIFFUSER_MS: [f32; 6] = [13.718, 18.419, 28.547, 34.530, 8.718, 6.154];
/// The taps a side by rank, (stage, fraction of its line), ADR 0095 §
/// HALL, as built.
const HALL_TAPS: [[(usize, f32); 8]; 2] = [
    [
        (0, 0.62),
        (1, 0.55),
        (2, 0.30),
        (0, 0.17),
        (1, 0.24),
        (2, 0.48),
        (3, 0.06),
        (3, 0.82),
    ],
    [
        (1, 0.02),
        (2, 0.90),
        (3, 0.93),
        (1, 0.48),
        (2, 0.57),
        (3, 0.26),
        (0, 0.43),
        (0, 0.60),
    ],
];

#[test]
fn hall_layout_matches_adr_0095() {
    let spec_ms: [f32; 8] = [
        113.0, 157.0, 124.0, 146.0, // stage delays S1–S4
        11.0, 17.0, 13.0, 19.0, // stage APs S1–S4
    ];
    let got: Vec<usize> = HLENS
        .delay
        .as_flattened()
        .iter()
        .chain(HLENS.ap.as_flattened())
        .copied()
        .collect();
    let mut taken = Vec::new();
    for (&ms, &n) in spec_ms.iter().zip(&got) {
        assert!(is_prime(n), "{ms} ms → {n}");
        assert_eq!(n, nearest_prime(ms, &taken), "{ms} ms");
        taken.push(n);
    }
    // The diffuser's lines hold SPREAD's top, × 1.10.
    for (j, &ms) in HALL_DIFFUSER_MS.iter().enumerate() {
        assert_eq!(
            HLENS.diffuser[j],
            (ms * 24.0 * 1.10 + 0.5) as usize,
            "AP {j}"
        );
        assert_eq!(le::DIFFUSER_MS[j], ms, "AP {j}");
    }
    assert_eq!(HLENS.pre, 3_000, "PRE-DLY's 125 ms");
    const { assert!(<H as Layout<1, 4>>::SLOTS + 2 * <H as Layout<1, 4>>::FSLOTS <= RING_LEN) };
    // SIZE's ends: the round trip 120 and 600 ms (within a sample a stage),
    // `lin` between: SIZE ½ is 360 ms.
    for (size, ms) in [(0.0, 120.0), (0.5, 360.0), (1.0, 600.0)] {
        let mut p = hall(1.0);
        p.size = size;
        let (e, _) = hall_engine(&p);
        let trip: usize = e.delays()[0].iter().map(|&d| d as usize).sum::<usize>()
            + HLENS.ap[0].iter().sum::<usize>();
        assert!(
            (trip as f32 / 24.0 - ms).abs() <= 1.0,
            "SIZE {size}: {trip}"
        );
    }
    // The taps: their table, their signs, and each one `frac` of its line.
    let taps = <H as Layout<1, 4>>::TAPS;
    for (side, row) in HALL_TAPS.iter().enumerate() {
        for (q, &(stage, frac)) in row.iter().enumerate() {
            let t = taps[side][q];
            assert_eq!((t.lp, t.stage, t.at.get()), (0, stage, frac), "{side} {q}");
            let sign = if stage % 2 == 0 {
                Sign::Plus
            } else {
                Sign::Minus
            };
            assert_eq!(t.sign, sign, "{side} {q}");
        }
    }
    let p = hall(1.0);
    let (e, _) = hall_engine(&p);
    let d = e.delays()[0];
    for (side, row) in HALL_TAPS.iter().enumerate() {
        for (q, &(stage, frac)) in row.iter().enumerate() {
            // `LATENCY` (17) nearer the write: the bus's half-bands.
            let want = ((frac * d[stage] as f32 + 0.5) as usize)
                .saturating_sub(17)
                .max(2);
            assert_eq!(e.tap_offsets()[side][q], want, "{side} {q}");
        }
    }
}

/// SPREAD scales the diffuser whole, × 0.05 → 1.10, and the diffuser
/// lands there by crossfades of one allpass and at most 16 samples each.
#[test]
fn spread_scales_the_six_allpass_diffuser() {
    let at = |spread: f32| {
        let mut p = hall(1.0);
        p.shape = spread;
        hall_engine(&p).0.diffuser()
    };
    let want = |s: f32| HALL_DIFFUSER_MS.map(|ms| ((ms * 24.0 * s + 0.5) as u16).max(2));
    assert_eq!(at(0.0), want(0.05));
    assert_eq!(at(1.0), want(1.10));
    let mut p = hall(1.0);
    p.shape = 0.0;
    let (mut e, mut arena) = hall_engine(&p);
    p.shape = 1.0;
    let k = knobs_120(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    let mut last = e.diffuser();
    for _ in 0..20_000 {
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let now = e.diffuser();
        let moved: Vec<usize> = (0..6).filter(|&j| now[j] != last[j]).collect();
        assert!(moved.len() <= 1, "{last:?} → {now:?}");
        for j in moved {
            assert!(now[j].abs_diff(last[j]) <= 16, "{last:?} → {now:?}");
        }
        last = now;
        if !e.moving() {
            assert_eq!(e.diffuser(), want(1.10));
            return;
        }
    }
    panic!("never landed");
}

/// The diffuser is allpasses: what goes in comes out, energy for energy.
/// With DECAY at its floor the loop barely recirculates, so the return's
/// energy (a click's, over its 2 s) is close at either end of SPREAD and
/// of DIFFUSE.
#[test]
fn hall_diffuser_keeps_the_energy() {
    let energy = |spread: f32, diffuse: f32| {
        let mut p = hall(1.0);
        (p.time, p.shape, p.diffuse, p.smear, p.size) = (0.0, spread, diffuse, 0.0, 0.5);
        let (rv, pc) = (fresh(&p), RefCell::new(p));
        let (l, r) = run(&mut bus(&rv, &pc), &mut |i| (i == 0) as u8 as f32, 2.0);
        energy_of(&l) + energy_of(&r)
    };
    let e0 = energy(0.0, 0.0);
    for (s, d) in [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let e = energy(s, d);
        let db = 10.0 * (e / e0).log10();
        println!("SPREAD {s} DIFFUSE {d}: {db:+.2} dB");
        assert!(db.abs() <= 1.5, "SPREAD {s} DIFFUSE {d}: {db} dB");
    }
}

/// DENSITY fades taps 4–8 a side in, a fifth of the knob each: 3 taps at
/// 0, 8 at 1. The 1 kHz RT60 doesn't move, and the early return grows.
#[test]
fn density_fades_in_taps_four_to_eight() {
    assert_eq!(TapCount::from_unit(0.0), TapCount::MIN);
    assert_eq!(TapCount::from_unit(1.0), TapCount::MAX);
    let c = TapCount::from_unit(0.5);
    assert_eq!(c.get(), 5.5);
    assert_eq!(
        (0..8).map(|q| c.weight(q)).collect::<Vec<_>>(),
        [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0, 0.0]
    );
    let mut p = hall(1.0);
    p.density = 0.0;
    assert_eq!(knobs_120(&p).density, TapCount::MIN);
    let (e, _) = hall_engine(&p);
    assert_eq!(e.taps(), 3.0);
    // Every tap plays at its weight: sign by its line, ¾, DENSITY's
    // fade, × √(3/Σw²) so the level holds.
    for (density, count) in [(0.0, 3.0), (0.5, 5.5), (1.0, 8.0)] {
        let (e, _) = hall_engine(&ReverbParams { density, ..p });
        let c = TapCount::from_unit(density);
        assert_eq!(c.get(), count);
        let norm = (3.0 / (0..8).map(|q| c.weight(q).powi(2)).sum::<f32>()).sqrt();
        for (side, row) in HALL_TAPS.iter().enumerate() {
            for (q, &(stage, _)) in row.iter().enumerate() {
                let sign = if stage % 2 == 0 { 1.0 } else { -1.0 };
                let want = sign * 0.75 * norm * c.weight(q);
                let got = e.tap_weights()[side][q];
                assert!(
                    (got - want).abs() <= 1e-6,
                    "DENSITY {density}: {side} {q}: {got}"
                );
            }
        }
    }
    let early = |density: f32| {
        let mut q = hall(1.0);
        (q.density, q.smear) = (density, 0.0);
        let (l, r) = noise_burst(&q, 0.8);
        let w = SR as usize / 10..SR as usize / 2;
        energy_of(&l[w.clone()]) + energy_of(&r[w])
    };
    // The level holds across the knob (F8: the taps are normalised).
    let (sparse, dense) = (early(0.0), early(1.0));
    let db = 10.0 * (dense / sparse).log10();
    println!("DENSITY 0 → 1: {db:+.1} dB");
    assert!(db.abs() <= 1.0, "{sparse} → {dense}");
    let rt = |density: f32| {
        band_rt60(
            &ReverbParams {
                density,
                ..hall(1.0)
            },
            1_000.0,
        )
    };
    let (a, b) = (rt(0.0), rt(1.0));
    assert!((b / a - 1.0).abs() <= 0.05, "1 kHz RT60 {a} → {b}");
}

/// One damper ends the loop: DAMP darkens the 4 kHz octave's decay and
/// leaves the 500 Hz one. TONE 0 is OFF (an open one-pole), its top 2 kHz.
#[test]
fn hall_damp_darkens_the_loop_and_tone_opens_at_0() {
    let mut p = hall(1.0);
    (p.size, p.time, p.smear, p.tone, p.damping) = (0.5, 0.5, 0.0, 0.0, 0.0);
    let k = knobs_120(&p);
    assert!(k.tone_hz.is_infinite(), "TONE 0: {}", k.tone_hz);
    assert_eq!(
        hall_engine(&p).0.targets().0,
        1.0,
        "TONE OFF passes the send"
    );
    assert!((knobs_120(&ReverbParams { tone: 1.0, ..p }).tone_hz - 2_000.0).abs() < 0.01);
    assert_eq!(k.damp_hz, 14_000.0);
    assert!((knobs_120(&ReverbParams { damping: 1.0, ..p }).damp_hz - 1_300.0).abs() < 1.0);
    let dark = ReverbParams { damping: 1.0, ..p };
    let (o4, d4) = (band_rt60(&p, 4_000.0), band_rt60(&dark, 4_000.0));
    let (o5, d5) = (band_rt60(&p, 500.0), band_rt60(&dark, 500.0));
    println!("DAMP 14 → 1.3 kHz: 4 kHz {o4:.3} → {d4:.3} s, 500 Hz {o5:.3} → {d5:.3} s");
    assert!(d4 <= 0.7 * o4, "4 kHz: {o4} → {d4}");
    assert!(d5 >= 0.85 * o5, "500 Hz: {o5} → {d5}");
}

/// DECAY's top is 16 s (#29 Xlarge Warm 15.0 fits there): at SIZE 1 the
/// stage gains carry it under the 0.985 cap.
#[test]
fn hall_reaches_16_s() {
    let mut p = hall(1.0);
    (p.size, p.time, p.damping, p.smear) = (1.0, 1.0, 0.0, 0.0);
    assert_eq!(knobs_120(&p).rt60_s, 16.0);
    let (e, _) = hall_engine(&p);
    let g = e.targets().2[0];
    assert!(g.iter().all(|&g| g <= 0.985), "{g:?}");
    let rt = band_rt60(&p, 1_000.0);
    println!("SIZE 1 DECAY 1: 1 kHz RT60 {rt:.2} s");
    assert!((rt / 16.0 - 1.0).abs() <= 0.2, "RT60 {rt}");
}

/// HALL's 1 kHz RT60 tracks DECAY across the knob, at its default SIZE
/// with DAMP open.
#[test]
fn hall_rt60_tracks_decay() {
    for decay in [0.0, 0.25, 0.45, 0.5, 0.75, 1.0] {
        let mut p = hall(1.0);
        (p.time, p.damping) = (decay, 0.0);
        let want = knobs_120(&p).rt60_s;
        let rt = band_rt60(&p, 1_000.0);
        println!("DECAY {decay}: 1 kHz RT60 {rt:.2} s, target {want:.2} s");
        assert!(
            (rt / want - 1.0).abs() <= 0.2,
            "DECAY {decay}: {rt} vs {want}"
        );
    }
}

#[test]
fn plate_corner_onset_is_short() {
    let p = plate_corner();
    let k = knobs_120(&p);
    assert_eq!(
        (k.diffuse_g, k.tone_hz, k.predelay_ms, k.spread.scale()),
        (0.8, f32::INFINITY, 0.0, 0.05)
    );
    let (l, r) = noise_burst(&p, 1.0);
    let onset = onset_ms(&l).min(onset_ms(&r));
    println!("plate corner: onset {onset} ms");
    assert!(onset <= 8.0, "onset {onset} ms");
}

/// PRE-DLY at 0.5 and SIZE 0.05 → 0: the plate zone pulls PRE-DLY's
/// 62.5 ms to 0, and the read only moves at a crossfade's start, by at
/// most 120 samples (5 ms) a crossfade, in the same crossfades as SIZE.
#[test]
fn plate_zone_moves_predelay_by_crossfade() {
    let mut p = hall(1.0);
    (p.predelay, p.size) = (0.5, 0.05);
    let (mut e, mut arena) = hall_engine(&p);
    assert_eq!(e.predelay(), 1_500);
    p.size = 0.0;
    let k = knobs_120(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    let (mut crossfades, mut last) = (0, None);
    for b in 0..20_000 {
        let before = e.predelay();
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        match e.crossfade() {
            // A crossfade of the other lengths (SPREAD's pull) holds PRE-DLY.
            Some((_, _, pre)) if pre.0 == pre.1 => {
                assert_eq!(pre.0, before, "block {b}: PRE-DLY held");
            }
            Some((_, _, pre)) if last != Some(pre) => {
                crossfades += 1;
                assert_eq!(pre.0, before, "block {b}: from where it landed");
                assert!(pre.0.abs_diff(pre.1) <= 120, "block {b}: {pre:?}");
                last = Some(pre);
            }
            Some((_, _, pre)) => assert_eq!(Some(pre), last, "block {b}: moved mid-crossfade"),
            None => {
                // Between crossfades the read sits where the last landed.
                assert_eq!(e.predelay(), last.map_or(1_500, |p| p.1), "block {b}");
                if !e.moving() {
                    println!("{crossfades} crossfades");
                    assert_eq!(e.predelay(), 2, "PRE-DLY 0 is 2 behind");
                    assert_eq!(crossfades, 13);
                    return;
                }
            }
        }
    }
    panic!("never landed");
}

#[test]
fn hall_decorrelates() {
    let (l, r) = noise_burst(&hall(1.0), 2.5);
    let c = corr(&l, &r);
    println!("defaults: corr(L, R) {c:.3}");
    assert!(c.abs() <= 0.2, "corr {c}");
}

/// SIZE 0 → 1, then PRE-DLY 0 → 1 once it lands, after a 1 kHz tone has
/// rung into HALL's 120 ms loop: no read resamples the loop, so nothing
/// accumulates. Once landed, the tail's peak holds still: three 2.4 s
/// windows within 1/L (L 0.6 s) of each other. And it sits within the
/// SIZE-0 loop's mode spacing (8.3 Hz) of 1 kHz: the 121 crossfades'
/// combs can hand the peak to a neighbouring line once, mid-sweep (here
/// 999.7 → 1002.7 Hz, 5 cents), but never walk it.
#[test]
fn hall_settles_on_its_input_pitch() {
    let mut p = hall(1.0);
    (p.time, p.smear, p.size, p.predelay) = (1.0, 0.0, 0.0, 0.0);
    let sine = tone(1_000.0, 0.5);
    let send = |b: usize| -> Blk {
        core::array::from_fn(|i| {
            let n = b * BLOCK_SIZE + i;
            if n < SR as usize / 2 { sine(n) } else { 0.0 }
        })
    };
    let (window, trip) = (blocks(1.2), blocks(0.6));
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let (mut l, mut done) = (Vec::new(), None);
    for b in 0..blocks(30.0) {
        if b == blocks(0.6) {
            pc.borrow_mut().size = 1.0;
        } else if b > blocks(0.6) && done.is_none() && !rv.borrow().hall().unwrap().moving() {
            let mut q = pc.borrow_mut();
            if q.predelay == 0.0 {
                q.predelay = 1.0;
            } else {
                done = Some(b);
            }
        }
        l.push(bus(&rv, &pc)(&send(b)).0);
        if done.is_some_and(|d| b >= d + trip + 8 * window) {
            break;
        }
    }
    // The power-weighted mean of 1 kHz ± 25 Hz (one Hann frame of 2^16):
    // the tail's modes trade the peak as they decay at their own rates,
    // but an accumulated shift moves the whole band.
    let centroid_hz = |x: &[f32]| {
        let n = 1 << 16;
        let mut pad = x.to_vec();
        pad.resize(n, 0.0);
        let p = common::sweep::power(&pad, n);
        let hz = SR as f64 / n as f64;
        let (lo, hi) = ((975.0 / hz) as usize, (1_025.0 / hz) as usize);
        let (w, m) = (lo..=hi).fold((0.0, 0.0), |(w, m), k| (w + p[k], m + p[k] * k as f64 * hz));
        (m / w) as f32
    };
    let d = done.expect("the moves land");
    // Judged only 20 dB or more over the storage's LSB at the return.
    let lsb = wet_gain(ReverbType::Hall) / FULL_SCALE;
    let f: Vec<(f32, f32)> = (0..8)
        .map(|w| {
            let x = l[d + trip + w * window..][..window].as_flattened();
            (centroid_hz(x), common::rev::rms_db(x) - 20.0 * lsb.log10())
        })
        .take_while(|&(_, over)| over >= 20.0)
        .collect();
    println!(
        "landed at {:.2} s; settled peaks (Hz, dB over the LSB) {f:?}",
        d as f32 * BLOCK_SIZE as f32 / SR as f32
    );
    assert!(f.len() >= 3, "{f:?}");
    // Nothing walks: the windows 30 dB or more over the LSB within 0.25
    // Hz of each other. Where the modes weight the band sits within 1 Hz
    // of the input, under the SIZE-1 loop's 1.67 Hz spacing (one damper
    // and eight taps put it 0.8 Hz over; ADR 0095 § HALL, as built).
    let loud: Vec<f32> = f.iter().filter(|w| w.1 >= 30.0).map(|w| w.0).collect();
    let (lo, hi) = loud
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    assert!(hi - lo <= 0.25, "{f:?}");
    for &(x, _) in &f {
        assert!((x - 1_000.0).abs() <= 1.0, "{f:?}");
    }
}

/// SMEAR at 1 and RATE 1 (1.5 Hz) at SIZE 0, its 120 ms loop: D is capped
/// at 0.36 % of the loop, and the engine holds both smeared reads to it
/// however deep they are asked, so E ≤ 1.45 % of the round trip: 25 cents
/// (ADR 0085's ROOM deviation 5, the bound held in the engine). Zero
/// crossings judge a still HALL (0–2 cents pass to pass), but the
/// smeared tail's per-pass reading wanders among the 8 Hz-spaced loop
/// modes beside its frozen reference; the spectral peak is judged instead.
#[test]
fn hall_smear_is_bounded_without_drift_or_vibrato() {
    let mut p = hall(1.0);
    (p.size, p.time, p.smear, p.rate) = (0.0, 1.0, 1.0, 1.0);
    let k = knobs_120(&p);
    assert!(k.smear_ms <= 0.0036 * k.length_ms * (1.0 + 1e-6));
    assert!(k.smear_ms > 0.4, "capped, not off: {}", k.smear_ms);
    for size in [0.0, 0.5, 1.0] {
        p.size = size;
        let k = knobs_120(&p);
        let (mut e, mut arena) = hall_engine(&p);
        let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
        e.set_smear([(1e9, -1e9), (-5.0, 1e9)]);
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        let trip: usize = e.delays()[0].iter().map(|&d| d as usize).sum::<usize>()
            + HLENS.ap[0].iter().sum::<usize>();
        let held = e.smear();
        for (m, &(_, st)) in <H as Layout<1, 4>>::SMEARED.iter().enumerate() {
            for x in [held[m].0, held[m].1] {
                assert!(
                    (0.0..=2.0 * 0.0036 * trip as f32 + 1e-3).contains(&x),
                    "{x}"
                );
                assert!(HLENS.ap[0][st] as f32 - x >= 2.0);
            }
        }
        let e_sum: f32 = held.iter().map(|h| h.0.max(h.1)).sum();
        assert!(
            e_sum / trip as f32 <= 0.0145,
            "SIZE {size}: E {e_sum} of {trip}"
        );
    }
    // A held 1 kHz tone's tail at SIZE 0, its peak in two 2 s windows:
    // SMEAR 1 hops it no more than 25 cents beyond SMEAR 0's own hop.
    let hop = |smear: f32| {
        // DECAY 0.7 (8 s): the tail still rings in the second window.
        let mut p = hall(1.0);
        (p.smear, p.rate, p.size, p.time) = (smear, 1.0, 0.0, 0.7);
        let sine = tone(1_000.0, 0.25);
        let (l, _) = run(
            &mut smeared::<Hall>(p),
            &mut |i| if i < SR as usize / 2 { sine(i) } else { 0.0 },
            9.5,
        );
        let at = |a: f32, b: f32| {
            settled_peak_hz(&l[(a * SR as f32) as usize..(b * SR as f32) as usize])
        };
        // Two 2 s windows while the tail stands well over the LSB: the
        // stage delays truncate under 3 LSB, so SIZE 0's tail is gone by 8 s.
        let (f1, f2) = (at(1.0, 3.0), at(3.0, 5.0));
        println!("SMEAR {smear}: {f1:.2} → {f2:.2} Hz");
        (1200.0 * (f2 / f1).log2()).abs()
    };
    let (still, moving) = (hop(0.0), hop(1.0));
    assert!(
        moving <= still + 25.0,
        "SMEAR 1 hops {moving} cents, SMEAR 0 {still}"
    );
}

#[test]
fn level_plan_holds_for_hall() {
    let mut p = hall(1.0);
    (p.size, p.diffuse) = (0.0, 1.0);
    // A 0 dBFS tone held on a loop mode at DECAY's top drives the clip far
    // past its knee (G 12.5 against ROOM's 2.7): soft saturation, not 3 %
    // THD. One damper a loop (ADR 0095 § HALL, as built) leaves 5 kHz's
    // mode G 4.5 against two dampers' 2.4, so a held 5 kHz clips too and
    // aliases: −30 dB as built (−21 before the f32 diffuser), held to −20
    // for the owner's sign-off (ADR 0095 § HALL, as built).
    check_loop_level_plan::<Hall>(p, 0.2, -20.0);
}

#[test]
fn hall_knobs_ease() {
    let p = hall(1.0);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let ids = [1, 2, 3, 7, 8, 9, 10, 11, 12, 14].map(ParamId);
    check_knobs_ease(&mut bus(&rv, &pc), &ids, &mut |id, v| {
        pc.borrow_mut().write(id, v)
    });
}

/// A 300 Hz tone held through HALL at START (SMEAR 0) while `id` moves
/// `from` → `to`, at once or swept over `sweep_s`: the move's largest
/// second difference against the larger of the steady tail's before and
/// the settled tail's after (2 s each, both sides), and the same ratio of
/// the peaks, the tone's own level as it moves.
fn held_tone_move_ratio(id: ParamId, from: f32, to: f32, sweep_s: f32) -> (f32, f32) {
    let mut p = hall(1.0);
    p.smear = 0.0;
    p.write(id, from);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let held = tone(300.0, 0.25);
    let (pre, sweep, post) = (blocks(3.0), blocks(sweep_s).max(1), blocks(8.0));
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let drive = |b: usize| -> Blk { core::array::from_fn(|i| held(b * BLOCK_SIZE + i)) };
    for b in 0..pre + sweep + post {
        if b >= pre && b < pre + sweep {
            let x = (b - pre + 1) as f32 / sweep as f32;
            pc.borrow_mut().write(id, from + (to - from) * x);
        }
        let (ol, or) = bus(&rv, &pc)(&drive(b));
        l.extend(ol);
        r.extend(or);
    }
    let at = |blk: usize| blk * BLOCK_SIZE;
    let d2 = |a: usize, b: usize| max_d2(&l[a..b]).max(max_d2(&r[a..b]));
    // Judged over as long a stretch as the move's own, so a rare peak
    // weighs alike: the 2 s before the move, and the last 2 s, after the
    // diffuser has landed.
    let span = 2 * SR as usize;
    let steady = d2(at(pre) - span, at(pre));
    let settled = d2(l.len() - span, l.len());
    let moving = d2(at(pre), l.len() - span);
    let pk = |a: usize, b: usize| {
        l[a..b]
            .iter()
            .chain(&r[a..b])
            .fold(0.0f32, |m, v| m.max(v.abs()))
    };
    let base = pk(at(pre) - span, at(pre)).max(pk(l.len() - span, l.len()));
    (
        moving / steady.max(settled),
        pk(at(pre), l.len() - span) / base,
    )
}

/// SPREAD and DENSITY move under a held tone with no click (F2): the
/// move's d2 within 1.2× the tail's own, once the tone's level is
/// counted. Either move changes the tone's phase into a 3.5 s loop (SPREAD
/// through the diffuser, DENSITY through which taps sum it), so the old
/// and new phases beat and the tail's level swings, up to 1.7× for SPREAD
/// as for PRE-DLY (1.4×); d2 follows the level. DENSITY's weights ramp each
/// sample while they move, SPREAD's allpasses move 16 samples a crossfade.
#[test]
fn spread_and_density_move_smoothly_under_a_held_tone() {
    for (id, name) in [(ParamId(14), "DENSITY"), (ParamId(10), "SPREAD")] {
        for (from, to, sweep) in [
            (0.0, 1.0, 0.0),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.3),
            (0.45, 0.55, 0.0),
        ] {
            let (d2, level) = held_tone_move_ratio(id, from, to, sweep);
            println!("{name} {from} → {to} over {sweep} s: d2 ratio {d2:.2}, level {level:.2}");
            assert!(
                d2 <= 1.2 * level.max(1.0),
                "{name} {from} → {to} over {sweep} s: {d2} at level {level}"
            );
        }
    }
}

/// At MIX 1, a held pad's peak through HALL stays within 1 dB of QUAD's at
/// DENSITY 0 and 1, and DENSITY moves the loudness under 1 dB (F8).
#[test]
fn hall_peak_and_level_hold_across_density() {
    let quad = ReverbParams {
        mix: 1.0,
        ..ReverbParams::default()
    };
    let (qv, qc) = (fresh(&quad), RefCell::new(quad));
    let (_, qp) = common::rev::sustained_db(&mut bus(&qv, &qc));
    let mut level = Vec::new();
    for density in [0.0, 1.0] {
        let p = ReverbParams {
            density,
            ..hall(1.0)
        };
        let (rv, pc) = (fresh(&p), RefCell::new(p));
        let (a, ap) = common::rev::sustained_db(&mut bus(&rv, &pc));
        println!("DENSITY {density}: {a:.2} dB, peak {ap:.2} dBFS; QUAD peak {qp:.2}");
        assert!(
            ap <= qp + 1.0,
            "DENSITY {density}: peak {ap} over QUAD's {qp}"
        );
        level.push(a);
    }
    assert!((level[1] - level[0]).abs() <= 1.0, "{level:?}");
}

#[test]
fn hall_noise_and_loudness() {
    let (p, q) = (
        hall(0.5),
        ReverbParams {
            mix: 0.5,
            ..ReverbParams::default()
        },
    );
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let (qv, qc) = (fresh(&q), RefCell::new(q));
    check_sustained_loudness(&mut bus(&rv, &pc), &mut bus(&qv, &qc));
    // Each floor from a fresh engine: HALL's 2.5 s would still ring with
    // the pad. With the four shortest diffuser allpasses on f32 states it
    // sits 3.9 and 5.5 dB over QUAD's at START and DIFFUSE 1 (WET_HALL
    // 14.0): within ADR 0085's 4 and 6 dB again.
    for (diffuse, extra) in [(p.diffuse, 4.0), (1.0, 6.0)] {
        let h = ReverbParams { diffuse, ..p };
        let (rv, pc) = (fresh(&h), RefCell::new(h));
        let (qv, qc) = (fresh(&q), RefCell::new(q));
        check_noise_floor(&mut bus(&rv, &pc), &mut bus(&qv, &qc), extra);
    }
}

/// The cached targets follow every move: DECAY, TONE, DAMP, then SIZE
/// turned mid-run; once the lengths land they equal a fresh engine's
/// primed at the final knobs.
#[test]
fn hall_targets_follow_decay_tone_damp_and_size() {
    let mut p = hall(1.0);
    let (mut e, mut arena) = hall_engine(&p);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    for b in 0..4_000 {
        match b {
            10 => p.time = 1.0,
            20 => p.tone = 0.9,
            25 => p.damping = 1.0,
            30 => p.size = 1.0,
            _ => {}
        }
        e.run(&mut arena, &u, &knobs_120(&p), &mut l, &mut r);
    }
    assert!(!e.moving(), "the move lands");
    let (fresh, _) = hall_engine(&p);
    assert_eq!(e.delays(), fresh.delays());
    let ((ta, da, g), (fta, fda, fg)) = (e.targets(), fresh.targets());
    assert_eq!((ta, da), (fta, fda));
    assert_eq!(g, fg, "stage-gain targets stale");
}

/// The return reaches exact zero within 1.5 × RT60 (plus the taps), from
/// a noise burst at the defaults, at SMEAR 1, and at DAMP 1, where the
/// dampers' coefficient sits under ½ (fc 1.5 kHz) and a rounding one-pole
/// would hold ±1 LSB of DC; and from a held 1 kHz tone at the plate.
#[test]
fn hall_falls_silent() {
    for (smear, damp, size, noise) in [
        (0.0, 0.3, 0.65, true),
        (1.0, 0.3, 0.65, true),
        (0.0, 1.0, 0.65, true),
        (0.0, 1.0, 0.0, false),
    ] {
        let mut p = hall(1.0);
        (p.time, p.smear, p.damping, p.size) = (0.0, smear, damp, size);
        let seconds = 0.5 + 1.5 * knobs_120(&p).rt60_s + 1.0;
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
            "SMEAR {smear} DAMP {damp} SIZE {size}: still ringing"
        );
    }
}

/// H1's own case: the plate's 120 ms loop at DECAY 1 (10 s, a round trip
/// of 0.927), where R2's rounded i16 allpass states held a cycle 45 dB
/// down. HALL alone at 24 kHz, a 0.5 s noise burst, its SMEAR offsets
/// moving at SMEAR 1: the return is exactly 0 by 2 × RT60 + 1 s. (ROOM's
/// 1.5 × is 90 dB at the nominal rate; this burst stands 81 dB over the
/// LSB and HALL measures 11.9 s here, so its last LSBs go near 19 s.)
#[test]
fn hall_plate_at_10_s_falls_silent() {
    for (smear, damp) in [(0.0, 0.0), (1.0, 1.0)] {
        let mut p = hall(1.0);
        (p.smear, p.damping, p.size, p.time) = (smear, damp, 0.0, 1.0);
        let k = knobs_120(&p);
        let (mut e, mut arena) = hall_engine(&p);
        let full = k.smear_ms * PER_MS;
        let mut n = Noise(0x5eed);
        let ring = 24_000.0;
        let blocks = ((0.5 + 2.0 * k.rt60_s + 1.0) * ring) as usize / HALF;
        let (mut l, mut r) = ([0.0f32; HALF], [0.0; HALF]);
        let mut last = Vec::new();
        for b in 0..blocks {
            let u: [f32; HALF] = core::array::from_fn(|i| {
                if b * HALF + i < 12_000 {
                    0.5 * n.next()
                } else {
                    0.0
                }
            });
            // A slow sweep of both offsets over 0..2D: SMEAR moving.
            let x = |b: usize| (b as f32 * 0.003).sin();
            e.set_smear([0, 1].map(|m| {
                let ph = m as f32 * 1.7;
                (
                    full * (1.0 + x(b) * ph.cos()),
                    full * (1.0 + x(b + 1) * ph.cos()),
                )
            }));
            e.run(&mut arena, &u, &k, &mut l, &mut r);
            if b + 25 >= blocks {
                last.extend(l.iter().chain(&r).copied());
            }
        }
        let loud = last.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert_eq!(
            loud, 0.0,
            "SMEAR {smear} DAMP {damp}: still ringing at {loud} LSB"
        );
    }
}

/// HALL's TAIL draws its pre-delay gap; in the plate zone the gap closes,
/// as PRE-DLY plays (spec § PLATE is a zone of HALL: TAIL draws the
/// effective values).
#[test]
fn hall_tail_draws_the_effective_predelay() {
    use chimera_core::ui::glyph::tail_shape;
    let mut p = hall(0.5);
    p.predelay = 1.0;
    let gap = tail_shape(ReverbType::Hall, &knobs_120(&p));
    // 10 s over 60 columns: 125 ms is under the first.
    assert_eq!(gap[0], 0, "{gap:?}");
    p.size = 0.0;
    let plate = tail_shape(ReverbType::Hall, &knobs_120(&p));
    assert!(plate[0] > 0, "{plate:?}");
}

/// Every tap reads its own stage line, at its own offset: an impulse into
/// one loop alone (stage allpasses at g 0, θ 0, dampers open, SIZE 1)
/// reaches each side's two taps on that loop's lines at the line's write
/// time plus the tap's offset.
#[test]
fn each_tap_reads_its_line_at_its_offset() {
    let mut p = params(1.0);
    (
        p.shape, p.predelay, p.damping, p.time, p.size, p.tone, p.smear,
    ) = (0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0);
    let k = knobs_120(&p);
    for lp in 0..4 {
        let (mut e, mut arena) = engine(&p);
        e.probe_loop(lp as u8);
        let (mut l, mut r) = ([0.0f32; HALF], [0.0; HALF]);
        let mut out = [Vec::new(), Vec::new()];
        for b in 0..6_000 / HALF {
            let u: [f32; HALF] = core::array::from_fn(|i| (b == 0 && i == 0) as u8 as f32);
            e.run(&mut arena, &u, &k, &mut l, &mut r);
            out[0].extend_from_slice(&l);
            out[1].extend_from_slice(&r);
        }
        // The send writes line (lp, 0) 2 samples on (PRE-DLY's floor);
        // stage 1's output, a delay and an allpass of g 0 on, line (lp, 1).
        let w0 = 2;
        let w1 = w0 + room::DELAY[lp][0] + room::AP[lp][0];
        let peak = out.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        for side in 0..2 {
            for (s, w) in [(0, w0), (1, w1)] {
                let j = 2 * lp + s;
                let tap = if side == 1 && j == 0 {
                    12
                } else {
                    n24(TAP_MS[side][j])
                };
                let x = out[side][w + tap];
                assert!(
                    x.abs() >= 0.3 * peak,
                    "loop {lp} side {side} tap {j}: {x} at {} (peak {peak})",
                    w + tap
                );
            }
        }
    }
}

/// The diffuser is ADR 0095's five allpasses at g 0.75: before any loop
/// returns, R reads loop A's first line through its fixed 12-sample tap,
/// so its first 100 samples are ¾ of the diffused send, 14 samples late
/// (PRE-DLY's 2 and the tap's 12), within the lines' rounding.
#[test]
fn the_diffuser_is_five_allpasses() {
    let mut p = params(1.0);
    // TONE under its open top: a one-pole that spreads the impulse.
    (p.predelay, p.tone, p.size) = (0.0, 0.1, 0.0);
    let k = knobs_120(&p);
    let (mut e, mut arena) = engine(&p);
    // The structure alone: GRAIN rides on the return.
    e.grain_off();
    let (mut l, mut r) = ([0.0f32; HALF], [0.0; HALF]);
    let mut got = Vec::new();
    for b in 0..4 {
        let u: [f32; HALF] = core::array::from_fn(|i| (b == 0 && i == 0) as u8 as f32);
        e.run(&mut arena, &u, &k, &mut l, &mut r);
        got.extend_from_slice(&r);
    }
    // The model: TONE's one-pole, PRE-DLY's rounding store, then five
    // allpasses (v = x − g·z, y = z + g·v).
    let a = 1.0 - (-std::f64::consts::TAU * f64::from(k.tone_hz) / 24_000.0).exp();
    let (mut tone, mut lines): (f64, Vec<Vec<f64>>) = (
        0.0,
        DIFFUSER_MS.iter().map(|&ms| vec![0.0; n24(ms)]).collect(),
    );
    let mut want = vec![0.0f64; got.len()];
    for t in 0..got.len() - 14 {
        tone += a * ((t == 0) as u8 as f64 * 8_192.0 - tone);
        let mut x = tone.round();
        for line in &mut lines {
            let m = line.len();
            let z = line[t % m];
            let v = x - 0.75 * z;
            line[t % m] = v;
            x = z + 0.75 * v;
        }
        want[t + 14] = 0.75 * x.round();
    }
    for t in 0..100 {
        assert!(
            (got[t] as f64 - want[t]).abs() <= 1.0,
            "sample {t}: {} against {}",
            got[t],
            want[t]
        );
    }
    assert!(want[14..100].iter().filter(|x| x.abs() > 1.0).count() > 20);
}

/// The return through a SIZE move: each moving tap blends its two reads
/// across the crossfade, so a held tone's return steps at most twice as
/// far as at a held SIZE. A tap snapping to its new read steps by up to the
/// tone's swing over 120 samples.
#[test]
fn taps_blend_through_a_size_move() {
    let steps = |moving: bool| {
        let mut p = params(1.0);
        (p.size, p.smear, p.time, p.predelay) = (0.0, 0.0, 0.5, 0.0);
        let (mut e, mut arena) = engine(&p);
        let (mut l, mut r) = ([0.0f32; HALF], [0.0; HALF]);
        let mut most = 0.0f32;
        let mut prev = (0.0f32, 0.0f32);
        for b in 0..1_500 {
            if b == 500 && moving {
                p.size = 1.0;
            }
            let k = knobs_120(&p);
            let u: [f32; HALF] = core::array::from_fn(|i| {
                (std::f32::consts::TAU * 150.0 * (b * HALF + i) as f32 / 24_000.0).sin() * 0.25
            });
            e.run(&mut arena, &u, &k, &mut l, &mut r);
            for i in 0..HALF {
                if b >= 500 {
                    most = most.max((l[i] - prev.0).abs()).max((r[i] - prev.1).abs());
                }
                prev = (l[i], r[i]);
            }
        }
        most
    };
    let (held, moved) = (steps(false), steps(true));
    println!("largest step: {moved:.1} moving, {held:.1} held");
    // The move itself colours the loops (1.45× here); a snapping tap
    // reads 35×.
    assert!(
        moved <= 2.0 * held,
        "a SIZE move steps {moved}, held {held}"
    );
}

/// An impulse into each loop alone (stage allpasses at g 0, θ 0) reaches
/// its second line exactly a stage delay and a stage allpass after its
/// first, and its first line again a second stage later, at SIZE 0 and 1:
/// L's tap on each line reads the pulse there, and nothing a sample early.
/// The lengths are the engine's own, which `room_layout_matches_adr_0095`
/// pins to ADR 0095.
#[test]
fn a_stage_is_its_delay_and_its_allpass_exactly() {
    for size in [0.0, 1.0] {
        let mut p = params(1.0);
        (
            p.shape, p.predelay, p.damping, p.time, p.size, p.tone, p.smear,
        ) = (0.0, 0.0, 0.0, 1.0, size, 0.0, 0.0);
        let k = knobs_120(&p);
        for lp in 0..4 {
            let (mut e, mut arena) = engine(&p);
            e.probe_loop(lp as u8);
            let (d, tap) = (e.delays()[lp], e.taps().0[0]);
            let (mut l, mut r) = ([0.0f32; HALF], [0.0; HALF]);
            let mut out = Vec::new();
            for b in 0..6_000 / HALF {
                let u: [f32; HALF] = core::array::from_fn(|i| (b == 0 && i == 0) as u8 as f32);
                e.run(&mut arena, &u, &k, &mut l, &mut r);
                out.extend_from_slice(&l);
            }
            // The send writes line 1 at 2 (PRE-DLY's floor); stage 1 writes
            // line 2 a delay and an allpass on; stage 2 line 1 again.
            let line2 = 2 + d[0] as usize + room::AP[lp][0];
            let back = line2 + d[1] as usize + room::AP[lp][1];
            let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            for (what, at) in [
                ("line 2", line2 + tap[2 * lp + 1] as usize),
                ("line 1, a trip on", back + tap[2 * lp] as usize),
            ] {
                assert!(
                    out[at].abs() >= 0.3 * peak,
                    "SIZE {size} loop {lp} {what}: {} at {at}",
                    out[at]
                );
                assert!(
                    out[at - 1].abs() <= 1e-3 * peak,
                    "SIZE {size} loop {lp} {what}: {} a sample early",
                    out[at - 1]
                );
            }
        }
    }
}
