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
/// face, to the right of `x` (or its left, when it would not fit). Placed
/// above the highest point `curve` reaches across the text's own columns —
/// clear of the curve, not just of the anchor — or, when the curve is too
/// close to `PLOT_TOP` to fit the block above it, below the curve's lowest
/// point there instead, inside the fill.
pub fn readout<D>(d: &mut D, x: i32, curve: impl Fn(i32) -> i32, label: &str, value: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    // Measured glyph extents relative to each line's own baseline
    // (FONT_LABEL, FONT_READOUT): the label's ink spans baseline-8..baseline;
    // the value's spans baseline-21..baseline-1. The value sits on `vy`; the
    // label sits `LABEL_RISE` above it.
    const LABEL_RISE: i32 = 24;
    const BLOCK_TOP: i32 = LABEL_RISE + 8;
    const GAP: i32 = 3;

    let w = draw::text_width(&theme::FONT_READOUT, value, 0).max(draw::text_width(&theme::FONT_LABEL, label, theme::LABEL_TRACKING));
    let left = if x + 8 + w <= theme::VIZ_RIGHT { x + 8 } else { x - 8 - w };

    // The curve's highest and lowest point across the columns the text will
    // actually occupy — not just at the anchor, which can be far from flat
    // this close to a resonant peak.
    let (lo, hi) = (left.max(theme::VIZ_LEFT), (left + w).min(theme::VIZ_RIGHT));
    let (mut top, mut bottom) = (curve(lo), curve(lo));
    for cx in lo..=hi {
        let cy = curve(cx);
        top = top.min(cy);
        bottom = bottom.max(cy);
    }
    let bottom = bottom + 1; // the curve's 2-px accent line draws one row below `y(x)` too

    let vy = if top - GAP - BLOCK_TOP >= PLOT_TOP { top - GAP } else { bottom + GAP + BLOCK_TOP };
    let vy = vy.clamp(PLOT_TOP + BLOCK_TOP, PLOT_BASE - 2);

    draw::text_tracked(d, &theme::FONT_LABEL, label, left + 1, vy - LABEL_RISE, theme::MID, theme::LABEL_TRACKING);
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
        readout(d, mx, y, label, value);
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
    }
    let xs = pts.map(|p| p.0);
    for (s, span) in stage_label_spans(&xs, labels, lit).iter().enumerate() {
        if let Some((left, _)) = *span {
            let color = if lit == Some(s) { theme::ACCENT } else { theme::MID };
            draw::text_tracked(d, &theme::FONT_LABEL, labels[s], left, base + 14, color, theme::LABEL_TRACKING);
        }
    }
    for (i, &(x, y)) in pts.iter().enumerate() {
        let on_lit = lit.is_some_and(|s| i == s || i == s + 1);
        draw::dot(d, x, y, 2, if on_lit { theme::ACCENT } else { theme::INK2 });
    }
}

/// Horizontal gap kept between two stage labels.
const STAGE_LABEL_GAP: i32 = 2;

/// Where each stage label of `envelope` goes, as `(left, right)` columns
/// (inclusive), centred under its segment `xs[s]..xs[s + 1]`; `None` = left
/// out. The lit stage's label is always drawn. Any other is drawn only when
/// it fits its own segment and keeps `STAGE_LABEL_GAP` from every label
/// already placed (the lit one first), so no two labels ever touch.
pub fn stage_label_spans(xs: &[i32; 5], labels: &[&str; 4], lit: Option<usize>) -> [Option<(i32, i32)>; 4] {
    let span = |s: usize| {
        let w = draw::text_width(&theme::FONT_LABEL, labels[s], theme::LABEL_TRACKING);
        let left = (xs[s] + xs[s + 1]) / 2 - w / 2;
        (w, (left, left + w - 1))
    };
    let mut spans = [None; 4];
    if let Some(s) = lit.filter(|&s| s < 4) {
        spans[s] = Some(span(s).1);
    }
    for s in (0..4).filter(|&s| lit != Some(s)) {
        let (w, (l, r)) = span(s);
        let fits = w + 2 <= xs[s + 1] - xs[s];
        let clear = spans.iter().flatten().all(|&(pl, pr)| r + STAGE_LABEL_GAP < pl || pr + STAGE_LABEL_GAP < l);
        if fits && clear {
            spans[s] = Some((l, r));
        }
    }
    spans
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

/// One Part in the Mixer overview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// 0..1
    pub level: f32,
    /// −1 (left) .. 1 (right)
    pub pan: f32,
}

