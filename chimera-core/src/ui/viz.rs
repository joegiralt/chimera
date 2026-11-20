//! Direction A visualizations (ADR 0016): drawn as the main element with a
//! soft accent fill under a 1.5-px accent line (spec § Shared components);
//! drawn 2 px since the display has no anti-aliasing. No grid lines.

use crate::dsp::clampf;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::algo::algorithms::Algorithm;
use crate::dsp::algo::env::{
    DECAY_OCTAVES, DECAY_OVER_ATTACK_OCTAVES, EnvRates, decay_seconds, effective, effective_release,
};
use crate::dsp::algo::math::log2;
use crate::dsp::algo::plan::{OPS, blend};
use crate::dsp::algo::tx::d1l_level;
use crate::dsp::modulator::{EnvForm, EnvSpeed, Func, HoldPos, LfoForm};
use crate::dsp::voice_fx::look::{self, Rows};
use crate::scope::{self, SCOPE_LEN};
use crate::ui::alg_layout;
use crate::ui::draw;
use crate::ui::theme;

/// Columns of the viz band (x 12..=228).
pub const LIVE_COLS: usize = (theme::VIZ_RIGHT - theme::VIZ_LEFT + 1) as usize;

/// Live output as pixel offsets from the band's centre line, auto-scaled to
/// ±`VIZ_BAND_AMP` from the peak of the columns actually drawn (flat while
/// silent).
pub fn live_columns(buf: &[f32; SCOPE_LEN]) -> [i8; LIVE_COLS] {
    let peak = scope::peak(&buf[..LIVE_COLS]);
    let scale = if peak > scope::SOUNDING_PEAK {
        theme::VIZ_BAND_AMP as f32 / peak
    } else {
        0.0
    };
    core::array::from_fn(|i| libm::roundf(buf[i] * scale) as i8)
}

/// Cheap fingerprint of what `live_output` draws: the viz region redraws
/// only when it changes (a silent or frozen scope costs no SPI traffic).
pub fn live_key(buf: &[f32; SCOPE_LEN]) -> u32 {
    crate::ui::region::settings_key(&[&live_columns(buf).map(|c| c as u8)])
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
        let (a, b) = if y(i) < mid {
            (y(i) + 1, mid)
        } else {
            (mid, y(i))
        };
        draw::fill_rect(d, x, a, 1, b - a, theme::ACCENT_SOFT);
    }
    // The line's extra row stays inside the band (152±24, +1 ≤ 185).
    for i in 1..cols.len() {
        let x = theme::VIZ_LEFT + i as i32;
        draw::thick_line(d, x - 1, y(i - 1), x, y(i), theme::ACCENT);
    }
}

/// BigViz plot area: curves between `PLOT_TOP` and `PLOT_BASE`.
pub const PLOT_TOP: i32 = 40;
pub const PLOT_BASE: i32 = 170;
/// Filter pass band (the mockup's 0 dB line); resonance peaks above it.
pub const FILTER_PASS_Y: i32 = 72;
/// The response's dB scale: the pass line to the base is 48 dB.
const PX_PER_DB: f32 = (PLOT_BASE - FILTER_PASS_Y) as f32 / 48.0;
/// The LEVEL LED in the FLT viz's top-right corner.
pub const LED_X: i32 = theme::VIZ_RIGHT - 4;
pub const LED_Y: i32 = PLOT_TOP + 4;

/// The shape the viz draws for a kind's mode (spec § 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    /// CLEAN LP2 and DIRTY LP2: `1/(s² + ks + 1)`.
    Low,
    /// CLEAN LP4: two such stages, the second at `clean_k2`.
    Low24,
    /// LP1: `(1 + s)/(s² + ks + 1)`, half the low-pass's roll-off.
    Lp1,
    /// HP2 (DIRTY): `s²/(s² + ks + 1)`.
    High,
    /// CLEAN HP4.
    High24,
    /// CLEAN BP2: `2s/(s² + ks + 1)`, BP4 the cascade.
    Band,
    Band24,
    /// DIRTY BP2: `s/(s² + qs + 1)`, no boost.
    DirtyBand,
    /// DIRTY's NOTCH1: `(1 + s²)/(s² + qs + 1)`, a zero at FREQ.
    Notch,
    /// CLEAN's NOTCH1: `(1 + s²/4)/(s² + ks + 1)`, a zero at 2 × FREQ and
    /// the treble 12 dB down.
    Notch2x,
    /// NOTCH2: notches at 0.414 × and 2.414 × FREQ, Q raising the peak
    /// between them.
    DoubleNotch,
    /// The 2040's LP4 and LADDER's LP4: `1/((1 + s)⁴ + k)`, the pass band
    /// lowered by `1/(1 + k)` as RES rises.
    K2040,
    /// The 2040 on VOICE EXP: LP4 under its make-up gain `1 + m(k)`, the
    /// pass band lowered by about half as much (ADR 0191).
    K2040Exp,
    /// LADDER's LP2, BP2, BP4 and HP4: LP4's denominator, each mode's
    /// numerator and gain (ADR 0104).
    LadderLp2,
    LadderBp2,
    LadderBp4,
    LadderHp4,
    /// SEM's MODE output, `(1 − m) + m·s²` over `s² + ks + 1`: LP, the
    /// notch at `√((1 − m)/m)` × FREQ, HP (ADR 0192); `m` in 64ths.
    SemMode(u8),
    /// SEM's BANDPASS, `s/(s² + ks + 1)`: 1/k at FREQ.
    SemBand,
}

