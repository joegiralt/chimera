//! Direction A shared components (UI refresh spec § Shared components):
//! header, focus band, cells. The map lives in `dungeon_map`.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::modal::{EXCITER_NAMES, ModalPage, ResonatorMode};
use crate::part::DacPair;
use crate::ui::PrimeStatus;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::chain::{ChainId, ChainNav};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::Gauge;
use crate::ui::theme;

/// `s` in upper case (names are stored mixed case: "Filter", "4opFM").
pub fn upper(s: &str) -> FmtBuf {
    let mut buf = FmtBuf::new();
    for ch in s.chars() {
        let _ = buf.write_char(ch.to_ascii_uppercase());
    }
    buf
}

/// Whether `def`'s cells are the exciter's (EXC): its name is the exciter's.
fn is_exciter(def: &BlockDef) -> bool {
    def.params
        .iter()
        .any(|s| matches!(s.binding, SlotBinding::ModalPanel(ModalPage::Exciter, _)))
}

/// A header's text: context, page name, and the OUT warning.
pub struct HeaderText {
    pub context: FmtBuf,
    pub name: FmtBuf,
    pub warn: Option<&'static str>,
}

/// `PART 2 · SOUND` or `PART 2 · MIX` and the page (`FILTER`, `SENDS`),
/// `SYSTEM`, `DEMO`; `suffix` follows the name (`/ B`). A page of the
/// exciter's cells is named after `model`'s exciter (PLUCK, STRIKE, BOW).
/// `OUT P2`/`OUT P3` warns on a Part's pages when `out` isn't P1 (ADR
/// 0057). A name too long for the line falls back to the page's short one.
pub fn header_text(
    nav: &ChainNav,
    def: &BlockDef,
    model: ResonatorMode,
    suffix: &str,
    out: DacPair,
) -> HeaderText {
    let mut context = FmtBuf::new();
    let _ = match nav.chain_id {
        ChainId::Part(n) => write!(context, "PART {} · SOUND", n + 1),
        ChainId::Mixer(n) => write!(context, "PART {} · MIX", n + 1),
        ChainId::System => context.write_str("SYSTEM"),
        ChainId::Demo => context.write_str("DEMO"),
    };
    let warn = match (nav.chain_id, out) {
        (ChainId::System | ChainId::Demo, _) | (_, DacPair::P1) => None,
        (_, DacPair::P2) => Some("OUT P2"),
        (_, DacPair::P3) => Some("OUT P3"),
    };
    let named = |full: bool| {
        let mut name = match (full, is_exciter(def)) {
            (true, true) => upper(EXCITER_NAMES[model as usize]),
            (true, false) => upper(def.name),
            (false, _) => upper(def.short),
        };
        let _ = name.write_str(suffix);
        name
    };
    let mut h = HeaderText {
        context,
        name: named(true),
        warn,
    };
    if !header_fits(&h) {
        h.name = named(false);
    }
    h
}

/// Right edge of the CPU readout and of the OUT warning.
const HEADER_RIGHT: i32 = theme::HEADER_DOT_X - 8;

/// Gap between the context and the name.
const HEADER_GAP: i32 = 7;
/// Least gap before what the header draws on the right.
const RIGHT_GAP: i32 = 6;

/// Where the context and name end.
fn header_end(context: &str, name: &str) -> i32 {
    theme::MARGIN_X
        + draw::text_width(&theme::FONT_LABEL, context, theme::LABEL_TRACKING)
        + HEADER_GAP
        + draw::text_width(&theme::FONT_LABEL_BOLD, name, theme::LABEL_TRACKING)
}

/// Whether the context and name end `RIGHT_GAP` short of the OUT warning,
/// or of the sounding dot.
pub fn header_fits(h: &HeaderText) -> bool {
    let right = match h.warn {
        Some(warn) => {
            HEADER_RIGHT - draw::text_width(&theme::FONT_LABEL, warn, theme::LABEL_TRACKING)
        }
        None => theme::HEADER_DOT_X - theme::HEADER_DOT_R,
    };
    header_end(h.context.as_str(), h.name.as_str()) + RIGHT_GAP <= right
}

