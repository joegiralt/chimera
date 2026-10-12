//! `AlgoEngine`: the imperative shell around the pure core.

use super::math::clampf;
use crate::taps::TapRows;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::{ALGORITHMS, AlgoId, plan};
use crate::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use crate::dsp::algo::formant::{FormantLaw, NYQUIST_HZ, Skirt, cycles_to_phase, formant_law};
use crate::dsp::algo::kernel::{
    FormantBlock, Kernel, KernelBlock, KindBlock, NoiseBlock, OpBlock, SAMPLE_SCALE,
};
use crate::dsp::algo::kinds::{Kinds, OpKind};
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::algo::morph::{Morph, carrier_norm, incoming};
use crate::dsp::algo::noise::{NOISE_GAIN, noise_coef, noise_edge, res_mix};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams, bw_hz};
use crate::dsp::algo::plan::{EvalPlan, MAX_EDGES, OPS, blend};
use crate::dsp::algo::tx::{detune_at, feedback_cycles, level_gain, patch_gain, ratio_at};
use crate::dsp::algo::vowel::{Formant, VowelSlot, fc_hz, resolve_slots};
use crate::dsp::algo::waves::{WaveId, mip_position, mip_step};
use crate::dsp::glide::Glide;
use crate::hw::{BLOCK_SIZE, Cost};
use crate::voice_alloc::VoiceIdx;
use crate::{MidiNote, Velocity};

/// LEVEL steps (3 dB) one VELOCITY step takes off at velocity 0.
const VELOCITY_STEPS: f32 = 4.0;

/// What the engine plays this block in place of the stored bytes, in the
/// stored ranges, unrounded: MORPH and the LEVELs after the Part's knobs
/// and modulation (ADR 0010's offset formula), and the operators' other
/// glided settings (ADR 0112). From `from_params`, the stored values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlgoLive {
    pub morph: f32,
    pub level: [f32; OPS],
    /// The LEVELs as the patch plays them: glided, before routes. The
    /// output scale's and what keeps a released voice (ADR 0049).
    pub patch: [f32; OPS],
    pub fine: [f32; OPS],
    pub detune: [f32; OPS],
    pub feedback: [f32; OPS],
    pub velocity: [f32; OPS],
    /// The formant operators' glided settings.
    pub frmt: [f32; OPS],
    pub bw: [f32; OPS],
    pub vowel: f32,
    /// LEVELs with a mod route (`ModState::algo_levels_routed`): live even
    /// at a stored 0, as `cost` bills them.
    pub routed: [bool; OPS],
    /// MORPH has a mod route: a carrier of either algorithm may be heard.
    pub morph_routed: bool,
}

impl AlgoLive {
    pub fn from_params(p: &AlgoParams) -> Self {
        let op = |f: fn(&AlgoOpParams) -> f32| core::array::from_fn(|i| f(&p.ops[i]));
        let level = op(|o| f32::from(o.level));
        Self {
            morph: f32::from(p.morph),
            level,
            patch: level,
            fine: op(|o| f32::from(o.fine)),
            detune: op(|o| f32::from(o.detune)),
            feedback: op(|o| f32::from(o.feedback)),
            velocity: op(|o| f32::from(o.velocity)),
            frmt: op(|o| f32::from(o.frmt)),
            bw: op(|o| f32::from(o.bw)),
            vowel: f32::from(p.vowel),
            routed: [false; OPS],
            morph_routed: false,
        }
    }

    /// The slot that plays `addr`, if it is one of these.
    fn slot(&mut self, addr: ParamAddr) -> Option<&mut f32> {
        Some(match (addr.block, addr.param) {
            (BlockRef::Algo, AlgoParams::MORPH) => &mut self.morph,
            (BlockRef::Algo, AlgoParams::VOWEL) => &mut self.vowel,
            (BlockRef::AlgoOp(op), id) => {
                let i = op.index();
                match id {
                    AlgoOpParams::LEVEL => &mut self.level[i],
                    AlgoOpParams::FINE => &mut self.fine[i],
                    AlgoOpParams::DETUNE => &mut self.detune[i],
                    AlgoOpParams::FEEDBACK => &mut self.feedback[i],
                    AlgoOpParams::VELOCITY => &mut self.velocity[i],
                    AlgoOpParams::FRMT => &mut self.frmt[i],
                    AlgoOpParams::BW => &mut self.bw[i],
                    _ => return None,
                }
            }
            _ => return None,
        })
    }

