//! FX at the swap (ADR 0046): a project load kills every voice, then
//! publishes the new snapshot under the FX tails. Every FX setting,
//! stepped min↔max either way and in a project-to-project change, never
//! steps a tail: no `common::clicks`, and the largest sample step after the
//! swap is within 2× the largest either project's own tail takes (the 20 ms
//! before the swap, or the new project's tail over the same span).

mod common;

use chimera_core::block::{Block, ParamSpec};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{CHORUS_SPECS, ChorusParams};
use chimera_core::dsp::comp::{COMP_SPECS, CompParams};
use chimera_core::dsp::delay::{DELAY_SPECS, DelayParams};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::reverb::{REVERB_SPECS, ReverbParams};
use chimera_core::dsp::tape::{TAPE_SPECS, TapeParams};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::DAC_PAIRS;
use chimera_hal::BLOCK_SIZE;
use common::{SR, clicks};

const MS: usize = SR as usize / 1000;
/// The voices play into every send to here; the load's kill then fades
/// them over `Voice::FADE`, and the publish is the next block.
const PLAY: usize = 1500 * MS;
const SWAP: usize = PLAY + Voice::FADE as usize;
const BEFORE: usize = 20 * MS;
/// Long enough for the delay's longest TIME to play back what was written
/// at the swap, and for every ease to land.
const AFTER: usize = 600 * MS;
const _: () = assert!(SWAP.is_multiple_of(BLOCK_SIZE) && AFTER.is_multiple_of(BLOCK_SIZE));

/// What every Part sends: two partials, faded in, then the kill's fade.
fn send(n: usize) -> f32 {
    let t = n as f32 / SR as f32;
    let tau = core::f32::consts::TAU;
    let x = 0.4 * libm::sinf(tau * 220.0 * t) + 0.2 * libm::sinf(tau * 660.0 * t + 0.3);
    let up = (n as f32 / (20 * MS) as f32).min(1.0);
    let kill = 1.0 - ((n as f32 - PLAY as f32) / Voice::FADE as f32).clamp(0.0, 1.0);
    x * up * kill
}

type Sides = [Vec<f32>; 2];

/// DAC pair 1 (the delay and the tape live there): the bus on `a` until
/// the swap, on `b` from it.
fn render(a: &FxParams, b: &FxParams) -> Sides {
    let mut bus = Box::new(FxBus::new());
    let mut sides: Sides = [Vec::new(), Vec::new()];
    for blk in 0..(SWAP + AFTER) / BLOCK_SIZE {
        let n0 = blk * BLOCK_SIZE;
        let p = if n0 < SWAP { a } else { b };
        let x: [f32; BLOCK_SIZE] = core::array::from_fn(|i| send(n0 + i));
        let mut sends = [x; FX_SENDS];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, p, SR, &mut ret);
        let mut out = [[0.0f32; 2 * BLOCK_SIZE]; DAC_PAIRS];
        for i in 0..BLOCK_SIZE {
            (out[0][2 * i], out[0][2 * i + 1]) = (ret.l[i], ret.r[i]);
        }
        bus.master(&mut out, p, SR);
        for (i, s) in out[0].iter().enumerate() {
            sides[i % 2].push(*s);
        }
    }
    sides
}

/// The largest |x[n] − x[n−1]|.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

/// `swapped` (on `a`, then `b` from the swap) against `b_only` (on `b`
/// throughout): no click, and no step past 2× what either tail takes.
fn verdict(swapped: &Sides, b_only: &Sides) -> Result<(), String> {
    for side in 0..2 {
        let s = &swapped[side];
        let before = max_step(&s[SWAP - BEFORE..SWAP]);
        let after = max_step(&s[SWAP - 1..]);
        let own = max_step(&b_only[side][SWAP - 1..]);
        let bound = 2.0 * before.max(own) + 1e-6;
        if after > bound {
            return Err(format!(
                "side {side}: step {after} > 2 × max({before} before, {own} on its own)"
            ));
        }
        let c = clicks(&s[SWAP - BEFORE..]);
        if !c.is_empty() {
            return Err(format!("side {side}: clicks {c:?}"));
        }
    }
    Ok(())
}

fn swap(a: &FxParams, b: &FxParams) -> Result<(), String> {
    verdict(&render(a, b), &render(b, b))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fx {
    Chorus,
    Delay,
    Reverb,
    Comp,
    Tape,
}

/// Project A: every effect on.
fn project_a() -> FxParams {
    FxParams {
        chorus: ChorusParams {
            mode: 3,
            rate: 0.5,
            depth: 0.5,
            mix: 0.7,
        },
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.6,
            wow_flutter: 0.15,
            saturation: 0.2,
            tone: 0.6,
            mix: 0.7,
            rev_send: 0.3,
        },
        reverb: ReverbParams {
            grit: 0.3,
            time: 0.7,
            damping: 0.3,
            size: 0.5,
            mix: 0.7,
        },
        comp: CompParams {
            thresh: 0.5,
            ratio: 3,
            attack: 0.5,
            release: 0.5,
            makeup: 0.1,
            mix: 1.0,
        },
        tape: TapeParams {
            drive: 0.5,
            tone: 0.5,
            wow: 0.3,
            mix: 0.5,
        },
    }
}

