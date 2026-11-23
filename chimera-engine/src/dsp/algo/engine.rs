// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `AlgoEngine`: the imperative shell around the pure core.

use super::math::clampf;
use crate::taps::TapRows;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::{ALGORITHMS, AlgoId, plan};
use crate::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use crate::dsp::algo::formant::{
    Drive, FormantLaw, GrainPm, NYQUIST_HZ, PeakRamp, Skirt, cycles_to_phase, delay_scale,
    formant_law, formant_law_pm, peak_bound, peak_ramp,
};
use crate::dsp::algo::kernel::{
    FormantBlock, Kernel, KernelBlock, KindBlock, NoiseBlock, OpBlock, PM_CYCLES, SAMPLE_SCALE,
};
use crate::dsp::algo::kinds::{Kinds, OpKind};
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::algo::morph::{Morph, carrier_norm, incoming};
use crate::dsp::algo::noise::{NOISE_GAIN, noise_coef, noise_edge, res_mix};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams, bw_hz};
use crate::dsp::algo::plan::{EvalPlan, MAX_EDGES, OPS, blend};
use crate::dsp::algo::tx::{
    DETUNE_CENTS, detune_at, feedback_cycles, level_gain, patch_gain, ratio_at,
};
use crate::dsp::algo::vowel::{Formant, VowelSlot, fc_hz, resolve_slots};
use crate::dsp::algo::waves::{WaveId, mip_position, mip_step};
use crate::dsp::glide::Glide;
use crate::hw::{BLOCK_SIZE, Cost, SAMPLE_RATE};
use crate::voice_alloc::VoiceIdx;
use crate::{MidiNote, Velocity};

/// What an OP page's band draws of a voice (plan Ruling 8): each formant
/// and noise operator's FC and BW_eff, Hz, as its last block took them.
/// All zeros, so a buffer of them is `.bss`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormantView {
    pub fc: [f32; OPS],
    pub bw_eff: [f32; OPS],
    /// A voice of the Part sounds; else the view is the stored params'.
    pub sounding: bool,
}

/// Each Part's view: the audio publishes them all, the UI reads the
/// edited Part's.
pub type FormantViews = [FormantView; crate::hw::MAX_PARTS];

impl FormantView {
    pub const NONE: FormantView = FormantView {
        fc: [0.0; OPS],
        bw_eff: [0.0; OPS],
        sounding: false,
    };

    /// A voice's view, sounding.
    pub fn live(v: [(f32, f32); OPS]) -> FormantView {
        FormantView {
            fc: v.map(|o| o.0),
            bw_eff: v.map(|o| o.1),
            sounding: true,
        }
    }

    /// `p` with no voice (spec § 6): each operator's FC and BW_eff at C4,
    /// VOWEL and its slot as stored; at C4 KEY moves nothing.
    pub fn resting(p: &AlgoParams) -> FormantView {
        const C4_HZ: f32 = 261.625_58;
        let vslot = core::array::from_fn(|i| {
            VowelSlot::from_code(p.ops[i].vslot).unwrap_or(VowelSlot::Off)
        });
        let slots = resolve_slots(&p.kinds(), vslot);
        let vowel = f32::from(p.vowel);
        let mut v = FormantView::NONE;
        for (i, o) in p.ops.iter().enumerate() {
            let fc = fc_hz(f32::from(o.frmt), vowel, slots[i], 0.0, 0.0);
            let bw = bw_hz(f32::from(o.bw));
            (v.fc[i], v.bw_eff[i]) = match p.kinds().get(i) {
                OpKind::Formant => {
                    let f0 = C4_HZ
                        * ratio_at(o.coarse, f32::from(o.fine))
                        * detune_at(f32::from(o.detune));
                    let law = formant_law(f0, fc, bw, Skirt::from_code(o.skirt));
                    (law.fc, law.bw_eff)
                }
                OpKind::Noise => (fc, bw),
                OpKind::Table(_) => (0.0, 0.0),
            };
        }
        v
    }
}

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
            feedback: op(|o| o.feedback.steps()),
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

    /// Takes a mod offset aimed at one of these: MORPH, VOWEL, a LEVEL,
    /// FRMT or BW, the modulatable ones (`false` for any other destination,
    /// which the voice applies to its blocks as before).
    pub fn offset(&mut self, addr: ParamAddr, off: f32) -> bool {
        let Some(slot) = self.slot(addr) else {
            return false;
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

/// A formant operator's drive at the end of the last block, where the
/// next block's peak bound starts: FDBK's cycles, the links' depth, the
/// lanes, the bound (`peak_bound`) and the modulator gain under it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Peak {
    feedback: f32,
    depth: f32,
    lanes: f32,
    bound: f32,
    gm: f32,
}

impl Peak {
    const INIT: Peak = Peak {
        feedback: 0.0,
        depth: 0.0,
        lanes: 1.0,
        bound: 1.0,
        gm: 1.0,
    };

    /// Settled at `law` under `drive`: its `g_m`.
    fn at(law: &FormantLaw, drive: &Drive) -> Peak {
        let bound = peak_bound(law.p_hat, law.lanes, drive.feedback + law.pm * drive.depth);
        Peak {
            feedback: drive.feedback,
            depth: drive.depth,
            lanes: law.lanes,
            bound,
            gm: 1.0 / bound.min(1.0),
        }
    }

    /// The block's modulator-gain ramp from here to `law` under `drive`,
    /// and where it ends. Across the block the feedback ramps, the PM share
    /// ramps from `k` and the sources' LEVELs from the last depth, so the
    /// carrier's displacement stays under the line to the bound at the
    /// larger share, depth and lane count: settled, exactly `g_m`.
    fn next(&self, k: f32, law: &FormantLaw, drive: &Drive) -> (PeakRamp, Peak) {
        let phi = drive.feedback + k.max(law.pm) * self.depth.max(drive.depth);
        let bound = peak_bound(law.p_hat, self.lanes.max(law.lanes), phi);
        let end = Peak {
            feedback: drive.feedback,
            depth: drive.depth,
            lanes: law.lanes,
            bound,
            gm: 1.0 / bound.min(1.0),
        };
        (peak_ramp(self.gm, self.bound, bound), end)
    }
}

/// A formant operator's law, its drive and what its block takes from them
/// beyond the ramps: kept while the law's inputs hold (`LawKey`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct LawBlock {
    law: FormantLaw,
    drive: Drive,
    inc: u32,
    inv_inc: f32,
    inc_c: u32,
    inv_d: f32,
    pm: GrainPm,
    m: u32,
    c0: [u32; 2],
    /// `g_c / g_m`.
    ratio: f32,
}

