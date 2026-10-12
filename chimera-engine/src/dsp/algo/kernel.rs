//! The per-sample loop: six phase-modulation operators on an `EvalPlan`.

use crate::dsp::algo::env::{EnvRun, OpEnv};
use crate::dsp::algo::formant::{GrainPm, PeakRamp, Skirt, delay_phase, grain_delay, peak_track};
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
    /// `FEEDBACK_CYCLES` of the operator's setting; noise ignores it, and
    /// a formant ramps its own (`FormantBlock::feedback`).
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
    Formant(FormantBlock),
}

/// A formant operator's grain law this block (spec § 3). A grain that
/// starts in the block latches it; the clock is the operator's `inc`.
#[derive(Clone, Copy, Debug)]
pub struct FormantBlock {
    /// `1 / inc`.
    pub inv_inc: f32,
    /// FC per sample; FC < 24 kHz, so < 2^31.
    pub inc_c: u32,
    /// The window's length's reciprocal, 1/periods.
    pub inv_d: f32,
    /// Half the slack `2 − D` and the PM slope (`GrainPm`).
    pub h: f32,
    pub inv_h: f32,
    /// `delay_scale(h, FC/f0)`: the PM-moved carrier offset's scale.
    pub m: u32,
    /// The carrier's start at its grain's rest delay, so its zero sits at
    /// the window's centre: `cycles_to_phase(−FC/f0)` mid-slack (`h + D/2`
    /// is one period), and `cycles_to_phase(−D·FC/(2·f0))` for the reset
    /// grain (delay 0).
    pub c0: [u32; 2],
    /// `g_c / g_m`, ramped into the carrier weight.
    pub c_ratio: (f32, f32),
    /// The share of the links' PM taken, ramped: into both the grain delay
    /// and the carrier, so the delay law holds at the reduced depth.
    pub pm: (f32, f32),
    /// FDBK's cycles `(from, to)`, ramped into the carrier only.
    pub feedback: (f32, f32),
    /// The modulator gain `g_m` across the block, under the peak bound as
    /// the drive moves (`peak_ramp`).
    pub gm: PeakRamp,
    /// The latched SKIRT.
    pub skirt: Skirt,
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
    Formant(FormantState),
}

impl KindState {
    /// The poles: closed (0) on any state but noise's.
    fn poles(self) -> (f32, f32) {
        match self {
            KindState::Noise { y1, y2 } => (y1, y2),
            KindState::Table | KindState::Formant(_) => (0.0, 0.0),
        }
    }

    /// The lanes: closed on any state but a formant's.
    fn grains(self) -> FormantState {
        match self {
            KindState::Formant(f) => f,
            KindState::Table | KindState::Noise { .. } => FormantState::CLOSED,
        }
    }
}

/// A formant lane: one grain, latched at its start (Ruling 5).
#[derive(Clone, Copy, Debug)]
struct Grain {
    c: u32,
    inv_d: f32,
    pm: GrainPm,
    m: u32,
    live: bool,
}

/// Two lanes, alternating: each wrap restarts the one that fired less
/// recently, two periods after its last start.
#[derive(Clone, Copy, Debug)]
struct FormantState {
    lanes: [Grain; 2],
    newest: u8,
    /// Fire lane 0 on the next sample, at no delay (I2): set by `reset`,
    /// with the phase at 0.
    fire_now: bool,
}

impl FormantState {
    const DEAD: Grain = Grain {
        c: 0,
        inv_d: 0.0,
        pm: GrainPm {
            rest: 0.0,
            h: 0.0,
            inv_h: 0.0,
            beta: 0.0,
        },
        m: 0,
        live: false,
    };

    const CLOSED: FormantState = FormantState {
        lanes: [Self::DEAD; 2],
        newest: 1,
        fire_now: false,
    };
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
    /// Restarts that found their lane's window still open.
    #[cfg(any(test, feature = "test-support"))]
    cuts: u32,
    /// Formants on `formant_tick_ref`, the spans' reference.
    #[cfg(any(test, feature = "test-support"))]
    per_sample: bool,
}

#[derive(Clone, Copy)]
struct Lane<'a> {
    op: usize,
    phase: u32,
    inc: u32,
    gain: f32,
    dgain: f32,
    carrier: f32,
    dcarrier: f32,
    feedback: f32,
    hist: f32,
    src: Src<'a>,
    edges: (usize, usize),
    env: OpEnv,
}

