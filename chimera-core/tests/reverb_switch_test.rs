// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Switching TYPE (reverb TYPEs spec § Switching TYPE): the old tail
//! fades out on its latched knobs, four blocks clear the arena a quarter
//! each, and the new TYPE fades in as a fresh engine. Never a click, never
//! a stale tail.

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::ease::{at, step_of};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::rev::{CLEAR_BLOCKS, FADE_BLOCKS, Left, Reverb, SwitchPhase};
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType};
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{Noise, max_d2};

/// Every TYPE: each has its engine.
const TYPES: &[ReverbType] = &ReverbType::ALL;

/// The switch's blocks: FADE OUT, CLEAR, FADE IN.
const SWITCH: usize = 2 * FADE_BLOCKS as usize + CLEAR_BLOCKS as usize;

fn fade_out(left: u8) -> SwitchPhase {
    SwitchPhase::FadeOut {
        left: Left::new(left).expect("1..=15"),
    }
}

fn clear(left: u8) -> SwitchPhase {
    SwitchPhase::Clear {
        left: Left::new(left).expect("1..=4"),
    }
}

fn fade_in(left: u8) -> SwitchPhase {
    SwitchPhase::FadeIn {
        left: Left::new(left).expect("1..=15"),
    }
}
/// The tail's own window, before the switch.
const BEFORE: usize = 30;
/// The tone burst's blocks, and the switch's first block: mid-tail, its
/// `BEFORE` window all tail.
const BURST: usize = 20;
/// Blocks ROOM's and HALL's bursts come earlier. ROOM on its starting
/// knobs (MidiVerb II program 5's) settles about 0.1 s past its burst: at
/// 0 its switch window reads 1.3× rougher than the window before it, at
/// 200 the gain-jump control is lost in it; 70 passes both. HALL on its
/// starting knobs (program 25's, ADR 0095 § HALL, as built) passes from
/// 100 to 300; at 400 and 1000 the gain jump is lost in its tail, and at
/// 800 the tail alone reads rougher than the window before: 200.
/// BLOOM's tail roughens each time its loop comes round (about 0.65 s at
/// its starting SWELL): at most leads the gain jump is lost in it, or the
/// tail alone reads rougher than the window before. At its START's SMEAR 0
/// the control's margin (the jump's roughness over the bound, which must
/// pass 1.5) swings with the lead, 10 blocks at a time: 1.85 at 350, the
/// widest of 200–900 bar an isolated 1.88 at 540; 1.61 at 340, 1.18 at 360.
fn lead(t: ReverbType) -> usize {
    match t {
        ReverbType::Room => 70,
        ReverbType::Hall => 200,
        ReverbType::Bloom => 350,
        _ => 0,
    }
}
const AT: usize = BURST + 5 + BEFORE;

type Block = [f32; BLOCK_SIZE];

/// A 330 Hz burst under a Hann window, then silence: a smooth tail.
fn burst(b: usize) -> Block {
    core::array::from_fn(|i| {
        let n = b * BLOCK_SIZE + i;
        let len = BURST * BLOCK_SIZE;
        if n >= len {
            return 0.0;
        }
        let t = n as f32 / SR as f32;
        let w = 0.5 - 0.5 * libm::cosf(core::f32::consts::TAU * n as f32 / len as f32);
        0.5 * w * libm::sinf(core::f32::consts::TAU * 330.0 * t)
    })
}

/// A long, dark tail at MIX 1 on `t`'s starting knobs: smooth, so a cut
/// in it stands out.
fn params(t: ReverbType) -> ReverbParams {
    let p = ReverbParams {
        mix: 1.0,
        ..Default::default()
    };
    let mut p = p.with_type(t, &KnobMemory::new());
    // DAMP darkens QUAD's loop and GATE's TAIL, TONE a BURST's input; a
    // BURST at its shortest LENGTH is in its TAIL or REGEN loop by the
    // switch, DENSITY at 0.7 its diffuser whole (GATE's fast-gate start,
    // 0.28, leaves a tail too rough for the gain-jump control to show), and
    // PRE-DLY (REVERSE's ONSET) 0, so the window before the switch hears it.
    (p.time, p.damping, p.tone) = (1.0, 0.9, 1.0);
    if matches!(t, ReverbType::Gate | ReverbType::Reverse) {
        (p.size, p.diffuse, p.predelay) = (0.0, 0.7, 0.0);
    }
    // REVERSE's ramp flat: CURVE's rise would put the window before the
    // switch under the louder end it switches into.
    if t == ReverbType::Reverse {
        p.shape = 0.0;
    }
    p
}