impl Response {
    /// `of`, with the 2040's VOICE: EXP draws its make-up gain.
    pub fn of_voice(
        kind: crate::dsp::filter::FilterKind,
        m: crate::dsp::filter::FilterMode,
        voice: crate::dsp::filter::K2040Voice,
    ) -> Self {
        match Self::of(kind, m) {
            Response::K2040 if kind == crate::dsp::filter::FilterKind::K2040 && voice.make_up() => {
                Response::K2040Exp
            }
            r => r,
        }
    }

    /// SEM's output `mode` at MODE `morph` (0..1).
    pub fn sem(mode: crate::dsp::filter::FilterMode, morph: f32) -> Self {
        match mode {
            crate::dsp::filter::FilterMode::Bp12 => Response::SemBand,
            _ => Response::SemMode((clampf(morph, 0.0, 1.0) * 64.0 + 0.5) as u8),
        }
    }

    pub fn of(kind: crate::dsp::filter::FilterKind, m: crate::dsp::filter::FilterMode) -> Self {
        use crate::dsp::filter::{FilterKind as K, FilterMode as M};
        match (kind, m) {
            (K::Svf, M::Lp6) => Response::Lp1,
            (K::Svf, M::Hp24 | M::Hp12) => Response::High24,
            (K::Svf, M::Bp12) => Response::Band,
            (K::Svf, M::Bp24) => Response::Band24,
            (K::Svf, M::Notch) => Response::Notch2x,
            (K::Svf, M::Phaser) => Response::DoubleNotch,
            (K::Svf, M::Lp12) => Response::Low,
            (K::Svf, M::Lp24) => Response::Low24,
            (K::K2040, _) => Response::K2040,
            // DIRTY's NOTCH1 notches at FREQ (spec § 5).
            (K::Dirty, M::Notch) => Response::Notch,
            (K::Dirty, M::Bp12 | M::Bp24) => Response::DirtyBand,
            (K::Dirty, M::Hp12 | M::Hp24) => Response::High,
            (K::Dirty, _) => Response::Low,
            // LADDER keeps the loop's bass loss and its mode gains (ADR 0104).
            (K::Ladder, M::Bp12) => Response::LadderBp2,
            (K::Ladder, M::Bp24) => Response::LadderBp4,
            (K::Ladder, M::Hp24 | M::Hp12) => Response::LadderHp4,
            (K::Ladder, M::Lp12) => Response::LadderLp2,
            (K::Ladder, _) => Response::K2040,
            (K::Sem, m) => Response::sem(m, 0.0),
        }
    }
}

/// 0..1: how far into a notch drawn at `at` column `t` is (cos-shaped,
/// a twelfth of the plot each side).
fn dip(t: f32, at: f32) -> f32 {
    let d = ((t - at) * 6.0).abs();
    if d < 0.5 {
        0.5 + 0.5 * libm::cosf(d * 2.0 * core::f32::consts::PI)
    } else {
        0.0
    }
}

/// The lowest FREQ column the curve is drawn for (a FREQ of 0 would put
/// every column at infinity).
const MIN_CUTOFF: f32 = 0.01;

/// A 2-pole denominator's `|s² + ks + 1|²` at `s = jw`.
fn den2(w: f32, k: f32) -> f32 {
    let a = 1.0 - w * w;
    a * a + k * k * w * w
}

/// The 4-pole's `|(1 + s)⁴ + k|²` at `s = jw`.
fn den4(w: f32, k: f32) -> f32 {
    let w2 = w * w;
    let re = 1.0 - 6.0 * w2 + w2 * w2 + k;
    let im = 4.0 * w * (1.0 - w2);
    re * re + im * im
}

