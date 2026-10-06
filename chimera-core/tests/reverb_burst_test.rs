//! GATE and REVERSE on the BURST engine (reverb TYPEs spec § BURST engine,
//! § Level plan, § Moving a length, § Pitch bounds, § Tests).

mod common;

use std::cell::RefCell;

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::halfband::{Decimator, HALF, Interpolator};
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::arena::ReverbArena;
use chimera_core::dsp::rev::burst::{
    BurstEngine, GATE_TAPS, MOVERS, REVERSE_TAPS, SLOTS, STEP, TAP_SEED, tap_table, tap_targets,
    tap_weights,
};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType};
use chimera_core::ui::glyph::tail_shape;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{
    MS, Noise, blocks, check_knobs_ease, check_level_plan, check_no_slap, check_noise_floor,
    check_settles_on_pitch, check_sustained_loudness, corr, end_20db_ms, knobs_120, peak_ms,
    rms_db, run,
};

type Blk = [f32; BLOCK_SIZE];

const BURSTS: [ReverbType; 2] = [ReverbType::Gate, ReverbType::Reverse];

/// `t`'s starting knobs at MIX `mix`.
fn params(t: ReverbType, mix: f32) -> ReverbParams {
    ReverbParams {
        mix,
        ..ReverbParams::default()
    }
    .with_type(t, &KnobMemory::new())
}

/// One block through `rv` on `p`.
fn tick(rv: &mut Reverb, send: &Blk, p: &ReverbParams) -> (Blk, Blk) {
    let mut out = Stereo::SILENT;
    rv.process(send, p, p.mix, Tempo::DEFAULT, SR, &mut out);
    (out.l, out.r)
}

/// A fresh `p.kind` through the bus's reverb, its knobs read from `p`.
fn bus<'a>(
    rv: &'a RefCell<Box<Reverb>>,
    p: &'a RefCell<ReverbParams>,
) -> impl FnMut(&Blk) -> (Blk, Blk) + 'a {
    move |send| tick(&mut rv.borrow_mut(), send, &p.borrow())
}

fn fresh(p: &ReverbParams) -> RefCell<Box<Reverb>> {
    RefCell::new(Box::new(Reverb::of_type(p.kind)))
}

#[test]
fn tap_table_is_seeded_and_bounded() {
    let t = tap_table::<GATE_TAPS>(TAP_SEED);
    assert_eq!(t, tap_table(TAP_SEED));
    assert_eq!((t[0].0, t[11].0), (0.0, 0.0));
    assert!(
        t.iter()
            .all(|&(j, s)| j.abs() <= 0.3 && (s == 1 || s == -1))
    );
    assert_ne!(t, tap_table::<GATE_TAPS>(TAP_SEED ^ 1), "the seed matters");
    // Not a regular spacing: the jitter breaks the comb.
    assert!(t[1..11].iter().filter(|&&(j, _)| j.abs() > 0.05).count() >= 5);
}

/// `t`'s tap weights (GATE's 12 then zeros) and hit's at SHAPE (or
/// CURVE) `shape` and DENSITY `density`.
fn curve_hit(t: ReverbType, shape: f32, density: f32) -> ([f32; REVERSE_TAPS], f32) {
    let mut p = params(t, 0.0);
    (p.shape, p.diffuse) = (shape, density);
    tap_weights(&knobs_120(&p))
}

fn curve(t: ReverbType, shape: f32, density: f32) -> [f32; REVERSE_TAPS] {
    curve_hit(t, shape, density).0
}

#[test]
fn weights_keep_sum_of_squares() {
    let grid = [0.0, 0.25, 0.5, 0.75, 1.0];
    for t in BURSTS {
        for shape in grid {
            for density in grid {
                let w = curve(t, shape, density);
                let db = 10.0 * w.iter().map(|x| x * x).sum::<f32>().log10();
                assert!(db.abs() <= 0.1, "{t:?} {shape} {density}: {db} dB");
            }
        }
    }
}

/// GATE's alone: REVERSE's DENSITY is its two allpasses' g (ADR 0095).
#[test]
fn density_fades_taps_continuously() {
    for t in [ReverbType::Gate] {
        for shape in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mut was = curve(t, shape, 0.0);
            for i in 1..=128 {
                let now = curve(t, shape, i as f32 / 128.0);
                for k in 0..GATE_TAPS {
                    let d = (now[k] - was[k]).abs();
                    assert!(d <= 1.0 / 64.0, "{t:?} SHAPE {shape} step {i} tap {k}: {d}");
                }
                was = now;
            }
            // 6 taps at DENSITY 0, 12 at 1.
            let live = |w: [f32; REVERSE_TAPS]| w.iter().filter(|x| x.abs() > 1e-6).count();
            assert_eq!(live(curve(t, shape, 0.0)), 6);
            assert_eq!(live(curve(t, shape, 1.0)), 12);
        }
    }
}

