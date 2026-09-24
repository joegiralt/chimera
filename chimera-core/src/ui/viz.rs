//! Direction A visualizations (ADR 0016): drawn as the main element with a
//! soft accent fill under a 1.5-px accent line (spec § Shared components);
//! drawn 2 px since the display has no anti-aliasing. No grid lines.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::scope::{self, SCOPE_LEN};
use crate::ui::draw;
use crate::ui::theme;

/// Columns of the viz band (x 12..=228).
pub const LIVE_COLS: usize = (theme::VIZ_RIGHT - theme::VIZ_LEFT + 1) as usize;

/// Live output as pixel offsets from the band's centre line, auto-scaled to
/// ±`VIZ_BAND_AMP` from the peak of the columns actually drawn (flat while
/// silent).
pub fn live_columns(buf: &[f32; SCOPE_LEN]) -> [i8; LIVE_COLS] {
    let peak = scope::peak(&buf[..LIVE_COLS]);
    let scale = if peak > scope::SOUNDING_PEAK { theme::VIZ_BAND_AMP as f32 / peak } else { 0.0 };
    core::array::from_fn(|i| libm::roundf(buf[i] * scale) as i8)
}

/// Cheap fingerprint of what `live_output` draws: the viz region redraws
/// only when it changes (a silent or frozen scope costs no SPI traffic).
pub fn live_key(buf: &[f32; SCOPE_LEN]) -> u32 {
    live_columns(buf).iter().fold(0x811c_9dc5u32, |h, &c| (h ^ c as u8 as u32).wrapping_mul(0x0100_0193))
}

/// The page's live output as a filled waveform in the viz band.
pub fn live_output<D>(d: &mut D, buf: &[f32; SCOPE_LEN])
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid = theme::VIZ_BAND_MID;
    let cols = live_columns(buf);
    let y = |i: usize| mid - cols[i] as i32;
    for (i, _) in cols.iter().enumerate() {
        let x = theme::VIZ_LEFT + i as i32;
        let (a, b) = if y(i) < mid { (y(i) + 1, mid) } else { (mid, y(i)) };
        draw::fill_rect(d, x, a, 1, b - a, theme::ACCENT_SOFT);
    }
    // 1.5-px line (spec § Shared components), rounded up to 2 px since the
    // display has no anti-aliasing: two adjacent 1-px strokes, offset down
    // by one row so the extra pixel stays inside the band (152±24, +1 ≤ 185).
    for i in 1..cols.len() {
        let x = theme::VIZ_LEFT + i as i32;
        draw::line(d, x - 1, y(i - 1), x, y(i), theme::ACCENT, 1);
        draw::line(d, x - 1, y(i - 1) + 1, x, y(i) + 1, theme::ACCENT, 1);
    }
}
