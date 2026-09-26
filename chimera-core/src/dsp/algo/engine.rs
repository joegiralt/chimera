//! `AlgoEngine`: the imperative shell around the pure core.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::{AlgoId, plan};
use crate::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use crate::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use crate::dsp::algo::math::exp2;
use crate::dsp::algo::morph::{Morph, carrier_norm, incoming};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::{EvalPlan, OPS};
use crate::dsp::algo::tx::{FEEDBACK_CYCLES, detune_factor, level_gain, ratio};
use crate::dsp::algo::waves::{WaveId, mip_position, mip_step};
use crate::hw::{BLOCK_SIZE, Cost};
use crate::in_place::by_value;
use crate::{MidiNote, Velocity};

/// LEVEL steps (3 dB) one VELOCITY step takes off at velocity 0.
const VELOCITY_STEPS: f32 = 4.0;

/// The block's MORPH and LEVELs after modulation, unrounded, in the stored
/// ranges (ADR 0010's offset formula).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlgoLive {
    pub morph: f32,
    pub level: [f32; OPS],
}

impl AlgoLive {
    pub fn from_params(p: &AlgoParams) -> Self {
        Self {
            morph: p.morph as f32,
            level: core::array::from_fn(|i| p.ops[i].level as f32),
        }
    }

    /// Takes a mod offset aimed at MORPH or a LEVEL (`false` for any other
    /// destination, which the voice applies to its blocks as before).
    pub fn offset(&mut self, addr: ParamAddr, off: f32) -> bool {
        let slot = match (addr.block, addr.param) {
            (BlockRef::Algo, AlgoParams::MORPH) => &mut self.morph,
            (BlockRef::AlgoOp(op), AlgoOpParams::LEVEL) => &mut self.level[op.index()],
            _ => return false,
        };
        if let Some(s) = addr.spec() {
            *slot = (*slot + off * (s.max - s.min)).clamp(s.min, s.max);
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swap {
    Idle,
    /// `waves`: mask of operators whose WAVE changed.
    Ducking {
        alg: bool,
        waves: u8,
    },
    Rising,
}

pub struct AlgoEngine {
    kernel: Kernel,
    env: [OpEnv; OPS],
    /// Rebuilt only when ALG A or B changes.
    plan: EvalPlan,
    plan_key: (AlgoId, AlgoId),
    waves: [WaveId; OPS],
    rates: [EnvRates; OPS],
    /// Gain, mip position, MORPH and output scale at the end of the last
    /// block: the next block's ramps start here.
    gain: [f32; OPS],
    mip: [f32; OPS],
    morph: f32,
    norm: f32,
    swap: Swap,
    note: MidiNote,
    velocity: f32,
    active: bool,
}

crate::in_place::field_list!(AlgoEngine => AlgoEngine {
    kernel, env, plan, plan_key, waves, rates, gain, mip, morph, norm, swap, note, velocity, active,
});

impl Default for AlgoEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AlgoEngine {
    pub const COST: Cost = Cost(560); // estimate

    pub fn new() -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(Self::init_in_place) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        let key = (AlgoId::T1, AlgoId::T1);
        let rates = AlgoParams::default().ops[0].rates();
        // SAFETY: `p` comes from a live `&mut MaybeUninit<Self>`, so it is
        // valid, aligned and unaliased; each field is written through a raw
        // place (no reference to uninitialised memory) exactly once, and
        // `field_list!` above fails to compile if a field is added, so every
        // field is written before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).kernel).write(Kernel::new());
            addr_of_mut!((*p).env).write([OpEnv::IDLE; OPS]);
            addr_of_mut!((*p).plan).write(plan(key.0, key.1));
            addr_of_mut!((*p).plan_key).write(key);
            addr_of_mut!((*p).waves).write([WaveId::W1; OPS]);
            addr_of_mut!((*p).rates).write([rates; OPS]);
            addr_of_mut!((*p).gain).write([0.0; OPS]);
            addr_of_mut!((*p).mip).write([0.0; OPS]);
            addr_of_mut!((*p).morph).write(0.0);
            addr_of_mut!((*p).norm).write(1.0);
            addr_of_mut!((*p).swap).write(Swap::Idle);
            addr_of_mut!((*p).note).write(MidiNote::A4);
            addr_of_mut!((*p).velocity).write(1.0);
            addr_of_mut!((*p).active).write(false);
            slot.assume_init_mut()
        }
    }

    /// A silent voice starts clean from the patch; a sounding one keeps its
    /// plan, gains and mips, and `render` moves them without a step. An
    /// idle operator is silent, so it takes its wave, gain and mip at once.
    pub fn note_on(
        &mut self,
        note: MidiNote,
        velocity: Velocity,
        p: &AlgoParams,
        sample_rate: u32,
    ) {
        let live = AlgoLive::from_params(p);
        let sr = sample_rate as f32;
        self.note = note;
        self.velocity = velocity.unit();
        let m = Morph::from_param(live.morph);
        if !self.active {
            self.adopt_alg(p);
            self.swap = Swap::Idle;
            self.env = [OpEnv::IDLE; OPS];
            self.morph = m.get();
            self.norm = carrier_norm(&self.plan, m);
        }
        let level = level_gains(&live);
        let cycles = self.cycles(p, sr);
        for i in 0..OPS {
            self.rates[i] = p.ops[i].rates();
            if self.env[i].is_idle() {
                self.kernel.reset(i);
                self.waves[i] = WaveId::clamped(p.ops[i].wave);
                self.gain[i] = self.target_gain(p, &live, i);
                self.mip[i] = self.mip_target(p, cycles, sr, m, &level, i);
            }
            self.env[i].note_on(EnvCoefs::new(self.rates[i], note, sr));
        }
        self.active = true;
    }

    pub fn note_off(&mut self) {
        for e in &mut self.env {
            e.note_off();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &AlgoParams,
        live: &AlgoLive,
        sample_rate: u32,
    ) {
        if !self.active {
            out.fill(0.0);
            return;
        }
        let sr = sample_rate as f32;
        self.begin_swap(p);
        for i in 0..OPS {
            let r = p.ops[i].rates();
            if r != self.rates[i] {
                self.rates[i] = r;
                self.env[i].set_coefs(EnvCoefs::new(r, self.note, sr));
            }
        }
        let m = Morph::from_param(live.morph);
        let level = level_gains(live);
        let cycles = self.cycles(p, sr);
        let mips: [(usize, f32, f32); OPS] = core::array::from_fn(|i| {
            mip_step(self.mip[i], self.mip_target(p, cycles, sr, m, &level, i))
        });
        let ops: [OpBlock; OPS] = core::array::from_fn(|i| {
            let o = &p.ops[i];
            let (lo, xfade_from, xfade_to) = mips[i];
            OpBlock {
                inc: (op_cycles(cycles, p, i) * 4_294_967_296.0) as u32,
                gain_from: self.gain[i],
                gain_to: self.target_gain(p, live, i) * self.op_duck(i),
                feedback: FEEDBACK_CYCLES[o.feedback.min(7) as usize],
                lo: self.waves[i].table(lo),
                hi: self.waves[i].table(lo + 1),
                xfade_from,
                xfade_to,
            }
        });
        let norm = carrier_norm(&self.plan, m) * self.alg_duck();
        let blk = KernelBlock {
            plan: &self.plan,
            ops,
            morph_from: self.morph,
            morph_to: m.get(),
            norm_from: self.norm,
            norm_to: norm,
        };
        self.kernel.render(&blk, &mut self.env, out);
        for (i, o) in ops.iter().enumerate() {
            self.gain[i] = o.gain_to;
            self.mip[i] = mips[i].0 as f32 + mips[i].2;
        }
        (self.morph, self.norm) = (m.get(), norm);
        self.finish_swap(p);
        self.active = (0..OPS).any(|i| {
            self.plan.carrier_a[i] + self.plan.carrier_b[i] > 0.0
                && p.ops[i].level > 0
                && !self.env[i].is_idle()
        });
    }

    /// Cycles per sample of a ratio-1 operator.
    fn cycles(&self, p: &AlgoParams, sr: f32) -> f32 {
        440.0 * exp2((self.note.get() as f32 + p.transpose as f32 - 69.0) / 12.0) / sr
    }

    fn mip_target(
        &self,
        p: &AlgoParams,
        cycles: f32,
        sr: f32,
        m: Morph,
        level: &[f32; OPS],
        i: usize,
    ) -> f32 {
        let bandwidth = op_cycles(cycles, p, i) * sr * (1.0 + incoming(&self.plan, m, i, level));
        mip_position(bandwidth)
    }

    fn target_gain(&self, p: &AlgoParams, live: &AlgoLive, i: usize) -> f32 {
        let vel = p.ops[i].velocity.min(7) as f32 * (1.0 - self.velocity) * VELOCITY_STEPS;
        level_gain(live.level[i] - vel) * SAMPLE_SCALE
    }

    fn adopt_alg(&mut self, p: &AlgoParams) {
        let key = (AlgoId::clamped(p.alg_a), AlgoId::clamped(p.alg_b));
        if key != self.plan_key {
            self.plan = plan(key.0, key.1);
            self.plan_key = key;
        }
    }

    fn begin_swap(&mut self, p: &AlgoParams) {
        if self.swap != Swap::Idle {
            return;
        }
        let alg = (AlgoId::clamped(p.alg_a), AlgoId::clamped(p.alg_b)) != self.plan_key;
        let waves = (0..OPS).fold(0u8, |m, i| {
            if WaveId::clamped(p.ops[i].wave) != self.waves[i] {
                m | 1 << i
            } else {
                m
            }
        });
        if alg || waves != 0 {
            self.swap = Swap::Ducking { alg, waves };
        }
    }

    fn finish_swap(&mut self, p: &AlgoParams) {
        self.swap = match self.swap {
            Swap::Ducking { alg, waves } => {
                if alg {
                    self.adopt_alg(p);
                }
                for i in 0..OPS {
                    if waves & (1 << i) != 0 {
                        self.waves[i] = WaveId::clamped(p.ops[i].wave);
                    }
                }
                Swap::Rising
            }
            Swap::Rising | Swap::Idle => Swap::Idle,
        };
    }

    fn op_duck(&self, i: usize) -> f32 {
        match self.swap {
            Swap::Ducking { waves, .. } if waves & (1 << i) != 0 => 0.0,
            _ => 1.0,
        }
    }

    fn alg_duck(&self) -> f32 {
        match self.swap {
            Swap::Ducking { alg: true, .. } => 0.0,
            _ => 1.0,
        }
    }
}

fn level_gains(live: &AlgoLive) -> [f32; OPS] {
    core::array::from_fn(|i| level_gain(live.level[i]))
}

/// Cycles per sample of operator `i`, kept under Nyquist.
fn op_cycles(cycles: f32, p: &AlgoParams, i: usize) -> f32 {
    let o = &p.ops[i];
    (cycles * ratio(o.coarse, o.fine) * detune_factor(o.detune)).min(0.5)
}
