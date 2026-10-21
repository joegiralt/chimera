//! LEVEL (Filter 12), the filter's input level, Cascadia-style: it
//! attenuates below unity (spec § LEVEL, ADR 0096). It replaced DRIVE
//! (Filter 2, retired), which translates to it on load.

use crate::block::Block;
use crate::dsp::ease::Gate;
use crate::dsp::{clampf, fast_exp2};
use crate::hw::BLOCK_SIZE;
use crate::meter::Clip;
use crate::params::FilterParams;
use crate::storage::{FILTER_DRIVE, Retired};
use crate::taps::TapLane;

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
#[derive(Clone, Copy, Debug, Default)]
pub struct FilterMods<'a> {
    /// A route moves LEVEL: its gain may cross unity inside the block.
    pub level_routed: bool,
    /// The FREQ taps (FM3, spec § 6): ±4 octaves at 100 %, per sample.
    pub freq: Option<TapLane<'a>>,
    /// The Q taps (QM): RES moved by the lane, per sample.
    pub q: Option<TapLane<'a>>,
    /// Only a knob moves `g` and `k` (no route or tap does): a glide's
    /// block steps them `STEPS` times, each step on the steady loop, not
    /// per sample (ADR 0117).
    pub glide: bool,
    /// The 2040's and LADDER's STABILITY this block (ADR 0168).
    pub spread: super::Spread,
    /// FB, the filter's own output into its `g` (#393).
    pub fb: super::Fed,
}

impl FilterMods<'_> {
    /// Nothing routed or tapped.
    pub const NONE: FilterMods<'static> = FilterMods {
        level_routed: false,
        freq: None,
        q: None,
        glide: false,
        spread: super::Spread::NONE,
        fb: super::Fed::OFF,
    };
}

/// One block's LEVEL gain, `(from, to)`, and its gate: open while the gain
/// tops unity at either end or a route moves LEVEL (spec § LEVEL), never on
/// the DRIVE stage. Only `new` builds one, so the gate always matches its
/// gain. CLEAN and DIRTY clip through it (`LevelStage`); the 2040 takes only
/// the gain, into its stages, whose ±1 V clip is its clipper and its LED.
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
pub fn clip_amount(peak: f32) -> Clip {
    Clip::new((peak - 1.0) * 0.5)
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

/// LEVEL's gain and its clipper: `sat_c1`, transparent to ±1, faded in
/// by its `Gate` (20 ms) when the gate opens and out when it shuts, so
/// crossing unity never steps. A fresh one (`Default`) lands where its
/// gate is: nothing came before it to fade from.
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelStage {
    gate: Gate,
    /// The last block's `max|x·gain|` while the clipper ran; 0 while off.
    peak: f32,
    /// The clipper runs next block even if the gate shuts: it is open or
    /// still fading out.
    running: bool,
}

impl LevelStage {
    /// `buf × gain`, ramped per sample; through the clipper, blended by
    /// the fade, while the gate is open or the fade is falling. The ramps
    /// step by adding, not `g_at`'s convert and multiply: a block's last
    /// sample lands within an ulp or so of its end.
    #[inline(always)]
    pub fn run(&mut self, buf: &mut [f32; BLOCK_SIZE], lv: LevelRamp) {
        // 0 and 1 are exact shares, so the held cases match exactly.
        let (f0, f1) = self.gate.step(lv.open);
        self.running = f1 > 0.0;
        let (mut g, g1) = lv.gain;
        let n = BLOCK_SIZE as f32;
        let g_step = (g1 - g) / n;
        if (f0, f1) == (0.0, 0.0) {
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
        if (f0, f1) == (1.0, 1.0) {
            // Faded in: the clipper alone, the common held case.
            for s in buf.iter_mut() {
                g += g_step;
                let y = *s * g;
                peak = peak.max(y.abs());
                *s = clipped(y);
            }
        } else {
            let (mut f, f_step) = (f0, (f1 - f0) / n);
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
    pub fn clip(&self) -> Clip {
        clip_amount(self.peak)
    }

    /// The clipper is open or fading out, so it runs next block whatever
    /// the gate says: what the bill reads (spec § Cost, "while it runs").
    pub fn running(&self) -> bool {
        self.running
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