impl LawBlock {
    /// At `c` cycles a sample (the operator's pitch) and rate `sr`.
    #[cfg_attr(feature = "probe", inline(never))]
    fn new(law: FormantLaw, drive: Drive, c: f32, sr: f32) -> LawBlock {
        // c ≤ ½: under 2^31.
        let inc = (c * 4_294_967_296.0) as u32;
        let shift = law.fc / (c * sr);
        let pm = GrainPm::new(law.slack, false);
        LawBlock {
            law,
            drive,
            inc,
            inv_inc: 1.0 / inc.max(1) as f32,
            inc_c: cycles_to_phase(law.fc / sr),
            inv_d: 1.0 / law.d,
            pm,
            m: delay_scale(pm.h, shift),
            c0: [
                cycles_to_phase(-shift),
                cycles_to_phase(-law.d * shift * 0.5),
            ],
            ratio: law.g_c / law.g_m,
        }
    }
}

/// Every input of the formant laws a block can move, as bits: each
/// operator's FRMT, BW, LEVEL, FINE, DETUNE and FDBK as played, VOWEL,
/// MORPH, the pitch offset, the glide's slide and the rate, the note,
/// TRNSP and the COARSEs. The latched slots, KEY and SKIRT, the kinds the
/// engine runs and its plan (the links) move only at a note-on, a swap or
/// a plan change, which each drop the key where they write.
const LAW_KEY: usize = 6 * OPS + 8;
type LawKey = [u32; LAW_KEY];

/// A NOIS operator's band law: its centre and WIDTH, the poles' `(c, k)`
/// and the upper-edge fade. `key` holds FRMT, BW and VOWEL as played and
/// the slot it was taken at.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NoiseLaw {
    key: [u32; 4],
    band: (f32, f32),
    coef: (f32, f32),
    edge: f32,
}