/// The response's gain in dB at `w` × FREQ, from the filter's own transfer
/// function (the analog prototype of the coefficients in `dsp::filter`; the
/// TPT and Chamberlin forms follow it to a fraction of a dB below a few kHz).
/// NOTCH2 is drawn by `response_y`'s schematic instead.
fn mag_db(r: Response, w: f32, reso: f32) -> f32 {
    use crate::dsp::filter::{FilterMode as M, clean, ladder};
    let k = clean::clean_k(reso);
    let k2 = clean::clean_k2(k);
    let (w2, k4) = (w * w, crate::dsp::filter::k2040::k2040_k(reso));
    // |H|² as numerator over denominator.
    let (num, den) = match r {
        Response::Low => (1.0, den2(w, k)),
        Response::Low24 => (1.0, den2(w, k) * den2(w, k2)),
        Response::Lp1 => (1.0 + w2, den2(w, k)),
        Response::High => (w2 * w2, den2(w, k)),
        Response::High24 => (w2 * w2 * w2 * w2, den2(w, k) * den2(w, k2)),
        Response::Band => (4.0 * w2, den2(w, k)),
        Response::Band24 => (4.0 * w2 * w2, den2(w, k) * den2(w, k2)),
        Response::DirtyBand => (w2, den2(w, k)),
        Response::Notch => ((1.0 - w2) * (1.0 - w2), den2(w, k)),
        Response::Notch2x => {
            let z = 1.0 - 0.25 * w2;
            (z * z, den2(w, k))
        }
        Response::K2040 => (1.0, den4(w, k4)),
        Response::K2040Exp => {
            let up = crate::dsp::filter::k2040::make_up(k4);
            (up * up, den4(w, k4))
        }
        Response::LadderLp2 | Response::LadderBp2 | Response::LadderBp4 | Response::LadderHp4 => {
            let (m, n) = match r {
                Response::LadderLp2 => (M::Lp12, (1.0 + w2) * (1.0 + w2)),
                Response::LadderBp2 => (M::Bp12, w2 * (1.0 + w2) * (1.0 + w2)),
                Response::LadderBp4 => (M::Bp24, w2 * w2),
                _ => (M::Hp24, w2 * w2 * w2 * w2),
            };
            let g = ladder::mode_gain(m, k4);
            (g * g * n, den4(w, k4))
        }
        Response::SemMode(q) => {
            let m = f32::from(q) * (1.0 / 64.0);
            let z = (1.0 - m) - m * w2;
            (z * z, den2(w, crate::dsp::filter::sem::sem_k_tap(reso)))
        }
        Response::SemBand => (w2, den2(w, crate::dsp::filter::sem::sem_k_tap(reso))),
        Response::DoubleNotch => return 0.0,
    };
    // A ring at the onset is off the plot, not infinite.
    10.0 * libm::log10f(num.max(1e-12) / den.max(1e-6))
}

/// The curve's y at column `t` (0..1 across the plot) for `cutoff` and
/// `reso`. The plot is linear in Hz, so k × FREQ sits at k × `cutoff`; the
/// transfer-function responses are drawn at their real gain, 2.04 px a dB
/// below the pass line, and clamp at the plot's edges. Only NOTCH2's pair of
/// notches is schematic. CLEAN's output trim (its RES 1 level) is not drawn:
/// the pass band stays on the 0 dB line.
pub fn response_y(t: f32, cutoff: f32, reso: f32, r: Response) -> i32 {
    let span = (PLOT_BASE - FILTER_PASS_Y) as f32;
    let peak_h = (FILTER_PASS_Y - PLOT_TOP) as f32;
    // `h` px down at FREQ (up if negative), in a notch's shape.
    let bump = |h: f32| h * dip(t, cutoff);
    let y = match r {
        Response::DoubleNotch => {
            let n = dip(t, 0.414 * cutoff).max(dip(t, 2.414 * cutoff));
            FILTER_PASS_Y as f32 - bump(peak_h * reso) * (1.0 - n) + span * n
        }
        _ => FILTER_PASS_Y as f32 - mag_db(r, t / cutoff.max(MIN_CUTOFF), reso) * PX_PER_DB,
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
        draw::thick_line(d, x - 1, y(x - 1), x, y(x), theme::ACCENT);
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

    let w = draw::text_width(&theme::FONT_READOUT, value, 0).max(draw::text_width(
        &theme::FONT_LABEL,
        label,
        theme::LABEL_TRACKING,
    ));
    let left = if x + 8 + w <= theme::VIZ_RIGHT {
        x + 8
    } else {
        x - 8 - w
    };

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

    let vy = if top - GAP - BLOCK_TOP >= PLOT_TOP {
        top - GAP
    } else {
        bottom + GAP + BLOCK_TOP
    };
    let vy = vy.clamp(PLOT_TOP + BLOCK_TOP, PLOT_BASE - 2);

    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        label,
        left + 1,
        vy - LABEL_RISE,
        theme::MID,
        theme::LABEL_TRACKING,
    );
    draw::text(d, &theme::FONT_READOUT, value, left, vy, theme::INK);
}

/// Filter: response with a soft fill, a faint pass-band line, and a marker
/// at the cutoff carrying `readout` (the focused slot's label and value).
pub fn filter<D>(
    d: &mut D,
    cutoff: f32,
    reso: f32,
    response: Response,
    readout_text: Option<(&str, &str)>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32;
    let y = |x: i32| response_y((x - theme::VIZ_LEFT) as f32 / w, cutoff, reso, response);
    filled_curve(d, theme::VIZ_LEFT, theme::VIZ_RIGHT, PLOT_BASE, y);
    draw::fill_rect(
        d,
        theme::VIZ_LEFT,
        FILTER_PASS_Y,
        theme::VIZ_RIGHT - theme::VIZ_LEFT,
        1,
        theme::FAINT,
    );
    let mx = theme::VIZ_LEFT + (w * clampf(cutoff, 0.0, 1.0)) as i32;
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

/// Where modulation moves the filter from its set value, in pixels: the
/// ghost the FLT viz draws beside the solid curve. Each `Some` holds
/// (set, moved) and is `None` while the two share a pixel, so a still
/// modulation redraws nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModMark {
    /// Columns of the cutoff corner.
    pub cutoff: Option<(i32, i32)>,
    /// Rows of the resonance peak's top.
    pub res: Option<(i32, i32)>,
}

