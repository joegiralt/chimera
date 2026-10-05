//! TRANSWAVE (spec § 2 TRANSWAVE): a table of single-cycle frames, scanned.
//! Each frame is held at octave mip levels from a top level (512, or 1 024)
//! down to 16 samples, each band-limited to `⌊0.35 · N⌋` harmonics by the
//! importer: the PPG Wave and Waldorf Microwave approach, for the Ensoniq
//! VFX/SD-1's transwaves. POSITION and a triangle scan (MOTION its rate,
//! SIZE its width) pick the frame, as the Polyend Tracker v1.5's wavetable
//! POSITION and WINDOW do.
//!
//! The layout is frame-major: frame `f`'s level `N` starts at sample
//! `f · S + level_offset(top, log2 N)`, `S = 2 · top − 16` (1 008 or 2 032).
//!
//! Two heads at one phase read frames `⌊p⌋` and `⌊p⌋ + 1` at one level,
//! lerped by `p`'s fraction; a level change reads both levels for one block
//! (four heads) and fades between them. The note-on level is the largest
//! whose step is at most 1; it drops only past `ALIAS_STEP` and rises only
//! under `RISE_STEP`, so vibrato never changes it.

use chimera_hal::BLOCK_SIZE;

use super::params::TapeModel;
use super::read::{Pos, Shape, SourceView};
use crate::dsp::algo::math::{clampf, exp2};

/// A level's content stops at 0.35 N, so its step is alias-free to
/// 1/(2 · 0.35): past it the level drops.
pub const ALIAS_STEP: f32 = 1.43;
/// Under this step the level rises: 0.35, so a rise lands under 0.7 and a
/// drop over 0.715, and neither undoes the other.
pub const RISE_STEP: f32 = 0.35;
/// The smallest level, `log2 N`: 16 samples, 5 harmonics.
pub const MIN_LOG2: u32 = 4;
/// The fundamental's most cycles a sample: Nyquist.
pub const CYCLES_MAX: f32 = 0.5;
/// MOTION's scan rate at ±1, Hz.
pub const SCAN_HZ: f32 = 8.0;

/// The top level of `count` frames held in `frames`: 512 or 1 024, else
/// `None`.
pub fn top_of(frames: u32, count: u8) -> Option<u32> {
    let c = u32::from(count);
    [512, 1_024]
        .into_iter()
        .find(|&top| c > 0 && frames == c * (2 * top - 16))
}

/// Where level `2^log2_n` starts in a frame: after the larger ones.
pub fn level_offset(top: u32, log2_n: u32) -> u32 {
    (2 * top).saturating_sub(2u32 << (log2_n & 15))
}

/// The largest level `N` (16 to `top`) with `N · freq / sample_rate ≤ 1`.
pub fn note_on_level(freq: f32, top: u32, sample_rate: u32) -> u32 {
    let mut n = top.max(1 << MIN_LOG2);
    while n > 1 << MIN_LOG2 && n as f32 * freq > sample_rate as f32 {
        n >>= 1;
    }
    n
}

/// The level after one at `log2_n` read at `step`: smaller while the step
/// is past `ALIAS_STEP`, larger while under `RISE_STEP`, within 16 and
/// `top`.
pub fn next_level(log2_n: u32, step: f32, top: u32) -> u32 {
    let hi = top.max(1 << MIN_LOG2).trailing_zeros();
    let (mut l, mut s) = (log2_n.clamp(MIN_LOG2, hi), step);
    while s > ALIAS_STEP && l > MIN_LOG2 {
        (l, s) = (l - 1, s * 0.5);
    }
    while s < RISE_STEP && l < hi {
        (l, s) = (l + 1, s * 2.0);
    }
    l
}

/// TRANSWAVE plays only a `Transwave` whose frames fit its count; every
/// other model plays any source.
pub fn playable(model: TapeModel, v: &SourceView<'_>) -> bool {
    let d = v.desc();
    model != TapeModel::Transwave
        || (d.shape == Shape::Transwave && top_of(v.frames(), d.count).is_some())
}