fn tick(rv: &mut Reverb, send: &Block, p: &ReverbParams) -> Stereo {
    let mut out = Stereo::SILENT;
    rv.process(send, p, p.mix, Tempo::DEFAULT, SR, &mut out);
    out
}

/// Start a switch from `a`'s `p` to `b`: a TYPE change, or a forced
/// restart when they match. The params to play from now on.
fn switch(rv: &mut Reverb, p: &ReverbParams, b: ReverbType) -> ReverbParams {
    if p.kind == b {
        rv.force_restart();
        *p
    } else {
        p.with_type(b, &KnobMemory::new())
    }
}

/// Each side's samples over `blocks`.
#[derive(Default)]
struct Sides([Vec<f32>; 2]);

impl Sides {
    fn push(&mut self, o: &Stereo) {
        self.0[0].extend_from_slice(&o.l);
        self.0[1].extend_from_slice(&o.r);
    }

    /// Block range `r` of `side`, two samples early so the first block's
    /// edge is judged.
    fn at(&self, side: usize, r: core::ops::Range<usize>) -> &[f32] {
        let s = (r.start * BLOCK_SIZE).saturating_sub(2);
        &self.0[side][s..r.end * BLOCK_SIZE]
    }
}

/// Each block's roughness over `w` (two samples early, as `Sides::at`
/// gives): its largest second difference over the loudest sample of it and
/// its neighbours, floored at `floor`. A tail that only decays or ramps
/// keeps it; a step in it, to 0 or by one ramp step, raises it.
fn roughness(w: &[f32], floor: f32) -> f32 {
    let blocks = (w.len() - 2) / BLOCK_SIZE;
    (0..blocks)
        .map(|b| {
            let d2 = max_d2(&w[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE + 2]);
            let near = &w[b.saturating_sub(1) * BLOCK_SIZE..((b + 2).min(blocks)) * BLOCK_SIZE + 2];
            d2 / near.iter().fold(floor, |m, x| m.max(x.abs()))
        })
        .fold(0.0, f32::max)
}

/// The click bound: across `window` (blocks from `AT`), each side's
/// roughness stays within 1.25 × the `BEFORE` blocks' before `AT`, under a
/// floor 60 dB below their peak. And it has teeth: the tail cut to 0 at
/// `AT` passes 2 ×, and a one-ramp-step gain jump mid-FADE OUT 1.5 ×
/// (`gain_jump_breaks_it`).
fn no_click(out: &Sides, window: usize, what: &str) {
    for side in 0..2 {
        let (bound, floor) = bound(out, side);
        let w = out.at(side, AT..AT + window);
        let got = roughness(w, floor);
        assert!(
            got <= 1.25 * bound,
            "{what}, side {side}: roughness {got} over {bound}"
        );
        let mut cut = out.at(side, AT - 1..AT + 1).to_vec();
        cut[BLOCK_SIZE + 2..].fill(0.0);
        let heard = roughness(&cut, floor);
        assert!(
            heard > 2.0 * bound,
            "{what}, side {side}: a cut ({heard}) is within {bound}"
        );
        if window > usize::from(FADE_BLOCKS) {
            let heard = roughness(&gain_jump(w), floor);
            assert!(
                heard > 1.5 * bound,
                "{what}, side {side}: a gain jump ({heard}) is within {bound}"
            );
        }
    }
}

/// `side`'s roughness over the `BEFORE` blocks, and the floor: their peak
/// less 60 dB.
fn bound(out: &Sides, side: usize) -> (f32, f32) {
    let before = out.at(side, AT - BEFORE..AT);
    let floor = 1e-3 * before.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    (roughness(before, floor), floor)
}

