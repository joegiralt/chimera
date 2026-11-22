//! VA: two variable-wave oscillators, an operator, a sub and STABILITY
//! (spec § Pages, § Params and Motion, § Combine, § Stability): the params
//! block and `VaEngine`.

pub mod blep;
pub mod combine;
pub mod lbfm;
pub mod noise;
pub mod osc;
pub mod stability;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::block::{
    Block, Change, DiskCode, Motion, ParamId, ParamSpec, Smoothing, ValFmt, apply_code,
};
use crate::dsp::algo::math::exp2;
use crate::dsp::ease::{Ramp, step_of};
use crate::dsp::glide::Glide;
use crate::dsp::{clampf, sqrt};
use crate::glyph::{FocusGlyph, VaGlyph, VaOsc};
use crate::hw::Cost;
use crate::project::PartId;
use crate::voice_alloc::VoiceIdx;
use combine::{AmKinks, Crusher, MaxKink, Operator, Peaks, am_gain, am_reach, ring_gain};
use noise::Noise;
pub use noise::{KIND_NAMES, NoiseKind};
use osc::solo::SoloRun;
use osc::{MAX_INC, NO_WRAP, R, Ramps, Sub, SubEdge, VarOsc, ZEROS, wave_reach};
use stability::{Depth, JitterScale, Seed, Stability, WrapJitter, depth};
#[cfg(any(test, feature = "test-support"))]
use {osc::Correction, stability::Character};

/// How the two oscillators combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum VaMode {
    #[default]
    Mix = 0,
    Sync = 1,
    Ring = 2,
    Xor = 3,
    Bits = 4,
    /// OSC 2 frequency-modulates OSC 1 linearly, through zero (#601).
    Fm = 5,
    /// OSC 2 scales OSC 1, unipolar to BLEND 85, then on into ring at 127
    /// (#603, #675, ADR 0176).
    Am = 6,
    /// The larger of the two oscillators, its kinks rounded (#605).
    Max = 7,
    /// Each oscillator loops back on its own frequency (#613).
    LoopFm = 8,
    /// OSC 2's read reverses at OSC 1's wraps: mirrored sync (#604).
    SoftSync = 9,
    /// OSC 2 phase-modulates OSC 1, a CZ-like phase distortion (#602).
    Phase = 10,
}

impl VaMode {
    pub const ALL: [VaMode; 11] = [
        VaMode::Mix,
        VaMode::Sync,
        VaMode::Ring,
        VaMode::Xor,
        VaMode::Bits,
        VaMode::Fm,
        VaMode::Am,
        VaMode::Max,
        VaMode::LoopFm,
        VaMode::SoftSync,
        VaMode::Phase,
    ];

    /// Whether this MODE's oscillators are `osc::door`'s, which with
    /// `bank2` play only while bank 2 is open (ADR 0163).
    pub const fn in_bank2(self) -> bool {
        matches!(
            self,
            VaMode::Fm | VaMode::LoopFm | VaMode::SoftSync | VaMode::Phase
        )
    }
}

/// MODE's names, in `VaMode`'s order (display only; the long words read
/// small beside the scope, `Layout::SmallBeside`).
pub const MODE_NAMES: [&str; 11] = [
    "MIX",
    "SYNC",
    "RING",
    "XOR",
    "BITS",
    "FM",
    "AM",
    "MAX",
    "LOOPFM",
    "SOFT SYNC",
    "PHASE",
];

impl DiskCode for VaMode {
    fn disk_code(self) -> u8 {
        match self {
            VaMode::Mix => 0,
            VaMode::Sync => 1,
            VaMode::Ring => 2,
            VaMode::Xor => 3,
            VaMode::Bits => 4,
            VaMode::Fm => 5,
            VaMode::Am => 6,
            VaMode::Max => 7,
            VaMode::LoopFm => 8,
            VaMode::SoftSync => 9,
            VaMode::Phase => 10,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            VaMode::Mix => "MIX",
            VaMode::Sync => "SYNC",
            VaMode::Ring => "RING",
            VaMode::Xor => "XOR",
            VaMode::Bits => "BITS",
            VaMode::Fm => "FM",
            VaMode::Am => "AM",
            VaMode::Max => "MAX",
            VaMode::LoopFm => "LOOPFM",
            VaMode::SoftSync => "SOFTSYNC",
            VaMode::Phase => "PHASE",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(VaMode::Mix),
            1 => Some(VaMode::Sync),
            2 => Some(VaMode::Ring),
            3 => Some(VaMode::Xor),
            4 => Some(VaMode::Bits),
            5 => Some(VaMode::Fm),
            6 => Some(VaMode::Am),
            7 => Some(VaMode::Max),
            8 => Some(VaMode::LoopFm),
            9 => Some(VaMode::SoftSync),
            10 => Some(VaMode::Phase),
            _ => None,
        }
    }
}

/// The VA Sound's parameters; each pair is OSC 1's, then OSC 2's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VaParams {
    pub wave: [f32; 2],
    pub shape: [f32; 2],
    pub coarse: [i8; 2],
    /// Cents.
    pub fine: [f32; 2],
    pub mode: VaMode,
    pub amount: f32,
    pub sub: f32,
    pub stability: f32,
    /// NOISE's colour and LEVEL (ADR 0197).
    pub noise_kind: NoiseKind,
    pub noise: f32,
}

/// The format's neutral (ADR 0087): a saved Sound omits what is neutral.
impl Default for VaParams {
    fn default() -> Self {
        Self {
            wave: [osc::SAW_ANCHOR.get(); 2],
            shape: [0.0; 2],
            coarse: [0; 2],
            fine: [0.0; 2],
            mode: VaMode::Mix,
            amount: 0.5,
            sub: 0.0,
            stability: 1.0,
            noise_kind: NoiseKind::White,
            noise: 0.0,
        }
    }
}

impl VaParams {
    pub const WAVE1: ParamId = ParamId(0);
    pub const WAVE2: ParamId = ParamId(1);
    pub const MODE: ParamId = ParamId(2);
    pub const AMOUNT: ParamId = ParamId(3);
    pub const SUB: ParamId = ParamId(4);
    pub const STAB: ParamId = ParamId(5);
    pub const CRSE1: ParamId = ParamId(6);
    pub const FINE1: ParamId = ParamId(7);
    pub const SHAPE1: ParamId = ParamId(8);
    pub const CRSE2: ParamId = ParamId(9);
    pub const FINE2: ParamId = ParamId(10);
    pub const SHAPE2: ParamId = ParamId(11);
    pub const NOISE_KIND: ParamId = ParamId(12);
    pub const NOISE: ParamId = ParamId(13);
}

const fn uni(id: u8, label: &'static str, ident: &'static str, default: f32) -> ParamSpec {
    ParamSpec::continuous(
        id,
        label,
        ValFmt::Uni,
        0.0,
        1.0,
        default,
        1.0 / 128.0,
        true,
        Smoothing::KNOB,
    )
    .ident(ident)
}

const fn coarse(id: u8, label: &'static str, short: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Signed(24),
        -24.0,
        24.0,
        0.0,
        true,
        Motion::Stepped(Change::Snap),
    )
    .ident(label)
    .short(short)
    .semitones(24.0)
}

const fn fine(id: u8, label: &'static str, short: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Signed(100),
        -100.0,
        100.0,
        0.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident(label)
    .short(short)
    .cents(100.0)
}

pub static VA_SPECS: [ParamSpec; 14] = [
    uni(0, "WAVE1", "WAVE1", osc::SAW_ANCHOR.get())
        .short("WV1")
        .glyph(FocusGlyph::Va(VaGlyph::Wave(VaOsc::One))),
    uni(1, "WAVE2", "WAVE2", osc::SAW_ANCHOR.get())
        .short("WV2")
        .glyph(FocusGlyph::Va(VaGlyph::Wave(VaOsc::Two))),
    ParamSpec::choice(
        2,
        "MODE",
        ValFmt::Names(&MODE_NAMES),
        10.0,
        0.0,
        Change::Crossfade,
    )
    .ident("MODE")
    .glyph(FocusGlyph::Va(VaGlyph::Mode)),
    uni(3, "AMNT", "AMOUNT", 0.5).glyph(FocusGlyph::Va(VaGlyph::Blend)),
    uni(4, "SUB", "SUB", 0.0).glyph(FocusGlyph::LevelBar),
    uni(5, "STAB", "STAB", 1.0).glyph(FocusGlyph::Stability),
    coarse(6, "CRSE1", "CR1"),
    fine(7, "FINE1", "FN1"),
    // SHAPE draws the wave it makes at its oscillator's WAVE (#980).
    uni(8, "SHAPE1", "SHAPE1", 0.0)
        .short("SH1")
        .glyph(FocusGlyph::Va(VaGlyph::Wave(VaOsc::One))),
    coarse(9, "CRSE2", "CR2"),
    fine(10, "FINE2", "FN2"),
    uni(11, "SHAPE2", "SHAPE2", 0.0)
        .short("SH2")
        .glyph(FocusGlyph::Va(VaGlyph::Wave(VaOsc::Two))),
    ParamSpec::choice(
        12,
        "KIND",
        ValFmt::Names(&KIND_NAMES),
        3.0,
        0.0,
        Change::Snap,
    )
    .ident("NOISEKIND")
    .glyph(FocusGlyph::None),
    uni(13, "NOISE", "NOISE", 0.0).glyph(FocusGlyph::LevelBar),
];

