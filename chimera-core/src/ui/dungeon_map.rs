//! The chain map (y 266..320, Direction A): nodes on a thin line, the
//! current block a filled accent pill with a dark label, the others a small
//! ring with a grey label below. A block's sub-pages hang under the pill as
//! indented nodes, the current one lit.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::modal::{MODEL_NAMES, ResonatorMode};
use crate::ui::block_def::{BlockDef, ChainDef2};
use crate::ui::block_registry::MODAL_2;
use crate::ui::draw;
use crate::ui::nav::PageAt;
use crate::ui::theme;

/// Centre x of node `i` of `n`, spread evenly over the map line.
pub fn node_x(i: usize, n: usize) -> i32 {
    if n <= 1 {
        theme::SCREEN_W / 2
    } else {
        theme::MAP_X0 + (theme::MAP_X1 - theme::MAP_X0) * i as i32 / (n as i32 - 1)
    }
}

/// A page's name on the map: MDL2 is named after `model`.
pub fn page_label(def: &BlockDef, model: ResonatorMode) -> &'static str {
    if def.id == MODAL_2.id {
        MODEL_NAMES[model as usize]
    } else {
        def.short
    }
}

/// Draw `chain`'s map at `at`. `branch_scroll_px` scrolls the sub-page
/// list (animated).
pub fn draw<D>(
    d: &mut D,
    chain: &ChainDef2,
    at: PageAt,
    model: ResonatorMode,
    branch_scroll_px: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let node = at.node as usize;
    let n = chain.blocks.len();
    if n > 1 {
        draw::fill_rect(
            d,
            theme::MAP_X0,
            theme::MAP_LINE_Y,
            theme::MAP_X1 - theme::MAP_X0,
            1,
            theme::FAINT,
        );
    }
    for (i, block) in chain.blocks.iter().enumerate() {
        let x = node_x(i, n);
        let label = block.map.unwrap_or(block.def.short);
        if i == node {
            pill_node(d, x, theme::MAP_LINE_Y, label, theme::ACCENT);
        } else {
            ring_node(d, x, theme::MAP_LINE_Y, label, theme::NODE_LABEL_Y);
        }
    }
    draw_branches(d, chain, at, model, node_x(node, n), branch_scroll_px);
}

/// A current node: a `fill` pill centred on (x, cy) with a dark bold label.
pub fn pill_node<D>(d: &mut D, x: i32, cy: i32, label: &str, fill: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::pill(
        d,
        x - theme::PILL_W / 2,
        cy - theme::PILL_H / 2,
        theme::PILL_W,
        theme::PILL_H,
        fill,
    );
    draw::text_center(
        d,
        &theme::FONT_LABEL_BOLD,
        label,
        x,
        cy + theme::PILL_LABEL_Y - theme::MAP_LINE_Y,
        theme::BG,
        0,
    );
}

/// Any other node: a small ring on the line, its grey label below at `label_y`.
pub fn ring_node<D>(d: &mut D, x: i32, cy: i32, label: &str, label_y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::dot(d, x, cy, theme::NODE_R, theme::BG);
    draw::ring(d, x, cy, theme::NODE_R, theme::MID, 1);
    draw::text_center(d, &theme::FONT_LABEL, label, x, label_y, theme::MID, 0);
}

/// Sub-pages of the current block, under its pill.
fn draw_branches<D>(
    d: &mut D,
    chain: &ChainDef2,
    at: PageAt,
    model: ResonatorMode,
    pill_x: i32,
    branch_scroll_px: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let Some(block) = chain.block_at(at.node as usize) else {
        return;
    };
    let count = block.sub_page_count();
    if count == 0 {
        return;
    }
    let x = pill_x - 8;
    let pill_bottom = theme::MAP_LINE_Y + theme::PILL_H / 2;
    let visible = |i: i32| {
        let y = theme::BRANCH_START_Y + i * theme::BRANCH_LINE_HEIGHT - branch_scroll_px;
        (y >= theme::BRANCH_START_Y && y + theme::BRANCH_LINE_HEIGHT <= theme::SCREEN_H)
            .then_some(y)
    };
    // Trunk first, so a lit dot's fill draws over it instead of the other
    // way round (#29).
    if let Some(last_y) = (0..count as i32).rev().find_map(visible) {
        let cy = last_y + theme::BRANCH_LINE_HEIGHT / 2;
        draw::fill_rect(d, x, pill_bottom, 1, cy - 3 - pill_bottom, theme::FAINT);
    }
    for i in 0..count as i32 {
        let Some(y) = visible(i) else { continue };
        let def = if i == 0 {
            block.def
        } else {
            block.sub_pages[i as usize - 1]
        };
        let label = page_label(def, model);
        let cy = y + theme::BRANCH_LINE_HEIGHT / 2;
        if i == at.sub as i32 {
            draw::dot(d, x, cy, 2, theme::ACCENT);
            draw::text(d, &theme::FONT_LABEL, label, x + 6, y + 8, theme::ACCENT);
        } else {
            draw::ring(d, x, cy, 2, theme::MID, 1);
            draw::text(d, &theme::FONT_LABEL, label, x + 6, y + 8, theme::MID);
        }
    }
}
