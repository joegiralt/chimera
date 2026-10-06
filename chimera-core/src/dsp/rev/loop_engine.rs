//! LOOP (reverb TYPEs spec § LOOP engine): HALL's 1 loop × 4 stages,
//! generic in its loops and stages (ROOM v2 has its own engine, `room`),
//! as ADR 0095 § HALL, as built, has it. The send, through TONE and
//! PRE-DLY, crosses six allpasses at program 17's lengths × SPREAD
//! (normalised i16 states, outside the loop), then enters each loop at its
//! own sign. A stage is a delay then an f32 allpass; one damper ends each
//! loop, and the stage gains carry the decay, each re-entry past R1's
//! clip. Eight taps a side, one per stage line in turn, scaled with SIZE,
//! make the return; DENSITY fades the last five in.
//!
//! SIZE scales the stage delays only, by one k for all, SPREAD the
//! diffuser, and both move by pitch-free crossfades: one 5 ms slice a loop
//! and one diffuser allpass a crossfade, PRE-DLY in the same crossfades
//! (§ Moving a length). The taps move with their lines. SMEAR moves two
//! stage allpasses, which never crossfade. The re-entries write by
//! `store_settle`, the diffuser's states by `store_settle_ap`, and the
//! stage delays truncate (a rounding loop at DAMP's 14 kHz held 3 LSB). Laid out for the chip's
//! 24 kHz ring, as BURST is. Values are in LSBs.

// Indexed loops over loops and stages read as the spec's graph does.
#![allow(clippy::needless_range_loop)]

use core::f32::consts::LOG2_10;
#[cfg(any(test, feature = "test-support"))]
use core::ops::Range;

use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::reverb::EngineKnobs;
use crate::dsp::ring::{FULL_SCALE, RING_LEN};

use super::arena::ReverbArena;
pub use super::parts::XFADE;
use super::parts::{
    Lines, XFADE_INV, ap_f32, ap_norm, ease_k, lg2, one_pole, pair_in, pow2, prime_ms, segment,
    wrap,
};

/// `$body` once per literal, `$k` that literal as a `const`: `ring.rs`'s.
macro_rules! unroll {
    ($k:ident in [$($n:literal),*] $body:block) => {
        $({
            const $k: usize = $n;
            $body
        })*
    };
}

/// The ring rate the lengths are laid out at.
const FS: f32 = 24_000.0;
/// Ring samples a millisecond.
pub const PER_MS: f32 = FS / 1000.0;

/// The diffuser's allpasses.
pub const DIFF: usize = 6;
/// Output taps a side.
pub const TAPS: usize = 8;
/// The taps a side DENSITY never fades.
pub const TAPS_FIXED: usize = 3;
/// Which diffuser allpasses keep f32 states: the four shortest (13.7,
/// 18.4, 8.7 and 6.2 ms), whose rounding the loop would hear most; the
/// two longest store normalised i16 (R2), as the arena has no room for
/// more f32 (ADR 0095 § HALL, as built).
pub const DIF_F32: [bool; DIFF] = [true, true, false, false, true, true];
/// The f32 and the i16 diffuser allpasses, in signal order.
const NF: usize = 4;
const NI: usize = DIFF - NF;
const DIF_F: [usize; NF] = [0, 1, 4, 5];
const DIF_I: [usize; NI] = [2, 3];
const _: () = {
    let mut j = 0;
    while j < NF {
        assert!(DIF_F32[DIF_F[j]]);
        j += 1;
    }
    j = 0;
    while j < NI {
        assert!(!DIF_F32[DIF_I[j]]);
        j += 1;
    }
};
/// Taps a crossfade can move: those on the one stage line a loop moves.
const TC: usize = 4;
/// The bus's half-bands delay the return 34 samples at 48 kHz, 17 at the
/// ring: every tap reads that much nearer its write, so the return's
/// first arrival is where the line puts it (ADR 0095 § HALL, as built).
pub const LATENCY: usize = 17;

// ADR 0095's table, ms: program 17's diffuser lengths, read from the
// MidiVerb II (github.com/thement/midiverb_emulator's decompilation,
// GPL-3.0), used by the owner's approval of 2026-10-05. Only this table
// carries them; nothing else here comes from the emulator.

/// HALL's diffuser at SPREAD × 1, in signal order: 321, 431, 668, 808,
/// 204 and 144 samples at the unit's 23.4 kHz.
pub const DIFFUSER_MS: [f32; DIFF] = [13.718, 18.419, 28.547, 34.530, 8.718, 6.154];
// End of ADR 0095's table.

/// SPREAD: the diffuser's scale, × 0.05 at 0 to × 1.10 at 1, `lin`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spread(f32);

impl Spread {
    pub const LO: f32 = 0.05;
    pub const HI: f32 = 1.10;
    pub const TOP: Self = Self(Self::HI);

    /// SPREAD's knob `p` (0..=1; NaN reads 0).
    pub fn from_unit(p: f32) -> Self {
        Self(Self::LO + (Self::HI - Self::LO) * clampf(p, 0.0, 1.0))
    }

    pub const fn scale(self) -> f32 {
        self.0
    }

    /// Allpass `j`'s length at this scale, ring samples (2 at least).
    pub fn len(self, j: usize) -> u16 {
        ((DIFFUSER_MS[j] * PER_MS * self.0 + 0.5) as u16).max(2)
    }
}

/// DENSITY as taps a side, 3 to 8: fractional while a tap fades in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapCount(f32);

impl TapCount {
    pub const MIN: Self = Self(TAPS_FIXED as f32);
    pub const MAX: Self = Self(TAPS as f32);