/// Header band (y 0..28): grey context label, bold name, the OUT warning
/// or else the audio load when measured, and an accent dot while the
/// instrument is sounding.
pub fn header<D>(
    d: &mut D,
    context: &str,
    name: &str,
    warn: Option<&str>,
    sounding: bool,
    load_pct: u8,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            context,
            theme::MARGIN_X,
            y,
            theme::MID,
            theme::LABEL_TRACKING,
        );
    draw::text_tracked(
        d,
        &theme::FONT_LABEL_BOLD,
        name,
        x + HEADER_GAP,
        y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    if let Some(warn) = warn {
        draw::text_right(
            d,
            &theme::FONT_LABEL,
            warn,
            HEADER_RIGHT,
            y,
            theme::WARN,
            theme::LABEL_TRACKING,
        );
    } else if load_pct > 0 {
        let mut buf = FmtBuf::new();
        let _ = write!(buf, "CPU {}%", load_pct);
        let color = match load_pct {
            81.. => theme::ALERT,
            61..=80 => theme::WARN,
            _ => theme::MID,
        };
        // The bench readout yields to a long page name.
        let w = draw::text_width(&theme::FONT_LABEL, buf.as_str(), 0);
        if header_end(context, name) + RIGHT_GAP <= HEADER_RIGHT - w {
            draw::text_right(
                d,
                &theme::FONT_LABEL,
                buf.as_str(),
                HEADER_RIGHT,
                y,
                color,
                0,
            );
        }
    }
    if sounding {
        draw::dot(
            d,
            theme::HEADER_DOT_X,
            theme::HEADER_DOT_Y,
            theme::HEADER_DOT_R,
            theme::ACCENT,
        );
    }
}

/// Overlay title in the header band: grey context, an arrow, bold name
/// (`LOAD SOUND → PART 1`).
pub fn title_to<D>(d: &mut D, context: &str, name: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            context,
            theme::MARGIN_X,
            y,
            theme::MID,
            theme::LABEL_TRACKING,
        )
        + 6;
    let x = x + draw::arrow(d, x, y, theme::MID) + 5;
    draw::text_tracked(
        d,
        &theme::FONT_LABEL_BOLD,
        name,
        x,
        y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
}

/// Focus band (y 28..118): the focused slot's label, its value large, and
/// its `gauge`. A dimmed or absent slot reads as its cell does, with no
/// gauge: a dimmed value in MID, an absent route's dash in INK2.
///
/// While a MIX+PLUS `status` is pending (issue #21) the value readout — the
/// large numerals and the arc gauge — is replaced by the status word(s) at
/// the mid-size value font, so the longest message (`NOT MODULATABLE`)
/// still fits the full row width; the label above is unchanged, so the
/// message still reads against the parameter it was tried on.
pub fn focus_band<D>(
    d: &mut D,
    label: &str,
    value_text: &str,
    gauge: Gauge,
    look: Look,
    status: Option<PrimeStatus>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    focus_label(d, label, theme::MARGIN_X);
    match status {
        Some(status) => {
            draw::text_tracked(
                d,
                &theme::FONT_VALUE,
                status.label(),
                theme::MARGIN_X,
                theme::FOCUS_VALUE_Y,
                theme::INK,
                theme::LABEL_TRACKING,
            );
        }
        None if look != Look::Live => {
            let color = match look {
                Look::Absent => theme::INK2,
                _ => theme::MID,
            };
            draw::text(
                d,
                &theme::FONT_FOCUS,
                value_text,
                theme::FOCUS_VALUE_X,
                theme::FOCUS_VALUE_Y,
                color,
            );
        }
        None => focus_value(d, value_text, gauge),
    }
}

/// A BigViz page's prime status (issue #21): one line at the top of the viz
/// band, in the focus band's status style, between the header and
/// `viz::PLOT_TOP` — above every curve and the touched-value readout, which
/// is clamped to the plot. The strip behind the text is cleared first so a
/// curve point at the plot's top edge can't run into the letters.
pub fn viz_status<D>(d: &mut D, status: PrimeStatus)
where
    D: DrawTarget<Color = Rgb565>,
{
    let label = status.label();
    let w = draw::text_width(&theme::FONT_VALUE, label, theme::LABEL_TRACKING);
    draw::fill_rect(
        d,
        theme::MARGIN_X - 2,
        theme::HEADER_BOTTOM,
        w + 4,
        crate::ui::viz::PLOT_TOP - theme::HEADER_BOTTOM,
        theme::BG,
    );
    draw::text_tracked(
        d,
        &theme::FONT_VALUE,
        label,
        theme::MARGIN_X,
        theme::BIGVIZ_STATUS_Y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
}

/// Focus label at `x`; returns where it ends.
fn focus_label<D>(d: &mut D, label: &str, x: i32) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    x + draw::text_tracked(
        d,
        &theme::FONT_VALUE,
        label,
        x,
        theme::FOCUS_LABEL_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    )
}

