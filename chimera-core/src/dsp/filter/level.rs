//! LEVEL (Filter 12), the filter's input level, Cascadia-style: it
//! attenuates below unity (spec § LEVEL, ADR 0096). It replaced DRIVE
//! (Filter 2, retired), which translates to it on load.

use crate::block::Block;
use crate::dsp::{clampf, fast_exp2};
use crate::hw::BLOCK_SIZE;
use crate::params::FilterParams;
use crate::storage::{FILTER_DRIVE, Retired};

/// LEVEL's gain, `2^(4·LEVEL − 2)`: ×¼ at 0, exactly 1 at ½, ×4 at 1. One
/// `fast_exp2` a block; the gain ramps per sample.
#[inline]
pub fn level_gain(level: f32) -> f32 {
    fast_exp2(4.0 * level - 2.0)
}

/// The LEVEL whose gain is DRIVE's old pre-gain `1 + 4·DRIVE`: exact to
/// DRIVE 0.75, then held at LEVEL 1 (×4), so DRIVE 1 loads 1.94 dB
/// quieter. DRIVE is clamped to its old range first, so any finite value
/// lands in 0.5..=1. On the load path, not in audio.
pub fn level_from_drive(drive: f32) -> f32 {
    let d = clampf(drive, 0.0, 1.0);
    clampf((libm::log2f(1.0 + 4.0 * d) + 2.0) / 4.0, 0.0, 1.0)
}

/// The Filter block's translation (`storage::TRANSLATIONS`): a file's
/// DRIVE becomes LEVEL. The firmware never writes DRIVE again, so a file
/// holding both is crafted: its LEVEL wins. One holding only FM, ENV or
/// KEY is untouched.
pub fn translate_drive(r: &Retired, b: &mut dyn Block) {
    if let Some(d) = r.get(FILTER_DRIVE.id)
        && !r.held(FilterParams::LEVEL)
    {
        b.set(FilterParams::LEVEL, level_from_drive(d));
    }
}

/// What moves the filter besides its stored settings, read once a block.
/// Task 14 adds the tap lanes.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilterMods {
    /// A route moves LEVEL: its gain may cross unity inside the block.
    pub level_routed: bool,
}

impl FilterMods {
    /// Nothing routed.
    pub const NONE: FilterMods = FilterMods {
        level_routed: false,
    };
}

/// One block's LEVEL gain, `(from, to)`, and its gate: open while the gain
/// tops unity at either end or a route moves LEVEL (spec § LEVEL), never on
/// the DRIVE stage. Only `new` builds one, so the gate always matches its
/// gain. CLEAN and DIRTY clip through it (`LevelStage`); the 2040 feeds the
/// gain to its stages, whose ±1 V clip is its clipper, and gates its LED on
/// `open`.
#[derive(Clone, Copy, Debug)]
pub struct LevelRamp {
    gain: (f32, f32),
    open: bool,
}

impl LevelRamp {
    #[inline(always)]
    pub fn new(gain: (f32, f32), mods: &FilterMods) -> Self {
        let open = gain.0.max(gain.1) > 1.0 || mods.level_routed;
        Self { gain, open }
    }

    pub fn gain(self) -> (f32, f32) {
        self.gain
    }

    pub fn open(self) -> bool {
        self.open
    }
}

/// The LED's fill from a block's peak: 0 to ±1, full at ±3.
#[inline(always)]
pub fn clip_amount(peak: f32) -> f32 {
    clampf((peak - 1.0) * 0.5, 0.0, 1.0)
}

/// `sat_c1`'s knee, branch-free, as `(c, e, u)`: `c = clamp(y, ±1)`, the
/// excess `e = y − c`, and `u = clamp(e, ±1)`. Then `sat_c1(y) = c + u −
/// ½·u·|u|`. Four `vmaxnm`/`vminnm`, no divide, no compare.
#[inline(always)]
fn knee(y: f32) -> (f32, f32, f32) {
    let c = clampf(y, -1.0, 1.0);
    let e = y - c;
    (c, e, clampf(e, -1.0, 1.0))
}

