//! STABILITY on the 2040 and LADDER (ADR 0168): each voice's filter is its
//! own chip with an exponential converter that is not temperature-
//! compensated, as a Prophet-5's (SD411's R479 a plain 1 K; tech manual
//! § 4-17, "the filters will drift"). A voice's cutoff sits a little apart,
//! wanders slowly and tracks the keyboard a little off, and its RES is a
//! little off. VA's oscillator STABILITY's law and seeds (ADR 0118): every
//! amount is scaled by `d = (1 − STAB)²`, and at `d == 0` the spread is
//! exactly neutral, so STAB 1 is bit-identical.
//!
//! It works on the block's ramp ends, never per sample: `g × cut` (the
//! prewarped coefficient, so a cent is a cent below a few kHz and a little
//! less near Nyquist) and `k × res`. While only the drift moves, both ends
//! take this block's spread, so a steady block stays on the steady loop
//! and the drift steps block to block, hundredths of a cent each.
//! A larger change (a note's tracking error, a turn of STABILITY) ramps
//! from the last block's spread as a knob's glide does.

use super::k2040::k2040_k;
use crate::dsp::clampf;
pub use crate::dsp::va::stability::depth;
use crate::dsp::va::stability::{Seed, mix32, uni, xorshift};
use crate::hw::Cost;
use crate::project::PartId;

/// Each amount at `d = 1` (STAB 0): four times the lab's Prophet-like
/// setting, which lands at STAB ½ (`d = ¼`): ±25 cents of offset, 6 cents
/// RMS of drift, ±2 % of tracking error, ±3 % of RES. The lab's
/// "pronounced" setting sits near STAB 0.23.
pub const OFFSET_CENTS: f32 = 100.0;
/// The walk's clamp (±1) in cents; its RMS is `WALK_RMS` of it.
pub const DRIFT_CENTS: f32 = 80.0;
/// Octaves of error an octave, about C4.
pub const TRACK: f32 = 0.08;
/// A fraction of `k`.
pub const RES_SPREAD: f32 = 0.12;
/// The Prophet-like setting: a new Sound's (`Sound::init`).
pub const PROPHET: f32 = 0.5;

/// The walk's corner, 1/(2π · 3 s): the lab's 3 s one-poles.
pub const WALK_HZ: f32 = 0.053;
/// VA's: the clamp at 3.3 σ.
pub const WALK_RMS: f32 = 0.30;
/// `walk_coefs(SAMPLE_RATE / BLOCK_SIZE, WALK_HZ, WALK_RMS)`, held by
/// `the_walk_constants_are_the_formulas`: a constant, so no voice stores
/// them and no note-on computes them.
pub const WALK_A: f32 = 4.439_132_2e-4;
pub const WALK_G: f32 = 49.319_042;

/// A drift-only step under this ratio (0.1 cent) holds for the block.
const HOLD: f32 = 5.78e-5;

pub const SALT_OFFSET: u32 = 0xF170;
pub const SALT_TRACK: u32 = 0xF171;
pub const SALT_RES: u32 = 0xF172;
pub const SALT_WALK: u32 = 0xF173;

/// The fixed draws' strides across a Part's slots: golden, silver and
/// plastic, so neighbouring voices spread out evenly (five hashed draws
/// bunched up too easily to hear, the lab's finding) and the three draws
/// don't line up.
const STRIDE: [f32; 3] = [0.618_034, 0.414_213_57, 0.754_877_7];

/// What STABILITY adds to a voice's bill, on the 2040 and LADDER below 1:
/// `step` (one walk step, an `exp2_poly`, the hold test), about 90
/// instructions on the thumbv7em ORBIT build, and the four multiplies on
/// the ends: 94 × 1.606 / 64 = 2.4. The unit reads 2040 STAB 4 over the
/// 2040, LADDER STAB 3 (2026-10-22, ROUTING bench): billed 5. The draws are
/// note-on work.
pub const STAB_COST: Cost = Cost(5);

