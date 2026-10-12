//! The per-sample loop: six phase-modulation operators on an `EvalPlan`.

use crate::dsp::algo::env::{EnvRun, OpEnv};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::math::clampf;
use crate::dsp::algo::noise::{NOISE_CLAMP, noise_seed};
use crate::dsp::algo::plan::{EvalPlan, MAX_EDGES, OPS, blend};
use crate::dsp::algo::waves::{Table, WAVE_LEN, WaveId};
use crate::hw::BLOCK_SIZE;
use crate::taps::{TapOp, TapRows};

/// Phase swing, in cycles, of a full-scale modulator at weight 1.
pub const PM_CYCLES: f32 = 4.0;
const PHASE_UNITS: f32 = 16_777_216.0;
const PM_SCALE: f32 = PM_CYCLES * PHASE_UNITS;
pub const SAMPLE_SCALE: f32 = 1.0 / 32_767.0;

#[derive(Clone, Copy, Debug)]
pub struct OpBlock {
    /// 2^32 is one cycle: the pitch, or a noise band's centre.
    pub inc: u32,
    /// `SAMPLE_SCALE` included on a table.
    pub gain_from: f32,
    pub gain_to: f32,
    /// `FEEDBACK_CYCLES` of the operator's setting; noise ignores it.
    pub feedback: f32,
    pub kind: KindBlock,
}

/// What the operator plays this block, by kind.
#[derive(Clone, Copy, Debug)]
pub enum KindBlock {
    Table {
        lo: &'static Table,
        hi: &'static Table,
        /// The weight of `hi`, ramped across the block like the gain.
        xfade_from: f32,
        xfade_to: f32,
    },
    Noise(NoiseBlock),
}

/// A noise band's settings, each `(from, to)`, ramped across the block
/// (spec § 4): the poles' coefficient `c`, their input gain `k`, and the
/// noise and tone weights `alpha` and `beta`.
#[derive(Clone, Copy, Debug)]
pub struct NoiseBlock {
    pub c: (f32, f32),
    pub k: (f32, f32),
    pub alpha: (f32, f32),
    pub beta: (f32, f32),
}

/// Each operator's own state beyond its phase.
#[derive(Clone, Copy, Debug)]
enum KindState {
    Table,
    Noise { y1: f32, y2: f32 },
}

impl KindState {
    /// The poles: closed (0) on any state but noise's.
    fn poles(self) -> (f32, f32) {
        match self {
            KindState::Noise { y1, y2 } => (y1, y2),
            KindState::Table => (0.0, 0.0),
        }
    }
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
    /// Each operator's xorshift state: seeded per voice, never reset.
    rng: [u32; OPS],
    state: [KindState; OPS],
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
    src: Src,
    edges: (usize, usize),
    env: OpEnv,
}

/// What a lane reads: its tables or its noise.
#[derive(Clone, Copy)]
enum Src {
    Table {
        xfade: f32,
        dxfade: f32,
        lo: &'static Table,
        hi: &'static Table,
    },
    Noise(Noise),
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    pub const fn new() -> Self {
        let mut k = Self {
            phase: [0; OPS],
            out: [0.0; 8],
            hist: [0.0; OPS],
            rng: [0; OPS],
            state: [KindState::Table; OPS],
        };
        k.seed(0);
        k
    }

    /// Every operator's noise from voice `voice`'s seeds.
    pub const fn seed(&mut self, voice: u8) {
        let mut op = 0;
        while op < OPS {
            self.rng[op] = noise_seed(voice, op);
            op += 1;
        }
    }

    /// Operator `op` starts afresh as `kind`, closed: noise's poles at 0,
    /// its generator running on.
    pub fn reset(&mut self, op: usize, kind: OpKind) {
        self.phase[op] = 0;
        self.out[op] = 0.0;
        self.hist[op] = 0.0;
        self.state[op] = match kind {
            OpKind::Noise => KindState::Noise { y1: 0.0, y2: 0.0 },
            OpKind::Table(_) | OpKind::Formant => KindState::Table,
        };
    }