#[test]
fn scheduler_moves_at_most_four_taps_by_120() {
    let mut p = params(ReverbType::Gate, 1.0);
    p.size = 0.0;
    let k0 = knobs_120(&p);
    assert!((k0.length_ms - 40.0).abs() < 0.1);
    let mut e = BurstEngine::default();
    e.prime(&k0);
    let mut arena = Box::new(ReverbArena::new());
    p.size = 1.0;
    let k1 = knobs_120(&p);
    let goal = tap_targets(&k1);
    let (u, mut l, mut r) = ([0.0f32; HALF], [0.0; HALF], [0.0; HALF]);
    let mut windows = 0;
    let mut landed = None;
    for b in 0..750 {
        e.run(&mut arena, &u, &k1, &mut l, &mut r);
        let moving: Vec<_> = e.crossfades().collect();
        assert!(moving.len() <= MOVERS, "block {b}: {} taps", moving.len());
        for &(tap, from, to) in &moving {
            assert!(from.abs_diff(to) <= STEP, "tap {tap}: {from} → {to}");
        }
        windows += usize::from(b % 4 == 0 && !moving.is_empty());
        if landed.is_none() && e.taps_at() == goal {
            landed = Some(b);
        }
    }
    let b = landed.expect("every tap at its target within 1 s");
    println!(
        "{windows} crossfades; landed in {} ms",
        (b + 1) * HALF * 1000 / 24_000
    );
    assert!(windows > 0);
}

/// A 20 ms noise burst into `t` on `p`, 1.5 s: (L, R).
fn noise_burst(p: &ReverbParams) -> (Vec<f32>, Vec<f32>) {
    let (rv, pc) = (fresh(p), RefCell::new(*p));
    let mut n = Noise(0x5eed);
    run(
        &mut bus(&rv, &pc),
        &mut |i| if i < 20 * MS { 0.5 * n.next() } else { 0.0 },
        1.5,
    )
}

#[test]
fn gate_window() {
    let mut p = params(ReverbType::Gate, 1.0);
    (p.size, p.time, p.shape, p.predelay) = (0.55, 0.0, 0.0, 0.0);
    assert!((knobs_120(&p).length_ms - 177.0).abs() < 1.0);
    let (l, r) = noise_burst(&p);
    let x: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
    let end = end_20db_ms(&x);
    println!("GATE end (−20 dB) at {end} ms");
    assert!((185.0..=660.0).contains(&end), "end {end} ms");
    let at = |ms: f32| (ms * MS as f32) as usize;
    let plateau = rms_db(&x[at(20.0)..at(end - 20.0)]);
    let after = rms_db(&x[at(end)..at(end + 50.0)]);
    println!("plateau {plateau:.1} dB, the 50 ms after {after:.1} dB");
    assert!(after <= plateau - 20.0, "no cliff: {after} vs {plateau}");
}

#[test]
fn reverse_window() {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.size, p.time) = (0.5, 0.0);
    assert!((knobs_120(&p).length_ms - 346.0).abs() < 1.0);
    let (l, r) = noise_burst(&p);
    let x: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
    let peak = peak_ms(&x);
    let rho = corr(&l, &r);
    println!("REVERSE peak {peak} ms, |ρ| {:.2}", rho.abs());
    assert!((peak / 346.0 - 1.0).abs() <= 0.15, "peak {peak} ms");
    // The hit is centred, as the unit's: the sides correlate more than the
    // spike's 0.4–0.65, but stay two.
    assert!((0.3..=0.95).contains(&rho.abs()), "ρ {rho}");
}

/// Whether `rv` still moves a tap toward `p`'s.
fn moving(rv: &Reverb, p: &ReverbParams) -> bool {
    let e = rv.burst().expect("BURST runs");
    e.crossfades().next().is_some() || e.taps_at() != tap_targets(&knobs_120(p))
}

#[test]
fn regen_settles_on_its_input_pitch() {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.time, p.size) = (1.0, 0.0);
    let k0 = knobs_120(&p);
    assert!((k0.length_ms - 150.0).abs() < 0.5 && (k0.regen - 0.8).abs() < 1e-4);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let start = blocks(0.25);
    let mut step = |b: usize| {
        if b == start {
            pc.borrow_mut().size = 1.0;
        }
        b >= start && moving(&rv.borrow(), &pc.borrow())
    };
    check_settles_on_pitch(&mut bus(&rv, &pc), &mut step, 0.8);
}