/// A voice slot's fixed draw for `salt`, −1..1: Part `part`'s start, `slot`
/// strides on.
fn spaced(part: PartId, slot: u32, k: usize, salt: u32) -> f32 {
    let start = 0.5 * (uni(mix32((part.index() as u32) << 8 ^ salt)) + 1.0);
    let x = start + slot as f32 * STRIDE[k];
    // `x` is positive: truncation is its floor.
    2.0 * (x - x as u32 as f32) - 1.0
}

/// A block's spread on the four-pole ends, `(from, to)`: `g × cut`, `k ×
/// res`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spread {
    pub cut: (f32, f32),
    pub res: (f32, f32),
}

impl Default for Spread {
    fn default() -> Self {
        Self::NONE
    }
}

impl Spread {
    pub const NONE: Spread = Spread {
        cut: (1.0, 1.0),
        res: (1.0, 1.0),
    };

    /// `(from, to)` on `g`. At `NONE` exactly `g`.
    #[inline(always)]
    pub(super) fn g(self, g: (f32, f32)) -> (f32, f32) {
        (g.0 * self.cut.0, g.1 * self.cut.1)
    }

    /// `(from, to)` on `k`, held to RES 1's unless `k` is past it already:
    /// at `NONE` exactly `k`.
    #[inline(always)]
    pub(super) fn k(self, k: (f32, f32)) -> (f32, f32) {
        let top = k2040_k(1.0);
        (
            (k.0 * self.res.0).min(k.0.max(top)),
            (k.1 * self.res.1).min(k.1.max(top)),
        )
    }

    /// `(from, to)` on a damping `k`, SEM's, which falls as RES rises: held
    /// at `floor`, RES 1's, unless `k` is under it already. At `NONE`
    /// exactly `k`.
    #[inline(always)]
    pub(super) fn k_damping(self, k: (f32, f32), floor: f32) -> (f32, f32) {
        let at = |k: f32, r: f32| {
            let f = if k < floor { k } else { floor };
            let v = k * r;
            if v > f { v } else { f }
        };
        (at(k.0, self.res.0), at(k.1, self.res.1))
    }
}

/// One voice slot's chip: its walk, its offset, tracking error and RES
/// draw, the note, and the last block's spread.
#[derive(Clone, Copy, Debug)]
pub struct FilterStability {
    rng: u32,
    y1: f32,
    y2: f32,
    /// Cents at `d = 1`, and cents a semitone from C4.
    offset: f32,
    track: f32,
    /// −1..1.
    res: f32,
    last: (f32, f32),
    note: u8,
    seeded: Option<PartId>,
}

// A slot's chip in the voice chain (ADR 0101's cap): 36 B.
const _: () = assert!(core::mem::size_of::<FilterStability>() == 36);

impl Default for FilterStability {
    fn default() -> Self {
        Self::NEW
    }
}

impl FilterStability {
    pub const NEW: FilterStability = FilterStability {
        rng: 0x6D2B_79F5,
        y1: 0.0,
        y2: 0.0,
        offset: 0.0,
        track: 0.0,
        res: 0.0,
        last: (1.0, 1.0),
        note: 60,
        seeded: None,
    };

    /// A note on `slot` of `part`: the chip's draws, and the walk from the
    /// seed, started mid-wander, when the slot plays a new Part; otherwise
    /// the walk runs on.
    pub fn note_on(&mut self, part: PartId, slot: u32, note: u8) {
        if self.seeded != Some(part) {
            let s = Seed::of(part.index() as u32, slot);
            self.rng = s.stream(SALT_WALK);
            // One σ of the walk's spread at most, √3 × uniform.
            let y = s.draw(SALT_WALK) * (1.732_050_8 * WALK_RMS / WALK_G);
            (self.y1, self.y2) = (y, y);
            self.offset = OFFSET_CENTS * spaced(part, slot, 0, SALT_OFFSET);
            self.track = 100.0 * TRACK * spaced(part, slot, 1, SALT_TRACK);
            self.res = spaced(part, slot, 2, SALT_RES);
            self.seeded = Some(part);
        }
        self.note = note;
    }

    /// The note moves: its tracking error with it.
    pub fn retune(&mut self, note: u8) {
        self.note = note;
    }

