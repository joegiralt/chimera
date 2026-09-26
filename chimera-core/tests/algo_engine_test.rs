//! Spec § Rendering, § Plan and morph, § Testing: pitch, ends of the morph
//! bit-identical to each algorithm alone, equal loudness, nothing snaps.

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::{WaveId, mip_step};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn render_with(
    p: &AlgoParams,
    note: u8,
    vel: u8,
    blocks: usize,
    mut change: impl FnMut(usize, &mut AlgoParams),
) -> Vec<f32> {
    let mut e = AlgoEngine::new();
    let mut p = *p;
    e.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(vel).unwrap(),
        &p,
        SR,
    );
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..blocks {
        change(b, &mut p);
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        out.extend_from_slice(&blk);
    }
    out
}

fn render(p: &AlgoParams, note: u8, blocks: usize) -> Vec<f32> {
    render_with(p, note, 100, blocks, |_, _| {})
}

/// Six sounding operators at distinct integer ratios, each on its own wave.
fn stack(a: AlgoId, b: AlgoId, morph: u8) -> AlgoParams {
    let mut p = AlgoParams {
        alg_a: a.get(),
        alg_b: b.get(),
        morph,
        ..AlgoParams::default()
    };
    for (i, o) in p.ops.iter_mut().enumerate() {
        *o = AlgoOpParams {
            level: 99,
            coarse: [4, 8, 10, 13, 16, 19][i],
            wave: i as u8,
            feedback: 3,
            ..AlgoOpParams::default()
        };
    }
    p
}

