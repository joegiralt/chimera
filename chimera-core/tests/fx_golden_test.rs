//! FX refactor lock (ADR 0011, ADR 0014): chorus, delay and reverb output
//! frozen bit-for-bit before their buffers were trimmed to what their ranges
//! need. Re-record only for an intended sound change (`common::golden`).

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, RevSync, ReverbParams, ReverbType};
use chimera_hal::BLOCK_SIZE;
use common::{SR, fnv1a};

/// 50 blocks of a 110 Hz saw at 0.5, then silence; 450 blocks in all so a
/// 500 ms delay repeats at least once.
const BURST_BLOCKS: usize = 50;
const FX_BLOCKS: usize = 450;

#[derive(Clone, Copy)]
enum Fx {
    Chorus(ChorusParams),
    Delay(DelayParams),
    Reverb(ReverbParams),
}

/// ROOM at each MidiVerb II program's knobs, fitted on the Rust render
/// by the reference harness (ADR 0095): (program, its golden's name,
/// SIZE, DECAY, TONE, DIFFUSE, PRE-DLY, DAMP, LOOPS), SMEAR 0. Only the
/// programs whose fit passes.
const MV2_ROOM: &[(u8, &str, [f32; 7])] = &[
    (
        1,
        "rev_room_mv01",
        [0.42, 0.189, 0.036, 0.0, 0.0, 0.923, 0.0],
    ), // Small Bright .1
    (
        2,
        "rev_room_mv02",
        [0.924, 0.495, 0.024, 0.0, 0.073, 0.695, 0.0],
    ), // Small Bright .2
    (
        3,
        "rev_room_mv03",
        [0.847, 0.534, 0.034, 0.026, 0.088, 0.611, 0.161],
    ), // Small Bright .3
    (
        5,
        "rev_room_mv05",
        [0.644, 0.718, 0.0, 0.0, 0.085, 0.426, 0.634],
    ), // Medium Bright .6
    (
        8,
        "rev_room_mv08",
        [0.615, 0.708, 0.91, 0.0, 0.085, 0.402, 0.796],
    ), // Medium Dark .6
    (
        9,
        "rev_room_mv09",
        [0.44, 0.736, 0.937, 1.0, 0.079, 0.0, 1.0],
    ), // Medium Dark .5
    (
        10,
        "rev_room_mv10",
        [0.729, 0.756, 0.03, 1.0, 0.075, 0.0, 0.636],
    ), // Medium Bright .6
    (
        11,
        "rev_room_mv11",
        [0.847, 0.791, 0.0, 0.57, 0.092, 0.25, 0.863],
    ), // Medium Bright .8
    (
        14,
        "rev_room_mv14",
        [0.739, 0.897, 0.982, 0.745, 0.078, 0.0, 0.39],
    ), // Medium Dark 1.0
];

/// BLOOM at each MidiVerb II program's knobs, fitted on the Rust render by
/// the reference harness (ADR 0095): (program, its golden's name, SWELL,
/// DECAY, TONE, DIFFUSE, PRE-DLY), SMEAR 0. One point for both, as the
/// unit's: only PRE-DLY differs, and each passes a step off on every knob.
const MV2_BLOOM: &[(u8, &str, [f32; 5])] = &[
    (45, "rev_bloom_mv45", [0.705, 0.948, 0.578, 0.165, 0.132]), // Bloom 1 8 Sec
    (49, "rev_bloom_mv49", [0.705, 0.948, 0.578, 0.165, 0.815]), // Bloom 2 7 Sec
];

