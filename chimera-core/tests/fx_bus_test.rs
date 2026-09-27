//! The shared FX bus (instrument-core spec § Audio path, FX diet spec
//! § Bus): each effect runs once on the sum of the parts' sends; the return
//! is the sum of the wet outputs of the effects that are on.

use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::ring::{RingReverb, first_reflection, size_step};
use chimera_core::hw::DAC_PAIRS;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn burst() -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| if i % 16 == 0 { 0.5 } else { -0.1 })
}

fn full() -> Stereo {
    Stereo {
        l: [1.0; BLOCK_SIZE],
        r: [1.0; BLOCK_SIZE],
    }
}

/// A mono effect's wet block, on both sides.
fn mono(mut x: [f32; BLOCK_SIZE], f: impl FnOnce(&mut [f32; BLOCK_SIZE])) -> Stereo {
    f(&mut x);
    Stereo { l: x, r: x }
}

/// Defaults match the old `ParamSnapshot` FX: everything off.
#[test]
fn defaults_are_all_off() {
    let p = FxParams::default();
    assert!(!p.chorus.is_on() && !p.delay.is_on() && !p.reverb.is_on());
    assert_eq!(p.reverb.mix, 0.0);
    assert_eq!(p.delay.time_ms, 375.0);
    assert_eq!(p.delay.rev_send, 0.0);
}

#[test]
fn effects_that_are_off_return_nothing() {
    let mut bus = Box::new(FxBus::new());
    let mut sends = [burst(); FX_SENDS];
    let mut ret = full();
    bus.process(&mut sends, &FxParams::default(), SR, &mut ret);
    assert_eq!(ret, Stereo::SILENT);
}

/// Send/return: the bus's return is each effect's own wet output (its wet
/// signal × MIX, the return level) and carries none of the dry send.
#[test]
fn return_is_each_effects_wet_output() {
    let mut p = FxParams {
        chorus: ChorusParams {
            mode: 3,
            rate: 0.5,
            depth: 0.5,
            mix: 0.5,
        },
        delay: DelayParams {
            time_ms: 1.0,
            feedback: 0.6,
            mix: 0.5,
            ..DelayParams::default()
        },
        ..FxParams::default()
    };
    p.reverb.mix = 0.5;
    p.reverb.time = 0.7;
    for slot in 0..FX_SENDS {
        let mut params = FxParams::default();
        match slot {
            0 => params.chorus = p.chorus,
            1 => params.delay = p.delay,
            _ => params.reverb = p.reverb,
        }
        let mut bus = Box::new(FxBus::new());
        let (mut c, mut d, mut r) = (
            Box::new(JunoChorus::new()),
            Box::new(TapeDelay::new()),
            Box::new(RingReverb::new()),
        );
        let mut heard = 0.0f32;
        for b in 0..80 {
            let input = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
            let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
            sends[slot] = input;
            let mut ret = Stereo::SILENT;
            bus.process(&mut sends, &params, SR, &mut ret);
            let want = match slot {
                0 => {
                    let mut w = Stereo::SILENT;
                    c.process_wet(&input, &params.chorus, SR, &mut w);
                    w
                }
                1 => mono(input, |w| d.process_wet(w, &params.delay, SR)),
                _ => {
                    let mut w = Stereo::SILENT;
                    r.process(
                        &input,
                        &params.reverb.controls(),
                        params.reverb.mix,
                        SR,
                        &mut w,
                    );
                    w
                }
            };
            assert_eq!(ret, want, "effect {slot} block {b}");
            heard = ret
                .l
                .iter()
                .chain(&ret.r)
                .fold(heard, |m, s| m.max(s.abs()));
        }
        assert!(heard > 1e-3, "effect {slot} returns something");
    }
}

/// Before its first tap the reverb's wet signal is silent: so is the
/// return, however loud the send.
#[test]
fn reverb_return_is_silent_before_its_first_reflection() {
    let mut p = FxParams::default();
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    for b in 0..first_reflection(size_step(p.reverb.size)) / BLOCK_SIZE {
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], burst()];
        let mut ret = full();
        bus.process(&mut sends, &p, SR, &mut ret);
        assert_eq!(ret, Stereo::SILENT, "block {b}");
    }
}

// ── REV SEND ──

/// The bus's left return: a burst into the delay for the first 8 blocks
/// (into the reverb too when `own`), REV SEND `send(block)`.
fn with_rev_send(blocks: usize, own: bool, send: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut p = FxParams::default();
    (p.delay.time_ms, p.delay.feedback, p.delay.mix) = (120.0, 0.5, 0.5);
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    let mut out = Vec::new();
    for b in 0..blocks {
        p.delay.rev_send = send(b);
        let x = if b < 8 { burst() } else { [0.0; BLOCK_SIZE] };
        let own = if own { x } else { [0.0; BLOCK_SIZE] };
        let mut sends = [[0.0; BLOCK_SIZE], x, own];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &p, SR, &mut ret);
        out.extend(ret.l);
    }
    out
}

