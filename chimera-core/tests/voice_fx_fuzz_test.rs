// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! FUZZ (ADR 0156): its aliasing at high notes, the tone stack's scoop, a
//! SUSTAIN sweep's ramp, and a hot 2040 after it.

mod common;

use chimera_core::dsp::algo::engine::AlgoLive;
use chimera_core::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::modal::SymPool;
use chimera_core::dsp::voice_fx::fuzz::{Aa, Fuzz};
use chimera_core::dsp::voice_fx::params::{FxType, Place, VoiceFxParams};
use chimera_core::dsp::voice_fx::{Ends, VoiceFx};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::taps::TapRows;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::spectrum::alias_db;
use common::tape::{OneSource, held_plain, sine};
use common::{Rig, SR, VEL, XorShift, assert_stable, clicks, goertzel};

const NOON: [f32; 3] = [0.5; 3];

fn held(pots: [f32; 3]) -> Ends {
    Ends {
        pots: pots.map(|p| (p, p)),
        mix: (1.0, 1.0),
        gate: (1.0, 1.0),
    }
}

/// `x` through a fresh `Fuzz`, a block at a time, `ends(b)` for block `b`.
fn fuzz(x: &[f32], aa: Aa, glide: bool, ends: impl Fn(usize) -> Ends) -> Vec<f32> {
    let mut f = Fuzz::new();
    let mut out = Vec::with_capacity(x.len());
    for (b, chunk) in x.as_chunks::<BLOCK_SIZE>().0.iter().enumerate() {
        let mut buf = *chunk;
        f.run_with(&mut buf, &ends(b), glide, aa);
        out.extend_from_slice(&buf);
    }
    out
}

/// A full-scale sine, phase in f64.
fn tone(hz: f64, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(SR)).sin() as f32)
        .collect()
}

#[test]
fn fuzz_aliasing_floor_at_high_notes() {
    let n = (1 << 12) + (1 << 15);
    for f0 in [1760.0f32, 2093.0] {
        let x = tone(f64::from(f0), n);
        let on = alias_db(&fuzz(&x, Aa::On, false, |_| held(NOON)), SR as f32, &[f0]);
        let off = alias_db(&fuzz(&x, Aa::Off, false, |_| held(NOON)), SR as f32, &[f0]);
        println!("{f0} Hz: ADAA {on:.1} dB, plain {off:.1} dB");
        assert!(on <= -50.0, "{f0} Hz: {on:.1} dB");
        assert!(on <= off - 12.0, "{f0} Hz: {on:.1} vs plain {off:.1} dB");
    }
}

/// The band at `hz`, dB: Goertzel's power averaged over 4,096-sample
/// chunks of noise.
fn band_db(x: &[f32], hz: f32) -> f32 {
    let chunks = x.as_chunks::<4096>().0;
    let p = chunks
        .iter()
        .map(|c| goertzel(c, hz, SR).powi(2))
        .sum::<f32>()
        / chunks.len() as f32;
    10.0 * p.log10()
}

/// The tone stack's own scoop at TONE 0.5, clippers bypassed (`tone_only`):
/// white noise through it, the 1 kHz band against the dB mean of the 200 Hz
/// and 4 kHz bands. At least 3 dB: the threshold follows the circuit, not the
/// plan's 6 dB (ADR 0156). The blend `(1 - t)·LP723 + t·HP1855` is 3.7 dB
/// under that mean at 1 kHz (3.5 measured) and 3.95 dB at its notch, 1.2 kHz.
#[test]
fn fuzz_tone_scoops_the_mids_at_noon() {
    let mut r = XorShift(0x9e37_79b9_7f4a_7c15);
    let mut x: Vec<f32> = (0..1 << 18)
        .map(|_| r.next_u32() as f32 / u32::MAX as f32 * 2.0 - 1.0)
        .collect();
    Fuzz::new().tone_only(&mut x, 0.5);
    let (lo, mid, hi) = (band_db(&x, 200.0), band_db(&x, 1000.0), band_db(&x, 4000.0));
    let edges = (lo + hi) / 2.0;
    println!("200 Hz {lo:.1}, 1 kHz {mid:.1}, 4 kHz {hi:.1}, edges {edges:.1} dB");
    assert!(mid <= edges - 3.0, "scoop {:.1} dB", edges - mid);
}

#[test]
fn fuzz_sustain_ramps_click_free() {
    let x: Vec<f32> = (0..60).flat_map(|b| sine_block(b, 0.5, 110.0)).collect();
    for glide in [false, true] {
        let out = fuzz(&x, Aa::On, glide, |b| {
            let s = match b {
                0..30 => (0.0, 0.0),
                30 => (0.0, 1.0),
                _ => (1.0, 1.0),
            };
            Ends {
                pots: [s, (0.5, 0.5), (0.5, 0.5)],
                ..held(NOON)
            }
        });
        assert!(
            clicks(&out).is_empty(),
            "glide {glide}: {:?}",
            &clicks(&out)[..1]
        );
    }
}

fn sine_block(b: usize, amp: f32, hz: f32) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| {
        let t = (b * BLOCK_SIZE + i) as f32 / SR as f32;
        amp * (std::f32::consts::TAU * hz * t).sin()
    })
}

const CHORD: [u8; 3] = [60, 64, 67];

/// VA INIT on the 2040 at INPUT LEVEL 1, FUZZ PRE with every pot at 1.
fn hot() -> ParamSnapshot {
    let mut p = Sound::init(EngineType::Va).params;
    p.filter.set_kind(FilterKind::K2040);
    p.filter.level = 1.0;
    p.fx.set_type(FxType::Fuzz);
    (p.fx.pots, p.fx.mix) = ([1.0; 3], 1.0);
    p
}

