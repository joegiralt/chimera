//! THEME's PAGES ROW (ADR 0145): the window the sub-page row shows, and
//! its arrows on every chain.

mod common;
mod screen;

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::ui::block_def::ChainDef2;
use chimera_core::ui::block_registry::{
    ALGO_CHAIN, MIXER_CHANNEL_CHAIN, MODAL_PLUCK_CHAIN, TAPE_CHAIN, VA_CHAIN,
};
use chimera_core::ui::draw::text_width;
use chimera_core::ui::dungeon_map::{self, page_label, page_window};
use chimera_core::ui::theme::{self, ROW_GAP};
use chimera_core::ui::theme_settings::Pages;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use screen::Fb;

/// `r`'s pages' widths with the gaps between.
fn used(w: &[i32], r: &core::ops::Range<usize>) -> i32 {
    w[r.clone()].iter().sum::<i32>() + ROW_GAP * (r.len() as i32 - 1)
}

/// Every window holds `cur`, fits `room`, and can't take a page more on
/// either side.
fn check(w: &[i32], cur: usize, room: i32) {
    let r = page_window(w, cur, room);
    assert!(r.contains(&cur), "{w:?} {cur} {room}: {r:?}");
    assert!(
        used(w, &r) <= room || r.len() == 1,
        "{w:?} {cur} {room}: {r:?} overflows"
    );
    if r.start > 0 {
        assert!(
            used(w, &r) + ROW_GAP + w[r.start - 1] > room,
            "{w:?} {cur}: grows left"
        );
    }
    if r.end < w.len() {
        assert!(
            used(w, &r) + ROW_GAP + w[r.end] > room,
            "{w:?} {cur}: grows right"
        );
    }
}

#[test]
fn the_window_holds_the_current_page_and_is_maximal() {
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..2_000 {
        let n = 1 + next() as usize % 20;
        let w: Vec<i32> = (0..n).map(|_| 8 + (next() % 30) as i32).collect();
        let room = 20 + (next() % 220) as i32;
        for cur in 0..n {
            check(&w, cur, room);
        }
    }
}

#[test]
fn the_window_cuts_only_what_does_not_fit() {
    // All fit: the whole row, no arrows.
    assert_eq!(page_window(&[20, 20, 20], 1, 196), 0..3);
    // 20 a page, 10 between: four fill 110.
    assert_eq!(page_window(&[20; 8], 0, 110), 0..4);
    assert_eq!(page_window(&[20; 8], 7, 110), 4..8);
    // From the middle, right first: 3 then 4, 2, 5.
    assert_eq!(page_window(&[20; 8], 3, 110), 2..6);
    // A page wider than the room shows alone.
    assert_eq!(page_window(&[300, 20], 0, 196), 0..1);
    // No pages, or a page past the end: nothing.
    assert_eq!(page_window(&[], 0, 196), 0..0);
    assert_eq!(page_window(&[20], 1, 196), 0..0);
}

/// Pixels in `INK2` inside the box: only the arrows draw in it.
fn ink2(fb: &Fb, xs: core::ops::Range<i32>) -> bool {
    (theme::ROW_Y - 8..=theme::ROW_Y).any(|y| xs.clone().any(|x| fb.at(x, y) == theme::INK2))
}

/// On every page of every chain, `<` shows exactly when pages are cut on
/// the left, `>` on the right; the current page is underlined; a node with
/// no sub-pages draws nothing under the rule.
#[test]
fn the_arrows_show_exactly_where_the_row_is_cut() {
    let chains: [&ChainDef2; 5] = [
        &ALGO_CHAIN,
        &VA_CHAIN,
        &MODAL_PLUCK_CHAIN,
        &TAPE_CHAIN,
        &MIXER_CHANNEL_CHAIN,
    ];
    let t = theme::LABEL_TRACKING;
    let mut cut_both = false;
    for chain in chains {
        for node in 0..chain.len() {
            let block = chain.block_at(node).unwrap();
            let count = block.sub_page_count();
            for sub in 0..count.max(1) {
                let at = chain.page(node, sub).unwrap();
                let mut fb = Fb::new();
                dungeon_map::draw(&mut fb, chain, at, ResonatorMode::String, Pages::Row, 0);
                let ctx = format!("{} {node} {sub}", chain.name);
                assert_eq!(fb.oob, 0, "{ctx}");
                if count == 0 {
                    let below = (theme::ROW_RULE_Y..theme::SCREEN_H)
                        .all(|y| (0..theme::SCREEN_W).all(|x| fb.at(x, y) == Rgb565::BLACK));
                    assert!(below, "{ctx}: no row");
                    continue;
                }
                let w: Vec<i32> = (0..count)
                    .map(|i| {
                        let def = if i == 0 {
                            block.def
                        } else {
                            block.sub_pages[i - 1]
                        };
                        let font = if i == sub {
                            &theme::FONT_LABEL_BOLD
                        } else {
                            &theme::FONT_LABEL
                        };
                        text_width(font, page_label(def, ResonatorMode::String), t)
                    })
                    .collect();
                let r = page_window(&w, sub, theme::ROW_R - theme::ROW_L);
                assert_eq!(ink2(&fb, 6..theme::ROW_L), r.start > 0, "{ctx}: <");
                assert_eq!(ink2(&fb, theme::ROW_R + 1..234), r.end < count, "{ctx}: >");
                cut_both |= r.start > 0 && r.end < count;
                let underline = (theme::ROW_L..theme::ROW_R)
                    .filter(|&x| fb.at(x, theme::ROW_Y + 3) == theme::ACCENT)
                    .count() as i32;
                assert_eq!(underline, w[sub], "{ctx}: underline");
            }
        }
    }
    assert!(cut_both, "some page is cut both ways");
}
