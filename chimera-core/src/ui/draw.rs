//! Direction A drawing primitives (ADR 0016): text in the u8g2 faces, thin
//! bars, arc gauges, pills, dots and rings. No outline boxes. Every function
//! ignores draw errors (the targets are infallible framebuffers).

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{AngleUnit, Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{
    Arc, Circle, CornerRadii, Line, PrimitiveStyle, Rectangle, RoundedRectangle, StyledDrawable,
};
use u8g2_fonts::FontRenderer;
use u8g2_fonts::types::{FontColor, VerticalPosition};

/// Draw `s` with its baseline at `y`; returns the advance in pixels.
pub fn text<D>(d: &mut D, font: &FontRenderer, s: &str, x: i32, y: i32, color: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    font.render(
        s,
        Point::new(x, y),
        VerticalPosition::Baseline,
        FontColor::Transparent(color),
        d,
    )
    .map_or(0, |dims| dims.advance.x)
}

/// Draw `s` with `tracking` extra pixels after each glyph; returns the advance.
pub fn text_tracked<D>(
    d: &mut D,
    font: &FontRenderer,
    s: &str,
    x: i32,
    y: i32,
    color: Rgb565,
    tracking: i32,
) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut cx = x;
    for ch in s.chars() {
        let adv = font
            .render(
                ch,
                Point::new(cx, y),
                VerticalPosition::Baseline,
                FontColor::Transparent(color),
                d,
            )
            .map_or(0, |dims| dims.advance.x);
        cx += adv + tracking;
    }
    cx - x
}

/// Advance of `s` in `font` (with `tracking` after each glyph).
pub fn text_width(font: &FontRenderer, s: &str, tracking: i32) -> i32 {
    let adv = font
        .get_rendered_dimensions(s, Point::zero(), VerticalPosition::Baseline)
        .map_or(0, |dims| dims.advance.x);
    adv + tracking * s.chars().count() as i32
}

/// Draw `s` ending at `right` (exclusive).
pub fn text_right<D>(
    d: &mut D,
    font: &FontRenderer,
    s: &str,
    right: i32,
    y: i32,
    color: Rgb565,
    tracking: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let w = text_width(font, s, tracking);
    text_tracked(d, font, s, right - w, y, color, tracking);
}

/// Draw `s` centred on `cx`.
pub fn text_center<D>(
    d: &mut D,
    font: &FontRenderer,
    s: &str,
    cx: i32,
    y: i32,
    color: Rgb565,
    tracking: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let w = text_width(font, s, tracking);
    text_tracked(d, font, s, cx - w / 2, y, color, tracking);
}

pub fn fill_rect<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if w > 0 && h > 0 {
        let _ = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
            .draw_styled(&PrimitiveStyle::with_fill(color), d);
    }
}

pub fn line<D>(d: &mut D, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb565, width: u32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1))
        .draw_styled(&PrimitiveStyle::with_stroke(color, width), d);
}

/// A viz accent line: the spec's 1.5-px line rounded up to 2 px, since the
/// display has no anti-aliasing. Two adjacent 1-px strokes, the second one
/// row below, so the extra pixel stays inside the band under the curve.
pub fn thick_line<D>(d: &mut D, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    line(d, x0, y0, x1, y1, color, 1);
    line(d, x0, y0 + 1, x1, y1 + 1, color, 1);
}

/// Filled circle of radius `r` centred on (cx, cy). A negative `r` draws
/// nothing (rather than clamping to 0, which would still draw a 1px dot).
pub fn dot<D>(d: &mut D, cx: i32, cy: i32, r: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if r < 0 {
        return;
    }
    let _ = Circle::with_center(Point::new(cx, cy), (2 * r + 1) as u32)
        .draw_styled(&PrimitiveStyle::with_fill(color), d);
}

/// Circle outline of radius `r`. A negative `r` draws nothing.
pub fn ring<D>(d: &mut D, cx: i32, cy: i32, r: i32, color: Rgb565, width: u32)
where
    D: DrawTarget<Color = Rgb565>,
{
    if r < 0 {
        return;
    }
    let _ = Circle::with_center(Point::new(cx, cy), (2 * r + 1) as u32)
        .draw_styled(&PrimitiveStyle::with_stroke(color, width), d);
}