/// `w` with a ramp step skipped mid-FADE OUT: from block `FADE_BLOCKS / 2`'s
/// loudest sample on, × (1 − 1/15).
fn gain_jump(w: &[f32]) -> Vec<f32> {
    let mut w = w.to_vec();
    let b = usize::from(FADE_BLOCKS / 2) * BLOCK_SIZE + 2;
    let k = (b..b + BLOCK_SIZE)
        .max_by(|&i, &j| w[i].abs().total_cmp(&w[j].abs()))
        .expect("a block");
    let step = 1.0 - 1.0 / f32::from(FADE_BLOCKS);
    w[k..].iter_mut().for_each(|s| *s *= step);
    w
}

/// A gain jump the size of one ramp step mid-FADE OUT, as a skipped or
/// doubled block would make, breaks the bound the switch itself keeps:
/// `no_click` hears more than cuts to 0.
#[test]
fn gain_jump_breaks_it() {
    let (out, _) = render(ReverbType::Quad, ReverbType::Quad, SWITCH + 10);
    for side in 0..2 {
        let (bound, floor) = bound(&out, side);
        let w = out.at(side, AT..AT + SWITCH);
        let clean = roughness(w, floor);
        let jumped = roughness(&gain_jump(w), floor);
        assert!(
            clean <= 1.25 * bound,
            "side {side}: the switch itself clicks"
        );
        assert!(jumped > 1.5 * bound, "side {side}: {jumped} within {bound}");
    }
}

/// `a` mid-tail at MIX 1, then the switch to `b` at `AT`, then `after`
/// blocks: every side's output, and the reverb.
fn render(a: ReverbType, b: ReverbType, after: usize) -> (Sides, Box<Reverb>) {
    let mut rv = Box::new(Reverb::of_type(a));
    let mut p = params(a);
    let mut out = Sides::default();
    let lead = lead(a);
    for blk in 0..lead + AT + after {
        if blk == lead + AT {
            p = switch(&mut rv, &p, b);
        }
        let o = tick(&mut rv, &burst(blk), &p);
        if blk >= lead {
            out.push(&o);
        }
    }
    (out, rv)
}

#[test]
fn every_ordered_pair_switches_without_a_click() {
    for &a in TYPES {
        for &b in TYPES {
            let (out, rv) = render(a, b, SWITCH + 10);
            no_click(&out, SWITCH + 10, &format!("{a:?} → {b:?}"));
            assert_eq!((rv.phase(), rv.running()), (SwitchPhase::Running, b));
        }
    }
}

/// The phase after each block from the switch's first.
fn phases_after(rv: &mut Reverb, p: &ReverbParams, blocks: usize) -> Vec<SwitchPhase> {
    (0..blocks)
        .map(|blk| {
            tick(rv, &burst(blk), p);
            rv.phase()
        })
        .collect()
}

/// FADE OUT `from`..=1, CLEAR 4..=1, FADE IN 15..=1 and RUNNING: the
/// phases after each block of a switch whose first block ran FADE OUT
/// `from + 1`.
fn sequence(from: u8) -> Vec<SwitchPhase> {
    let out = (1..=from).rev().map(fade_out);
    let clears = (1..=CLEAR_BLOCKS).rev().map(clear);
    let fade_ins = (1..=FADE_BLOCKS).rev().map(fade_in);
    out.chain(clears)
        .chain(fade_ins)
        .chain([SwitchPhase::Running])
        .collect()
}

#[test]
fn the_phases_run_15_4_15() {
    assert_eq!((FADE_BLOCKS, CLEAR_BLOCKS), (15, 4));
    // A TYPE change: the block that sees it runs FADE OUT 15.
    let mut rv = Box::new(Reverb::new());
    let p = params(ReverbType::Quad);
    phases_after(&mut rv, &p, 50);
    let room = p.with_type(ReverbType::Room, &KnobMemory::new());
    let seen = phases_after(&mut rv, &room, SWITCH);
    assert_eq!(seen, sequence(FADE_BLOCKS - 1));
    assert_eq!(rv.running(), ReverbType::Room);
    // A forced restart starts at FADE OUT 15 before its first block.
    rv.force_restart();
    assert_eq!(rv.phase(), fade_out(FADE_BLOCKS));
    let seen = phases_after(&mut rv, &room, SWITCH);
    assert_eq!(seen, sequence(FADE_BLOCKS - 1));
}