    /// DENSITY's knob `p` (0..=1; NaN reads 0): each of the five fades in
    /// over a fifth of the knob.
    pub fn from_unit(p: f32) -> Self {
        Self(Self::MIN.0 + (Self::MAX.0 - Self::MIN.0) * clampf(p, 0.0, 1.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    /// The weight of the tap at `rank` (0-based): 1 below the count, the
    /// count's fraction at it, 0 past it.
    pub fn weight(self, rank: usize) -> f32 {
        clampf(self.0 - rank as f32, 0.0, 1.0)
    }
}

impl Default for TapCount {
    fn default() -> Self {
        Self::MIN
    }
}

/// A place along a line, strictly inside it: a tap's offset is this share
/// of the line's length as it plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frac(f32);

impl Frac {
    /// `f`, in (0, 1): checked where the tables are built, at compile time.
    pub const fn new(f: f32) -> Self {
        assert!(f > 0.0 && f < 1.0, "a tap sits inside its line");
        Self(f)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Plus,
    Minus,
}

impl Sign {
    pub const fn f32(self) -> f32 {
        match self {
            Sign::Plus => 1.0,
            Sign::Minus => -1.0,
        }
    }
}

/// An output tap: stage `stage`'s line of loop `lp`, `at` along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    pub lp: usize,
    pub stage: usize,
    pub at: Frac,
    pub sign: Sign,
}

const fn tap(stage: usize, at: f32) -> Tap {
    // A line after an odd number of stage allpasses carries their sign:
    // flipping it keeps every tap in phase with the model the fits ran on.
    Tap {
        lp: 0,
        stage,
        at: Frac::new(at),
        sign: if stage.is_multiple_of(2) {
            Sign::Plus
        } else {
            Sign::Minus
        },
    }
}

/// A LOOP TYPE's lines at their longest, ring samples: the stage delays at
/// SIZE 1, the diffuser at SPREAD's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens<const L: usize, const S: usize> {
    /// PRE-DLY's longest.
    pub pre: usize,
    /// The diffuser's allpasses (`DIF_F32`'s f32, the rest normalised i16).
    pub diffuser: [usize; DIFF],
    /// Each loop's stage delays, then its stage allpasses.
    pub delay: [[usize; S]; L],
    pub ap: [[usize; S]; L],
}

/// Where each line writes, from the shared heads: line j owns its length
/// and 2 slots more (a pair's second read, and the guard), so a read d
/// back, 2 ≤ d ≤ len + 1, is what it wrote d samples ago. The i16 lines
/// (PRE, the i16 diffuser allpasses, the delays), then the f32 ones (the
/// f32 diffuser allpasses, the stage allpasses).
#[derive(Clone, Copy, Debug)]
pub struct Heads<const L: usize, const S: usize> {
    pre: usize,
    diffuser: [usize; DIFF],
    delay: [[usize; S]; L],
    ap: [[usize; S]; L],
    /// The i16 slots, even: the f32 region follows.
    slots: usize,
    fslots: usize,
}

const fn place<const L: usize, const S: usize>(n: &Lens<L, S>) -> Heads<L, S> {
    let mut o = n.pre + 2;
    let pre = o - 1;
    let mut diffuser = [0; DIFF];
    let mut j = 0;
    while j < NI {
        o += n.diffuser[DIF_I[j]] + 2;
        diffuser[DIF_I[j]] = o - 1;
        j += 1;
    }
    let (mut delay, mut ap) = ([[0; S]; L], [[0; S]; L]);
    let mut l = 0;
    while l < L {
        let mut s = 0;
        while s < S {
            o += n.delay[l][s] + 2;
            delay[l][s] = o - 1;
            s += 1;
        }
        l += 1;
    }
    let slots = o + o % 2;
    let mut f = 0;
    j = 0;
    while j < NF {
        f += n.diffuser[DIF_F[j]] + 2;
        diffuser[DIF_F[j]] = f - 1;
        j += 1;
    }
    l = 0;
    while l < L {
        let mut s = 0;
        while s < S {
            f += n.ap[l][s] + 2;
            ap[l][s] = f - 1;
            s += 1;
        }
        l += 1;
    }
    Heads {
        pre,
        diffuser,
        delay,
        ap,
        slots,
        fslots: f,
    }
}

/// A LOOP TYPE's graph: its lines, taps, smeared allpasses and send.
pub trait Layout<const L: usize, const S: usize> {
    const LENS: Lens<L, S>;
    /// Each side's taps, by rank: the first `TAPS_FIXED` always play,
    /// DENSITY fades the rest in, in order.
    const TAPS: [[Tap; TAPS]; 2];
    /// The two smeared stage allpasses, (loop, stage).
    const SMEARED: [(usize, usize); 2];
    /// The send into each loop's first delay.
    const INJECT: [f32; L];
    /// SMEAR's cap, a share of the loop (§ The SMEAR bound).
    const SMEAR_CAP: f32;

    const HEADS: Heads<L, S> = place(&Self::LENS);
    /// The i16 lines' slots, and the f32 lines' f32s above them.
    const SLOTS: usize = Self::HEADS.slots;
    const FSLOTS: usize = Self::HEADS.fslots;
    /// The longest stage allpass, ring samples.
    const AP_MAX: f32 = {
        let (mut m, mut l) = (0, 0);
        while l < L {
            let mut s = 0;
            while s < S {
                if Self::LENS.ap[l][s] > m {
                    m = Self::LENS.ap[l][s];
                }
                s += 1;
            }
            l += 1;
        }
        m as f32
    };
    /// Loop 0's stage allpasses, and 1/its stage delays at SIZE 1: SIZE's
    /// k from its round trip.
    const AP0: f32 = {
        let (mut a, mut s) = (0, 0);
        while s < S {
            a += Self::LENS.ap[0][s];
            s += 1;
        }
        a as f32
    };
    const INV_D0: f32 = {
        let (mut d, mut s) = (0, 0);
        while s < S {
            d += Self::LENS.delay[0][s];
            s += 1;
        }
        1.0 / d as f32
    };
    /// Every i16 access's offset from the head that no length moves: the
    /// writes (`Access`). The reads are filled in each block.
    const OFFSETS: [usize; MAX_ACCESS] = fixed_offsets(&Self::HEADS);
    /// The f32 accesses (`FAccess`): the f32 diffuser allpasses' writes,
    /// the stage allpasses' writes and fixed reads; the diffuser's reads
    /// are filled in each block.
    const F_OFFSETS: [usize; MAX_F_ACCESS] = {
        let mut o = [0; MAX_F_ACCESS];
        let mut j = 0;
        while j < NF {
            o[FAccess::<L, S>::W_DF + j] = Self::HEADS.diffuser[DIF_F[j]];
            j += 1;
        }
        let mut l = 0;
        while l < L {
            let mut s = 0;
            while s < S {
                let j = l * S + s;
                o[FAccess::<L, S>::W_AP + j] = Self::HEADS.ap[l][s];
                o[FAccess::<L, S>::R_AP + j] = Self::HEADS.ap[l][s] - Self::LENS.ap[l][s];
                s += 1;
            }
            l += 1;
        }
        o
    };
    /// The i16 and f32 accesses, every one in its segment.
    const ACCESS: usize = 3 + 3 * NI + 3 * L * S + 2 * TAPS + 2 * TC;
    const F_ACCESS: usize = 3 * NF + 2 * L * S;
    /// The smeared reads, `M − 2D` at the deepest D (twice the cap of the
    /// longest loop), stay 2 or more behind their write: `pair_in`'s
    /// contract.
    const SMEAR_FITS: bool = {
        let (mut m, mut ok) = (0, true);
        while m < 2 {
            let (l, st) = Self::SMEARED[m];
            let (mut trip, mut s) = (0, 0);
            while s < S {
                trip += Self::LENS.delay[l][s] + Self::LENS.ap[l][s];
                s += 1;
            }
            let d = Self::LENS.ap[l][st] as f32 - 2.0 * Self::SMEAR_CAP * trip as f32;
            ok &= d >= 2.0 && Self::LENS.ap[l][st] < Self::FSLOTS;
            m += 1;
        }
        ok
    };
    /// Every tap reads a stage line that exists, and the taps one
    /// crossfade moves (one stage line a loop, both sides) fit `TC`.
    const TAPS_FIT: bool = {
        let mut ok = true;
        let mut j = 0;
        while j < 2 * TAPS {
            let t = Self::TAPS[j / TAPS][j % TAPS];
            ok &= t.lp < L && t.stage < S;
            j += 1;
        }
        let mut line = 0;
        while line < L * S {
            let (mut n, mut j) = (0, 0);
            while j < 2 * TAPS {
                let t = Self::TAPS[j / TAPS][j % TAPS];
                n += (t.lp * S + t.stage == line) as usize;
                j += 1;
            }
            ok &= L * n <= TC;
            line += 1;
        }
        ok
    };
    const FITS: () = assert!(
        Self::SLOTS + 2 * Self::FSLOTS <= RING_LEN
            && Self::SLOTS < u16::MAX as usize
            && Self::FSLOTS < u16::MAX as usize
            && Self::ACCESS == MAX_ACCESS
            && Self::F_ACCESS == MAX_F_ACCESS
            && Self::SMEAR_FITS
            && Self::TAPS_FIT
    );

    /// The i16 regions a level-plan test reads, from the head: PRE, then
    /// the diffuser and the stage delays. A head's own slot is the next
    /// written, so it still holds what the line above wrote a lap ago:
    /// PRE's is left out.
    #[cfg(any(test, feature = "test-support"))]
    fn regions() -> [Range<usize>; 2] {
        let h = Self::HEADS;
        [0..h.pre, h.pre + 1..h.delay[L - 1][S - 1]]
    }
}

/// HALL's i16 and f32 accesses: 4 stages in all.
const MAX_ACCESS: usize = 45;
const MAX_F_ACCESS: usize = 20;

/// `Layout::OFFSETS`: the writes at their heads; the rest 0.
const fn fixed_offsets<const L: usize, const S: usize>(h: &Heads<L, S>) -> [usize; MAX_ACCESS] {
    let mut o = [0; MAX_ACCESS];
    o[Access::<L, S>::W_PRE] = h.pre;
    let mut j = 0;
    while j < NI {
        o[Access::<L, S>::W_DIF + j] = h.diffuser[DIF_I[j]];
        j += 1;
    }
    let mut l = 0;
    while l < L {
        let mut s = 0;
        while s < S {
            o[Access::<L, S>::W_D + l * S + s] = h.delay[l][s];
            s += 1;
        }
        l += 1;
    }
    o
}

/// HALL: one loop, four stages (§ LOOP engine's table), SIZE's k from
/// 0.111 to 1 (the round trip 120 → 600 ms); its plate zone is in the
/// knobs (`reverb::plate_pull`).
pub type Hall = LoopEngine<1, 4>;

const HALL_LENS: Lens<1, 4> = Lens {
    pre: (125.0 * PER_MS) as usize,
    diffuser: {
        let mut d = [0; DIFF];
        let mut j = 0;
        while j < DIFF {
            d[j] = (DIFFUSER_MS[j] * PER_MS * Spread::HI + 0.5) as usize;
            j += 1;
        }
        d
    },
    delay: [[
        prime_ms(113.0),
        prime_ms(157.0),
        prime_ms(124.0),
        prime_ms(146.0),
    ]],
    ap: [[
        prime_ms(11.0),
        prime_ms(17.0),
        prime_ms(13.0),
        prime_ms(19.0),
    ]],
};

impl Layout<1, 4> for Hall {
    const LENS: Lens<1, 4> = HALL_LENS;
    /// Our own places (ADR 0095 § HALL, as built), searched with the
    /// presets' knobs held: L on S1, S2, S3, S1, S2, S3, S4, S4; R on S2,
    /// S3, S4, S2, S3, S4, S1, S1.
    const TAPS: [[Tap; TAPS]; 2] = [
        [
            tap(0, 0.62),
            tap(1, 0.55),
            tap(2, 0.30),
            tap(0, 0.17),
            tap(1, 0.24),
            tap(2, 0.48),
            tap(3, 0.06),
            tap(3, 0.82),
        ],
        [
            tap(1, 0.02),
            tap(2, 0.90),
            tap(3, 0.93),
            tap(1, 0.48),
            tap(2, 0.57),
            tap(3, 0.26),
            tap(0, 0.43),
            tap(0, 0.60),
        ],
    ];
    /// Stages 1's and 3's.
    const SMEARED: [(usize, usize); 2] = [(0, 0), (0, 2)];
    const INJECT: [f32; 1] = [0.5];
    const SMEAR_CAP: f32 = crate::dsp::reverb::HALL.smear_cap;
}

/// The new engines' stage gain cap (§ Shared machinery).
const MAX_GAIN: f32 = 0.985;
/// A loop's round-trip gain cap: G = 1/(1 − 0.92) = 12.5 on a held loop
/// mode, the level plan's (§ Level plan). It binds only at small SIZE,
/// in the plate zone, where DECAY's 16 s needs more.
const TRIP_MAX: f32 = 0.92;
/// The send's injection, ½ (§ Shared machinery), in LSBs.
const IN_GAIN: f32 = 0.5 * FULL_SCALE;
/// Each tap's weight, before DENSITY's fade.
const TAP_W: f32 = 0.75;
/// The most one crossfade moves PRE-DLY or a loop's round trip.
pub const STEP: u16 = 120;
/// The most one crossfade moves a diffuser allpass: a quarter of `STEP`,
/// as a blend's comb dips over an allpass's whole length.
pub const DIF_STEP: u16 = 16;

/// A LOOP TYPE's state beside the arena. Zero bytes are a fresh engine,
/// primed by its first block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopEngine<const L: usize, const S: usize> {
    /// The shared write heads of the i16 and the f32 lines.
    h: u16,
    hf: u16,
    primed: bool,
    /// Ring samples left in the length crossfade; 0 between.
    left: u16,
    /// The stage delays, the diffuser's allpasses and PRE-DLY, ring
    /// samples: landed, crossfading to, and the knobs' goal.
    at: [[u16; S]; L],
    to: [[u16; S]; L],
    goal: [[u16; S]; L],
    dif_at: [u16; DIFF],
    dif_to: [u16; DIFF],
    dif_goal: [u16; DIFF],
    pre_at: u16,
    pre_to: u16,
    pre_goal: u16,
    tone: f32,
    /// The eased TONE coefficient, the diffuser's and the stage allpasses'
    /// g, the damper's coefficient, DENSITY's taps and the stage gains:
    /// the gains ramp each block from last block's, the rest step.
    ta: f32,
    gd: f32,
    gs: f32,
    da: f32,
    taps: f32,
    g: [[f32; S]; L],
    /// What the eases head for, worked out again only when RT60, TONE,
    /// the damper (`key`'s bits) or the lengths (`retarget`) change: the
    /// coefficients, the stage allpasses' cap and the stage gains.
    key: [u32; 3],
    retarget: bool,
    t_ta: f32,
    t_da: f32,
    t_cap: f32,
    t_g: [[f32; S]; L],
    /// The dampers, one ending each loop. Unrounded: they are state, not
    /// stored, and rounding them put HALL's floor 1.2 dB up; nor can one
    /// stick at ±1 LSB once its coefficient is under ½.
    lp: [f32; L],
    /// This block's SMEAR offsets (start, end), ring samples: `D·(1 + x)`.
    smear: [(f32, f32); 2],
    /// A crossfade step other than `STEP`, when not 0 (the slap test's
    /// negative control): never in firmware.
    #[cfg(any(test, feature = "test-support"))]
    coarse: u16,
}

impl<const L: usize, const S: usize> Default for LoopEngine<L, S> {
    fn default() -> Self {
        Self {
            h: 0,
            hf: 0,
            primed: false,
            left: 0,
            at: [[0; S]; L],
            to: [[0; S]; L],
            goal: [[0; S]; L],
            dif_at: [0; DIFF],
            dif_to: [0; DIFF],
            dif_goal: [0; DIFF],
            pre_at: 0,
            pre_to: 0,
            pre_goal: 0,
            tone: 0.0,
            ta: 0.0,
            gd: 0.0,
            gs: 0.0,
            da: 0.0,
            taps: 0.0,
            g: [[0.0; S]; L],
            key: [0; 3],
            retarget: false,
            t_ta: 0.0,
            t_da: 0.0,
            t_cap: 0.0,
            t_g: [[0.0; S]; L],
            lp: [0.0; L],
            smear: [(0.0, 0.0); 2],
            #[cfg(any(test, feature = "test-support"))]
            coarse: 0,
        }
    }
}

/// One block's one-pole toward `t`, `k` the share kept (0 lands).
fn ease(x: &mut f32, t: f32, k: f32) {
    *x = t + k * (*x - t);
}

/// R1's clip (`parts::soft_clip`) on an LSB value, its knee and ceiling
/// in LSBs: no scaling in and out.
#[inline(always)]
fn clip<const C: bool>(x: f32) -> f32 {
    if C {
        const INV_FS: f32 = 1.0 / FULL_SCALE;
        let a = x.abs();
        let u = clampf(a - FULL_SCALE, 0.0, 2.0 * FULL_SCALE);
        (a.min(FULL_SCALE) + u - (0.25 * INV_FS) * u * u).copysign(x)
    } else {
        x
    }
}

/// The i16 accesses, as offsets from the shared head: the writes (PRE,
/// the diffuser, the delays), then the moving reads in new/old pairs:
/// PRE's, the diffuser's, the delays' and the taps'. Each per-line group
/// is loop-major.
struct Access<const L: usize, const S: usize>;

impl<const L: usize, const S: usize> Access<L, S> {
    const W_PRE: usize = 0;
    const W_DIF: usize = 1;
    const W_D: usize = 1 + NI;
    const R_PRE: usize = 1 + NI + L * S;
    const R_DIF_TO: usize = 3 + NI + L * S;
    const R_DIF_FROM: usize = 3 + 2 * NI + L * S;
    const R_D_TO: usize = 3 + 3 * NI + L * S;
    const R_D_FROM: usize = 3 + 3 * NI + 2 * L * S;
    const R_TAP: usize = 3 + 3 * NI + 3 * L * S;
    const R_TC_TO: usize = 3 + 3 * NI + 3 * L * S + 2 * TAPS;
    const R_TC_FROM: usize = 3 + 3 * NI + 3 * L * S + 2 * TAPS + TC;
}

/// The f32 accesses, from the f32 head: the f32 diffuser allpasses'
/// writes and new and old reads, then the stage allpasses' writes and
/// fixed reads.
struct FAccess<const L: usize, const S: usize>;

impl<const L: usize, const S: usize> FAccess<L, S> {
    const W_DF: usize = 0;
    const R_DF_TO: usize = NF;
    const R_DF_FROM: usize = 2 * NF;
    const W_AP: usize = 3 * NF;
    const R_AP: usize = 3 * NF + L * S;
}

/// A tap `at` along a line `len` long, ring samples behind its write,
/// `LATENCY` nearer.
fn tap_off(at: Frac, len: u16) -> usize {
    ((at.get() * len as f32 + 0.5) as usize)
        .saturating_sub(LATENCY)
        .max(2)
}

/// The taps on the lines a crossfade moves, as (side, rank); `None` pads
/// (`LoopEngine::offsets`).
type Moving = [Option<(usize, usize)>; TC];

impl<const L: usize, const S: usize> LoopEngine<L, S>
where
    Self: Layout<L, S>,
{
    /// The stage delays at `k`, ring samples.
    fn delays_at(k: f32) -> [[u16; S]; L] {
        Self::LENS
            .delay
            .map(|l| l.map(|d| ((d as f32 * k + 0.5) as u16).clamp(2, d as u16)))
    }

    /// The knobs' goals: the stage delays at SIZE's k (loop 0's round
    /// trip is `length_ms`), the diffuser at SPREAD, and PRE-DLY, in ring
    /// samples.
    fn goals(k: &EngineKnobs) -> ([[u16; S]; L], [u16; DIFF], u16) {
        let size = clampf((k.length_ms * PER_MS - Self::AP0) * Self::INV_D0, 0.0, 1.0);
        let pre = clampf(k.predelay_ms * PER_MS + 0.5, 2.0, Self::LENS.pre as f32) as u16;
        let dif = core::array::from_fn(|j| k.spread.len(j).min(Self::LENS.diffuser[j] as u16));
        (Self::delays_at(size), dif, pre)
    }

    /// Land on `k` at once: a fresh engine's first block, which no one
    /// hears move.
    #[inline(never)]
    pub fn prime(&mut self, k: &EngineKnobs) {
        let (d, dif, pre) = Self::goals(k);
        (self.at, self.to, self.goal) = (d, d, d);
        (self.dif_at, self.dif_to, self.dif_goal) = (dif, dif, dif);
        (self.pre_at, self.pre_to, self.pre_goal) = (pre, pre, pre);
        (self.left, self.primed, self.retarget) = (0, true, true);
        self.ease_to(k, 0.0, 0.0);
    }

    /// Each eased value one block toward `k`: TONE's and the damper's
    /// coefficients, the g, DENSITY's taps, and each stage's gain for
    /// `k`'s RT60 at the lengths it heads for. The stage allpasses take
    /// DIFFUSE's g held to the longest one's stage gain, so its own ring
    /// never outlasts the RT60. The targets' `exp2`s run only when what
    /// they read changes.
    fn ease_to(&mut self, k: &EngineKnobs, k20: f32, k50: f32) {
        let key = [k.rt60_s.to_bits(), k.tone_hz.to_bits(), k.damp_hz.to_bits()];
        if key != self.key || self.retarget {
            (self.key, self.retarget) = (key, false);
            // TONE OFF is an open corner: the one-pole passes the send.
            self.t_ta = if k.tone_hz.is_finite() {
                one_pole(k.tone_hz, FS)
            } else {
                1.0
            };
            self.t_da = one_pole(k.damp_hz, FS);
            let per = -3.0 * LOG2_10 / (k.rt60_s.max(1e-3) * FS);
            self.t_cap = pow2(per * Self::AP_MAX);
            let d = self.to;
            for l in 0..L {
                for s in 0..S {
                    let len = (d[l][s] as usize + Self::LENS.ap[l][s]) as f32;
                    self.t_g[l][s] = pow2(per * len).min(MAX_GAIN);
                }
                // Past the trip cap, every stage gives up the same share.
                let trip: f32 = self.t_g[l].iter().product();
                if trip > TRIP_MAX {
                    let k = pow2(lg2(TRIP_MAX / trip) / S as f32);
                    self.t_g[l].iter_mut().for_each(|g| *g *= k);
                }
            }
        }
        ease(&mut self.ta, self.t_ta, k20);
        ease(&mut self.gd, k.diffuse_g, k20);
        ease(&mut self.da, self.t_da, k20);
        ease(&mut self.gs, k.diffuse_g.min(self.t_cap), k20);
        ease(&mut self.taps, k.density.get(), k20);
        for l in 0..L {
            for s in 0..S {
                ease(&mut self.g[l][s], self.t_g[l][s], k50);
            }
        }
    }

    /// The SMEAR offsets the next block reads its two smeared allpasses
    /// short by, (start, end) in ring samples: `D·(1 + x)`, each held to
    /// twice its loop's cap.
    pub fn set_smear(&mut self, s: [(f32, f32); 2]) {
        self.smear = s;
    }

    /// Step `step` ring samples a crossfade from now on: the slap a 5 ms
    /// step avoids, for the no-slap test to hear.
    #[cfg(any(test, feature = "test-support"))]
    pub fn coarse_steps(&mut self, step: u16) {
        self.coarse = step;
    }

    /// The eased values' targets as cached: TONE's and the damper's
    /// coefficients, and each stage's gain (tests: a stale cache).
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::type_complexity)]
    pub fn targets(&self) -> (f32, f32, [[f32; S]; L]) {
        (self.t_ta, self.t_da, self.t_g)
    }

    /// The i16 lines' shared write head: a line's slot i of `regions`
    /// sits at (head + i) mod `SLOTS` (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn head(&self) -> usize {
        self.h as usize
    }

    /// The SMEAR offsets the last block read by, as held.
    pub fn smear(&self) -> [(f32, f32); 2] {
        self.smear
    }

    /// The stage delays, landed.
    pub fn delays(&self) -> [[u16; S]; L] {
        self.at
    }

    /// The diffuser's allpasses, landed.
    pub fn diffuser(&self) -> [u16; DIFF] {
        self.dif_at
    }

    /// Each side's tap offsets as they play now, ring samples behind
    /// their lines' writes (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn tap_offsets(&self) -> [[usize; TAPS]; 2] {
        core::array::from_fn(|sd| {
            core::array::from_fn(|q| {
                let t = Self::TAPS[sd][q];
                tap_off(t.at, self.at[t.lp][t.stage])
            })
        })
    }

    /// Each tap's weight this block: its sign, ×¾, × DENSITY's fade.
    pub fn tap_weights(&self) -> [[f32; TAPS]; 2] {
        Self::weights_at(TapCount(self.taps))
    }

    /// The tap weights at `count`, scaled so the return's power holds
    /// with DENSITY: the taps read near-uncorrelated lines, so √(3/Σw²)
    /// keeps the 3 fixed taps' level at every count.
    fn weights_at(count: TapCount) -> [[f32; TAPS]; 2] {
        let e: f32 = (0..TAPS).map(|q| count.weight(q) * count.weight(q)).sum();
        let norm = libm::sqrtf(TAPS_FIXED as f32 / e);
        core::array::from_fn(|sd| {
            core::array::from_fn(|q| Self::TAPS[sd][q].sign.f32() * TAP_W * norm * count.weight(q))
        })
    }

    /// DENSITY's taps a side as they play (eased).
    pub fn taps(&self) -> f32 {
        self.taps
    }

    /// PRE-DLY, landed.
    pub fn predelay(&self) -> u16 {
        self.pre_at
    }

    /// The crossfade running: the stage delays from and to, and PRE-DLY's.
    #[allow(clippy::type_complexity)]
    pub fn crossfade(&self) -> Option<([[u16; S]; L], [[u16; S]; L], (u16, u16))> {
        (self.left > 0).then_some((self.at, self.to, (self.pre_at, self.pre_to)))
    }

    /// Whether a length still moves toward the knobs.
    pub fn moving(&self) -> bool {
        self.left > 0
            || self.at != self.goal
            || self.dif_at != self.dif_goal
            || self.pre_at != self.pre_goal
    }

    /// The next crossfade toward the goals: in each loop, the stage delay
    /// farthest from its goal moves `STEP` (or what is left), so a loop's
    /// round trip moves by one slice of at most 5 ms (§ Moving a length),
    /// every loop in the same crossfade; likewise the diffuser allpass
    /// farthest from its goal, and PRE-DLY. Two delays of a loop stepping
    /// unrelated amounts in one crossfade (k's way) left two combs where
    /// the spec has one at 200 Hz spacing.
    #[inline(never)]
    fn schedule(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        let step = if self.coarse > 0 { self.coarse } else { STEP };
        #[cfg(not(any(test, feature = "test-support")))]
        let step = STEP;
        let toward = |at: u16, goal: u16| {
            let d = goal.abs_diff(at).min(step);
            if goal > at { at + d } else { at - d }
        };
        let far = |at: &[u16], goal: &[u16]| {
            (0..at.len())
                .max_by_key(|&j| (goal[j].abs_diff(at[j]), usize::MAX - j))
                .unwrap_or(0)
        };
        self.to = self.at;
        for l in 0..L {
            let f = far(&self.at[l], &self.goal[l]);
            self.to[l][f] = toward(self.at[l][f], self.goal[l][f]);
        }
        self.dif_to = self.dif_at;
        let f = far(&self.dif_at, &self.dif_goal);
        let d = self.dif_goal[f].abs_diff(self.dif_at[f]).min(DIF_STEP);
        self.dif_to[f] = if self.dif_goal[f] > self.dif_at[f] {
            self.dif_at[f] + d
        } else {
            self.dif_at[f] - d
        };
        self.pre_to = toward(self.pre_at, self.pre_goal);
        if self.to != self.at || self.dif_to != self.dif_at || self.pre_to != self.pre_at {
            (self.left, self.retarget) = (XFADE, true);
        }
    }

    /// One ring block of the decimated send `u` (1.0 full scale); `l` and
    /// `r` get the return in LSBs.
    #[inline(never)]
    pub fn run(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let (lines, f, _) = arena.split(Self::SLOTS, Self::FSLOTS);
        self.block::<[i16], true>(lines, f, u, k, l, r);
    }

    /// Whether this block runs a crossfade: scheduled first, as `block`
    /// does, so the choice of read path sees it.
    fn crossfading(&mut self, k: &EngineKnobs) -> bool {
        if !self.primed {
            self.prime(k);
        }
        (self.goal, self.dif_goal, self.pre_goal) = Self::goals(k);
        if self.left == 0 && self.moving() {
            self.schedule();
        }
        self.left > 0
    }

    /// `run` with its i16 lines as `lines`' floats (unquantised), and the
    /// re-entry clips off unless `clipped`: the level plan's references.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments)]
    pub fn run_reference(
        &mut self,
        arena: &mut ReverbArena,
        lines: Option<&mut [f32]>,
        clipped: bool,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let (lo, f, _) = arena.split(Self::SLOTS, Self::FSLOTS);
        match (lines, clipped) {
            (Some(x), true) => self.block::<[f32], true>(x, f, u, k, l, r),
            (Some(x), false) => self.block::<[f32], false>(x, f, u, k, l, r),
            (None, true) => self.block::<[i16], true>(lo, f, u, k, l, r),
            (None, false) => self.block::<[i16], false>(lo, f, u, k, l, r),
        }
    }

    /// Each access's offset from the head this block (`OFFSETS`, and the
    /// reads the lengths move, new then old), the f32 ones, and what moves.
    fn offsets(&self) -> ([usize; MAX_ACCESS], [usize; MAX_F_ACCESS], Moving) {
        type A<const L: usize, const S: usize> = Access<L, S>;
        type F<const L: usize, const S: usize> = FAccess<L, S>;
        let h = &Self::HEADS;
        let (mut o, mut of) = (Self::OFFSETS, Self::F_OFFSETS);
        o[A::<L, S>::R_PRE] = h.pre - self.pre_to as usize;
        o[A::<L, S>::R_PRE + 1] = h.pre - self.pre_at as usize;
        for (j, &d) in DIF_I.iter().enumerate() {
            o[A::<L, S>::R_DIF_TO + j] = h.diffuser[d] - self.dif_to[d] as usize;
            o[A::<L, S>::R_DIF_FROM + j] = h.diffuser[d] - self.dif_at[d] as usize;
        }
        for (j, &d) in DIF_F.iter().enumerate() {
            of[F::<L, S>::R_DF_TO + j] = h.diffuser[d] - self.dif_to[d] as usize;
            of[F::<L, S>::R_DF_FROM + j] = h.diffuser[d] - self.dif_at[d] as usize;
        }
        let mut mv: Moving = [None; TC];
        for l in 0..L {
            for s in 0..S {
                let j = l * S + s;
                o[A::<L, S>::R_D_TO + j] = h.delay[l][s] - self.to[l][s] as usize;
                o[A::<L, S>::R_D_FROM + j] = h.delay[l][s] - self.at[l][s] as usize;
            }
        }
        let mut c = 0;
        for sd in 0..2 {
            for q in 0..TAPS {
                let t = Self::TAPS[sd][q];
                let w = h.delay[t.lp][t.stage];
                let (to, at) = (self.to[t.lp][t.stage], self.at[t.lp][t.stage]);
                o[A::<L, S>::R_TAP + TAPS * sd + q] = w - tap_off(t.at, to);
                // `TAPS_FIT` holds the moving line's taps to `TC`.
                if to != at && c < TC {
                    o[A::<L, S>::R_TC_TO + c] = w - tap_off(t.at, to);
                    o[A::<L, S>::R_TC_FROM + c] = w - tap_off(t.at, at);
                    mv[c] = Some((sd, q));
                    c += 1;
                }
            }
        }
        // The pads read where tap 0 does; their weight is 0.
        for p in c..TC {
            o[A::<L, S>::R_TC_TO + p] = o[A::<L, S>::R_TAP];
            o[A::<L, S>::R_TC_FROM + p] = o[A::<L, S>::R_TAP];
        }
        (o, of, mv)
    }

    /// One block: a moving block (a crossfade, or DENSITY's taps moving)
    /// blends only the reads that move and ramps the tap weights; a still
    /// one (most blocks) reads each line once.
    #[inline(always)]
    fn block<Ln: Lines + ?Sized, const CLIP: bool>(
        &mut self,
        lines: &mut Ln,
        f: &mut [f32],
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let xf = self.crossfading(k);
        // The eases: τ 50 ms for the gains, as QUAD's TIME, ramped each
        // sample; DENSITY's taps 20 ms, ramped each sample while they
        // move; 20 ms the rest, stepped once a block, as ROOM's.
        let (g0, taps0) = (self.g, self.taps);
        const K20: f32 = ease_k(0.02);
        const K50: f32 = ease_k(0.05);
        self.ease_to(k, K20, K50);
        // What moves: the stage delays, the taps on their lines and
        // PRE-DLY (SIZE, PRE-DLY); the diffuser (SPREAD); DENSITY's taps.
        let xs = xf && (self.to != self.at || self.pre_to != self.pre_at);
        let xd = xf && self.dif_to != self.dif_at;
        let dr = self.taps != taps0;
        // One path for each single move, and one for any two at once
        // (rare: it does all three).
        match (xs, xd, dr) {
            (false, false, false) => {
                self.block_xf::<Ln, CLIP, false, false, false>(lines, f, u, l, r, g0, taps0)
            }
            (true, false, false) => {
                self.block_xf::<Ln, CLIP, true, false, false>(lines, f, u, l, r, g0, taps0)
            }
            (false, true, false) => {
                self.block_xf::<Ln, CLIP, false, true, false>(lines, f, u, l, r, g0, taps0)
            }
            (false, false, true) => {
                self.block_xf::<Ln, CLIP, false, false, true>(lines, f, u, l, r, g0, taps0)
            }
            _ => self.block_xf::<Ln, CLIP, true, true, true>(lines, f, u, l, r, g0, taps0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn block_xf<
        Ln: Lines + ?Sized,
        const CLIP: bool,
        const XS: bool,
        const XD: bool,
        const DR: bool,
    >(
        &mut self,
        lines: &mut Ln,
        f: &mut [f32],
        u: &[f32; HALF],
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
        g0: [[f32; S]; L],
        taps0: f32,
    ) {
        let () = Self::FITS;
        let (slots, fslots) = (Self::SLOTS, Self::FSLOTS);
        // Every unchecked access below leans on these two.
        assert!(lines.len() >= slots && f.len() >= fslots);
        let n = 1.0 / HALF as f32;
        let (ta, gd, gs, da) = (self.ta, self.gd, self.gs, self.da);
        let inv_gd = 1.0 / (1.0 - gd);
        let mut g = g0;
        let dg: [[f32; S]; L] =
            core::array::from_fn(|l| core::array::from_fn(|s| (self.g[l][s] - g0[l][s]) * n));
        let wt1 = self.tap_weights();
        let mut wt = if DR {
            Self::weights_at(TapCount(taps0))
        } else {
            wt1
        };
        let dwt: [[f32; TAPS]; 2] =
            core::array::from_fn(|sd| core::array::from_fn(|q| (wt1[sd][q] - wt[sd][q]) * n));
        // The crossfade's blend: the new read's share.
        let (mut w, dw) = if self.left > 0 {
            ((XFADE - self.left) as f32 * XFADE_INV, XFADE_INV)
        } else {
            (1.0, 0.0)
        };
        // SMEAR, held to twice each loop's cap at the shorter of its
        // lengths this block.
        let short: [[u16; S]; L] =
            core::array::from_fn(|l| core::array::from_fn(|s| self.at[l][s].min(self.to[l][s])));
        let off: [(f32, f32); 2] = core::array::from_fn(|m| {
            let lp = Self::SMEARED[m].0;
            let trip = (0..S)
                .map(|s| (short[lp][s] as usize + Self::LENS.ap[lp][s]) as f32)
                .sum::<f32>();
            let top = 2.0 * Self::SMEAR_CAP * trip;
            let (a, b) = self.smear[m];
            (clampf(a, 0.0, top), clampf(b, 0.0, top))
        });
        self.smear = off;
        let mut sm = [off[0].0, off[1].0];
        let dsm = [(off[0].1 - off[0].0) * n, (off[1].1 - off[1].0) * n];
        let nom = Self::SMEARED.map(|(l, s)| Self::LENS.ap[l][s] as f32);
        const { assert!(L <= 2 && S <= 4) };
        let (off_i, off_f, mv) = self.offsets();
        // The moving line's taps' weights by side, at the block's start
        // (a DENSITY ramp under a crossfade leaves them a block stale).
        let cw: [[f32; TC]; 2] = core::array::from_fn(|side| {
            core::array::from_fn(|c| match mv[c] {
                Some((sd, q)) if sd == side => wt[sd][q],
                _ => 0.0,
            })
        });
        let (mut tone, mut lp) = (self.tone, self.lp);
        let (mut h, mut hf) = (self.h as usize % slots, self.hf as usize % fslots);
        let mut i = 0;
        while i < HALF {
            let (b, len) = segment(h, slots, &off_i, HALF - i);
            let (fb, len) = segment(hf, fslots, &off_f, len);
            // This segment's send and return: `t` < `len` bounds all three.
            let (us, ls, rs) = (&u[i..i + len], &mut l[i..i + len], &mut r[i..i + len]);
            for t in 0..len {
                macro_rules! rd {
                    ($j:expr) => {
                        // SAFETY: `segment` put each base below its
                        // region's length m (≤ the slice's, asserted
                        // above) and `len` at most m − base, and `t` <
                        // `len`. The writes likewise.
                        unsafe { lines.rd(b[$j] + t) }
                    };
                }
                macro_rules! fget {
                    ($j:expr) => {
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked(fb[$j] + t) }
                    };
                }
                macro_rules! fset {
                    ($j:expr, $v:expr) => {
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked_mut(fb[$j] + t) = $v }
                    };
                }
                macro_rules! blend {
                    ($to:expr, $from:expr) => {{
                        let o = rd!($from);
                        o + w * (rd!($to) - o)
                    }};
                }
                type A<const L: usize, const S: usize> = Access<L, S>;
                type F<const L: usize, const S: usize> = FAccess<L, S>;
                tone += ta * (us[t] * IN_GAIN - tone);
                // SAFETY: as `rd`'s.
                unsafe { lines.round(b[A::<L, S>::W_PRE] + t, tone) };
                let mut d = if XS {
                    blend!(A::<L, S>::R_PRE, A::<L, S>::R_PRE + 1)
                } else {
                    rd!(A::<L, S>::R_PRE)
                };
                // The diffuser in signal order: `DIF_F`'s on f32 states,
                // `DIF_I`'s normalised i16. While one moves all six blend;
                // the still ones' old and new reads are one slot.
                unroll!(J in [0, 1, 2, 3, 4, 5] {
                    if DIF_F32[J] {
                        const FJ: usize = match J { 0 => 0, 1 => 1, 4 => 2, _ => 3 };
                        let mut z = fget!(F::<L, S>::R_DF_TO + FJ);
                        if XD {
                            let o = fget!(F::<L, S>::R_DF_FROM + FJ);
                            z = o + w * (z - o);
                        }
                        let (y, v) = ap_f32(d, z, gd);
                        fset!(F::<L, S>::W_DF + FJ, v);
                        d = y;
                    } else {
                        const IJ: usize = J.saturating_sub(2);
                        let z = if XD {
                            blend!(A::<L, S>::R_DIF_TO + IJ, A::<L, S>::R_DIF_FROM + IJ)
                        } else {
                            rd!(A::<L, S>::R_DIF_TO + IJ)
                        };
                        let (y, s) = ap_norm(d, z, gd, inv_gd);
                        // SAFETY: as `rd`'s.
                        unsafe { lines.settle_ap(b[A::<L, S>::W_DIF + IJ] + t, s) };
                        d = y;
                    }
                });
                // Each loop's stages, unrolled: `LO` and `ST` are literals,
                // past `L` and `S` dead code.
                let mut out = [0.0f32; L];
                unroll!(LO in [0, 1] {
                    if LO < L {
                        unroll!(ST in [0, 1, 2, 3] {
                            if ST < S {
                                let j = LO * S + ST;
                                // Stage `ST`'s delay, 2 or more behind its
                                // write: before or after it, the same.
                                // While a length moves all four blend; the
                                // still ones' reads are one slot.
                                let x = if XS {
                                    blend!(A::<L, S>::R_D_TO + j, A::<L, S>::R_D_FROM + j)
                                } else {
                                    rd!(A::<L, S>::R_D_TO + j)
                                };
                                // SAFETY (`pair_in`): the write slot is below
                                // `fslots` (as `fget`'s); `sm` is clamped to
                                // [0, 2·cap·trip] above, and `SMEAR_FITS`
                                // keeps `nom − sm` in 2..`fslots`.
                                let z = if (LO, ST) == Self::SMEARED[0] {
                                    // SAFETY: as above (pair_in).
                                    unsafe { pair_in(&*f, fslots, fb[F::<L, S>::W_AP + j] + t, nom[0] - sm[0]) }
                                } else if (LO, ST) == Self::SMEARED[1] {
                                    // SAFETY: as above (pair_in).
                                    unsafe { pair_in(&*f, fslots, fb[F::<L, S>::W_AP + j] + t, nom[1] - sm[1]) }
                                } else {
                                    fget!(F::<L, S>::R_AP + j)
                                };
                                let (mut y, v) = ap_f32(x, z, gs);
                                fset!(F::<L, S>::W_AP + j, v);
                                if ST + 1 == S {
                                    lp[LO] += da * (y - lp[LO]);
                                    y = lp[LO];
                                }
                                y *= g[LO][ST];
                                if ST + 1 < S {
                                    // SAFETY: as `rd`'s.
                                    unsafe { lines.settle_ap(b[A::<L, S>::W_D + j + 1] + t, y) };
                                } else {
                                    out[LO] = y;
                                }
                            }
                        });
                    }
                });
                // The send, R1's clip at re-entry.
                unroll!(LO in [0, 1] {
                    if LO < L {
                        let x = clip::<CLIP>(out[LO] + Self::INJECT[LO] * d);
                        // SAFETY: as `rd`'s.
                        unsafe { lines.settle(b[A::<L, S>::W_D + LO * S] + t, x) };
                    }
                });
                let mut side = [0.0f32; 2];
                unroll!(SD in [0, 1] {
                    unroll!(Q in [0, 1, 2, 3, 4, 5, 6, 7] {
                        side[SD] += wt[SD][Q] * rd!(A::<L, S>::R_TAP + TAPS * SD + Q);
                    });
                });
                if XS {
                    // The moving line's taps: each read at its new length
                    // above, and here (1 − w) of the way back to its old.
                    let mut back = [0.0f32; 2];
                    unroll!(C in [0, 1, 2, 3] {
                        let d = rd!(A::<L, S>::R_TC_FROM + C) - rd!(A::<L, S>::R_TC_TO + C);
                        back[0] += cw[0][C] * d;
                        back[1] += cw[1][C] * d;
                    });
                    side[0] += (1.0 - w) * back[0];
                    side[1] += (1.0 - w) * back[1];
                }
                if DR {
                    unroll!(SD in [0, 1] {
                        unroll!(Q in [0, 1, 2, 3, 4, 5, 6, 7] {
                            wt[SD][Q] += dwt[SD][Q];
                        });
                    });
                }
                (ls[t], rs[t]) = (side[0], side[1]);
                unroll!(LO in [0, 1] {
                    unroll!(ST in [0, 1, 2, 3] {
                        if LO < L && ST < S {
                            g[LO][ST] += dg[LO][ST];
                        }
                    });
                });
                w = (w + dw).min(1.0);
                sm = [sm[0] + dsm[0], sm[1] + dsm[1]];
            }
            i += len;
            h = wrap(h + len, slots);
            hf = wrap(hf + len, fslots);
        }
        (self.tone, self.lp, self.h, self.hf) = (tone, lp, h as u16, hf as u16);
        if self.left > 0 {
            self.left = self.left.saturating_sub(HALF as u16);
            if self.left == 0 {
                (self.at, self.dif_at, self.pre_at) = (self.to, self.dif_to, self.pre_to);
            }
        }
    }
}