    /// A fresh voice: no ramp from the last note's spread.
    pub fn hold(&mut self) {
        self.last = (1.0, 1.0);
    }

    /// Once a block at depth `d`. `d == 0` is `Spread::NONE` and the walk
    /// holds where it is.
    #[inline(never)]
    pub fn step(&mut self, d: f32) -> Spread {
        let last = self.last;
        if d == 0.0 {
            self.last = (1.0, 1.0);
            return Spread {
                cut: (last.0, 1.0),
                res: (last.1, 1.0),
            };
        }
        let x = xorshift(&mut self.rng);
        self.y1 += WALK_A * (x - self.y1);
        self.y2 += WALK_A * (self.y1 - self.y2);
        let w = clampf(WALK_G * self.y2, -1.0, 1.0);
        let cents = d * (self.fixed_cents() + DRIFT_CENTS * w);
        let to = (
            super::coef::exp2_poly(cents * (1.0 / 1200.0)),
            1.0 + d * RES_SPREAD * self.res,
        );
        self.last = to;
        let held = (to.0 - last.0).abs() <= HOLD * to.0 && (to.1 - last.1).abs() <= HOLD;
        let from = if held { to } else { last };
        Spread {
            cut: (from.0, to.0),
            res: (from.1, to.1),
        }
    }

    /// The walk before its clamp (tests).
    pub fn walk(&self) -> f32 {
        WALK_G * self.y2
    }

    /// The fixed cents at `d = 1`: the offset and the tracking error at
    /// the note.
    pub fn fixed_cents(&self) -> f32 {
        self.offset + self.track * (f32::from(self.note) - 60.0)
    }

