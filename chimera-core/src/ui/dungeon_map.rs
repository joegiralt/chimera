//! The chain map (y 266..320, Direction A): nodes on a thin line, the
//! current block a filled accent pill with a dark label, the others a small
//! ring with a grey label below. THEME's PAGES (ADR 0145) picks how a
//! block's sub-pages show: ROW, a line along the bottom, the current one
//! lit and underlined; STACK, hung under the pill as indented nodes.

use core::ops::Range;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::modal::MODEL_NAMES;
use crate::dsp::tape_engine::machine::machine;
use crate::ui::block_def::{BlockDef, ChainDef2};
use crate::ui::block_registry as reg;
use crate::ui::block_registry::{MODAL_2, TAPE_MODEL};
use crate::ui::draw;
use crate::ui::nav::PageAt;
use crate::ui::theme;
use crate::ui::theme_settings::Pages;
use crate::ui::view::Models;

/// Centre x of node `i` of `n`, spread evenly over the map line.
pub fn node_x(i: usize, n: usize) -> i32 {
    if n <= 1 {
        theme::SCREEN_W / 2
    } else {
        theme::MAP_X0 + (theme::MAP_X1 - theme::MAP_X0) * i as i32 / (n as i32 - 1)
    }
}

/// A page's name on the map: MDL2 is named after MODAL's model, TAPE's
/// model page after its machine (`STRCH`).
pub fn page_label(def: &BlockDef, models: impl Into<Models>) -> &'static str {
    let m = models.into();
    match def.id {
        id if id == MODAL_2.id => MODEL_NAMES[m.modal as usize],
        id if id == TAPE_MODEL.id => machine(m.tape).page,
        _ => def.short,
    }
}

/// The row's word for each page on a node with sub-pages: the word a
/// musician reads, in full where it fits (ADR 0145). STACK and the crumbs
/// keep `short`. MDL2 and TAPE's model page are named at draw time.
pub static ROW_WORDS: [(&BlockDef, &str); 68] = [
    (&reg::ALGO_ALG, "ALGORITHM"),
    (&reg::PITCH, "PITCH"),
    (&reg::ALGO_WAVE, "WAVE"),
    (&reg::ALGO_COARSE, "COARSE"),
    (&reg::ALGO_FINE, "FINE"),
    (&reg::ALGO_DETUNE, "DETUNE"),
    (&reg::ALGO_LEVEL, "LEVEL"),
    (&reg::ALGO_VELOCITY, "VELOCITY"),
    (&reg::ALGO_AR, "ATTACK"),
    (&reg::ALGO_D1R, "DECAY 1"),
    (&reg::ALGO_D1L, "SUSTAIN"),
    (&reg::ALGO_D2R, "DECAY 2"),
    (&reg::ALGO_RR, "RELEASE"),
    (&reg::ALGO_RATE_SCALE, "RATE SCALE"),
    (&reg::ALGO_FEEDBACK, "FEEDBACK"),
    (&reg::ALGO_FORMANT, "FORMANT"),
    (&reg::ALGO_OP_PAGES[0], "OPERATOR 1"),
    (&reg::ALGO_OP_PAGES[1], "OPERATOR 2"),
    (&reg::ALGO_OP_PAGES[2], "OPERATOR 3"),
    (&reg::ALGO_OP_PAGES[3], "OPERATOR 4"),
    (&reg::ALGO_OP_PAGES[4], "OPERATOR 5"),
    (&reg::ALGO_OP_PAGES[5], "OPERATOR 6"),
    (&reg::FILTER, "FILTER"),
    (&reg::FILTER_MOD, "MODULATION"),
    (&reg::FILTER_FX, "FX"),
    (&reg::FILTER_JACKS, "JACKS"),
    (&reg::AMP, "AMP"),
    (&reg::SENDS, "SENDS"),
    (&reg::MOD_MATRIX, "MATRIX"),
    (&reg::ENVELOPE, "ENV1"),
    (&reg::ENV_2, "ENV2"),
    (&reg::ENV_3, "ENV3"),
    (&reg::ENV_SPEED, "SPEED"),
    (&reg::LFO, "LFO1"),
    (&reg::LFO_2, "LFO2"),
    (&reg::LFO_3, "LFO3"),
    (&reg::MODAL_1, "RESONATOR"),
    (&reg::TAPE_DECK, "DECK"),
    (&reg::VA_HOME, "VA"),
    (&reg::VA_OSC1, "OSC1"),
    (&reg::VA_OSC2, "OSC2"),
    (&reg::VA_NOISE, "NOISE"),
    (&reg::DRUM_KIT, "KIT"),
    (&reg::DRUM_KEYS, "KEYS"),
    (&reg::DRUM_MCH, "MACHINE"),
    (&reg::DRUM_MORE, "MORE"),
    (&reg::DRUM_FLT, "FILTER"),
    (&reg::DRUM_AMP, "AMP"),
    (&reg::DRUM_GATE, "GATE"),
    (&reg::DRUM_PENV, "PAD ENV"),
    (&reg::DRUM_PLFO, "PAD LFO"),
    (&reg::DRUM_ROUTE, "ROUTE"),
    (&reg::DELAY, "DELAY"),
    (&reg::DELAY_CHAR, "CHARACTER"),
    (&reg::DELAY_GRAIN, "GRAIN"),
    (&reg::EFX, "REVERB"),
    (&reg::EFX_MORE, "MORE"),
    (&reg::EFX_MORE2, "MORE 2"),
    (&reg::PART, "PART"),
    (&reg::PART_VOICES, "VOICES"),
    (&reg::PART_EQ, "EQ"),
    (&reg::MASTER, "MASTER"),
    (&reg::MASTER_LEVEL, "LEVEL"),
    (&crate::ui::orbit::ORBIT_ORB, "ORB"),
    (&crate::ui::orbit::ORBIT_CUSTOM, "CUSTOM"),
    (&crate::ui::orbit::ORBIT_RHYTHM, "RHYTHM"),
    (&crate::ui::orbit::ORBIT_VOICE, "VOICE"),
    (&crate::ui::orbit::ORBIT_NOTES, "NOTES"),
];

