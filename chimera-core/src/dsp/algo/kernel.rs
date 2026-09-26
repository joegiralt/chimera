//! The per-sample loop: six phase-modulation operators on an `EvalPlan`.

use crate::dsp::algo::env::{EnvRun, OpEnv};
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
        if blk.plan.delayed == 0 {
            self.render_ops(blk, env, out);
        } else {
            self.render_samples(blk, env, out);
        }
    }

    /// Operator-major: every link runs forward, so each operator renders
    /// its whole block in turn with its state and link weights in
    /// registers. The same operations in the same order as
    /// `render_samples`, so the output is identical.
    fn render_ops(
        &mut self,
        blk: &KernelBlock,
        env: &mut [OpEnv; OPS],
        out: &mut [f32; BLOCK_SIZE],
    ) {
        let plan = blk.plan;
        let dm = (blk.morph_to - blk.morph_from) * STEP;
        let mut pos = [0usize; OPS];
        for (k, &op) in plan.order.iter().enumerate() {
            pos[op as usize % OPS] = k;
        }
        // Row `k` holds the block of the operator `k`-th in the order.
        let mut bufs = [[0.0f32; BLOCK_SIZE]; OPS];
        out.fill(0.0);
        let mut k = 0;
        while k < OPS {
            let n = plan.starts[k + 1].saturating_sub(plan.starts[k]) as usize;
            let (done, rest) = bufs.split_at_mut(k);
            let link = |i: usize| {
                let e = plan.edges[plan.starts[k] as usize + i];
                Link {
                    w: blend(e.a, e.b, blk.morph_from) * PM_SCALE,
                    dw: (e.b - e.a) * dm * PM_SCALE,
                    src: &done[pos[e.src as usize]],
                }
            };
            let a = plan.order[k] as usize % OPS;
            let mut x = self.lane(blk, a, dm);
            let pair = k + 1 < OPS && n <= 2 && {
                let m = plan.starts[k + 2].saturating_sub(plan.starts[k + 1]) as usize;
                let from_a = plan.edges[plan.starts[k + 1] as usize..]
                    .iter()
                    .take(m)
                    .any(|e| e.src as usize == a);
                m <= 2 && !from_a
            };
            if pair {
                let b = plan.order[k + 1] as usize % OPS;
                let mut y = self.lane(blk, b, dm);
                let m = plan.starts[k + 2].saturating_sub(plan.starts[k + 1]) as usize;
                let link_b = |i: usize| {
                    let e = plan.edges[plan.starts[k + 1] as usize + i];
                    Link {
                        w: blend(e.a, e.b, blk.morph_from) * PM_SCALE,
                        dw: (e.b - e.a) * dm * PM_SCALE,
                        src: &done[pos[e.src as usize]],
                    }
                };
                let (row_a, row_b) = rest.split_at_mut(1);
                let (ea, eb) = pick2(env, a, b);
                let (da, db) = (&mut row_a[0], &mut row_b[0]);
                macro_rules! pair {
                    ($($n:literal $m:literal)*) => {
                        match (n, m) {
                            $(($n, $m) => run_pair::<$n, $m>(
                                (&mut x, from_fn(link), ea, da),
                                (&mut y, from_fn(link_b), eb, db),
                                out,
                            ),)*
                            _ => {}
                        }
                    };
                }
                pair!(0 0 0 1 0 2 1 0 1 1 1 2 2 0 2 1 2 2);
                self.keep(b, &y);
                k += 2;
            } else {
                let d = &mut rest[0];
                let e = &mut env[a];
                match n {
                    0 => run_one(&mut x, from_fn::<_, 0>(link), e, d, out),
                    1 => run_one(&mut x, from_fn::<_, 1>(link), e, d, out),
                    2 => run_one(&mut x, from_fn::<_, 2>(link), e, d, out),
                    3 => run_one(&mut x, from_fn::<_, 3>(link), e, d, out),
                    4 => run_one(&mut x, from_fn::<_, 4>(link), e, d, out),
                    _ => run_one(&mut x, from_fn::<_, 5>(link), e, d, out),
                }
                k += 1;
            }
            self.keep(a, &x);
        }
        let mut norm = blk.norm_from;
        let dnorm = (blk.norm_to - blk.norm_from) * STEP;
        for s in out.iter_mut() {
            *s *= norm;
            norm += dnorm;
        }
    }

    #[inline(always)]
    fn lane(&self, blk: &KernelBlock, op: usize, dm: f32) -> OpState {
        let (plan, o) = (blk.plan, &blk.ops[op]);
        OpState {
            phase: self.phase[op],
            inc: o.inc,
            gain: o.gain_from,
            dgain: (o.gain_to - o.gain_from) * STEP,
            carrier: blend(plan.carrier_a[op], plan.carrier_b[op], blk.morph_from),
            dcarrier: (plan.carrier_b[op] - plan.carrier_a[op]) * dm,
            feedback: o.feedback * 0.5 * PHASE_UNITS,
            prev: self.out[op],
            hist: self.hist[op],
            xfade: o.xfade_from,
            dxfade: (o.xfade_to - o.xfade_from) * STEP,
            lo: o.lo,
            hi: o.hi,
        }
    }

    #[inline(always)]
    fn keep(&mut self, op: usize, st: &OpState) {
        self.phase[op] = st.phase;
        self.out[op] = st.prev;
        self.hist[op] = st.hist;
    }

    fn render_samples(
        &mut self,
        blk: &KernelBlock,
        env: &mut [OpEnv; OPS],
        out: &mut [f32; BLOCK_SIZE],
    ) {
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

const STEP: f32 = 1.0 / BLOCK_SIZE as f32;

#[derive(Clone, Copy)]
struct OpState {
    phase: u32,
    inc: u32,
    gain: f32,
    dgain: f32,
    carrier: f32,
    dcarrier: f32,
    feedback: f32,
    prev: f32,
    hist: f32,
    xfade: f32,
    dxfade: f32,
    lo: &'static Table,
    hi: &'static Table,
}

#[derive(Clone, Copy)]
struct Link<'a> {
    w: f32,
    dw: f32,
    src: &'a [f32; BLOCK_SIZE],
}