/// Filled rounded rectangle with fully round ends (radius h/2). A
/// non-positive `w` or `h` draws nothing, the same guard `fill_rect` uses —
/// otherwise `w`/`h` (and the radius derived from `h`) would cast a
/// negative value to a huge `u32`.
pub fn pill<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if w <= 0 || h <= 0 {
        return;
    }
    let r = (h / 2) as u32;
    let _ = RoundedRectangle::new(
        Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)),
        CornerRadii::new(Size::new(r, r)),
    )
    .draw_styled(&PrimitiveStyle::with_fill(color), d);
}

/// Rounded-rectangle outline (the matrix cursor).
pub fn round_outline<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, radius: u32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if w <= 0 || h <= 0 {
        return;
    }
    let _ = RoundedRectangle::new(
        Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)),
        CornerRadii::new(Size::new(radius, radius)),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(color, 1), d);
}

/// Thin value bar: `track` over `w`, then the value in `fill`. Unipolar bars
/// grow from the left; bipolar bars grow from the centre. `value` is 0..1
/// (bipolar centre 0.5). The fill is at least 2 px so zero stays visible.
#[allow(clippy::too_many_arguments)]
pub fn bar<D>(
    d: &mut D,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    value: f32,
    bipolar: bool,
    track: Rgb565,
    fill: Rgb565,
) where
    D: DrawTarget<Color = Rgb565>,
{
    fill_rect(d, x, y, w, h, track);
    let v = value.clamp(0.0, 1.0);
    if bipolar {
        let c = x + w / 2;
        let len = ((v - 0.5) * w as f32) as i32;
        let (x0, x1) = if len >= 0 { (c, c + len) } else { (c + len, c) };
        let x1 = x1.max(x0 + 2);
        fill_rect(d, x0, y, x1 - x0, h, fill);
    } else {
        let len = ((v * w as f32 + 0.5) as i32).max(2);
        fill_rect(d, x, y, len, h, fill);
    }
}

/// 270° gauge from 7:30 clockwise to 4:30 (the mockup's 0.75π..2.25π).
/// Track in `track`; the value arc in `fill` from the start (unipolar) or
/// from 12:00 (bipolar). Round caps.
#[allow(clippy::too_many_arguments)]
pub fn arc_gauge<D>(
    d: &mut D,
    cx: i32,
    cy: i32,
    r: i32,
    width: u32,
    value: f32,
    bipolar: bool,
    track: Rgb565,
    fill: Rgb565,
) where
    D: DrawTarget<Color = Rgb565>,
{
    const START: f32 = 135.0;
    const SWEEP: f32 = 270.0;
    let v = value.clamp(0.0, 1.0);
    let dia = (2 * r + 1) as u32;
    let center = Point::new(cx, cy);
    let stroke = |c| PrimitiveStyle::with_stroke(c, width);
    let _ = Arc::with_center(center, dia, START.deg(), SWEEP.deg()).draw_styled(&stroke(track), d);
    let end = START + SWEEP * v;
    let (from, to) = if bipolar {
        let mid = START + SWEEP / 2.0;
        if end >= mid { (mid, end) } else { (end, mid) }
    } else {
        (START, end.max(START + 1.0))
    };
    let _ =
        Arc::with_center(center, dia, from.deg(), (to - from).deg()).draw_styled(&stroke(fill), d);
    let cap = (width as i32) / 2;
    for a in [from, to] {
        let rad = a.to_radians();
        let px = cx + libm::roundf(r as f32 * libm::cosf(rad)) as i32;
        let py = cy + libm::roundf(r as f32 * libm::sinf(rad)) as i32;
        dot(d, px, py, cap, fill);
    }
}

/// A right arrow ("→") at the text baseline in the label size; returns its width.
pub fn arrow<D>(d: &mut D, x: i32, y: i32, color: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let my = y - 3;
    line(d, x, my, x + 7, my, color, 1);
    line(d, x + 5, my - 2, x + 7, my, color, 1);
    line(d, x + 5, my + 2, x + 7, my, color, 1);
    8
}