/// A page's word on the row: `ROW_WORDS`, MDL2 by its model, TAPE's model
/// page by its machine in full (`STRETCH`), else `page_label`.
pub fn row_label(def: &BlockDef, models: impl Into<Models>) -> &'static str {
    let m = models.into();
    match def.id {
        id if id == MODAL_2.id => MODEL_NAMES[m.modal as usize],
        id if id == TAPE_MODEL.id => {
            crate::dsp::tape_engine::MACHINE_NAMES[m.tape.index() as usize]
        }
        id => ROW_WORDS
            .iter()
            .find(|w| w.0.id == id)
            .map_or_else(|| page_label(def, m), |w| w.1),
    }
}

/// Draw `chain`'s map at `at`, its sub-pages as `pages` says.
/// `branch_scroll_px` scrolls STACK's list (animated).
pub fn draw<D>(
    d: &mut D,
    chain: &ChainDef2,
    at: PageAt,
    models: impl Into<Models>,
    pages: Pages,
    branch_scroll_px: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    match pages {
        Pages::Row => draw_row(d, chain, at, models.into()),
        Pages::Stack => draw_stack(d, chain, at, models.into(), branch_scroll_px),
    }
}

/// STACK: the sub-pages hang under the pill.
fn draw_stack<D>(d: &mut D, chain: &ChainDef2, at: PageAt, models: Models, branch_scroll_px: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let node = at.node() as usize;
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
    // The sub-pages' labels own their rows: a node label they'd run into,
    // on the right (DECK's `DECK` into `FLT`) or, hanging left, on the
    // left (DRUM's `PENV` into `AMP`), gives way.
    let (left, right) = branch_span(chain, at, models, node_x(node, n));
    for (i, block) in chain.blocks.iter().enumerate() {
        let x = node_x(i, n);
        let mut label = block.map.unwrap_or(block.def.short);
        let half = draw::text_width(&theme::FONT_LABEL, label, 0) / 2;
        if (i > node && x - half < right) || (i < node && x + half > left) {
            label = "";
        }
        if i == node {
            pill_node(d, x, theme::MAP_LINE_Y, label, theme::ACCENT);
        } else {
            ring_node(d, x, theme::MAP_LINE_Y, label, theme::NODE_LABEL_Y);
        }
    }
    draw_branches(d, chain, at, models, node_x(node, n), branch_scroll_px);
}