/// `p`'s engine for `note`, then the slot PRE: the 2040's input, `blocks`
/// long.
fn into_the_filter(p: &ParamSnapshot, note: u8, blocks: usize) -> Vec<f32> {
    let mut src = OneSource::new(&held_plain(
        &sine(48_000, 48_000.0 / 261.63, 96_000),
        48_000,
    ));
    let id = Some(src.id);
    let mut pool = SymPool::boxed();
    let mut e = EngineSlot::new(SlotKind::of(p));
    let live = AlgoLive::from_params(&p.algo);
    let (mut slot, scale) = (VoiceFx::new(), p.engine().fx_scale());
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    for b in 0..blocks {
        let view = src.view();
        let mut c = EngineCtx {
            sym: &mut pool,
            src: &view,
            part: chimera_core::project::PartId::ALL[0],
            tune: 0.0,
            streams: None,
        };
        if b == 0 {
            let (n, v) = (MidiNote::new(note).unwrap(), Velocity::new(VEL).unwrap());
            e.note_on(n, v, p, SR, id, &mut c);
        }
        let mut buf = [0.0; BLOCK_SIZE];
        e.render(
            &mut buf,
            p,
            &live,
            SR,
            &mut c,
            &mut TapRows::new([false; 2]),
        );
        let blk = slot.step(&p.fx, true);
        slot.apply(&blk, Place::Pre, &mut buf, scale, false, None);
        out.extend_from_slice(&buf);
    }
    out
}

/// Each of the chord's notes, a voice each, held 2 s of a 12 s render.
fn chord(p: &ParamSnapshot) -> Vec<Vec<f32>> {
    let held_blocks = 2 * SR as usize / BLOCK_SIZE;
    CHORD
        .iter()
        .map(|&n| {
            let mut v = Rig::new(SR);
            v.note_on(MidiNote::new(n).unwrap(), Velocity::new(VEL).unwrap(), p);
            let mut out = vec![0.0f32; 12 * SR as usize];
            for (b, chunk) in out.as_chunks_mut::<BLOCK_SIZE>().0.iter_mut().enumerate() {
                if b == held_blocks {
                    v.note_off();
                }
                let mut block = [0.0f32; BLOCK_SIZE];
                v.render(&mut block, p, &ModState::new());
                chunk.copy_from_slice(&block);
            }
            out
        })
        .collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// (RF 4) The 2040 at INPUT LEVEL 1 after FUZZ at full SUSTAIN, TONE and
/// VOLUME: each voice of a chord held 2 s stays finite, bounded and dies
/// to silence; the slot hands the 2040 no more than 4 × the engine's
/// reference (VOLUME's ×4 top is by design); and the chord is no hotter than the 2040 alone at INPUT 1
/// (which on its own peaks near 7.7, over 4, as three voices summed).
#[test]
fn fuzz_into_a_hot_2040_stays_bounded() {
    let p = hot();
    let voices = chord(&p);
    for (v, n) in voices.iter().zip(CHORD) {
        assert_stable(v, 4.0, 0.0, &format!("FUZZ into the 2040, note {n}"));
    }
    let bound = 4.0 / p.engine().fx_scale();
    for &n in &CHORD {
        let x = into_the_filter(&p, n, 2 * SR as usize / BLOCK_SIZE);
        println!("note {n}: the 2040's input peaks at {:.3}", peak(&x));
        assert!(peak(&x) <= bound, "note {n}: {} > {bound}", peak(&x));
    }
    let sum = |vs: &[Vec<f32>]| -> Vec<f32> {
        (0..vs[0].len())
            .map(|i| vs.iter().map(|v| v[i]).sum())
            .collect()
    };
    let mut off = p.clone();
    off.fx.set_type(FxType::Off);
    let (fz, dry) = (peak(&sum(&voices)), peak(&sum(&chord(&off))));
    println!("chord peaks: FUZZ {fz:.3}, the 2040 alone {dry:.3}");
    assert!(fz <= 1.1 * dry, "{fz} vs {dry}");
}

fn rms_db(x: &[f32]) -> f32 {
    10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
}

/// At SUSTAIN, TONE and VOLUME 1/2 the wet is as loud as the dry, within
/// 1.75 dB (VA INIT, C3, into the slot PRE). The plan's 1.5 dB and RF 4's
/// bound of 4 can't both hold: MAKE_UP 0.092 peaks at 3.99 and sits 1.6 dB
/// under. RF 4's bound wins; Task 14 makes the final pass.
#[test]
fn fuzz_at_noon_is_as_loud_as_the_dry() {
    let mut p = Sound::init(EngineType::Va).params;
    p.fx.set_type(FxType::Fuzz);
    p.fx.pots = NOON;
    let blocks = 2 * SR as usize / BLOCK_SIZE;
    p.fx.mix = 1.0;
    let wet = rms_db(&into_the_filter(&p, 60, blocks));
    p.fx.mix = 0.0;
    let dry = rms_db(&into_the_filter(&p, 60, blocks));
    println!("wet {wet:.2} dB, dry {dry:.2} dB, gap {:.2}", wet - dry);
    assert!((wet - dry).abs() <= 1.75, "wet {wet:.1} vs dry {dry:.1} dB");
}

/// FUZZ in the slot is the kernel: a whole side at scale 1, PRE.
#[test]
fn the_slot_runs_fuzz_pre_from_off() {
    let mut q = VoiceFxParams::default();
    q.set_type(FxType::Fuzz);
    assert_eq!(q.place, Place::Pre);
    assert!(FxType::Fuzz.runs(0.0, false));
}
