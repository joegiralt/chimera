use embedded_graphics::Drawable;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable};
use embedded_graphics::text::Text;

use crate::ui::fmt::{self, FmtBuf};
use crate::ui::page::{CellIcon, ValFmt};
use crate::ui::theme;

/// Cell layout constants.
/// 3x2 grid filling the encoder zone (below header, above dungeon map).
const CELL_W: i32 = 76;
const CELL_H: i32 = 92;
const CELL_LEFT: i32 = 6;
const CELL_TOP: i32 = 22;
const CELL_PAD: i32 = 4;

/// Icon area within a cell.
const ICON_H: i32 = 52;
const ICON_TOP: i32 = 4;

/// Draw a single encoder cell with icon, label, value, and bar.
pub fn draw_cell<D>(
    display: &mut D,
    col: i32,
    row: i32,
    label: &str,
    value: f32,
    icon: CellIcon,
    val_fmt: ValFmt,
) where
    D: DrawTarget<Color = Rgb565>,
{
    if label == "--" {
        return;
    }

    let cx = CELL_LEFT + col * CELL_W;
    let cy = CELL_TOP + row * CELL_H;

    // Cell border (subtle)
    let _ = Rectangle::new(
        Point::new(cx, cy),
        Size::new((CELL_W - CELL_PAD) as u32, (CELL_H - CELL_PAD) as u32),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);

    // Icon area — quantize to 16 discrete frames (128 MIDI steps / 8)
    let ix = cx + 4;
    let iy = cy + ICON_TOP;
    let iw = CELL_W - CELL_PAD - 8;
    let ih = ICON_H;

    let quantized = libm::floorf(value * 16.0) / 16.0;
    draw_icon(display, icon, ix, iy, iw, ih, quantized);

    // Label + value below icon
    let text_y = cy + ICON_TOP + ICON_H + 8;
    let label_style = MonoTextStyle::new(&FONT_6X10, theme::PARAM_LABEL);
    let value_style = MonoTextStyle::new(&FONT_6X10, theme::PARAM_VALUE);

    let _ = Text::new(label, Point::new(cx + 4, text_y), label_style).draw(display);

    let mut buf = FmtBuf::new();
    fmt::fmt_val(&mut buf, value, val_fmt);
    let label_end = cx + 4 + label.len() as i32 * 6 + 4;
    let _ = Text::new(buf.as_str(), Point::new(label_end, text_y), value_style).draw(display);

    // Bar
    let bar_y = text_y + 5;
    let bar_w = CELL_W - CELL_PAD - 8;
    let _ = Rectangle::new(
        Point::new(cx + 4, bar_y),
        Size::new(bar_w as u32, theme::BAR_HEIGHT as u32),
    )
    .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_BG), display);

    let fill_w = (bar_w as f32 * value) as i32;
    if fill_w > 0 {
        let _ = Rectangle::new(
            Point::new(cx + 4, bar_y),
            Size::new(fill_w as u32, theme::BAR_HEIGHT as u32),
        )
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_FG), display);
    }
}

/// Draw a mini icon within the given bounds.
/// `val` is pre-quantized to 16 discrete steps.
fn draw_icon<D>(display: &mut D, icon: CellIcon, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    match icon {
        CellIcon::None => {}
        CellIcon::WaveClip => draw_icon_waveclip(display, x, y, w, h, val),
        CellIcon::ToneTilt => draw_icon_tone(display, x, y, w, h, val),
        CellIcon::DryWet => draw_icon_drywet(display, x, y, w, h, val),
        CellIcon::WaveShape => draw_icon_waveshape(display, x, y, w, h, val),
        CellIcon::PulseWidth => draw_icon_pulsewidth(display, x, y, w, h, val),
        CellIcon::Arc => draw_icon_arc(display, x, y, w, h, val),
        CellIcon::LevelBar => draw_icon_level(display, x, y, w, h, val),
        CellIcon::PanDot => draw_icon_pan(display, x, y, w, h, val),
        CellIcon::WaveFold => draw_icon_wavefold(display, x, y, w, h, val),
        CellIcon::Symmetry => draw_icon_symmetry(display, x, y, w, h, val),
        CellIcon::Ripple => draw_icon_ripple(display, x, y, w, h, val),
        CellIcon::Burst => draw_icon_burst(display, x, y, w, h, val),
        CellIcon::Orbit => draw_icon_orbit(display, x, y, w, h, val),
        CellIcon::Scatter => draw_icon_scatter(display, x, y, w, h, val),
        CellIcon::Breathe => draw_icon_breathe(display, x, y, w, h, val),
        CellIcon::Stack => draw_icon_stack(display, x, y, w, h, val),
        CellIcon::Bounce => draw_icon_bounce(display, x, y, w, h, val),
        CellIcon::Cube => draw_icon_cube(display, x, y, w, h, val),
    }
}