/// REVERSE at REGEN .7: noise for 2 s, LENGTH from `from` to .9 at
/// 0.5 s, then the tail from 2 s to 6 s.
fn regen_tail(from: f32) -> Vec<f32> {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.time, p.size) = (0.7, from);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let mut n = Noise(0xface);
    let mut render = bus(&rv, &pc);
    let mut out = Vec::new();
    for b in 0..blocks(6.0) {
        if b == blocks(0.5) {
            pc.borrow_mut().size = 0.9;
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
    out.split_off(2 * SR as usize)
}

#[test]
fn no_slap_after_a_length_move() {
    check_no_slap(&regen_tail(0.2), &regen_tail(0.9));
}

#[test]
fn regen_1_falls_to_exact_zero() {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.time, p.size) = (1.0, 1.0);
    // −1.9 dB a pass: 25 s at 800 ms.
    assert!((knobs_120(&p).rt60_s - 24.8).abs() < 0.5);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let mut n = Noise(0xbeef);
    let (l, r) = run(
        &mut bus(&rv, &pc),
        &mut |i| {
            if i < SR as usize / 2 {
                0.5 * n.next()
            } else {
                0.0
            }
        },
        34.0,
    );
    let last = blocks(1.0) * BLOCK_SIZE;
    assert!(
        l[l.len() - last..]
            .iter()
            .chain(&r[r.len() - last..])
            .all(|&s| s == 0.0),
        "still ringing after 34 s"
    );
    // No ±Δ hum nor DC on the way: every i16 line is zero too. (The f32
    // allpass states may rest on a subnormal, which the return's gain
    // takes to exact 0.)
    assert!(rv.borrow().arena().lines()[..SLOTS].iter().all(|&s| s == 0));
}

/// BURST alone at the ring's rate, behind the bus's half-bands: the
/// i16 engine, or its unclipped or f32-line references.
struct Alone {
    e: BurstEngine,
    arena: Box<ReverbArena>,
    f32_lines: Option<Vec<f32>>,
    clipped: bool,
    dec: Decimator,
    up: [Interpolator; 2],
}

impl Alone {
    fn new(f32_lines: bool, clipped: bool) -> Self {
        Self {
            e: BurstEngine::default(),
            arena: Box::new(ReverbArena::new()),
            f32_lines: f32_lines.then(|| vec![0.0; SLOTS]),
            clipped,
            dec: Decimator::new(),
            up: [Interpolator::new(), Interpolator::new()],
        }
    }