/// The span the current node's sub-page labels take: `(SCREEN_W, right
/// edge)` hanging right of the trunk, `(left edge, 0)` hanging left (as
/// `draw_branches` hangs them), `(SCREEN_W, 0)` with none.
fn branch_span(chain: &ChainDef2, at: PageAt, models: Models, pill_x: i32) -> (i32, i32) {
    let Some(block) = chain.block_at(at.node() as usize) else {
        return (theme::SCREEN_W, 0);
    };
    let count = block.sub_page_count();
    let widest = (0..count)
        .map(|i| {
            let def = if i == 0 {
                block.def
            } else {
                block.sub_pages[i - 1]
            };
            draw::text_width(&theme::FONT_LABEL, page_label(def, models), 0)
        })
        .max()
        .unwrap_or(0);
    let x = pill_x - 8;
    match (count, x + 6 + widest > theme::SCREEN_W) {
        (0, _) => (theme::SCREEN_W, 0),
        (_, true) => (x - 6 - widest, 0),
        (_, false) => (theme::SCREEN_W, x + 6 + widest),
    }
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
    model: Models,
    pill_x: i32,
    branch_scroll_px: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let Some(block) = chain.block_at(at.node() as usize) else {
        return;
    };
    let count = block.sub_page_count();
    if count == 0 {
        return;
    }
    let x = pill_x - 8;
    let def_at = |i: usize| {
        if i == 0 {
            block.def
        } else {
            block.sub_pages[i - 1]
        }
    };
    // Labels that would run off the right hang left of the trunk.
    let widest = (0..count)
        .map(|i| draw::text_width(&theme::FONT_LABEL, page_label(def_at(i), model), 0))
        .max()
        .unwrap_or(0);
    let hang_left = x + 6 + widest > theme::SCREEN_W;
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
        let label = page_label(def_at(i as usize), model);
        let cy = y + theme::BRANCH_LINE_HEIGHT / 2;
        let color = if i == at.sub() as i32 {
            draw::dot(d, x, cy, 2, theme::ACCENT);
            theme::ACCENT
        } else {
            draw::ring(d, x, cy, 2, theme::MID, 1);
            theme::MID
        };
        if hang_left {
            draw::text_right(d, &theme::FONT_LABEL, label, x - 6, y + 8, color, 0);
        } else {
            draw::text(d, &theme::FONT_LABEL, label, x + 6, y + 8, color);
        }
    }
}

/// The pages the row shows: grown from `cur` outward, a page right then a
/// page left, while they fit `room` with `ROW_GAP` between. Always holds
/// `cur`; can't grow either way. Empty for no pages.
pub fn page_window(widths: &[i32], cur: usize, room: i32) -> Range<usize> {
    let Some(&w) = widths.get(cur) else {
        return 0..0;
    };
    let (mut a, mut b, mut used) = (cur, cur + 1, w);
    loop {
        let mut grew = false;
        if let Some(&w) = widths.get(b)
            && used + theme::ROW_GAP + w <= room
        {
            used += theme::ROW_GAP + w;
            b += 1;
            grew = true;
        }
        if a > 0 && used + theme::ROW_GAP + widths[a - 1] <= room {
            a -= 1;
            used += theme::ROW_GAP + widths[a];
            grew = true;
        }
        if !grew {
            return a..b;
        }
    }
}

/// The most sub-pages the row measures; every chain keeps within it
/// (`page_row_test`).
pub const ROW_MAX: usize = 32;