fn sines(p: AlgoParams, level: u8) -> AlgoParams {
    let mut p = p;
    for o in p.ops.iter_mut() {
        (o.wave, o.feedback, o.level) = (WaveId::W1.get(), 0, level);
    }
    p
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

fn rms_db(s: &[f32]) -> f64 {
    10.0 * (s.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / s.len() as f64).log10()
}

fn hz(s: &[f32]) -> f64 {
    let ups: Vec<f64> = (1..s.len())
        .filter(|&i| s[i - 1] < 0.0 && s[i] >= 0.0)
        .map(|i| (i - 1) as f64 + (-s[i - 1] as f64) / ((s[i] - s[i - 1]) as f64))
        .collect();
    (ups.len() - 1) as f64 * SR as f64 / (ups[ups.len() - 1] - ups[0])
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// Largest sample-to-sample jump at block boundaries and elsewhere.
fn jumps(out: &[f32]) -> (f32, f32) {
    let (mut edge, mut inner) = (0.0f32, 0.0f32);
    for i in 1..out.len() {
        let j = (out[i] - out[i - 1]).abs();
        if i % BLOCK_SIZE == 0 {
            edge = edge.max(j)
        } else {
            inner = inner.max(j)
        }
    }
    (edge, inner)
}

#[test]
fn the_init_patch_plays_a4_within_a_cent() {
    let out = render(&AlgoParams::default(), 69, 750);
    let cents = 1200.0 * (hz(&out[4800..]) / 440.0).log2();
    assert!(cents.abs() < 1.0, "{cents} cents");
    assert!(peak(&out) <= 1.0);
}

#[test]
fn velocity_sensitivity_lowers_the_level() {
    let mut p = AlgoParams::default();
    p.ops[0].velocity = 7;
    let loud = rms_db(&render_with(&p, 60, 127, 50, |_, _| {})[640..]);
    let soft = rms_db(&render_with(&p, 60, 1, 50, |_, _| {})[640..]);
    let want = 7.0 * 3.0 * (1.0 - 1.0 / 127.0);
    assert!(((loud - soft) - want).abs() < 0.5, "{} dB", loud - soft);
}

#[test]
fn each_end_of_the_morph_is_bit_identical_to_its_algorithm_alone() {
    let a = render(&stack(AlgoId::A14, AlgoId::A22, 0), 60, 40);
    assert!(same_bits(
        &a,
        &render(&stack(AlgoId::A14, AlgoId::A14, 0), 60, 40)
    ));
    let b = render(&stack(AlgoId::A14, AlgoId::A22, 127), 60, 40);
    assert!(same_bits(
        &b,
        &render(&stack(AlgoId::A22, AlgoId::A22, 0), 60, 40)
    ));
    assert!(a.iter().zip(&b).any(|(x, y)| x != y));
}

#[test]
fn carrier_normalisation_keeps_a1_t1_and_a17_within_a_db() {
    let level: Vec<f64> = [AlgoId::A1, AlgoId::T1, AlgoId::A17]
        .iter()
        .map(|&a| rms_db(&render(&sines(stack(a, a, 0), 99), 57, 200)[3200..]))
        .collect();
    for l in &level {
        assert!((l - level[0]).abs() <= 1.0, "{level:?}");
    }
}

#[test]
fn a_morph_sweep_has_no_step_at_block_edges() {
    let base = sines(stack(AlgoId::T1, AlgoId::A17, 0), 80);
    let out = render_with(&base, 48, 100, 400, |b, p| p.morph = (b * 127 / 399) as u8);
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn a_level_change_ramps_instead_of_stepping() {
    let out = render_with(&AlgoParams::default(), 60, 100, 30, |b, p| {
        if b == 10 {
            p.ops[0].level = 40;
        }
    });
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn a_wave_change_ducks_swaps_and_returns() {
    let out = render_with(&AlgoParams::default(), 60, 100, 30, |b, p| {
        if b == 10 {
            p.ops[0].wave = WaveId::SQR.get();
        }
    });
    let blk = |b: usize| &out[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE];
    assert!(
        peak(&blk(10)[60..]) < 0.1 * peak(blk(9)),
        "ducked by the end of the block"
    );
    let edge = 11 * BLOCK_SIZE;
    assert!(
        (out[edge] - out[edge - 1]).abs() < 0.02,
        "the swap is silent"
    );
    assert!(peak(blk(13)) > 0.5 * peak(blk(9)), "back up");
}

#[test]
fn an_algorithm_change_ducks_the_whole_output() {
    let p = stack(AlgoId::T1, AlgoId::T1, 0);
    let out = render_with(&p, 60, 100, 30, |b, p| {
        if b == 10 {
            (p.alg_a, p.alg_b) = (AlgoId::A1.get(), AlgoId::A1.get());
        }
    });
    let blk = |b: usize| &out[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE];
    assert!(peak(&blk(10)[60..]) < 0.1 * peak(blk(9)));
    assert!(peak(blk(13)) > 0.1);
}

#[test]
fn a_wave_change_every_block_recovers_when_it_stops() {
    let out = render_with(&AlgoParams::default(), 60, 100, 40, |b, p| {
        if b < 20 {
            p.ops[0].wave = (b % 16) as u8;
        }
    });
    assert!(out.iter().all(|x| x.is_finite()));
    let late = &out[24 * BLOCK_SIZE..];
    assert!(peak(late) > 0.5, "stuck silent: {}", peak(late));
}

#[test]
fn retrigger_on_a_sounding_voice_does_not_click() {
    let p = AlgoParams::default();
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p, SR);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..30 {
        if b == 15 {
            e.note_on(MidiNote::new(64).unwrap(), Velocity::DEFAULT, &p, SR);
        }
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        out.extend_from_slice(&blk);
    }
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn extreme_parameter_bytes_render_finite_and_bounded() {
    let op = AlgoOpParams {
        wave: 255,
        coarse: 255,
        fine: 255,
        detune: i8::MIN,
        level: 255,
        ar: 255,
        d1r: 255,
        d1l: 255,
        d2r: 255,
        rr: 255,
        rate_scale: 255,
        feedback: 255,
        velocity: 255,
    };
    let p = AlgoParams {
        alg_a: 255,
        alg_b: 255,
        morph: 255,
        transpose: i8::MAX,
        ops: [op; 6],
    };
    let out = render(&p, 127, 50);
    assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 6f32.sqrt()));
}

#[test]
fn the_highest_note_and_ratio_stay_finite() {
    let mut p = AlgoParams {
        transpose: 24,
        ..AlgoParams::default()
    };
    (p.ops[0].coarse, p.ops[0].fine) = (63, 15);
    let out = render(&p, 127, 50);
    assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
}

#[test]
fn a_release_ends_the_voice_and_a_silent_carrier_does_not_hold_it() {
    let mut p = AlgoParams::default();
    p.ops[4].rr = 1; // operator 5 is a T1 carrier at LEVEL 0
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::A4, Velocity::DEFAULT, &p, SR);
    let mut blk = [0.0; BLOCK_SIZE];
    for _ in 0..50 {
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
    }
    e.note_off();
    let mut n = 0;
    while e.is_active() {
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        n += 1;
        assert!(n < 400, "never ends");
    }
    assert!(peak(&blk) < 1e-3);
}

#[test]
fn single_puts_operator_1_alone_on_a_wave() {
    let p = AlgoParams::single(WaveId::SAW);
    assert_eq!(p.ops[0].wave, WaveId::SAW.get());
    assert_eq!(p.ops[0].level, 99);
    assert!(p.ops[1..].iter().all(|o| o.level == 0));
    assert_eq!(AlgoLive::from_params(&p).level[0], 99.0);
}

#[test]
fn the_mip_position_moves_at_most_one_mip_per_block_and_never_jumps() {
    let mut pos = 0.0f32;
    for target in [5.3f32, 5.3, 5.3, 5.3, 5.3, 5.3, 0.2, 0.2, 7.0, 3.0, 3.0] {
        let (lo, from, to) = mip_step(pos, target);
        assert_eq!(lo as f32 + from, pos, "continuous");
        assert!((0.0..=1.0).contains(&from) && (0.0..=1.0).contains(&to));
        let next = lo as f32 + to;
        assert!((next - pos).abs() <= 1.0);
        assert!((next - target).abs() < (pos - target).abs() || next == target);
        pos = next;
    }
    assert_eq!(pos, 3.0);
    assert_eq!(mip_step(2.0, 1.5), (1, 1.0, 0.5));
    assert_eq!(mip_step(2.0, 2.5), (2, 0.0, 0.5));
}

#[test]
fn retrigger_at_another_velocity_does_not_click() {
    let mut p = AlgoParams::default();
    p.ops[0].velocity = 7;
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::new(60).unwrap(), Velocity::MAX, &p, SR);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..30 {
        if b == 15 {
            e.note_on(
                MidiNote::new(60).unwrap(),
                Velocity::new(1).unwrap(),
                &p,
                SR,
            );
        }
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        out.extend_from_slice(&blk);
    }
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn a_one_block_level_dip_to_zero_does_not_end_a_held_note() {
    let p = AlgoParams::default();
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::A4, Velocity::DEFAULT, &p, SR);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..30 {
        let mut live = AlgoLive::from_params(&p);
        if b == 10 {
            live.level[0] = 0.0;
        }
        e.render(&mut blk, &p, &live, SR);
        out.extend_from_slice(&blk);
        assert!(e.is_active(), "block {b}");
    }
    assert!(peak(&out[20 * BLOCK_SIZE..]) > 0.5);
}