impl Block for VaParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &VA_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVE1 => self.wave[0],
            Self::WAVE2 => self.wave[1],
            Self::MODE => self.mode.disk_code() as f32,
            Self::AMOUNT => self.amount,
            Self::SUB => self.sub,
            Self::STAB => self.stability,
            Self::CRSE1 => self.coarse[0] as f32,
            Self::FINE1 => self.fine[0],
            Self::SHAPE1 => self.shape[0],
            Self::CRSE2 => self.coarse[1] as f32,
            Self::FINE2 => self.fine[1],
            Self::SHAPE2 => self.shape[1],
            Self::NOISE_KIND => self.noise_kind.disk_code() as f32,
            Self::NOISE => self.noise,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        // A modulated CRSE is stepped; NaN rounds to 0 (`as` saturates).
        let semis = || clampf(libm::roundf(v), -24.0, 24.0) as i8;
        match id {
            Self::WAVE1 => self.wave[0] = v,
            Self::WAVE2 => self.wave[1] = v,
            Self::MODE => self.mode = VaMode::from_disk_code(v as u8).unwrap_or_default(),
            Self::AMOUNT => self.amount = v,
            Self::SUB => self.sub = v,
            Self::STAB => self.stability = v,
            Self::CRSE1 => self.coarse[0] = semis(),
            Self::FINE1 => self.fine[0] = v,
            Self::SHAPE1 => self.shape[0] = v,
            Self::CRSE2 => self.coarse[1] = semis(),
            Self::FINE2 => self.fine[1] = v,
            Self::SHAPE2 => self.shape[1] = v,
            Self::NOISE_KIND => {
                self.noise_kind = NoiseKind::from_disk_code(v as u8).unwrap_or_default()
            }
            Self::NOISE => self.noise = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::MODE => Some(self.mode.disk_code()),
            Self::NOISE_KIND => Some(self.noise_kind.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::MODE => Some(self.mode.disk_ident()),
            Self::NOISE_KIND => Some(self.noise_kind.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::MODE => apply_code(VaMode::from_disk_code(code), |m| self.mode = m),
            Self::NOISE_KIND => {
                apply_code(NoiseKind::from_disk_code(code), |k| self.noise_kind = k)
            }
            _ => false,
        }
    }
}

/// The morph's working level `R` to the chain's: the plain saw at ±1, the
/// "maximum saw" the 2040 is tuned on (ADR 0096).
pub const VA_CHAIN_GAIN: f32 = 1.0 / (R * 1.732_050_8);

/// VA's output gain at the VCA (ADR 0058), set so INIT (two saws, FINE2 +6,
/// the 2040 at its defaults; C4 at velocity 100, 4 whole beats) sits at ADR
/// 0109's reference, −11.0 LUFS, as TAPE_OUT_GAIN does for TAPE.
pub const VA_OUT_GAIN: f32 = 3.631;

/// Cycles a sample per MODE by ADR 0056's count on the release ELF (instr ×
/// 1.46 × 1.1, + 12 a `vdiv`/`vsqrt`; ADR 0118's scripts). Each oscillator
/// bills its dearest wave pair, constants lerped and SHAPE and WAVE moving:
/// its loop's round with no event, + 0.45 of its wrap's side (OSC 1: its
/// one gate to the sub's flip and the wrap, taken twice a cycle, #485; it
/// records its wraps in every MODE, one instance for flash), +
/// each edge's crossing side at 0.45 + 0.225/64 a sample (backward
/// 0.225/64), all stretched by the jitter's half cent; the SYNC slave + its
/// resets. Then the operator loop (its ducking variant; sub flips at 0.45),
/// the steady block's work ÷ 64 and one WAVE segment crossing an oscillator
/// every block (WAVE's reach, `wave_reach`), an extra run with its share of
/// the render loop, plus the crossing block's split peak bound, and LP24's
/// 95. In SYNC the master's events and the slave's resets run at its 7 kHz
/// ceiling, 7/48 a sample. Each line: OSC 1 (TRI/SAW) + OSC 2 + operator
/// and sub + block + crossings + 95 (2026-10-13, ADR 0118 point 13).
///
/// MIX: 158.2 + 134.4 (TRI/SAW) + 36.2 + 71.6 + 29.9 + 95 = 525.4 (the
/// recount of 2026-10-20 on the merged tree, ADR 0165).
///
/// Each MODE then billed at its worst voice as the 0.3 unit reads it (ADR
/// 0218; bench 2026-10-26, rev V at 480 MHz, 4eae5d81): the row's top note
/// (a voice costs more as it climbs), its Part's routes off, rounded up to
/// 10. AM, MAX, SOFT SYNC and PHASE moved; the rest read at these counts.
pub const COST_MIX: Cost = Cost(530);
/// SYNC: 121.7 (master at 7 kHz) + 184.1 (SINE/TRI slave: a reset's 197.5
/// × 7/48) + 36.2 + 75.0 + 31.0 + 95 = 543.0.
pub const COST_SYNC: Cost = Cost(550);
/// RING: 158.2 + 134.4 + 42.6 + 72.2 + 29.9 + 95 = 532.4.
pub const COST_RING: Cost = Cost(540);
/// XOR: 158.2 + 134.4 + 53.9 + 73.4 + 29.9 + 95 = 544.8.
pub const COST_XOR: Cost = Cost(550);
/// BITS: 158.2 + 134.4 + 42.6 + 72.4 + 29.9 + 95 = 532.5.
pub const COST_BITS: Cost = Cost(540);
/// FM: 209.3 (SINE/TRI, OSC 1's MOD instance: its edges follow the read,
/// crossed either way at the 0.45 cap, a divide a crossing) + 134.4 + 29.4
/// + 77.8 (5.1 of it the modulator's scaling) + 29.9 + 95 = 575.8.
pub const COST_FM: Cost = Cost(580);
/// LOOPFM: 84.7 (OSC 1's `render_loop`: its round with the closed form's
/// divide, the wrap at the 0.45 cap with the jitter's draw; the scale
/// ramps per block) + 70.1 (OSC 2's) + 37.8 + 52.9 (no VarOsc runs: the
/// block is `render`'s and `render_loopfm`'s own) + 8.9 + 95 = 349.4.
/// The unit read 712 a voice against 584 billed (2026-10-22, bench VA,
/// rev V at 480 MHz): +130.
pub const COST_LOOPFM: Cost = Cost(480);
/// AM: 158.2 + 134.4 + 41.0 + 3.3 (the sub) + 72.6 (with `am_reach` at the
/// BLEND ramp's two ends a block, ADR 0176) + 29.9 + 95 = 534.3; the unit
/// read 550. Its product's corrector (#660, ADR 0321) adds, by
/// `tools/loop_count.py` on `render_am_kinks` in the release ELF, in
/// cycles (instructions × 1.606, MAX's convention): the operator's round,
/// 55.25 instructions a sample against today's 25.5, +29.75 = +47.8; a
/// sample where both waves step, 80 instructions more (128.5 cycles), six
/// a block at the worst voice = +12.0; both oscillators' listing
/// instances, 8.5 each as PHASE's OSC 2, +17. 550 + 76.8 = 626.8, rounded
/// up.
pub const COST_AM: Cost = Cost(630);
/// MAX: 158.2 + 134.4 + 50.6 (the operator's loop, with the step test) +
/// 3.3 + 37.7 (a kink a sample at the worst: one sign change, its divide and
/// the BLAMP) + 75.3 + 29.9 + 95 = 584.4.
pub const COST_MAX: Cost = Cost(550);
/// SOFT SYNC: 121.7 (master at 7 kHz) + 232.5 (TRI, OSC 2's reversing
/// instance through the door: its edges either way at 0.45, a divide a
/// crossing, a turn's 86.7 × 7/48) + 36.2 + 71.3 + 30.2 + 95 = 586.9.
pub const COST_SOFTSYNC: Cost = Cost(610);
/// PHASE (ADR 0160): 223.5 (SINE/TRI, OSC 1's own MOD instance, FM's
/// with a jump's test) + 142.9 (OSC 2 marking its steps) + 26.5 + 2.9 +
/// 119.9 (33.5 of it six jumps and marks a block, 6.3 the offset loop) +
/// 31.3 + 95 = 641.9.
pub const COST_PHASE: Cost = Cost(590);

/// NOISE's pass by KIND (ADR 0197): `tools/loop_count.py` on
/// `va::noise::kern::add` in the release ELF (bank 2), a sample's round
/// plus the entry's setup ÷ 64 (1.7): WHITE 10.6 instructions, PINK 20.0,
/// RED 16.0, DIGITAL 18.25 with a draw every sample (its fast path 12.0).
/// `render`'s own share (the level's lane, `z²` in the norm, the door's
/// call) is under 0.1 a sample.
pub const COST_NOISE_WHITE: Cost = Cost(20);
/// The unit reads WHITE 16, PINK 36, RED 32, DIGITAL 30 over VA (2026-10-22,
/// ROUTING bench, rev V at 480 MHz): PINK and RED re-billed.
pub const COST_NOISE_PINK: Cost = Cost(40);
pub const COST_NOISE_RED: Cost = Cost(35);
pub const COST_NOISE_DIGITAL: Cost = Cost(35);