/// What a lane reads: its tables or its noise.
#[derive(Clone, Copy)]
enum Src<'a> {
    Table {
        xfade: f32,
        dxfade: f32,
        lo: &'static Table,
        hi: &'static Table,
    },
    Noise(Noise),
    Formant(Grains<'a>, FormantRamps),
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
            #[cfg(any(test, feature = "test-support"))]
            cuts: 0,
            #[cfg(any(test, feature = "test-support"))]
            per_sample: false,
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
    /// its generator running on; a formant's lanes closed, its first grain
    /// firing on the next sample.
    pub fn reset(&mut self, op: usize, kind: OpKind) {
        self.phase[op] = 0;
        self.out[op] = 0.0;
        self.hist[op] = 0.0;
        self.state[op] = match kind {
            OpKind::Noise => KindState::Noise { y1: 0.0, y2: 0.0 },
            OpKind::Formant => KindState::Formant(FormantState {
                fire_now: true,
                ..FormantState::CLOSED
            }),
            OpKind::Table(_) => KindState::Table,
        };
    }

    /// Restarts so far that found their lane's window open.
    #[cfg(any(test, feature = "test-support"))]
    pub fn grain_cuts(&self) -> u32 {
        self.cuts
    }

    /// Formant operators render on the per-sample reference from now on.
    #[cfg(any(test, feature = "test-support"))]
    pub fn formants_per_sample(&mut self) {
        self.per_sample = true;
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
            if let KindBlock::Formant(fb) = &blk.ops[a].kind {
                let links: [Option<Link>; 5] = core::array::from_fn(|i| (i < n).then(|| link(i)));
                self.formant_op(blk, a, dm, fb, &links, &mut env[a], &mut rest[0], out);
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
        let mut x = self.noise_lane(blk, op, dm, nb);
        if links[0].is_some() {
            run_noise::<1>(&mut x, &link_sum(links), env, dst, acc);
        } else {
            run_noise::<0>(&mut x, &ZEROS, env, dst, acc);
        }
        self.keep_noise(op, &x);
    }

    /// A formant operator's block: with links, their sum first, in the
    /// order `tick` adds them, then `run_moving`; with none, `run_formant`
    /// at the latched SKIRT. Out of line, so its row stays off the
    /// callers' frames; four span variants and one moving loop.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn formant_op(
        &mut self,
        blk: &KernelBlock,
        op: usize,
        dm: f32,
        fb: &FormantBlock,
        links: &[Option<Link>; 5],
        env: &mut OpEnv,
        dst: &mut [f32; BLOCK_SIZE],
        acc: &mut [f32; BLOCK_SIZE],
    ) {
        let linked = links[0].is_some();
        let sum;
        let pm = if linked {
            sum = link_sum(links);
            &sum
        } else {
            &ZEROS
        };
        let mut x = self.formant_lane(blk, op, dm, fb);
        #[cfg(any(test, feature = "test-support"))]
        if self.per_sample {
            match fb.skirt {
                Skirt::S0 => formant_tick_ref::<0>(&mut x, pm, env, dst, acc),
                Skirt::S1 => formant_tick_ref::<1>(&mut x, pm, env, dst, acc),
                Skirt::S2 => formant_tick_ref::<2>(&mut x, pm, env, dst, acc),
                Skirt::S3 => formant_tick_ref::<3>(&mut x, pm, env, dst, acc),
            }
            self.keep_formant(op, &x);
            return;
        }
        match (linked, fb.skirt) {
            (true, _) => run_moving(&mut x, pm, env, dst, acc),
            (false, Skirt::S0) => run_formant::<0>(&mut x, env, dst, acc),
            (false, Skirt::S1) => run_formant::<1>(&mut x, env, dst, acc),
            (false, Skirt::S2) => run_formant::<2>(&mut x, env, dst, acc),
            (false, Skirt::S3) => run_formant::<3>(&mut x, env, dst, acc),
        }
        self.keep_formant(op, &x);
    }

    #[inline(always)]
    fn formant_lane<'a>(
        &self,
        blk: &KernelBlock,
        op: usize,
        dm: f32,
        fb: &'a FormantBlock,
    ) -> FormantLane<'a> {
        let (plan, o) = (blk.plan, &blk.ops[op]);
        FormantLane {
            phase: self.phase[op],
            inc: o.inc,
            c: Carry {
                gain: o.gain_from,
                dgain: (o.gain_to - o.gain_from) * STEP,
                carrier: blend(plan.carrier_a[op], plan.carrier_b[op], blk.morph_from),
                dcarrier: (plan.carrier_b[op] - plan.carrier_a[op]) * dm,
                prev: self.out[op],
                hist: self.hist[op],
                r: FormantRamps::new(fb),
            },
            g: Grains::new(self.state[op].grains(), fb),
        }
    }

    #[inline(always)]
    fn keep_formant(&mut self, op: usize, st: &FormantLane) {
        self.phase[op] = st.phase;
        self.out[op] = st.c.prev;
        self.hist[op] = st.c.hist;
        self.keep_grains(op, &st.g);
    }

    fn keep_grains(&mut self, op: usize, g: &Grains) {
        self.state[op] = KindState::Formant(g.st);
        #[cfg(any(test, feature = "test-support"))]
        {
            self.cuts += g.cuts;
        }
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
    /// only its phase and envelope move; the output is unchanged. A
    /// formant's lanes close, and a reset's first grain waits for the wrap.
    fn idle(&mut self, o: &OpBlock, op: usize, env: &mut OpEnv) {
        if let KindState::Formant(f) = &mut self.state[op] {
            *f = FormantState::CLOSED;
        }
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
                KindBlock::Formant(fb) => (
                    Src::Formant(
                        Grains::new(self.state[op].grains(), fb),
                        FormantRamps::new(fb),
                    ),
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
                match &l.src {
                    Src::Noise(n) => self.keep_poles(l.op, n),
                    Src::Formant(g, _) => self.keep_grains(l.op, g),
                    Src::Table { .. } => {}
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
        let (ph, wrapped) = l.phase.overflowing_add(l.inc);
        l.phase = ph;
        let pm_phase = (pm as i32 as u32) << 8;
        let p = ph.wrapping_add(pm_phase);
        let (y, weight) = match &mut l.src {
            Src::Table {
                xfade,
                dxfade,
                lo,
                hi,
            } => {
                let y = read(lo, hi, *xfade, p) * l.env.step() * l.gain;
                *xfade += *dxfade;
                (y, l.carrier)
            }
            Src::Noise(n) => (n.tone(p) * l.env.step() * l.gain, l.carrier),
            // `pm` is the links' alone: the lane's feedback is 0.
            Src::Formant(g, r) => {
                let pm = g.links(pm);
                let pm_c = pm + r.feedback() * (prev + l.hist);
                let y = g.tick_at(ph, wrapped, pm, (pm_c as i32 as u32) << 8)
                    * l.env.step()
                    * l.gain
                    * r.gm();
                (y, r.weight(l.carrier))
            }
        };
        l.gain += l.dgain;
        l.hist = prev;
        self.out[l.op & 7] = y;
        let c = weight * y;
        l.carrier += l.dcarrier;
        c
    }
}

const STEP: f32 = 1.0 / BLOCK_SIZE as f32;

/// A block of no PM: what an operator with no links reads (and doesn't).
static ZEROS: [f32; BLOCK_SIZE] = [0.0; BLOCK_SIZE];

/// The links' PM, summed in the order `tick` adds them, from 0.0: the
/// first link's term is its sum's start, so no row is cleared first.
fn link_sum(links: &[Option<Link>; 5]) -> [f32; BLOCK_SIZE] {
    let mut ls = links.iter().flatten().copied();
    let Some(mut l) = ls.next() else {
        return ZEROS;
    };
    let mut pm: [f32; BLOCK_SIZE] = core::array::from_fn(|s| {
        let p = 0.0 + l.w * l.src[s];
        l.w += l.dw;
        p
    });
    for mut l in ls {
        for (p, &x) in pm.iter_mut().zip(l.src) {
            *p += l.w * x;
            l.w += l.dw;
        }
    }
    pm
}

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

/// A table operator's tables and crossfade; W1 for noise and formants,
/// which read their own lanes.
#[inline(always)]
fn tables(o: &OpBlock) -> (&'static Table, &'static Table, f32, f32) {
    match o.kind {
        KindBlock::Table {
            lo,
            hi,
            xfade_from,
            xfade_to,
        } => (lo, hi, xfade_from, xfade_to),
        KindBlock::Noise(_) | KindBlock::Formant(_) => {
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
/// links' sum, into the centre's phase (`N` 1; 0 with no links, `pm`
/// unread); no feedback.
#[cfg_attr(feature = "span-count", inline(never))]
fn run_noise<const N: usize>(
    st: &mut NoiseState,
    pm: &[f32; BLOCK_SIZE],
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (mut x, mut run) = (*st, env.run());
    for s in 0..BLOCK_SIZE {
        x.phase = x.phase.wrapping_add(x.inc);
        // No links: `pm` is 0.0, so its phase is 0.
        let p = if N == 0 {
            x.phase
        } else {
            x.phase.wrapping_add((pm[s] as i32 as u32) << 8)
        };
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

/// A formant operator's lanes running one block: its latched state, the
/// block's law for grains that start in it, and the PM share's ramp.
#[derive(Clone, Copy)]
struct Grains<'a> {
    st: FormantState,
    b: &'a FormantBlock,
    pm_k: f32,
    dpm_k: f32,
    sine: &'static Table,
    #[cfg(any(test, feature = "test-support"))]
    cuts: u32,
}

impl<'a> Grains<'a> {
    fn new(st: FormantState, b: &'a FormantBlock) -> Self {
        Grains {
            st,
            b,
            pm_k: b.pm.0,
            dpm_k: (b.pm.1 - b.pm.0) * STEP,
            sine: WaveId::W1.table(0),
            #[cfg(any(test, feature = "test-support"))]
            cuts: 0,
        }
    }

    /// The links' PM `pm` at this sample's share; the share steps on.
    #[inline(always)]
    fn links(&mut self, pm: f32) -> f32 {
        let p = pm * self.pm_k;
        self.pm_k += self.dpm_k;
        p
    }

    /// Each lane's periods behind the newest's start: 0 or 1.
    #[inline(always)]
    fn older(&self) -> [f32; 2] {
        if self.st.newest == 0 {
            [0.0, 1.0]
        } else {
            [1.0, 0.0]
        }
    }

    /// `tick` at the latched SKIRT, chosen per sample (`render_samples`,
    /// the delayed plans' path): out of line, so one copy of the four
    /// serves every lane.
    #[inline(never)]
    fn tick_at(&mut self, ph: u32, wrapped: bool, pm: f32, pm_phase: u32) -> f32 {
        match self.b.skirt {
            Skirt::S0 => self.tick::<0>(ph, wrapped, pm, pm_phase),
            Skirt::S1 => self.tick::<1>(ph, wrapped, pm, pm_phase),
            Skirt::S2 => self.tick::<2>(ph, wrapped, pm, pm_phase),
            Skirt::S3 => self.tick::<3>(ph, wrapped, pm, pm_phase),
        }
    }

    /// One sample of both lanes, the raw grain signal (spec § 3): `ph` is
    /// the unmodulated clock after this sample's step, `wrapped` whether
    /// the step carried, `pm` the links' PM (`PHASE_UNITS` a cycle), which
    /// delays each grain within its slack, and `pm_phase` the links' PM
    /// and the feedback, into the carrier.
    #[inline(always)]
    fn tick<const S: u8>(&mut self, ph: u32, wrapped: bool, pm: f32, pm_phase: u32) -> f32 {
        let pm_cycles = pm * (1.0 / PHASE_UNITS);
        if wrapped || self.st.fire_now {
            self.fire(ph, pm_cycles);
        }
        let (t, older) = (clock(ph), self.older());
        let (inc_c, sine) = (self.b.inc_c, self.sine);
        let mut y = 0.0f32;
        for (g, older) in self.st.lanes.iter_mut().zip(older) {
            g.moving::<S>(older, t, pm_cycles, pm_phase, inc_c, sine, &mut y);
        }
        y
    }

    /// A grain starts on the lane that fired less recently, latching the
    /// block's law. `_pm_cycles` is this sample's PM, for the cut count.
    /// Out of line: once a period, and one copy serves every SKIRT (in
    /// line under `span-count`, so `loop_count` counts the spans with it).
    #[cfg_attr(not(feature = "span-count"), inline(never))]
    #[cfg_attr(feature = "span-count", inline(always))]
    fn fire(&mut self, ph: u32, _pm_cycles: f32) {
        let st = &mut self.st;
        let k = 1 - (st.newest as usize & 1);
        #[cfg(any(test, feature = "test-support"))]
        {
            // Two periods since this lane's start.
            let g = &st.lanes[k];
            let o = grain_delay(g.pm.rest, g.pm.h, g.pm.moved(_pm_cycles));
            let x = (clock(ph) - (o - 2.0)) * g.inv_d;
            if g.live && crate::dsp::algo::formant::window(x, self.b.skirt) > 1e-6 {
                self.cuts += 1;
            }
        }
        let b = &self.b;
        // Samples since the wrap, or since the reset's phase 0 (`idle`
        // drops a fire it doesn't run): at most 1, so the carrier's
        // zero sits at the window's centre on the reset grain too (e = 0
        // would lag it a sample and lift a narrow grain's peak past 1).
        let e = ph as f32 * b.inv_inc;
        let reset = st.fire_now;
        st.lanes[k] = Grain {
            // e ≤ 1 and inc_c < 2^31: e·inc_c < 2^32, in u32's range.
            c: b.c0[reset as usize].wrapping_add((e * b.inc_c as f32) as u32),
            inv_d: b.inv_d,
            pm: GrainPm {
                rest: if reset { 0.0 } else { b.h },
                h: b.h,
                inv_h: b.inv_h,
                beta: if reset { -1.0 } else { 0.0 },
            },
            m: b.m,
            live: true,
        };
        st.newest = k as u8;
        st.fire_now = false;
    }
}

impl Grain {
    /// One sample of this lane into `y`, its start `older` periods before
    /// the newest's, `t` periods since the newest's. Delayed within its
    /// slack, never advanced: no grain is cut at any PM depth.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn moving<const S: u8>(
        &mut self,
        older: f32,
        t: f32,
        pm_cycles: f32,
        pm_phase: u32,
        inc_c: u32,
        sine: &Table,
        y: &mut f32,
    ) {
        if self.live {
            let f = self.pm.moved(pm_cycles);
            let x = (t - (grain_delay(self.pm.rest, self.pm.h, f) - older)) * self.inv_d;
            if x >= 1.0 {
                self.live = false;
            } else if x > 0.0 {
                let c = self
                    .c
                    .wrapping_add(pm_phase)
                    .wrapping_sub(delay_phase(f, self.m));
                *y += bell::<S>(x) * sine_at(sine, c);
            }
            self.c = self.c.wrapping_add(inc_c);
        }
    }
}

/// Periods since the newest lane's start at clock `ph`.
#[inline(always)]
fn clock(ph: u32) -> f32 {
    ph as f32 * (1.0 / 4_294_967_296.0)
}

/// `window(x, S)` on (0, 1): `4x(1 − x)` squared `S + 1` times,
/// straight-line per SKIRT.
#[inline(always)]
fn bell<const S: u8>(x: f32) -> f32 {
    squares::<S>(4.0 * x * (1.0 - x))
}

/// `bell` held to 0 off (0, 1), where `4x(1 − x)` is not positive (and
/// for NaN): `window` with no branch.
#[inline(always)]
fn bell0<const S: u8>(x: f32) -> f32 {
    let u = 4.0 * x * (1.0 - x);
    squares::<S>(if u > 0.0 { u } else { 0.0 })
}

#[inline(always)]
fn squares<const S: u8>(mut u: f32) -> f32 {
    u *= u;
    if S >= 1 {
        u *= u;
    }
    if S >= 2 {
        u *= u;
    }
    if S >= 3 {
        u *= u;
    }
    u
}

/// A formant operator's ramps beyond a table's: FDBK into the carrier,
/// the carrier ratio, and the modulator gain tracking its peak bound.
#[derive(Clone, Copy)]
struct FormantRamps {
    fb: f32,
    dfb: f32,
    c_ratio: f32,
    dc_ratio: f32,
    gm: f32,
    bound: f32,
    dbound: f32,
    floor: f32,
}

impl FormantRamps {
    fn new(b: &FormantBlock) -> Self {
        let fb = |c: f32| c * 0.5 * PHASE_UNITS;
        FormantRamps {
            fb: fb(b.feedback.0),
            dfb: (fb(b.feedback.1) - fb(b.feedback.0)) * STEP,
            c_ratio: b.c_ratio.0,
            dc_ratio: (b.c_ratio.1 - b.c_ratio.0) * STEP,
            gm: b.gm.g,
            bound: b.gm.u,
            dbound: b.gm.du,
            floor: b.gm.floor,
        }
    }

    /// This sample's feedback weight; the ramp steps on.
    #[inline(always)]
    fn feedback(&mut self) -> f32 {
        let f = self.fb;
        self.fb += self.dfb;
        f
    }

    /// This sample's modulator gain; it tracks the next sample's bound.
    #[inline(always)]
    fn gm(&mut self) -> f32 {
        let g = self.gm;
        self.bound += self.dbound;
        self.gm = peak_track(g, self.bound, self.floor);
        g
    }

    /// The carrier's weight this sample; the ratio steps on.
    #[inline(always)]
    fn weight(&mut self, carrier: f32) -> f32 {
        let w = carrier * self.c_ratio;
        self.c_ratio += self.dc_ratio;
        w
    }
}

/// What a formant operator's samples carry from one to the next: its
/// gain, carrier weight, last two outputs and ramps, all scalars.
#[derive(Clone, Copy)]
struct Carry {
    gain: f32,
    dgain: f32,
    carrier: f32,
    dcarrier: f32,
    prev: f32,
    hist: f32,
    r: FormantRamps,
}

impl Carry {
    /// The carrier's PM this sample: the links' `pm` and the feedback.
    #[inline(always)]
    fn pm_phase(&mut self, pm: f32) -> u32 {
        let pm_c = pm + self.r.feedback() * (self.prev + self.hist);
        (pm_c as i32 as u32) << 8
    }

    /// One sample's output from the raw grain sum `raw`, as every path
    /// ends it: envelope, gain and modulator gain, into `dst` and the
    /// carrier sum.
    #[inline(always)]
    fn out(&mut self, raw: f32, run: &mut EnvRun, env: &mut OpEnv, dst: &mut f32, acc: &mut f32) {
        let y = raw * run.step(env) * self.gain * self.r.gm();
        self.gain += self.dgain;
        self.hist = self.prev;
        self.prev = y;
        *dst = y;
        *acc += self.r.weight(self.carrier) * y;
        self.carrier += self.dcarrier;
    }
}

/// A formant operator's block state: its clock, carry and lanes, worked
/// in place (by `&mut`, never copied whole: it is 160 bytes).
struct FormantLane<'a> {
    phase: u32,
    inc: u32,
    c: Carry,
    g: Grains<'a>,
}

/// The per-sample formant path (Tasks 10–11): the reference the spans
/// match bit for bit.
#[cfg(any(test, feature = "test-support"))]
fn formant_tick_ref<const S: u8>(
    st: &mut FormantLane,
    pm: &[f32; BLOCK_SIZE],
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (x, mut run) = (st, env.run());
    for s in 0..BLOCK_SIZE {
        let (ph, wrapped) = x.phase.overflowing_add(x.inc);
        x.phase = ph;
        let pm = x.g.links(pm[s]);
        let pm_phase = x.c.pm_phase(pm);
        let raw = x.g.tick::<S>(ph, wrapped, pm, pm_phase);
        x.c.out(raw, &mut run, env, &mut dst[s], &mut acc[s]);
    }
    env.store(run);
}

/// A formant operator's block with links, sample by sample (spec § 3): its
/// lanes' delays move with the PM, so `Grains::tick_at` runs each sample,
/// the SKIRT chosen there (one copy, shared with `render_samples`). Bit
/// for bit `formant_tick_ref`.
#[cfg_attr(feature = "span-count", inline(never))]
fn run_moving(
    st: &mut FormantLane,
    pm: &[f32; BLOCK_SIZE],
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (x, mut run) = (st, env.run());
    for ((&pm, d), a) in pm.iter().zip(dst).zip(acc) {
        let (ph, wrapped) = x.phase.overflowing_add(x.inc);
        x.phase = ph;
        let p = x.g.links(pm);
        let pm_phase = x.c.pm_phase(p);
        let raw = x.g.tick_at(ph, wrapped, p, pm_phase);
        x.c.out(raw, &mut run, env, d, a);
    }
    env.store(run);
}

/// A formant operator's block with no links, in spans between grain
/// starts (spec § 3, Task 11b), bit for bit `formant_tick_ref`: each span
/// starts at a wrap (its fire) or the block's start, and no wrap falls
/// inside one, so the lanes are fixed through it.
#[cfg_attr(feature = "span-count", inline(never))]
fn run_formant<const S: u8>(
    st: &mut FormantLane,
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
) {
    let (x, mut run) = (st, env.run());
    let mut s = 0;
    while s < BLOCK_SIZE {
        let (ph, wrapped) = x.phase.overflowing_add(x.inc);
        // Samples after `s` before the next wrap: none with no clock.
        let room = (u32::MAX - ph).checked_div(x.inc).unwrap_or(u32::MAX);
        let end = BLOCK_SIZE.min(s + 1 + room.min(BLOCK_SIZE as u32) as usize);
        if wrapped || x.g.st.fire_now {
            x.g.fire(ph, 0.0);
        }
        still_span::<S>(x, &mut run, env, dst, acc, s, end, ph);
        // The clock at the span's last sample.
        x.phase = ph.wrapping_add(((end - 1 - s) as u32).wrapping_mul(x.inc));
        s = end;
    }
    env.store(run);
}

/// Samples `s..end` of a span with no links, `ph` the clock at `s`. With
/// no PM each lane's delay is fixed for its grain, so its window position
/// `x` only rises through the span, and `bell0` opens and closes it with
/// no test. A closed lane runs at `x` 0: silent.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn still_span<const S: u8>(
    x: &mut FormantLane,
    run: &mut EnvRun,
    env: &mut OpEnv,
    dst: &mut [f32; BLOCK_SIZE],
    acc: &mut [f32; BLOCK_SIZE],
    s: usize,
    end: usize,
    ph: u32,
) {
    let (inc, inc_c, sine) = (x.inc, x.g.b.inc_c, x.g.sine);
    // Per lane: its delay less its lag `q`, its `1/D` and its carrier,
    // as `moving` reads them at no PM, bit for bit: `pm` is +0, so `g` is
    // `delay_shape(β) − β`, +0 at β 0 and −1 (exact), the delay `rest +
    // h·0` is `rest` and `delay_phase(+0, m)` is 0.
    let older = x.g.older();
    let at = |k: usize| {
        let g = &x.g.st.lanes[k];
        if g.live {
            (g.pm.rest - older[k], g.inv_d, g.c)
        } else {
            (0.0, 0.0, 0)
        }
    };
    let ((q0, i0, mut c0), (q1, i1, mut c1)) = (at(0), at(1));
    let mut c = x.c;
    // `s..end` lies in the block: `get` keeps a panic out of the loop.
    let (Some(dst), Some(acc)) = (dst.get_mut(s..end), acc.get_mut(s..end)) else {
        return;
    };
    let mut p = ph;
    for (d, a) in dst.iter_mut().zip(acc) {
        let pm_phase = c.pm_phase(0.0);
        let t = clock(p);
        let mut y = 0.0f32;
        y += bell0::<S>((t - q0) * i0) * sine_at(sine, c0.wrapping_add(pm_phase));
        y += bell0::<S>((t - q1) * i1) * sine_at(sine, c1.wrapping_add(pm_phase));
        (c0, c1) = (c0.wrapping_add(inc_c), c1.wrapping_add(inc_c));
        c.out(y, run, env, d, a);
        p = p.wrapping_add(inc);
    }
    x.c = c;
    // Each live lane stepped every sample; closed if its last `x` reached
    // 1 (`x` rises, so it closed there or before, and is not read again).
    let last = clock(ph.wrapping_add(((end - 1 - s) as u32).wrapping_mul(inc)));
    let n = ((end - s) as u32).wrapping_mul(inc_c);
    for (g, (q, i)) in x.g.st.lanes.iter_mut().zip([(q0, i0), (q1, i1)]) {
        if g.live {
            g.c = g.c.wrapping_add(n);
            // Not `<`: a NaN read keeps the lane live, as the per-sample path does.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            {
                g.live = !((last - q) * i >= 1.0);
            }
        }
    }
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

#[cfg_attr(not(feature = "span-count"), inline(always))]
#[cfg_attr(feature = "span-count", inline(never))]
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