/// Gap below the plot's base to the cutoff band, and the band's height.
const MARK_DY: i32 = 4;
const MARK_H: i32 = 3;
/// The resonance band's column, right of the plot.
const MARK_RES_X: i32 = theme::VIZ_RIGHT + 5;

impl ModMark {
    /// The mark for the set `cutoff`/`reso` and where modulation puts them
    /// (all 0..1).
    pub fn of(cutoff: f32, moved_cutoff: f32, reso: f32, moved_reso: f32) -> Self {
        let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32;
        let x = |c: f32| theme::VIZ_LEFT + (w * clampf(c, 0.0, 1.0)) as i32;
        let y = |r: f32| {
            FILTER_PASS_Y - ((FILTER_PASS_Y - PLOT_TOP) as f32 * clampf(r, 0.0, 1.0)) as i32
        };
        let apart = |a: i32, b: i32| (a != b).then_some((a, b));
        Self {
            cutoff: apart(x(cutoff), x(moved_cutoff)),
            res: apart(y(reso), y(moved_reso)),
        }
    }

    /// A fingerprint: equal while the mark draws the same pixels.
    pub fn key(&self) -> u32 {
        // Columns span the 216 px plot (x 12..=228) and rows 40..=72: each
        // fits a byte.
        const _: () = assert!(theme::VIZ_RIGHT < 256 && FILTER_PASS_Y < 256);
        let p =
            |s: Option<(i32, i32)>| s.map_or(0, |(a, b)| (a as u32 & 0xff) << 8 | b as u32 & 0xff);
        p(self.cutoff) | p(self.res) << 16
    }

    /// The boxes the mark can occupy (x, y, w, h): the cutoff band under
    /// the plot, the resonance band right of it. Neither touches the curve.
    pub const BOXES: [(i32, i32, i32, i32); 2] = [
        (
            theme::VIZ_LEFT - 2,
            PLOT_BASE + MARK_DY - 1,
            theme::VIZ_RIGHT - theme::VIZ_LEFT + 5,
            MARK_H + 2,
        ),
        (
            MARK_RES_X - 3,
            PLOT_TOP - 1,
            7,
            FILTER_PASS_Y - PLOT_TOP + 3,
        ),
    ];

    /// Faint band from the set value to the moved one, and a ghost tick
    /// where modulation has it now. Secondary: grey, never the accent.
    pub fn draw<D>(&self, d: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let y0 = PLOT_BASE + MARK_DY;
        if let Some((a, b)) = self.cutoff {
            draw::fill_rect(d, a.min(b), y0, (a - b).abs() + 1, MARK_H, theme::FAINT);
            draw::fill_rect(d, b, y0 - 1, 1, MARK_H + 2, theme::MID);
        }
        if let Some((a, b)) = self.res {
            draw::fill_rect(d, MARK_RES_X, a.min(b), 3, (a - b).abs() + 1, theme::FAINT);
            draw::fill_rect(d, MARK_RES_X - 2, b, 7, 1, theme::MID);
        }
    }
}

/// Most stages `envelope` draws: A · H · D · S · R.
pub const MAX_STAGES: usize = 5;

/// Envelope: up to `MAX_STAGES` segments over proportional `widths`,
/// breakpoints at `heights` (0..1, one more than the widths), stage labels
/// below; segment `lit` (the one the focused slot edits) in the accent.
pub fn envelope<D>(d: &mut D, widths: &[f32], heights: &[f32], labels: &[&str], lit: Option<usize>)
where
    D: DrawTarget<Color = Rgb565>,
{
    envelope_from(d, PLOT_TOP, widths, heights, labels, lit);
}

/// Top of `envelope` in the viz band (`op_env`'s graph): clear of the band's
/// edge by a breakpoint dot.
pub const BAND_PLOT_TOP: i32 = theme::VIZ_BAND_TOP + 4;

