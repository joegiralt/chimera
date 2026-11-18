//! The shared FX bus (instrument-core spec § Audio path, FX diet spec
//! § Bus): each effect runs once on the sum of the parts' sends; the return
//! is the sum of the effects' wet outputs, each × its eased MIX.
mod common;
use common::SR;

use chimera_core::dsp::chorus::{ChorusMode, ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::ring::{first_reflection, size_step};
use chimera_core::dsp::{SendLevel, Stereo};
use chimera_core::hw::DAC_PAIRS;
use chimera_hal::BLOCK_SIZE;

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

/// `OFF` is everything off; the other defaults are the default's.
#[test]
fn off_is_all_off() {
    let p = FxParams::OFF;
    assert!(!p.chorus.is_on() && !p.delay.is_on() && !p.reverb.is_on());
    assert_eq!(p.reverb.mix, 0.0);
    assert_eq!(p.delay.time_ms, 375.0);
    assert_eq!(p.delay.rev_send, SendLevel::OFF);
}

#[test]
fn effects_that_are_off_return_nothing() {
    let mut bus = Box::new(FxBus::new());
    let mut sends = [burst(); FX_SENDS];
    let mut ret = full();
    bus.process(&mut sends, &FxParams::OFF, Tempo::DEFAULT, SR, &mut ret);
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
        ..FxParams::OFF
    };
    p.reverb.mix = 0.5;
    p.reverb.time = 0.7;
    for slot in 0..FX_SENDS {
        let mut params = FxParams::OFF;
        match slot {
            0 => params.chorus = p.chorus,
            1 => params.delay = p.delay,
            _ => params.reverb = p.reverb,
        }
        let mut bus = Box::new(FxBus::new());
        let (mut c, mut d, mut r) = (
            Box::new(JunoChorus::new()),
            Box::new(TapeDelay::new()),
            Box::new(Reverb::new()),
        );
        let mut heard = 0.0f32;
        for b in 0..80 {
            let input = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
            let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
            sends[slot] = input;
            let mut ret = Stereo::SILENT;
            bus.process(&mut sends, &params, Tempo::DEFAULT, SR, &mut ret);
            let want = match slot {
                0 => {
                    let mut w = Stereo::SILENT;
                    c.process_wet(&input, &params.chorus, SR, &mut w);
                    w
                }
                1 => mono(input, |w| {
                    d.process_wet(w, &params.delay, SR);
                }),
                _ => {
                    let mut w = Stereo::SILENT;
                    r.process(
                        &input,
                        &params.reverb,
                        params.reverb.mix,
                        Tempo::DEFAULT,
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
    let mut p = FxParams::OFF;
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    for b in 0..first_reflection(size_step(p.reverb.size)) / BLOCK_SIZE {
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], burst()];
        let mut ret = full();
        bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
        assert_eq!(ret, Stereo::SILENT, "block {b}");
    }
}

// ── REV SEND ──

/// The bus's left return: a burst into the delay for the first 8 blocks
/// (into the reverb too when `own`), REV SEND `send(block)`.
fn with_rev_send(blocks: usize, own: bool, send: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut p = FxParams::OFF;
    (p.delay.time_ms, p.delay.feedback, p.delay.mix) = (120.0, 0.5, 0.5);
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    let mut out = Vec::new();
    for b in 0..blocks {
        p.delay.rev_send = SendLevel::new(send(b));
        let x = if b < 8 { burst() } else { [0.0; BLOCK_SIZE] };
        let own = if own { x } else { [0.0; BLOCK_SIZE] };
        let mut sends = [[0.0; BLOCK_SIZE], x, own];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
        out.extend(ret.l);
    }
    out
}

/// FX diet spec § Testing: at REV SEND 0 the reverb hears only its own
/// send, bit for bit: the return is the delay and the reverb, each run
/// alone.
#[test]
fn rev_send_0_leaves_the_reverb_its_own_send() {
    let mut p = FxParams::OFF;
    (p.delay.time_ms, p.delay.feedback, p.delay.mix) = (120.0, 0.5, 0.5);
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    let (mut d, mut r) = (Box::new(TapeDelay::new()), Box::new(Reverb::new()));
    for b in 0..64 {
        let x = if b < 8 { burst() } else { [0.0; BLOCK_SIZE] };
        let mut sends = [[0.0; BLOCK_SIZE], x, x];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
        let mut dw = x;
        d.process_wet(&mut dw, &p.delay, SR);
        let mut rw = Stereo::SILENT;
        r.process(&x, &p.reverb, p.reverb.mix, Tempo::DEFAULT, SR, &mut rw);
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

/// REV SEND is on the Part sends' squared taper (ADR 0278): half way feeds
/// the reverb 12 dB under the top, where linear fed it 6. The reverb's
/// share of the return falls a little faster than its feed (7.2 dB
/// measured on the linear knob, 15.4 on the taper), so the bound is wide.
#[test]
fn rev_send_half_sits_12_db_under_full() {
    let base = with_rev_send(400, false, |_| 0.0);
    let reverb = |s: f32| -> f32 {
        with_rev_send(400, false, |_| s)
            .iter()
            .zip(&base)
            .map(|(a, b)| (a - b) * (a - b))
            .sum()
    };
    let db = 10.0 * (reverb(0.5) / reverb(1.0)).log10();
    assert!((-18.0..-12.0).contains(&db), "{db} dB");
}

/// With the delay off its buffer holds the raw send: REV SEND passes none
/// of it to the reverb.
#[test]
fn rev_send_feeds_nothing_while_the_delay_is_off() {
    let mut p = FxParams::OFF;
    (p.delay.rev_send, p.reverb.mix) = (SendLevel::FULL, 0.5);
    let mut bus = Box::new(FxBus::new());
    for b in 0..first_reflection(size_step(p.reverb.size)) / BLOCK_SIZE + 64 {
        let mut sends = [[0.0; BLOCK_SIZE], burst(), [0.0; BLOCK_SIZE]];
        let mut ret = full();
        bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
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

/// SMEAR reaches the ring through the bus: at 1 the return differs once
/// the burst has been round a stage delay (about 100 blocks at SIZE 0.5).
#[test]
fn smear_changes_the_return() {
    let render = |smear: f32| {
        let mut p = FxParams::OFF;
        (p.reverb.mix, p.reverb.smear, p.reverb.rate) = (0.5, smear, 1.0);
        let mut bus = Box::new(FxBus::new());
        let mut out = Vec::new();
        for b in 0..200 {
            let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
            sends[2] = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
            let mut ret = Stereo::SILENT;
            bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
            out.extend(ret.l);
        }
        out
    };
    assert!(render(0.0) != render(1.0));
}

/// FX diet spec § Master section: off by default, the master section
/// leaves every pair untouched, bit for bit.
#[test]
fn the_master_section_is_off_by_default() {
    let p = FxParams::OFF;
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

/// #61: an effect at MIX 0 keeps running on its send, so bringing MIX back
/// up plays what the send is doing now. A burst into each effect, MIX 0
/// through 2 s of silence, then MIX back to 0.5: the delay's line and the
/// reverb's ring have long let the burst go, and the return stays silent.
#[test]
fn an_effect_at_mix_0_never_replays_a_stale_tail() {
    let mut p = FxParams::OFF;
    p.chorus.mode = 3;
    (p.delay.time_ms, p.delay.feedback) = (400.0, 0.0);
    (p.reverb.time, p.reverb.size) = (0.3, 0.5);
    let on = |p: &mut FxParams, mix: f32| {
        (p.chorus.mix, p.delay.mix, p.reverb.mix) = (mix, mix, mix);
    };
    let mut bus = Box::new(FxBus::new());
    let mut peak_after = 0.0f32;
    for b in 0..1600 {
        on(&mut p, if (4..1500).contains(&b) { 0.0 } else { 0.5 });
        let x = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
        let mut sends = [x, x, x];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
        if b >= 1500 {
            peak_after = ret
                .l
                .iter()
                .chain(&ret.r)
                .fold(peak_after, |m, s| m.max(s.abs()));
        }
    }
    assert!(peak_after < 1e-4, "stale tail at {peak_after}");
}

/// Never snap: an effect's MIX 0.5 → 0 fades its return over the ease,
/// no step at the switch.
#[test]
fn an_effect_switched_off_fades_its_return() {
    let mut p = FxParams::OFF;
    p.delay.mix = 0.5;
    (p.delay.time_ms, p.delay.feedback) = (20.0, 0.0);
    let mut bus = Box::new(FxBus::new());
    let mut rets = Vec::new();
    for b in 0..40 {
        if b == 20 {
            p.delay.mix = 0.0;
        }
        let mut sends = [[0.0; BLOCK_SIZE], [0.25; BLOCK_SIZE], [0.0; BLOCK_SIZE]];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
        rets.extend(ret.l);
    }
    let at = 20 * BLOCK_SIZE;
    // A DC send's echo: 0.125 held, then gliding down, never stepping.
    assert!((rets[at - 1] - 0.125).abs() < 1e-3, "{}", rets[at - 1]);
    let steps = rets[at - 1..].windows(2).map(|w| (w[1] - w[0]).abs());
    assert!(steps.fold(0.0f32, f32::max) < 0.005);
    assert!(rets[at + BLOCK_SIZE] > 0.05, "a fade, not a cut");
}

/// A new project's returns are at unity: a send is all it takes to hear an
/// effect (sends spec § 4).
#[test]
fn new_returns_are_audible() {
    let p = FxParams::default();
    assert_eq!(p.chorus.mode, ChorusMode::JunoI as u8);
    assert_eq!((p.chorus.mix, p.delay.mix, p.reverb.mix), (1.0, 1.0, 1.0));
    let o = FxParams::OFF;
    assert_eq!(o.chorus.mode, ChorusMode::Off as u8);
    assert_eq!((o.chorus.mix, o.delay.mix, o.reverb.mix), (0.0, 0.0, 0.0));
    assert!(!o.chorus.is_on() && !o.delay.is_on() && !o.reverb.is_on());
}

/// A NEW project, Part 0 playing INIT on pair 3 with only a reverb send:
/// pair 1 carries the return, above -60 dBFS inside half a second.
#[test]
fn a_send_on_new_is_heard() {
    use common::scene::{db, init_perf, run};
    let mut perf = init_perf(chimera_core::params::EngineType::Algo);
    let e = perf.edit(chimera_core::project::PartId::ALL[0]);
    e.mix.output = chimera_core::part::DacPair::P3;
    e.sound.params.out.sends = [0.0, 0.0, 0.5].map(SendLevel::new);
    let blocks = 24_000 / BLOCK_SIZE;
    let r = run(&perf, &[(0, 60)], 100, blocks, blocks, None);
    let p1 = &r.out[0];
    let rms = (p1.iter().map(|x| x * x).sum::<f32>() / p1.len() as f32).sqrt();
    assert!(db(rms) > -60.0, "{} dBFS", db(rms));
}
