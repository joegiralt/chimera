//! Gain staging (ADR 0050, #190): the voice sum is trimmed by 1/√8, and one
//! linked peak limiter at −1 dBFS is the only stage that bends the signal
//! before the DACs; the `to_dac` clamp is never reached.

use chimera_core::dsp::chorus::ChorusParams;
use chimera_core::dsp::comp::CompParams;
use chimera_core::dsp::delay::DelayParams;
use chimera_core::dsp::fx_bus::{FxBus, FxParams};
use chimera_core::dsp::limiter::{CEILING, LOOKAHEAD, Limiter};
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::dsp::tape::TapeParams;
use chimera_core::factory::factory_sound;
use chimera_core::hw::{CPU_HZ_REV_V, DAC_PAIRS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument, VOICE_SUM_TRIM, pan_gains};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

/// Two seconds.
const BLOCKS: usize = 1500;
const CHORD: [u8; 8] = [48, 55, 60, 64, 67, 71, 72, 76];

fn typical() -> FxParams {
    FxParams {
        chorus: ChorusParams {
            mode: 1,
            mix: 0.5,
            ..Default::default()
        },
        delay: DelayParams {
            mix: 0.5,
            ..Default::default()
        },
        reverb: ReverbParams {
            mix: 0.5,
            ..Default::default()
        },
        tape: TapeParams {
            mix: 1.0,
            drive: 0.5,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn max() -> FxParams {
    FxParams {
        chorus: ChorusParams {
            mode: 3,
            mix: 1.0,
            rate: 1.0,
            depth: 1.0,
        },
        delay: DelayParams {
            mix: 1.0,
            feedback: 1.0,
            saturation: 0.0,
            tone: 1.0,
            rev_send: 1.0,
            ..Default::default()
        },
        reverb: ReverbParams {
            mix: 1.0,
            time: 1.0,
            size: 1.0,
            damping: 0.0,
            grit: 0.3,
        },
        tape: TapeParams {
            mix: 1.0,
            drive: 1.0,
            ..Default::default()
        },
        comp: CompParams {
            ratio: 7,
            makeup: 1.0,
            ..Default::default()
        },
    }
}

/// Which Sound the chord plays.
#[derive(Clone, Copy, Debug)]
enum Voices {
    /// SAW LEAD, its filter wide open: two saw operators a voice.
    Saw,
    /// The default Sound, the hottest single voice.
    Init,
}

/// Eight voices of `voices` at velocity 127 on Part 1 at `level`, every
/// send at `send`, through `fx`; `each` sees every block after `render`.
fn chord(
    voices: Voices,
    fx_params: FxParams,
    level: f32,
    send: f32,
    mut each: impl FnMut(&Instrument, &FxBus, &DacOut),
) {
    let mut shared = Box::new(AudioShared::default());
    if let Voices::Saw = voices {
        let saw = factory_sound(4).unwrap();
        shared.parts[0].params = saw.params;
        shared.parts[0].params.filter.cutoff = 20_000.0;
        shared.parts[0].params.filter.resonance = 0.0;
        shared.parts[0].mod_state = saw.mod_state;
    }
    shared.parts[0].mix.level = level;
    shared.parts[0].mix.sends = [send; 3];
    shared.fx = fx_params;
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let mut fx = Box::new(FxBus::new());
    let (w, _r) = Box::leak(Box::new(scope_buffer())).split();
    let mut scope = ScopeWriter::new(w);
    for n in CHORD {
        inst.handle(
            NoteEvent {
                channel: shared.parts[0].mix.channel,
                note: MidiNote::new(n).unwrap(),
                kind: NoteKind::On(Velocity::new(127).unwrap()),
            },
            &shared,
        );
    }
    assert_eq!(
        inst.allocator().slots().iter().filter(|s| s.held()).count(),
        8
    );
    let mut dac: DacOut = [[0.0; 2 * BLOCK_SIZE]; DAC_PAIRS];
    for _ in 0..BLOCKS {
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        each(&inst, &fx, &dac);
    }
}

fn clamped(dac: &DacOut) -> usize {
    dac.iter().flatten().filter(|x| x.abs() >= 1.0).count()
}

/// #190: eight full-level saws, every LEVEL and send at 1, typical FX. The
/// pairs sum to at most full scale before the limiter, the reverb's ring
/// never sits on its i16 rail, and nothing reaches the final clamp.
#[test]
fn no_stage_exceeds_ceiling() {
    for voices in [Voices::Saw, Voices::Init] {
        let (mut pre, mut rails, mut over) = (0.0f32, 0usize, 0usize);
        chord(voices, typical(), 1.0, 1.0, |_, fx, dac| {
            let input = fx.limiter().input();
            pre = input.iter().flatten().fold(pre, |m, x| m.max(x.abs()));
            let lines = fx.reverb().ring().lines();
            rails += lines
                .iter()
                .filter(|&&v| v == i16::MAX || v == i16::MIN)
                .count();
            over += clamped(dac);
        });
        assert!(pre <= 1.0, "{voices:?}: pair peak before the limiter {pre}");
        assert_eq!(rails, 0, "{voices:?}: ring words on the i16 rail");
        assert_eq!(over, 0, "{voices:?}: samples clamped at the DAC");
    }
}

/// FX off: the output is each voice-sum times the fixed trims (trim, level,
/// pan), one lookahead late, bit for bit: superposition holds until the
/// limiter's detector passes its threshold, and only then does the output
/// bend, never past the ceiling.
#[test]
fn only_final_limiter_is_nonlinear() {
    let (gl, gr) = pan_gains(0.0);
    for level in [0.5, 1.0] {
        // The same products `mix_parts` hoists.
        let g = level * VOICE_SUM_TRIM;
        let (gl, gr) = (gl * g, gr * g);
        let (mut bus, mut out) = (Vec::new(), Vec::new());
        chord(
            Voices::Init,
            FxParams::default(),
            level,
            0.0,
            |inst, _, dac| {
                bus.extend_from_slice(inst.part_bus(0));
                out.extend_from_slice(&dac[0]);
            },
        );
        let over = bus
            .iter()
            .position(|&b| (b * gl).abs() > CEILING || (b * gr).abs() > CEILING);
        let linear = over.map_or(bus.len(), |i| i.saturating_sub(LOOKAHEAD));
        for i in LOOKAHEAD..linear {
            let b = bus[i - LOOKAHEAD];
            assert_eq!(out[2 * i].to_bits(), (b * gl).to_bits(), "L {i} at {level}");
            assert_eq!(
                out[2 * i + 1].to_bits(),
                (b * gr).to_bits(),
                "R {i} at {level}"
            );
        }
        assert!(out.iter().all(|x| x.abs() <= CEILING), "{level}");
        assert_eq!(
            over.is_some(),
            level == 1.0,
            "the limiter engages at {level} only"
        );
    }
}

/// Everything at its maximum, the compressor's makeup at +24 dB: the
/// output peaks at −1 dBFS at most and the final clamp never engages.
#[test]
fn max_settings_bounded() {
    for voices in [Voices::Saw, Voices::Init] {
        let (mut peak, mut over) = (0.0f32, 0usize);
        chord(voices, max(), 1.0, 1.0, |_, _, dac| {
            peak = dac.iter().flatten().fold(peak, |m, x| m.max(x.abs()));
            over += clamped(dac);
        });
        assert!(peak <= CEILING, "{voices:?}: peak {peak}");
        assert!(
            peak > 0.5 * CEILING,
            "{voices:?}: held near the ceiling, {peak}"
        );
        assert_eq!(over, 0, "{voices:?}");
    }
}

/// A −6 dBFS sine on every pair comes out bit for bit, one lookahead late.
#[test]
fn limiter_transparent_below_threshold() {
    let mut lim = Box::new(Limiter::new());
    let a = 0.501_187_2; // −6 dBFS
    let x = |n: usize| a * libm::sinf(n as f32 * 0.057_3);
    let mut out = [[0.0f32; 2 * BLOCK_SIZE]; DAC_PAIRS];
    for b in 0..200 {
        for (p, pair) in out.iter_mut().enumerate() {
            for i in 0..BLOCK_SIZE {
                let n = b * BLOCK_SIZE + i;
                pair[2 * i] = x(n) * (p + 1) as f32 / 3.0;
                pair[2 * i + 1] = -x(n + 7);
            }
        }
        lim.process(&mut out, SAMPLE_RATE);
        for (p, pair) in out.iter().enumerate() {
            for i in 0..BLOCK_SIZE {
                let n = b * BLOCK_SIZE + i;
                let (l, r) = match n.checked_sub(LOOKAHEAD) {
                    Some(m) => (x(m) * (p + 1) as f32 / 3.0, -x(m + 7)),
                    None => (0.0, 0.0),
                };
                assert_eq!(pair[2 * i].to_bits(), l.to_bits(), "pair {p} L {n}");
                assert_eq!(pair[2 * i + 1].to_bits(), r.to_bits(), "pair {p} R {n}");
            }
        }
    }
}

/// A lone +12 dBFS spike on one pair, then a step to +12 dBFS: the gain
/// is down before either leaves the lookahead, so no sample passes the
/// ceiling; afterwards it releases back to unity, bit for bit.
#[test]
fn limiter_catches_a_step_and_releases() {
    let mut lim = Box::new(Limiter::new());
    let mut out = [[0.0f32; 2 * BLOCK_SIZE]; DAC_PAIRS];
    let (mut peak, mut last) = (0.0f32, 0.0f32);
    for b in 0..1000 {
        let v = if (10..20).contains(&b) { 4.0 } else { 0.25 };
        for pair in out.iter_mut() {
            pair.fill(v);
        }
        if b == 5 {
            out[1][37] = -4.0;
        }
        lim.process(&mut out, SAMPLE_RATE);
        peak = out.iter().flatten().fold(peak, |m, x| m.max(x.abs()));
        last = out[0][2 * BLOCK_SIZE - 1];
    }
    assert!(peak <= CEILING, "{peak}");
    assert_eq!(last, 0.25, "released to unity");
}