    /// Plays `v` for `addr`, a knob on its way (`false` for any other
    /// address, which the voice writes to its blocks).
    pub fn set(&mut self, addr: ParamAddr, v: f32) -> bool {
        if let (BlockRef::AlgoOp(op), AlgoOpParams::LEVEL) = (addr.block, addr.param) {
            self.patch[op.index()] = v;
        }
        self.slot(addr).map(|s| *s = v).is_some()
    }

    /// What it plays for `addr`, if it is one of these.
    pub fn get(&self, addr: ParamAddr) -> Option<f32> {
        let mut c = *self;
        c.slot(addr).map(|s| *s)
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
            *slot = clampf(*slot + off * (s.max - s.min), s.min, s.max);
        }
        true
    }
}

/// What an operator holds from its note-on (spec § 7): SKIRT only when it
/// was idle, so a retrigger never changes an open grain's exponent
/// (Ruling 7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpLatch {
    pub skirt: Skirt,
    pub slot: Option<Formant>,
    pub key: f32,
    /// RES's `(α, β)`.
    pub mix: (f32, f32),
}

impl OpLatch {
    const INIT: OpLatch = OpLatch {
        skirt: Skirt::S1,
        slot: None,
        key: 0.0,
        mix: (1.0, 0.0),
    };
}

