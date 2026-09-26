//! Spec § Rendering: six PM operators on an edge list, feedback averaged
//! over two samples, a crossfade between adjacent mips.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_core::dsp::algo::waves::{MIP0_TOP_HZ, WaveId};
use chimera_hal::BLOCK_SIZE;

const SR: f32 = 48_000.0;
const HOLD: EnvRates = EnvRates {
    ar: 31,
    d1r: 0,
    d1l: 15,
    d2r: 0,
    rr: 8,
    rs: 0,
};
const NONE: [u8; OPS] = [0; OPS];

fn op(hz: f32, gain: f32, wave: WaveId) -> OpBlock {
    OpBlock {
        inc: (hz / SR * 4_294_967_296.0) as u32,
        gain_from: gain * SAMPLE_SCALE,
        gain_to: gain * SAMPLE_SCALE,
        feedback: 0.0,
        lo: wave.table(0),
        hi: wave.table(0),
        xfade_from: 0.0,
        xfade_to: 0.0,
    }
}

fn silent() -> [OpBlock; OPS] {
    core::array::from_fn(|_| op(0.0, 0.0, WaveId::W1))
}

fn envs() -> [OpEnv; OPS] {
    core::array::from_fn(|_| {
        let mut e = OpEnv::IDLE;
        e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, SR));
        e
    })
}

fn render(plan: &EvalPlan, ops: [OpBlock; OPS], blocks: usize) -> Vec<f32> {
    let (mut k, mut env) = (Kernel::new(), envs());
    let blk = KernelBlock {
        plan,
        ops,
        morph_from: 0.0,
        morph_to: 0.0,
        norm_from: 1.0,
        norm_to: 1.0,
    };
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    for _ in 0..blocks {
        k.render(&blk, &mut env, &mut b);
        out.extend_from_slice(&b);
    }
    out
}

fn hz(s: &[f32]) -> f64 {
    let ups: Vec<f64> = (1..s.len())
        .filter(|&i| s[i - 1] < 0.0 && s[i] >= 0.0)
        .map(|i| (i - 1) as f64 + (-s[i - 1] as f64) / ((s[i] - s[i - 1]) as f64))
        .collect();
    (ups.len() - 1) as f64 * SR as f64 / (ups[ups.len() - 1] - ups[0])
}

fn goertzel(s: &[f32], f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / SR as f64;
    let (mut s1, mut s2) = (0.0, 0.0);
    for &x in s {
        let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
        (s2, s1) = (s1, s0);
    }
    (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt()
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

#[test]
fn a_lone_carrier_plays_its_table_at_its_increment() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = op(480.0, 1.0, WaveId::W1);
    let out = render(&plan, ops, 200);
    let f = hz(&out[BLOCK_SIZE..]);
    assert!((f - 480.0).abs() < 0.01, "{f} Hz");
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak - 1.0).abs() < 1e-3, "{peak}");
}

#[test]
fn a_silent_modulator_changes_nothing_and_a_loud_one_changes_the_tone() {
    let mut mods = NONE;
    mods[1] = 0b1;
    let plan = EvalPlan::build(&mods, 1, &mods, 1);
    let mut ops = silent();
    ops[0] = op(440.0, 1.0, WaveId::W1);
    let alone = render(&EvalPlan::build(&NONE, 1, &NONE, 1), ops, 20);
    ops[1] = op(880.0, 0.0, WaveId::W1);
    assert!(same_bits(&render(&plan, ops, 20), &alone));
    ops[1] = op(880.0, 0.5, WaveId::W1);
    let fm = render(&plan, ops, 20);
    assert!(fm.iter().zip(&alone).any(|(a, b)| (a - b).abs() > 0.1));
}

#[test]
fn feedback_adds_harmonics_to_a_sine() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let harmonics = |fb: f32| {
        let mut ops = silent();
        ops[0] = OpBlock {
            feedback: fb,
            ..op(440.0, 1.0, WaveId::W1)
        };
        let out = render(&plan, ops, 100);
        (2..=8)
            .map(|h| goertzel(&out[1024..], 440.0 * h as f64))
            .sum::<f64>()
    };
    assert!(harmonics(FEEDBACK_CYCLES[4]) > 10.0 * harmonics(0.0));
}