fn cases() -> Vec<(&'static str, Fx)> {
    let reverb = |smear: f32, size: f32| ReverbParams {
        time: 0.7,
        damping: 0.3,
        size,
        mix: 0.5,
        smear,
        rate: 1.0,
        ..Default::default()
    };
    // A TYPE on its starting knobs, `f` tweaking them. MIX 0.3: at QUAD's
    // loudness a burst, and REGEN's, peak over 1.0 at 0.5.
    let burst = |t: ReverbType, f: fn(&mut ReverbParams)| {
        let mut p = ReverbParams {
            mix: 0.3,
            ..Default::default()
        }
        .with_type(t, &KnobMemory::new());
        f(&mut p);
        Fx::Reverb(p)
    };
    let mut v = vec![
        (
            "chorus_both",
            Fx::Chorus(ChorusParams {
                mode: 3,
                rate: 0.5,
                depth: 0.5,
                mix: 0.5,
            }),
        ),
        (
            "delay_375ms",
            Fx::Delay(DelayParams {
                feedback: 0.6,
                mix: 0.5,
                ..DelayParams::default()
            }),
        ),
        (
            "delay_500ms",
            Fx::Delay(DelayParams {
                time_ms: 500.0,
                feedback: 0.6,
                wow_flutter: 1.0,
                mix: 0.5,
                ..DelayParams::default()
            }),
        ),
        // Five saturated repeats in the render: the feedback loop's lock.
        (
            "delay_feedback_100ms",
            Fx::Delay(DelayParams {
                time_ms: 100.0,
                feedback: 0.6,
                mix: 0.5,
                ..DelayParams::default()
            }),
        ),
        ("reverb_ring", Fx::Reverb(reverb(0.0, 0.5))),
        ("reverb_ring_max_size", Fx::Reverb(reverb(0.0, 1.0))),
        ("reverb_ring_smear", Fx::Reverb(reverb(1.0, 0.5))),
        ("rev_room", burst(ReverbType::Room, |_| {})),
        ("rev_hall", burst(ReverbType::Hall, |_| {})),
        // The plate corner (reverb TYPEs spec § PLATE is a zone of HALL).
        (
            "rev_hall_plate",
            burst(ReverbType::Hall, |p| {
                (
                    p.size, p.time, p.tone, p.diffuse, p.damping, p.predelay, p.smear, p.rate,
                ) = (0.0, 0.35, 0.0, 1.0, 0.1, 0.0, 0.5, 0.6)
            }),
        ),
        ("rev_gate", burst(ReverbType::Gate, |_| {})),
        // Fast Gate: a hard cut, no TAIL.
        (
            "rev_gate_flat_tail0",
            burst(ReverbType::Gate, |p| (p.time, p.shape) = (0.0, 0.0)),
        ),
        // LENGTH synced to 1/8 at 120 BPM: 250 ms (reverb TYPEs T12).
        (
            "rev_gate_sync_8th_120",
            burst(ReverbType::Gate, |p| {
                (p.sync, p.size) = (RevSync::Time, 0.7)
            }),
        ),
        ("rev_reverse", burst(ReverbType::Reverse, |_| {})),
        ("rev_bloom", burst(ReverbType::Bloom, |_| {})),
        (
            "rev_reverse_regen",
            burst(ReverbType::Reverse, |p| p.time = 0.7),
        ),
    ];
    for &(_, name, k) in MV2_ROOM {
        let mut p = ReverbParams {
            mix: 0.3,
            ..Default::default()
        }
        .with_type(ReverbType::Room, &KnobMemory::new());
        (
            p.size, p.time, p.tone, p.diffuse, p.predelay, p.damping, p.shape, p.smear,
        ) = (k[0], k[1], k[2], k[3], k[4], k[5], k[6], 0.0);
        v.push((name, Fx::Reverb(p)));
    }
    for &(_, name, k) in MV2_BLOOM {
        let mut p = ReverbParams {
            mix: 0.3,
            ..Default::default()
        }
        .with_type(ReverbType::Bloom, &KnobMemory::new());
        (p.size, p.time, p.tone, p.diffuse, p.predelay, p.smear) =
            (k[0], k[1], k[2], k[3], k[4], 0.0);
        v.push((name, Fx::Reverb(p)));
    }
    v
}

/// Block `b` of the input signal; `phase` carries the saw between blocks.
fn input(b: usize, phase: &mut f32) -> [f32; BLOCK_SIZE] {
    let mut block = [0.0f32; BLOCK_SIZE];
    if b < BURST_BLOCKS {
        for s in block.iter_mut() {
            *s = *phase - 0.5;
            *phase = (*phase + 110.0 / SR as f32).fract();
        }
    }
    block
}

/// Stereo effects add L then R per block; the delay its insert output.
fn render(fx: Fx) -> Vec<f32> {
    let mut chorus = Box::new(JunoChorus::new());
    let mut delay = Box::new(TapeDelay::new());
    let mut reverb = Box::new(match fx {
        Fx::Reverb(p) => Reverb::of_type(p.kind),
        _ => Reverb::new(),
    });
    let mut out = Vec::with_capacity(2 * FX_BLOCKS * BLOCK_SIZE);
    let mut phase = 0.0f32;
    for b in 0..FX_BLOCKS {
        let mut block = input(b, &mut phase);
        match fx {
            Fx::Chorus(p) => {
                let mut wet = Stereo::SILENT;
                chorus.process_wet(&block, &p, SR, &mut wet);
                out.extend_from_slice(&wet.l);
                out.extend_from_slice(&wet.r);
            }
            Fx::Delay(p) => {
                delay.process(&mut block, &p, SR);
                out.extend_from_slice(&block);
            }
            Fx::Reverb(p) => {
                let mut wet = Stereo::SILENT;
                reverb.process(&block, &p, p.mix, Tempo::DEFAULT, SR, &mut wet);
                out.extend_from_slice(&wet.l);
                out.extend_from_slice(&wet.r);
            }
        }
    }
    out
}