/// The value large and its gauge; with none, the text has the band.
fn focus_value<D>(d: &mut D, value_text: &str, gauge: Gauge)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text(
        d,
        &theme::FONT_FOCUS,
        value_text,
        theme::FOCUS_VALUE_X,
        theme::FOCUS_VALUE_Y,
        theme::INK,
    );
    match gauge {
        Gauge::None => {}
        Gauge::Switch { on } => switch(d, on),
        Gauge::Arc { value, bipolar } => draw::arc_gauge(
            d,
            theme::ARC_CX,
            theme::ARC_CY,
            theme::ARC_R,
            theme::ARC_WIDTH,
            value,
            bipolar,
            theme::FAINT,
            theme::ACCENT,
        ),
    }
}

/// The SWITCH glyph at the band's right: a faint pill track, lit in the
/// accent up to the knob, which slides with `on` (0..1) and reads INK on,
/// MID off.
fn switch<D>(d: &mut D, on: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (w, h) = (theme::SWITCH_W, theme::SWITCH_H);
    let (x, y) = (theme::SWITCH_CX - w / 2, theme::ARC_CY - h / 2);
    let on = on.clamp(0.0, 1.0);
    let knob = x + h / 2 + libm::roundf(on * (w - h) as f32) as i32;
    draw::pill(d, x, y, w, h, theme::FAINT);
    if on > 0.0 {
        draw::pill(d, x, y, knob + h / 2 - x, h, theme::ACCENT);
    }
    let color = if on >= 0.5 { theme::INK } else { theme::MID };
    draw::dot(d, knob, theme::ARC_CY, h / 2 - 4, color);
}

/// How a cell reads (spec § UI). The discriminants pack into the Cells
/// region's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Look {
    Live = 0,
    /// A route knob with no route: a dash where the value goes, no bar.
    Absent = 1,
    /// Fixed or inapplicable: label and value in MID, no bar.
    Dimmed = 2,
}

/// One cell of the 3×2 grid.
pub struct Cell<'a> {
    pub label: &'a str,
    /// Formatted value.
    pub text: &'a str,
    /// Animated 0..1 value for the bar.
    pub value: f32,
    pub fmt: crate::block::ValFmt,
    /// The focused slot: accent label and bar.
    pub active: bool,
    /// Summed mod amount (−1..1) when the param is a mod destination.
    pub mod_amount: Option<f32>,
    pub look: Look,
}

/// Draw cell `i` (knob order a–f, 3×2) with its label baseline `top + row·36`.
/// `None` is an empty slot: a dim dash.
pub fn cell<D>(d: &mut D, i: usize, top: i32, cell: Option<&Cell>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = theme::MARGIN_X + (i % 3) as i32 * theme::CELL_COL_W;
    let y = top + (i / 3) as i32 * theme::CELL_ROW_H;
    let Some(c) = cell else {
        draw::fill_rect(d, x, y - 3, 8, 1, theme::FAINT);
        return;
    };
    let dim = c.look == Look::Dimmed;
    let label_color = if c.active && !dim {
        theme::ACCENT
    } else {
        theme::MID
    };
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        c.label,
        x,
        y,
        label_color,
        theme::LABEL_TRACKING,
    );
    if c.look == Look::Absent {
        draw::fill_rect(d, x, y + theme::CELL_VALUE_DY - 5, 12, 2, theme::INK2);
        return;
    }
    let value_color = match (dim, c.active) {
        (true, _) => theme::MID,
        (false, true) => theme::INK,
        (false, false) => theme::INK2,
    };
    draw::text(
        d,
        &theme::FONT_VALUE,
        c.text,
        x,
        y + theme::CELL_VALUE_DY,
        value_color,
    );
    if !c.fmt.is_discrete() && !dim {
        let fill = if c.active {
            theme::ACCENT
        } else {
            theme::BAR_REST
        };
        draw::bar(
            d,
            x,
            y + theme::CELL_BAR_DY,
            theme::CELL_BAR_W,
            theme::CELL_BAR_H,
            c.value,
            c.fmt.is_bipolar(),
            theme::FAINT,
            fill,
        );
    }
    if let Some(m) = c.mod_amount.filter(|_| !dim) {
        let mid = x + theme::CELL_BAR_W / 2;
        let len = (m.clamp(-1.0, 1.0) * (theme::CELL_BAR_W / 2) as f32) as i32;
        let (x0, x1) = if len >= 0 {
            (mid, mid + len)
        } else {
            (mid + len, mid)
        };
        draw::fill_rect(d, mid, y + theme::CELL_MOD_DY - 1, 1, 3, theme::MID);
        draw::fill_rect(
            d,
            x0,
            y + theme::CELL_MOD_DY,
            (x1 - x0).max(1),
            1,
            theme::INK2,
        );
    }
}