#[test]
fn the_crossfade_ends_are_its_two_mips() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let with = |lo: WaveId, hi: WaveId, xfade: f32| {
        let mut ops = silent();
        ops[0] = OpBlock {
            lo: lo.table(0),
            hi: hi.table(0),
            xfade_from: xfade,
            xfade_to: xfade,
            ..op(300.0, 1.0, WaveId::W1)
        };
        render(&plan, ops, 10)
    };
    assert!(same_bits(
        &with(WaveId::W1, WaveId::SAW, 0.0),
        &with(WaveId::W1, WaveId::W1, 0.0)
    ));
    let top = with(WaveId::W1, WaveId::SAW, 1.0);
    let saw = with(WaveId::SAW, WaveId::SAW, 0.0);
    assert!(top.iter().zip(&saw).all(|(a, b)| (a - b).abs() < 1e-5));
}

#[test]
fn a_gain_change_ramps_across_the_block() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = OpBlock {
        gain_from: 0.0,
        ..op(480.0, 1.0, WaveId::W1)
    };
    let out = render(&plan, ops, 1);
    let early = out[..8].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(early < 0.15, "{early}");
}

#[test]
fn a_link_that_runs_backwards_is_flagged_as_delayed() {
    let (mut a, mut b) = (NONE, NONE);
    a[1] = 0b01; // 2 → 1
    b[0] = 0b10; // 1 → 2: the union is a cycle
    let plan = EvalPlan::build(&a, 1, &b, 1);
    assert_eq!(plan.delayed.count_ones(), 1);
    let e = plan.delayed.trailing_zeros() as usize;
    assert_eq!((plan.edges[e].src, plan.edges[e].dst), (0, 1));
}

#[test]
fn links_from_higher_to_lower_operators_all_run_forward() {
    let mut seed = 0x2545_f491u32;
    for _ in 0..500 {
        let mut mods = [[0u8; OPS]; 2];
        for m in mods.iter_mut() {
            for (i, bits) in m.iter_mut().enumerate() {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *bits = (seed as u8) & ((1u8 << i) - 1);
            }
        }
        let plan = EvalPlan::build(&mods[0], 1, &mods[1], 1);
        assert_eq!(plan.order, [5, 4, 3, 2, 1, 0]);
        assert_eq!(plan.delayed, 0);
        assert!(plan.edge_count() <= 15);
    }
}

#[test]
fn the_mip_follows_the_bandwidth_and_crossfades() {
    let w = WaveId::SAW;
    let (lo, hi, x) = w.mip_pair(90.0);
    assert!(core::ptr::eq(lo, w.table(0)) && core::ptr::eq(hi, w.table(1)) && x == 0.0);
    let (lo, hi, x) = w.mip_pair(MIP0_TOP_HZ * 2f32.powf(2.5));
    assert!(core::ptr::eq(lo, w.table(3)) && core::ptr::eq(hi, w.table(4)));
    assert!((x - 0.5).abs() < 1e-3, "{x}");
    let (lo, hi, x) = w.mip_pair(1.0e6);
    assert!(core::ptr::eq(lo, w.table(7)) && core::ptr::eq(hi, w.table(7)) && x == 0.0);
}

#[test]
fn both_mips_of_a_pair_keep_the_bandwidth_under_nyquist() {
    let w = WaveId::SAW;
    for i in 0..400 {
        let bw = 50.0 * 1.02f32.powi(i);
        let (lo, _, _) = w.mip_pair(bw);
        let mip = (0..8).find(|&m| core::ptr::eq(lo, w.table(m))).unwrap();
        if mip < 7 {
            assert!(
                bw <= MIP0_TOP_HZ * (1 << mip) as f32 * 1.0001,
                "{bw} Hz on mip {mip}"
            );
        }
    }
}

#[test]
fn the_crossfade_ramps_across_the_block() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let with = |from: f32, to: f32| {
        let mut ops = silent();
        ops[0] = OpBlock {
            lo: WaveId::W1.table(0),
            hi: WaveId::SAW.table(0),
            xfade_from: from,
            xfade_to: to,
            ..op(300.0, 1.0, WaveId::W1)
        };
        render(&plan, ops, 1)
    };
    let (sine, ramp) = (with(0.0, 0.0), with(0.0, 1.0));
    assert_eq!(ramp[0].to_bits(), sine[0].to_bits());
    let saw = with(1.0, 1.0);
    let s = BLOCK_SIZE - 1;
    let expect = sine[s] + (saw[s] - sine[s]) * (s as f32 / BLOCK_SIZE as f32);
    assert!((ramp[s] - expect).abs() < 1e-4, "{} vs {expect}", ramp[s]);
}