    /// `blk.plan` must come from `EvalPlan::build` unchanged; see `EvalPlan`.
    /// Each wanted tap's row takes its op's output, clamped to ±1.
    pub fn render(
        &mut self,
        blk: &KernelBlock,
        env: &mut [OpEnv; OPS],
        out: &mut [f32; BLOCK_SIZE],
        taps: &mut TapRows,
    ) {
        if blk.plan.delayed == 0 {
            self.render_ops(blk, env, out, taps);
        } else {
            self.render_samples(blk, env, out, taps);
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
        taps: &mut TapRows,
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
            let a = plan.order[k] as usize % OPS;
            if silent(&blk.ops[a]) {
                self.idle(&blk.ops[a], a, &mut env[a]);
                k += 1;
                continue;
            }
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
            if let KindBlock::Noise(nb) = &blk.ops[a].kind {
                let links: [Option<Link>; 5] = core::array::from_fn(|i| (i < n).then(|| link(i)));
                self.noise_op(blk, a, dm, nb, &links, &mut env[a], &mut rest[0], out);
                k += 1;
                continue;
            }
            let mut x = self.lane(blk, a, dm);
            let pair = k + 1 < OPS
                && n <= 2
                && is_table(&blk.ops[plan.order[k + 1] as usize % OPS])
                && !silent(&blk.ops[plan.order[k + 1] as usize % OPS])
                && {
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
        // A silent op's row was never written: zeros.
        for (op, row) in TapOp::ALL.iter().zip(taps.rows_mut()) {
            if let Some(row) = row {
                for (r, &y) in row.iter_mut().zip(&bufs[pos[op.op_index()]]) {
                    *r = tap_sample(y);
                }
            }
        }
        let mut norm = blk.norm_from;
        let dnorm = (blk.norm_to - blk.norm_from) * STEP;
        for s in out.iter_mut() {
            *s *= norm;
            norm += dnorm;
        }
    }

    /// A table operator's lane (`render_ops` sends noise to `noise_lane`).
    #[inline(always)]
    fn lane(&self, blk: &KernelBlock, op: usize, dm: f32) -> OpState {
        let (plan, o) = (blk.plan, &blk.ops[op]);
        let (lo, hi, xfade_from, xfade_to) = tables(o);
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
            xfade: xfade_from,
            dxfade: (xfade_to - xfade_from) * STEP,
            lo,
            hi,
        }
    }

    #[inline(always)]
    fn noise_lane(&self, blk: &KernelBlock, op: usize, dm: f32, nb: &NoiseBlock) -> NoiseState {
        let (plan, o) = (blk.plan, &blk.ops[op]);
        NoiseState {
            phase: self.phase[op],
            inc: o.inc,
            gain: o.gain_from,
            dgain: (o.gain_to - o.gain_from) * STEP,
            carrier: blend(plan.carrier_a[op], plan.carrier_b[op], blk.morph_from),
            dcarrier: (plan.carrier_b[op] - plan.carrier_a[op]) * dm,
            prev: self.out[op],
            hist: self.hist[op],
            n: Noise::new(self.rng[op], self.state[op].poles(), nb),
        }
    }

    /// A noise operator's block: its links summed first, in the order
    /// `tick` adds them, then `run_noise`. Out of line, so its row stays
    /// off the callers' frames.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn noise_op(
        &mut self,
        blk: &KernelBlock,
        op: usize,
        dm: f32,
        nb: &NoiseBlock,
        links: &[Option<Link>; 5],
        env: &mut OpEnv,
        dst: &mut [f32; BLOCK_SIZE],
        acc: &mut [f32; BLOCK_SIZE],
    ) {
        let mut pm = [0.0f32; BLOCK_SIZE];
        for mut l in links.iter().flatten().copied() {
            for (p, &x) in pm.iter_mut().zip(l.src) {
                *p += l.w * x;
                l.w += l.dw;
            }
        }
        let mut x = self.noise_lane(blk, op, dm, nb);
        run_noise(&mut x, &pm, env, dst, acc);
        self.keep_noise(op, &x);
    }

    #[inline(always)]
    fn keep_noise(&mut self, op: usize, st: &NoiseState) {
        self.phase[op] = st.phase;
        self.out[op] = st.prev;
        self.hist[op] = st.hist;
        self.keep_poles(op, &st.n);
    }

    fn keep_poles(&mut self, op: usize, n: &Noise) {
        self.rng[op] = n.rng;
        self.state[op] = KindState::Noise { y1: n.y1, y2: n.y2 };
    }