/// The note's fundamental in cycles a sample, `cents` on it: the source's
/// rate and root don't enter (spec § 2).
pub fn cycles(note: u8, cents: f32, sample_rate: u32) -> f32 {
    let oct = clampf(
        (note as f32 * 100.0 + cents - 6_900.0) / 1_200.0,
        -40.0,
        40.0,
    );
    clampf(
        440.0 / sample_rate as f32 * exp2(oct),
        f32::EPSILON,
        CYCLES_MAX,
    )
}

/// The triangle at `x` in [0, 1): 0 at 0, 1 at ¼, −1 at ¾.
fn tri(x: f32) -> f32 {
    if x < 0.25 {
        4.0 * x
    } else if x < 0.75 {
        2.0 - 4.0 * x
    } else {
        4.0 * x - 4.0
    }
}

/// A transwave note's state.
#[derive(Clone, Copy, Debug)]
pub struct Wave {
    /// `log2` of the top level; 0 while the source isn't playable.
    top: u8,
    count: u8,
    /// The level in use, `log2 N`.
    level: u8,
    /// This block's level change: the level it fades from.
    from: Option<u8>,
    /// The cycle's phase, a whole cycle 2³².
    phase: u32,
    /// MOTION's scan phase, [0, 1).
    scan: f32,
    /// The frame position at the last block's end, Q16.
    p: u32,
}

impl Wave {
    pub const IDLE: Wave = Wave {
        top: 0,
        count: 1,
        level: MIN_LOG2 as u8,
        from: None,
        phase: 0,
        scan: 0.0,
        p: 0,
    };

    /// A note at `cycles` a sample on `v`: the largest level whose step is
    /// at most 1, phase and scan at 0. Silent on a source it can't play.
    pub fn on(v: &SourceView<'_>, cycles: f32) -> Wave {
        let count = v.desc().count;
        let top = match top_of(v.frames(), count) {
            Some(top) if playable(TapeModel::Transwave, v) => top,
            _ => return Wave::IDLE,
        };
        Wave {
            top: top.trailing_zeros() as u8,
            count,
            level: note_on_level(cycles, top, 1).trailing_zeros() as u8,
            ..Wave::IDLE
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// The level a change fades from, in the block it fades.
    pub fn from(&self) -> Option<u8> {
        self.from
    }

    /// The frames read, `⌊p⌋` and `⌊p⌋ + 1` (the last frame twice at the
    /// end), and the second's share.
    pub fn frames(&self) -> (u32, u32, f32) {
        let last = u32::from(self.count) - 1;
        let f = (self.p >> 16).min(last);
        (
            f,
            (f + 1).min(last),
            (self.p & 0xFFFF) as f32 * (1.0 / 65_536.0),
        )
    }

    /// A block: the fundamental ramps from `c0` to `c1` cycles a sample,
    /// `p` from the last block's end to this one's (POSITION, MOTION and
    /// SIZE as eased; taken whole on a note's `first` block). The level
    /// moves once a block, by the block's larger step, so the new level
    /// reads at most `ALIAS_STEP` throughout; a change fades over the block.
    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (c0, c1): (f32, f32),
        (position, motion, size): (f32, f32, f32),
        (first, sample_rate): (bool, u32),
    ) {
        let top = 1u32 << self.top;
        self.from = None;
        if self.top == 0 || top_of(v.frames(), self.count) != Some(top) {
            out.fill(0.0);
            return;
        }
        let s = self.scan + SCAN_HZ * motion * BLOCK_SIZE as f32 / sample_rate as f32;
        self.scan = s - libm::floorf(s);
        let last = f32::from(self.count - 1);
        let p1 = (clampf(last * (position + size * tri(self.scan)), 0.0, last) * 65_536.0) as u32;
        let p0 = if first { p1 } else { self.p };
        self.p = p1;

        let old = u32::from(self.level);
        let level = next_level(old, c0.max(c1) * (1u32 << old) as f32, top);
        self.level = level as u8;
        let span = 2 * top - 16;
        // In Q22 the block's 64 steps of `p1 − p0` land on `p1` exactly.
        let k = Run {
            p0: p0 << 6,
            dp: p1.wrapping_sub(p0),
            last: u32::from(self.count) - 1,
            span,
            d0: (c0 * 4_294_967_296.0) as u32,
            dd: (((c1 - c0) * 4_294_967_296.0) as i64 / BLOCK_SIZE as i64) as u32,
        };
        self.phase = if level == old {
            k.run(out, v, self.phase, (level, level_offset(top, level)))
        } else {
            self.from = Some(old as u8);
            k.fade(
                out,
                v,
                self.phase,
                [
                    (old, level_offset(top, old)),
                    (level, level_offset(top, level)),
                ],
            )
        };
    }
}

/// The phase `ph` (a cycle 2³²) in samples of a frame `2^l` long, as a
/// 32.32 position: built from two 32-bit shifts (`l` is 4 to 10), not a
/// 64-bit one.
#[inline(always)]
fn at_level(ph: u32, l: u32) -> Pos {
    Pos((u64::from(ph >> (32 - l)) << 32) | u64::from(ph << l))
}

/// One block's ramps: `p` (Q22) from `p0` by `dp` a sample, the phase's
/// step from `d0` by `dd` a sample (both wrapping); frames `last` at most,
/// `span` samples apart.
struct Run {
    p0: u32,
    dp: u32,
    last: u32,
    span: u32,
    d0: u32,
    dd: u32,
}

impl Run {
    /// The two frames' starts at `p` (Q22: 32 frames fit) and the second's
    /// share.
    #[inline(always)]
    fn frames(&self, p: u32) -> (u32, u32, f32) {
        let f = (p >> 22).min(self.last);
        let g = (f + 1).min(self.last);
        let t = (p & 0x3F_FFFF) as f32 * (1.0 / 4_194_304.0);
        (f * self.span, g * self.span, t)
    }

