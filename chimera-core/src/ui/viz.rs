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

/// BigViz plot area: curves between `PLOT_TOP` and `PLOT_BASE`.
pub const PLOT_TOP: i32 = 40;
pub const PLOT_BASE: i32 = 170;
/// Filter pass band (the mockup's 0 dB line); resonance peaks above it.
pub const FILTER_PASS_Y: i32 = 72;

/// Filter response y at column `t` (0..1 across the plot): flat pass band,
/// a resonance bump at `cutoff`, then roll-off to the base (the pre-refresh
/// curve's shape).
pub fn filter_y(t: f32, cutoff: f32, reso: f32) -> i32 {
    let dist = (t - cutoff) * 6.0;
    let peak_h = (FILTER_PASS_Y - PLOT_TOP) as f32 * reso;
    let y = if dist < -0.5 {
        FILTER_PASS_Y as f32
    } else if dist < 0.5 {
        let peak = libm::cosf(dist * core::f32::consts::PI) * 0.5 + 0.5;
        FILTER_PASS_Y as f32 - peak_h * peak
    } else {
        let rolloff = (dist - 0.5).min(4.0) / 4.0;
        FILTER_PASS_Y as f32 + (PLOT_BASE - FILTER_PASS_Y) as f32 * rolloff
    };
    (y as i32).clamp(PLOT_TOP, PLOT_BASE)
}

/// Columns `x0..=x1`: soft fill from the curve point `y(x)` down to `base`,
/// then the curve as an accent line (2 px: see the module doc comment).
fn filled_curve<D>(d: &mut D, x0: i32, x1: i32, base: i32, y: impl Fn(i32) -> i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for x in x0..=x1 {
        draw::fill_rect(d, x, y(x) + 1, 1, base - y(x) - 1, theme::ACCENT_SOFT);
    }
    for x in x0 + 1..=x1 {
        draw::line(d, x - 1, y(x - 1), x, y(x), theme::ACCENT, 1);
        draw::line(d, x - 1, y(x - 1) + 1, x, y(x) + 1, theme::ACCENT, 1);
    }
}

/// The touched value riding on a viz: label above, value in the readout
/// face, to the right of (x, y) or to its left when it would not fit.
pub fn readout<D>(d: &mut D, x: i32, y: i32, label: &str, value: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = draw::text_width(&theme::FONT_READOUT, value, 0).max(draw::text_width(&theme::FONT_LABEL, label, theme::LABEL_TRACKING));
    let left = if x + 8 + w <= theme::VIZ_RIGHT { x + 8 } else { x - 8 - w };
    let vy = (y + 2).clamp(PLOT_TOP + 20, PLOT_BASE - 2);
    draw::text_tracked(d, &theme::FONT_LABEL, label, left + 1, vy - 24, theme::MID, theme::LABEL_TRACKING);
    draw::text(d, &theme::FONT_READOUT, value, left, vy, theme::INK);
}

/// Filter: response with a soft fill, a faint pass-band line, and a marker
/// at the cutoff carrying `readout` (the focused slot's label and value).
pub fn filter<D>(d: &mut D, cutoff: f32, reso: f32, readout_text: Option<(&str, &str)>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32;
    let y = |x: i32| filter_y((x - theme::VIZ_LEFT) as f32 / w, cutoff, reso);
    filled_curve(d, theme::VIZ_LEFT, theme::VIZ_RIGHT, PLOT_BASE, y);
    draw::fill_rect(d, theme::VIZ_LEFT, FILTER_PASS_Y, theme::VIZ_RIGHT - theme::VIZ_LEFT, 1, theme::FAINT);
    let mx = theme::VIZ_LEFT + (w * cutoff.clamp(0.0, 1.0)) as i32;
    let my = y(mx);
    let mut dy = my + 6;
    while dy < PLOT_BASE {
        draw::fill_rect(d, mx, dy, 1, 2.min(PLOT_BASE - dy), theme::INK);
        dy += 5;
    }
    draw::dot(d, mx, my, 4, theme::INK);
    if let Some((label, value)) = readout_text {
        readout(d, mx, my, label, value);
    }
}