/// `noise_coef` at an unbounded WIDTH: white noise at unit RMS.
const WHITE: (f32, f32) = (1.0, 1.732_050_8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swap {
    Idle,
    /// `kinds`: mask of operators whose WAVE changed.
    Ducking {
        alg: bool,
        kinds: u8,
    },
    Rising,
}

pub struct AlgoEngine {
    kernel: Kernel,
    env: [OpEnv; OPS],
    /// Rebuilt only when ALG A or B changes.
    plan: EvalPlan,
    plan_key: (AlgoId, AlgoId),
    kinds: [OpKind; OPS],
    rates: [EnvRates; OPS],
    /// Gain, mip position, MORPH and output scale at the end of the last
    /// block: the next block's ramps start here.
    gain: [f32; OPS],
    mip: [f32; OPS],
    morph: f32,
    norm: f32,
    swap: Swap,
    note: MidiNote,
    /// The voice's pitch offset in semitones (`set_pitch`, ADR 0042).
    pitch: f32,
    /// A glide steal's portamento: the sounding pitch over the note's,
    /// gliding to 1 (#254).
    slide: Glide,
    velocity: f32,
    active: bool,
    /// Key down since the last note-on: every carrier holds the voice.
    held: bool,
    latch: [OpLatch; OPS],
    /// The noise ramps' starts: the last block's poles' coefficient and
    /// input gain, and RES's `(α, β)`.
    noise_c: [f32; OPS],
    noise_k: [f32; OPS],
    mix: [(f32, f32); OPS],
    /// The carrier-weight ramps' starts: each formant operator's last
    /// `g_c / g_m`.
    c_ratio: [f32; OPS],
    /// The last block's `(fc, bw_eff)` of each formant or noise operator.
    view: [(f32, f32); OPS],
}

crate::in_place::field_list!(AlgoEngine => AlgoEngine {
    kernel, env, plan, plan_key, kinds, rates, gain, mip, morph, norm, swap, note, pitch, slide,
    velocity, active, held, latch, noise_c, noise_k, mix, c_ratio, view,
});

#[cfg(any(test, feature = "test-support"))]
impl Default for AlgoEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// The table an operator reads: W1 under a formant kind, which reads its
/// own lane.
const fn table_of(k: OpKind) -> WaveId {
    match k {
        OpKind::Table(w) => w,
        OpKind::Formant | OpKind::Noise => WaveId::W1,
    }
}

impl AlgoEngine {
    // Cycles/sample terms of `cost`, solved from the bench rows and rounded
    // up. The mip crossfade always runs, so it is in `COST_OP`.
    pub const COST_BASE: Cost = Cost(370); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_OP: Cost = Cost(60); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_LINK: Cost = Cost(8); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_FEEDBACK: Cost = Cost(1); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_FRMT_ONE: Cost = Cost(90); // estimate, ADR 0056's method; bench at ship
    pub const COST_FRMT: Cost = Cost(130); // estimate, ADR 0056's method; bench at ship
    pub const COST_NOIS: Cost = Cost(70); // estimate, ADR 0056's method; bench at ship

    /// A voice's cycles/sample from its patch's shape (spec addendum). An
    /// operator is priced when its LEVEL is above 0 or has a mod route
    /// (`level_routed`): the kernel skips only operators silent all block.
    /// Links are those into priced operators in the union of ALG A and B,
    /// at any MORPH, as the plan runs them all. One block can cost more than
    /// this: a LEVEL ramping down to 0 still runs, and an ALG edit renders
    /// the old plan through its duck block. The 30% reserve absorbs it.
    pub const fn cost(p: &AlgoParams, level_routed: &[bool; OPS]) -> Cost {
        let a = &ALGORITHMS[AlgoId::clamped(p.alg_a).get() as usize];
        let b = &ALGORITHMS[AlgoId::clamped(p.alg_b).get() as usize];
        let (mut active, mut feedback, mut ops, mut i) = (0u8, 0, 0, 0);
        while i < OPS {
            if p.ops[i].level > 0 || level_routed[i] {
                active |= 1 << i;
                let kind = p.kinds().get(i);
                ops += match kind {
                    OpKind::Table(_) => Self::COST_OP.0,
                    // SKIRT 0 keeps one lane open at a time.
                    OpKind::Formant if p.ops[i].skirt == 0 => Self::COST_FRMT_ONE.0,
                    OpKind::Formant => Self::COST_FRMT.0,
                    OpKind::Noise => Self::COST_NOIS.0,
                };
                if p.ops[i].feedback > 0 && !matches!(kind, OpKind::Noise) {
                    feedback += 1;
                }
            }
            i += 1;
        }
        let (mut links, mut i) = (0, 0);
        while i < OPS {
            links += ((a.mods[i] | b.mods[i]) & active).count_ones();
            i += 1;
        }
        if links > MAX_EDGES as u32 {
            links = MAX_EDGES as u32;
        }
        Cost(Self::COST_BASE.0 + ops + links * Self::COST_LINK.0 + feedback * Self::COST_FEEDBACK.0)
    }

    /// The ops the kernel runs next block: gain off zero where the last
    /// block ended (a LEVEL gliding to 0 runs until it lands, ADR 0112).
    pub fn running(&self) -> [bool; OPS] {
        core::array::from_fn(|i| self.gain[i] != 0.0)
    }

    /// By value, through the stack: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new() -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { crate::in_place::by_value(Self::init_in_place) }
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
            addr_of_mut!((*p).kinds).write(Kinds::TABLE.all());
            addr_of_mut!((*p).rates).write([rates; OPS]);
            addr_of_mut!((*p).gain).write([0.0; OPS]);
            addr_of_mut!((*p).mip).write([0.0; OPS]);
            addr_of_mut!((*p).morph).write(0.0);
            addr_of_mut!((*p).norm).write(1.0);
            addr_of_mut!((*p).swap).write(Swap::Idle);
            addr_of_mut!((*p).note).write(MidiNote::A4);
            addr_of_mut!((*p).pitch).write(0.0);
            addr_of_mut!((*p).slide).write(Glide::new(1.0, 1.0));
            addr_of_mut!((*p).velocity).write(1.0);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).held).write(false);
            addr_of_mut!((*p).latch).write([OpLatch::INIT; OPS]);
            addr_of_mut!((*p).noise_c).write([WHITE.0; OPS]);
            addr_of_mut!((*p).noise_k).write([WHITE.1; OPS]);
            addr_of_mut!((*p).mix).write([OpLatch::INIT.mix; OPS]);
            addr_of_mut!((*p).c_ratio).write([1.0; OPS]);
            addr_of_mut!((*p).view).write([(0.0, 0.0); OPS]);
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
        // A new note is at its pitch; the one sounding, struck again, glides on.
        if !(self.active && note == self.note) {
            self.slide.toward(1.0);
            self.slide.snap();
        }
        self.note = note;
        self.velocity = velocity.unit();
        let m = Morph::from_param(live.morph);
        let level = level_gains(&live);
        if !self.active {
            self.adopt_alg(p);
            self.swap = Swap::Idle;
            self.env = [OpEnv::IDLE; OPS];
            self.morph = m.get();
            self.norm = carrier_norm(&self.plan, m, &patch_gains(&live));
        }
        let cycles = self.cycles(p, sr);
        let vslot = core::array::from_fn(|i| {
            VowelSlot::from_code(p.ops[i].vslot).unwrap_or(VowelSlot::Off)
        });
        let slots = resolve_slots(&p.kinds(), vslot);
        for i in 0..OPS {
            self.rates[i] = p.ops[i].rates();
            let o = &p.ops[i];
            let l = &mut self.latch[i];
            (l.slot, l.key, l.mix) = (slots[i], f32::from(o.key), res_mix(o.res));
            if self.env[i].is_idle() {
                l.skirt = Skirt::from_code(o.skirt);
                self.kernel.reset(i, p.kinds().get(i));
                self.kinds[i] = p.kinds().get(i);
                (self.noise_c[i], self.noise_k[i]) = noise_coef(self.noise_band(p, i).1, sr);
                if self.kinds[i] == OpKind::Formant {
                    let law = self.formant(p, &live, cycles, sr, i);
                    self.c_ratio[i] = law.g_c / law.g_m;
                }
                self.gain[i] = self.target_gain(p, &live, cycles, sr, i);
                self.mip[i] = self.mip_target(p, &live, cycles, sr, m, &level, i);
            }
            self.env[i].note_on(EnvCoefs::new(self.rates[i], note, sr));
        }
        self.active = true;
        self.held = true;
    }

    /// `note_on` on a sounding voice (#254): its operators glide from the
    /// pitch they sound to the new note's, a one-pole of `tau` seconds in
    /// log pitch, the classic portamento; the envelopes strike as a CUT.
    /// On an idle voice, or the note it sounds, a plain `note_on`.
    pub fn glide_on(
        &mut self,
        note: MidiNote,
        velocity: Velocity,
        p: &AlgoParams,
        sample_rate: u32,
        tau: f32,
    ) {
        let sr = sample_rate as f32;
        let from = (self.active && note != self.note).then(|| self.cycles(p, sr));
        self.note_on(note, velocity, p, sample_rate);
        if let Some(from) = from {
            self.slide = Glide::new(from / self.cycles(p, sr), tau);
            self.slide.toward(1.0);
        }
    }

    /// A sounding note moved to `note` with no strike (a re-snap, spec
    /// § 5.2): its operators glide there from the pitch they sound, as a
    /// glide steal's do, and the envelopes run on. Idle, or on `note`,
    /// nothing moves.
    pub fn glide_to(&mut self, note: MidiNote, tau: f32) {
        if !self.active || note == self.note {
            return;
        }
        let from = exp2((self.note.get() as f32 - note.get() as f32) / 12.0) * self.slide.value();
        self.note = note;
        self.slide = Glide::new(from, tau);
        self.slide.toward(1.0);
    }

    /// The glide steal's ratio now, 1 at rest: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        self.slide.value()
    }

    /// The voice's pitch offset in semitones, for the next `note_on` or
    /// `render`: every operator's frequency, stepped per block as the note is.
    pub fn set_pitch(&mut self, semitones: f32) {
        self.pitch = semitones;
    }

    pub fn note_off(&mut self) {
        self.held = false;
        for e in &mut self.env {
            e.note_off();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Restarts so far that found their lane's window open: 0 by
    /// construction (spec § 3).
    #[cfg(any(test, feature = "test-support"))]
    pub fn grain_cuts(&self) -> u32 {
        self.kernel.grain_cuts()
    }

    /// Every operator's noise from `voice`'s seeds: once, as the slot is
    /// built, never at a note-on.
    pub fn seed(&mut self, voice: VoiceIdx) {
        self.kernel.seed(voice.index() as u8);
    }

    /// The formant and noise operators' last `(fc, bw_eff)`, Hz.
    pub fn view(&self) -> [(f32, f32); OPS] {
        self.view
    }

    /// Ruling 7: a sounding FRMT operator latched at SKIRT ≥ 1 runs two
    /// lanes where a stored SKIRT 0 bills one; the difference, per
    /// operator.
    pub fn held_skirt_extra(&self, p: &AlgoParams) -> Cost {
        if !self.active {
            return Cost::ZERO;
        }
        let n = (0..OPS)
            .filter(|&i| {
                self.kinds[i] == OpKind::Formant
                    && self.latch[i].skirt != Skirt::S0
                    && p.ops[i].skirt == 0
            })
            .count() as u32;
        Cost(n * (Self::COST_FRMT.0 - Self::COST_FRMT_ONE.0))
    }

    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &AlgoParams,
        live: &AlgoLive,
        sample_rate: u32,
        taps: &mut TapRows,
    ) {
        if !self.active {
            out.fill(0.0);
            return;
        }
        let sr = sample_rate as f32;
        self.slide.tick();
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
            // A formant reads no table: its mip holds.
            let to = if self.kinds[i] == OpKind::Formant {
                self.mip[i]
            } else {
                self.mip_target(p, live, cycles, sr, m, &level, i)
            };
            mip_step(self.mip[i], to)
        });
        let mut view = self.view;
        let ops: [OpBlock; OPS] = core::array::from_fn(|i| {
            if self.kinds[i] == OpKind::Formant {
                let law = self.formant(p, live, cycles, sr, i);
                view[i] = (law.fc, law.bw_eff);
                let c = op_cycles(cycles, p, live, i);
                // c ≤ ½: under 2^31.
                let inc = (c * 4_294_967_296.0) as u32;
                let shift = law.fc / (c * sr);
                return OpBlock {
                    inc,
                    gain_from: self.gain[i],
                    gain_to: self.level_gain(live, i) * law.g_m * law.edge * self.op_duck(i),
                    // Task 11 takes FDBK to the carrier.
                    feedback: 0.0,
                    kind: KindBlock::Formant(FormantBlock {
                        inv_inc: 1.0 / inc.max(1) as f32,
                        inc_c: cycles_to_phase(law.fc / sr),
                        inv_d: 1.0 / law.d,
                        slack: law.slack,
                        shift,
                        c0: cycles_to_phase(-law.d * shift * 0.5),
                        c_ratio: (self.c_ratio[i], law.g_c / law.g_m),
                        skirt: self.latch[i].skirt,
                    }),
                };
            }
            let gain_to = self.target_gain(p, live, cycles, sr, i) * self.op_duck(i);
            if self.kinds[i] == OpKind::Noise {
                let (fc, w) = self.noise_band(p, i);
                let (c, k) = noise_coef(w, sr);
                let mix = (self.mix[i], self.latch[i].mix);
                return OpBlock {
                    // FC/sr ≤ ½ at any rate ≥ 48 kHz; wrapping besides.
                    inc: cycles_to_phase(fc.min(NYQUIST_HZ) / sr),
                    gain_from: self.gain[i],
                    gain_to,
                    feedback: 0.0,
                    kind: KindBlock::Noise(NoiseBlock {
                        c: (self.noise_c[i], c),
                        k: (self.noise_k[i], k),
                        alpha: (mix.0.0, mix.1.0),
                        beta: (mix.0.1, mix.1.1),
                    }),
                };
            }
            let (lo, xfade_from, xfade_to) = mips[i];
            OpBlock {
                inc: (op_cycles(cycles, p, live, i) * 4_294_967_296.0) as u32,
                gain_from: self.gain[i],
                gain_to,
                feedback: feedback_cycles(live.feedback[i]),
                kind: KindBlock::Table {
                    lo: table_of(self.kinds[i]).table(lo),
                    hi: table_of(self.kinds[i]).table(lo + 1),
                    xfade_from,
                    xfade_to,
                },
            }
        });
        let norm = carrier_norm(&self.plan, m, &patch_gains(live)) * self.alg_duck();
        let blk = KernelBlock {
            plan: &self.plan,
            ops,
            morph_from: self.morph,
            morph_to: m.get(),
            norm_from: self.norm,
            norm_to: norm,
        };
        self.kernel.render(&blk, &mut self.env, out, taps);
        self.view = view;
        for (i, o) in ops.iter().enumerate() {
            self.gain[i] = o.gain_to;
            self.mip[i] = mips[i].0 as f32 + mips[i].2;
            self.mix[i] = self.latch[i].mix;
            match o.kind {
                KindBlock::Noise(n) => {
                    (self.noise_c[i], self.noise_k[i]) = (n.c.1, n.k.1);
                    self.view[i] = self.noise_band(p, i);
                }
                KindBlock::Formant(f) => self.c_ratio[i] = f.c_ratio.1,
                KindBlock::Table { .. } => {}
            }
        }
        (self.morph, self.norm) = (m.get(), norm);
        self.finish_swap(p);
        // A held key: a carrier of either algorithm holds the voice, so no
        // MORPH, by hand or by route, ends a held note (58b35e9). Released:
        // only a carrier heard at the MORPH does, unless a route may bring
        // the other algorithm's back (ADR 0061).
        let (a, b) = (&self.plan.carrier_a, &self.plan.carrier_b);
        let heard = |i: usize| {
            if self.held || live.morph_routed {
                a[i] + b[i] > 0.0
            } else {
                blend(a[i], b[i], m.get()) > 0.0
            }
        };
        self.active = (0..OPS)
            .any(|i| heard(i) && (live.patch[i] > 0.0 || live.routed[i]) && !self.env[i].is_idle());
    }

    /// Cycles per sample of a ratio-1 operator.
    fn cycles(&self, p: &AlgoParams, sr: f32) -> f32 {
        let mut st = self.note.get() as f32 + p.transpose as f32 - 69.0;
        if self.pitch != 0.0 {
            st += self.pitch;
        }
        440.0 * exp2(st / 12.0) / sr * self.slide.value()
    }

    #[allow(clippy::too_many_arguments)]
    fn mip_target(
        &self,
        p: &AlgoParams,
        live: &AlgoLive,
        cycles: f32,
        sr: f32,
        m: Morph,
        level: &[f32; OPS],
        i: usize,
    ) -> f32 {
        let bandwidth =
            op_cycles(cycles, p, live, i) * sr * (1.0 + incoming(&self.plan, m, i, level));
        mip_position(bandwidth)
    }

    /// LEVEL's gain after VELOCITY.
    fn level_gain(&self, live: &AlgoLive, i: usize) -> f32 {
        let vel = clampf(live.velocity[i], 0.0, 7.0) * (1.0 - self.velocity) * VELOCITY_STEPS;
        level_gain(live.level[i] - vel)
    }

    /// A table's in sample units; a noise band's at the −12 dB reference,
    /// faded at the top of the band; a formant's peak-referenced (`g_m`),
    /// faded at the edge, its carrier's `g_c / g_m` in the carrier weight.
    fn target_gain(&self, p: &AlgoParams, live: &AlgoLive, cycles: f32, sr: f32, i: usize) -> f32 {
        let g = self.level_gain(live, i);
        match self.kinds[i] {
            OpKind::Table(_) => g * SAMPLE_SCALE,
            OpKind::Noise => {
                let (fc, w) = self.noise_band(p, i);
                g * NOISE_GAIN * noise_edge(fc, w)
            }
            OpKind::Formant => {
                let law = self.formant(p, live, cycles, sr, i);
                g * law.g_m * law.edge
            }
        }
    }

    /// A formant operator's law this block (spec § 3): its pitch is the
    /// grain clock's, FC from FRMT, VOWEL and KEY on its latched slot, KEY
    /// and SKIRT. A kind swapped in under a held note keeps the slot and
    /// SKIRT latched at the note-on until the next one.
    fn formant(
        &self,
        p: &AlgoParams,
        live: &AlgoLive,
        cycles: f32,
        sr: f32,
        i: usize,
    ) -> FormantLaw {
        let l = &self.latch[i];
        let fc = fc_hz(live.frmt[i], live.vowel, l.slot, l.key, self.key_pitch(p));
        let f0 = op_cycles(cycles, p, live, i) * sr;
        formant_law(f0, fc, bw_hz(live.bw[i]), l.skirt)
    }

    /// The pitch KEY follows, semitones: the note, TRNSP, the voice's
    /// offset and a glide's slide (spec § 3).
    fn key_pitch(&self, p: &AlgoParams) -> f32 {
        let st = self.note.get() as f32 + p.transpose as f32 + self.pitch;
        let slide = self.slide.value();
        if slide == 1.0 {
            st
        } else {
            st + 12.0 * log2(slide)
        }
    }

    /// A noise operator's centre and WIDTH, Hz: CENTRE on its latched
    /// vowel slot, with no KEY (spec § 4); stored FRMT, BW and VOWEL.
    fn noise_band(&self, p: &AlgoParams, i: usize) -> (f32, f32) {
        let o = &p.ops[i];
        let fc = fc_hz(
            f32::from(o.frmt),
            f32::from(p.vowel),
            self.latch[i].slot,
            0.0,
            0.0,
        );
        (fc, bw_hz(f32::from(o.bw)))
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
        let kinds = (0..OPS).fold(0u8, |m, i| {
            if p.kinds().get(i) != self.kinds[i] {
                m | 1 << i
            } else {
                m
            }
        });
        if alg || kinds != 0 {
            self.swap = Swap::Ducking { alg, kinds };
        }
    }

    fn finish_swap(&mut self, p: &AlgoParams) {
        self.swap = match self.swap {
            Swap::Ducking { alg, kinds } => {
                if alg {
                    self.adopt_alg(p);
                }
                for i in 0..OPS {
                    let (old, new) = (self.kinds[i], p.kinds().get(i));
                    if kinds & (1 << i) != 0 {
                        // A table to a table runs on; any other kind starts
                        // closed, under the duck.
                        if old.is_formant() || new.is_formant() {
                            self.kernel.reset(i, new);
                        }
                        self.kinds[i] = new;
                    }
                }
                Swap::Rising
            }
            Swap::Rising | Swap::Idle => Swap::Idle,
        };
    }

    fn op_duck(&self, i: usize) -> f32 {
        match self.swap {
            Swap::Ducking { kinds, .. } if kinds & (1 << i) != 0 => 0.0,
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

/// The patch's LEVELs' gains, for the output scale: a property of the
/// patch (glided, ADR 0112), so no route, VELOCITY or note on one carrier
/// moves another (ADR 0049).
fn patch_gains(live: &AlgoLive) -> [f32; OPS] {
    core::array::from_fn(|i| patch_gain(live.patch[i]))
}

fn level_gains(live: &AlgoLive) -> [f32; OPS] {
    core::array::from_fn(|i| level_gain(live.level[i]))
}

/// Cycles per sample of operator `i`, kept under Nyquist: CRSE stored,
/// FINE and DETUNE as `live` plays them.
fn op_cycles(cycles: f32, p: &AlgoParams, live: &AlgoLive, i: usize) -> f32 {
    (cycles * ratio_at(p.ops[i].coarse, live.fine[i]) * detune_at(live.detune[i])).min(0.5)
}
