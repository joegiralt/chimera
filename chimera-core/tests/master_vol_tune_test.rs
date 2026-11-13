//! MST > LVL's VOL and PAN and SETTINGS > TUNING (#438): the master
//! gains sit after the limiter and ease; TUNING moves every engine's pitch.

mod common;

use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::comp::CompParams;
use chimera_core::dsp::fx_bus::{FxBus, FxParams};
use chimera_core::dsp::limiter::{CEILING, DacBlocks, Limiter};
use chimera_core::dsp::master_out::{balance, vol_gain};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::storage::{decode_block, encode_block};
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{InstRig, Rig};

const SR: u32 = 48_000;

fn fx_with(vol: f32, pan: f32) -> FxParams {
    FxParams {
        comp: CompParams {
            vol,
            pan,
            ..CompParams::default()
        },
        ..FxParams::default()
    }
}

/// A sine of `amp` on every pair and side, block `b`.
fn feed(dac: &mut DacBlocks, amp: f32, b: usize) {
    for pair in dac.mix().iter_mut() {
        for (i, s) in pair.iter_mut().enumerate() {
            let n = (b * BLOCK_SIZE + i / 2) as f32;
            *s = amp * libm::sinf(n * 0.05);
        }
    }
}

fn run(fx: &FxParams, amp: f32, blocks: usize) -> Vec<[[f32; 2 * BLOCK_SIZE]; 3]> {
    let mut bus = FxBus::new();
    let mut dac = DacBlocks::new();
    (0..blocks)
        .map(|b| {
            feed(&mut dac, amp, b);
            bus.limit(&mut dac, fx, SR);
            *dac.out()
        })
        .collect()
}

#[test]
fn vol_runs_from_silence_to_unity_on_a_square() {
    assert_eq!(vol_gain(0.0), 0.0);
    assert_eq!(vol_gain(1.0), 1.0);
    assert_eq!(vol_gain(0.5), 0.25, "-12 dB at half travel");
    assert_eq!(vol_gain(f32::NAN), 1.0);
    assert_eq!(vol_gain(7.0), 1.0, "never past unity: the ceiling holds");
    let mut last = 0.0;
    for i in 0..=128 {
        let g = vol_gain(i as f32 / 128.0);
        assert!(g >= last);
        last = g;
    }
}

#[test]
fn pan_is_a_unity_centre_balance_with_a_cosine_far_side() {
    assert_eq!(balance(0.0), (1.0, 1.0));
    assert_eq!(balance(1.0), (0.0, 1.0));
    assert_eq!(balance(-1.0), (1.0, 0.0));
    assert_eq!(balance(f32::NAN), (1.0, 1.0));
    for i in 1..=100 {
        let p = i as f32 / 100.0;
        let (l, r) = balance(p);
        assert_eq!(r, 1.0);
        assert!((l - libm::cosf(p * core::f32::consts::FRAC_PI_2)).abs() < 1e-6 || l == 0.0);
        assert!((0.0..1.0).contains(&l));
        assert_eq!(balance(-p), (r, l), "mirror");
    }
}

#[test]
fn defaults_are_bit_identical_to_the_limiter_alone() {
    let mut lim = Limiter::new();
    let mut dac = DacBlocks::new();
    let mut want = Vec::new();
    for b in 0..40 {
        feed(&mut dac, if b < 20 { 0.3 } else { 1.4 }, b);
        lim.process(&mut dac, SR);
        want.push(*dac.out());
    }
    let got = run(&FxParams::default(), 0.0, 0);
    assert!(got.is_empty());
    let mut bus = FxBus::new();
    let mut dac = DacBlocks::new();
    for (b, w) in want.iter().enumerate() {
        feed(&mut dac, if b < 20 { 0.3 } else { 1.4 }, b);
        bus.limit(&mut dac, &FxParams::default(), SR);
        let bits = |x: &[[f32; 2 * BLOCK_SIZE]; 3]| x.map(|p| p.map(f32::to_bits));
        assert_eq!(bits(dac.out()), bits(w), "block {b}");
    }
}