/// Plays `first`, releases it until the voice is inactive, then plays the
/// same note on the same patch.
fn after(first: &AlgoParams, first_note: u8) -> Vec<u32> {
    let mut e = AlgoEngine::new();
    let mut blk = [0.0; BLOCK_SIZE];
    e.note_on(MidiNote::new(first_note).unwrap(), Velocity::MAX, first, SR);
    for _ in 0..20 {
        e.render(&mut blk, first, &AlgoLive::from_params(first), SR);
    }
    e.note_off();
    while e.is_active() {
        e.render(&mut blk, first, &AlgoLive::from_params(first), SR);
    }
    let p = stack(AlgoId::A14, AlgoId::A22, 40);
    e.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p, SR);
    let mut out = Vec::new();
    for _ in 0..20 {
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        out.extend(blk.iter().map(|s| s.to_bits()));
    }
    out
}

#[test]
fn a_note_on_a_silent_voice_does_not_depend_on_the_note_before() {
    // Carriers release fast; the modulators are still sounding when the
    // voice goes inactive.
    let mut a = stack(AlgoId::T1, AlgoId::A17, 100);
    for (i, o) in a.ops.iter_mut().enumerate() {
        o.rr = if i == 0 || i == 4 { 15 } else { 1 };
        o.ar = 10;
    }
    let mut b = stack(AlgoId::A1, AlgoId::A5, 0);
    for o in b.ops.iter_mut() {
        o.rr = 15;
    }
    b.ops[5].level = 0;
    b.ops[5].rr = 1;
    let fresh = {
        let mut e = AlgoEngine::new();
        let p = stack(AlgoId::A14, AlgoId::A22, 40);
        e.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p, SR);
        let mut out = Vec::new();
        let mut blk = [0.0; BLOCK_SIZE];
        for _ in 0..20 {
            e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
            out.extend(blk.iter().map(|s| s.to_bits()));
        }
        out
    };
    assert_eq!(after(&a, 30), fresh);
    assert_eq!(after(&b, 100), fresh);
}