// ── Icon implementations ────────────────────────────────────────────

/// Sine morphing to clipped square.
fn draw_icon_waveclip<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let amp = h as f32 * 0.35;

    // Cubic threshold for smooth visual range
    let d = 1.0 - val;
    let threshold = 0.05 + d * d * d * 0.95;

    let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1);
    let segments = 24;
    let mut prev: Option<Point> = None;

    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let px = x + (w as f32 * t) as i32;
        let input = libm::sinf(t * core::f32::consts::PI * 2.0);

        let clipped = if input > threshold {
            threshold
        } else if input < -threshold {
            -threshold
        } else {
            input
        };
        let output = clipped / threshold;

        let py = mid_y - (amp * output) as i32;
        let pt = Point::new(px, py);
        if let Some(p) = prev {
            let _ = Line::new(p, pt).draw_styled(&stroke, display);
        }
        prev = Some(pt);
    }
}

/// Tone tilt: line pivoting from flat to tilted.
fn draw_icon_tone<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;

    // Dead zone at center: values within 1/15 of 0.5 snap to flat
    let offset = val - 0.5;
    let tilt = if libm::fabsf(offset) < 0.04 {
        0.0
    } else {
        offset * h as f32 * 0.6
    };

    let _ = Line::new(
        Point::new(x + 4, mid_y + tilt as i32),
        Point::new(x + w - 4, mid_y - tilt as i32),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2), display);

    // Center dot
    let _ = Rectangle::new(Point::new(x + w / 2 - 1, mid_y - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);
}

/// Dry/wet: two overlapping arcs or a blend bar.
fn draw_icon_drywet<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let mid_x = x + w / 2;

    // Two vertical bars: DRY (dim) and WET (bright), height proportional
    let bar_w = 8;
    let gap = 12;
    let max_h = h - 12;

    let dry_h = (max_h as f32 * (1.0 - val)) as i32;
    let wet_h = (max_h as f32 * val) as i32;

    // DRY bar
    if dry_h > 0 {
        let _ = Rectangle::new(
            Point::new(mid_x - gap - bar_w, mid_y + max_h / 2 - dry_h),
            Size::new(bar_w as u32, dry_h as u32),
        )
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_BG), display);
    }

    // WET bar
    if wet_h > 0 {
        let _ = Rectangle::new(
            Point::new(mid_x + gap, mid_y + max_h / 2 - wet_h),
            Size::new(bar_w as u32, wet_h as u32),
        )
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_FG), display);
    }

    // Labels
    let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
    let _ = Text::new("D", Point::new(mid_x - gap - 4, y + h - 2), dim).draw(display);
    let _ = Text::new("W", Point::new(mid_x + gap + 2, y + h - 2), dim).draw(display);
}