/// `envelope` with its full level at `top`; the base line and labels stay
/// where they are, which fits the viz band too.
pub fn envelope_from<D>(
    d: &mut D,
    top: i32,
    widths: &[f32],
    heights: &[f32],
    labels: &[&str],
    lit: Option<usize>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let n = widths.len().min(MAX_STAGES);
    let base = PLOT_BASE - 8;
    let (x0, w, h) = (
        theme::VIZ_LEFT,
        (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32,
        (base - top) as f32,
    );
    let mut pts = [(0i32, 0i32); MAX_STAGES + 1];
    let mut cx = x0 as f32;
    for i in 0..=n {
        pts[i] = (cx as i32, base - (h * clampf(heights[i], 0.0, 1.0)) as i32);
        if i < n {
            cx += w * widths[i];
        }
    }
    let pts = &pts[..=n];
    let y_at = |x: i32| {
        let s = (0..n).find(|&s| x <= pts[s + 1].0).unwrap_or(n - 1);
        let ((xa, ya), (xb, yb)) = (pts[s], pts[s + 1]);
        if xb == xa {
            yb
        } else {
            ya + (yb - ya) * (x - xa) / (xb - xa)
        }
    };
    for x in x0..=pts[n].0 {
        draw::fill_rect(d, x, y_at(x) + 1, 1, base - y_at(x) - 1, theme::ACCENT_SOFT);
    }
    draw::fill_rect(d, x0, base, theme::VIZ_RIGHT - x0, 1, theme::FAINT);
    for s in 0..n {
        let ((xa, ya), (xb, yb)) = (pts[s], pts[s + 1]);
        if lit == Some(s) {
            draw::thick_line(d, xa, ya, xb, yb, theme::ACCENT);
        } else {
            draw::line(d, xa, ya, xb, yb, theme::INK2, 1);
        }
    }
    let mut xs = [0i32; MAX_STAGES + 1];
    for (x, p) in xs.iter_mut().zip(pts) {
        *x = p.0;
    }
    let labels = &labels[..n];
    for (s, span) in stage_label_spans(&xs[..=n], labels, lit).iter().enumerate() {
        if let Some((left, _)) = *span {
            let color = if lit == Some(s) {
                theme::ACCENT
            } else {
                theme::MID
            };
            draw::text_tracked(
                d,
                &theme::FONT_LABEL,
                labels[s],
                left,
                base + 14,
                color,
                theme::LABEL_TRACKING,
            );
        }
    }
    for (i, &(x, y)) in pts.iter().enumerate() {
        let on_lit = lit.is_some_and(|s| i == s || i == s + 1);
        draw::dot(d, x, y, 2, if on_lit { theme::ACCENT } else { theme::INK2 });
    }
}

/// Horizontal gap kept between two stage labels.
const STAGE_LABEL_GAP: i32 = 2;

/// The least width, in the units of `rest` (the other stages' widths
/// summed), that gives a stage room for `label` and the gaps either side.
pub fn label_floor(label: &str, rest: f32) -> f32 {
    let m = (draw::text_width(&theme::FONT_LABEL, label, theme::LABEL_TRACKING)
        + 2
        + 2 * STAGE_LABEL_GAP
        + 2) as f32;
    m * rest / ((theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32 - m)
}

/// The operator envelope's stage labels, one per `op_env` width.
pub const OP_ENV_LABELS: [&str; 4] = ["A", "D1", "D2", "R"];

/// D2 runs while the key is held, so like the amp's sustain it gets a set
/// weight beside the other stages' widths rather than a time; how far D2R
/// sinks it is read over `OP_D2_SECONDS`.
const OP_D2_WIDTH: f32 = 0.3;
pub const OP_D2_SECONDS: f32 = 1.0;
/// The graph's level axis: 48 dB, D1L's own range (3 dB a step), so D1L 1
/// sits near the floor.
pub const OP_LEVEL_OCTAVES: f32 = 8.0;

/// An operator envelope (key scaling aside) as `envelope`'s widths and
/// heights: A · D1 · D2 · R. Each stage's width is the log of its own DSP
/// time (`env`'s law, `t = K * 2^(-rate/4)`, in closed form), placed
/// between the fastest attack and the slowest stage that can occur, a fall
/// down the whole axis at effective rate 2; a rate 0 holds, so its stage
/// is full width. Levels are in dB (`OP_LEVEL_OCTAVES`), the axis the
/// decays fall straight along. A keeps the amp's least width; D1 and R
/// widen to fit their labels; all are then scaled to fill the plot.
pub fn op_env(r: EnvRates) -> ([f32; 4], [f32; 5]) {
    // Octaves of time above the fastest attack (effective rate 63).
    let attack = |rate: u8| (63 - rate) as f32 / 4.0;
    // A fall of `by` heights at `rate`: a full decay's time, scaled.
    let fall = |rate: u8, by: f32| {
        attack(rate) + DECAY_OVER_ATTACK_OCTAVES + log2(by * OP_LEVEL_OCTAVES / DECAY_OCTAVES)
    };
    let span = |oct: f32| clampf(oct / fall(2, 1.0), 0.0, 1.0);
    let (ar, d1r, d2r) = (effective(r.ar, 0), effective(r.d1r, 0), effective(r.d2r, 0));
    let l = d1l_level(r.d1l);
    // D1R 0 holds D1, unless D1L 15 meets it at once and D2 runs (`OpEnv`).
    let d1_holds = d1r == 0 && l < 1.0;
    let peak = if ar == 0 { 0.0 } else { 1.0 };
    let knee = if d1_holds {
        peak
    } else if l > 0.0 {
        clampf(1.0 + log2(l) / OP_LEVEL_OCTAVES, 0.0, peak)
    } else {
        0.0
    };
    let end = if d1_holds || d2r == 0 {
        knee
    } else {
        let per_height = decay_seconds(d2r) * OP_LEVEL_OCTAVES / DECAY_OCTAVES;
        (knee - OP_D2_SECONDS / per_height).max(0.0)
    };
    let a = if ar == 0 { 1.0 } else { span(attack(ar)) }.max(0.02);
    let d1 = if d1_holds {
        1.0
    } else {
        span(fall(d1r, peak - knee))
    };
    let rel = span(fall(effective_release(r.rr, 0), end)).max(0.02);
    let d1 = d1.max(label_floor("D1", a + OP_D2_WIDTH + rel));
    let rel = rel.max(label_floor("R", a + d1 + OP_D2_WIDTH));
    let total = a + d1 + OP_D2_WIDTH + rel;
    (
        [a / total, d1 / total, OP_D2_WIDTH / total, rel / total],
        [0.0, peak, knee, end, 0.0],
    )
}

/// Where each stage label of `envelope` goes, as `(left, right)` columns
/// (inclusive), centred under its segment `xs[s]..xs[s + 1]`; `None` = left
/// out. The lit stage's label is always drawn. Any other is drawn only when
/// it fits its own segment and keeps `STAGE_LABEL_GAP` from every label
/// already placed (the lit one first), so no two labels ever touch.
pub fn stage_label_spans(
    xs: &[i32],
    labels: &[&str],
    lit: Option<usize>,
) -> [Option<(i32, i32)>; MAX_STAGES] {
    let n = labels.len().min(MAX_STAGES);
    let span = |s: usize| {
        let w = draw::text_width(&theme::FONT_LABEL, labels[s], theme::LABEL_TRACKING);
        let left = (xs[s] + xs[s + 1]) / 2 - w / 2;
        (w, (left, left + w - 1))
    };
    let mut spans = [None; MAX_STAGES];
    if let Some(s) = lit.filter(|&s| s < n) {
        spans[s] = Some(span(s).1);
    }
    for s in (0..n).filter(|&s| lit != Some(s)) {
        let (w, (l, r)) = span(s);
        let fits = w + 2 <= xs[s + 1] - xs[s];
        let clear = spans
            .iter()
            .flatten()
            .all(|&(pl, pr)| r + STAGE_LABEL_GAP < pl || pr + STAGE_LABEL_GAP < l);
        if fits && clear {
            spans[s] = Some((l, r));
        }
    }
    spans
}

/// Envelope B's shape for the viz: 0..1 across `t` (0..1). ENV: one
/// rise-and-fall (AHR holds a quarter; CYCLE twice); LFO: three cycles, or
/// a fixed walk for LFV; BURST: eight pulses under the burst.
pub fn func_shape(f: Func, rise: f32, fall: f32, shape: f32, t: f32) -> f32 {
    use crate::dsp::modulator::law::{B_TIME, curve, shape_w, tilt};
    let frac = |x: f32| x - (x as u32) as f32;
    match f {
        Func::Env(e) => {
            let (cycles, hold) = match e {
                EnvForm::Cycle => (2.0, 0.0),
                EnvForm::Ahr => (1.0, 0.25),
                EnvForm::Ad => (1.0, 0.0),
            };
            let x = frac(t * cycles);
            let (tr, tf) = (B_TIME.at(rise), B_TIME.at(fall));
            let r = tr / (tr + tf) * (1.0 - hold);
            let w = shape_w(shape);
            if x < r {
                curve(x / r, w)
            } else if x < r + hold {
                1.0
            } else {
                1.0 - curve((x - r - hold) / (1.0 - r - hold), w)
            }
        }
        Func::Lfo(LfoForm::Lfv) => {
            const WALK: [f32; 9] = [0.0, 0.7, -0.4, 0.9, -0.8, 0.3, -0.2, 0.6, -0.5];
            let x = t * 8.0;
            let k = (x as usize).min(7);
            let u = x - k as f32;
            let at = |i: usize| 0.5 + 0.5 * clampf(WALK[i] * fall.max(0.2), -1.0, 1.0);
            let lin = at(k) + (at(k + 1) - at(k)) * u;
            // SLEW rounds each corner toward the segment's middle.
            let mid = 0.5 * (at(k) + at(k + 1));
            lin + (mid - lin) * shape * (1.0 - (2.0 * u - 1.0).abs())
        }
        Func::Lfo(_) => tilt(frac(t * 3.0 + fall), shape),
        Func::Burst(e) => {
            let p = frac(t * 8.0);
            let pulse = if e == EnvForm::Cycle {
                tilt(p, shape)
            } else {
                let m = 1.0 - (2.0 * shape - 1.0).abs();
                let square = if p < 0.5 { 1.0 } else { 0.0 };
                (1.0 - m) * square + m * (0.5 - 0.5 * libm::cosf(core::f32::consts::TAU * p))
            };
            tilt(t, shape) * pulse
        }
    }
}

/// The B tabs' baseline, above the curve.
pub const TAB_Y: i32 = 48;

/// B's viz: ENV · LFO · BURST tabs (MODE lit), then the shape, filled.
pub fn func<D>(d: &mut D, f: Func, rise: f32, fall: f32, shape: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, name) in ["ENV", "LFO", "BURST"].iter().enumerate() {
        let x = theme::VIZ_LEFT + i as i32 * 52;
        let on = i == f.mode() as usize;
        if on {
            draw::pill(d, x, TAB_Y - 11, 46, 14, theme::ACCENT);
        } else {
            draw::round_outline(d, x, TAB_Y - 11, 46, 14, 7, theme::FAINT);
        }
        let color = if on { theme::BG } else { theme::MID };
        draw::text_center(d, &theme::FONT_LABEL_BOLD, name, x + 23, TAB_Y, color, 0);
    }
    let top = TAB_Y + 8;
    let (w, h) = (
        (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32,
        (PLOT_BASE - top) as f32,
    );
    let y = |x: i32| {
        let t = (x - theme::VIZ_LEFT) as f32 / w;
        PLOT_BASE - (h * clampf(func_shape(f, rise, fall, shape, t), 0.0, 1.0)) as i32
    };
    filled_curve(d, theme::VIZ_LEFT, theme::VIZ_RIGHT, PLOT_BASE, y);
}

/// The GR meter: a bar at the plot's right edge, lit down from the top,
/// full height at `GR_RANGE_DB`.
pub const GR_X: i32 = theme::VIZ_RIGHT - 6;
pub const GR_W: i32 = 6;
pub const GR_RANGE_DB: f32 = 24.0;

/// MST: the compressor's static curve at `thresh_db` and `ratio`, input and
/// output over −48..0 dB, and the GR meter at `gr_db`.
pub fn compressor<D>(d: &mut D, thresh_db: f32, ratio: f32, gr_db: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x0, x1) = (theme::VIZ_LEFT + 30, theme::VIZ_RIGHT - 30);
    let (w, h) = ((x1 - x0) as f32, (PLOT_BASE - PLOT_TOP) as f32);
    draw::line(d, x0, PLOT_BASE, x1, PLOT_TOP, theme::FAINT, 1);
    let out = |db: f32| {
        if db < thresh_db {
            db
        } else {
            thresh_db + (db - thresh_db) / ratio
        }
    };
    filled_curve(d, x0, x1, PLOT_BASE, |x| {
        let db = -48.0 + 48.0 * (x - x0) as f32 / w;
        PLOT_BASE - (h * (out(db) + 48.0) / 48.0) as i32
    });
    draw::text(d, &theme::FONT_LABEL, "IN", x1 + 4, PLOT_BASE, theme::MID);
    draw::text(
        d,
        &theme::FONT_LABEL,
        "OUT",
        x0 - 20,
        PLOT_TOP + 8,
        theme::MID,
    );
    let lit = (h * clampf(gr_db / GR_RANGE_DB, 0.0, 1.0) + 0.5) as i32;
    draw::fill_rect(d, GR_X, PLOT_TOP, GR_W, PLOT_BASE - PLOT_TOP, theme::FAINT);
    draw::fill_rect(d, GR_X, PLOT_TOP, GR_W, lit, theme::ACCENT);
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
        let (font, color) = if sel {
            (&theme::FONT_LABEL_BOLD, theme::INK)
        } else {
            (&theme::FONT_LABEL, theme::MID)
        };
        draw::text_center(d, font, num.as_str(), x + 4, STRIP_NUM_Y, color, 0);
        draw::fill_rect(d, x, STRIP_TOP, 8, STRIP_H, theme::FAINT);
        let h = (STRIP_H as f32 * clampf(s.level, 0.0, 1.0) + 0.5) as i32;
        let fill = if sel {
            theme::ACCENT
        } else if s.level > 0.0 {
            theme::BAR_REST
        } else {
            theme::FAINT
        };
        draw::fill_rect(d, x, STRIP_TOP + STRIP_H - h, 8, h, fill);
        draw::fill_rect(d, x - 6, STRIP_PAN_Y, 20, 1, theme::FAINT);
        let px = x + 4 + libm::roundf(clampf(s.pan, -1.0, 1.0) * 10.0) as i32;
        draw::dot(
            d,
            px,
            STRIP_PAN_Y,
            2,
            if sel { theme::INK } else { theme::MID },
        );
    }
}

