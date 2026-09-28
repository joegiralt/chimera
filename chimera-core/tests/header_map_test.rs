//! Direction A header and map (UI refresh spec § Shared components 1, 5).

mod screen;

use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::ALGO_CHAIN;
use chimera_core::ui::chain::{ChainId, ChainNav};
use chimera_core::ui::components::{header, header_text};
use chimera_core::ui::dungeon_map::{self, node_x};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_hal::ButtonId;
use screen::{Fb, Input, feed, osc_node, scope_fixture, settle, to_osc};

fn texts(nav: &ChainNav) -> (String, String) {
    let (c, n) = header_text(nav, nav.active_block_def());
    (c.as_str().to_string(), n.as_str().to_string())
}

#[test]
fn header_names_context_and_page() {
    let mut nav = ChainNav::new();
    assert_eq!(texts(&nav), ("PART 1".into(), "ALGORITHM".into()));
    nav.node = 3;
    assert_eq!(texts(&nav), ("PART 1".into(), "FILTER".into()));
    nav.chain_id = ChainId::Mixer(1);
    nav.node = 0;
    assert_eq!(texts(&nav), ("MIXER".into(), "PART 2".into()));
    nav.node = 1;
    assert_eq!(texts(&nav), ("MIXER".into(), "SENDS 2".into()));
    nav.node = 2;
    assert_eq!(
        texts(&nav),
        ("MIXER".into(), "CHORUS".into()),
        "shared FX are not numbered"
    );
    nav.chain_id = ChainId::System;
    nav.node = 0;
    assert_eq!(texts(&nav), ("SYSTEM".into(), "MIDI SETUP".into()));
}

#[test]
fn header_dot_shows_only_while_sounding_and_stays_in_the_band() {
    for sounding in [false, true] {
        let mut fb = Fb::new();
        header(&mut fb, "PART 1", "WAVE", sounding, 0);
        assert_eq!(
            fb.at(theme::HEADER_DOT_X, theme::HEADER_DOT_Y) == theme::ACCENT,
            sounding
        );
        assert!(
            fb.px[theme::HEADER_BOTTOM as usize * 240..]
                .iter()
                .all(|&p| p == 0),
            "nothing below y 28"
        );
        assert_eq!(fb.oob, 0);
    }
}

#[test]
fn header_shows_audio_load_in_warning_colours() {
    let mut fb = Fb::new();
    header(&mut fb, "PART 1", "WAVE", false, 85);
    let alert = (0..28)
        .flat_map(|y| (120..225).map(move |x| (x, y)))
        .any(|(x, y)| fb.at(x, y) == theme::ALERT);
    assert!(alert, "85 % is drawn in the alert colour");
}

/// Every page's header text fits left of the load readout and the dot (and
/// inside FmtBuf's 32 bytes).
#[test]
fn every_header_fits() {
    use chimera_core::ui::draw::text_width;
    for chain_id in [
        ChainId::Part(5),
        ChainId::Mixer(5),
        ChainId::System,
        ChainId::Demo,
    ] {
        let mut nav = ChainNav::new();
        nav.chain_id = chain_id;
        for node in 0..nav.active_chain().len() {
            nav.node = node;
            let subs = nav
                .active_chain_block()
                .map_or(0, |b| b.sub_page_count())
                .max(1);
            for sub in 0..subs {
                nav.sub_page = sub;
                let def = nav.active_block_def();
                let (c, n) = header_text(&nav, def);
                assert_eq!(
                    n.as_str().len(),
                    def.name.len() + if n.as_str().ends_with(" 6") { 2 } else { 0 },
                    "{}",
                    def.name
                );
                let w = theme::MARGIN_X
                    + text_width(&theme::FONT_LABEL, c.as_str(), 1)
                    + 7
                    + text_width(&theme::FONT_LABEL_BOLD, n.as_str(), 1);
                assert!(
                    w < theme::HEADER_DOT_X - 40,
                    "{} / {}: {w}",
                    c.as_str(),
                    n.as_str()
                );
            }
        }
    }
}

#[test]
fn nodes_spread_over_the_line_and_a_single_node_is_centred() {
    assert_eq!(node_x(0, 5), theme::MAP_X0);
    assert_eq!(node_x(4, 5), theme::MAP_X1);
    assert_eq!(node_x(2, 5), 120);
    assert_eq!(node_x(0, 1), 120, "no division by zero");
}

#[test]
fn current_block_is_an_accent_pill_others_are_rings() {
    let mut nav = ChainNav::new();
    nav.node = 3; // FLT of OSC ALG DRV FLT FLD MOD
    let mut fb = Fb::new();
    dungeon_map::draw(&mut fb, &nav, 0);
    let (pill, other) = (node_x(3, 6), node_x(0, 6));
    assert_eq!(
        fb.at(pill - 14, theme::MAP_LINE_Y),
        theme::ACCENT,
        "pill body"
    );
    assert_eq!(
        fb.at(other + theme::NODE_R, theme::MAP_LINE_Y),
        theme::MID,
        "ring edge"
    );
    assert_eq!(fb.at(other, theme::MAP_LINE_Y), theme::BG, "ring is hollow");
    let label = (theme::NODE_LABEL_Y - 7..=theme::NODE_LABEL_Y)
        .any(|y| (other - 8..other + 8).any(|x| fb.at(x, y) == theme::MID));
    assert!(label, "grey label under a ring");
}