/// ROW: the nodes on one line, the current node's sub-pages along the
/// bottom, windowed round the current one, `<` and `>` where it's cut.
fn draw_row<D>(d: &mut D, chain: &ChainDef2, at: PageAt, models: Models)
where
    D: DrawTarget<Color = Rgb565>,
{
    let node = at.node() as usize;
    let n = chain.blocks.len();
    let y = theme::ROW_LINE_Y;
    if n > 1 {
        draw::fill_rect(
            d,
            theme::MAP_X0,
            y,
            theme::MAP_X1 - theme::MAP_X0,
            1,
            theme::FAINT,
        );
    }
    let t = theme::LABEL_TRACKING;
    for (i, block) in chain.blocks.iter().enumerate() {
        let x = node_x(i, n);
        let label = block.map.unwrap_or(block.def.short);
        if i == node {
            let w = theme::PILL_W
                .max(draw::text_width(&theme::FONT_LABEL_BOLD, label, t) + theme::ROW_PILL_PAD);
            draw::pill(
                d,
                x - w / 2,
                y - theme::ROW_PILL_H / 2,
                w,
                theme::ROW_PILL_H,
                theme::ACCENT,
            );
            draw::text_center(
                d,
                &theme::FONT_LABEL_BOLD,
                label,
                x,
                theme::ROW_PILL_LABEL_Y,
                theme::BG,
                t,
            );
        } else {
            draw::dot(d, x, y, theme::ROW_NODE_R, theme::BG);
            draw::ring(d, x, y, theme::ROW_NODE_R, theme::MID, 1);
            draw::text_center(
                d,
                &theme::FONT_LABEL,
                label,
                x,
                theme::ROW_NODE_LABEL_Y,
                theme::MID,
                t,
            );
        }
    }
    let Some(block) = chain.block_at(node) else {
        return;
    };
    let count = block.sub_page_count().min(ROW_MAX);
    if count == 0 {
        return;
    }
    let cur = at.sub() as usize;
    let label = |i: usize| {
        let def = if i == 0 {
            block.def
        } else {
            block.sub_pages[i - 1]
        };
        row_label(def, models)
    };
    let font = |i: usize| {
        if i == cur {
            &theme::FONT_LABEL_BOLD
        } else {
            &theme::FONT_LABEL
        }
    };
    let mut widths = [0; ROW_MAX];
    for (i, w) in widths[..count].iter_mut().enumerate() {
        *w = draw::text_width(font(i), label(i), t);
    }
    let widths = &widths[..count];
    let shown = page_window(widths, cur, theme::ROW_R - theme::ROW_L);
    if shown.is_empty() {
        return;
    }
    let ry = theme::ROW_Y;
    draw::fill_rect(
        d,
        theme::MARGIN_X,
        theme::ROW_RULE_Y,
        theme::CONTENT_W,
        1,
        theme::FAINT,
    );
    if shown.start > 0 {
        draw::text_tracked(
            d,
            &theme::FONT_LABEL_BOLD,
            "<",
            theme::ROW_ARROW_L,
            ry,
            theme::INK2,
            t,
        );
    }
    if shown.end < count {
        draw::text_right(
            d,
            &theme::FONT_LABEL_BOLD,
            ">",
            theme::ROW_ARROW_R,
            ry,
            theme::INK2,
            t,
        );
    }
    let used: i32 =
        widths[shown.clone()].iter().sum::<i32>() + theme::ROW_GAP * (shown.len() as i32 - 1);
    let mut x = theme::ROW_L + (theme::ROW_R - theme::ROW_L - used) / 2;
    for i in shown {
        let on = i == cur;
        let color = if on { theme::ACCENT } else { theme::MID };
        draw::text_tracked(d, font(i), label(i), x, ry, color, t);
        if on {
            draw::fill_rect(d, x, ry + 3, widths[i], 2, theme::ACCENT);
        }
        x += widths[i] + theme::ROW_GAP;
    }
}