    /// The RES draw, −1..1 (tests).
    pub fn res_draw(&self) -> f32 {
        self.res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::va::stability::walk_coefs;
    use crate::hw::{BLOCK_SIZE, SAMPLE_RATE};

    #[test]
    fn the_walk_constants_are_the_formulas() {
        let (a, g) = walk_coefs(SAMPLE_RATE as f32 / BLOCK_SIZE as f32, WALK_HZ, WALK_RMS);
        assert_eq!((WALK_A, WALK_G), (a, g));
    }

    fn part(i: usize) -> PartId {
        PartId::ALL[i]
    }

    fn chip(p: usize, slot: u32, note: u8) -> FilterStability {
        let mut c = FilterStability::NEW;
        c.note_on(part(p), slot, note);
        c
    }

    fn cents(r: f32) -> f32 {
        1200.0 * libm::log2f(r)
    }

    /// STAB 1 (d 0) is exactly neutral, block after block, so the 2040 and
    /// LADDER read their ends bit for bit.
    #[test]
    fn full_stability_is_exactly_neutral() {
        let mut c = chip(0, 3, 72);
        for _ in 0..1000 {
            assert_eq!(c.step(depth(1.0)), Spread::NONE);
        }
        let (g, k) = ((0.123_456_7f32, 0.5f32), (3.9f32, 4.0f32));
        assert_eq!(Spread::NONE.g(g), g);
        assert_eq!(Spread::NONE.k(k), k);
        // A k past RES 1's (a route past RES's top) is left where it is.
        assert_eq!(Spread::NONE.k((5.0, 5.0)), (5.0, 5.0));
    }

    /// The same Part and slot draw the same chip, note after note and
    /// chip after chip; a different slot or Part another.
    #[test]
    fn a_slot_is_the_same_chip() {
        let run = |p, slot| {
            let mut c = chip(p, slot, 60);
            [(); 500].map(|_| c.step(0.25))
        };
        assert_eq!(run(0, 2), run(0, 2));
        assert_ne!(run(0, 2), run(0, 3));
        assert_ne!(run(0, 2), run(1, 2));
        // A new note on the slot keeps its walk and its draws.
        let mut c = chip(0, 2, 60);
        let fixed = c.fixed_cents();
        c.step(0.25);
        let w = c.walk();
        c.note_on(part(0), 2, 60);
        assert_eq!((c.fixed_cents(), c.walk()), (fixed, w));
    }

    /// The fixed draws stay in range and spread out across a Part's slots:
    /// no two of eight within a twentieth of the range.
    #[test]
    fn the_draws_stay_in_range_and_spread() {
        for p in 0..PartId::ALL.len() {
            let mut offs = [0.0f32; crate::hw::MAX_VOICES];
            for slot in 0..crate::hw::MAX_VOICES as u32 {
                let c = chip(p, slot, 60);
                // At C4 the tracking error is 0: `fixed` is the offset.
                assert!(c.fixed_cents().abs() <= OFFSET_CENTS);
                assert!(c.res_draw().abs() <= 1.0);
                for n in [0u8, 127] {
                    let t = chip(p, slot, n).fixed_cents() - c.fixed_cents();
                    assert!(t.abs() <= 100.0 * TRACK * 67.0 + 1e-3, "{t}");
                }
                offs[slot as usize] = c.fixed_cents() / OFFSET_CENTS;
            }
            offs.sort_by(f32::total_cmp);
            for w in offs.windows(2) {
                assert!(w[1] - w[0] > 0.1, "Part {p}: {offs:?}");
            }
        }
    }

    /// At STAB ½ (the Prophet-like setting, d ¼): offsets within ±25
    /// cents, tracking within ±2 %, RES within ±3 %, the drift 6 cents RMS
    /// within a fifth, its peaks ±20 cents.
    #[test]
    fn the_prophet_setting_is_the_labs() {
        let d = depth(PROPHET);
        assert_eq!(d, 0.25);
        assert_eq!(d * OFFSET_CENTS, 25.0);
        assert_eq!(d * TRACK, 0.02);
        assert!((d * RES_SPREAD - 0.03).abs() < 1e-7);
        let mut c = chip(0, 1, 60);
        let fixed = d * c.fixed_cents();
        let n = 200_000;
        let (mut sum2, mut peak) = (0.0f64, 0.0f32);
        for _ in 0..n {
            let s = c.step(d);
            let drift = cents(s.cut.1) - fixed;
            sum2 += f64::from(drift * drift);
            peak = peak.max(drift.abs());
            assert!((s.res.1 - 1.0).abs() <= 0.03 + 1e-6);
        }
        let rms = (sum2 / n as f64).sqrt() as f32;
        assert!((rms - 6.0).abs() < 1.2, "drift RMS {rms} cents");
        assert!(peak <= 20.1, "drift peak {peak} cents");
    }

    /// No zipper: the drift alone steps a held block's cutoff by under
    /// 0.05 cent at STAB 0 (a pitch's just-noticeable step is ~3 cents),
    /// and the block holds its ends
    /// (the steady loop); a turn of STABILITY ramps from the last spread.
    #[test]
    fn the_drift_steps_far_below_audibility() {
        let mut c = chip(0, 4, 60);
        let mut last = c.step(1.0);
        let mut worst = 0.0f32;
        for _ in 0..200_000 {
            let s = c.step(1.0);
            assert_eq!(s.cut.0, s.cut.1, "held");
            assert_eq!(s.res.0, s.res.1);
            worst = worst.max((cents(s.cut.1) - cents(last.cut.1)).abs());
            last = s;
        }
        assert!(worst < 0.05, "a block's step {worst} cents");
        // STABILITY from 0 to ½ in a block: from the last spread.
        let s = c.step(depth(0.5));
        assert_eq!(s.cut.0, last.cut.1);
        assert_ne!(s.cut.0, s.cut.1);
        // Back to 1: one ramp to neutral, then neutral.
        let s1 = c.step(0.0);
        assert_eq!((s1.cut.0, s1.cut.1), (s.cut.1, 1.0));
        assert_eq!(c.step(0.0), Spread::NONE);
    }

    /// RES's spread never takes `k` past RES 1's.
    #[test]
    fn res_spread_holds_at_res_one() {
        let top = k2040_k(1.0);
        let s = Spread {
            cut: (1.0, 1.0),
            res: (1.12, 1.12),
        };
        assert_eq!(s.k((top, top)), (top, top));
        assert_eq!(s.k((1.0, 2.0)), (1.12, 2.24));
    }
}