    fn block(&mut self, send: &Blk, p: &ReverbParams) -> (Blk, Blk) {
        let (mut u, mut l, mut r) = ([0.0; HALF], [0.0; HALF], [0.0; HALF]);
        self.dec.process(send, &mut u);
        let k = knobs_120(p);
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
}

#[test]
fn level_plan_holds_for_burst() {
    for t in BURSTS {
        let mut p = params(t, 1.0);
        p.size = 0.0;
        let pc = RefCell::new(p);
        let mut a = Alone::new(false, true);
        let mut b = Alone::new(false, false);
        let mut c = Alone::new(true, false);
        let mode = 1_000.0 / knobs_120(&p).length_ms;
        let mut ra = |s: &Blk| a.block(s, &pc.borrow());
        let mut rb = |s: &Blk| b.block(s, &pc.borrow());
        let mut rc = |s: &Blk| c.block(s, &pc.borrow());
        println!("{t:?}");
        check_level_plan(
            &mut ra,
            &mut || pc.borrow_mut().time = 1.0,
            &mut rb,
            &mut rc,
            mode,
        );
    }
}

#[test]
fn burst_knobs_ease() {
    for t in BURSTS {
        let p = params(t, 1.0);
        let (rv, pc) = (fresh(&p), RefCell::new(p));
        let ids = [1, 2, 3, 7, 8, 9, 10, 11, 12].map(ParamId);
        check_knobs_ease(&mut bus(&rv, &pc), &ids, &mut |id, v| {
            pc.borrow_mut().write(id, v)
        });
    }
}

#[test]
fn burst_noise_and_loudness() {
    for t in BURSTS {
        let (p, q) = (params(t, 0.5), params(ReverbType::Quad, 0.5));
        let (rv, pc) = (fresh(&p), RefCell::new(p));
        let (qv, qc) = (fresh(&q), RefCell::new(q));
        println!("{t:?}");
        check_sustained_loudness(&mut bus(&rv, &pc), &mut bus(&qv, &qc));
        check_noise_floor(&mut bus(&rv, &pc), &mut bus(&qv, &qc), 4.0);
    }
}

#[test]
fn tail_shape_moves_with_every_knob() {
    use chimera_core::ui::reverb_panel::{RevPage, knob};
    for t in BURSTS {
        let mut base = params(t, 0.5);
        for id in [1, 2, 3, 7, 8, 9, 10, 11, 12] {
            base.write(ParamId(id), 0.5);
        }
        let shape = tail_shape(t, &knobs_120(&base));
        let cells = (0..4)
            .filter_map(|k| knob(t, RevPage::Main, k))
            .chain((0..6).filter_map(|k| knob(t, RevPage::More, k)))
            // SYNC is a word choice, not a knob.
            .filter(|c| c.id != ReverbParams::SYNC);
        let mut n = 0;
        for c in cells {
            let mut p = base;
            p.write(c.id, 0.9);
            assert_ne!(tail_shape(t, &knobs_120(&p)), shape, "{t:?} {}", c.label);
            n += 1;
        }
        // GATE's PRE-DLY and DAMP, REVERSE's ONSET.
        let want = if t == ReverbType::Gate { 9 } else { 8 };
        assert_eq!(n, want, "{t:?}: its knobs");
    }
}

/// `p`'s click response, mono, `seconds` long.
fn click(p: &ReverbParams, seconds: f32) -> Vec<f32> {
    let (rv, pc) = (fresh(p), RefCell::new(*p));
    let (l, r) = run(
        &mut bus(&rv, &pc),
        &mut |i| if i == 0 { 0.5 } else { 0.0 },
        seconds,
    );
    l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect()
}

/// The first sample within `db` of `x`'s peak magnitude, ms.
fn first_ms(x: &[f32], db: f32) -> f32 {
    let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let thr = pk * 10f32.powf(db / 20.0);
    x.iter().position(|v| v.abs() >= thr).expect("silent") as f32 / MS as f32
}

/// The bus's half-bands and the line's floor: 1.2 ms ahead of every tap.
const BUS_MS: f32 = 1.2;

#[test]
fn gate_predelay_reaches_71_ms() {
    let mut p = params(ReverbType::Gate, 1.0);
    (p.size, p.time, p.shape, p.diffuse) = (0.3, 0.0, 0.0, 0.0);
    p.predelay = 71.0 / 80.0;
    let at = first_ms(&click(&p, 0.5), -20.0);
    println!("PRE-DLY 71 ms: first arrival {at:.2} ms");
    assert!((at - 71.0 - BUS_MS).abs() <= 1.0, "{at} ms");
    p.predelay = 0.0;
    assert!(first_ms(&click(&p, 0.5), -20.0) <= 2.0 * BUS_MS);
}

#[test]
fn reverse_is_silent_before_its_onset() {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.size, p.time, p.shape, p.diffuse) = (0.6, 0.0, 0.6, 0.5);
    p.predelay = 60.0 / 150.0;
    let x = click(&p, 1.0);
    let at = first_ms(&x, -60.0);
    println!("ONSET 60 ms: first −60 dB arrival {at:.1} ms");
    assert!(at >= 55.0, "{at} ms");
}

#[test]
fn reverse_ends_on_its_hit() {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.size, p.time, p.shape, p.diffuse) = (0.6, 0.0, 0.6, 0.5);
    let len = knobs_120(&p).length_ms;
    let x = click(&p, 1.0);
    let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let i = x.iter().position(|v| v.abs() == pk).unwrap();
    let at = i as f32 / MS as f32;
    println!("LENGTH {len:.1} ms: peak at {at:.1} ms");
    assert!(
        (at - len - BUS_MS).abs() <= 1.0,
        "peak {at} ms, LENGTH {len}"
    );
    // Then −20 dB within 20 ms: the 2 ms windows after it.
    let w = |a: usize| x[a..a + 2 * MS].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let after = w(i + 18 * MS);
    assert!(
        after <= 0.1 * pk,
        "{} dB 18 ms on",
        20.0 * (after / pk).log10()
    );
    // The hit stands over the ramp: 9 dB over the top tap.
    let (wt, hit) = curve_hit(ReverbType::Reverse, 0.6, 0.5);
    let top = wt[REVERSE_TAPS - 1].abs();
    assert!((20.0 * (hit / top).log10() - 9.0).abs() < 0.01);
}