/// The FX flow's node labels, in the Mixer chain's order.
pub const FX_NODES: [&str; 5] = ["IN", "CHR", "DLY", "REV", "OUT"];
pub const FLOW_Y: i32 = 146;
pub const FLOW_SEND_Y: i32 = 176;

/// FX pages and SENDS: IN → CHR → DLY → REV → OUT on a line (the
/// pre-refresh flow diagram, drawn with the map's nodes). `lit` (0 = CHR)
/// is the page's effect, or on SENDS the focused send: an INK pill, so the
/// map's pill stays the page's one accent pill. `sends` shows each send
/// level under its effect.
pub fn effects_flow<D>(d: &mut D, lit: Option<usize>, sends: Option<[f32; 3]>)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::ui::dungeon_map::{node_x, pill_node, ring_node};
    let n = FX_NODES.len();
    draw::fill_rect(
        d,
        theme::MAP_X0,
        FLOW_Y,
        theme::MAP_X1 - theme::MAP_X0,
        1,
        theme::FAINT,
    );
    for (i, label) in FX_NODES.iter().enumerate() {
        let x = node_x(i, n);
        let fx = i.checked_sub(1).filter(|&k| k < 3);
        if fx.is_some() && fx == lit {
            pill_node(d, x, FLOW_Y, label, theme::INK);
        } else {
            ring_node(d, x, FLOW_Y, label, FLOW_Y + 18);
        }
        if let (Some(k), Some(levels)) = (fx, sends) {
            let fill = if lit == Some(k) {
                theme::ACCENT
            } else {
                theme::BAR_REST
            };
            draw::bar(
                d,
                x - 14,
                FLOW_SEND_Y,
                28,
                2,
                levels[k],
                false,
                theme::FAINT,
                fill,
            );
        }
    }
}