/// Run `rv` on `p` until `phase()` reads `want`.
fn run_to(rv: &mut Reverb, p: &ReverbParams, want: SwitchPhase, blk: &mut usize, out: &mut Sides) {
    let stop = *blk + 4 * SWITCH;
    while rv.phase() != want {
        assert!(*blk < stop, "never reached {want:?}");
        out.push(&tick(rv, &burst(*blk), p));
        *blk += 1;
    }
}

#[test]
fn clear_blocks_return_exact_zero() {
    for &b in TYPES {
        let mut rv = Box::new(Reverb::new());
        let p = params(ReverbType::Quad);
        let (mut blk, mut out) = (0, Sides::default());
        while blk < AT {
            out.push(&tick(&mut rv, &burst(blk), &p));
            blk += 1;
        }
        assert!(rv.arena().lines().iter().any(|&s| s != 0));
        let p = switch(&mut rv, &p, b);
        run_to(&mut rv, &p, clear(CLEAR_BLOCKS), &mut blk, &mut out);
        // The send is loud: CLEAR drops it. Each block zeroes a quarter.
        let loud = [0.9f32; BLOCK_SIZE];
        let mut live = rv.arena().lines().iter().filter(|&&s| s != 0).count();
        for left in (1..=CLEAR_BLOCKS).rev() {
            let o = tick(&mut rv, &loud, &p);
            assert!(
                o.l.iter().chain(&o.r).all(|&s| s == 0.0),
                "{b:?}: CLEAR {left} not silent"
            );
            let now = rv.arena().lines().iter().filter(|&&s| s != 0).count();
            assert!(now < live || now == 0, "{b:?}: CLEAR {left} zeroed nothing");
            live = now;
        }
        assert_eq!(rv.phase(), fade_in(FADE_BLOCKS));
        assert!(
            rv.arena().lines().iter().all(|&s| s == 0),
            "{b:?}: arena not clear"
        );
    }
}

/// FADE IN `left`'s gain at sample `i`, as the switch ramps it.
fn fade_in_gain(left: u8, i: usize) -> f32 {
    let g = |n: u8| f32::from(n) / f32::from(FADE_BLOCKS);
    let (from, to) = (g(FADE_BLOCKS - left), g(FADE_BLOCKS + 1 - left));
    at(from, step_of((from, to), BLOCK_SIZE), i)
}

/// The gain `phase`'s ramp starts from.
fn from_gain(phase: SwitchPhase) -> f32 {
    let g = |n: u8| f32::from(n) / f32::from(FADE_BLOCKS);
    match phase {
        SwitchPhase::FadeOut { left } => g(left.get()),
        SwitchPhase::FadeIn { left } => g(left.done()),
        _ => panic!("{phase:?} has no ramp"),
    }
}

/// FADE OUT `left`'s gain at sample `i`.
fn fade_out_gain(left: u8, i: usize) -> f32 {
    let g = |n: u8| f32::from(n) / f32::from(FADE_BLOCKS);
    let (from, to) = (g(left), g(left - 1));
    at(from, step_of((from, to), BLOCK_SIZE), i)
}

/// Noise, every block: a send that never stops, through the switch.
fn noise_send(n: &mut Noise) -> Block {
    core::array::from_fn(|_| 0.3 * n.next())
}

