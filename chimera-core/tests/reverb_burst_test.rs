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
    BurstEngine, MOVERS, SLOTS, STEP, TAP_SEED, tap_table, tap_targets, tap_weights,
};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType, knobs};
use chimera_core::ui::glyph::tail_shape;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{
    MS, Noise, blocks, check_knobs_ease, check_level_plan, check_no_slap, check_noise_floor,
    check_settles_on_pitch, check_sustained_loudness, corr, end_20db_ms, peak_ms, rms_db, run,
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
    let t = tap_table(TAP_SEED);
    assert_eq!(t, tap_table(TAP_SEED));
    assert_eq!((t[0].0, t[11].0), (0.0, 0.0));
    assert!(
        t.iter()
            .all(|&(j, s)| j.abs() <= 0.3 && (s == 1 || s == -1))
    );
    assert_ne!(t, tap_table(TAP_SEED ^ 1), "the seed matters");
    // Not a regular spacing: the jitter breaks the comb.
    assert!(t[1..11].iter().filter(|&&(j, _)| j.abs() > 0.05).count() >= 5);
}

/// `t`'s knobs at SHAPE (or CURVE) `shape` and DENSITY `density`.
fn curve(t: ReverbType, shape: f32, density: f32) -> [f32; 12] {
    let mut p = params(t, 0.0);
    (p.shape, p.diffuse) = (shape, density);
    tap_weights(&knobs(&p))
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

#[test]
fn density_fades_taps_continuously() {
    for t in BURSTS {
        for shape in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mut was = curve(t, shape, 0.0);
            for i in 1..=128 {
                let now = curve(t, shape, i as f32 / 128.0);
                for k in 0..12 {
                    let d = (now[k] - was[k]).abs();
                    assert!(d <= 1.0 / 64.0, "{t:?} SHAPE {shape} step {i} tap {k}: {d}");
                }
                was = now;
            }
            // 6 taps at DENSITY 0, 12 at 1.
            let live = |w: [f32; 12]| w.iter().filter(|x| x.abs() > 1e-6).count();
            assert_eq!(live(curve(t, shape, 0.0)), 6);
            assert_eq!(live(curve(t, shape, 1.0)), 12);
        }
    }
}

#[test]
fn scheduler_moves_at_most_four_taps_by_120() {
    let mut p = params(ReverbType::Gate, 1.0);
    p.size = 0.0;
    let k0 = knobs(&p);
    assert!((k0.length_ms - 40.0).abs() < 0.1);
    let mut e = BurstEngine::default();
    e.prime(&k0);
    let mut arena = Box::new(ReverbArena::new());
    p.size = 1.0;
    let k1 = knobs(&p);
    let goal = tap_targets(k1.length_ms);
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
    (p.size, p.time, p.shape) = (0.55, 0.0, 0.0);
    assert!((knobs(&p).length_ms - 177.0).abs() < 1.0);
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
    let p = params(ReverbType::Reverse, 1.0);
    assert!((knobs(&p).length_ms - 346.0).abs() < 1.0);
    let (l, r) = noise_burst(&p);
    let x: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
    let peak = peak_ms(&x);
    let rho = corr(&l, &r);
    println!("REVERSE peak {peak} ms, |ρ| {:.2}", rho.abs());
    assert!((peak / 346.0 - 1.0).abs() <= 0.15, "peak {peak} ms");
    assert!((0.3..=0.75).contains(&rho.abs()), "ρ {rho}");
}

/// Whether `rv` still moves a tap toward `length_ms`.
fn moving(rv: &Reverb, length_ms: f32) -> bool {
    let e = rv.burst().expect("BURST runs");
    e.crossfades().next().is_some() || e.taps_at() != tap_targets(length_ms)
}

#[test]
fn regen_settles_on_its_input_pitch() {
    let mut p = params(ReverbType::Reverse, 1.0);
    (p.time, p.size) = (1.0, 0.0);
    let k0 = knobs(&p);
    assert!((k0.length_ms - 150.0).abs() < 0.5 && (k0.regen - 0.708).abs() < 1e-4);
    let (rv, pc) = (fresh(&p), RefCell::new(p));
    let start = blocks(0.25);
    let mut step = |b: usize| {
        if b == start {
            pc.borrow_mut().size = 1.0;
        }
        b >= start && moving(&rv.borrow(), 800.0)
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
    assert!((knobs(&p).rt60_s - 16.0).abs() < 1.0);
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
        24.0,
    );
    let last = blocks(1.0) * BLOCK_SIZE;
    assert!(
        l[l.len() - last..]
            .iter()
            .chain(&r[r.len() - last..])
            .all(|&s| s == 0.0),
        "still ringing after 24 s"
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
        let mode = 1_000.0 / knobs(&p).length_ms;
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
        let ids = [1, 3, 7, 8, 10, 11, 12].map(ParamId);
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
        for id in [1, 3, 7, 8, 10, 11, 12] {
            base.write(ParamId(id), 0.5);
        }
        let shape = tail_shape(t, &knobs(&base));
        let cells = (0..4)
            .filter_map(|k| knob(t, RevPage::Main, k))
            .chain((0..6).filter_map(|k| knob(t, RevPage::More, k)));
        let mut n = 0;
        for c in cells {
            let mut p = base;
            p.write(c.id, 0.9);
            assert_ne!(tail_shape(t, &knobs(&p)), shape, "{t:?} {}", c.label);
            n += 1;
        }
        assert_eq!(n, 7, "{t:?}: its seven knobs");
    }
}