/// FLT › FX's strip: `source › FX › FLT`, or `source › FLT › FX` for
/// POST, the FX node lit. The same nodes as `effects_flow`.
pub fn fx_strip<D>(d: &mut D, source: &str, post: bool, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::ui::dungeon_map::{node_x, pill_node, ring_node};
    draw::fill_rect(
        d,
        theme::MAP_X0,
        y,
        theme::MAP_X1 - theme::MAP_X0,
        1,
        theme::FAINT,
    );
    let order = if post {
        [source, "FLT", "FX"]
    } else {
        [source, "FX", "FLT"]
    };
    for (i, label) in order.into_iter().enumerate() {
        let x = node_x(i, 3);
        if label == "FX" {
            pill_node(d, x, y, label, theme::INK);
        } else {
            ring_node(d, x, y, label, y + 18);
        }
    }
}

const _: () = assert!(look::COLS == (theme::VIZ_RIGHT - theme::VIZ_LEFT + 1) as usize);

/// The FX type's transfer, `rows` from the door (`look::trace`): the sine
/// period in FAINT, the type's curve of it in INK, about row `cy`.
pub fn fx_curve<D>(d: &mut D, cy: i32, rows: &Rows)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (r, color) in rows.iter().zip([theme::FAINT, theme::INK]) {
        for (i, w) in r.windows(2).enumerate() {
            let x = theme::VIZ_LEFT + i as i32;
            draw::line(d, x, cy - w[0] as i32, x + 1, cy - w[1] as i32, color, 1);
        }
    }
}