/// PHASE's peak depth at BLEND `blend`, in cycles: `blend/2`. BLEND ½ is
/// the CZ's full warp (PD is PM at index 2π(½ − d), Lazzarini et al.,
/// DAFx-09).
pub fn phase_depth(blend: f32) -> f32 {
    0.5 * blend
}

/// STABILITY's jitter draw at scale `js`: one closure type for every
/// MODE, so the renders share the oscillators' instances (flash).
fn jitter_draw(stab: &mut Stability, js: JitterScale) -> impl FnMut() -> WrapJitter + '_ {
    move || js.draw(stab)
}

/// What a plain block's oscillator played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Played {
    /// The pair's runs.
    Pair,
    /// The saw alone, WAVE on its anchor (`osc::OnAnchor`, ADR 0283).
    Alone,
}

/// A MODE that plays the plain renders (`run`): the MODEs with renders of
/// their own (FM, LOOPFM, SOFT SYNC, PHASE) aren't one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlainMode {
    Mix,
    Sync,
    Ring,
    Xor,
    Bits,
    /// AM's uncorrected product: bank 1's with bank 2's door closed only
    /// (#660).
    #[cfg_attr(not(feature = "bank2"), allow(dead_code))]
    Am,
    Max,
}

impl PlainMode {
    /// The MODE it is, for the operator.
    #[inline(always)]
    fn mode(self) -> VaMode {
        match self {
            PlainMode::Mix => VaMode::Mix,
            PlainMode::Sync => VaMode::Sync,
            PlainMode::Ring => VaMode::Ring,
            PlainMode::Xor => VaMode::Xor,
            PlainMode::Bits => VaMode::Bits,
            PlainMode::Am => VaMode::Am,
            PlainMode::Max => VaMode::Max,
        }
    }

    /// OSC 2's instance in it.
    #[inline(always)]
    fn osc2(self) -> Osc2Run {
        match self {
            PlainMode::Sync => Osc2Run::Slave,
            PlainMode::Mix
            | PlainMode::Ring
            | PlainMode::Xor
            | PlainMode::Bits
            | PlainMode::Am
            | PlainMode::Max => Osc2Run::Plain,
        }
    }
}

/// OSC 2's plain instance in a MODE that plays the plain renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Osc2Run {
    /// SYNC's slave, resetting at OSC 1's wraps: always its pair.
    Slave,
    /// Plain: the saw alone on its anchor, else its pair.
    Plain,
}

/// `o`'s plain block (`run`): the saw alone through bank 2's door when the
/// block is on its anchor (`VarOsc::on_anchor`, ADR 0283), else, or with
/// the door closed, the pair's runs as ever (`pair`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn plain(
    o: &mut VarOsc,
    run: SoloRun,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut WrapJitter,
    stab: &mut Stability,
    js: JitterScale,
    pair: impl FnOnce(
        &mut VarOsc,
        &mut [f32; BLOCK_SIZE],
        &mut [[f32; 2]; BLOCK_SIZE],
        &mut WrapJitter,
        &mut Stability,
    ),
) -> Played {
    let o = match o.on_anchor(r) {
        Ok(at) => {
            // the proof goes through the door, which plays it; closed, the
            // door never takes it and it comes back
            let mut held = Some(at);
            osc::solo::through(|t| {
                if let Some(at) = held.take() {
                    t.solo().render(run, at, out, mk, jit, stab, js);
                }
            });
            match held {
                None => return Played::Alone,
                Some(at) => at.release(),
            }
        }
        Err(o) => o,
    };
    pair(o, out, mk, jit, stab);
    Played::Pair
}

/// FM's index at BLEND `blend`, `5·blend²`, capped so OSC 1's fastest read
/// `inc·(1 + I)` stays under `MAX_INC` (0.45 cycles a sample); `inc = 0`
/// is uncapped (the glyph).
pub fn fm_index(blend: f32, inc: f32) -> f32 {
    (5.0 * blend * blend).min(fm_cap(inc))
}

/// The index cap at `inc`, `max(0, MAX_INC/inc − 1)`.
fn fm_cap(inc: f32) -> f32 {
    (MAX_INC / inc - 1.0).max(0.0)
}

/// SYNC's master pitch ceiling, A8 − 10 cents (owner, 2026-10-11; ADR 0118
/// point 12): it bounds the slave's resets at 7 kHz.
pub const SYNC_MASTER_HZ: f32 = 7_000.0;

/// A MODE duck's gain over one block, if one runs.
type Gain = Option<&'static [f32; BLOCK_SIZE]>;

/// `render_loopfm` as the door carries it (ADR 0163, LOOPFM's whole render).
/// AM's render as the door carries it (`VaEngine::render_am_kinks`).
pub type AmRender = fn(&mut VaEngine, &mut [f32; BLOCK_SIZE], &Lanes, Gain);

pub type LoopFmRender = fn(&mut VaEngine, &mut [f32; BLOCK_SIZE], &Lanes, Gain, f32, [f32; 2]);

/// The MODE duck's length each way, two blocks.
const DUCK: usize = 2 * BLOCK_SIZE;

/// A jitter's cents as a share of the increment, `2^(c/1200) − 1` to first
/// order.
const CENT: f32 = core::f32::consts::LN_2 / 1200.0;

/// A MODE change: down over two blocks, the mode swaps, up over two. A
/// change while it goes down retargets it; one while it comes up waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Duck {
    Steady,
    /// Which of the two blocks down.
    Down(u8),
    Up(u8),
}

/// The duck's gain at sample `j` of its way down, `½(1 + cos(π(j + 1)/128))`:
/// a raised cosine landing on 0 (a linear duck clicks at its turnaround).
/// The cosine is its Taylor series in f64 at compile time.
static DUCK_DOWN: [[f32; BLOCK_SIZE]; 2] = {
    let mut g = [[0.0f32; BLOCK_SIZE]; 2];
    let mut j = 0;
    while j < DUCK {
        let x = core::f64::consts::PI * (j + 1) as f64 / DUCK as f64;
        // x ≤ π: 40 terms are past f64's precision
        let (mut term, mut sum, mut k) = (1.0f64, 1.0f64, 1);
        while k < 40 {
            term *= -x * x / ((2 * k - 1) * (2 * k)) as f64;
            sum += term;
            k += 1;
        }
        g[j / BLOCK_SIZE][j % BLOCK_SIZE] = (0.5 + 0.5 * sum) as f32;
        j += 1;
    }
    g
};

/// The duck's gain on its way up, `1 − DUCK_DOWN`.
static DUCK_UP: [[f32; BLOCK_SIZE]; 2] = {
    let mut g = DUCK_DOWN;
    let mut j = 0;
    while j < DUCK {
        g[j / BLOCK_SIZE][j % BLOCK_SIZE] = 1.0 - g[j / BLOCK_SIZE][j % BLOCK_SIZE];
        j += 1;
    }
    g
};

/// `clampf` of a stored value: VMAXNM then VMINNM on the M7 (the compiler
/// quiets a load's possible signalling NaN with selects first, three times
/// the work). A quiet NaN lands on `lo`; a signalling one (never an
/// arithmetic result) on `hi`, either way.
#[inline(always)]
fn held(v: f32, lo: f32, hi: f32) -> f32 {
    #[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
    {
        let r: f32;
        // SAFETY: two register-to-register VFP ops (FPv5's VMAXNM and
        // VMINNM); no memory or stack touched. `r` is an early output, so
        // it never shares `hi`'s register. They may set FPSCR's IOC on a
        // signalling NaN; nothing reads it.
        unsafe {
            core::arch::asm!(
                "vmaxnm.f32 {r}, {v}, {lo}",
                "vminnm.f32 {r}, {r}, {hi}",
                r = out(sreg) r,
                v = in(sreg) v,
                lo = in(sreg) lo,
                hi = in(sreg) hi,
                options(pure, nomem, nostack),
            );
        }
        r
    }
    #[cfg(not(all(target_arch = "arm", target_feature = "vfp2")))]
    {
        // VMAXNM passes a quiet NaN's other operand, a signalling NaN's NaN
        if v.is_nan() && v.to_bits() & 0x0040_0000 == 0 {
            hi
        } else {
            clampf(v, lo, hi)
        }
    }
}

/// `Lanes::norm` for SUB's and NOISE's blocks `(from, to)`: the two
/// sources beside the operator, level-stable. NOISE at 0 adds `+ 0.0` to
/// `1 + s²`, the same bits.
fn norm((s0, s1): (f32, f32), (z0, z1): (f32, f32)) -> (f32, f32, f32) {
    let n = |s: f32, z: f32| VA_CHAIN_GAIN / sqrt(1.0 + s * s + z * z);
    let h = (BLOCK_SIZE / 2) as f32;
    let (n0, n1, n2) = (n(s0, z0), n(0.5 * (s0 + s1), 0.5 * (z0 + z1)), n(s1, z1));
    // n(j) = n0 + a·j + b·j², j = 0 the sample before, through j = h and 2h
    let b = (n2 - 2.0 * n1 + n0) / (2.0 * h * h);
    let a = (n1 - n0) / h - b * h;
    (n0 + a + b, a + 3.0 * b, 2.0 * b)
}

