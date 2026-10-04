//! Switching TYPE (reverb TYPEs spec § Switching TYPE): the old tail
//! fades out on its latched knobs, one block clears the arena, and the new
//! TYPE fades in as a fresh engine. Never a click, never a stale tail.

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::ease::{at, step_of};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::rev::{FADE_BLOCKS, Reverb, SwitchPhase};
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType};
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{Noise, max_d2};

/// The TYPEs with an engine. Tasks 8–11 each append theirs.
const TYPES: &[ReverbType] = &[ReverbType::Quad];

/// The switch's blocks: FADE OUT, CLEAR, FADE IN.
const SWITCH: usize = 2 * FADE_BLOCKS as usize + 1;
/// The tail's own window, before the switch.
const BEFORE: usize = 30;
/// The tone burst's blocks, and the switch's first block: mid-tail, its
/// `BEFORE` window all tail.
const BURST: usize = 20;
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
    (p.time, p.damping) = (1.0, 0.9);
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

/// The click bound: across `window` (blocks from `AT`), each side's
/// largest second difference stays within the `BEFORE` blocks' before
/// `AT`. And it has teeth: the tail cut to 0 at `AT` breaks it.
fn no_click(out: &Sides, window: usize, what: &str) {
    for side in 0..2 {
        let bound = max_d2(out.at(side, AT - BEFORE..AT));
        let got = max_d2(out.at(side, AT..AT + window));
        assert!(got <= bound, "{what}, side {side}: d2 {got} over {bound}");
        let mut cut = out.at(side, AT - 1..AT + 1).to_vec();
        cut[BLOCK_SIZE + 2..].fill(0.0);
        let heard = max_d2(&cut);
        assert!(
            heard > 2.0 * bound,
            "{what}, side {side}: a cut ({heard}) is within {bound}"
        );
    }
}

/// `a` mid-tail at MIX 1, then the switch to `b` at `AT`, then `after`
/// blocks: every side's output, and the reverb.
fn render(a: ReverbType, b: ReverbType, after: usize) -> (Sides, Box<Reverb>) {
    let mut rv = Box::new(Reverb::of_type(a));
    let mut p = params(a);
    let mut out = Sides::default();
    for blk in 0..AT + after {
        if blk == AT {
            p = switch(&mut rv, &p, b);
        }
        out.push(&tick(&mut rv, &burst(blk), &p));
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

/// FADE OUT `from`..=1 then CLEAR, FADE IN 15..=1 and RUNNING: the phases
/// after each block of a switch whose first block ran FADE OUT `from + 1`.
fn sequence(from: u8) -> Vec<SwitchPhase> {
    let out = (1..=from).rev().map(|left| SwitchPhase::FadeOut { left });
    let fade_in = (1..=FADE_BLOCKS)
        .rev()
        .map(|left| SwitchPhase::FadeIn { left });
    out.chain([SwitchPhase::Clear])
        .chain(fade_in)
        .chain([SwitchPhase::Running])
        .collect()
}

#[test]
fn the_phases_run_15_1_15() {
    assert_eq!(FADE_BLOCKS, 15);
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
    assert_eq!(rv.phase(), SwitchPhase::FadeOut { left: FADE_BLOCKS });
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
fn clear_block_returns_exact_zero() {
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
        run_to(&mut rv, &p, SwitchPhase::Clear, &mut blk, &mut out);
        // The send is loud: CLEAR drops it.
        let loud = [0.9f32; BLOCK_SIZE];
        let o = tick(&mut rv, &loud, &p);
        assert!(
            o.l.iter().chain(&o.r).all(|&s| s == 0.0),
            "{b:?}: CLEAR block not silent"
        );
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
            while rv.phase() != (SwitchPhase::FadeIn { left: FADE_BLOCKS }) {
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
                            SwitchPhase::FadeIn { left } => w[i] * fade_in_gain(left, i),
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
    assert_eq!(sw.phase(), SwitchPhase::Clear);
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
    let five = SwitchPhase::FadeIn { left: 5 };
    run_to(&mut rv, &p, five, &mut blk, &mut out);
    let reached = five.gains().expect("FADE IN ramps").0;
    assert!((reached - 10.0 / 15.0).abs() <= 1e-6, "{reached}");
    // A TYPE change in this block: it runs FADE OUT from that gain.
    let room = p.with_type(ReverbType::Room, &KnobMemory::new());
    out.push(&tick(&mut rv, &burst(blk), &room));
    assert_eq!(rv.phase(), SwitchPhase::FadeOut { left: 9 });
    let first = SwitchPhase::FadeOut { left: 10 }
        .gains()
        .expect("FADE OUT ramps")
        .0;
    assert!((first - reached).abs() <= 1e-6, "{first} from {reached}");
    // And a forced one, from FADE IN 5, starts at the same place.
    let mut rv = Box::new(Reverb::new());
    rv.force_restart();
    let mut blk = 0;
    run_to(&mut rv, &p, five, &mut blk, &mut out);
    rv.force_restart();
    assert_eq!(rv.phase(), SwitchPhase::FadeOut { left: 10 });
    // From FADE IN 15 (no gain yet), straight to CLEAR.
    let mut rv = Box::new(Reverb::new());
    rv.force_restart();
    run_to(
        &mut rv,
        &p,
        SwitchPhase::FadeIn { left: FADE_BLOCKS },
        &mut blk,
        &mut out,
    );
    rv.force_restart();
    assert_eq!(rv.phase(), SwitchPhase::Clear);
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
                if fresh.is_none() && rv.phase() == (SwitchPhase::FadeIn { left: FADE_BLOCKS }) {
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
/// carrying another TYPE: the same 15, 1, 15.
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