fn from_fn<'a, F: FnMut(usize) -> Link<'a>, const N: usize>(f: F) -> [Link<'a>; N] {
    core::array::from_fn(f)
}

fn pick2(env: &mut [OpEnv; OPS], a: usize, b: usize) -> (&mut OpEnv, &mut OpEnv) {
    let (lo, hi) = env.split_at_mut(a.max(b));
    let (x, y) = (&mut lo[a.min(b)], &mut hi[0]);
    if a < b { (x, y) } else { (y, x) }
}

/// One sample of one operator, exactly as `render_samples` does it.
#[inline(always)]
fn tick<const N: usize>(
    st: &mut OpState,
    links: &mut [Link; N],
    env: &mut EnvRun,
    env_mem: &mut OpEnv,
    s: usize,
) -> f32 {
    let mut pm = st.feedback * (st.prev + st.hist);
    for l in links.iter_mut() {
        pm += l.w * l.src[s];
        l.w += l.dw;
    }
    st.phase = st.phase.wrapping_add(st.inc);
    let p = st.phase.wrapping_add((pm as i32 as u32) << 8);
    let y = read(st.lo, st.hi, st.xfade, p) * env.step(env_mem) * st.gain;
    st.gain += st.dgain;
    st.xfade += st.dxfade;
    st.hist = st.prev;
    st.prev = y;
    y
}

#[inline(always)]
fn run_one<const N: usize>(
    st: &mut OpState,
    mut links: [Link; N],
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (mut x, mut run) = (*st, env.run());
    for s in 0..BLOCK_SIZE {
        let y = tick(&mut x, &mut links, &mut run, env, s);
        dst[s] = y;
        acc[s] += x.carrier * y;
        x.carrier += x.dcarrier;
    }
    *st = x;
    env.store(run);
}

type PairLane<'a, 'b, const N: usize> = (
    &'b mut OpState,
    [Link<'a>; N],
    &'b mut OpEnv,
    &'b mut [f32; BLOCK_SIZE],
);

/// Two operators that don't read each other, interleaved so their chains
/// overlap; `a` still sums into `acc` first.
#[inline(always)]
fn run_pair<const N: usize, const M: usize>(
    a: PairLane<N>,
    b: PairLane<M>,
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (sa, mut la, ea, da) = a;
    let (sb, mut lb, eb, db) = b;
    let (mut x, mut rx) = (*sa, ea.run());
    let (mut y, mut ry) = (*sb, eb.run());
    for s in 0..BLOCK_SIZE {
        let ya = tick(&mut x, &mut la, &mut rx, ea, s);
        let yb = tick(&mut y, &mut lb, &mut ry, eb, s);
        da[s] = ya;
        db[s] = yb;
        acc[s] += x.carrier * ya;
        acc[s] += y.carrier * yb;
        x.carrier += x.dcarrier;
        y.carrier += y.dcarrier;
    }
    *sa = x;
    *sb = y;
    ea.store(rx);
    eb.store(ry);
}

const _: () = assert!(cfg!(target_endian = "little") && WAVE_LEN == 256);

/// Samples `i` and `i + 1` in one 32-bit load.
#[inline(always)]
fn pair(t: &Table, i: usize) -> (f32, f32) {
    let i = i & (WAVE_LEN - 1);
    // SAFETY: `i + 1 <= WAVE_LEN`, so the four bytes read lie inside the
    // `WAVE_LEN + 1` samples of `t`; the read is unaligned, which every
    // target (Cortex-M7 included) allows.
    let w = unsafe { t.as_ptr().add(i).cast::<u32>().read_unaligned() };
    (w as u16 as i16 as f32, (w as i32 >> 16) as f32)
}

#[inline(always)]
fn read(lo: &Table, hi: &Table, xfade: f32, p: u32) -> f32 {
    let i = (p >> 24) as usize;
    let f = (p & 0x00ff_ffff) as f32 * (1.0 / PHASE_UNITS);
    let (l0, l1) = pair(lo, i);
    let (h0, h1) = pair(hi, i);
    let a = l0 + (l1 - l0) * f;
    a + (h0 + (h1 - h0) * f - a) * xfade
}