#[test]
fn a_retrigger_adopts_an_idle_operators_new_wave_without_a_duck() {
    // Operator 5 (a T1 carrier) decays to idle while operator 1 holds.
    let mut p = AlgoParams::default();
    (p.ops[4].level, p.ops[4].d1r, p.ops[4].d1l) = (99, 31, 0);
    let mut q = p;
    q.ops[4].wave = WaveId::SQR.get();
    let play = |first: &AlgoParams| {
        let mut e = AlgoEngine::new();
        let mut blk = [0.0; BLOCK_SIZE];
        e.note_on(MidiNote::A4, Velocity::DEFAULT, first, SR);
        for _ in 0..40 {
            e.render(&mut blk, first, &AlgoLive::from_params(first), SR);
        }
        e.note_on(MidiNote::A4, Velocity::DEFAULT, &q, SR);
        let mut out = Vec::new();
        for _ in 0..10 {
            e.render(&mut blk, &q, &AlgoLive::from_params(&q), SR);
            out.extend(blk.iter().map(|s| s.to_bits()));
        }
        out
    };
    assert_eq!(play(&p), play(&q));
}

#[test]
fn a_one_block_morph_away_from_the_only_sounding_carrier_does_not_end_the_note() {
    // Operator 5 carries in T1 but not in A17; operator 1 is silent.
    let mut p = AlgoParams {
        alg_b: AlgoId::A17.get(),
        ..AlgoParams::default()
    };
    (p.ops[0].level, p.ops[4].level) = (0, 99);
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::A4, Velocity::DEFAULT, &p, SR);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..30 {
        let mut live = AlgoLive::from_params(&p);
        if b == 10 {
            live.morph = 127.0;
        }
        e.render(&mut blk, &p, &live, SR);
        out.extend_from_slice(&blk);
        assert!(e.is_active(), "block {b}");
    }
    assert!(peak(&out[20 * BLOCK_SIZE..]) > 0.5);
}

#[test]
fn live_values_take_offsets_by_the_adr_0010_formula_and_clamp() {
    use chimera_core::addr::{BlockRef, Op, ParamAddr};
    use chimera_core::params::FilterParams;
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let level = |op| ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL);
    let mut live = AlgoLive::from_params(&AlgoParams::default());
    assert!(live.offset(morph, 0.5));
    assert_eq!(live.morph, 63.5);
    assert!(live.offset(morph, 2.0));
    assert_eq!(live.morph, 127.0);
    assert!(live.offset(level(Op::F), -0.25));
    assert_eq!(live.level[5], 0.0);
    assert!(live.offset(level(Op::A), -0.25));
    assert_eq!(live.level[0], 99.0 - 24.75);
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    assert!(!live.offset(cutoff, 0.5));
}
