//! The per-sample loop: six phase-modulation operators on an `EvalPlan`.

use crate::dsp::algo::env::OpEnv;
use crate::dsp::algo::plan::{EvalPlan, MAX_EDGES, OPS, blend};
use crate::dsp::algo::waves::{Table, WAVE_LEN};
use crate::hw::BLOCK_SIZE;

/// Phase swing, in cycles, of a full-scale modulator at weight 1.
pub const PM_CYCLES: f32 = 4.0;
const PHASE_UNITS: f32 = 16_777_216.0;
const PM_SCALE: f32 = PM_CYCLES * PHASE_UNITS;
pub const SAMPLE_SCALE: f32 = 1.0 / 32_767.0;

#[derive(Clone, Copy, Debug)]
pub struct OpBlock {
    /// 2^32 is one cycle.
    pub inc: u32,
    /// `SAMPLE_SCALE` included.
    pub gain_from: f32,
    pub gain_to: f32,
    /// `FEEDBACK_CYCLES` of the operator's setting.
    pub feedback: f32,
    pub lo: &'static Table,
    pub hi: &'static Table,
    /// The weight of `hi`, ramped across the block like the gain.
    pub xfade_from: f32,
    pub xfade_to: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct KernelBlock<'a> {
    pub plan: &'a EvalPlan,
    pub ops: [OpBlock; OPS],
    pub morph_from: f32,
    pub morph_to: f32,
    pub norm_from: f32,
    pub norm_to: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Kernel {
    phase: [u32; OPS],
    /// Each operator's latest output (eight slots: an index masked with 7
    /// needs no bounds check).
    out: [f32; 8],
    /// The output before that, for feedback averaged over two samples.
    hist: [f32; OPS],
}

#[derive(Clone, Copy)]
struct Lane {
    op: usize,
    phase: u32,
    inc: u32,
    gain: f32,
    dgain: f32,
    carrier: f32,
    dcarrier: f32,
    feedback: f32,
    hist: f32,
    xfade: f32,
    dxfade: f32,
    lo: &'static Table,
    hi: &'static Table,
    edges: (usize, usize),
    env: OpEnv,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    pub const fn new() -> Self {
        Self {
            phase: [0; OPS],
            out: [0.0; 8],
            hist: [0.0; OPS],
        }
    }

    pub fn reset(&mut self, op: usize) {
        self.phase[op] = 0;
        self.out[op] = 0.0;
        self.hist[op] = 0.0;
    }

    pub fn render(
        &mut self,
        blk: &KernelBlock,
        env: &mut [OpEnv; OPS],
        out: &mut [f32; BLOCK_SIZE],
    ) {
        const STEP: f32 = 1.0 / BLOCK_SIZE as f32;
        let plan = blk.plan;
        let dm = (blk.morph_to - blk.morph_from) * STEP;
        let mut w = [0.0f32; MAX_EDGES];
        let mut dw = [0.0f32; MAX_EDGES];
        let mut src = [0u8; MAX_EDGES];
        for e in 0..plan.edge_count() {
            let edge = plan.edges[e];
            w[e] = blend(edge.a, edge.b, blk.morph_from) * PM_SCALE;
            dw[e] = (edge.b - edge.a) * dm * PM_SCALE;
            src[e] = edge.src;
        }
        let mut lanes: [Lane; OPS] = core::array::from_fn(|k| {
            let op = plan.order[k] as usize % OPS;
            let o = &blk.ops[op];
            Lane {
                op,
                phase: self.phase[op],
                inc: o.inc,
                gain: o.gain_from,
                dgain: (o.gain_to - o.gain_from) * STEP,
                carrier: blend(plan.carrier_a[op], plan.carrier_b[op], blk.morph_from),
                dcarrier: (plan.carrier_b[op] - plan.carrier_a[op]) * dm,
                feedback: o.feedback * 0.5 * PHASE_UNITS,
                hist: self.hist[op],
                xfade: o.xfade_from,
                dxfade: (o.xfade_to - o.xfade_from) * STEP,
                lo: o.lo,
                hi: o.hi,
                edges: (plan.starts[k] as usize, plan.starts[k + 1] as usize),
                env: env[op],
            }
        });
        let mut norm = blk.norm_from;
        let dnorm = (blk.norm_to - blk.norm_from) * STEP;
        for s in out.iter_mut() {
            let mut acc = 0.0f32;
            for l in lanes.iter_mut() {
                let prev = self.out[l.op & 7];
                let mut pm = l.feedback * (prev + l.hist);
                for e in l.edges.0..l.edges.1.min(MAX_EDGES) {
                    pm += w[e] * self.out[src[e] as usize & 7];
                    w[e] += dw[e];
                }
                l.phase = l.phase.wrapping_add(l.inc);
                let p = l.phase.wrapping_add((pm as i32 as u32) << 8);
                let y = read(l.lo, l.hi, l.xfade, p) * l.env.step() * l.gain;
                l.gain += l.dgain;
                l.xfade += l.dxfade;
                l.hist = prev;
                self.out[l.op & 7] = y;
                acc += l.carrier * y;
                l.carrier += l.dcarrier;
            }
            *s = acc * norm;
            norm += dnorm;
        }
        for l in &lanes {
            self.phase[l.op] = l.phase;
            self.hist[l.op] = l.hist;
            env[l.op] = l.env;
        }
    }
}

#[inline(always)]
fn read(lo: &Table, hi: &Table, xfade: f32, p: u32) -> f32 {
    let i = (p >> 24) as usize;
    let j = (i + 1) & (WAVE_LEN - 1);
    let f = (p & 0x00ff_ffff) as f32 * (1.0 / PHASE_UNITS);
    let (l0, l1) = (lo[i] as f32, lo[j] as f32);
    let (h0, h1) = (hi[i] as f32, hi[j] as f32);
    let a = l0 + (l1 - l0) * f;
    a + (h0 + (h1 - h0) * f - a) * xfade
}