const GOLDENS: &[(&str, u64)] = &[
    ("chorus_both", 0x666b43969f8ed31a),          // u32 LFO phase
    ("delay_375ms", 0x036a3a3f29f429c6),          // MECHANICS (ADR 0053)
    ("delay_500ms", 0x40eb96efc0f4fefe),          // MECHANICS (ADR 0053)
    ("delay_feedback_100ms", 0x07e67081e5f28449), // SAT × 1/gain, not ÷ gain (ADR 0053)
    // GRIT out (reverb TYPEs T5): SMEAR 0 is the T4 reference's GRIT 0.
    ("reverb_ring", 0xd17b7769e13816bf),
    ("reverb_ring_max_size", 0x83591bf24cab49f6),
    ("reverb_ring_smear", 0xa7e01c103cc02150),
    // ROOM v2 (ADR 0095, reverb TYPEs T9b), on program 5's knobs; HALL on
    // the LOOP engine (T10).
    ("rev_room", 0x97b4651629782f42),
    // ROOM v2 at each MidiVerb II program's fitted knobs (`MV2_ROOM`).
    ("rev_room_mv01", 0x60df4ce32d30852d),
    ("rev_room_mv02", 0x2262abda362a7850),
    ("rev_room_mv03", 0xbf5c053012b74e28),
    ("rev_room_mv05", 0x97b4651629782f42),
    ("rev_room_mv08", 0x24116176210c0052),
    ("rev_room_mv09", 0xfdff8543b7be57e0),
    ("rev_room_mv10", 0x85a254c66db5f2c7),
    ("rev_room_mv11", 0x193920776227df0d),
    ("rev_room_mv14", 0x7f3146738ce901c8),
    ("rev_hall", 0x6d09691da61c04c5),
    ("rev_hall_plate", 0xcc09501b70f66688),
    // GATE and REVERSE on the BURST engine (reverb TYPEs T8).
    ("rev_gate", 0xf58be0a4e64aeddf),
    ("rev_gate_flat_tail0", 0x951817e3e86edf28),
    // LENGTH synced (reverb TYPEs T12).
    ("rev_gate_sync_8th_120", 0xfdf822a972a4be23),
    ("rev_reverse", 0x6556c8e280534d44),
    ("rev_reverse_regen", 0x510ced0081cb2515),
    // BLOOM (ADR 0095, reverb TYPEs T11) on program 45's knobs, and at
    // each MidiVerb II Bloom's fitted knobs (`MV2_BLOOM`).
    ("rev_bloom", 0xedc341b56f20614f),
    ("rev_bloom_mv45", 0xd80822fb3d18e458),
    ("rev_bloom_mv49", 0x2014f5741eb7a296),
];

#[test]
fn fx_goldens_match() {
    let got: Vec<_> = cases()
        .into_iter()
        .map(|(name, fx)| (name, fnv1a(&render(fx))))
        .collect();
    common::golden::check(GOLDENS, &got);
}

/// ADR 0011's gate for the FX cases, run before their goldens are
/// recorded: finite, within ±1.0, and audible.
#[test]
fn fx_cases_pass_the_sanity_gate() {
    for (name, fx) in cases() {
        let out = render(fx);
        assert!(out.iter().all(|s| s.is_finite()), "{name}: non-finite");
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((1e-3..=1.0).contains(&peak), "{name}: peak {peak}");
    }
}

/// A case whose effect is bypassed would lock only the dry input.
#[test]
fn fx_cases_are_not_dry() {
    let mut phase = 0.0f32;
    let dry: Vec<f32> = (0..FX_BLOCKS).flat_map(|b| input(b, &mut phase)).collect();
    for (name, fx) in cases() {
        assert_ne!(fnv1a(&render(fx)), fnv1a(&dry), "{name} is bypassed");
    }
}
