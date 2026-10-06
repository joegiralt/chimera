//! LEVEL (Filter 12), the filter's input level, Cascadia-style: it
//! attenuates below unity (spec § LEVEL, ADR 0096). It replaced DRIVE
//! (Filter 2, retired), which translates to it on load.

use crate::block::{Block, ParamId};
use crate::dsp::{clampf, fast_exp2};
use crate::params::FilterParams;
use crate::storage::Retired;

/// DRIVE's retired id: read from old files, never written or reused.
const DRIVE: ParamId = ParamId(2);

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
/// DRIVE becomes LEVEL. Such a file has no LEVEL, as the firmware never
/// writes DRIVE again; one holding only FM, ENV or KEY is untouched.
pub fn translate_drive(r: &Retired, b: &mut dyn Block) {
    if let Some(d) = r.get(DRIVE) {
        b.set(FilterParams::LEVEL, level_from_drive(d));
    }
}