#[test]
fn fade_in_is_bit_identical_to_a_fresh_engine() {
    for &a in TYPES {
        for &b in TYPES {
            let mut rv = Box::new(Reverb::of_type(a));
            let mut n = Noise(0xC0FF_EE11);
            let mut p = params(a);
            for _ in 0..AT {
                tick(&mut rv, &noise_send(&mut n), &p);
            }
            p = switch(&mut rv, &p, b);
            while rv.phase() != fade_in(FADE_BLOCKS) {
                tick(&mut rv, &noise_send(&mut n), &p);
            }
            let mut fresh = Box::new(Reverb::of_type(b));
            for blk in 0..FADE_BLOCKS as usize + 100 {
                let phase = rv.phase();
                let x = noise_send(&mut n);
                let (got, want) = (tick(&mut rv, &x, &p), tick(&mut fresh, &x, &p));
                for (g, w) in [(&got.l, &want.l), (&got.r, &want.r)] {
                    for i in 0..BLOCK_SIZE {
                        let w = match phase {
                            SwitchPhase::FadeIn { left } => w[i] * fade_in_gain(left.get(), i),
                            _ => w[i],
                        };
                        assert_eq!(
                            g[i].to_bits(),
                            w.to_bits(),
                            "{a:?} → {b:?}: block {blk} sample {i} ({phase:?})"
                        );
                    }
                }
            }
            assert!(fresh.arena().lines().iter().any(|&s| s != 0));
        }
    }
}

#[test]
fn fade_out_plays_the_latched_knobs() {
    let old = ReverbParams {
        time: 0.85,
        damping: 0.6,
        size: 0.8,
        mix: 1.0,
        ..Default::default()
    };
    let new = old.with_type(ReverbType::Room, &KnobMemory::new());
    assert!(new.time != old.time && new.damping != old.damping && new.size != old.size);
    // Switched, its old self running on, and the old engine on the new knobs.
    let (mut sw, mut stay, mut wrong) = (
        Box::new(Reverb::new()),
        Box::new(Reverb::new()),
        Box::new(Reverb::new()),
    );
    let mut n = Noise(0x5EED_1234);
    for _ in 0..AT {
        let x = noise_send(&mut n);
        for rv in [&mut sw, &mut stay, &mut wrong] {
            tick(rv, &x, &old);
        }
    }
    let mut differs = false;
    for left in (1..=FADE_BLOCKS).rev() {
        let x = noise_send(&mut n);
        let got = tick(&mut sw, &x, &new);
        let ran = tick(&mut stay, &x, &old);
        let other = tick(&mut wrong, &x, &new);
        for (g, (r, o)) in [(&got.l, (&ran.l, &other.l)), (&got.r, (&ran.r, &other.r))] {
            for i in 0..BLOCK_SIZE {
                let gain = fade_out_gain(left, i);
                assert_eq!(
                    g[i].to_bits(),
                    (r[i] * gain).to_bits(),
                    "FADE OUT {left} sample {i}"
                );
                differs |= g[i] != o[i] * gain;
            }
        }
    }
    assert!(
        differs,
        "the new knobs sound the same: the test would not tell"
    );
    assert_eq!(sw.phase(), clear(CLEAR_BLOCKS));
}

#[test]
fn a_switch_during_fade_in_fades_from_the_current_gain() {
    let mut rv = Box::new(Reverb::new());
    let p = params(ReverbType::Quad);
    let (mut blk, mut out) = (0, Sides::default());
    while blk < AT {
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
    }
    rv.force_restart();
    let five = fade_in(5);
    run_to(&mut rv, &p, five, &mut blk, &mut out);
    let reached = from_gain(five);
    assert!((reached - 10.0 / 15.0).abs() <= 1e-6, "{reached}");
    // A TYPE change in this block: it runs FADE OUT from that gain.
    let room = p.with_type(ReverbType::Room, &KnobMemory::new());
    out.push(&tick(&mut rv, &burst(blk), &room));
    assert_eq!(rv.phase(), fade_out(9));
    let first = from_gain(fade_out(10));
    assert!((first - reached).abs() <= 1e-6, "{first} from {reached}");
    // And a forced one, from FADE IN 5, starts at the same place.
    let mut rv = Box::new(Reverb::new());
    rv.force_restart();
    let mut blk = 0;
    run_to(&mut rv, &p, five, &mut blk, &mut out);
    rv.force_restart();
    assert_eq!(rv.phase(), fade_out(10));
    // From FADE IN 15 (no gain yet), straight to CLEAR.
    let mut rv = Box::new(Reverb::new());
    rv.force_restart();
    run_to(&mut rv, &p, fade_in(FADE_BLOCKS), &mut blk, &mut out);
    rv.force_restart();
    assert_eq!(rv.phase(), clear(CLEAR_BLOCKS));
}