    /// Two heads at one level `(log2 N, offset)`. Returns the phase after.
    #[inline(never)]
    fn run(
        &self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        mut ph: u32,
        (l, off): (u32, u32),
    ) -> u32 {
        let (mut d, mut p) = (self.d0, self.p0);
        for y in out.iter_mut() {
            p = p.wrapping_add(self.dp);
            let (a, b, t) = self.frames(p);
            let pos = at_level(ph, l);
            let x = v.read_cycle(a + off, l, pos);
            *y = x + t * (v.read_cycle(b + off, l, pos) - x);
            d = d.wrapping_add(self.dd);
            ph = ph.wrapping_add(d);
        }
        ph
    }

    /// Four heads: both frames at both levels, the output fading from the
    /// first level to the second across the block.
    #[inline(never)]
    fn fade(
        &self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        mut ph: u32,
        [(l0, o0), (l1, o1)]: [(u32, u32); 2],
    ) -> u32 {
        let (mut d, mut p, mut g) = (self.d0, self.p0, 0.0f32);
        for y in out.iter_mut() {
            p = p.wrapping_add(self.dp);
            let (a, b, t) = self.frames(p);
            let (p0, p1) = (at_level(ph, l0), at_level(ph, l1));
            let x0 = v.read_cycle(a + o0, l0, p0);
            let y0 = x0 + t * (v.read_cycle(b + o0, l0, p0) - x0);
            let x1 = v.read_cycle(a + o1, l1, p1);
            let y1 = x1 + t * (v.read_cycle(b + o1, l1, p1) - x1);
            // Exact: a sum of 1/64s.
            g += 1.0 / BLOCK_SIZE as f32;
            *y = y0 + g * (y1 - y0);
            d = d.wrapping_add(self.dd);
            ph = ph.wrapping_add(d);
        }
        ph
    }
}