/// What a voice's VA keeps across the idle slot's rebuild (spec
/// § Stability, free-run): both phases, the sub's level and whether it
/// flipped this cycle, the walks and the seeded character, and whose seed
/// it is.
#[derive(Clone, Copy)]
pub(crate) struct VaCarry {
    phase: [f32; 2],
    high: bool,
    flipped: bool,
    stab: Stability,
    seeded: Option<PartId>,
}

/// One block's lanes: each a `(from, step)` ramp a sample (`ease::at`).
#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct Lanes {
    osc: [Ramps; 2],
    amount: (f32, f32),
    sub: (f32, f32),
    k: (f32, f32),
    /// `VA_CHAIN_GAIN / √(1 + SUB² + NOISE²)` along their ramps: a quadratic through
    /// the block's ends and middle, its value at sample 0, its step and its
    /// step's step (within 10⁻⁷ of exact on a knob's sweep).
    norm: (f32, f32, f32),
    pk: Peaks,
    d: Depth,
    /// NOISE's RMS (a share of R) along its ramp; `(0, 0)`: off.
    nz: (f32, f32),
}

/// Two variable-wave oscillators, combined by MODE, with a sub under OSC 1
/// and STABILITY's drift (spec § Combine, § Stability).
pub struct VaEngine {
    osc: [VarOsc; 2],
    sub: Sub,
    stab: Stability,
    crusher: Crusher,
    /// Each oscillator's cycle factor from its last wrap's jitter.
    jit: [WrapJitter; 2],
    /// Scratch, written before it is read every block: OSC 2's block and
    /// OSC 1's marks, (SYNC) its wraps and its sub flips (kept here so no
    /// block zeroes them).
    osc2: [f32; BLOCK_SIZE],
    marks: [[f32; 2]; BLOCK_SIZE],
    inc: [Ramp; 2],
    wave: [Ramp; 2],
    shape: [Ramp; 2],
    amount: Ramp,
    sub_level: Ramp,
    ring_k: Ramp,
    /// FM's index (`fm_index`), PHASE's depth (`phase_depth`): one plays.
    fm: Ramp,
    /// LOOPFM's `b0` for each oscillator.
    lb: [Ramp; 2],
    /// AM's level make-up (`am_gain`).
    am_n: Ramp,
    /// NOISE: the voice's source and its level's ramp.
    noise: Noise,
    nz: Ramp,
    /// MAX's corrector.
    max: MaxKink,
    /// AM's product corrector (#660).
    amk: AmKinks,
    /// PHASE's offset (cycles) at the last sample, `D·b/pk`; NaN: none yet.
    pm_prev: f32,
    /// OSC 2's steps at the last block's last sample (`door::osc2_steps`'
    /// mark): a jump in the next block's first.
    pm_step: [f32; 2],
    note: u8,
    pitch: f32,
    slide: Glide,
    sample_rate: u32,
    voice: VoiceIdx,
    /// The Part whose seed `stab` holds.
    seeded: Option<PartId>,
    active: bool,
    playing: VaMode,
    duck: Duck,
    #[cfg(any(test, feature = "test-support"))]
    last_k: [f32; 2],
    #[cfg(any(test, feature = "test-support"))]
    backwards: bool,
    #[cfg(any(test, feature = "test-support"))]
    swap_dirs: [f32; 2],
    /// What each plain oscillator played in the last block, `None` where
    /// the plain renders didn't run (`oscs`).
    #[cfg(any(test, feature = "test-support"))]
    played: [Option<Played>; 2],
}

#[cfg(not(any(test, feature = "test-support")))]
const _: () = assert!(size_of::<VaEngine>() <= size_of::<crate::dsp::modal::ModalEngine>());

impl VaEngine {
    fn idle() -> Self {
        Self {
            osc: [VarOsc::NEW; 2],
            sub: Sub::NEW,
            stab: Stability::NEW,
            crusher: Crusher::NEW,
            jit: [WrapJitter::NONE; 2],
            osc2: [0.0; BLOCK_SIZE],
            marks: [[NO_WRAP; 2]; BLOCK_SIZE],
            inc: [Ramp::default(); 2],
            wave: [Ramp::default(); 2],
            shape: [Ramp::default(); 2],
            amount: Ramp::default(),
            sub_level: Ramp::default(),
            ring_k: Ramp::default(),
            fm: Ramp::default(),
            lb: [Ramp::default(); 2],
            am_n: Ramp::default(),
            noise: Noise::new(1),
            nz: Ramp::default(),
            max: MaxKink::NEW,
            amk: AmKinks::NEW,
            pm_prev: 0.0,
            pm_step: [0.0; 2],
            note: 60,
            pitch: 0.0,
            slide: Glide::new(1.0, 1.0),
            sample_rate: 0,
            voice: VoiceIdx::ALL[0],
            seeded: None,
            active: false,
            playing: VaMode::Mix,
            duck: Duck::Steady,
            #[cfg(any(test, feature = "test-support"))]
            last_k: [0.0; 2],
            #[cfg(any(test, feature = "test-support"))]
            backwards: false,
            #[cfg(any(test, feature = "test-support"))]
            swap_dirs: [1.0; 2],
            #[cfg(any(test, feature = "test-support"))]
            played: [None; 2],
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(Self::idle())
    }

    /// The voice slot whose seed this engine draws.
    pub fn place(&mut self, voice: VoiceIdx) {
        self.voice = voice;
        // each voice its own stream
        self.noise = Noise::new(0x9e37_79b9u32.wrapping_mul(voice.index() as u32 + 1));
    }

    pub(crate) fn carry(&self) -> VaCarry {
        VaCarry {
            phase: [self.osc[0].phase(), self.osc[1].phase()],
            high: self.sub.high(),
            flipped: self.osc[0].flipped(),
            stab: self.stab,
            seeded: self.seeded,
        }
    }

    /// A fresh engine where the voice's last one left off: the phases run
    /// on from there at the next note-on, below STABILITY 1.
    pub(crate) fn restore(&mut self, c: VaCarry) {
        for (o, p) in self.osc.iter_mut().zip(c.phase) {
            o.set_phase(p);
        }
        self.sub.set_high(c.high);
        self.osc[0].set_flipped(c.flipped);
        (self.stab, self.seeded) = (c.stab, c.seeded);
    }

    /// The oscillators' and the walks' rates.
    fn prepare(&mut self, sample_rate: u32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            for o in &mut self.osc {
                o.prepare(sample_rate as f32);
            }
            self.stab.prepare(sample_rate as f32 / BLOCK_SIZE as f32);
        }
    }

    /// The voice's PITCH and FINE with their routes, in cents.
    pub fn set_pitch(&mut self, cents: f32) {
        self.pitch = cents;
    }

    /// At STABILITY 1 an idle voice starts both oscillators and the sub at
    /// phase 0; otherwise, and on a sounding voice, the phases run on.
    pub fn note_on(&mut self, note: u8, p: &VaParams, part: PartId, sample_rate: u32) {
        self.prepare(sample_rate);
        if !self.active {
            if p.stability >= 1.0 {
                for o in &mut self.osc {
                    o.reset();
                }
                self.sub.reset();
                self.jit = [WrapJitter::NONE; 2];
            }
            // the corrector's memory goes with the voice, at any STABILITY
            self.max.reset();
            self.amk.reset();
            (self.playing, self.duck) = (p.mode, Duck::Steady);
            // a note starts PHASE's offset from 0, so it ramps in; a MODE
            // switch (NaN) starts it where it already is
            self.pm_prev = 0.0;
            self.pm_step = [0.0; 2];
        }
        if self.seeded != Some(part) {
            self.stab
                .seed(Seed::of(part.index() as u32, self.voice.index() as u32));
            self.seeded = Some(part);
        }
        self.note = note;
        self.slide = Glide::new(1.0, 1.0);
        self.active = true;
    }

    /// A steal's glide (ADR 0065): sounding, the phases play on and the
    /// pitch glides to `note`'s; idle, a plain `note_on`.
    pub fn glide_on(
        &mut self,
        note: u8,
        p: &VaParams,
        part: PartId,
        (sample_rate, tau): (u32, f32),
    ) {
        if self.active {
            self.glide_to(note, (sample_rate, tau));
        } else {
            self.note_on(note, p, part, sample_rate);
        }
    }

    /// The pitch glides to `note`'s from where it sounds, a one-pole of
    /// `tau` seconds in log pitch. Idle, or on `note`, nothing moves.
    pub fn glide_to(&mut self, note: u8, (_sample_rate, tau): (u32, f32)) {
        if !self.active || note == self.note {
            return;
        }
        let from = exp2((self.note as f32 - note as f32) / 12.0) * self.slide.value();
        self.slide = Glide::new(from, tau);
        self.slide.toward(1.0);
        self.note = note;
    }

    /// Nothing: the VCA ends the note, and the oscillators run on.
    pub fn note_off(&mut self) {}

