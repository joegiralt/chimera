//! Direction A shared components (UI refresh spec § Shared components):
//! header, focus band, cells. The map lives in `dungeon_map`.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::addr::BlockRef;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::chain::{ChainId, ChainNav};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

/// `s` in upper case (names are stored mixed case: "Filter", "4opFM").
pub fn upper(s: &str) -> FmtBuf {
    let mut buf = FmtBuf::new();
    for ch in s.chars() {
        let _ = buf.write_char(ch.to_ascii_uppercase());
    }
    buf
}

/// Whether any slot of `def` edits the Part's own mix settings (PART, SENDS).
fn edits_part(def: &BlockDef) -> bool {
    def.params.iter().any(|s| matches!(s.binding, SlotBinding::Param(a) if a.block == BlockRef::Part))
}

/// Header context label and page name: `PART 1` `FILTER`; on the Mixer
/// chain `MIXER` and the page, numbered when it edits that Part (`PART 2`,
/// `SENDS 2`; the FX are shared, so `CHORUS`).
pub fn header_text(nav: &ChainNav, def: &BlockDef) -> (FmtBuf, FmtBuf) {
    let mut context = FmtBuf::new();
    let mut name = upper(def.name);
    let _ = match nav.chain_id {
        ChainId::Part(n) => write!(context, "PART {}", n + 1),
        ChainId::Mixer(n) => {
            if edits_part(def) {
                let _ = write!(name, " {}", n + 1);
            }
            context.write_str("MIXER")
        }
        ChainId::System => context.write_str("SYSTEM"),
        ChainId::Demo => context.write_str("DEMO"),
    };
    (context, name)
}

/// Header band (y 0..28): grey context label, bold name, the audio load
/// when measured, and an accent dot while the instrument is sounding.
pub fn header<D>(d: &mut D, context: &str, name: &str, sounding: bool, load_pct: u8)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(d, &theme::FONT_LABEL, context, theme::MARGIN_X, y, theme::MID, theme::LABEL_TRACKING);
    draw::text_tracked(d, &theme::FONT_LABEL_BOLD, name, x + 7, y, theme::INK, theme::LABEL_TRACKING);
    if load_pct > 0 {
        let mut buf = FmtBuf::new();
        let _ = write!(buf, "CPU {}%", load_pct);
        let color = match load_pct {
            81.. => theme::ALERT,
            61..=80 => theme::WARN,
            _ => theme::MID,
        };
        draw::text_right(d, &theme::FONT_LABEL, buf.as_str(), theme::HEADER_DOT_X - 8, y, color, 0);
    }
    if sounding {
        draw::dot(d, theme::HEADER_DOT_X, theme::HEADER_DOT_Y, theme::HEADER_DOT_R, theme::ACCENT);
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
        + draw::text_tracked(d, &theme::FONT_LABEL, context, theme::MARGIN_X, y, theme::MID, theme::LABEL_TRACKING)
        + 6;
    let x = x + draw::arrow(d, x, y, theme::MID) + 5;
    draw::text_tracked(d, &theme::FONT_LABEL_BOLD, name, x, y, theme::INK, theme::LABEL_TRACKING);
}

/// Focus band (y 28..118): the focused slot's label, its value large, and an
/// arc gauge (from 12:00 for bipolar params). `value` is the animated 0..1.
pub fn focus_band<D>(d: &mut D, label: &str, value_text: &str, value: f32, bipolar: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    focus_label(d, label, theme::MARGIN_X);
    focus_value(d, value_text, value, bipolar);
}

/// Focus label at `x`; returns where it ends.
fn focus_label<D>(d: &mut D, label: &str, x: i32) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    x + draw::text_tracked(d, &theme::FONT_VALUE, label, x, theme::FOCUS_LABEL_Y, theme::MID, theme::LABEL_TRACKING)
}

fn focus_value<D>(d: &mut D, value_text: &str, value: f32, bipolar: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text(d, &theme::FONT_FOCUS, value_text, theme::FOCUS_VALUE_X, theme::FOCUS_VALUE_Y, theme::INK);
    draw::arc_gauge(d, theme::ARC_CX, theme::ARC_CY, theme::ARC_R, theme::ARC_WIDTH, value, bipolar, theme::FAINT, theme::ACCENT);
}

/// Mod matrix focus band: the selected route `SOURCE → DEST` (`LFO → OP1
/// LEVEL`) and its bipolar amount.
pub fn focus_route<D>(d: &mut D, source: &str, dest: &str, value_text: &str, value: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = focus_label(d, source, theme::MARGIN_X) + 6;
    let x = x + draw::arrow(d, x, theme::FOCUS_LABEL_Y, theme::MID) + 6;
    focus_label(d, dest, x);
    focus_value(d, value_text, value, true);
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
    let label_color = if c.active { theme::ACCENT } else { theme::MID };
    draw::text_tracked(d, &theme::FONT_LABEL, c.label, x, y, label_color, theme::LABEL_TRACKING);
    let value_color = if c.active { theme::INK } else { theme::INK2 };
    draw::text(d, &theme::FONT_VALUE, c.text, x, y + theme::CELL_VALUE_DY, value_color);
    if !c.fmt.is_discrete() {
        let fill = if c.active { theme::ACCENT } else { theme::BAR_REST };
        draw::bar(d, x, y + theme::CELL_BAR_DY, theme::CELL_BAR_W, theme::CELL_BAR_H, c.value, c.fmt.is_bipolar(), theme::FAINT, fill);
    }
    if let Some(m) = c.mod_amount {
        let mid = x + theme::CELL_BAR_W / 2;
        let len = (m.clamp(-1.0, 1.0) * (theme::CELL_BAR_W / 2) as f32) as i32;
        let (x0, x1) = if len >= 0 { (mid, mid + len) } else { (mid + len, mid) };
        draw::fill_rect(d, mid, y + theme::CELL_MOD_DY - 1, 1, 3, theme::MID);
        draw::fill_rect(d, x0, y + theme::CELL_MOD_DY, (x1 - x0).max(1), 1, theme::INK2);
    }
}
