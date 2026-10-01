//! FX at the swap (ADR 0046): a project load kills every voice, then
//! publishes the new snapshot under the FX tails. Every FX setting,
//! stepped min↔max either way with its effect alone, the delay also at
//! its brightest, and in a project-to-project change, never steps a tail
//! (`verdict`).

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

/// What every Part sends, faded in, then the kill's fade.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sig {
    /// 220 and 660 Hz.
    Partials,
    /// 1 kHz: bright, so a read or write step stands out.
    Sine1k,
    /// A 220 Hz saw's first eight partials.
    Saw,
}

const SIGS: [Sig; 3] = [Sig::Partials, Sig::Sine1k, Sig::Saw];

fn send(sig: Sig, n: usize) -> f32 {
    let t = n as f32 / SR as f32;
    let sin = |hz: f32, ph: f32| libm::sinf(core::f32::consts::TAU * hz * t + ph);
    let x = match sig {
        Sig::Partials => 0.4 * sin(220.0, 0.0) + 0.2 * sin(660.0, 0.3),
        Sig::Sine1k => 0.6 * sin(1000.0, 0.0),
        Sig::Saw => (1..=8)
            .map(|k| 0.3 / k as f32 * sin(220.0 * k as f32, 0.0))
            .sum(),
    };
    let up = (n as f32 / (20 * MS) as f32).min(1.0);
    let kill = 1.0 - ((n as f32 - PLAY as f32) / Voice::FADE as f32).clamp(0.0, 1.0);
    x * up * kill
}

type Sides = [Vec<f32>; 2];