    /// A fresh note's ramps start on their values.
    pub fn hold(&mut self) {
        for r in self
            .inc
            .iter_mut()
            .chain(&mut self.wave)
            .chain(&mut self.shape)
        {
            r.hold();
        }
        self.amount.hold();
        self.sub_level.hold();
        self.ring_k.hold();
        self.fm.hold();
        for r in &mut self.lb {
            r.hold();
        }
        self.am_n.hold();
        self.nz.hold();
        for o in &mut self.osc {
            o.hold();
        }
    }

    /// From the first note-on on: VA never decays.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The MODE playing: a new one takes over halfway through its duck.
    pub fn mode(&self) -> VaMode {
        self.playing
    }

    pub const fn mode_cost(m: VaMode) -> Cost {
        match m {
            VaMode::Mix => COST_MIX,
            VaMode::Sync => COST_SYNC,
            VaMode::Ring => COST_RING,
            VaMode::Xor => COST_XOR,
            VaMode::Bits => COST_BITS,
            VaMode::Fm => COST_FM,
            VaMode::LoopFm => COST_LOOPFM,
            VaMode::Am => COST_AM,
            VaMode::Max => COST_MAX,
            VaMode::SoftSync => COST_SOFTSYNC,
            VaMode::Phase => COST_PHASE,
        }
    }

    pub fn cost(p: &VaParams) -> Cost {
        Self::mode_cost(p.mode)
    }

    /// NOISE's pass a sample, per KIND, when it plays (ADR 0197).
    pub const fn noise_cost(k: NoiseKind) -> Cost {
        match k {
            NoiseKind::White => COST_NOISE_WHITE,
            NoiseKind::Pink => COST_NOISE_PINK,
            NoiseKind::Red => COST_NOISE_RED,
            NoiseKind::Digital => COST_NOISE_DIGITAL,
        }
    }

    /// The playing MODE's bill, while a note sounds.
    pub fn playing_cost(&self) -> Option<Cost> {
        self.active.then(|| Self::mode_cost(self.playing))
    }