/// Envelope: four segments over proportional `widths`, breakpoints at
/// `heights` (0..1), stage labels below; segment `lit` (the one the focused
/// slot edits) in the accent.
pub fn envelope<D>(d: &mut D, widths: &[f32; 4], heights: &[f32; 5], labels: &[&str; 4], lit: Option<usize>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let base = PLOT_BASE - 8;
    let (x0, w, h) = (theme::VIZ_LEFT, (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32, (base - PLOT_TOP) as f32);
    let mut pts = [(0i32, 0i32); 5];
    let mut cx = x0 as f32;
    for i in 0..5 {
        pts[i] = (cx as i32, base - (h * heights[i].clamp(0.0, 1.0)) as i32);
        if i < 4 {
            cx += w * widths[i];
        }
    }
    let y_at = |x: i32| {
        let s = (0..4).find(|&s| x <= pts[s + 1].0).unwrap_or(3);
        let ((xa, ya), (xb, yb)) = (pts[s], pts[s + 1]);
        if xb == xa { yb } else { ya + (yb - ya) * (x - xa) / (xb - xa) }
    };
    for x in x0..=pts[4].0 {
        draw::fill_rect(d, x, y_at(x) + 1, 1, base - y_at(x) - 1, theme::ACCENT_SOFT);
    }
    draw::fill_rect(d, x0, base, theme::VIZ_RIGHT - x0, 1, theme::FAINT);
    for s in 0..4 {
        let ((xa, ya), (xb, yb)) = (pts[s], pts[s + 1]);
        if lit == Some(s) {
            // 2-px accent (spec's 1.5-px line; the display has no
            // anti-aliasing — binding ruling, Task 6): two adjacent 1-px
            // strokes, offset down by one row so the extra pixel stays
            // inside the band.
            draw::line(d, xa, ya, xb, yb, theme::ACCENT, 1);
            draw::line(d, xa, ya + 1, xb, yb + 1, theme::ACCENT, 1);
        } else {
            draw::line(d, xa, ya, xb, yb, theme::INK2, 1);
        }
        // A label wider than its segment is left out, unless it is the lit one.
        let fits = draw::text_width(&theme::FONT_LABEL, labels[s], theme::LABEL_TRACKING) + 2 <= xb - xa;
        if lit == Some(s) {
            draw::text_center(d, &theme::FONT_LABEL, labels[s], (xa + xb) / 2, base + 14, theme::ACCENT, theme::LABEL_TRACKING);
        } else if fits {
            draw::text_center(d, &theme::FONT_LABEL, labels[s], (xa + xb) / 2, base + 14, theme::MID, theme::LABEL_TRACKING);
        }
    }
    for (i, &(x, y)) in pts.iter().enumerate() {
        let on_lit = lit.is_some_and(|s| i == s || i == s + 1);
        draw::dot(d, x, y, 2, if on_lit { theme::ACCENT } else { theme::INK2 });
    }
}

/// Compressor transfer curve (knee at 60 %, 0.3 above it) over a faint 1:1 line.
pub fn compressor<D>(d: &mut D)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x0, x1) = (theme::VIZ_LEFT + 30, theme::VIZ_RIGHT - 30);
    let (w, h) = ((x1 - x0) as f32, (PLOT_BASE - PLOT_TOP) as f32);
    draw::line(d, x0, PLOT_BASE, x1, PLOT_TOP, theme::FAINT, 1);
    let out = |t: f32| if t < 0.6 { t } else { 0.6 + (t - 0.6) * 0.3 };
    filled_curve(d, x0, x1, PLOT_BASE, |x| PLOT_BASE - (h * out((x - x0) as f32 / w)) as i32);
    draw::text(d, &theme::FONT_LABEL, "IN", x1 + 4, PLOT_BASE, theme::MID);
    draw::text(d, &theme::FONT_LABEL, "OUT", x0 - 20, PLOT_TOP + 8, theme::MID);
}