/// The ALGO page: A's layout moving to B's with MORPH. A link is drawn in
/// MID once its blended weight reaches 0.5, FAINT below; an operator is a
/// filled carrier once its blended carrier gain reaches 0.5. Each of
/// `algo`'s operators that feeds back wears a loop (bank 2's
/// `notes::loops`, ADR 0298: closed, none).
pub fn algo_diagram<D>(
    d: &mut D,
    a: &Algorithm,
    b: &Algorithm,
    morph: f32,
    algo: &crate::dsp::algo::params::AlgoParams,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let on = |bit: bool| if bit { 1.0 } else { 0.0 };
    let l = alg_layout::blend(&alg_layout::layout(a), &alg_layout::layout(b), morph);
    // The links drawn: the loops keep clear of them.
    let mut links = [0u8; OPS];
    for src in 0..OPS {
        for dst in 0..OPS {
            let bit = 1 << dst;
            let w = blend(
                on(a.mods[src] & bit != 0),
                on(b.mods[src] & bit != 0),
                morph,
            );
            if w > 0.0 {
                links[src] |= bit;
                let ((x0, y0), (x1, y1)) = (l.pos[src], l.pos[dst]);
                let c = if w >= 0.5 { theme::MID } else { theme::FAINT };
                draw::line(d, x0, y0, x1, y1, c, 1);
            }
        }
    }
    crate::bank2::ui_door(|u| {
        u.notes(crate::ui::notes::Ask::Loops {
            on: &mut crate::ui::tape_strip::On(d),
            algo,
            at: &l,
            links,
        })
    });
    for (op, &label) in crate::part::NUMERALS[1..=OPS].iter().enumerate() {
        let (x, y) = l.pos[op];
        let c = blend(
            on(a.carriers & (1 << op) != 0),
            on(b.carriers & (1 << op) != 0),
            morph,
        );
        if c >= 0.5 {
            draw::dot(d, x, y, l.r, theme::INK2);
            draw::text_center(
                d,
                &theme::FONT_LABEL_BOLD,
                label,
                x + 1,
                y + 4,
                theme::BG,
                0,
            );
        } else {
            draw::dot(d, x, y, l.r, theme::BG);
            draw::ring(d, x, y, l.r, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, label, x + 1, y + 4, theme::MID, 0);
        }
    }
}

/// SPD's picture (the approved mockup): per ENV slot, its SPEED as three
/// pills and its HOLD POSITION's name; a type-B slot's column is faint.
pub fn env_speed<D>(d: &mut D, slots: [(bool, EnvSpeed, HoldPos); 3])
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, &(is_a, speed, hold)) in slots.iter().enumerate() {
        let x = theme::VIZ_LEFT + 4 + i as i32 * 74;
        let ink = if is_a { theme::MID } else { theme::FAINT };
        draw::text_center(
            d,
            &theme::FONT_LABEL,
            ["E1", "E2", "E3"][i],
            x + 30,
            50,
            ink,
            1,
        );
        for (k, name) in ["FAST", "MED", "SLOW"].iter().enumerate() {
            let y = 56 + k as i32 * 18;
            let lit = is_a && k == speed as usize;
            if lit {
                draw::pill(d, x + 6, y, 48, 14, theme::ACCENT);
            } else {
                draw::round_outline(d, x + 6, y, 48, 14, 7, theme::FAINT);
            }
            let c = if lit { theme::BG } else { ink };
            draw::text_center(d, &theme::FONT_LABEL_BOLD, name, x + 30, y + 10, c, 0);
        }
        let under = if is_a {
            ["OFF", "AHDSR", "GATE EXT"][hold as usize]
        } else {
            "RISE/FALL"
        };
        draw::text_center(d, &theme::FONT_LABEL, under, x + 30, 140, ink, 1);
    }
}
