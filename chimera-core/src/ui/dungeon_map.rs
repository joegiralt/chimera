use embedded_graphics::Drawable;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable};
use embedded_graphics::text::Text;

use crate::ui::chain::ChainNav;
use crate::ui::theme;

/// Render the dungeon map in the bottom zone of the screen.
/// Shows: separator, chain nodes as boxes, connector lines, vertical branches.
/// `branch_scroll_px` is the animated scroll offset in pixels for the sub-page list.
pub fn draw<D>(display: &mut D, nav: &ChainNav, branch_scroll_px: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let chain = nav.active_chain();

    // Separator line
    let _ = Line::new(
        Point::new(0, theme::MAP_TOP),
        Point::new(theme::SCREEN_W - 1, theme::MAP_TOP),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);

    // Chain name — small, top-left of map zone
    let dim_style = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
    let _ = Text::new(chain.name, Point::new(8, theme::MAP_TOP + 12), dim_style).draw(display);

    draw_nodes(display, chain, nav);
    draw_branches(display, nav, branch_scroll_px);
}

/// Draw the horizontal row of node boxes with connectors.
fn draw_nodes<D>(display: &mut D, chain: &crate::ui::block_def::ChainDef2, nav: &ChainNav)
where
    D: DrawTarget<Color = Rgb565>,
{
    let node_count = chain.blocks.len();
    let total_width =
        node_count as i32 * theme::NODE_WIDTH + (node_count as i32 - 1) * theme::NODE_GAP;
    let start_x = (theme::SCREEN_W - total_width) / 2;
    let y = theme::NODE_ROW_Y;

    for (i, block) in chain.blocks.iter().enumerate() {
        let x = start_x + i as i32 * (theme::NODE_WIDTH + theme::NODE_GAP);
        let is_active = i == nav.node;
        let short = block.def.short;

        // Connector line to next node
        if i + 1 < node_count {
            let line_y = y + theme::NODE_HEIGHT / 2;
            let _ = Line::new(
                Point::new(x + theme::NODE_WIDTH, line_y),
                Point::new(x + theme::NODE_WIDTH + theme::NODE_GAP, line_y),
            )
            .draw_styled(
                &PrimitiveStyle::with_stroke(theme::NODE_CONNECTOR, 1),
                display,
            );
        }

        // Node box
        let rect = Rectangle::new(
            Point::new(x, y),
            Size::new(theme::NODE_WIDTH as u32, theme::NODE_HEIGHT as u32),
        );

        if is_active {
            // Filled box
            let _ = rect.draw_styled(&PrimitiveStyle::with_fill(theme::NODE_ACTIVE_BG), display);
            // Label inside, dark on bright
            let text_style = MonoTextStyle::new(&FONT_6X10, theme::NODE_ACTIVE_TEXT);
            let tx = x + (theme::NODE_WIDTH - short.len() as i32 * 6) / 2;
            let ty = y + 11;
            let _ = Text::new(short, Point::new(tx, ty), text_style).draw(display);
        } else {
            // Outline only
            let _ = rect.draw_styled(
                &PrimitiveStyle::with_stroke(theme::NODE_INACTIVE_BORDER, 1),
                display,
            );
            let text_style = MonoTextStyle::new(&FONT_6X10, theme::NODE_INACTIVE_TEXT);
            let tx = x + (theme::NODE_WIDTH - short.len() as i32 * 6) / 2;
            let ty = y + 11;
            let _ = Text::new(short, Point::new(tx, ty), text_style).draw(display);
        }

        // Sub-page dot indicator below active node
        if is_active && !block.sub_pages.is_empty() {
            // Small tick mark below active node
            let tick_x = x + theme::NODE_WIDTH / 2;
            let _ = Line::new(
                Point::new(tick_x, y + theme::NODE_HEIGHT),
                Point::new(tick_x, y + theme::NODE_HEIGHT + 4),
            )
            .draw_styled(
                &PrimitiveStyle::with_stroke(theme::BRANCH_MARKER, 1),
                display,
            );
        }
    }
}

/// Draw vertical branch list below the active node (if it has sub-pages).
fn draw_branches<D>(display: &mut D, nav: &ChainNav, branch_scroll_px: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let block = match nav.active_chain_block() {
        Some(b) => b,
        None => return,
    };

    if block.sub_pages.is_empty() {
        return;
    }

    let chain = nav.active_chain();
    let node_count = chain.blocks.len();
    let total_width =
        node_count as i32 * theme::NODE_WIDTH + (node_count as i32 - 1) * theme::NODE_GAP;
    let start_x = (theme::SCREEN_W - total_width) / 2;
    let node_x = start_x + nav.node as i32 * (theme::NODE_WIDTH + theme::NODE_GAP);
    let branch_x = node_x + 4;

    // Build label list: first entry is the block's own def (sub_page 0),
    // then each sub_page def.
    // Total count = 1 + sub_pages.len()
    let count = block.sub_page_count();

    for i in 0..count {
        let label = if i == 0 {
            block.def.short
        } else {
            block.sub_pages[i - 1].short
        };

        // Apply animated scroll offset (in pixels)
        let y = theme::BRANCH_START_Y + i as i32 * theme::BRANCH_LINE_HEIGHT - branch_scroll_px;
        // Skip items scrolled above the branch area
        if y < theme::BRANCH_START_Y - theme::BRANCH_LINE_HEIGHT {
            continue;
        }
        // Skip items below screen
        if y >= chimera_hal::SCREEN_HEIGHT as i32 {
            break;
        }
        let is_active = i == nav.sub_page;

        // Branch connector: vertical line + horizontal tick
        let connector_color = if is_active {
            theme::BRANCH_MARKER
        } else {
            theme::NODE_CONNECTOR
        };

        // Vertical line segment
        let _ = Line::new(Point::new(branch_x, y), Point::new(branch_x, y + 10))
            .draw_styled(&PrimitiveStyle::with_stroke(connector_color, 1), display);

        // Horizontal tick
        let _ = Line::new(Point::new(branch_x, y + 5), Point::new(branch_x + 6, y + 5))
            .draw_styled(&PrimitiveStyle::with_stroke(connector_color, 1), display);

        let text_color = if is_active {
            theme::BRANCH_TEXT_ACTIVE
        } else {
            theme::BRANCH_TEXT
        };
        let style = MonoTextStyle::new(&FONT_6X10, text_color);

        // Active indicator: small filled rect
        if is_active {
            let _ = Rectangle::new(Point::new(branch_x + 8, y + 2), Size::new(3, 3))
                .draw_styled(&PrimitiveStyle::with_fill(theme::BRANCH_MARKER), display);
        }

        let _ = Text::new(label, Point::new(branch_x + 14, y + 9), style).draw(display);
    }
}
