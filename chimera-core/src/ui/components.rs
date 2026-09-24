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