/// DAC pair 1 (the delay and the tape live there): the bus on `a` until
/// the swap, on `b` from it.
fn render(sig: Sig, a: &FxParams, b: &FxParams) -> Sides {
    let mut bus = Box::new(FxBus::new());
    let mut sides: Sides = [Vec::new(), Vec::new()];
    for blk in 0..(SWAP + AFTER) / BLOCK_SIZE {
        let n0 = blk * BLOCK_SIZE;
        let p = if n0 < SWAP { a } else { b };
        let x: [f32; BLOCK_SIZE] = core::array::from_fn(|i| send(sig, n0 + i));
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

/// The largest |x[n] − 2x[n−1] + x[n−2]|.
fn max_d2(x: &[f32]) -> f32 {
    x.windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f32::max)
}

/// The window each check looks at: a click is local.
const WINDOW: usize = 5 * MS;
/// Each project's own tail is judged this far either side of the window:
/// the swapped tail's content is neither project's sample for sample, so
/// a beat or a peak lands a few ms off.
const NEAR: usize = 20 * MS;
/// Below this a window is silence (−80 dB).
const FLOOR: f32 = 1e-4;

/// `swapped` (on A, then B from the swap) against `a_only` and `b_only`
/// (each on its own settings throughout), window by window from 20 ms
/// before the swap. A window clicks if it stands out both ways:
/// - from the projects' own tails: its largest step past 2×, or its
///   largest second difference past 4× (the sweep's `CLICK_RATIO`), the
///   larger either tail takes within `NEAR` of it, or a `common::clicks`
///   neither tail has there;
/// - from the swapped tail around it (`NEAR` either side) by the same
///   ratios: a click is local, where a level the swap legitimately
///   changes (a MIX brought up, a loop that no longer cancels itself) is
///   as large in the windows beside it.
fn verdict(swapped: &Sides, a_only: &Sides, b_only: &Sides) -> Result<(), String> {
    for side in 0..2 {
        let s = &swapped[side];
        let mut w0 = SWAP - BEFORE;
        while w0 < s.len() {
            let end = (w0 + WINDOW).min(s.len());
            let w = w0 - 2..end;
            let near = w0 - NEAR..(end + NEAR).min(s.len());
            let own = |f: fn(&[f32]) -> f32| {
                f(&a_only[side][near.clone()]).max(f(&b_only[side][near.clone()]))
            };
            let around = |f: fn(&[f32]) -> f32| {
                let after = (end + NEAR).min(s.len());
                let right = if after > end + 2 {
                    f(&s[end..after])
                } else {
                    0.0
                };
                f(&s[near.start..w0]).max(right)
            };
            let (step, d2) = (max_step(&s[w.clone()]), max_d2(&s[w.clone()]));
            let at = (w0 as f32 - SWAP as f32) / MS as f32;
            let out = |v: f32, r: f32, k: f32| v > k * r + FLOOR;
            if out(step, own(max_step), 2.0) && out(step, around(max_step), 2.0) {
                return Err(format!(
                    "side {side} at {at:+.0} ms: step {step}, own {}, around {}",
                    own(max_step),
                    around(max_step)
                ));
            }
            if out(d2, own(max_d2), 4.0) && out(d2, around(max_d2), 4.0) {
                return Err(format!(
                    "side {side} at {at:+.0} ms: d2 {d2}, own {}, around {}",
                    own(max_d2),
                    around(max_d2)
                ));
            }
            // A loud bright tail trips `clicks` on its own.
            let heard = |x: &[f32], r: &core::ops::Range<usize>| !clicks(&x[r.clone()]).is_empty();
            if heard(s, &w)
                && !heard(&a_only[side], &near)
                && !heard(&b_only[side], &near)
                && out(d2, around(max_d2), 4.0)
            {
                return Err(format!("side {side} at {at:+.0} ms: clicks"));
            }
            w0 += WINDOW;
        }
    }
    Ok(())
}

fn swap(sig: Sig, a: &FxParams, b: &FxParams) -> Result<(), String> {
    verdict(&render(sig, a, b), &render(sig, a, a), &render(sig, b, b))
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

/// Only `fx` on, at project A's settings, every other effect off. The
/// comp and the tape work on the delay's tail.
fn solo(fx: Fx) -> FxParams {
    let a = project_a();
    let mut p = FxParams::default();
    match fx {
        Fx::Chorus => p.chorus = a.chorus,
        Fx::Reverb => p.reverb = a.reverb,
        Fx::Delay | Fx::Comp | Fx::Tape => p.delay = a.delay,
    }
    p.delay.rev_send = 0.0;
    match fx {
        Fx::Comp => p.comp = a.comp,
        Fx::Tape => p.tape = a.tape,
        _ => {}
    }
    p
}

/// The delay at its brightest and longest: TONE 1, FDBK 0.9, MIX 1.
fn bright() -> FxParams {
    let mut p = solo(Fx::Delay);
    (p.delay.tone, p.delay.feedback, p.delay.mix) = (1.0, 0.9, 1.0);
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

/// Each of `base`'s `fx` settings stepped min → max and max → min at
/// the swap, on each signal. REV SEND is heard only with the reverb on.
fn each_setting(fx: Fx, base: FxParams) {
    let mut bad = Vec::new();
    for s in specs(fx) {
        let mut base = base;
        if fx == Fx::Delay && s.id == DelayParams::REV_SEND {
            base.reverb = project_a().reverb;
        }
        for (from, to) in [(s.min, s.max), (s.max, s.min)] {
            let (mut a, mut b) = (base, base);
            write(&mut a, fx, s, from);
            write(&mut b, fx, s, to);
            for sig in SIGS {
                if let Err(e) = swap(sig, &a, &b) {
                    bad.push(format!("{fx:?} {} {from} → {to}, {sig:?}: {e}", s.label));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn chorus_settings_never_step_the_tail() {
    each_setting(Fx::Chorus, solo(Fx::Chorus));
}

#[test]
fn delay_settings_never_step_the_tail() {
    each_setting(Fx::Delay, solo(Fx::Delay));
}

#[test]
fn bright_delay_settings_never_step_the_tail() {
    each_setting(Fx::Delay, bright());
}

#[test]
fn reverb_settings_never_step_the_tail() {
    each_setting(Fx::Reverb, solo(Fx::Reverb));
}

#[test]
fn comp_settings_never_step_the_tail() {
    each_setting(Fx::Comp, solo(Fx::Comp));
}

/// The tape runs only with `master-tape` (ADR 0055); without it this
/// checks nothing moves.
#[test]
fn tape_settings_never_step_the_tail() {
    each_setting(Fx::Tape, solo(Fx::Tape));
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
        for sig in SIGS {
            if let Err(e) = swap(sig, &from, &to) {
                bad.push(format!("{name}, {sig:?}: {e}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Each effect is sounding at the swap, so the steps above are heard.
#[test]
fn every_tail_is_live_at_the_swap() {
    for fx in [Fx::Chorus, Fx::Delay, Fx::Reverb, Fx::Comp, Fx::Tape] {
        let p = solo(fx);
        let s = render(Sig::Partials, &p, &p);
        let level = common::rms(&s[0][SWAP..SWAP + 2 * MS]);
        assert!(level > 0.01, "{fx:?}: {level}");
    }
}

/// The verdict hears a return cut at the swap, as an un-eased MIX would.
#[test]
fn the_verdict_hears_a_cut() {
    let a = project_a();
    let on = render(Sig::Partials, &a, &a);
    let mut cut = on.clone();
    for side in &mut cut {
        side[SWAP..].fill(0.0);
    }
    let silent = [vec![0.0; SWAP + AFTER], vec![0.0; SWAP + AFTER]];
    assert!(verdict(&on, &on, &on).is_ok());
    assert!(verdict(&cut, &on, &silent).is_err());
}