#[test]
fn vol_leaves_the_limiter_alone_and_scales_what_it_puts_out() {
    // Hot enough to be limited: the knee works the same at any VOL.
    let unity = run(&fx_with(1.0, 0.0), 1.2, 60);
    for vol in [0.25f32, 0.5, 0.8] {
        let low = run(&fx_with(vol, 0.0), 1.2, 60);
        let g = vol_gain(vol);
        for (a, b) in unity[59].iter().zip(&low[59]) {
            for (x, y) in a.iter().zip(b) {
                assert!((x * g - y).abs() <= 1e-6, "{vol}: {x} {y}");
            }
        }
    }
}

#[test]
fn vol_and_pan_never_pass_the_ceiling() {
    for (vol, pan) in [(1.0, 0.0), (1.0, 0.7), (0.9, -1.0), (1.0, 1.0)] {
        for blk in run(&fx_with(vol, pan), 3.0, 80) {
            assert!(blk.iter().flatten().all(|s| s.abs() <= CEILING));
        }
    }
}

#[test]
fn vol_and_pan_ease_never_snap() {
    // A DC-ish input through the stage: VOL 1 -> 0.2 and PAN centre -> hard right.
    let mut bus = FxBus::new();
    let mut dac = DacBlocks::new();
    let mut trace = Vec::new();
    for b in 0..400 {
        for pair in dac.mix().iter_mut() {
            pair.fill(0.2);
        }
        let fx = if b < 20 {
            fx_with(1.0, 0.0)
        } else {
            fx_with(0.2, 1.0)
        };
        bus.limit(&mut dac, &fx, SR);
        if b >= 2 {
            trace.extend(dac.out()[0].chunks(2).map(|s| (s[0], s[1])));
        }
    }
    let first_move = trace
        .windows(2)
        .position(|w| w[0] != w[1])
        .expect("the gain moved");
    let after = &trace[first_move..];
    let step_l = after.windows(2).map(|w| (w[1].0 - w[0].0).abs());
    let step_r = after.windows(2).map(|w| (w[1].1 - w[0].1).abs());
    assert!(step_l.chain(step_r).fold(0.0, f32::max) < 0.002, "no step");
    let (l, r) = *trace.last().unwrap();
    let want = 0.2 * 1.25 * vol_gain(0.2);
    assert_eq!(l, 0.0, "hard right: left exactly off");
    assert!((r - want).abs() < 1e-6, "{r} {want}");
}

fn rig_pitch_cents(engine: EngineType, tune: f32) -> f64 {
    const N: usize = 1 << 17;
    let params = ParamSnapshot::for_engine(engine);
    let mods = ModState::new();
    let mut rig = Rig::new(SR);
    rig.tune = tune;
    rig.note_on(
        MidiNote::new(48).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );
    let mut x = Vec::with_capacity(N);
    let mut block = [0.0f32; BLOCK_SIZE];
    while x.len() < N {
        rig.render(&mut block, &params, &mods);
        x.extend_from_slice(&block);
    }
    let f = common::spectrum::peak_hz(&x, SR as f32, f0(engine));
    1200.0 * (f / f64::from(f0(engine))).log2()
}

/// Where each engine's strongest partial sits at note 48 untuned: C3, but
/// TAPE's saw source sounds an octave under its 220 Hz at this key.
fn f0(e: EngineType) -> f32 {
    if e == EngineType::Tape { 110.0 } else { 130.81 }
}

#[test]
fn tuning_moves_every_engines_pitch_by_the_stated_amount() {
    // DRUM plays pads, not notes: `drum_engine_test`.
    for engine in EngineType::ALL
        .into_iter()
        .filter(|&e| e != EngineType::Drum)
    {
        let base = rig_pitch_cents(engine, 0.0);
        for tune in [1.0f32, -0.5, 0.17] {
            let moved = rig_pitch_cents(engine, tune) - base;
            let want = f64::from(tune) * 100.0;
            assert!(
                (moved - want).abs() < 3.0,
                "{engine:?} tune {tune}: moved {moved:.1} cents, want {want}"
            );
        }
    }
}

