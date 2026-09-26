//! Spec § Rendering: six PM operators on an edge list, feedback averaged
//! over two samples, a crossfade between adjacent mips.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, PM_CYCLES, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS, blend};
use chimera_core::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_core::dsp::algo::waves::{MIP0_TOP_HZ, WaveId, mip_position};
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
    assert_eq!(mip_position(90.0), 0.0);
    let m = mip_position(MIP0_TOP_HZ * 2f32.powf(2.5));
    assert!((m - 3.5).abs() < 1e-3, "{m}");
    assert_eq!(mip_position(1.0e6), 7.0);
}

#[test]
fn both_mips_of_a_pair_keep_the_bandwidth_under_nyquist() {
    for i in 0..400 {
        let bw = 50.0 * 1.02f32.powi(i);
        let mip = mip_position(bw) as usize;
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

#[test]
fn a_self_link_reads_the_previous_sample() {
    let mut mods = NONE;
    mods[2] = 0b100;
    let plan = EvalPlan::build(&mods, 1, &NONE, 1);
    assert_eq!(plan.edge_count(), 1);
    assert_eq!(plan.delayed, 1);
}

#[test]
fn a_union_of_more_than_fifteen_links_is_capped() {
    let all = [0b11_1111u8; OPS];
    let plan = EvalPlan::build(&all, 1, &all, 1);
    assert_eq!(plan.edge_count(), 15);
    let (mut a, mut b) = (NONE, NONE);
    for i in 0..OPS {
        a[i] = (1u8 << i) - 1;
        b[i] = !((2u8 << i) - 1) & 0b11_1111;
    }
    assert_eq!(EvalPlan::build(&a, 1, &b, 1).edge_count(), 15);
}

/// The kernel restated sample by sample, operation for operation, so a
/// faster kernel must match it bit for bit. `one_sample_fb` swaps the
/// two-sample feedback average for the last sample only.
struct Model {
    phase: [u32; OPS],
    out: [f32; OPS],
    hist: [f32; OPS],
    one_sample_fb: bool,
}

impl Model {
    fn new() -> Self {
        Model {
            phase: [0; OPS],
            out: [0.0; OPS],
            hist: [0.0; OPS],
            one_sample_fb: false,
        }
    }

    fn render(&mut self, blk: &KernelBlock, env: &mut [OpEnv; OPS], out: &mut [f32; BLOCK_SIZE]) {
        const UNITS: f32 = 16_777_216.0;
        const STEP: f32 = 1.0 / BLOCK_SIZE as f32;
        let plan = blk.plan;
        let pm_scale = PM_CYCLES * UNITS;
        let dm = (blk.morph_to - blk.morph_from) * STEP;
        let edges = &plan.edges[..plan.edge_count()];
        let mut w: Vec<f32> = edges
            .iter()
            .map(|e| blend(e.a, e.b, blk.morph_from) * pm_scale)
            .collect();
        let dw: Vec<f32> = edges.iter().map(|e| (e.b - e.a) * dm * pm_scale).collect();
        let mut gain: [f32; OPS] = core::array::from_fn(|i| blk.ops[i].gain_from);
        let mut xfade: [f32; OPS] = core::array::from_fn(|i| blk.ops[i].xfade_from);
        let mut carrier: [f32; OPS] =
            core::array::from_fn(|i| blend(plan.carrier_a[i], plan.carrier_b[i], blk.morph_from));
        let mut norm = blk.norm_from;
        for s in out.iter_mut() {
            let mut acc = 0.0f32;
            for k in 0..OPS {
                let op = plan.order[k] as usize;
                let o = &blk.ops[op];
                let prev = self.out[op];
                let fb = o.feedback * 0.5 * UNITS;
                let mut pm = if self.one_sample_fb {
                    fb * (prev + prev)
                } else {
                    fb * (prev + self.hist[op])
                };
                for e in plan.starts[k] as usize..plan.starts[k + 1] as usize {
                    pm += w[e] * self.out[edges[e].src as usize];
                    w[e] += dw[e];
                }
                self.phase[op] = self.phase[op].wrapping_add(o.inc);
                let p = self.phase[op].wrapping_add((pm as i32 as u32) << 8);
                let i = (p >> 24) as usize;
                let j = (i + 1) % 256;
                let f = (p & 0x00ff_ffff) as f32 * (1.0 / UNITS);
                let (l0, l1) = (o.lo[i] as f32, o.lo[j] as f32);
                let (h0, h1) = (o.hi[i] as f32, o.hi[j] as f32);
                let a = l0 + (l1 - l0) * f;
                let read = a + (h0 + (h1 - h0) * f - a) * xfade[op];
                let y = read * env[op].step() * gain[op];
                gain[op] += (o.gain_to - o.gain_from) * STEP;
                xfade[op] += (o.xfade_to - o.xfade_from) * STEP;
                self.hist[op] = prev;
                self.out[op] = y;
                acc += carrier[op] * y;
                carrier[op] += (plan.carrier_b[op] - plan.carrier_a[op]) * dm;
            }
            *s = acc * norm;
            norm += (blk.norm_to - blk.norm_from) * STEP;
        }
    }
}

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

fn random_envs(rng: &mut Rng) -> [OpEnv; OPS] {
    core::array::from_fn(|_| {
        let rates = EnvRates {
            ar: 18 + rng.below(14) as u8,
            d1r: 10 + rng.below(22) as u8,
            d1l: rng.below(16) as u8,
            d2r: rng.below(32) as u8,
            rr: 1 + rng.below(15) as u8,
            rs: rng.below(4) as u8,
        };
        let mut e = OpEnv::IDLE;
        e.note_on(EnvCoefs::new(
            rates,
            MidiNote::new(30 + rng.below(70) as u8).unwrap(),
            SR,
        ));
        e
    })
}

fn random_ops(
    rng: &mut Rng,
    gains: &[f32; OPS],
    xfades: &[f32; OPS],
) -> ([OpBlock; OPS], [f32; OPS], [f32; OPS]) {
    let mut to_gain = [0.0; OPS];
    let mut to_xfade = [0.0; OPS];
    let ops = core::array::from_fn(|i| {
        let wave = WaveId::clamped(rng.below(16) as u8);
        let mip = rng.below(8) as usize;
        to_gain[i] = rng.unit() * SAMPLE_SCALE;
        to_xfade[i] = rng.unit();
        OpBlock {
            inc: rng.next() >> 4,
            gain_from: gains[i],
            gain_to: to_gain[i],
            feedback: FEEDBACK_CYCLES[rng.below(8) as usize],
            lo: wave.table(mip),
            hi: wave.table(mip + 1),
            xfade_from: xfades[i],
            xfade_to: to_xfade[i],
        }
    });
    (ops, to_gain, to_xfade)
}

fn random_mods(rng: &mut Rng, acyclic: bool) -> [u8; OPS] {
    core::array::from_fn(|i| {
        let m = rng.next() as u8;
        if acyclic {
            m & ((1u8 << i) - 1)
        } else {
            m & 0b11_1111 & rng.next() as u8
        }
    })
}

/// Renders `blocks` chained blocks of a random patch through the kernel and
/// the model; returns whether every sample matched bit for bit.
fn kernel_matches_model(seed: u32, acyclic: bool, one_sample_fb: bool) -> bool {
    let mut rng = Rng(seed);
    let plan = EvalPlan::build(
        &random_mods(&mut rng, acyclic),
        rng.below(63) as u8 + 1,
        &random_mods(&mut rng, acyclic),
        rng.below(63) as u8 + 1,
    );
    let (mut k, mut m) = (Kernel::new(), Model::new());
    m.one_sample_fb = one_sample_fb;
    let mut env_k = random_envs(&mut rng);
    let mut env_m = env_k;
    let (mut gains, mut xfades) = ([0.0; OPS], [0.0; OPS]);
    let (mut morph, mut norm) = (rng.unit(), rng.unit());
    let (mut a, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
    for block in 0..40 {
        let (ops, g, x) = random_ops(&mut rng, &gains, &xfades);
        let (morph_to, norm_to) = (rng.unit(), rng.unit());
        let blk = KernelBlock {
            plan: &plan,
            ops,
            morph_from: morph,
            morph_to,
            norm_from: norm,
            norm_to,
        };
        (gains, xfades, morph, norm) = (g, x, morph_to, norm_to);
        if block == 30 {
            for (x, y) in env_k.iter_mut().zip(env_m.iter_mut()) {
                x.note_off();
                y.note_off();
            }
        }
        k.render(&blk, &mut env_k, &mut a);
        m.render(&blk, &mut env_m, &mut b);
        if !same_bits(&a, &b) {
            return false;
        }
    }
    true
}

#[test]
fn the_kernel_matches_the_model_bit_for_bit() {
    for seed in 1..200u32 {
        let seed = seed.wrapping_mul(0x9e37_79b9) | 1;
        assert!(
            kernel_matches_model(seed, true, false),
            "acyclic seed {seed}"
        );
        assert!(
            kernel_matches_model(seed, false, false),
            "cyclic seed {seed}"
        );
    }
}

#[test]
fn feedback_averages_the_last_two_samples() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = OpBlock {
        feedback: FEEDBACK_CYCLES[6],
        ..op(440.0, 1.0, WaveId::W1)
    };
    let blk = KernelBlock {
        plan: &plan,
        ops,
        morph_from: 0.0,
        morph_to: 0.0,
        norm_from: 1.0,
        norm_to: 1.0,
    };
    let with = |one_sample_fb: bool| {
        let (mut m, mut env, mut b) = (Model::new(), envs(), [0.0; BLOCK_SIZE]);
        m.one_sample_fb = one_sample_fb;
        m.render(&blk, &mut env, &mut b);
        b
    };
    let got = render(&plan, ops, 1);
    assert!(same_bits(&got, &with(false)));
    assert!(!same_bits(&got, &with(true)));
    assert!((1..200u32).map(|s| s | 1).all(|s| !kernel_matches_model(
        s.wrapping_mul(0x9e37_79b9) | 1,
        true,
        true
    )));
}

#[test]
fn a_delayed_link_reads_its_source_one_sample_late() {
    let mut back = NONE;
    back[0] = 0b10; // 1 → 2, with 2 → 1 in the other half: a cycle
    let mut fwd_a = NONE;
    fwd_a[1] = 0b1;
    let delayed = EvalPlan::build(&fwd_a, 0b10, &back, 0b10);
    let forward = EvalPlan::build(&NONE, 0b10, &back, 0b10);
    assert_ne!(delayed.delayed, 0);
    assert_eq!(forward.delayed, 0);
    let mut ops = silent();
    ops[0] = op(700.0, 1.0, WaveId::W1);
    ops[1] = op(300.0, 1.0, WaveId::W1);
    let at_b = |plan: &EvalPlan| {
        let (mut k, mut env, mut b) = (Kernel::new(), envs(), [0.0; BLOCK_SIZE]);
        let blk = KernelBlock {
            plan,
            ops,
            morph_from: 1.0,
            morph_to: 1.0,
            norm_from: 1.0,
            norm_to: 1.0,
        };
        k.render(&blk, &mut env, &mut b);
        b
    };
    let mut plain = ops;
    plain[0] = op(700.0, 0.0, WaveId::W1);
    let alone = {
        let (mut k, mut env, mut b) = (Kernel::new(), envs(), [0.0; BLOCK_SIZE]);
        let blk = KernelBlock {
            plan: &forward,
            ops: plain,
            morph_from: 1.0,
            morph_to: 1.0,
            norm_from: 1.0,
            norm_to: 1.0,
        };
        k.render(&blk, &mut env, &mut b);
        b
    };
    let (late, now) = (at_b(&delayed), at_b(&forward));
    assert_eq!(late[0].to_bits(), alone[0].to_bits());
    assert_ne!(now[0].to_bits(), alone[0].to_bits());
    assert!(!same_bits(&late, &now));
}

#[test]
fn morph_ramps_the_link_weights_and_carriers() {
    let mut with_link = NONE;
    with_link[1] = 0b1;
    let a_to_b = EvalPlan::build(&with_link, 0b1, &NONE, 0b11);
    let a_only = EvalPlan::build(&with_link, 0b1, &with_link, 0b1);
    let mut ops = silent();
    ops[0] = op(440.0, 1.0, WaveId::W1);
    ops[1] = op(660.0, 1.0, WaveId::W1);
    let at = |plan: &EvalPlan, from: f32, to: f32| {
        let (mut k, mut env, mut out) = (Kernel::new(), envs(), [0.0; BLOCK_SIZE]);
        let blk = KernelBlock {
            plan,
            ops,
            morph_from: from,
            morph_to: to,
            norm_from: 1.0,
            norm_to: 1.0,
        };
        k.render(&blk, &mut env, &mut out);
        out
    };
    let only_a = at(&a_only, 0.0, 0.0);
    assert!(same_bits(&at(&a_to_b, 0.0, 0.0), &only_a));
    let ramp = at(&a_to_b, 0.0, 1.0);
    assert_eq!(ramp[0].to_bits(), only_a[0].to_bits());
    assert!(!same_bits(&ramp, &only_a));
    let only_b = at(&EvalPlan::build(&NONE, 0b11, &NONE, 0b11), 0.0, 0.0);
    assert!(same_bits(&at(&a_to_b, 1.0, 1.0), &only_b));
}

#[test]
fn norm_ramps_across_the_block() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = op(480.0, 1.0, WaveId::W1);
    let (mut k, mut env, mut out) = (Kernel::new(), envs(), [0.0; BLOCK_SIZE]);
    let blk = KernelBlock {
        plan: &plan,
        ops,
        morph_from: 0.0,
        morph_to: 0.0,
        norm_from: 0.0,
        norm_to: 1.0,
    };
    k.render(&blk, &mut env, &mut out);
    let full = render(&plan, ops, 1);
    for s in 0..BLOCK_SIZE {
        assert_eq!(
            out[s].to_bits(),
            (full[s] * (s as f32 / BLOCK_SIZE as f32)).to_bits(),
            "{s}"
        );
    }
}

#[test]
fn a_gain_ramp_ends_at_its_target() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = OpBlock {
        gain_from: 0.0,
        ..op(480.0, 1.0, WaveId::W1)
    };
    let ramp = render(&plan, ops, 1);
    let full = render(&plan, silent_but(op(480.0, 1.0, WaveId::W1)), 1);
    let s = BLOCK_SIZE - 1;
    let expect = full[s] * s as f32 / BLOCK_SIZE as f32;
    assert!((ramp[s] - expect).abs() < 1e-5, "{} vs {expect}", ramp[s]);
}

fn silent_but(first: OpBlock) -> [OpBlock; OPS] {
    let mut ops = silent();
    ops[0] = first;
    ops
}