/// Review Focus 1.
#[test]
fn scrolling_types_mid_tail_lands_on_the_last_without_a_click() {
    let mut rv = Box::new(Reverb::new());
    let mut p = params(ReverbType::Quad);
    let mut mem = KnobMemory::new();
    let mut out = Sides::default();
    let mut blk = 0;
    while blk < AT {
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
    }
    let order = [
        ReverbType::Room,
        ReverbType::Hall,
        ReverbType::Bloom,
        ReverbType::Gate,
        ReverbType::Reverse,
        ReverbType::Gate,
    ];
    for t in order {
        mem.store(&p);
        p = p.with_type(t, &mem);
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
    }
    let last = ReverbType::Gate;
    for _ in 0..SWITCH + 10 {
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
    }
    assert_eq!((rv.phase(), rv.running()), (SwitchPhase::Running, last));
    no_click(&out, blk - AT, "scrolling");
}

/// Review Focus 3.
#[test]
fn switch_at_mix_0_then_mix_up_has_no_stale_tail() {
    for &a in TYPES {
        for &b in TYPES {
            let mut rv = Box::new(Reverb::of_type(a));
            let mut n = Noise(0x0BAD_CAFE);
            let mut p = params(a);
            for _ in 0..AT {
                tick(&mut rv, &noise_send(&mut n), &p);
            }
            // MIX to 0, and landed: the return is exactly 0.
            p.mix = 0.0;
            for _ in 0..200 {
                tick(&mut rv, &noise_send(&mut n), &p);
            }
            let o = tick(&mut rv, &noise_send(&mut n), &p);
            assert!(
                o.l.iter().chain(&o.r).all(|&s| s == 0.0),
                "MIX never landed on 0"
            );
            p = switch(&mut rv, &p, b);
            p.mix = 0.0;
            let mut fresh: Option<Box<Reverb>> = None;
            for blk in 0..40 + 200 {
                if blk == 40 {
                    p.mix = 1.0;
                }
                if fresh.is_none() && rv.phase() == fade_in(FADE_BLOCKS) {
                    fresh = Some(Box::new(Reverb::of_type(b)));
                }
                let x = noise_send(&mut n);
                let got = tick(&mut rv, &x, &p);
                let Some(f) = fresh.as_deref_mut() else {
                    assert!(got.l.iter().chain(&got.r).all(|&s| s == 0.0));
                    continue;
                };
                let want = tick(f, &x, &p);
                assert!(
                    got.l
                        .iter()
                        .chain(&got.r)
                        .zip(want.l.iter().chain(&want.r))
                        .all(|(g, w)| g.to_bits() == w.to_bits()),
                    "{a:?} → {b:?}: block {blk} is not the fresh engine's"
                );
            }
            let fresh = fresh.expect("FADE IN reached");
            assert!(
                fresh.arena().lines().iter().any(|&s| s != 0),
                "the send never reached it"
            );
        }
    }
}

/// A project swap (ADR 0046) through the bus, as `fx_swap_test` runs it,
/// carrying another TYPE: the same 15, 4, 15.
#[test]
fn a_project_swap_changing_type_takes_the_same_path() {
    let a = FxParams {
        reverb: params(ReverbType::Quad),
        ..Default::default()
    };
    let mut b = a;
    b.reverb = ReverbParams {
        kind: ReverbType::Room,
        time: 0.4,
        size: 0.9,
        mix: 0.4,
        ..Default::default()
    };
    let mut bus = Box::new(FxBus::new());
    let mut seen = Vec::new();
    for blk in 0..AT + SWITCH {
        let fx = if blk < AT { &a } else { &b };
        let mut sends = [burst(blk); FX_SENDS];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, fx, Tempo::DEFAULT, SR, &mut ret);
        if blk >= AT {
            seen.push(bus.reverb().phase());
        }
    }
    assert_eq!(seen, sequence(FADE_BLOCKS - 1));
    assert_eq!(bus.reverb().running(), ReverbType::Room);
}