/// Waveform shape: saw / square / tri morphing.
fn draw_icon_waveshape<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let amp = h as f32 * 0.32;
    let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1);

    // val: 0=saw, 0.5=square, 1.0=tri
    let pw = w;

    if val < 0.33 {
        // Sawtooth
        let _ = Line::new(
            Point::new(x, mid_y + amp as i32),
            Point::new(x + pw, mid_y - amp as i32),
        )
        .draw_styled(&stroke, display);
        let _ = Line::new(
            Point::new(x + pw, mid_y - amp as i32),
            Point::new(x + pw, mid_y + amp as i32),
        )
        .draw_styled(&stroke, display);
    } else if val < 0.66 {
        // Square
        let half = pw / 2;
        let _ = Line::new(
            Point::new(x, mid_y - amp as i32),
            Point::new(x + half, mid_y - amp as i32),
        )
        .draw_styled(&stroke, display);
        let _ = Line::new(
            Point::new(x + half, mid_y - amp as i32),
            Point::new(x + half, mid_y + amp as i32),
        )
        .draw_styled(&stroke, display);
        let _ = Line::new(
            Point::new(x + half, mid_y + amp as i32),
            Point::new(x + pw, mid_y + amp as i32),
        )
        .draw_styled(&stroke, display);
    } else {
        // Triangle
        let q = pw / 4;
        let _ = Line::new(Point::new(x, mid_y), Point::new(x + q, mid_y - amp as i32))
            .draw_styled(&stroke, display);
        let _ = Line::new(
            Point::new(x + q, mid_y - amp as i32),
            Point::new(x + 3 * q, mid_y + amp as i32),
        )
        .draw_styled(&stroke, display);
        let _ = Line::new(
            Point::new(x + 3 * q, mid_y + amp as i32),
            Point::new(x + pw, mid_y),
        )
        .draw_styled(&stroke, display);
    }
}

/// Pulse width: square wave with variable duty.
fn draw_icon_pulsewidth<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let amp = h as f32 * 0.32;
    let duty = (w as f32 * (0.1 + val * 0.8)) as i32;
    let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1);

    // Up edge
    let _ = Line::new(
        Point::new(x, mid_y + amp as i32),
        Point::new(x, mid_y - amp as i32),
    )
    .draw_styled(&stroke, display);
    // High
    let _ = Line::new(
        Point::new(x, mid_y - amp as i32),
        Point::new(x + duty, mid_y - amp as i32),
    )
    .draw_styled(&stroke, display);
    // Down edge
    let _ = Line::new(
        Point::new(x + duty, mid_y - amp as i32),
        Point::new(x + duty, mid_y + amp as i32),
    )
    .draw_styled(&stroke, display);
    // Low
    let _ = Line::new(
        Point::new(x + duty, mid_y + amp as i32),
        Point::new(x + w, mid_y + amp as i32),
    )
    .draw_styled(&stroke, display);

    // Duty marker line
    let _ = Line::new(
        Point::new(x + duty, mid_y - amp as i32 - 4),
        Point::new(x + duty, mid_y - amp as i32 - 4),
    )
    .draw_styled(
        &PrimitiveStyle::with_stroke(theme::ACCENT_BRIGHT, 1),
        display,
    );
}

/// Arc/knob indicator: partial circle fill.
fn draw_icon_arc<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2;
    let r = (w.min(h) / 2 - 4) as f32;

    // 16-segment arc, 270° sweep from 135° to 405°.
    // Precomputed cos/sin for each of 17 arc vertices.
    const ARC_N: usize = 16;
    static ARC_COS: [f32; 17] = [
        -0.707, -0.924, -1.000, -0.924, -0.707, -0.383, 0.000, 0.383, 0.707, 0.924, 1.000, 0.924,
        0.707, 0.383, 0.000, -0.383, -0.707,
    ];
    static ARC_SIN: [f32; 17] = [
        0.707, 0.383, 0.000, -0.383, -0.707, -0.924, -1.000, -0.924, -0.707, -0.383, 0.000, 0.383,
        0.707, 0.924, 1.000, 0.924, 0.707,
    ];

    let bg_stroke = PrimitiveStyle::with_stroke(theme::PARAM_BAR_BG, 2);
    let fg_stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);
    let active_segments = (ARC_N as f32 * val) as usize;

    for i in 0..ARC_N {
        let x0 = cx + (r * ARC_COS[i]) as i32;
        let y0 = cy + (r * ARC_SIN[i]) as i32;
        let x1 = cx + (r * ARC_COS[i + 1]) as i32;
        let y1 = cy + (r * ARC_SIN[i + 1]) as i32;
        let style = if i < active_segments {
            &fg_stroke
        } else {
            &bg_stroke
        };
        let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1)).draw_styled(style, display);
    }

    // End dot
    if active_segments > 0 {
        let dx = cx + (r * ARC_COS[active_segments]) as i32;
        let dy = cy + (r * ARC_SIN[active_segments]) as i32;
        let _ = Rectangle::new(Point::new(dx - 1, dy - 1), Size::new(3, 3))
            .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);
    }
}

