use embedded_graphics::Drawable;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable};
use embedded_graphics::text::Text;

use crate::params::ParamSnapshot;
use crate::ui::animation::AnimatedValue;
use crate::ui::block_def::{BlockDef, VizType};
use crate::ui::cell;
use crate::ui::chain::ChainNav;
use crate::ui::dungeon_map;
use crate::ui::fmt::{self, FmtBuf};
use crate::ui::page::{PageId, PageLayout};
use crate::ui::perf::PerfStats;
use crate::ui::region::RegionKind;
use crate::ui::theme;

use core::fmt::Write;

/// Full-screen renderer. Composites header, visualization, parameters, and dungeon map.
pub struct Renderer {
    /// Animated display values for the 6 encoders (normalized 0..1).
    pub anim: [AnimatedValue; 6],
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            anim: [AnimatedValue::new(0.5); 6],
        }
    }

    pub fn update(&mut self, page: PageId, params: &ParamSnapshot) {
        let values = page.read_values(params);
        for (a, &v) in self.anim.iter_mut().zip(values.iter()) {
            a.set_target(v);
            a.update();
        }
    }

    pub fn snap_to_current(&mut self, page: PageId, params: &ParamSnapshot) {
        let values = page.read_values(params);
        for (a, &v) in self.anim.iter_mut().zip(values.iter()) {
            a.snap(v);
        }
    }

    // ── Modal / Physical Modeling ───────────────────────────────────

    fn draw_modal_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT;
        let x1 = theme::VIZ_RIGHT;
        let y0 = theme::VIZ_TOP + 12;
        let y1 = theme::VIZ_BOTTOM - 12;
        let w = x1 - x0;
        let h = y1 - y0;
        let mid_y = y0 + h / 2;

        // Baseline
        let _ = Line::new(Point::new(x0, mid_y), Point::new(x1, mid_y))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

        let excite = self.anim[0].current();
        let decay = self.anim[1].current();
        let bright = self.anim[4].current();

        // Draw modal resonance peaks — series of decaying sine peaks
        let num_modes = 3 + (bright * 5.0) as i32; // 3-8 modes based on brightness
        let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);

        for mode in 0..num_modes {
            let mode_x = x0 + (w * (mode + 1)) / (num_modes + 1);
            let peak_h =
                (h as f32 * 0.4 * excite * (1.0 - mode as f32 * decay * 0.1).max(0.1)) as i32;

            // Draw a peak shape: 3 line segments
            let pw = 8 + (4.0 * (1.0 - bright)) as i32; // peak width
            let _ = Line::new(
                Point::new(mode_x - pw, mid_y),
                Point::new(mode_x, mid_y - peak_h),
            )
            .draw_styled(&stroke, display);
            let _ = Line::new(
                Point::new(mode_x, mid_y - peak_h),
                Point::new(mode_x + pw, mid_y),
            )
            .draw_styled(&stroke, display);
        }

        // Label
        let _ = Text::new(
            "MODES",
            Point::new(x0, y1 + 10),
            MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM),
        )
        .draw(display);
    }

    // ── VA / Analog ─────────────────────────────────────────────────

    fn draw_va_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT + 20;
        let x1 = theme::VIZ_RIGHT - 20;
        let y0 = theme::VIZ_TOP + 16;
        let y1 = theme::VIZ_BOTTOM - 16;
        let w = x1 - x0;
        let h = y1 - y0;
        let mid_y = y0 + h / 2;

        // Baseline
        let _ = Line::new(Point::new(x0, mid_y), Point::new(x1, mid_y))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

        let wave = self.anim[0].current(); // wave shape: 0=saw, 0.5=square, 1=tri
        let pw = self.anim[1].current(); // pulse width

        let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);
        let periods = 2;

        // Draw 2 periods of the waveform
        let period_w = w / periods;
        for p in 0..periods {
            let px = x0 + p * period_w;

            if wave < 0.33 {
                // Sawtooth: ramp up, drop
                let _ = Line::new(
                    Point::new(px, mid_y + h / 3),
                    Point::new(px + period_w - 2, mid_y - h / 3),
                )
                .draw_styled(&stroke, display);
                let _ = Line::new(
                    Point::new(px + period_w - 2, mid_y - h / 3),
                    Point::new(px + period_w, mid_y + h / 3),
                )
                .draw_styled(&stroke, display);
            } else if wave < 0.66 {
                // Square/pulse with variable width
                let duty = (period_w as f32 * (0.2 + pw * 0.6)) as i32;
                let _ = Line::new(Point::new(px, mid_y + h / 3), Point::new(px, mid_y - h / 3))
                    .draw_styled(&stroke, display);
                let _ = Line::new(
                    Point::new(px, mid_y - h / 3),
                    Point::new(px + duty, mid_y - h / 3),
                )
                .draw_styled(&stroke, display);
                let _ = Line::new(
                    Point::new(px + duty, mid_y - h / 3),
                    Point::new(px + duty, mid_y + h / 3),
                )
                .draw_styled(&stroke, display);
                let _ = Line::new(
                    Point::new(px + duty, mid_y + h / 3),
                    Point::new(px + period_w, mid_y + h / 3),
                )
                .draw_styled(&stroke, display);
            } else {
                // Triangle
                let half = period_w / 2;
                let _ = Line::new(Point::new(px, mid_y), Point::new(px + half, mid_y - h / 3))
                    .draw_styled(&stroke, display);
                let _ = Line::new(
                    Point::new(px + half, mid_y - h / 3),
                    Point::new(px + period_w, mid_y),
                )
                .draw_styled(&stroke, display);
            }
        }
    }

    // ── Drive ───────────────────────────────────────────────────────
    // Shows a sine wave being progressively saturated/crushed by drive amount.

    fn draw_drive_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT;
        let x1 = theme::VIZ_RIGHT;
        let y0 = theme::VIZ_TOP + 12;
        let y1 = theme::VIZ_BOTTOM - 12;
        let w = x1 - x0;
        let h = y1 - y0;
        let mid_y = y0 + h / 2;

        // Baseline
        let _ = Line::new(Point::new(x0, mid_y), Point::new(x1, mid_y))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

        let drive = self.anim[0].current();

        // Cubic curve so clipping develops gradually across the full range.
        // drive=0 -> threshold=1.0 (no clip), drive=1 -> threshold=0.05 (full square)
        let d = 1.0 - drive;
        let threshold = 0.05 + d * d * d * 0.95;

        let segments = 48;
        let ghost = PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1);
        let mut prev_clean: Option<Point> = None;
        let mut prev_driven: Option<Point> = None;

        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let px = x0 + (w as f32 * t) as i32;
            let input = libm::sinf(t * core::f32::consts::PI * 4.0);

            // Clean sine (ghost)
            let clean_y = mid_y - (h as f32 * 0.38 * input) as i32;
            let clean_pt = Point::new(px, clean_y);
            if let Some(p) = prev_clean {
                let _ = Line::new(p, clean_pt).draw_styled(&ghost, display);
            }
            prev_clean = Some(clean_pt);

            // Hard clip at threshold, rescale to fill amplitude
            let clipped = if input > threshold {
                threshold
            } else if input < -threshold {
                -threshold
            } else {
                input
            };
            let driven = clipped / threshold;
            let driven_y = mid_y - (h as f32 * 0.38 * driven) as i32;
            let driven_pt = Point::new(px, driven_y);
            if let Some(p) = prev_driven {
                let _ = Line::new(p, driven_pt)
                    .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2), display);
            }
            prev_driven = Some(driven_pt);
        }
    }

    // ── Filter ──────────────────────────────────────────────────────

    fn draw_filter_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT;
        let x1 = theme::VIZ_RIGHT;
        let y0 = theme::VIZ_TOP + 8;
        let y1 = theme::VIZ_BOTTOM - 8;
        let h = y1 - y0;
        let w = x1 - x0;

        // Grid lines
        for i in 0..4 {
            let gy = y0 + (h * i) / 3;
            let _ = Line::new(Point::new(x0, gy), Point::new(x1, gy))
                .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);
        }

        let cutoff = self.anim[0].current();
        let reso = self.anim[1].current();

        let cx = x0 + (w as f32 * cutoff) as i32;
        let peak_height = (h as f32 * 0.3 * reso) as i32;

        let segments = 32;
        let mut prev = Point::new(x0, y0 + 4);

        for i in 1..=segments {
            let t = i as f32 / segments as f32;
            let px = x0 + (w as f32 * t) as i32;

            let dist = (t - cutoff) * 6.0;
            let y = if dist < -0.5 {
                y0 + 4
            } else if dist < 0.5 {
                let peak = libm::cosf(dist * core::f32::consts::PI) * 0.5 + 0.5;
                y0 + 4 - (peak_height as f32 * peak) as i32
            } else {
                let rolloff = (dist - 0.5).min(4.0) / 4.0;
                y0 + 4 + (h as f32 * 0.8 * rolloff) as i32
            };

            let y = y.max(y0).min(y1);
            let curr = Point::new(px, y);

            let _ = Line::new(prev, curr)
                .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2), display);
            prev = curr;
        }

        // Cutoff marker
        let _ = Line::new(Point::new(cx, y0), Point::new(cx, y1))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::ACCENT_DIM, 1), display);
    }

    // ── Wavefolder ──────────────────────────────────────────────────

    fn draw_folder_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT + 20;
        let x1 = theme::VIZ_RIGHT - 20;
        let y0 = theme::VIZ_TOP + 16;
        let y1 = theme::VIZ_BOTTOM - 16;
        let w = x1 - x0;
        let h = y1 - y0;
        let mid_y = y0 + h / 2;

        // Baseline
        let _ = Line::new(Point::new(x0, mid_y), Point::new(x1, mid_y))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

        let fold = self.anim[0].current();
        let sym = self.anim[1].current();

        // Folded sine wave
        let segments = 48;
        let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);
        let mut prev: Option<Point> = None;

        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let px = x0 + (w as f32 * t) as i32;

            // Input sine
            let input = libm::sinf(t * core::f32::consts::PI * 4.0);

            // Apply symmetry bias
            let biased = input + (sym - 0.5) * 0.5;

            // Fold: gain ramps from 1x to 5x using quadratic curve
            // so the folding develops gradually across the full 0-100% range
            let gain = 1.0 + fold * fold * 4.0;
            let driven = biased * gain;
            let folded = cell::fold_wave(driven);

            let py = mid_y - (h as f32 * 0.4 * folded) as i32;
            let curr = Point::new(px, py.max(y0).min(y1));

            if let Some(p) = prev {
                let _ = Line::new(p, curr).draw_styled(&stroke, display);
            }
            prev = Some(curr);
        }
    }

    // ── Envelope (ADSR) ─────────────────────────────────────────────

    fn draw_envelope_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT;
        let x1 = theme::VIZ_RIGHT;
        let y0 = theme::VIZ_TOP + 8;
        let y1 = theme::VIZ_BOTTOM - 8;
        let w = x1 - x0;
        let h = y1 - y0;

        // Baseline
        let _ = Line::new(Point::new(x0, y1), Point::new(x1, y1))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

        let atk = self.anim[0].current().max(0.02);
        let dec = self.anim[1].current().max(0.02);
        let sus = self.anim[2].current();
        let rel = self.anim[3].current().max(0.02);

        // Proportional widths
        let total = atk + dec + 0.3 + rel; // sustain gets fixed width
        let atk_w = (w as f32 * atk / total) as i32;
        let dec_w = (w as f32 * dec / total) as i32;
        let sus_w = (w as f32 * 0.3 / total) as i32;
        let rel_w = (w as f32 * rel / total) as i32;

        let sus_y = y1 - (h as f32 * sus) as i32;

        let p0 = Point::new(x0, y1);
        let p1 = Point::new(x0 + atk_w, y0);
        let p2 = Point::new(x0 + atk_w + dec_w, sus_y);
        let p3 = Point::new(x0 + atk_w + dec_w + sus_w, sus_y);
        let p4 = Point::new(x0 + atk_w + dec_w + sus_w + rel_w, y1);

        let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);
        let _ = Line::new(p0, p1).draw_styled(&stroke, display);
        let _ = Line::new(p1, p2).draw_styled(&stroke, display);
        let _ = Line::new(p2, p3).draw_styled(&stroke, display);
        let _ = Line::new(p3, p4).draw_styled(&stroke, display);

        // Breakpoint dots
        for &p in &[p0, p1, p2, p3, p4] {
            let _ = Rectangle::new(Point::new(p.x - 1, p.y - 1), Size::new(3, 3))
                .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_BRIGHT), display);
        }

        // Stage labels (tiny, below baseline)
        let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let _ = Text::new("A", Point::new(x0 + atk_w / 2 - 3, y1 + 12), dim).draw(display);
        let _ = Text::new("D", Point::new(x0 + atk_w + dec_w / 2 - 3, y1 + 12), dim).draw(display);
        let _ = Text::new(
            "S",
            Point::new(x0 + atk_w + dec_w + sus_w / 2 - 3, y1 + 12),
            dim,
        )
        .draw(display);
        let _ = Text::new(
            "R",
            Point::new(x0 + atk_w + dec_w + sus_w + rel_w / 2 - 3, y1 + 12),
            dim,
        )
        .draw(display);
    }

    // ── Effects ─────────────────────────────────────────────────────

    fn draw_efx_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let cx = theme::SCREEN_W / 2;
        let cy = (theme::VIZ_TOP + theme::VIZ_BOTTOM) / 2;

        // Signal flow: IN -> [DLY] -> [REV] -> [CHR] -> OUT
        let boxes = ["DLY", "REV", "CHR"];
        let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let border = PrimitiveStyle::with_stroke(theme::NODE_INACTIVE_BORDER, 1);
        let conn = PrimitiveStyle::with_stroke(theme::NODE_CONNECTOR, 1);

        let total_w = boxes.len() as i32 * 32 + (boxes.len() as i32 - 1) * 12;
        let start_x = cx - total_w / 2;

        // IN label
        let _ = Text::new("IN", Point::new(start_x - 24, cy + 4), dim).draw(display);

        for (i, &name) in boxes.iter().enumerate() {
            let bx = start_x + i as i32 * 44;

            let _ = Rectangle::new(Point::new(bx, cy - 8), Size::new(32, 16))
                .draw_styled(&border, display);
            let _ = Text::new(name, Point::new(bx + 5, cy + 4), dim).draw(display);

            // Connector to next
            if i + 1 < boxes.len() {
                let _ = Line::new(Point::new(bx + 32, cy), Point::new(bx + 44, cy))
                    .draw_styled(&conn, display);
            }
        }

        // OUT label
        let last_x = start_x + (boxes.len() as i32 - 1) * 44 + 32;
        let _ = Text::new("OUT", Point::new(last_x + 8, cy + 4), dim).draw(display);
    }

    // ── Mixer ───────────────────────────────────────────────────────

    fn draw_mixer_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT + 20;
        let y0 = theme::VIZ_TOP + 16;
        let y1 = theme::VIZ_BOTTOM - 8;
        let h = y1 - y0;

        let vol = self.anim[0].current();
        let pan = self.anim[1].current(); // 0=L, 0.5=C, 1=R

        // Channel level bars (4 channels)
        let bar_w: i32 = 20;
        let bar_gap: i32 = 24;
        let labels = ["CH1", "CH2", "CH3", "CH4"];

        let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let bar_bg = PrimitiveStyle::with_fill(theme::PARAM_BAR_BG);

        for (i, &name) in labels.iter().enumerate() {
            let bx = x0 + i as i32 * (bar_w + bar_gap);

            // Bar background
            let _ = Rectangle::new(Point::new(bx, y0), Size::new(bar_w as u32, h as u32))
                .draw_styled(&bar_bg, display);

            // Fill level — ch1 uses vol param, others are at 50%
            let level = if i == 0 { vol } else { 0.5 };
            let fill_h = (h as f32 * level) as i32;
            if fill_h > 0 {
                let _ = Rectangle::new(
                    Point::new(bx, y1 - fill_h),
                    Size::new(bar_w as u32, fill_h as u32),
                )
                .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_FG), display);
            }

            // Label below
            let _ = Text::new(name, Point::new(bx, y1 + 12), dim).draw(display);
        }

        // Pan indicator
        let pan_x0 = x0;
        let pan_x1 = x0 + 3 * (bar_w + bar_gap) + bar_w;
        let pan_y = y0 - 12;
        let pan_pos = pan_x0 + ((pan_x1 - pan_x0) as f32 * pan) as i32;

        let _ = Line::new(Point::new(pan_x0, pan_y), Point::new(pan_x1, pan_y))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);
        let _ = Rectangle::new(Point::new(pan_pos - 2, pan_y - 2), Size::new(5, 5))
            .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT), display);

        let _ = Text::new("L", Point::new(pan_x0 - 10, pan_y + 4), dim).draw(display);
        let _ = Text::new("R", Point::new(pan_x1 + 4, pan_y + 4), dim).draw(display);
    }

    // ── Routing ─────────────────────────────────────────────────────

    fn draw_routing_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let cx = theme::SCREEN_W / 2;
        let cy = (theme::VIZ_TOP + theme::VIZ_BOTTOM) / 2;
        let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let border = PrimitiveStyle::with_stroke(theme::NODE_INACTIVE_BORDER, 1);
        let conn = PrimitiveStyle::with_stroke(theme::NODE_CONNECTOR, 1);

        // 4 inputs -> routing matrix -> 3 outputs (DAC pairs)
        let inputs = ["P1", "P2", "P3", "P4"];
        let outputs = ["L/R", "3/4", "5/6"];

        // Input column
        for (i, &name) in inputs.iter().enumerate() {
            let y = cy - 30 + i as i32 * 18;
            let _ = Text::new(name, Point::new(cx - 60, y + 4), dim).draw(display);
            let _ = Line::new(Point::new(cx - 40, y), Point::new(cx - 20, y))
                .draw_styled(&conn, display);
        }

        // Matrix box
        let _ = Rectangle::new(Point::new(cx - 20, cy - 34), Size::new(40, 68))
            .draw_styled(&border, display);
        let _ = Text::new("MTX", Point::new(cx - 10, cy + 4), dim).draw(display);

        // Output column
        for (i, &name) in outputs.iter().enumerate() {
            let y = cy - 22 + i as i32 * 22;
            let _ = Line::new(Point::new(cx + 20, y), Point::new(cx + 40, y))
                .draw_styled(&conn, display);
            let _ = Text::new(name, Point::new(cx + 44, y + 4), dim).draw(display);
        }
    }

    // ── Compressor ──────────────────────────────────────────────────

    fn draw_comp_viz<D>(&self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let x0 = theme::VIZ_LEFT + 30;
        let x1 = theme::VIZ_RIGHT - 30;
        let y0 = theme::VIZ_TOP + 16;
        let y1 = theme::VIZ_BOTTOM - 16;
        let w = x1 - x0;
        let h = y1 - y0;

        // Axes
        let grid = PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1);
        let _ = Line::new(Point::new(x0, y1), Point::new(x1, y1)).draw_styled(&grid, display);
        let _ = Line::new(Point::new(x0, y0), Point::new(x0, y1)).draw_styled(&grid, display);

        // 1:1 reference line (diagonal)
        let _ = Line::new(Point::new(x0, y1), Point::new(x1, y0))
            .draw_styled(&PrimitiveStyle::with_stroke(theme::VIZ_GRID, 1), display);

        // Compression curve — knee at ~60%
        let threshold = 0.6;
        let ratio = 0.3; // compression above threshold

        let segments = 20;
        let stroke = PrimitiveStyle::with_stroke(theme::VIZ_LINE, 2);
        let mut prev = Point::new(x0, y1);

        for i in 1..=segments {
            let t = i as f32 / segments as f32;
            let output = if t < threshold {
                t
            } else {
                threshold + (t - threshold) * ratio
            };

            let px = x0 + (w as f32 * t) as i32;
            let py = y1 - (h as f32 * output) as i32;
            let curr = Point::new(px, py.max(y0).min(y1));
            let _ = Line::new(prev, curr).draw_styled(&stroke, display);
            prev = curr;
        }

        // Labels
        let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let _ = Text::new("IN", Point::new(x1 + 4, y1 + 4), dim).draw(display);
        let _ = Text::new("OUT", Point::new(x0 - 4, y0 - 4), dim).draw(display);
    }

    // ── BlockDef-based rendering ─────────────────────────────────────

    /// Render the parameter grid from a `BlockDef` instead of a `PageId`.
    pub fn draw_params_from_def<D>(&self, display: &mut D, def: &BlockDef)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let label_style = MonoTextStyle::new(&FONT_6X10, theme::PARAM_LABEL);
        let value_style = MonoTextStyle::new(&FONT_6X10, theme::PARAM_VALUE);

        for (i, slot) in def.params.iter().enumerate() {
            let label = slot.label;
            if label == "--" {
                continue;
            }

            let col = i % 3;
            let row = i / 3;
            let x = theme::PARAM_LEFT + col as i32 * theme::PARAM_COL_WIDTH;
            let y = theme::PARAM_TOP + row as i32 * theme::PARAM_ROW_HEIGHT;

            let val = self.anim[i].current();

            // Label (dim)
            let _ = Text::new(label, Point::new(x, y + 10), label_style).draw(display);

            // Numeric value
            let mut buf = FmtBuf::new();
            fmt::fmt_val(&mut buf, val, slot.format);
            let label_end = x + label.len() as i32 * 6 + 4;
            let _ =
                Text::new(buf.as_str(), Point::new(label_end, y + 10), value_style).draw(display);

            // Value bar below
            let bar_y = y + 15;
            let _ = Rectangle::new(
                Point::new(x, bar_y),
                Size::new(theme::BAR_WIDTH as u32, theme::BAR_HEIGHT as u32),
            )
            .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_BG), display);

            let fill_w = (theme::BAR_WIDTH as f32 * val) as i32;
            if fill_w > 0 {
                let _ = Rectangle::new(
                    Point::new(x, bar_y),
                    Size::new(fill_w as u32, theme::BAR_HEIGHT as u32),
                )
                .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_FG), display);
            }
        }
    }

    /// Render the cell grid from a `BlockDef` instead of a `PageId`.
    pub fn draw_cell_grid_from_def<D>(&self, display: &mut D, def: &BlockDef)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        for (i, slot) in def.params.iter().enumerate() {
            let col = (i % 3) as i32;
            let row = (i / 3) as i32;
            cell::draw_cell(
                display,
                col,
                row,
                slot.label,
                self.anim[i].current(),
                slot.icon,
                slot.format,
            );
        }
    }

    /// Dispatch to the appropriate visualization method based on `VizType`.
    pub fn draw_viz_from_type<D>(&self, display: &mut D, viz: VizType)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match viz {
            VizType::AlgorithmDiagram => { /* FM removed */ }
            VizType::ModalPeaks => self.draw_modal_viz(display),
            VizType::WaveformPreview => self.draw_va_viz(display),
            VizType::DriveClip => self.draw_drive_viz(display),
            VizType::FilterResponse => self.draw_filter_viz(display),
            VizType::WaveFold => self.draw_folder_viz(display),
            VizType::Adsr => self.draw_envelope_viz(display),
            VizType::EffectsFlow => self.draw_efx_viz(display),
            VizType::MixerLevels => self.draw_mixer_viz(display),
            VizType::RoutingMatrix => self.draw_routing_viz(display),
            VizType::CompressorCurve => self.draw_comp_viz(display),
            VizType::None | VizType::EqResponse | VizType::LpgResponse | VizType::Logo => {}
        }
    }

    // ── BlockDef-based full render ─────────────────────────────────────

    /// Render full screen using a `BlockDef` for layout, viz, and params.
    pub fn draw_with_def<D>(&self, display: &mut D, nav: &ChainNav, def: &BlockDef, perf: &PerfStats)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        // Clear
        let _ = Rectangle::new(Point::zero(), Size::new(240, 320))
            .draw_styled(&PrimitiveStyle::with_fill(theme::BG), display);

        self.draw_header_with_def(display, nav, def);

        match def.layout {
            PageLayout::BigViz => {
                self.draw_viz_from_type(display, def.viz);
                self.draw_params_from_def(display, def);
            }
            PageLayout::CellGrid => {
                self.draw_cell_grid_from_def(display, def);
            }
        }

        // Separator
        let _ = Line::new(
            Point::new(0, theme::ENCODER_ZONE_BOTTOM),
            Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
        )
        .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);

        dungeon_map::draw(display, nav);
        self.draw_perf(display, perf);
    }

    /// Draw a single region using BlockDef. The caller has already cleared the region.
    pub fn draw_region_with_def<D>(
        &self,
        display: &mut D,
        kind: RegionKind,
        nav: &ChainNav,
        def: &BlockDef,
        perf: &PerfStats,
    )
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match kind {
            RegionKind::Header => {
                self.draw_header_with_def(display, nav, def);
                self.draw_perf(display, perf);
            }
            RegionKind::Viz => {
                self.draw_viz_from_type(display, def.viz);
            }
            RegionKind::Params => {
                self.draw_params_from_def(display, def);
                let _ = Line::new(
                    Point::new(0, theme::ENCODER_ZONE_BOTTOM),
                    Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
                )
                .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
            }
            RegionKind::Cells => {
                self.draw_cell_grid_from_def(display, def);
                let _ = Line::new(
                    Point::new(0, theme::ENCODER_ZONE_BOTTOM),
                    Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
                )
                .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
            }
            RegionKind::Nav => {
                dungeon_map::draw(display, nav);
            }
        }
    }

    /// Header showing ChainId context and BlockDef name.
    /// Format: "Context > BlockName"
    fn draw_header_with_def<D>(&self, display: &mut D, nav: &ChainNav, def: &BlockDef)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        use crate::ui::chain::ChainId;
        use crate::ui::fmt::FmtBuf;
        use core::fmt::Write;

        let dim = MonoTextStyle::new(&FONT_6X10, theme::HEADER_LABEL);
        let bright = MonoTextStyle::new(&FONT_6X10, theme::TEXT);
        let y = theme::HEADER_Y + 10;

        let mut context_buf = FmtBuf::new();
        match nav.chain_id {
            ChainId::Part(n) => { let _ = write!(context_buf, "Part {}", n + 1); }
            ChainId::Mixer(n) => { let _ = write!(context_buf, "Mix CH{}", n + 1); }
            ChainId::System => { let _ = write!(context_buf, "System"); }
            ChainId::Demo => { let _ = write!(context_buf, "Demo"); }
        }

        let mut x = 8;
        let _ = Text::new(context_buf.as_str(), Point::new(x, y), dim).draw(display);
        x += context_buf.as_str().len() as i32 * 6;

        let _ = Text::new(" > ", Point::new(x, y), dim).draw(display);
        x += 18;

        let _ = Text::new(def.name, Point::new(x, y), bright).draw(display);
    }

    // ── Dirty region helpers ─────────────────────────────────────────

    /// Clear a screen region by direct framebuffer fill. Much faster than draw_iter.
    pub fn clear_region_fb(fb: &mut [u16], y_start: u16, y_end: u16) {
        let start = y_start as usize * 240;
        let end = y_end as usize * 240;
        for px in &mut fb[start..end] {
            *px = 0; // theme::BG is black = 0x0000
        }
    }

    // ── Perf overlay ────────────────────────────────────────────────

    fn draw_perf<D>(&self, display: &mut D, perf: &PerfStats)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let y = theme::HEADER_Y + 10;

        // Render time — top right
        let mut buf = FmtBuf::new();
        let _ = write!(buf, "{}us", perf.render_us);
        let text_w = buf.as_str().len() as i32 * 6;
        let _ = Text::new(
            buf.as_str(),
            Point::new(theme::SCREEN_W - text_w - 4, y),
            dim,
        )
        .draw(display);

        // Audio load — below render time (if measured)
        if perf.audio_load_pct > 0 {
            let mut buf2 = FmtBuf::new();
            let _ = write!(buf2, "CPU {}%", perf.audio_load_pct);
            let text_w2 = buf2.as_str().len() as i32 * 6;

            // Color based on load
            let color = if perf.audio_load_pct > 80 {
                Rgb565::new(31, 0, 0) // red
            } else if perf.audio_load_pct > 60 {
                Rgb565::new(31, 32, 0) // yellow
            } else {
                theme::TEXT_DIM
            };
            let style = MonoTextStyle::new(&FONT_6X10, color);
            let _ = Text::new(
                buf2.as_str(),
                Point::new(theme::SCREEN_W - text_w2 - 4, y + 12),
                style,
            )
            .draw(display);
        }
    }
}