/// FX diet spec § Testing: at REV SEND 0 the reverb hears only its own
/// send, bit for bit: the return is the delay and the reverb, each run
/// alone.
#[test]
fn rev_send_0_leaves_the_reverb_its_own_send() {
    let mut p = FxParams::default();
    (p.delay.time_ms, p.delay.feedback, p.delay.mix) = (120.0, 0.5, 0.5);
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    let (mut d, mut r) = (Box::new(TapeDelay::new()), Box::new(RingReverb::new()));
    for b in 0..64 {
        let x = if b < 8 { burst() } else { [0.0; BLOCK_SIZE] };
        let mut sends = [[0.0; BLOCK_SIZE], x, x];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &p, SR, &mut ret);
        let mut dw = x;
        d.process_wet(&mut dw, &p.delay, SR);
        let mut rw = Stereo::SILENT;
        r.process(&x, &p.reverb.controls(), p.reverb.mix, SR, &mut rw);
        let want = Stereo {
            l: core::array::from_fn(|i| dw[i] + rw.l[i]),
            r: core::array::from_fn(|i| dw[i] + rw.r[i]),
        };
        assert_eq!(ret, want, "block {b}");
    }
}

/// FX diet spec § Testing: REV SEND above 0 raises the reverb's energy.
/// Only the delay is sent to, so the return less REV SEND 0's is the
/// reverb's alone.
#[test]
fn rev_send_raises_the_reverbs_energy() {
    let base = with_rev_send(400, false, |_| 0.0);
    let reverb = |s: f32| -> f32 {
        with_rev_send(400, false, |_| s)
            .iter()
            .zip(&base)
            .map(|(a, b)| (a - b) * (a - b))
            .sum()
    };
    let (half, full) = (reverb(0.5), reverb(1.0));
    assert!(half > 0.1, "{half}");
    assert!(full > 2.0 * half, "{half} → {full}");
}

/// With the delay off its buffer holds the raw send: REV SEND passes none
/// of it to the reverb.
#[test]
fn rev_send_feeds_nothing_while_the_delay_is_off() {
    let mut p = FxParams::default();
    (p.delay.rev_send, p.reverb.mix) = (1.0, 0.5);
    let mut bus = Box::new(FxBus::new());
    for b in 0..first_reflection(size_step(p.reverb.size)) / BLOCK_SIZE + 64 {
        let mut sends = [[0.0; BLOCK_SIZE], burst(), [0.0; BLOCK_SIZE]];
        let mut ret = full();
        bus.process(&mut sends, &p, SR, &mut ret);
        assert_eq!(ret, Stereo::SILENT, "block {b}");
    }
}

/// Largest second difference.
fn kink(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0f32, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

/// FX diet spec § Testing: REV SEND jumping 0 → 1 or 1 → 0 mid-echo does
/// not click: over the next 50 ms the largest second difference is at most
/// 1.5× the larger of the renders held at 0 and at 1.
#[test]
fn a_rev_send_jump_does_not_click() {
    let (at_b, win) = (176, 2_400);
    let w = at_b * BLOCK_SIZE..at_b * BLOCK_SIZE + win;
    let (off, on) = (
        with_rev_send(220, false, |_| 0.0),
        with_rev_send(220, false, |_| 1.0),
    );
    let bound = kink(&off[w.clone()]).max(kink(&on[w.clone()]));
    for (a, b) in [(0.0, 1.0), (1.0, 0.0)] {
        let moved = with_rev_send(220, false, |blk| if blk < at_b { a } else { b });
        let k = kink(&moved[w.clone()]);
        assert!(k <= 1.5 * bound, "{a} → {b}: {k} vs {bound}");
    }
}

/// FX diet spec § Master section: off by default, the master section
/// leaves every pair untouched, bit for bit.
#[test]
fn the_master_section_is_off_by_default() {
    let p = FxParams::default();
    assert!(!p.tape.is_on());
    assert!(!p.comp.is_on());
    let mut out: [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS] =
        core::array::from_fn(|k| core::array::from_fn(|i| ((i * 7 + k) % 13) as f32 / 6.5 - 1.0));
    let before = out;
    let mut bus = Box::new(FxBus::new());
    for b in 0..16 {
        bus.master(&mut out, &p, SR);
        assert_eq!(out, before, "block {b}");
    }
}