/// Vertical level bar (mixer volume).
fn draw_icon_level<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let bar_w = 12;
    let bx = x + (w - bar_w) / 2;
    let bar_h = h - 8;
    let by = y + 4;

    // Background
    let _ = Rectangle::new(Point::new(bx, by), Size::new(bar_w as u32, bar_h as u32))
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_BG), display);

    // Fill from bottom
    let fill_h = (bar_h as f32 * val) as i32;
    if fill_h > 0 {
        let _ = Rectangle::new(
            Point::new(bx, by + bar_h - fill_h),
            Size::new(bar_w as u32, fill_h as u32),
        )
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_FG), display);
    }
}

/// Pan position dot on L-R line.
fn draw_icon_pan<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let lx = x + 6;
    let rx = x + w - 6;

    // L-R line
    let _ = Line::new(Point::new(lx, mid_y), Point::new(rx, mid_y)).draw_styled(
        &PrimitiveStyle::with_stroke(theme::PARAM_BAR_BG, 1),
        display,
    );

    // Center tick
    let cx = (lx + rx) / 2;
    let _ = Line::new(Point::new(cx, mid_y - 3), Point::new(cx, mid_y + 3))
        .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

    // Pan dot
    let pos = lx + ((rx - lx) as f32 * val) as i32;
    let _ = Rectangle::new(Point::new(pos - 2, mid_y - 2), Size::new(5, 5))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);

    // L / R labels
    let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
    let _ = Text::new("L", Point::new(lx - 2, mid_y + 16), dim).draw(display);
    let _ = Text::new("R", Point::new(rx - 2, mid_y + 16), dim).draw(display);
}

/// Wavefold: sine getting progressively folded.
fn draw_icon_wavefold<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let amp = h as f32 * 0.35;
    let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1);

    let gain = 1.0 + val * val * 4.0;
    let segments = 24;
    let mut prev: Option<Point> = None;

    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let px = x + (w as f32 * t) as i32;
        let input = libm::sinf(t * core::f32::consts::PI * 2.0);
        let driven = input * gain;
        let folded = fold_wave(driven);

        let py = mid_y - (amp * folded) as i32;
        let pt = Point::new(px, py);
        if let Some(p) = prev {
            let _ = Line::new(p, pt).draw_styled(&stroke, display);
        }
        prev = Some(pt);
    }
}

/// Symmetry: wave with bias indicator.
fn draw_icon_symmetry<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid_y = y + h / 2;
    let offset = val - 0.5;
    let bias = if libm::fabsf(offset) < 0.04 {
        0.0
    } else {
        offset * h as f32 * 0.4
    };
    let biased_mid = mid_y - bias as i32;

    // Center line (shows zero)
    let _ = Line::new(Point::new(x + 4, mid_y), Point::new(x + w - 4, mid_y))
        .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

    // Biased center (bright)
    let _ = Line::new(
        Point::new(x + 4, biased_mid),
        Point::new(x + w - 4, biased_mid),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1), display);

    // Arrow showing offset direction
    if (val - 0.5).abs() > 0.05 {
        let arrow_x = x + w / 2;
        let _ = Line::new(Point::new(arrow_x, mid_y), Point::new(arrow_x, biased_mid))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::ACCENT_DIM, 1), display);
        // Arrowhead
        let _ = Rectangle::new(Point::new(arrow_x - 1, biased_mid - 1), Size::new(3, 3))
            .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);
    }
}

// ── Helper: draw a circle from line segments ────────────────────────

/// Precomputed sin/cos for 12-segment polygon (circle approximation).
/// Covers 0°, 30°, 60°, ... 330°, and wraps to 360° = 0°.
const RING_SEGMENTS: usize = 12;
static RING_COS: [f32; 13] = [
    1.0, 0.866, 0.5, 0.0, -0.5, -0.866, -1.0, -0.866, -0.5, 0.0, 0.5, 0.866, 1.0,
];
static RING_SIN: [f32; 13] = [
    0.0, 0.5, 0.866, 1.0, 0.866, 0.5, 0.0, -0.5, -0.866, -1.0, -0.866, -0.5, 0.0,
];