/// A project load (ADR 0046) mid-switch, in FADE OUT and in CLEAR: the
/// switch runs on, unrestarted, to the loaded TYPE, and FADE IN is a fresh
/// engine on the loaded knobs. No click.
#[test]
fn a_project_load_mid_switch_lands_on_the_loaded_type() {
    let loaded = ReverbParams {
        kind: ReverbType::Hall,
        time: 0.5,
        size: 0.7,
        damping: 0.3,
        mix: 1.0,
        ..Default::default()
    };
    for at in [fade_out(8), clear(2)] {
        // Noise for the fresh-engine check; the tail for the click.
        for noisy in [true, false] {
            let mut n = Noise(0xF00D_0001);
            let mut send = |blk| {
                if noisy {
                    noise_send(&mut n)
                } else {
                    burst(blk)
                }
            };
            let mut rv = Box::new(Reverb::new());
            let mut p = params(ReverbType::Quad);
            let (mut out, mut seen) = (Sides::default(), Vec::new());
            let mut fresh: Option<Box<Reverb>> = None;
            for blk in 0..AT + SWITCH + 10 {
                if blk == AT {
                    p = switch(&mut rv, &p, ReverbType::Room);
                }
                if rv.phase() == at {
                    p = loaded;
                }
                if fresh.is_none() && rv.phase() == fade_in(FADE_BLOCKS) {
                    fresh = Some(Box::new(Reverb::of_type(ReverbType::Hall)));
                }
                let (x, phase) = (send(blk), rv.phase());
                let got = tick(&mut rv, &x, &p);
                out.push(&got);
                if blk >= AT {
                    seen.push(rv.phase());
                }
                let Some(f) = fresh.as_deref_mut() else {
                    continue;
                };
                let want = tick(f, &x, &p);
                let gain = |i| match phase {
                    SwitchPhase::FadeIn { left } => fade_in_gain(left.get(), i),
                    _ => 1.0,
                };
                for (g, w) in [(&got.l, &want.l), (&got.r, &want.r)] {
                    for i in 0..BLOCK_SIZE {
                        let w = w[i] * gain(i);
                        assert_eq!(g[i].to_bits(), w.to_bits(), "{at:?}: {phase:?} sample {i}");
                    }
                }
            }
            assert_eq!(seen[..SWITCH], sequence(FADE_BLOCKS - 1)[..], "{at:?}");
            assert_eq!(rv.running(), ReverbType::Hall, "{at:?}");
            let fresh = fresh.expect("FADE IN reached");
            if noisy {
                assert!(fresh.arena().lines().iter().any(|&s| s != 0));
            } else {
                no_click(&out, SWITCH + 10, &format!("load in {at:?}"));
            }
        }
    }
}

/// Scrolling slowly enough that the next TYPE lands in FADE IN: each turns
/// it back into FADE OUT from the gain reached, and the last one plays.
#[test]
fn scrolling_into_fade_in_turns_back_without_a_click() {
    let mut rv = Box::new(Reverb::new());
    let mut p = params(ReverbType::Quad);
    let mut mem = KnobMemory::new();
    let mut out = Sides::default();
    let mut blk = 0;
    while blk < AT {
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
    }
    let order = [ReverbType::Room, ReverbType::Hall, ReverbType::Bloom];
    let mut turned = 0;
    for t in order {
        mem.store(&p);
        p = p.with_type(t, &mem);
        let was = rv.phase();
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
        if let SwitchPhase::FadeIn { left } = was {
            assert!(
                left.get() < FADE_BLOCKS,
                "{t:?} landed before any FADE IN gain"
            );
            assert_eq!(rv.phase(), fade_out(left.done() - 1), "{t:?}");
            turned += 1;
        }
        // On to FADE IN, two blocks in.
        while rv.phase() != fade_in(FADE_BLOCKS - 2) {
            out.push(&tick(&mut rv, &burst(blk), &p));
            blk += 1;
        }
    }
    assert_eq!(turned, 2, "the scroll never reached FADE IN");
    for _ in 0..SWITCH + 10 {
        out.push(&tick(&mut rv, &burst(blk), &p));
        blk += 1;
    }
    assert_eq!(
        (rv.phase(), rv.running()),
        (SwitchPhase::Running, ReverbType::Bloom)
    );
    no_click(&out, blk - AT, "scrolling into FADE IN");
}