impl NoiseLaw {
    const STALE: NoiseLaw = NoiseLaw {
        key: [u32::MAX; 4],
        band: (0.0, 0.0),
        coef: WHITE,
        edge: 0.0,
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
    /// The last block's ratio-1 cycles a sample, which a ramped block's
    /// increments start from (#164); 0 before a note's first block.
    ramp_from: Option<f32>,
    /// Its Part's fast pitch routes can pass a semitone
    /// (`engines::pitch_ramps`, taken once a publish by the Instrument):
    /// this voice's table operators ramp (#164).
    ramps: bool,
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
    /// The PM-depth ramps' starts: each formant operator's last share of
    /// its links' PM.
    pm_k: [f32; OPS],
    /// Each formant operator's drive where the last block left it.
    peak: [Peak; OPS],
    /// The last block's `(fc, bw_eff)` of each formant or noise operator.
    view: [(f32, f32); OPS],
    /// The formant laws' inputs when they were taken (`None`: to take),
    /// and each formant operator's law block from them.
    law_key: Option<LawKey>,
    laws: [Option<LawBlock>; OPS],
    /// Each noise operator's band law, kept while its stored values hold.
    noise: [NoiseLaw; OPS],
}

crate::in_place::field_list!(AlgoEngine => AlgoEngine {
    kernel, env, plan, plan_key, kinds, rates, gain, mip, morph, norm, swap, note, pitch, ramp_from, ramps,
    slide, velocity, active, held, latch, noise_c, noise_k, mix, c_ratio, pm_k, peak, view, law_key,
    laws, noise,
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

/// `COST_RAMP`: a ramped table operator's start increment, taken each
/// block (`op_cycles` again, its cast, and the difference's divide by 64,
/// which compiles to shifts: ~100 cycles), over 64 samples. The loop's own
/// step, an integer add a sample that adds 0 when steady, runs on every
/// operator in `COST_OP`.
const RAMP_CYCLES: u32 = 2;

/// DETUNE's top ratio, +3 steps.
const DETUNE_TOP: f32 = exp2(3.0 * DETUNE_CENTS / 1200.0);

impl AlgoEngine {
    // Cycles/sample terms of `cost`, solved from the bench rows and rounded
    // up. The mip crossfade always runs, so it is in `COST_OP`.
    // BASE: the unit reads every table row 55–65 over its bill (2026-10-22,
    // bench ALGO: 1 OP 657 against 596, A16+17 1,050 against 998). Kept at
    // 0.2: 0.3.0's efficiency pass makes ALGO cheaper instead
    // (private issue #793).
    pub const COST_BASE: Cost = Cost(370); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_OP: Cost = Cost(60); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_LINK: Cost = Cost(8); // measured 2026-09-27, bench, rev V at 480 MHz
    pub const COST_FEEDBACK: Cost = Cost(1); // measured 2026-09-27, bench, rev V at 480 MHz
    /// A priced table operator under a fast pitch route (#164): its
    /// block's start increment (`render_ramped`).
    pub const COST_RAMP: Cost = Cost(RAMP_CYCLES);
    // Counted on the bench-algo ELF with tools/loop_count.py (ADR 0131): a
    // SKIRT 1–3 FRMT's two lanes in spans (`run_formant::<3>`), SKIRT 0's
    // (`::<0>`), and with links the moving loop (`run_moving` and
    // `tick_at`'s longest path), each at its worst block for a grain clock
    // up to `SPAN_TOP_HZ`: `SPANS_COVERED` spans (fires, linked). A higher
    // clock pays `high_note_extra` per voice.
    pub const COST_FRMT_ONE: Cost = Cost(220); // the 883b worst blocks (FRMT PM), charge × 1.03, rounded up (ADR 0262); was 210, counted (Task 15)
    pub const COST_FRMT: Cost = Cost(230); // the 883b worst blocks (FRMT PM), charge × 1.03, rounded up (ADR 0262); was 220, counted (Task 15)
    pub const COST_FRMT_ONE_LINKED: Cost = Cost(400); // the 883b worst blocks (FRMT PM), charge × 1.03, rounded up (ADR 0262); was 380, counted (Task 15)
    pub const COST_FRMT_LINKED: Cost = Cost(410); // the 883b worst blocks (FRMT PM), charge × 1.03, rounded up (ADR 0262); was 390, counted (Task 15)
    // `run_noise::<0>` (`::<1>` with a link in, its sum and `link_sum`),
    // `noise_op` and its band law taken afresh every block
    // (`take_noise_law` 213 instructions and 5 divides or roots): a route
    // or a knob on its way moves it any block.
    pub const COST_NOIS: Cost = Cost(95); // the 883b worst blocks (NOIS), charge × 1.6, rounded up (ADR 0262); was 80, counted (Task 15)
    pub const COST_NOIS_LINKED: Cost = Cost(125); // the 883b worst blocks (NOIS), charge × 1.6, rounded up (ADR 0262); was 100, counted (Task 15)
    /// A formant law taken afresh: `take_laws` 507 instructions and 6
    /// divides, its scan of 15 edges (+105) and its six operators' turns
    /// (+55), `formant_law_pm` 290 and 10, `LawBlock::new` 113 and 6,
    /// `formant_fc` 126 and 1 and a 100 B copy. Billed on every FRMT, as
    /// Modal bills its pitch (#437): the pitch wheel or a glide steal can
    /// move any voice's laws with no route to see.
    pub const COST_FRMT_MOVE: Cost = Cost(40); // counted, ADR 0056's method, Task 15
    /// With links in, `drive` sums up to five edges (four more bodies of
    /// 140 and 2 divides) and a FRMT source's `formant_fc`.
    pub const COST_FRMT_MOVE_LINKED: Cost = Cost(60); // counted, ADR 0056's method, Task 15

    /// The grain clock the FRMT terms cover: C8, MIDI 108.
    pub const SPAN_TOP_HZ: f32 = 4186.009;
    /// Spans a block at it, the worst block: ⌈64·4186/48000⌉ + 1. A linked
    /// FRMT's fires are as many (the wraps and the note-on's).
    pub const SPANS_COVERED: u32 = 7;
    /// Instructions a span adds with no link (`run_formant`'s span path 93
    /// with the fire, and its block path 183, an upper bound) and a fire
    /// with one (`Grains::fire` 42 and its call 5): counted, 5901f08e.
    const SPAN_INSTR: u32 = 276;
    const FIRE_INSTR: u32 = 47;

    /// Spans (fires) at grain clock `hz` in the worst block.
    pub fn spans(hz: f32) -> u32 {
        let per = (hz.max(0.0) * BLOCK_SIZE as f32 / SAMPLE_RATE as f32).min(BLOCK_SIZE as f32);
        let whole = per as u32;
        whole + u32::from(per > whole as f32) + 1
    }

    /// What a voice whose pitch can reach `top_hz` (note, TRNSP, bend,
    /// routes and a glide's start, before the operators' ratios) costs over
    /// the static terms: `SpanReach::of(..).extra(top_hz)`.
    pub fn high_note_extra(
        p: &AlgoParams,
        priced: &[bool; OPS],
        fine_top: &[bool; OPS],
        top_hz: f32,
    ) -> Cost {
        SpanReach::of(p, priced, fine_top).extra(top_hz)
    }

    /// A priced FRMT operator: its lanes (SKIRT 0's spans or moving loop
    /// cost less), with links or without, and its laws moving.
    pub const fn frmt_cost(skirt: u8, linked: bool) -> u32 {
        match (skirt == 0, linked) {
            (true, false) => Self::COST_FRMT_ONE.0 + Self::COST_FRMT_MOVE.0,
            (false, false) => Self::COST_FRMT.0 + Self::COST_FRMT_MOVE.0,
            (true, true) => Self::COST_FRMT_ONE_LINKED.0 + Self::COST_FRMT_MOVE_LINKED.0,
            (false, true) => Self::COST_FRMT_LINKED.0 + Self::COST_FRMT_MOVE_LINKED.0,
        }
    }

    /// A priced NOIS operator, with a link in or without.
    pub const fn nois_cost(linked: bool) -> u32 {
        if linked {
            Self::COST_NOIS_LINKED.0
        } else {
            Self::COST_NOIS.0
        }
    }

    /// A voice's cycles/sample from its patch's shape (spec addendum). An
    /// operator is priced when its LEVEL is above 0 or has a mod route
    /// (`level_routed`): the kernel skips only operators silent all block.
    /// Links are those into priced operators in the union of ALG A and B,
    /// at any MORPH, as the plan runs them all. One block can cost more than
    /// this: a LEVEL ramping down to 0 still runs, and an ALG edit renders
    /// the old plan through its duck block. The 30% reserve absorbs it.
    pub const fn cost(p: &AlgoParams, level_routed: &[bool; OPS]) -> Cost {
        let (a, b) = p.algs();
        let (a, b) = (&ALGORITHMS[a.get() as usize], &ALGORITHMS[b.get() as usize]);
        // A formant or noise band with a link in runs its linked path
        // (`Linked`, as the budget bills it).
        let linked = p.linked();
        let (mut active, mut feedback, mut ops, mut i) = (0u8, 0, 0, 0);
        while i < OPS {
            if p.ops[i].level > 0 || level_routed[i] {
                active |= 1 << i;
                let kind = p.kinds().get(i);
                ops += match kind {
                    OpKind::Table(_) => Self::COST_OP.0,
                    OpKind::Formant => Self::frmt_cost(p.ops[i].skirt, linked.has(i)),
                    OpKind::Noise => Self::nois_cost(linked.has(i)),
                };
                if !p.ops[i].feedback.is_off() && !matches!(kind, OpKind::Noise) {
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

    /// What a fast pitch route (`engines::pitch_ramps`) adds to a voice's
    /// bill (#164): `COST_RAMP` for every table operator, an upper bound
    /// (the block takes a start increment for each one sounding). The
    /// Instrument bills it once a publish (`part_sound::ramps_in`).
    #[inline(always)]
    pub fn ramp_cost(p: &AlgoParams) -> Cost {
        let tables = (0..OPS)
            .filter(|&i| matches!(p.kinds().get(i), OpKind::Table(_)))
            .count();
        Cost(tables as u32 * Self::COST_RAMP.0)
    }

    /// Whether this voice's table operators ramp (`ramps`): its Part's
    /// `engines::pitch_ramps`, set by the Instrument before each block.
    pub fn set_pitch_ramps(&mut self, on: bool) {
        self.ramps = on;
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
            addr_of_mut!((*p).ramp_from).write(None);
            addr_of_mut!((*p).ramps).write(false);
            addr_of_mut!((*p).slide).write(Glide::new(1.0, 1.0));
            addr_of_mut!((*p).velocity).write(1.0);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).held).write(false);
            addr_of_mut!((*p).latch).write([OpLatch::INIT; OPS]);
            addr_of_mut!((*p).noise_c).write([WHITE.0; OPS]);
            addr_of_mut!((*p).noise_k).write([WHITE.1; OPS]);
            addr_of_mut!((*p).mix).write([OpLatch::INIT.mix; OPS]);
            addr_of_mut!((*p).c_ratio).write([1.0; OPS]);
            addr_of_mut!((*p).pm_k).write([1.0; OPS]);
            addr_of_mut!((*p).peak).write([Peak::INIT; OPS]);
            addr_of_mut!((*p).view).write([(0.0, 0.0); OPS]);
            addr_of_mut!((*p).law_key).write(None);
            addr_of_mut!((*p).laws).write([None; OPS]);
            addr_of_mut!((*p).noise).write([NoiseLaw::STALE; OPS]);
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
        self.ramp_from = None;
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
        // Slots, KEY, SKIRT, kinds and plan may all have moved: every law
        // is taken afresh.
        (self.law_key, self.noise) = (None, [NoiseLaw::STALE; OPS]);
        let mut idle = [false; OPS];
        for i in 0..OPS {
            self.rates[i] = p.ops[i].rates();
            let o = &p.ops[i];
            let l = &mut self.latch[i];
            (l.slot, l.key, l.mix) = (slots[i], f32::from(o.key), res_mix(o.res));
            if self.env[i].is_idle() {
                idle[i] = true;
                l.skirt = Skirt::from_code(o.skirt);
                self.kernel.reset(i, p.kinds().get(i));
                self.kinds[i] = p.kinds().get(i);
                if let OpKind::Table(_) = self.kinds[i] {
                    self.mip[i] = self.mip_target(p, &live, cycles, sr, m, &level, i)
                }
            }
            self.env[i].note_on(EnvCoefs::new(self.rates[i], note, sr));
        }
        // Every kind set first: a formant's PM depth reads its sources'.
        self.formant_blocks(p, &live, cycles, sr);
        let idle = (0..OPS).fold(0u8, |m, i| m | u8::from(idle[i]) << i);
        self.seed_ramps(&live, sr, idle);
        for i in (0..OPS).filter(|&i| idle & (1 << i) != 0) {
            self.gain[i] = self.target_gain(&live, i, sr);
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

    /// The glide steal's ratio now, 1 at rest.
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

    /// The loudest sounding carrier's envelope, 0..1: what the LPG gate
    /// follows with no VCA route (the max, a few compares a block).
    pub fn carrier_level(&self) -> f32 {
        let m = self.morph;
        (0..OPS)
            .filter(|&i| {
                self.gain[i] > 0.0 && blend(self.plan.carrier_a[i], self.plan.carrier_b[i], m) > 0.0
            })
            .fold(0.0f32, |t, i| t.max(self.env[i].level()))
            .min(1.0)
    }

    /// Restarts so far that found their lane's window open: 0 by
    /// construction (spec § 3).
    #[cfg(any(test, feature = "test-support"))]
    pub fn grain_cuts(&self) -> u32 {
        self.kernel.grain_cuts()
    }

    /// Formant operators render on the kernel's per-sample reference.
    #[cfg(any(test, feature = "test-support"))]
    pub fn formants_per_sample(&mut self) {
        self.kernel.formants_per_sample();
    }

    /// The next block takes every formant and noise law afresh, as if each
    /// input had moved.
    #[cfg(any(test, feature = "test-support"))]
    pub fn retake_laws(&mut self) {
        (self.law_key, self.noise) = (None, [NoiseLaw::STALE; OPS]);
    }

    /// Operator `i`'s ramp starts for the next block: the carrier ratio,
    /// the PM share, the peak's `g_m` and the noise band's coefficients.
    #[cfg(any(test, feature = "test-support"))]
    pub fn ramp_starts(&self, i: usize) -> [f32; 5] {
        [
            self.c_ratio[i],
            self.pm_k[i],
            self.peak[i].gm,
            self.noise_c[i],
            self.noise_k[i],
        ]
    }

    /// Every operator's noise from `voice`'s seeds: once, as the slot is
    /// built, never at a note-on.
    pub fn seed(&mut self, voice: VoiceIdx) {
        self.kernel.seed(voice.index() as u8);
    }

    /// The formant and noise operators' last `(fc, bw_eff)`, Hz.
    pub fn formant_view(&self) -> [(f32, f32); OPS] {
        self.view
    }

    /// Ruling 7: a sounding FRMT operator latched at SKIRT ≥ 1 runs two
    /// lanes where a stored SKIRT 0 bills one; the difference, per
    /// operator.
    pub fn held_skirt_extra(&self, p: &AlgoParams) -> Cost {
        if !self.active {
            return Cost::ZERO;
        }
        let l = p.linked();
        Cost(
            (0..OPS)
                .filter(|&i| {
                    self.kinds[i] == OpKind::Formant
                        && self.latch[i].skirt != Skirt::S0
                        && p.ops[i].skirt == 0
                })
                .map(|i| Self::frmt_cost(1, l.has(i)) - Self::frmt_cost(0, l.has(i)))
                .sum(),
        )
    }

    /// One block, through the door (ADR 0212): `kern::render`, bank 2's.
    /// With the door closed the voice is silent and ends.
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
        if through(|d| (d.render)(self, out, p, live, sample_rate, taps)).is_none() {
            out.fill(0.0);
            self.active = false;
        }
    }

    /// An active voice's block: its ramps, the kernel and its lifetime.
    #[inline(always)]
    fn block(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &AlgoParams,
        live: &AlgoLive,
        sample_rate: u32,
        taps: &mut TapRows,
    ) {
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
        // Only a table reads a table: the others' mips hold. Each table's
        // pitch is taken once, for its mip and its increment.
        let opc: [f32; OPS] = core::array::from_fn(|i| match self.kinds[i] {
            OpKind::Table(_) => op_cycles(cycles, p, live, i),
            OpKind::Formant | OpKind::Noise => 0.0,
        });
        let mips: [(usize, f32, f32); OPS] = core::array::from_fn(|i| match self.kinds[i] {
            OpKind::Table(_) => mip_step(self.mip[i], self.mip_of(opc[i], sr, m, &level, i)),
            OpKind::Formant | OpKind::Noise => (0, 0.0, 0.0),
        });
        // Under a fast pitch route each table operator starts the block at
        // the last block's pitch and ramps to this one's (#164).
        let from = self.ramp_from.filter(|_| self.ramps);
        self.ramp_from = Some(cycles);
        let mut dinc = [0i32; OPS];
        let mut view = self.view;
        let mut peak = self.peak;
        self.formant_blocks(p, live, cycles, sr);
        let ops: [OpBlock; OPS] = core::array::from_fn(|i| {
            let gain_to = self.target_gain(live, i, sr) * self.op_duck(i);
            if let Some(b) = &self.laws[i] {
                view[i] = (b.law.fc, b.law.bw_eff);
                let (o, end) = self.formant_block(i, b, gain_to);
                peak[i] = end;
                return o;
            }
            if self.kinds[i] == OpKind::Noise {
                let NoiseLaw {
                    band: (fc, _),
                    coef: (c, k),
                    ..
                } = self.noise[i];
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
            let to = (opc[i] * 4_294_967_296.0) as u32;
            // A silent operator takes no start: it plays nothing to ramp.
            let sounds = self.gain[i] != 0.0f32 || gain_to != 0.0f32;
            let inc = match from.filter(|_| sounds) {
                Some(c) => {
                    let at = (op_cycles(c, p, live, i) * 4_294_967_296.0) as u32;
                    dinc[i] = ((i64::from(to) - i64::from(at)) / BLOCK_SIZE as i64) as i32;
                    at
                }
                None => to,
            };
            OpBlock {
                inc,
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
        if from.is_some() {
            self.kernel
                .render_ramped(&blk, &dinc, &mut self.env, out, taps);
        } else {
            self.kernel.render(&blk, &mut self.env, out, taps);
        }
        (self.view, self.peak) = (view, peak);
        for (i, o) in ops.iter().enumerate() {
            self.gain[i] = o.gain_to;
            self.mix[i] = self.latch[i].mix;
            match o.kind {
                KindBlock::Noise(n) => {
                    (self.noise_c[i], self.noise_k[i]) = (n.c.1, n.k.1);
                    self.view[i] = self.noise[i].band;
                }
                KindBlock::Formant(f) => (self.c_ratio[i], self.pm_k[i]) = (f.c_ratio.1, f.pm.1),
                KindBlock::Table { .. } => self.mip[i] = mips[i].0 as f32 + mips[i].2,
            }
        }
        (self.morph, self.norm) = (m.get(), norm);
        self.finish_swap(p, live, cycles, sr);
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

    /// Formant operator `i`'s block from its law block `b`: the ramps from
    /// where the last block left them, and where its drive ends.
    #[cfg_attr(feature = "probe", inline(never))]
    fn formant_block(&self, i: usize, b: &LawBlock, gain_to: f32) -> (OpBlock, Peak) {
        let (gm, end) = self.peak[i].next(self.pm_k[i], &b.law, &b.drive);
        let o = OpBlock {
            inc: b.inc,
            gain_from: self.gain[i],
            gain_to,
            feedback: b.drive.feedback,
            kind: KindBlock::Formant(FormantBlock {
                inv_inc: b.inv_inc,
                inc_c: b.inc_c,
                inv_d: b.inv_d,
                h: b.pm.h,
                inv_h: b.pm.inv_h,
                m: b.m,
                c0: b.c0,
                c_ratio: (self.c_ratio[i], b.ratio),
                pm: (self.pm_k[i], b.law.pm),
                feedback: (self.peak[i].feedback, b.drive.feedback),
                gm,
                skirt: self.latch[i].skirt,
            }),
        };
        (o, end)
    }

    /// Cycles per sample of a ratio-1 operator.
    fn cycles(&self, p: &AlgoParams, sr: f32) -> f32 {
        let mut st = self.note.get() as f32 + p.transpose as f32 - 69.0;
        if self.pitch != 0.0 {
            st += self.pitch;
        }
        440.0 * exp2(st / 12.0) / sr * self.slide.value()
    }

    /// `mip_of` at operator `i`'s own pitch: the note-on's, out of line
    /// in bank 1.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
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
        self.mip_of(op_cycles(cycles, p, live, i), sr, m, level, i)
    }

    /// The mip of table operator `i` at `opc` cycles a sample (`op_cycles`).
    #[inline(always)]
    fn mip_of(&self, opc: f32, sr: f32, m: Morph, level: &[f32; OPS], i: usize) -> f32 {
        let bandwidth = opc * sr * (1.0 + incoming(&self.plan, m, i, level));
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
    /// `g_m` is the kernel's (`Peak`), so a formant's is LEVEL's at the edge.
    /// A noise operator's band law is taken afresh only when it moves.
    fn target_gain(&mut self, live: &AlgoLive, i: usize, sr: f32) -> f32 {
        let g = self.level_gain(live, i);
        match (self.kinds[i], &self.laws[i]) {
            (OpKind::Formant, Some(b)) => g * b.law.edge,
            (OpKind::Noise, _) => g * NOISE_GAIN * self.noise_law(live, i, sr).edge,
            _ => g * SAMPLE_SCALE,
        }
    }

    /// Noise operator `i`'s band law: taken afresh when FRMT, BW or VOWEL
    /// as played, or its slot, moved.
    fn noise_law(&mut self, live: &AlgoLive, i: usize, sr: f32) -> NoiseLaw {
        let slot = self.latch[i].slot.map_or(0, |s| s as u32 + 1);
        let key = [
            live.frmt[i].to_bits(),
            live.bw[i].to_bits(),
            live.vowel.to_bits(),
            slot,
        ];
        if self.noise[i].key != key {
            self.noise[i] = self.take_noise_law(live, i, sr, key);
        }
        self.noise[i]
    }

    /// Noise operator `i`'s band law taken afresh: the moving path.
    #[cfg_attr(feature = "probe", inline(never))]
    fn take_noise_law(&self, live: &AlgoLive, i: usize, sr: f32, key: [u32; 4]) -> NoiseLaw {
        let band = self.noise_band(live, i);
        NoiseLaw {
            key,
            band,
            coef: noise_coef(band.1, sr),
            edge: noise_edge(band.0, band.1),
        }
    }

    /// Every formant operator's law block this block, in `laws`: taken
    /// afresh only when an input moved since the last (Task 11b); held,
    /// the last. None for any other kind.
    fn formant_blocks(&mut self, p: &AlgoParams, live: &AlgoLive, cycles: f32, sr: f32) {
        if !self.kinds.contains(&OpKind::Formant) {
            (self.laws, self.law_key) = ([None; OPS], None);
            return;
        }
        let key = self.law_key(p, live, sr);
        // Word by word, no call: the held path's test.
        let held = self
            .law_key
            .as_ref()
            .is_some_and(|k| k.iter().zip(&key).fold(0, |d, (a, b)| d | (a ^ b)) == 0);
        if !held {
            self.take_laws(p, live, cycles, sr);
            self.law_key = Some(key);
        }
    }

    /// The formant laws taken afresh: the moving path.
    #[cfg_attr(feature = "probe", inline(never))]
    fn take_laws(&mut self, p: &AlgoParams, live: &AlgoLive, cycles: f32, sr: f32) {
        // Each law in place, in the plan's order, so a source faded at its
        // edge carries its fade into its destinations' PM depth; a source
        // the order hasn't reached counts at full. (A returned array of
        // laws cost a 624 B copy a block.)
        let mut fade = [1.0f32; OPS];
        self.laws.fill(None);
        for k in 0..OPS {
            let i = self.plan.order[k] as usize % OPS;
            if self.kinds[i] == OpKind::Formant {
                let (law, drive) = self.formant(p, live, cycles, sr, i, &fade);
                fade[i] = law.edge;
                self.laws[i] = Some(LawBlock::new(law, drive, op_cycles(cycles, p, live, i), sr));
            }
        }
    }

    /// `LawKey` now.
    fn law_key(&self, p: &AlgoParams, live: &AlgoLive, sr: f32) -> LawKey {
        let mut k = [0u32; LAW_KEY];
        let ops = [
            &live.frmt,
            &live.bw,
            &live.level,
            &live.fine,
            &live.detune,
            &live.feedback,
        ];
        for (row, a) in k.as_chunks_mut::<OPS>().0.iter_mut().zip(ops) {
            for (w, v) in row.iter_mut().zip(a) {
                *w = v.to_bits();
            }
        }
        let c = |i: usize| p.ops[i].coarse;
        k[6 * OPS..].copy_from_slice(&[
            live.vowel.to_bits(),
            live.morph.to_bits(),
            self.pitch.to_bits(),
            self.slide.value().to_bits(),
            sr.to_bits(),
            u32::from_le_bytes([self.note.get(), p.transpose as u8, c(0), c(1)]),
            u32::from_le_bytes([c(2), c(3), c(4), c(5)]),
            0,
        ]);
        k
    }

    /// A formant operator's law and drive this block (spec § 3): its pitch is the
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
        fade: &[f32; OPS],
    ) -> (FormantLaw, Drive) {
        let l = &self.latch[i];
        let fc = self.formant_fc(p, live, i);
        let f0 = op_cycles(cycles, p, live, i) * sr;
        let drive = self.drive(p, live, cycles, sr, i, fade);
        (
            formant_law_pm(f0, fc, bw_hz(live.bw[i]), l.skirt, drive),
            drive,
        )
    }

    /// A formant operator's FC before its law: FRMT, VOWEL and KEY on its
    /// latched slot.
    fn formant_fc(&self, p: &AlgoParams, live: &AlgoLive, i: usize) -> f32 {
        let l = &self.latch[i];
        fc_hz(live.frmt[i], live.vowel, l.slot, l.key, self.key_pitch(p))
    }

    /// What drives operator `i`'s carrier at full envelope: its FDBK, and
    /// its links' PM as depth (Σ `PM_CYCLES`·weight·LEVEL·fade, cycles) and
    /// as a plain carrier's deviation (the same, times 2π·f_m: a table's
    /// pitch, a formant's FC). `fade` is a formant source's edge. Noise
    /// counts in the depth, not the deviation: its sidebands are noise, and
    /// folding them is no alias.
    fn drive(
        &self,
        p: &AlgoParams,
        live: &AlgoLive,
        cycles: f32,
        sr: f32,
        i: usize,
        fade: &[f32; OPS],
    ) -> Drive {
        let m = Morph::from_param(live.morph).get();
        let plan = &self.plan;
        let (mut dev, mut depth) = (0.0f32, 0.0f32);
        for e in plan.edges.iter().take(plan.edge_count()) {
            if e.dst as usize != i {
                continue;
            }
            let src = e.src as usize % OPS;
            let d = blend(e.a, e.b, m) * level_gain(live.level[src]) * fade[src];
            depth += d;
            dev += d * match self.kinds[src] {
                OpKind::Table(_) => op_cycles(cycles, p, live, src) * sr,
                OpKind::Formant => self.formant_fc(p, live, src),
                OpKind::Noise => 0.0,
            };
        }
        Drive {
            dev: dev * core::f32::consts::TAU * PM_CYCLES,
            depth: depth * PM_CYCLES,
            feedback: feedback_cycles(live.feedback[i]),
        }
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
    /// vowel slot, with no KEY (spec § 4); FRMT, BW and VOWEL as played.
    fn noise_band(&self, live: &AlgoLive, i: usize) -> (f32, f32) {
        let fc = fc_hz(live.frmt[i], live.vowel, self.latch[i].slot, 0.0, 0.0);
        (fc, bw_hz(live.bw[i]))
    }

    fn adopt_alg(&mut self, p: &AlgoParams) {
        let key = p.algs();
        if key != self.plan_key {
            self.plan = plan(key.0, key.1);
            (self.plan_key, self.law_key) = (key, None);
        }
    }

    fn begin_swap(&mut self, p: &AlgoParams) {
        if self.swap != Swap::Idle {
            return;
        }
        let alg = p.algs() != self.plan_key;
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

    /// The duck's end: the new plan and kinds in, each swapped-in formant
    /// or noise band starting closed, its ramps seeded from its new law as
    /// an idle operator's are at `note_on`. The laws are taken here, so the
    /// next block holds them.
    fn finish_swap(&mut self, p: &AlgoParams, live: &AlgoLive, cycles: f32, sr: f32) {
        let Swap::Ducking { alg, kinds } = self.swap else {
            self.swap = Swap::Idle;
            return;
        };
        if alg {
            self.adopt_alg(p);
        }
        for i in (0..OPS).filter(|&i| kinds & (1 << i) != 0) {
            let (old, new) = (self.kinds[i], p.kinds().get(i));
            // A table to a table runs on; any other kind starts closed,
            // under the duck.
            if old.is_formant() || new.is_formant() {
                self.kernel.reset(i, new);
            }
            (self.kinds[i], self.law_key) = (new, None);
        }
        self.formant_blocks(p, live, cycles, sr);
        self.seed_ramps(live, sr, kinds);
        self.swap = Swap::Rising;
    }

    /// The operators in `ops` start closed: a formant's ramps (carrier
    /// ratio, PM share, peak) from its law in `laws`, a noise band's from
    /// its band law. Out of line: `note_on`'s and the swap's one copy.
    #[inline(never)]
    fn seed_ramps(&mut self, live: &AlgoLive, sr: f32, ops: u8) {
        for i in (0..OPS).filter(|&i| ops & (1 << i) != 0) {
            match (self.kinds[i], &self.laws[i]) {
                (OpKind::Formant, Some(b)) => {
                    (self.c_ratio[i], self.pm_k[i]) = (b.ratio, b.law.pm);
                    self.peak[i] = Peak::at(&b.law, &b.drive);
                }
                (OpKind::Noise, _) => {
                    (self.noise_c[i], self.noise_k[i]) = self.noise_law(live, i, sr).coef;
                }
                _ => {}
            }
        }
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

/// A Sound's priced FRMT operators' ratios to the voice's pitch, and what
/// a span past `SPANS_COVERED` costs each: taken once per Part a block, so
/// a voice's own reach is a span count an operator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpanReach {
    /// 0: not a priced FRMT.
    ratio: [f32; OPS],
    instr: [u32; OPS],
}

impl SpanReach {
    /// Each priced FRMT's ratio at FINE's top where a route moves it
    /// (`fine_top`), and DETUNE at its top always: a few cents, never
    /// worth a lookup.
    pub fn of(p: &AlgoParams, priced: &[bool; OPS], fine_top: &[bool; OPS]) -> Self {
        let l = p.linked();
        let mut r = SpanReach {
            ratio: [0.0; OPS],
            instr: [0; OPS],
        };
        for i in 0..OPS {
            let o = &p.ops[i];
            if p.kinds().get(i) != OpKind::Formant || !(priced[i] || o.level > 0) {
                continue;
            }
            let fine = if fine_top[i] { 15.0 } else { f32::from(o.fine) };
            r.ratio[i] = ratio_at(o.coarse, fine) * DETUNE_TOP;
            r.instr[i] = if l.has(i) {
                AlgoEngine::FIRE_INSTR
            } else {
                AlgoEngine::SPAN_INSTR
            };
        }
        r
    }

    /// Each priced FRMT's spans past `SPANS_COVERED` at its grain clock
    /// for a voice reaching `top_hz`, ADR 0056's cycles a sample, rounded
    /// up.
    pub fn extra(&self, top_hz: f32) -> Cost {
        let instr: u32 = (0..OPS)
            .filter(|&i| self.instr[i] > 0)
            .map(|i| {
                let over = AlgoEngine::spans(top_hz * self.ratio[i])
                    .saturating_sub(AlgoEngine::SPANS_COVERED);
                over * self.instr[i]
            })
            .sum();
        // instr · 1.46 · 1.1 / 64, rounded up.
        Cost((instr * 1606).div_ceil(64_000))
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

/// ALGO's block (`AlgoEngine::block`).
pub type AlgoRun =
    fn(&mut AlgoEngine, &mut [f32; BLOCK_SIZE], &AlgoParams, &AlgoLive, u32, &mut TapRows);

/// ALGO's entry (ADR 0212): one call a voice a block.
#[repr(C)]
pub struct AlgoDoor {
    pub render: AlgoRun,
}

impl AlgoDoor {
    pub const DOOR: AlgoDoor = AlgoDoor {
        render: kern::render,
    };
}

/// `f` on ALGO's block: through the door with `bank2` (`None` while it is
/// closed), else bank 1's own on a host. A firmware built without bank 2
/// has no room for it in bank 1: there ALGO is silent.
#[inline(always)]
fn through<R>(f: impl FnOnce(&AlgoDoor) -> R) -> Option<R> {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL.with(|d| f(&d.algo))
    }
    #[cfg(all(not(feature = "bank2"), not(target_os = "none")))]
    {
        Some(f(&AlgoDoor::DOOR))
    }
    #[cfg(all(not(feature = "bank2"), target_os = "none"))]
    {
        let _ = f;
        None
    }
}

/// Bank 2's: `bank2.x` links `*4algo6engine4kern*`, and the kernel.
mod kern {
    use super::{AlgoEngine, AlgoLive, AlgoParams, BLOCK_SIZE, TapRows};

    #[inline(never)]
    pub(super) fn render(
        e: &mut AlgoEngine,
        out: &mut [f32; BLOCK_SIZE],
        p: &AlgoParams,
        live: &AlgoLive,
        sample_rate: u32,
        taps: &mut TapRows,
    ) {
        e.block(out, p, live, sample_rate, taps);
    }
}