fn draw_ellipse<D>(display: &mut D, cx: i32, cy: i32, rx: f32, ry: f32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    for i in 0..RING_SEGMENTS {
        let _ = Line::new(
            Point::new(
                cx + (rx * RING_COS[i]) as i32,
                cy + (ry * RING_SIN[i]) as i32,
            ),
            Point::new(
                cx + (rx * RING_COS[i + 1]) as i32,
                cy + (ry * RING_SIN[i + 1]) as i32,
            ),
        )
        .draw_styled(&stroke, display);
    }
}

fn draw_ring<D>(display: &mut D, cx: i32, cy: i32, r: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_ellipse(display, cx, cy, r as f32, r as f32, color);
}

// ── Ripple: concentric circles expanding from center ────────────────
// Frame 0: empty. Frame 1: dot. Frame 2+: rings expanding outward.
// Older rings fade dimmer. Newest ring is brightest.

fn draw_icon_ripple<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2;
    let max_r = w.min(h) / 2 - 2;
    let frame = (val * 15.0) as i32;

    if frame == 0 {
        return;
    }

    // Center dot (always present once started)
    let _ = Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);

    if frame == 1 {
        return;
    }

    // Up to 4 concentric rings, expanding outward
    let num_rings = ((frame - 1) as usize).min(4);
    let ring_spacing = max_r / 5;

    for i in 0..num_rings {
        // Outermost ring = newest = brightest
        let age = num_rings - 1 - i; // 0 = newest
        let r = ring_spacing * (i as i32 + 1) + (frame - 2);
        let r = r.min(max_r);

        let brightness = match age {
            0 => theme::VIZ_LINE,
            1 => theme::ACCENT_DIM,
            2 => Rgb565::new(0, 16, 8),
            _ => theme::VIZ_GRID,
        };

        if r > 3 {
            draw_ring(display, cx, cy, r, brightness);
        }
    }
}

// ── Burst: rays from center ─────────────────────────────────────────
// Frame 0: dot. Frame 15: long rays filling the cell.

fn draw_icon_burst<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2;
    let max_r = (w.min(h) / 2 - 2) as f32;

    // Center dot
    let _ = Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);

    if val < 0.05 {
        return;
    }

    let num_rays = 8;
    let ray_len = max_r * val;
    let inner_r = 3.0;

    let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1);

    for i in 0..num_rays {
        let angle = core::f32::consts::PI * 2.0 * i as f32 / num_rays as f32;
        let cos_a = libm::cosf(angle);
        let sin_a = libm::sinf(angle);

        let _ = Line::new(
            Point::new(cx + (inner_r * cos_a) as i32, cy + (inner_r * sin_a) as i32),
            Point::new(cx + (ray_len * cos_a) as i32, cy + (ray_len * sin_a) as i32),
        )
        .draw_styled(&stroke, display);
    }
}

// ── Orbit: dot circling center ──────────────────────────────────────
// Frame position determines where the dot sits on the orbit path.
// Higher values = larger orbit radius.

fn draw_icon_orbit<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2;
    let max_r = (w.min(h) / 2 - 4) as f32;

    // Orbit path (dim ring)
    let orbit_r = (max_r * 0.7) as i32;
    draw_ring(display, cx, cy, orbit_r, theme::VIZ_GRID);

    // Center dot
    let _ = Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_BG), display);

    // Orbiting dot — position along circle, speed increases with value
    // Use val to set angular position (wraps around)
    let angle = val * core::f32::consts::PI * 2.0 * 3.0; // 3 full rotations across range
    let r = orbit_r as f32;
    let dx = cx + (r * libm::cosf(angle)) as i32;
    let dy = cy + (r * libm::sinf(angle)) as i32;

    let _ = Rectangle::new(Point::new(dx - 2, dy - 2), Size::new(5, 5))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);

    // Trail — previous position (dimmer)
    let trail_angle = angle - 0.4;
    let tx = cx + (r * libm::cosf(trail_angle)) as i32;
    let ty = cy + (r * libm::sinf(trail_angle)) as i32;
    let _ = Rectangle::new(Point::new(tx - 1, ty - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_DIM), display);
}