/// Bar x of Part `i` in the overview.
pub fn strip_x(i: usize) -> i32 {
    20 + i as i32 * 36
}
pub const STRIP_NUM_Y: i32 = 128;
pub const STRIP_TOP: i32 = 132;
pub const STRIP_H: i32 = 38;
pub const STRIP_PAN_Y: i32 = 177;

/// Mixer PART viz band: each Part's level bar and pan dot, `selected` lit.
pub fn parts_overview<D>(d: &mut D, strips: &[Strip], selected: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut num = crate::ui::fmt::FmtBuf::new();
    for (i, s) in strips.iter().enumerate() {
        let (x, sel) = (strip_x(i), i == selected);
        num.clear();
        let _ = core::fmt::Write::write_fmt(&mut num, format_args!("{}", i + 1));
        let (font, color) = if sel { (&theme::FONT_LABEL_BOLD, theme::INK) } else { (&theme::FONT_LABEL, theme::MID) };
        draw::text_center(d, font, num.as_str(), x + 4, STRIP_NUM_Y, color, 0);
        draw::fill_rect(d, x, STRIP_TOP, 8, STRIP_H, theme::FAINT);
        let h = (STRIP_H as f32 * s.level.clamp(0.0, 1.0) + 0.5) as i32;
        let fill = if sel { theme::ACCENT } else if s.level > 0.0 { theme::BAR_REST } else { theme::FAINT };
        draw::fill_rect(d, x, STRIP_TOP + STRIP_H - h, 8, h, fill);
        draw::fill_rect(d, x - 6, STRIP_PAN_Y, 20, 1, theme::FAINT);
        let px = x + 4 + libm::roundf(s.pan.clamp(-1.0, 1.0) * 10.0) as i32;
        draw::dot(d, px, STRIP_PAN_Y, 2, if sel { theme::INK } else { theme::MID });
    }
}

/// The FX flow's node labels, in the Mixer chain's order.
pub const FX_NODES: [&str; 5] = ["IN", "CHR", "DLY", "REV", "OUT"];
pub const FLOW_Y: i32 = 146;
pub const FLOW_SEND_Y: i32 = 176;

/// FM algorithm topologies 0..=7 (the pre-refresh diagrams, as data):
/// operator 1..4 positions as (x in half steps from centre, row), the
/// modulation edges (from, to), and the carriers (bit n-1 = operator n).
// Algorithms 2 and 3 are repositioned from the pre-refresh diagram so every
// edge reads unambiguously: no edge line passes near an unrelated node
// (alg 2's old layout put operator 2 almost on the 4→1 line), and every
// edge runs strictly downward, modulator above target (alg 3's old layout
// put operators 2 and 3 on the same row, which hid the (3, 2) direction).
const ALG_POS: [[(i8, i8); 4]; 8] = [
    [(0, 3), (0, 2), (0, 1), (0, 0)],
    [(0, 2), (0, 1), (-1, 0), (1, 0)],
    [(0, 2), (-1, 1), (-1, 0), (1, 1)],
    [(0, 3), (1, 2), (-1, 1), (-1, 0)],
    [(-1, 1), (-1, 0), (1, 1), (1, 0)],
    [(-2, 1), (0, 1), (2, 1), (0, 0)],
    [(-2, 1), (0, 1), (2, 1), (2, 0)],
    [(-3, 0), (-1, 0), (1, 0), (3, 0)],
];
// Edges verified against `dsp::engine_fm::FmEngine::render`'s routing per
// algorithm, not just the pre-refresh diagram: algorithm 3 (TX81Z ALG4)
// forks operator 3's own output into operator 1's modulation input in
// addition to operator 3 feeding operator 2 (`(3, 2)`, not `(4, 2)` as the
// render() match's own inline comment claims) — see the GitHub issue on
// ALG 4 routing.
const ALG_EDGES: [&[(u8, u8)]; 8] = [
    &[(4, 3), (3, 2), (2, 1)],
    &[(3, 2), (4, 2), (2, 1)],
    &[(3, 2), (2, 1), (4, 1)],
    &[(4, 3), (3, 2), (3, 1), (2, 1)],
    &[(2, 1), (4, 3)],
    &[(4, 1), (4, 2), (4, 3)],
    &[(4, 3)],
    &[],
];
const ALG_CARRIERS: [u8; 8] = [0b0001, 0b0001, 0b0001, 0b0001, 0b0101, 0b0111, 0b0111, 0b1111];
const ALG_STEP_X: i32 = 20;
const ALG_STEP_Y: i32 = 17;
/// Drawn radius of an operator node; also the minimum clearance an edge
/// keeps from any node that isn't one of its own endpoints (`+2`, tested).
pub const ALG_OP_R: i32 = 7;