/// Project B: every effect on, every setting other than A's.
fn project_b() -> FxParams {
    FxParams {
        chorus: ChorusParams {
            mode: 2,
            rate: 0.8,
            depth: 0.3,
            mix: 0.4,
        },
        delay: DelayParams {
            time_ms: 120.0,
            feedback: 0.8,
            wow_flutter: 0.6,
            saturation: 0.7,
            tone: 0.3,
            mix: 0.5,
            rev_send: 0.0,
        },
        reverb: ReverbParams {
            grit: 0.8,
            time: 0.4,
            damping: 0.7,
            size: 0.9,
            mix: 0.4,
        },
        comp: CompParams {
            thresh: 0.3,
            ratio: 6,
            attack: 0.2,
            release: 0.8,
            makeup: 0.3,
            mix: 0.7,
        },
        tape: TapeParams {
            drive: 0.2,
            tone: 0.8,
            wow: 0.0,
            mix: 0.8,
        },
    }
}

/// Only `fx` on, at project A's settings; the comp and the tape work on
/// the delay's and the reverb's tails, and the delay feeds the reverb.
fn solo(fx: Fx) -> FxParams {
    let a = project_a();
    let mut p = FxParams::default();
    match fx {
        Fx::Chorus => p.chorus = a.chorus,
        Fx::Delay | Fx::Reverb | Fx::Comp | Fx::Tape => {
            (p.delay, p.reverb) = (a.delay, a.reverb);
            if fx == Fx::Reverb {
                p.delay.mix = 0.0;
            }
        }
    }
    match fx {
        Fx::Comp => p.comp = a.comp,
        Fx::Tape => p.tape = a.tape,
        _ => {}
    }
    p
}

fn specs(fx: Fx) -> &'static [ParamSpec] {
    match fx {
        Fx::Chorus => &CHORUS_SPECS,
        Fx::Delay => &DELAY_SPECS,
        Fx::Reverb => &REVERB_SPECS,
        Fx::Comp => &COMP_SPECS,
        Fx::Tape => &TAPE_SPECS,
    }
}

fn write(p: &mut FxParams, fx: Fx, s: &ParamSpec, v: f32) {
    match fx {
        Fx::Chorus => p.chorus.write(s.id, v),
        Fx::Delay => p.delay.write(s.id, v),
        Fx::Reverb => p.reverb.write(s.id, v),
        Fx::Comp => p.comp.write(s.id, v),
        Fx::Tape => p.tape.write(s.id, v),
    }
}

/// Each of `fx`'s settings stepped min → max and max → min at the swap.
fn each_setting(fx: Fx) {
    let mut bad = Vec::new();
    for s in specs(fx) {
        for (from, to) in [(s.min, s.max), (s.max, s.min)] {
            let (mut a, mut b) = (solo(fx), solo(fx));
            write(&mut a, fx, s, from);
            write(&mut b, fx, s, to);
            if let Err(e) = swap(&a, &b) {
                bad.push(format!("{fx:?} {} {from} → {to}: {e}", s.label));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn chorus_settings_never_step_the_tail() {
    each_setting(Fx::Chorus);
}

#[test]
fn delay_settings_never_step_the_tail() {
    each_setting(Fx::Delay);
}

#[test]
fn reverb_settings_never_step_the_tail() {
    each_setting(Fx::Reverb);
}

#[test]
fn comp_settings_never_step_the_tail() {
    each_setting(Fx::Comp);
}

/// The tape runs only with `master-tape` (ADR 0055); without it this
/// checks nothing moves.
#[test]
fn tape_settings_never_step_the_tail() {
    each_setting(Fx::Tape);
}

/// A project to another, and to and from NEW (every effect off).
#[test]
fn a_project_change_never_steps_the_tail() {
    let (a, b, new) = (project_a(), project_b(), FxParams::default());
    let mut bad = Vec::new();
    for (name, from, to) in [
        ("A → B", a, b),
        ("B → A", b, a),
        ("A → NEW", a, new),
        ("NEW → A", new, a),
    ] {
        if let Err(e) = swap(&from, &to) {
            bad.push(format!("{name}: {e}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Each effect is sounding at the swap, so the steps above are heard.
#[test]
fn every_tail_is_live_at_the_swap() {
    for fx in [Fx::Chorus, Fx::Delay, Fx::Reverb, Fx::Comp, Fx::Tape] {
        let p = solo(fx);
        let s = render(&p, &p);
        let level = common::rms(&s[0][SWAP..SWAP + 2 * MS]);
        assert!(level > 0.01, "{fx:?}: {level}");
    }
}

/// The verdict hears a return cut at the swap, as an un-eased MIX would.
#[test]
fn the_verdict_hears_a_cut() {
    let a = project_a();
    let on = render(&a, &a);
    let mut cut = on.clone();
    for side in &mut cut {
        side[SWAP..].fill(0.0);
    }
    let silent = [vec![0.0; SWAP + AFTER], vec![0.0; SWAP + AFTER]];
    assert!(verdict(&on, &on).is_ok());
    assert!(verdict(&cut, &silent).is_err());
}
