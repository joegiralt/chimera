//! The reverb's dispatcher (reverb TYPEs plan, Task 4): QUAD through
//! `Reverb` is bit for bit what `RingReverb` was. The goldens were recorded
//! from `RingReverb` (695a6ef3), before the arena moved out of the ring.
//! Task 5: the sweep walked GRIT; it walks SMEAR now (re-recorded), and at
//! SMEAR 0 it is the T4 reference (a2abe82f) at GRIT 0, bit for bit.

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::rev::{EngineState, QuadState, Reverb};
use chimera_core::dsp::reverb::ReverbParams;
use chimera_hal::BLOCK_SIZE;
use common::rev::Noise;
use common::{SR, fnv1a};

const BLOCKS: usize = 900;

/// Block `b`'s settings: SIZE stepping every 37 blocks (some requests land
/// mid-fade, or wait on SMEAR), SMEAR (or 0), TIME and DAMP walking, MIX
/// ramping up, down to 0 and back.
fn sweep(b: usize, smear: bool) -> (ReverbParams, f32) {
    let t = b as f32 / BLOCKS as f32;
    let p = ReverbParams {
        smear: if smear { t } else { 0.0 },
        rate: 1.0,
        time: 0.2 + 0.7 * (1.0 - t),
        damping: (3.0 * t).fract(),
        size: [0.5, 1.0, 0.0, 0.8, 0.3, 0.9][(b / 37) % 6],
        mix: 0.5,
        ..Default::default()
    };
    let mix = match b {
        0..200 => 0.5,
        200..300 => 1.0,
        300..400 => 0.0,
        _ => 0.3,
    };
    (p, mix)
}

/// The send: noise bursts, the second hot enough to saturate the lines.
fn send(n: &mut Noise, b: usize) -> [f32; BLOCK_SIZE] {
    let g = match b {
        0..60 => 0.5,
        250..270 => 6.0,
        _ => 0.0,
    };
    core::array::from_fn(|_| g * n.next())
}

fn render(sample_rate: u32, tempo: Tempo, smear: bool) -> Vec<f32> {
    let mut rv = Box::new(Reverb::new());
    let mut n = Noise(0x1234_5678);
    let mut out = Vec::with_capacity(2 * BLOCKS * BLOCK_SIZE);
    for b in 0..BLOCKS {
        let x = send(&mut n, b);
        let (p, mix) = sweep(b, smear);
        let mut wet = Stereo::SILENT;
        rv.process(&x, &p, mix, tempo, sample_rate, &mut wet);
        out.extend_from_slice(&wet.l);
        out.extend_from_slice(&wet.r);
    }
    out
}

/// The whole bus, the delay feeding the reverb through REV SEND.
fn render_bus(smear: bool) -> Vec<f32> {
    let mut bus = Box::new(FxBus::new());
    let mut n = Noise(0x9abc_def0);
    let mut out = Vec::with_capacity(2 * BLOCKS * BLOCK_SIZE);
    for b in 0..BLOCKS {
        let mut fx = FxParams::default();
        (fx.reverb, _) = sweep(b, smear);
        fx.reverb.mix = if (300..400).contains(&b) { 0.0 } else { 0.4 };
        (fx.delay.time_ms, fx.delay.feedback, fx.delay.mix) = (180.0, 0.5, 0.3);
        fx.delay.rev_send = if b < 450 { 0.7 } else { 0.0 };
        let x = send(&mut n, b);
        let mut sends = [x; FX_SENDS];
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &fx, Tempo::DEFAULT, SR, &mut ret);
        out.extend_from_slice(&ret.l);
        out.extend_from_slice(&ret.r);
    }
    out
}

const GOLDENS: &[(&str, u64)] = &[
    ("quad_sweep_48k", 0x30ce15233183a0b4),  // SMEAR walking (T5)
    ("quad_sweep_44k1", 0x85b675fbe09af4bc), // SMEAR walking (T5)
    ("quad_bus", 0x1afee7d755033931),        // SMEAR walking (T5)
    ("quad_sweep_48k_smear_0", 0x384f412e20e5c6e5), // a2abe82f at GRIT 0
    ("quad_sweep_44k1_smear_0", 0x7b4cd2df097e08f8), // a2abe82f at GRIT 0
    ("quad_bus_smear_0", 0x07a6da7f4e2099ae), // a2abe82f at GRIT 0
];

#[test]
fn quad_is_bit_identical_through_the_dispatcher() {
    let renders = [
        ("quad_sweep_48k", render(SR, Tempo::DEFAULT, true)),
        ("quad_sweep_44k1", render(44_100, Tempo::DEFAULT, true)),
        ("quad_bus", render_bus(true)),
        ("quad_sweep_48k_smear_0", render(SR, Tempo::DEFAULT, false)),
        (
            "quad_sweep_44k1_smear_0",
            render(44_100, Tempo::DEFAULT, false),
        ),
        ("quad_bus_smear_0", render_bus(false)),
    ];
    for (name, out) in &renders {
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak.is_finite() && peak > 1e-3, "{name}: peak {peak}");
    }
    let got = renders.map(|(name, out)| (name, fnv1a(&out)));
    common::golden::check(GOLDENS, &got);
}

#[test]
fn zeroed_engine_state_is_quad_default() {
    // SAFETY: the test checks exactly this claim.
    let z: EngineState = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
    assert_eq!(z, EngineState::Quad(QuadState::default()));
}

#[test]
fn engine_state_fits() {
    // ROOM v2's four loops and GRAIN (ADR 0095): 112 B past ADR 0085's 160.
    assert!(core::mem::size_of::<EngineState>() <= 272);
}

#[test]
fn tempo_is_threaded_but_inert_for_quad() {
    let (slow, fast) = (
        render(SR, Tempo(60.0), true),
        render(SR, Tempo(240.0), true),
    );
    assert!(
        slow.iter()
            .zip(&fast)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn tempo_default_is_120_bpm_free_of_the_rate() {
    assert_eq!(Tempo::DEFAULT, Tempo(120.0));
    assert_eq!(Tempo::DEFAULT.beat_ms(), 500.0);
}