    /// An operator silent all block outputs zeros (its row stays 0), so
    /// only its phase and envelope move; the output is unchanged.
    fn idle(&mut self, o: &OpBlock, op: usize, env: &mut OpEnv) {
        let mut run = env.run();
        for _ in 0..BLOCK_SIZE {
            run.step(env);
        }
        env.store(run);
        self.phase[op] = self.phase[op].wrapping_add(o.inc.wrapping_mul(BLOCK_SIZE as u32));
        self.out[op] = 0.0;
        self.hist[op] = 0.0;
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
        taps: &mut TapRows,
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
            let (src, feedback) = match &o.kind {
                &KindBlock::Table {
                    lo,
                    hi,
                    xfade_from,
                    xfade_to,
                } => (
                    Src::Table {
                        xfade: xfade_from,
                        dxfade: (xfade_to - xfade_from) * STEP,
                        lo,
                        hi,
                    },
                    o.feedback * 0.5 * PHASE_UNITS,
                ),
                KindBlock::Noise(nb) => (
                    Src::Noise(Noise::new(self.rng[op], self.state[op].poles(), nb)),
                    0.0,
                ),
            };
            Lane {
                op,
                phase: self.phase[op],
                inc: o.inc,
                gain: o.gain_from,
                dgain: (o.gain_to - o.gain_from) * STEP,
                carrier: blend(plan.carrier_a[op], plan.carrier_b[op], blk.morph_from),
                dcarrier: (plan.carrier_b[op] - plan.carrier_a[op]) * dm,
                feedback,
                hist: self.hist[op],
                src,
                edges: (plan.starts[k] as usize, plan.starts[k + 1] as usize),
                env: env[op],
            }
        });
        // Silent lanes are skipped, as in `render_ops` and `AlgoEngine::cost`.
        // Only on the first sample does a silent operator's last output still
        // reach the lanes before it; from its turn on it reads 0.
        let mut live = [0usize; OPS];
        let mut n = 0;
        for (k, l) in lanes.iter().enumerate() {
            if !silent(&blk.ops[l.op]) {
                live[n] = k;
                n += 1;
            }
        }
        let mut norm = blk.norm_from;
        let dnorm = (blk.norm_to - blk.norm_from) * STEP;
        let (first, rest) = out.split_at_mut(1);
        let mut acc = 0.0f32;
        for l in lanes.iter_mut() {
            if silent(&blk.ops[l.op]) {
                self.out[l.op & 7] = 0.0;
            } else {
                acc += self.step(l, &mut w, &dw, &src);
            }
        }
        first[0] = acc * norm;
        norm += dnorm;
        self.tap(taps, 0);
        for (i, s) in rest.iter_mut().enumerate() {
            let mut acc = 0.0f32;
            for &k in &live[..n] {
                acc += self.step(&mut lanes[k], &mut w, &dw, &src);
            }
            *s = acc * norm;
            norm += dnorm;
            self.tap(taps, i + 1);
        }
        for l in &lanes {
            if silent(&blk.ops[l.op]) {
                self.idle(&blk.ops[l.op], l.op, &mut env[l.op]);
            } else {
                self.phase[l.op] = l.phase;
                self.hist[l.op] = l.hist;
                env[l.op] = l.env;
                if let Src::Noise(n) = &l.src {
                    self.keep_poles(l.op, n);
                }
            }
        }
    }

    /// Sample `s` of each wanted tap: its op's latest output (0 if silent).
    #[inline(always)]
    fn tap(&self, taps: &mut TapRows, s: usize) {
        for (op, row) in TapOp::ALL.iter().zip(taps.rows_mut()) {
            if let Some(row) = row {
                row[s] = tap_sample(self.out[op.op_index()]);
            }
        }
    }

    /// One sample of one lane; returns its carrier share.
    #[inline(always)]
    fn step(
        &mut self,
        l: &mut Lane,
        w: &mut [f32; MAX_EDGES],
        dw: &[f32; MAX_EDGES],
        src: &[u8; MAX_EDGES],
    ) -> f32 {
        let prev = self.out[l.op & 7];
        let mut pm = l.feedback * (prev + l.hist);
        for e in l.edges.0..l.edges.1.min(MAX_EDGES) {
            pm += w[e] * self.out[src[e] as usize & 7];
            w[e] += dw[e];
        }
        l.phase = l.phase.wrapping_add(l.inc);
        let p = l.phase.wrapping_add((pm as i32 as u32) << 8);
        let y = match &mut l.src {
            Src::Table {
                xfade,
                dxfade,
                lo,
                hi,
            } => {
                let y = read(lo, hi, *xfade, p) * l.env.step() * l.gain;
                *xfade += *dxfade;
                y
            }
            Src::Noise(n) => n.tone(p) * l.env.step() * l.gain,
        };
        l.gain += l.dgain;
        l.hist = prev;
        self.out[l.op & 7] = y;
        let c = l.carrier * y;
        l.carrier += l.dcarrier;
        c
    }
}

const STEP: f32 = 1.0 / BLOCK_SIZE as f32;