// ── Scatter: dots spreading from center ─────────────────────────────
// Frame 0: single center dot. Frame 15: dots scattered across the cell.

fn draw_icon_scatter<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2;
    let max_r = (w.min(h) / 2 - 2) as f32;

    // Fixed "random" positions using a simple hash-like pattern
    // 8 dots at fixed angles, radius scales with val
    let num_dots = 8;
    let spread = max_r * val;

    // Center dot always
    let _ = Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);

    if val < 0.05 {
        return;
    }

    let dot_style = PrimitiveStyle::with_fill(theme::VIZ_LINE);

    for i in 0..num_dots {
        // Golden angle spacing for even distribution
        let angle = i as f32 * 2.399; // golden angle ≈ 137.5°
        // Vary radius per dot for organic feel
        let r_factor = 0.5 + 0.5 * libm::sinf(i as f32 * 1.7 + 0.3);
        let r = spread * r_factor;

        let dx = cx + (r * libm::cosf(angle)) as i32;
        let dy = cy + (r * libm::sinf(angle)) as i32;

        // Dots get smaller at edges
        if r_factor > 0.7 {
            let _ = Rectangle::new(Point::new(dx - 1, dy - 1), Size::new(3, 3))
                .draw_styled(&dot_style, display);
        } else {
            let _ = Rectangle::new(Point::new(dx, dy), Size::new(2, 2))
                .draw_styled(&dot_style, display);
        }
    }
}

// ── Breathe: circle pulsing in size ─────────────────────────────────
// Frame 0: tiny dot. Frame 15: circle filling the cell.

fn draw_icon_breathe<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2;
    let max_r = w.min(h) / 2 - 2;

    let r = (max_r as f32 * val).max(1.0) as i32;

    if r <= 2 {
        // Tiny dot
        let _ = Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(3, 3))
            .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);
    } else {
        // Circle outline
        draw_ring(display, cx, cy, r, theme::VIZ_LINE);
        // Center dot
        let _ = Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(3, 3))
            .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_DIM), display);
    }
}

// ── Stack: horizontal lines accumulating ────────────────────────────
// Frame 0: empty. Frame 15: full stack of lines.

fn draw_icon_stack<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let max_lines = 8;
    let num_lines = (val * max_lines as f32) as i32;
    let line_gap = h / (max_lines + 1);
    let margin = 6;

    let stroke_dim = PrimitiveStyle::with_stroke(theme::PARAM_BAR_BG, 1);
    let stroke_lit = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);

    // Draw all slots (dim)
    for i in 0..max_lines {
        let ly = y + h - (i + 1) * line_gap;
        let _ = Line::new(Point::new(x + margin, ly), Point::new(x + w - margin, ly))
            .draw_styled(&stroke_dim, display);
    }

    // Fill from bottom (bright)
    for i in 0..num_lines {
        let ly = y + h - (i + 1) * line_gap;
        let _ = Line::new(Point::new(x + margin, ly), Point::new(x + w - margin, ly))
            .draw_styled(&stroke_lit, display);
    }
}

// ── Cube: isometric 3D cube that fills from bottom to top ───────────
// val 0.0: empty wireframe cube
// val 1.0: fully filled cube