#[test]
fn sub_pages_hang_under_the_pill_with_the_current_one_lit() {
    let mut nav = ChainNav::new();
    nav.node = 5; // MOD: MOD, ENV, LFO
    nav.sub_page = 1;
    let mut fb = Fb::new();
    dungeon_map::draw(&mut fb, &nav, 0);
    let x = node_x(5, 6) - 8;
    let lit_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x, lit_row), theme::ACCENT, "ENV lit");
    let first_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x - 2, first_row), theme::MID, "MOD ring");
}

#[test]
fn the_map_draws_only_in_its_band_on_every_chain() {
    for chain_id in [
        ChainId::Part(0),
        ChainId::Mixer(0),
        ChainId::System,
        ChainId::Demo,
    ] {
        let n = {
            let mut nav = ChainNav::new();
            nav.chain_id = chain_id;
            nav.active_chain().len()
        };
        for node in 0..n {
            let mut nav = ChainNav::new();
            nav.chain_id = chain_id;
            nav.node = node;
            let subs = nav.active_chain_block().map_or(0, |b| b.sub_page_count());
            for sub in 0..subs.max(1) {
                nav.sub_page = sub;
                let mut fb = Fb::new();
                dungeon_map::draw(&mut fb, &nav, 0);
                assert!(
                    fb.px[..theme::MAP_TOP as usize * 240]
                        .iter()
                        .all(|&p| p == 0),
                    "{chain_id:?} {node} {sub}"
                );
                assert_eq!(fb.oob, 0, "{chain_id:?} {node} {sub}");
            }
        }
    }
}

/// Spec addendum: ALGO is the Algo engine's home. A fresh Part lands on it,
/// and so does entering another Part's chain or pressing its button again.
#[test]
fn a_fresh_algo_part_lands_on_the_algo_page() {
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::ALGO_ALG;
    let mut ui = UiState::new();
    assert_eq!(ui.nav.engine, EngineType::Algo);
    assert_eq!(ui.nav.active_block_def().id, ALGO_ALG.id);
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::B2));
    assert_eq!(ui.nav.chain_id, ChainId::Part(1));
    assert_eq!(ui.nav.active_block_def().id, ALGO_ALG.id);
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::B2));
    assert_eq!(ui.nav.active_block_def().id, ALGO_ALG.id);
}

/// Spec § UI: whichever node is current, the Algo chain's map pill and
/// labels never overlap, and no sub-page label runs into the next node's.
#[test]
fn the_algo_map_has_no_overlapping_nodes() {
    use chimera_core::ui::draw::text_width;
    let n = ALGO_CHAIN.len();
    let half = |label: &str| text_width(&theme::FONT_LABEL, label, 0) / 2;
    for cur in 0..n {
        let ext: Vec<(i32, i32)> = (0..n)
            .map(|i| {
                let x = node_x(i, n);
                if i == cur {
                    (x - theme::PILL_W / 2, x + theme::PILL_W / 2)
                } else {
                    let b = &ALGO_CHAIN.blocks[i];
                    let h = half(b.map.unwrap_or(b.def.short));
                    (x - h, x + h)
                }
            })
            .collect();
        for w in ext.windows(2) {
            assert!(w[0].1 < w[1].0, "node {cur} current: {ext:?}");
        }
        if cur + 1 < n {
            let block = &ALGO_CHAIN.blocks[cur];
            let x = node_x(cur, n) - 8 + 6;
            for def in core::iter::once(block.def).chain(block.sub_pages.iter().copied()) {
                let end = x + text_width(&theme::FONT_LABEL, def.short, 0);
                assert!(
                    end < ext[cur + 1].0,
                    "{} runs into node {}",
                    def.short,
                    cur + 1
                );
            }
        }
    }
}

/// Spec § UI: EDIT reaches every OSC sub-page, and the map scrolls so the
/// current one is always drawn lit.
#[test]
fn every_osc_sub_page_is_reachable_and_lit_on_the_map() {
    let mut ui = UiState::new();
    to_osc(&mut ui);
    let block = &ALGO_CHAIN.blocks[osc_node()];
    // The dot's centre column; its fill draws over the trunk beneath it.
    let x = node_x(osc_node(), ALGO_CHAIN.len()) - 8;
    for sub in 0..block.sub_page_count() {
        if sub > 0 {
            feed(&mut ui, Input::press(ButtonId::Edit));
        }
        assert_eq!(ui.nav.active_block_def().id, block.active_def(sub).id);
        settle(&mut ui);
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        let cy = if sub == 0 {
            theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT / 2
        } else {
            theme::SCREEN_H - theme::BRANCH_LINE_HEIGHT / 2
        };
        assert_eq!(
            fb.at(x, cy),
            theme::ACCENT,
            "sub-page {sub} ({})",
            block.active_def(sub).short
        );
        assert_eq!(fb.oob, 0);
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(
        ui.nav.sub_page,
        block.sub_page_count() - 1,
        "EDIT stops at the last"
    );
}