#[test]
fn curve_reaches_55_db() {
    let w = curve(ReverbType::Reverse, 1.0, 0.5);
    let span = 20.0 * (w[REVERSE_TAPS - 1].abs() / w[0].abs()).log10();
    assert!((span - 55.0).abs() < 0.05, "{span} dB");
    // Along (1 − x)^1.6: half way is 55 × 0.5^1.6 = 18.1 dB under the top.
    let mid = 20.0 * (w[REVERSE_TAPS - 1].abs() / w[(REVERSE_TAPS - 1) / 2].abs()).log10();
    let x = ((REVERSE_TAPS - 1) / 2) as f32 / (REVERSE_TAPS - 1) as f32;
    assert!((mid - 55.0 * (1.0 - x).powf(1.6)).abs() < 0.05, "{mid} dB");
}

/// The TAIL's energy above 3 kHz against below, dB, over 300–900 ms.
fn tail_tilt(damp: f32) -> f32 {
    let mut p = params(ReverbType::Gate, 1.0);
    (p.size, p.time, p.damping) = (0.2, 1.0, damp);
    let x = click(&p, 1.0);
    let seg = &x[300 * MS..900 * MS];
    let hi: f32 = seg.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
    let all: f32 = seg.iter().map(|v| v * v).sum();
    10.0 * (hi / all).log10()
}

#[test]
fn gate_damp_darkens_its_tail() {
    let (open, dark) = (tail_tilt(0.0), tail_tilt(1.0));
    println!("TAIL's HF share: DAMP 0 {open:.1} dB, DAMP 1 {dark:.1} dB");
    assert!(dark <= open - 6.0, "{open} → {dark}");
}

/// The click's sample magnitudes over a fast gate's span above −40 dB.
fn busy(density: f32) -> usize {
    let mut p = params(ReverbType::Gate, 1.0);
    (p.size, p.time, p.shape, p.predelay, p.diffuse) = (0.4, 0.0, 0.0, 0.0, density);
    let x = click(&p, 0.3);
    let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    x[..60 * MS].iter().filter(|v| v.abs() >= 0.01 * pk).count()
}

#[test]
fn gate_density_low_end_is_dry() {
    // DENSITY 0: twelve taps, undiffused, a few samples each through the
    // decorrelators; DENSITY 1: the five allpasses fill the gaps.
    let (dry, dense) = (busy(0.0), busy(1.0));
    println!("samples over −40 dB in 60 ms: {dry} dry, {dense} dense");
    assert!(dense >= 2 * dry, "{dry} → {dense}");
}

#[test]
fn regen_is_flat() {
    // Each pass of the hit at the regen's gain, with no one-pole: the
    // second hit's peak over the first's is REGEN's g.
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.size, p.time, p.shape, p.predelay) = (0.5, 0.6, 1.0, 0.0);
    let k = knobs_120(&p);
    let x = click(&p, 1.0);
    let at = |ms: f32| ((ms + BUS_MS) * MS as f32) as usize;
    let pk = |c: f32| {
        x[at(c) - MS..at(c) + MS]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
    };
    let ratio = pk(2.0 * k.length_ms) / pk(k.length_ms);
    println!("second hit / first {ratio:.3}, REGEN {:.3}", k.regen);
    assert!(
        (ratio / k.regen - 1.0).abs() < 0.08,
        "{ratio} vs {}",
        k.regen
    );
}

#[test]
fn taps_land_on_their_samples() {
    use chimera_core::dsp::rev::burst::HIT;
    // GATE's last tap at PRE-DLY + LENGTH, REVERSE's hit at LENGTH, to the
    // nearest ring sample (24 a ms).
    let mut p = params(ReverbType::Gate, 1.0);
    (p.size, p.predelay) = (0.5, 0.5);
    let k = knobs_120(&p);
    let t = tap_targets(&k);
    let want = ((k.predelay_ms + k.length_ms) * 24.0 + 0.5) as u16;
    assert_eq!(t[GATE_TAPS - 1], want);
    assert_eq!(t[0], (k.predelay_ms * 24.0 + 0.5) as u16);
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.size, p.predelay) = (0.5, 0.4);
    let k = knobs_120(&p);
    let t = tap_targets(&k);
    assert_eq!(t[HIT], (k.length_ms * 24.0 + 0.5) as u16);
    assert_eq!(t[0], (k.predelay_ms * 24.0 + 0.5) as u16);
    // The ramp's last tap 2.7 ms before the hit.
    assert_eq!(
        t[REVERSE_TAPS - 1],
        ((k.length_ms - 2.7) * 24.0 + 0.5) as u16
    );
}