fn draw_icon_cube<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let cy = y + h / 2 + 4;

    // Isometric axes (pixel offsets for one cube edge)
    let rx = 14_i32; // right-axis x component
    let ry = 7_i32; // right-axis y component (goes down)
    let lx = -14_i32; // left-axis x
    let ly = 7_i32; // left-axis y (goes down)
    let cube_h = 24_i32; // vertical height in pixels

    // 7 visible vertices of the isometric cube
    // Bottom front (lowest visible point)
    let bf = (cx, cy);
    let br = (cx + rx, cy + ry); // bottom right
    let bl = (cx + lx, cy + ly); // bottom left
    let bk = (cx + rx + lx, cy + ry + ly); // bottom back

    let tf = (cx, cy - cube_h); // top front
    let tr = (cx + rx, cy + ry - cube_h); // top right
    let tl = (cx + lx, cy + ly - cube_h); // top left
    let tk = (cx + rx + lx, cy + ry + ly - cube_h); // top back

    // Fill: scan lines on the left and right visible faces
    let fill_h = (cube_h as f32 * val) as i32;

    // Bottom face (always visible — the floor of the cube)
    {
        let bot_stroke = PrimitiveStyle::with_stroke(theme::VIZ_FILL, 1);
        // Fill the diamond: bf-br-bk-bl by scanning lines from bf->bk direction
        for i in 0..=ry {
            let t = i as f32 / ry as f32;
            // Scan from the bf-br edge toward the bl-bk edge
            let x0 = bf.0 + (rx as f32 * t) as i32;
            let y0 = bf.1 + (ry as f32 * t) as i32;
            let x1 = bl.0 + (rx as f32 * t) as i32;
            let y1 = bl.1 + (ry as f32 * t) as i32;
            let _ =
                Line::new(Point::new(x0, y0), Point::new(x1, y1)).draw_styled(&bot_stroke, display);
        }
    }

    // Fill: draw horizontal lines from bottom of cube up to fill level.
    // Each horizontal line spans the full width of the cube at that Y.
    // The cube's left edge runs from bl to tl, right edge from br to tr,
    // front edge from bf to tf. We interpolate X positions at each Y.
    if fill_h > 0 {
        let fill_stroke = PrimitiveStyle::with_stroke(theme::VIZ_FILL, 1);

        for row in 0..fill_h {
            // t = how far up the cube we are (0 = bottom, 1 = top)

            // Front edge X at this height (straight vertical, so always cx)
            let front_x = bf.0;
            // Left edge X at this height: interpolate from bl.x to tl.x
            // (they're the same since vertical edges are straight up)
            let left_x = bl.0;
            // Right edge X
            let right_x = br.0;

            // Y position: the front edge goes from bf.1 up to tf.1
            let front_y = bf.1 - row;
            // Left edge Y: goes from bl.1 up to tl.1
            let left_y = bl.1 - row;
            // Right edge Y
            let right_y = br.1 - row;

            // Left face: line from front to left at this height
            let _ = Line::new(Point::new(front_x, front_y), Point::new(left_x, left_y))
                .draw_styled(&fill_stroke, display);

            // Right face: line from front to right at this height
            let _ = Line::new(Point::new(front_x, front_y), Point::new(right_x, right_y))
                .draw_styled(&fill_stroke, display);
        }

        // Liquid surface line (bright accent)
        let level_stroke = PrimitiveStyle::with_stroke(theme::ACCENT, 1);
        let _ = Line::new(
            Point::new(bl.0, bl.1 - fill_h),
            Point::new(bf.0, bf.1 - fill_h),
        )
        .draw_styled(&level_stroke, display);
        let _ = Line::new(
            Point::new(bf.0, bf.1 - fill_h),
            Point::new(br.0, br.1 - fill_h),
        )
        .draw_styled(&level_stroke, display);
    }

    // Top face fill when nearly full
    if val > 0.92 {
        let top_stroke = PrimitiveStyle::with_stroke(theme::VIZ_FILL, 1);
        for i in 0..=ly {
            let t = i as f32 / ly as f32;
            let x0 = tf.0 + (lx as f32 * t) as i32;
            let y0 = tf.1 + i;
            let x1 = tr.0 + (lx as f32 * t) as i32;
            let y1 = tr.1 + i;
            let _ =
                Line::new(Point::new(x0, y0), Point::new(x1, y1)).draw_styled(&top_stroke, display);
        }
    }

    // Wireframe edges
    let wire = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 1);
    let wire_dim = PrimitiveStyle::with_stroke(theme::NODE_INACTIVE_BORDER, 1);

    // Bottom edges (front two visible)
    let _ =
        Line::new(Point::new(bf.0, bf.1), Point::new(br.0, br.1)).draw_styled(&wire_dim, display);
    let _ =
        Line::new(Point::new(bf.0, bf.1), Point::new(bl.0, bl.1)).draw_styled(&wire_dim, display);

    // Back bottom edges (dim)
    let _ =
        Line::new(Point::new(bl.0, bl.1), Point::new(bk.0, bk.1)).draw_styled(&wire_dim, display);
    let _ =
        Line::new(Point::new(br.0, br.1), Point::new(bk.0, bk.1)).draw_styled(&wire_dim, display);

    // Vertical edges
    let _ = Line::new(Point::new(bf.0, bf.1), Point::new(tf.0, tf.1)).draw_styled(&wire, display);
    let _ = Line::new(Point::new(br.0, br.1), Point::new(tr.0, tr.1)).draw_styled(&wire, display);
    let _ = Line::new(Point::new(bl.0, bl.1), Point::new(tl.0, tl.1)).draw_styled(&wire, display);
    let _ =
        Line::new(Point::new(bk.0, bk.1), Point::new(tk.0, tk.1)).draw_styled(&wire_dim, display);

    // Top edges
    let _ = Line::new(Point::new(tf.0, tf.1), Point::new(tr.0, tr.1)).draw_styled(&wire, display);
    let _ = Line::new(Point::new(tf.0, tf.1), Point::new(tl.0, tl.1)).draw_styled(&wire, display);
    let _ = Line::new(Point::new(tl.0, tl.1), Point::new(tk.0, tk.1)).draw_styled(&wire, display);
    let _ = Line::new(Point::new(tr.0, tr.1), Point::new(tk.0, tk.1)).draw_styled(&wire, display);
}