/// `sat_c1(y)`, branch-free (`knee`).
#[inline(always)]
pub fn clipped(y: f32) -> f32 {
    let (c, _, u) = knee(y);
    c + u - 0.5 * u * u.abs()
}

/// `sat_c1(y) − y`, branch-free (`knee`): what the fade blends in.
#[inline(always)]
pub fn clip_delta(y: f32) -> f32 {
    let (_, e, u) = knee(y);
    u - e - 0.5 * u * u.abs()
}

/// Blocks the gate's fade takes: 20 ms at 64 samples, 48 kHz.
pub const FADE_BLOCKS: u8 = 15;

/// LEVEL's gain and its clipper: `sat_c1`, transparent to ±1, faded in
/// over `FADE_BLOCKS` when the gate opens and out when it shuts, so
/// crossing unity never steps. The fade counts whole blocks, so it lands
/// exactly on 0 and 1, and ramps per sample between them.
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelStage {
    /// Blocks into the fade, `0..=FADE_BLOCKS`.
    fade: u8,
    /// The last block's `max|x·gain|` while the clipper ran; 0 while off.
    peak: f32,
}

impl LevelStage {
    /// A fresh voice's stage: the clipper on or off from its first sample,
    /// with nothing before it to step from.
    pub fn start(&mut self, open: bool) {
        self.fade = if open { FADE_BLOCKS } else { 0 };
        self.peak = 0.0;
    }

    /// `buf × gain`, ramped per sample; through the clipper, blended by
    /// the fade, while the gate is open or the fade is falling. The ramps
    /// step by adding, not `g_at`'s convert and multiply: a block's last
    /// sample lands within an ulp or so of its end.
    #[inline(always)]
    pub fn run(&mut self, buf: &mut [f32; BLOCK_SIZE], lv: LevelRamp) {
        let from = self.fade;
        self.fade = if lv.open {
            (from + 1).min(FADE_BLOCKS)
        } else {
            from.saturating_sub(1)
        };
        let (mut g, g1) = lv.gain;
        let n = BLOCK_SIZE as f32;
        let g_step = (g1 - g) / n;
        if from == 0 && self.fade == 0 {
            self.peak = 0.0;
            if (g, g1) != (1.0, 1.0) {
                for s in buf.iter_mut() {
                    g += g_step;
                    *s *= g;
                }
            }
            return;
        }
        let mut peak = 0.0f32;
        if from == FADE_BLOCKS && self.fade == FADE_BLOCKS {
            // Faded in: the clipper alone, the common held case.
            for s in buf.iter_mut() {
                g += g_step;
                let y = *s * g;
                peak = peak.max(y.abs());
                *s = clipped(y);
            }
        } else {
            let mut f = f32::from(from) * (1.0 / FADE_BLOCKS as f32);
            let f_step = (f32::from(self.fade) * (1.0 / FADE_BLOCKS as f32) - f) / n;
            for s in buf.iter_mut() {
                (g, f) = (g + g_step, f + f_step);
                let y = *s * g;
                peak = peak.max(y.abs());
                *s = y + f * clip_delta(y);
            }
        }
        self.peak = peak;
    }

    /// The last block's `clip_amount`: 0 while the clipper is off.
    pub fn clip(&self) -> f32 {
        clip_amount(self.peak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filter::coef::sat_c1;

    /// `clipped` and `y + clip_delta(y)` are `sat_c1`, the filter's knee,
    /// everywhere.
    #[test]
    fn clip_delta_is_sat_c1() {
        let mut y = -4.0f32;
        while y < 4.0 {
            let (got, want) = (y + clip_delta(y), sat_c1(y));
            assert!((got - want).abs() <= 1e-6, "{y}: {got} vs {want}");
            assert!(
                (clipped(y) - want).abs() <= 1e-6,
                "{y}: {} vs {want}",
                clipped(y)
            );
            y += 0.00731;
        }
        assert_eq!(clip_delta(0.999), 0.0);
        assert_eq!(clipped(-0.999), -0.999);
    }
}