/// Centre of operator `op` (0-based) in algorithm `alg`, in the viz band.
pub fn alg_op_center(alg: u8, op: usize) -> (i32, i32) {
    let a = (alg as usize).min(7);
    let rows = ALG_POS[a].iter().map(|p| p.1).max().unwrap_or(0) as i32 + 1;
    let top = theme::VIZ_BAND_MID - (rows - 1) * ALG_STEP_Y / 2;
    let (hx, row) = ALG_POS[a][op];
    (theme::SCREEN_W / 2 + hx as i32 * ALG_STEP_X, top + row as i32 * ALG_STEP_Y)
}

/// The algorithm's modulation edges, `(from, to)`, both 1-based operator numbers.
pub fn alg_edges(alg: u8) -> &'static [(u8, u8)] {
    ALG_EDGES[(alg as usize).min(7)]
}

/// FM algorithm page and operator page: the algorithm's operators and
/// edges; carriers filled, modulators as rings. `selected` lights an
/// operator in the accent colour — the FM operator page passes the operator
/// being edited; the FM algorithm page edits no single operator, so it
/// passes `None` (accent is reserved for the active element).
/// Edges are drawn in `theme::MID` (non-accent grey): the visualization
/// accent-line width rule doesn't apply to them, so they stay 1 px.
pub fn fm_algorithm<D>(d: &mut D, alg: u8, selected: Option<usize>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let a = (alg as usize).min(7);
    for &(from, to) in ALG_EDGES[a] {
        let (x0, y0) = alg_op_center(alg, from as usize - 1);
        let (x1, y1) = alg_op_center(alg, to as usize - 1);
        draw::line(d, x0, y0, x1, y1, theme::MID, 1);
    }
    for (op, label) in ["1", "2", "3", "4"].into_iter().enumerate() {
        let (x, y) = alg_op_center(alg, op);
        let carrier = ALG_CARRIERS[a] & (1 << op) != 0;
        if selected == Some(op) {
            draw::dot(d, x, y, ALG_OP_R, theme::ACCENT);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x + 1, y + 4, theme::BG, 0);
        } else if carrier {
            draw::dot(d, x, y, ALG_OP_R, theme::INK2);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x + 1, y + 4, theme::BG, 0);
        } else {
            draw::dot(d, x, y, ALG_OP_R, theme::BG);
            draw::ring(d, x, y, ALG_OP_R, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, label, x + 1, y + 4, theme::MID, 0);
        }
    }
}

/// FX pages and SENDS: IN → CHR → DLY → REV → OUT on a line (the
/// pre-refresh flow diagram, restyled like the map). `lit` (0 = CHR) is the
/// page's effect, or on SENDS the focused send; `sends` shows each send level
/// under its effect.
pub fn effects_flow<D>(d: &mut D, lit: Option<usize>, sends: Option<[f32; 3]>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let n = FX_NODES.len();
    draw::fill_rect(d, theme::MAP_X0, FLOW_Y, theme::MAP_X1 - theme::MAP_X0, 1, theme::FAINT);
    for (i, label) in FX_NODES.iter().enumerate() {
        let x = crate::ui::dungeon_map::node_x(i, n);
        let fx = i.checked_sub(1).filter(|&k| k < 3);
        if fx.is_some() && fx == lit {
            draw::pill(d, x - theme::PILL_W / 2, FLOW_Y - theme::PILL_H / 2, theme::PILL_W, theme::PILL_H, theme::ACCENT);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x, FLOW_Y + 4, theme::BG, 0);
        } else {
            draw::dot(d, x, FLOW_Y, theme::NODE_R, theme::BG);
            draw::ring(d, x, FLOW_Y, theme::NODE_R, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, label, x, FLOW_Y + 18, theme::MID, 0);
        }
        if let (Some(k), Some(levels)) = (fx, sends) {
            let fill = if lit == Some(k) { theme::ACCENT } else { theme::BAR_REST };
            draw::bar(d, x - 14, FLOW_SEND_Y, 28, 2, levels[k], false, theme::FAINT, fill);
        }
    }
}