// ── Bounce: ball moves vertically, squashes at extremes ─────────────
// val 0.0 (-64): squashed at bottom
// val 0.5 (0):   round, centered
// val 1.0 (+63): squashed at top

fn draw_icon_bounce<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32, val: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = x + w / 2;
    let pad = 6;
    let top = y + pad;
    let bot = y + h - pad;
    let range = (bot - top) as f32;
    let base_r = 10.0_f32;

    // Ball vertical position: 0=bottom, 0.5=center, 1=top
    let ball_y = bot as f32 - range * val;

    // Squash: 0 at center, 1 at extremes
    let dist = libm::fabsf(val - 0.5) * 2.0; // 0..1
    let squeeze = dist * dist; // quadratic for smooth feel

    // Ellipse radii
    let rx = base_r * (1.0 + squeeze * 0.8); // wider when squashed
    let ry = base_r * (1.0 - squeeze * 0.6); // shorter when squashed
    let ry = if ry < 3.0 { 3.0 } else { ry };

    // Floor line
    let _ = Line::new(Point::new(x + pad, bot), Point::new(x + w - pad, bot))
        .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

    // Ceiling line
    let _ = Line::new(Point::new(x + pad, top), Point::new(x + w - pad, top))
        .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

    // Shadow on contact surface when squashed
    if squeeze > 0.3 {
        let shadow_w = (rx * 1.2) as i32;
        let shadow_color = theme::ACCENT_DIM;
        if val < 0.5 {
            // Shadow on floor
            let _ = Line::new(
                Point::new(cx - shadow_w, bot),
                Point::new(cx + shadow_w, bot),
            )
            .draw_styled(&PrimitiveStyle::with_stroke(shadow_color, 2), display);
        } else {
            // Shadow on ceiling
            let _ = Line::new(
                Point::new(cx - shadow_w, top),
                Point::new(cx + shadow_w, top),
            )
            .draw_styled(&PrimitiveStyle::with_stroke(shadow_color, 2), display);
        }
    }

    let by = ball_y as i32;
    draw_ellipse(display, cx, by, rx, ry, theme::VIZ_LINE);

    // Highlight dot at ball center
    let _ = Rectangle::new(Point::new(cx - 1, by - 1), Size::new(3, 3))
        .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);
}

/// Triangle-fold waveshaping.
pub fn fold_wave(x: f32) -> f32 {
    let x = x + 1.0;
    let period = 4.0;
    let t = x - libm::floorf(x / period) * period;
    if t < 2.0 { t - 1.0 } else { 3.0 - t }
}