    /// One block. Never inlined: its block work is counted on its own.
    #[inline(never)]
    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], p: &VaParams, sample_rate: u32) {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.played = [None; 2];
        }
        if !self.active {
            out.fill(0.0);
            return;
        }
        self.prepare(sample_rate);
        let unit = |v: f32| held(v, 0.0, 1.0);
        let d = depth(held(p.stability, 0.0, 1.0));
        self.slide.tick();
        let drift = self.stab.step(d);
        let base = self.note as f32 * 100.0 + self.pitch;
        let mut cents = [0.0f32; 2];
        for (k, c) in cents.iter_mut().enumerate() {
            *c = base
                + f32::from(p.coarse[k].clamp(-24, 24)) * 100.0
                + held(p.fine[k], -100.0, 100.0)
                + drift[k];
        }
        let scale = self.stab.scale_cents(cents, d);
        let slide = self.slide.value();
        let hz = [0, 1].map(|k| 440.0 * exp2((cents[k] + scale[k] - 6900.0) / 1200.0) * slide);
        let hz = self.stab.tracked(hz, d);

        let lane = |r: &mut Ramp, v: f32| {
            let ft = r.step(v);
            (ft, (ft.0, step_of(ft, BLOCK_SIZE)))
        };
        let (sub_ft, sub) = lane(&mut self.sub_level, unit(p.sub));
        let (nz_ft, nz) = lane(&mut self.nz, noise::level(unit(p.noise), p.noise_kind));
        let mut l = Lanes {
            osc: [Ramps::default(); 2],
            amount: lane(&mut self.amount, unit(p.amount)).1,
            sub,
            k: (0.0, 0.0),
            norm: norm(sub_ft, nz_ft),
            pk: Peaks { a: 0.0, b: 0.0 },
            d,
            nz,
        };
        let mut pk = [0.0f32; 2];
        // SYNC's and SOFT SYNC's master stops at 7 kHz: while either plays
        // or is coming (the duck down), so the cap ramps in and out under
        // the duck (ADR 0161: SOFT SYNC's turns bill at it)
        let synced = |m| matches!(m, VaMode::Sync | VaMode::SoftSync);
        let sync = synced(p.mode) || synced(self.playing);
        let hz = [
            if sync && hz[0] > SYNC_MASTER_HZ {
                SYNC_MASTER_HZ
            } else {
                hz[0]
            },
            hz[1],
        ];
        // each oscillator's lanes (a macro: two straight copies, so the
        // count bills both; a loop here stays rolled)
        macro_rules! lanes {
            ($k:expr) => {{
                let k = $k;
                let (_, li) = lane(&mut self.inc[k], self.osc[k].inc_for(hz[k]));
                // one WAVE segment edge a block at most, the rest carried on
                let w = unit(p.wave[k]);
                let w = wave_reach(self.wave[k].value().unwrap_or(w), w);
                let (_, lw) = lane(&mut self.wave[k], w);
                let (_, ls) = lane(&mut self.shape[k], unit(p.shape[k]));
                l.osc[k] = Ramps {
                    inc: li,
                    wave: lw,
                    shape: ls,
                };
                pk[k] = self.osc[k].begin(&l.osc[k], BLOCK_SIZE);
            }};
        }
        lanes!(0);
        lanes!(1);
        l.pk = Peaks { a: pk[0], b: pk[1] };
        // in SYNC the sub flips with the louder edge, AMOUNT mid-block: the
        // master's shaped edge, or the slave's reset at the wrap (ADR 0118
        // point 13). A place between the two notches both (#485)
        let slave = l.amount.0 + l.amount.1 * (BLOCK_SIZE / 2) as f32 >= 0.5;
        self.osc[0].set_sub_edge(if self.playing == VaMode::Sync && slave {
            SubEdge::Wrap
        } else {
            SubEdge::Saw
        });
        let k = self.ring_k.step(ring_gain(l.pk));
        l.k = (k.0, step_of(k, BLOCK_SIZE));
        // AM's make-up rides in the same lane, ramped per block
        if self.playing == VaMode::Am {
            let n = self.am_n.step(am_gain(unit(p.amount), l.pk));
            l.k = (n.0, step_of(n, BLOCK_SIZE));
            // the operator reads AM's reach: BLEND's ramp mapped at its ends,
            // straight between them (continuous through the knee)
            let (x0, dx) = l.amount;
            let u = (am_reach(x0), am_reach(x0 + dx * BLOCK_SIZE as f32));
            l.amount = (u.0, step_of(u, BLOCK_SIZE));
        }
        #[cfg(any(test, feature = "test-support"))]
        {
            self.last_k = [k.0, k.1];
        }

        let duck = match self.duck {
            Duck::Steady if p.mode != self.playing => Duck::Down(0),
            d => d,
        };
        // the duck's gain this block, applied in the operator's loop
        let gain = match duck {
            Duck::Steady => None,
            Duck::Down(b) => Some(&DUCK_DOWN[usize::from(b & 1)]),
            Duck::Up(b) => Some(&DUCK_UP[usize::from(b & 1)]),
        };
        match self.playing {
            VaMode::Mix => self.render_mix(out, &l, gain),
            VaMode::Sync => self.render_sync(out, &l, gain),
            VaMode::SoftSync => self.render_soft_sync(out, &l, gain),
            VaMode::Ring => self.render_ring(out, &l, gain),
            VaMode::Xor => self.render_xor(out, &l, gain),
            VaMode::Bits => {
                self.crusher.set(unit(p.amount));
                self.render_bits(out, &l, gain)
            }
            VaMode::Am => self.render_am(out, &l, gain),
            VaMode::Max => self.render_max(out, &l, gain),
            VaMode::Fm => {
                // capped at the block's faster end; a pitch jump past the
                // last block's cap lands on it
                let (i0, di) = l.osc[0].inc;
                let inc = i0.max(i0 + di * BLOCK_SIZE as f32);
                let (from, to) = self.fm.step(fm_index(unit(p.amount), inc));
                self.render_fm(out, &l, gain, (from.min(fm_cap(inc)), to))
            }
            VaMode::LoopFm => {
                let (x, wave) = (unit(p.amount), [unit(p.wave[0]), unit(p.wave[1])]);
                // bank 2's with `bank2` (ADR 0163); closed, the voice is
                // silent and the duck runs on
                #[cfg(feature = "bank2")]
                if crate::bank2::DOOR_CELL
                    .with(|d| d.va.loopfm(self, out, &l, gain, x, wave))
                    .is_none()
                {
                    out.fill(0.0);
                }
                #[cfg(not(feature = "bank2"))]
                self.render_loopfm(out, &l, gain, x, wave);
            }
            VaMode::Phase => {
                let d = self.fm.step(phase_depth(unit(p.amount)));
                self.render_phase(out, &l, gain, d)
            }
        }
        // NOISE beside the operator; at 0 both ends, not a bit moves
        if nz_ft.0 > 0.0 || nz_ft.1 > 0.0 {
            let (i0, di) = l.osc[0].inc;
            let inc = i0 + di * (BLOCK_SIZE / 2) as f32;
            self.noise.add(out, p.noise_kind, l.nz, l.norm, inc);
        }
        self.duck = match duck {
            Duck::Steady => Duck::Steady,
            Duck::Down(0) => Duck::Down(1),
            Duck::Down(_) => {
                self.playing = p.mode;
                // MAX and AM's corrector start from nothing under the duck
                self.max.reset();
                self.amk.reset();
                // FM's index and PHASE's depth start on their value; PHASE's
                // offset starts at the first sample's (the read is the clock
                // after `settle`, and an offset's jump would click)
                self.fm.hold();
                for r in &mut self.lb {
                    r.hold();
                }
                self.am_n.hold();
                #[cfg(any(test, feature = "test-support"))]
                {
                    self.swap_dirs = self.osc.each_ref().map(|o| o.read_dir());
                }
                self.pm_prev = f32::NAN;
                self.pm_step = [0.0; 2];
                // a MOD MODE's read phase becomes the clock (ADR 0158)
                for o in &mut self.osc {
                    o.settle();
                }
                Duck::Up(0)
            }
            Duck::Up(0) => Duck::Up(1),
            Duck::Up(_) => Duck::Steady,
        };
    }

    #[inline(never)]
    fn render_mix(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        self.run(out, l, PlainMode::Mix, gain);
    }

    #[inline(never)]
    fn render_sync(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        self.run(out, l, PlainMode::Sync, gain);
    }

    /// AM: bank 2's, its product's steps and slope kinks corrected
    /// (`render_am_kinks`, #660); closed, the uncorrected product.
    #[inline(never)]
    fn render_am(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        #[cfg(feature = "bank2")]
        if osc::solo::through_am(|t| t.am(self, out, l, gain)).is_none() {
            self.run(out, l, PlainMode::Am, gain);
        }
        #[cfg(not(feature = "bank2"))]
        self.render_am_kinks(out, l, gain);
    }

    /// AM with its product corrected (#660, ADR 0321): both oscillators
    /// leave their steps naive and list them (OSC 2 first, so OSC 1's own
    /// marks, its wraps and sub flips, are the block's), and the operator's
    /// corrector (`AmKinks`) steps and bends the product at each, one
    /// sample late. Bank 2's (`*8VaEngine15render_am_kinks*`), reached
    /// through the solos' table (`osc::solo::through_am`).
    #[inline(never)]
    pub fn render_am_kinks(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            osc2: b,
            marks: mk,
            ..
        } = self;
        let js = JitterScale::new(l.d);
        osc::door::osc2_steps(
            o2,
            b,
            mk,
            &l.osc[1],
            &mut jit[1],
            stab,
            js,
            &ZEROS,
            (0.0, 0.0),
        );
        osc::am::osc1_steps(o1, out, mk, &l.osc[0], &mut jit[0], stab, js);
        #[cfg(any(test, feature = "test-support"))]
        {
            self.played = [Some(Played::Pair); 2];
        }
        match gain {
            Some(g) => self.operator_in::<true, true>(out, l, VaMode::Am, g),
            None => self.operator_in::<false, true>(out, l, VaMode::Am, &DUCK_UP[1]),
        }
    }

    #[inline(never)]
    fn render_soft_sync(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            osc2: b,
            marks: mk,
            ..
        } = self;
        let js = JitterScale::new(l.d);
        // OSC 1 the shared instance, its wraps marked: OSC 2's turns
        o1.render::<BLOCK_SIZE, true, false, true, false>(
            out,
            mk,
            &l.osc[0],
            &mut jit[0],
            &mut jitter_draw(stab, JitterScale::new(l.d)),
            &ZEROS,
            (0.0, 0.0),
        );
        // OSC 2's reversing render is bank 2's with `bank2` (ADR 0163).
        // Closed, the voice is silent and OSC 2 waits as it was.
        let mut osc2 = |f: &osc::door::ModEntry| {
            f.render(
                o2,
                b,
                mk,
                &l.osc[1],
                &mut jit[1],
                stab,
                js,
                &ZEROS,
                (0.0, 0.0),
            )
        };
        #[cfg(feature = "bank2")]
        if crate::bank2::DOOR_CELL
            .with(|d| osc2(&d.va.osc2_rev))
            .is_none()
        {
            out.fill(0.0);
            return;
        }
        #[cfg(not(feature = "bank2"))]
        osc2(&osc::door::OSC2_REV);
        match gain {
            Some(g) => self.operator::<true>(out, l, VaMode::SoftSync, g),
            None => self.operator::<false>(out, l, VaMode::SoftSync, &DUCK_UP[1]),
        }
    }

    #[inline(never)]
    fn render_ring(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        self.run(out, l, PlainMode::Ring, gain);
    }

    #[inline(never)]
    fn render_xor(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        self.run(out, l, PlainMode::Xor, gain);
    }

    #[inline(never)]
    fn render_bits(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        self.run(out, l, PlainMode::Bits, gain);
    }

    #[inline(never)]
    fn render_max(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain) {
        self.run(out, l, PlainMode::Max, gain);
    }

    /// FM (#601): OSC 2 plain, at unit peak times the index `ix` (from,
    /// to), drives OSC 1's read rate `inc·(1 + I·b)` through zero; the
    /// operator plays OSC 1 alone, with the sub and the chain gain.
    #[inline(never)]
    fn render_fm(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain, ix: (f32, f32)) {
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            osc2: b,
            marks: mk,
            ..
        } = self;
        let js = JitterScale::new(l.d);
        o2.render::<BLOCK_SIZE, false, false, false, false>(
            b,
            mk,
            &l.osc[1],
            &mut jit[1],
            &mut jitter_draw(stab, JitterScale::new(l.d)),
            &ZEROS,
            (0.0, 0.0),
        );
        let s = 1.0 / l.pk.b;
        let (mut x, dx) = (ix.0 * s, step_of(ix, BLOCK_SIZE) * s);
        for v in b.iter_mut() {
            x += dx;
            *v *= x;
        }
        // OSC 1's MOD render is bank 2's with `bank2` (ADR 0163). Closed,
        // the voice is silent and OSC 1 waits as it was.
        let mut osc1 = |f: &osc::door::ModEntry| {
            f.render(o1, out, mk, &l.osc[0], &mut jit[0], stab, js, b, (1.0, 0.0))
        };
        #[cfg(feature = "bank2")]
        if crate::bank2::DOOR_CELL
            .with(|d| osc1(&d.va.osc1_mod))
            .is_none()
        {
            out.fill(0.0);
            return;
        }
        #[cfg(not(feature = "bank2"))]
        osc1(&osc::door::OSC1_MOD);
        #[cfg(any(test, feature = "test-support"))]
        {
            self.backwards = self.osc2.iter().any(|&v| v < -1.0);
        }
        match gain {
            Some(g) => self.operator::<true>(out, l, VaMode::Fm, g),
            None => self.operator::<false>(out, l, VaMode::Fm, &DUCK_UP[1]),
        }
    }

    /// LOOPFM (#613) at BLEND `x` and each oscillator's WAVE `wave`: each
    /// loops back on its own frequency (`lbfm::render_loop` on its
    /// `VarOsc` phase) with its own `b0`, ramped; the operator mixes OSC 2
    /// in to an equal share.
    #[inline(never)]
    fn render_loopfm(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        l: &Lanes,
        gain: Gain,
        x: f32,
        wave: [f32; 2],
    ) {
        // each |b0| capped at its block's faster end, as FM's index
        let b0 = [0, 1].map(|k| {
            let (i0, di) = l.osc[k].inc;
            let cap = lbfm::b0_cap(i0.max(i0 + di * BLOCK_SIZE as f32));
            let b = lbfm::target_b0(x, wave[k]);
            let (from, to) = self.lb[k].step(b.max(-cap));
            let from = from.max(-cap);
            (from, step_of((from, to), BLOCK_SIZE))
        });
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            osc2: b,
            marks: mk,
            ..
        } = self;
        let js = JitterScale::new(l.d);
        let mut draw = || js.draw(stab);
        let mut p = o1.phase();
        lbfm::render_loop::<BLOCK_SIZE, true>(
            &mut p,
            b0[0],
            l.osc[0].inc,
            out,
            mk,
            &mut jit[0],
            &mut draw,
        );
        o1.set_phase(p);
        let mut p = o2.phase();
        lbfm::render_loop::<BLOCK_SIZE, false>(
            &mut p,
            b0[1],
            l.osc[1].inc,
            b,
            mk,
            &mut jit[1],
            &mut draw,
        );
        o2.set_phase(p);
        match gain {
            Some(g) => self.operator::<true>(out, l, VaMode::LoopFm, g),
            None => self.operator::<false>(out, l, VaMode::LoopFm, &DUCK_UP[1]),
        }
    }

    /// PHASE (#602): OSC 2 plain, at unit peak times the depth `dp` (from,
    /// to) cycles, offsets OSC 1's read phase; the read's rate is the
    /// offset's difference (`k = (0, 1)`), but for OSC 2's steps, each a
    /// jump OSC 1 takes at its mark (ADR 0160). The operator plays OSC 1
    /// alone.
    #[inline(never)]
    fn render_phase(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, gain: Gain, dp: (f32, f32)) {
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            osc2: b,
            marks: mk,
            pm_prev,
            pm_step,
            ..
        } = self;
        let js = JitterScale::new(l.d);
        let s = 1.0 / l.pk.b;
        let (x0, dx) = (dp.0 * s, step_of(dp, BLOCK_SIZE) * s);
        // both oscillators are bank 2's with `bank2` (ADR 0163)
        let mut play = |osc2: &osc::door::ModEntry, osc1: &osc::door::ModEntry| {
            // OSC 2 naive at its steps, each marked for OSC 1's next sample
            osc2.render(
                o2,
                b,
                mk,
                &l.osc[1],
                &mut jit[1],
                stab,
                js,
                &ZEROS,
                (0.0, 0.0),
            );
            // the last sample's steps are the next block's first
            (mk[0], *pm_step) = (*pm_step, mk[0]);
            let (mut x, mut prev) = (x0, *pm_prev);
            if prev.is_nan() {
                prev = (x0 + dx) * b[0];
            }
            for v in b.iter_mut() {
                x += dx;
                let off = x * *v;
                *v = off - prev;
                prev = off;
            }
            *pm_prev = prev;
            // the read's rate is the rest: each marked sample's difference
            // less its jump (slot 0 is the last block's carry)
            if mk[0][0] < osc::JUMP {
                b[0] -= (x0 + dx) * mk[0][1];
            }
            for &k in o2.pm_marks() {
                let k = usize::from(k);
                if (1..BLOCK_SIZE).contains(&k) {
                    b[k] -= (x0 + dx * (k + 1) as f32) * mk[k][1];
                }
            }
            o1.set_pm_scale((x0, dx));
            osc1.render(o1, out, mk, &l.osc[0], &mut jit[0], stab, js, b, (0.0, 1.0));
        };
        // closed, the voice is silent and both oscillators wait as they were
        #[cfg(feature = "bank2")]
        if crate::bank2::DOOR_CELL
            .with(|d| play(&d.va.osc2_steps, &d.va.osc1_phase))
            .is_none()
        {
            out.fill(0.0);
            return;
        }
        #[cfg(not(feature = "bank2"))]
        play(&osc::door::OSC2_STEPS, &osc::door::OSC1_PHASE);
        match gain {
            Some(g) => self.operator::<true>(out, l, VaMode::Phase, g),
            None => self.operator::<false>(out, l, VaMode::Phase, &DUCK_UP[1]),
        }
    }

    /// `run`'s two oscillators, out of line: one copy for every MODE
    /// (flash). OSC 1, and OSC 2 but as SYNC's slave, play the saw alone on
    /// its anchor through bank 2's door (`plain`), else their pairs' runs.
    #[inline(never)]
    fn oscs(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, o2run: Osc2Run) {
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            osc2: b,
            marks: mk,
            ..
        } = self;
        // these MODEs read no modulated phase (FM's and SOFT SYNC's
        // renders are their own, through the door); a wrap's factor,
        // `1 + jitter(d)·CENT`, from the raw draw: exactly 1 at d = 0
        // (STAB 1)
        let (z, nk, js) = (&ZEROS, (0.0, 0.0), JitterScale::new(l.d));
        let (r1, r2) = (&l.osc[0], &l.osc[1]);
        let [j1, j2] = jit;
        // OSC 1 marks its sub flips and its wraps, one instance for every
        // MODE (flash): only SYNC's slave reads the wraps here, resetting
        // at them (ADR 0118 point 13); SOFT SYNC's turns at them (its own
        // render)
        let p1 = plain(
            o1,
            SoloRun::Osc1,
            out,
            mk,
            r1,
            j1,
            stab,
            js,
            |o, out, mk, j, s| {
                let draw = &mut jitter_draw(s, js);
                o.render::<BLOCK_SIZE, true, false, true, false>(out, mk, r1, j, draw, z, nk);
            },
        );
        let p2 = match o2run {
            Osc2Run::Slave => {
                let draw = &mut jitter_draw(stab, js);
                o2.render::<BLOCK_SIZE, false, true, false, false>(b, mk, r2, j2, draw, z, nk);
                Played::Pair
            }
            Osc2Run::Plain => plain(
                o2,
                SoloRun::Osc2,
                b,
                mk,
                r2,
                j2,
                stab,
                js,
                |o, b, mk, j, s| {
                    let draw = &mut jitter_draw(s, js);
                    o.render::<BLOCK_SIZE, false, false, false, false>(b, mk, r2, j, draw, z, nk);
                },
            ),
        };
        #[cfg(any(test, feature = "test-support"))]
        {
            self.played = [Some(p1), Some(p2)];
        }
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = (p1, p2);
    }

    /// One block in `mode`, a constant in each caller, so each mode's loop
    /// is its own (the count sees each). OSC 1 renders into `out` with its
    /// sub flips and its wraps, OSC 2 beside it (synced to the
    /// wraps in SYNC), then the operator, the sub, the chain gain and a
    /// duck's gain run over both.
    #[inline(always)]
    fn run(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, plain: PlainMode, gain: Gain) {
        let mode = plain.mode();
        self.oscs(out, l, plain.osc2());
        // a loop of its own while the duck runs (the count bills it)
        match gain {
            Some(g) => self.operator::<true>(out, l, mode, g),
            None => self.operator::<false>(out, l, mode, &DUCK_UP[1]),
        }
    }

    /// The operator, the sub and the chain gain over both oscillators'
    /// block, times `g` when `DUCKED`.
    #[inline(always)]
    fn operator<const DUCKED: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        l: &Lanes,
        mode: VaMode,
        g: &[f32; BLOCK_SIZE],
    ) {
        self.operator_in::<DUCKED, false>(out, l, mode, g);
    }

    /// `operator`, and with `KINKS` (AM, bank 2's) AM's product corrected
    /// at the oscillators' listed steps (`AmKinks`).
    #[inline(always)]
    fn operator_in<const DUCKED: bool, const KINKS: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        l: &Lanes,
        mode: VaMode,
        g: &[f32; BLOCK_SIZE],
    ) {
        // each oscillator's listed steps, the last block's carry first
        let (sa, sb) = if KINKS {
            let [ca, cb] = &mut self.amk.carry;
            (
                AmKinks::list(ca, self.osc[0].listed_steps(), BLOCK_SIZE),
                AmKinks::list(cb, self.osc[1].listed_steps(), BLOCK_SIZE),
            )
        } else {
            let none = [combine::Step::end(BLOCK_SIZE); combine::STEPS];
            (none, none)
        };
        let (mut ia, mut ib) = (0usize, 0usize);
        let Self {
            sub,
            max,
            amk,
            crusher,
            osc2: b,
            marks: mk,
            ..
        } = self;
        let mut am = *amk;
        // RING and XOR run at twice their level (`Operator`), the chain
        // gain halved: the same bits out
        let op = Operator::new(mode, l.pk, crusher);
        let (up, down) = (op.scale, 1.0 / op.scale);
        let (mut x, dx) = l.amount;
        let (mut k, dk) = (up * l.k.0, up * l.k.1);
        let (s0, ds) = (up * l.sub.0, up * l.sub.1);
        let (mut n, mut dn, ddn) = (down * l.norm.0, down * l.norm.1, down * l.norm.2);
        // SUB's level, a sample's step at a time
        let (mut s, mut sl) = (*sub, s0);
        let mut kink = *max;
        // one sample (a macro: a closure here is left out of line)
        macro_rules! one {
            ($i:expr) => {{
                let i = $i;
                x += dx;
                k += dk;
                sl += ds;
                // MAX and AM's corrector play one sample late, the sub on
                // the clock
                let c = if mode == VaMode::Max {
                    kink.step(out[i], b[i], x)
                } else if KINKS {
                    let at = |l: &combine::Steps, n: &mut usize| {
                        let s = l[*n];
                        (s.slot == i).then(|| {
                            *n += 1;
                            (s.t, s.d)
                        })
                    };
                    let (ea, eb) = (at(&sa, &mut ia), at(&sb, &mut ib));
                    let (c0, c1) = op.am_terms(x);
                    let c = op.at(out[i], b[i], x, k);
                    am.step((out[i], b[i], c), (k, c0, c1), ea, eb)
                } else {
                    op.at(out[i], b[i], x, k)
                };
                let sv = s.step(mk[i][1]);
                let y = (c + sl * sv) * n;
                out[i] = if DUCKED { y * g[i] } else { y };
                n += dn;
                dn += ddn;
            }};
        }
        // four samples a round, unrolled here: XOR's and BITS' FPU
        // instructions (inline asm) keep the compiler from unrolling it
        for i in (0..BLOCK_SIZE).step_by(4) {
            one!(i);
            one!(i + 1);
            one!(i + 2);
            one!(i + 3);
        }
        *sub = s;
        *max = kink;
        *amk = am;
    }

    /// A glide steal's ratio now, 1 at rest.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        self.slide.value()
    }

    /// MAX's kink corrector off, the oscillators corrected.
    #[cfg(any(test, feature = "test-support"))]
    pub fn kink_uncorrected(&mut self) {
        self.max.off = true;
    }

    /// Both oscillators uncorrected (and MAX's and AM's kinks): the alias
    /// gate's baseline.
    #[cfg(any(test, feature = "test-support"))]
    pub fn uncorrected(&mut self) {
        self.max.off = true;
        self.amk.off = true;
        self.osc = self.osc.map(|o| o.with_correction(Correction::Off));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn osc_phase(&self) -> [f32; 2] {
        [self.osc[0].phase(), self.osc[1].phase()]
    }

    /// The seeded character, once a note-on (or a carry) seeded it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn character(&self) -> Option<Character> {
        self.seeded.map(|_| self.stab.character())
    }

    /// Each oscillator's WAVE where the last block's ramp ended (`None`
    /// before a fresh note's first block).
    #[cfg(any(test, feature = "test-support"))]
    pub fn wave_now(&self) -> [Option<f32>; 2] {
        [self.wave[0].value(), self.wave[1].value()]
    }

    /// The runs each oscillator's last block started (two at most).
    #[cfg(any(test, feature = "test-support"))]
    pub fn runs(&self) -> [u8; 2] {
        [self.osc[0].runs(), self.osc[1].runs()]
    }

    /// What each oscillator's plain render played in the last block,
    /// `None` where none ran (a MODE with renders of its own, a closed
    /// voice).
    #[cfg(any(test, feature = "test-support"))]
    pub fn played(&self) -> [Option<Played>; 2] {
        self.played
    }

    /// Whether OSC 1's read ran backwards in the last FM block (its rate
    /// `inc·(1 + I·b)` went negative).
    #[cfg(any(test, feature = "test-support"))]
    pub fn read_ran_backwards(&self) -> bool {
        self.backwards
    }

    /// Both reads' directions at the last MODE swap, before `settle`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn dirs_at_swap(&self) -> [f32; 2] {
        self.swap_dirs
    }

    /// Both clocks jumped to `p`, as a carry restores them.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_osc_phase(&mut self, p: [f32; 2]) {
        for (o, p) in self.osc.iter_mut().zip(p) {
            o.set_phase(p);
        }
    }

    /// The last block's RING gain ramp, from and to.
    #[cfg(any(test, feature = "test-support"))]
    pub fn ring_k(&self) -> [f32; 2] {
        self.last_k
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;

    const SR: u32 = 48_000;

    fn with_engine<R>(f: impl FnOnce(&mut VaEngine) -> R) -> R {
        let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
        f(VaEngine::init_in_place(&mut slot))
    }

    fn run(e: &mut VaEngine, p: &VaParams, blocks: usize) -> Vec<u32> {
        let mut out = [0.0f32; BLOCK_SIZE];
        (0..blocks)
            .flat_map(|_| {
                e.render(&mut out, p, SR);
                out.map(f32::to_bits)
            })
            .collect()
    }

    /// RING of two saws 6 cents apart beats at 0.906 Hz at C4. Its mean over
    /// whole beats is zero (the saws' correlation averages out across the
    /// beat), so the sweep's "DC" on VA INIT is the beat seen through a
    /// window shorter than it, not a DC term in the operator.
    #[test]
    fn ring_has_no_dc_over_whole_beats() {
        for (amount, stability) in [(0.5, 1.0), (1.0, 1.0)] {
            let p = VaParams {
                mode: VaMode::Ring,
                amount,
                stability,
                fine: [0.0, 6.0],
                ..VaParams::default()
            };
            let y = with_engine(|e| {
                e.note_on(60, &p, PartId::new(0).unwrap(), SR);
                run(e, &p, 225); // past the sweep's DC_FROM
                // 4 beats, to a whole block
                let beat = SR as f32 / (261.625_58 * (exp2(6.0 / 1200.0) - 1.0));
                run(e, &p, (4.0 * beat / BLOCK_SIZE as f32) as usize)
            });
            let y: Vec<f32> = y.into_iter().map(f32::from_bits).collect();
            let mean = |y: &[f32]| y.iter().sum::<f32>() / y.len() as f32;
            let rms = (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt();
            assert!(mean(&y).abs() < 0.003 * rms, "{} of {rms}", mean(&y));
        }
    }

    /// The carry is `carry_restores_the_state` (spec § Stability, free-run):
    /// `carry` is the crate's, so its test lives here.
    #[test]
    fn carry_restores_the_state() {
        let p = VaParams {
            stability: 0.5,
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let part = PartId::new(2).unwrap();
        let c = with_engine(|e| {
            e.place(VoiceIdx::ALL[3]);
            e.note_on(69, &p, part, SR);
            run(e, &p, 225);
            let c = e.carry();
            with_engine(|f| {
                f.restore(c);
                assert_eq!(f.osc_phase(), e.osc_phase());
                assert_eq!(f.character(), e.character());
                assert!(f.character().is_some());
                assert!(!f.is_active());
            });
            c
        });
        let next = || {
            with_engine(|e| {
                e.place(VoiceIdx::ALL[3]);
                e.restore(c);
                e.hold();
                e.note_on(64, &p, part, SR);
                assert_ne!(e.osc_phase(), [0.0; 2], "below STAB 1 the phases run on");
                run(e, &p, 150)
            })
        };
        assert_eq!(next(), next());
    }

    /// LOOPFM stores its phase in `VarOsc`: the carry restores it, and
    /// below STAB 1 the next note runs on from it.
    #[test]
    fn loopfm_carry_restores() {
        let p = VaParams {
            mode: VaMode::LoopFm,
            amount: 0.8,
            stability: 0.5,
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let part = PartId::new(2).unwrap();
        let c = with_engine(|e| {
            e.place(VoiceIdx::ALL[3]);
            e.note_on(69, &p, part, SR);
            run(e, &p, 225);
            let c = e.carry();
            with_engine(|f| {
                f.restore(c);
                assert_eq!(f.osc_phase(), e.osc_phase());
                assert_ne!(f.osc_phase(), [0.0; 2]);
            });
            c
        });
        let next = || {
            with_engine(|e| {
                e.place(VoiceIdx::ALL[3]);
                e.restore(c);
                e.hold();
                e.note_on(64, &p, part, SR);
                assert_ne!(e.osc_phase(), [0.0; 2], "below STAB 1 the phases run on");
                run(e, &p, 150)
            })
        };
        let (a, b) = (next(), next());
        assert!(a.iter().all(|v| f32::from_bits(*v).is_finite()));
        assert_eq!(a, b);
    }

    /// The carry holds the walks and the streams themselves: two engines
    /// restoring one carry would agree whatever it held.
    #[test]
    fn the_carry_holds_the_walks_and_streams() {
        let p = VaParams {
            stability: 0.25,
            ..VaParams::default()
        };
        with_engine(|e| {
            e.place(VoiceIdx::ALL[1]);
            e.note_on(69, &p, PartId::new(1).unwrap(), SR);
            run(e, &p, 300);
            with_engine(|f| {
                f.restore(e.carry());
                let (mut a, mut b) = (e.stab, f.stab);
                for _ in 0..64 {
                    let d = Depth::new(0.25);
                    assert_eq!(a.step(d), b.step(d));
                    let js = JitterScale::new(d);
                    assert_eq!(js.draw(&mut a), js.draw(&mut b));
                }
            });
        });
    }

    /// STAB 1 on an idle engine that ran before starts at phase 0: it
    /// renders as an engine that never played.
    #[test]
    fn stab_one_resets_a_restored_engine() {
        let part = PartId::new(0).unwrap();
        let alive = VaParams {
            stability: 0.5,
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let c = with_engine(|e| {
            e.note_on(69, &alive, part, SR);
            run(e, &alive, 77);
            e.carry()
        });
        let steady = VaParams {
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let played = with_engine(|e| {
            e.restore(c);
            assert_ne!(e.osc_phase(), [0.0; 2]);
            e.hold();
            e.note_on(69, &steady, part, SR);
            assert_eq!(e.osc_phase(), [0.0; 2]);
            run(e, &steady, 150)
        });
        let fresh = with_engine(|e| {
            e.hold();
            e.note_on(69, &steady, part, SR);
            run(e, &steady, 150)
        });
        assert_eq!(played, fresh);
    }

    /// Every MODE's output bits, hashed (FNV-1a): two notes, STABILITY's
    /// jitter, SUB, a SHAPE and a MODE change under way. The constant is
    /// main's before NOISE (ADR 0197): LEVEL 0 must leave every bit as it was.
    /// Re-pinned for AM's corrected product (#660, ADR 0321).
    #[test]
    fn noise_off_is_bit_identical() {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for (i, &m) in VaMode::ALL.iter().enumerate() {
            let p = VaParams {
                mode: m,
                amount: 0.7,
                sub: 0.4,
                stability: 0.6,
                wave: [0.5, 0.9],
                shape: [0.3, 0.0],
                fine: [0.0, 7.0],
                coarse: [0, 5],
                ..VaParams::default()
            };
            let next = VaParams {
                mode: VaMode::ALL[(i + 3) % VaMode::ALL.len()],
                ..p
            };
            let bits = with_engine(|e| {
                e.place(VoiceIdx::ALL[1]);
                e.note_on(45, &p, PartId::new(1).unwrap(), SR);
                let mut v = run(e, &p, 120);
                e.note_on(64, &p, PartId::new(1).unwrap(), SR);
                v.extend(run(e, &next, 120));
                v
            });
            for b in bits {
                for byte in b.to_le_bytes() {
                    h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        assert_eq!(h, 0xda44_0441_b15b_6d7a, "hash {h:#018x}");
    }
}