/// A tap row's sample: the op's output held to ±1 (a NaN to −1, as
/// `clampf`), by compare and select: `max`/`min` on a value read back from
/// memory cost twice the instructions.
#[inline(always)]
fn tap_sample(y: f32) -> f32 {
    let y = if y >= -1.0 { y } else { -1.0 };
    if y <= 1.0 { y } else { 1.0 }
}

fn silent(o: &OpBlock) -> bool {
    o.gain_from == 0.0 && o.gain_to == 0.0
}

fn is_table(o: &OpBlock) -> bool {
    matches!(o.kind, KindBlock::Table { .. })
}

/// A table operator's tables and crossfade; W1 for noise, which reads its
/// own lane.
#[inline(always)]
fn tables(o: &OpBlock) -> (&'static Table, &'static Table, f32, f32) {
    match o.kind {
        KindBlock::Table {
            lo,
            hi,
            xfade_from,
            xfade_to,
        } => (lo, hi, xfade_from, xfade_to),
        KindBlock::Noise(_) => {
            let w = WaveId::W1.table(0);
            (w, w, 0.0, 0.0)
        }
    }
}

/// A noise operator's generator, poles and ramps (spec § 4). The input
/// gain ramps as `g = c·k`, the first pole's real gain: ramping `k` alone
/// would scale wide noise by a narrow band's `k` mid-ramp, a burst the
/// poles then hold (+9 dB over 10 ms, 4 kHz to 20 Hz).
#[derive(Clone, Copy)]
struct Noise {
    rng: u32,
    y1: f32,
    y2: f32,
    c: f32,
    dc: f32,
    g: f32,
    dg: f32,
    alpha: f32,
    dalpha: f32,
    beta: f32,
    dbeta: f32,
    sine: &'static Table,
}

impl Noise {
    fn new(rng: u32, (y1, y2): (f32, f32), b: &NoiseBlock) -> Self {
        let d = |(from, to): (f32, f32)| (to - from) * STEP;
        Noise {
            rng,
            y1,
            y2,
            c: b.c.0,
            dc: d(b.c),
            g: b.c.0 * b.k.0,
            dg: d((b.c.0 * b.k.0, b.c.1 * b.k.1)),
            alpha: b.alpha.0,
            dalpha: d(b.alpha),
            beta: b.beta.0,
            dbeta: d(b.beta),
            sine: WaveId::W1.table(0),
        }
    }

    /// One sample of band noise and tone, ring-modulated by the centre's
    /// sine at phase `p`: scaled before the poles, clamped after them.
    #[inline(always)]
    fn tone(&mut self, p: u32) -> f32 {
        let mut r = self.rng;
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        self.rng = r;
        let x = self.g * (r as i32 as f32 * (1.0 / 2_147_483_648.0));
        self.y1 += x - self.c * self.y1;
        self.y2 += self.c * (self.y1 - self.y2);
        let y = (self.alpha * clampf(self.y2, -NOISE_CLAMP, NOISE_CLAMP) + self.beta)
            * sine_at(self.sine, p);
        self.g += self.dg;
        self.c += self.dc;
        self.alpha += self.dalpha;
        self.beta += self.dbeta;
        y
    }
}

#[derive(Clone, Copy)]
struct NoiseState {
    phase: u32,
    inc: u32,
    gain: f32,
    dgain: f32,
    carrier: f32,
    dcarrier: f32,
    prev: f32,
    hist: f32,
    n: Noise,
}

/// A noise operator's block, as `run_one` renders a table's: `pm`, its
/// links' sum, into the centre's phase; no feedback.
fn run_noise(
    st: &mut NoiseState,
    pm: &[f32; BLOCK_SIZE],
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (mut x, mut run) = (*st, env.run());
    for s in 0..BLOCK_SIZE {
        x.phase = x.phase.wrapping_add(x.inc);
        let p = x.phase.wrapping_add((pm[s] as i32 as u32) << 8);
        let y = x.n.tone(p) * run.step(env) * x.gain;
        x.gain += x.dgain;
        x.hist = x.prev;
        x.prev = y;
        dst[s] = y;
        acc[s] += x.carrier * y;
        x.carrier += x.dcarrier;
    }
    *st = x;
    env.store(run);
}

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

/// W1 at mip 0, interpolated, at unit scale: a noise band's centre.
pub fn sine(p: u32) -> f32 {
    sine_at(WaveId::W1.table(0), p)
}

#[inline(always)]
fn sine_at(t: &Table, p: u32) -> f32 {
    let f = (p & 0x00ff_ffff) as f32 * (1.0 / PHASE_UNITS);
    let (a, b) = pair(t, (p >> 24) as usize);
    (a + (b - a) * f) * SAMPLE_SCALE
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