#[test]
fn tuning_reaches_a_live_instrument_and_eases_in() {
    let hz = |rig: &mut InstRig, blocks: usize| {
        let mut x = Vec::new();
        for _ in 0..blocks {
            rig.render(1);
            x.extend(rig.out()[0].chunks(2).map(|s| s[0]));
        }
        x
    };
    let pitch = |x: &[f32]| {
        let n = x.len().min(1 << 17);
        let mut y = x.to_vec();
        y.resize(1 << 17, 0.0);
        let _ = n;
        1200.0 * (common::spectrum::peak_hz(&y, SR as f32, 130.81) / 130.81f64).log2()
    };
    let mut rig = InstRig::with_budget(chimera_core::hw::SampleBudget::for_cpu(
        chimera_core::hw::CPU_HZ_REV_V,
    ));
    rig.note_on(0, 48);
    hz(&mut rig, 100);
    let before = pitch(&hz(&mut rig, 512));
    rig.shared.tune = 100;
    let rise = hz(&mut rig, 16);
    let during = pitch(&rise);
    assert!(
        during > before + 1.0 && during < before + 99.0,
        "eased: {during}"
    );
    hz(&mut rig, 400);
    let after = pitch(&hz(&mut rig, 512));
    assert!((after - before - 100.0).abs() < 4.0, "{before} {after}");
}

#[test]
fn tune_zero_is_the_untuned_bits() {
    let params = ParamSnapshot::for_engine(EngineType::Algo);
    let mods = ModState::new();
    let go = |tune: f32| {
        let mut rig = Rig::new(SR);
        rig.tune = tune;
        rig.note_on(
            MidiNote::new(60).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );
        let mut out = [0.0f32; BLOCK_SIZE];
        rig.render(&mut out, &params, &mods);
        out.map(f32::to_bits)
    };
    assert_eq!(go(0.0), go(-0.0));
    assert_ne!(go(0.0), go(0.5));
}

struct One(MidiSettings, CompParams);

impl chimera_core::addr::BlockRead for One {
    fn block(&self, b: BlockRef) -> Option<chimera_core::addr::BlockView<'_>> {
        match b {
            BlockRef::Midi => Some(chimera_core::addr::BlockView::Plain(&self.0)),
            BlockRef::Comp => Some(chimera_core::addr::BlockView::Plain(&self.1)),
            _ => None,
        }
    }
}

impl chimera_core::addr::Blocks for One {
    fn block_mut(&mut self, b: BlockRef) -> Option<chimera_core::addr::BlockViewMut<'_>> {
        match b {
            BlockRef::Midi => Some(chimera_core::addr::BlockViewMut::Plain(&mut self.0)),
            BlockRef::Comp => Some(chimera_core::addr::BlockViewMut::Plain(&mut self.1)),
            _ => None,
        }
    }
}

#[test]
fn tune_vol_and_pan_round_trip_through_the_block_codec() {
    use chimera_core::storage::RecordBuf;
    let sent = One(
        MidiSettings {
            tune: -73,
            ..MidiSettings::DEFAULT
        },
        CompParams {
            vol: 0.375,
            pan: -0.5,
            ..CompParams::default()
        },
    );
    let mut got = One(MidiSettings::DEFAULT, CompParams::default());
    for (b, blk) in [
        (BlockRef::Midi, &sent.0 as &dyn Block),
        (BlockRef::Comp, &sent.1 as &dyn Block),
    ] {
        let mut r = RecordBuf::new();
        encode_block(b, blk, None, &mut r);
        decode_block(r.as_slice(), &[], &[], Some(&mut got)).unwrap();
    }
    assert_eq!(got.0.tune, -73);
    assert_eq!((got.1.vol, got.1.pan), (0.375, -0.5));
    let _ = ParamId(0);
}

#[test]
fn defaults_are_unity_centre_and_concert_pitch() {
    let c = CompParams::default();
    assert_eq!((c.vol, c.pan), (1.0, 0.0));
    assert_eq!(MidiSettings::DEFAULT.tune, 0);
    assert_eq!(MidiSettings::DEFAULT.tune_semitones(), 0.0);
}
