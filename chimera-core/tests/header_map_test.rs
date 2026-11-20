//! Direction A header and map (UI refresh spec § Shared components 1, 5).

mod screen;

use chimera_core::bank2::NEEDS_UPDATE_CELL;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::part::DacPair::{self, P1};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, ChainDef2};
use chimera_core::ui::block_registry::ALGO_CHAIN;
use chimera_core::ui::block_registry::MIXER_CHANNEL_CHAIN;
use chimera_core::ui::components::{Head, HeaderStatus, header, header_fits, header_text};
use chimera_core::ui::dungeon_map::{self, node_x};
use chimera_core::ui::nav::{Location, PageAt};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::Pages;
use chimera_hal::ButtonId;
use screen::{Fb, Input, feed, osc_node, scope_fixture, settle, to_osc};

/// A page: whose, on which chain, where.
#[derive(Clone, Copy)]
struct At {
    head: Head,
    chain: &'static ChainDef2,
    node: usize,
    sub: usize,
}

impl At {
    fn new(head: Head, chain: &'static ChainDef2) -> Self {
        At {
            head,
            chain,
            node: 0,
            sub: 0,
        }
    }

    fn sound(n: usize) -> Self {
        Self::new(Head::Sound(PartId::ALL[n]), &ALGO_CHAIN)
    }

    fn mix(n: usize) -> Self {
        Self::new(Head::Mix(PartId::ALL[n]), &MIXER_CHANNEL_CHAIN)
    }

    fn at(self, node: usize, sub: usize) -> Self {
        At { node, sub, ..self }
    }

    fn def(&self) -> &'static BlockDef {
        self.chain.active_def(self.node, self.sub).unwrap()
    }

    fn subs(&self) -> usize {
        self.chain
            .block_at(self.node)
            .map_or(0, |b| b.sub_page_count())
    }

    fn page(&self) -> PageAt {
        self.chain.page(self.node, self.sub).unwrap()
    }
}

fn texts(at: At) -> (String, String) {
    let h = header_text(
        at.head,
        at.def(),
        ResonatorMode::String,
        FilterKind::Svf,
        "",
        P1,
        None,
    );
    (h.context.as_str().to_string(), h.name.as_str().to_string())
}

#[test]
fn header_names_context_and_page() {
    let p1 = At::sound(0);
    assert_eq!(texts(p1), ("PART 1 · SOUND".into(), "ALGORITHM".into()));
    assert_eq!(
        texts(p1.at(2, 0)),
        ("PART 1 · SOUND".into(), "FILTER".into())
    );
    let m2 = At::mix(1);
    assert_eq!(texts(m2), ("PART 2 · MIX".into(), "PART".into()));
    assert_eq!(texts(m2.at(1, 0)), ("PART 2 · MIX".into(), "SENDS".into()));
    assert_eq!(texts(m2.at(2, 0)), ("PART 2 · MIX".into(), "CHORUS".into()));
    let tuning = At::new(
        Head::Settings,
        chimera_core::ui::settings::leaves::TUNING_LEAF.chain(),
    );
    assert_eq!(texts(tuning), ("SETTINGS".into(), "TUNING".into()));
}

#[test]
fn header_dot_shows_only_while_sounding_and_stays_in_the_band() {
    for sounding in [false, true] {
        let mut fb = Fb::new();
        header(
            &mut fb,
            "PART 1",
            "WAVE",
            None,
            HeaderStatus {
                sounding,
                ..HeaderStatus::default()
            },
        );
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
    header(
        &mut fb,
        "PART 1",
        "WAVE",
        None,
        HeaderStatus {
            load_pct: 85,
            ..HeaderStatus::default()
        },
    );
    let alert = (0..28)
        .flat_map(|y| (120..225).map(move |x| (x, y)))
        .any(|(x, y)| fb.at(x, y) == theme::ALERT);
    assert!(alert, "85 % is drawn in the alert colour");
}

/// Every page's header text fits left of ORBIT's beat dot, or of the OUT warning
/// (and inside FmtBuf's 32 bytes). Without a warning every name is whole;
/// with one, a long name falls back to its short form (ADR 0057).
#[test]
fn every_header_fits() {
    use chimera_core::dsp::modal::{EXCITER_NAMES, ResonatorMode as M};
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::{MODAL_EXC, TAPE_MODEL};
    use chimera_core::ui::nav::chain_def_for;
    let models = [M::String, M::Modal, M::Bowed, M::Sympathetic];
    let p6 = PartId::ALL[5];
    // SETTINGS' pages draw a breadcrumb, not this header
    // (`region::settings_regions`); ORBIT's every page reads `SCENE n`.
    let scene = chimera_core::orbit::scene::SceneIdx::ALL[7];
    let places = [
        At::new(Head::Sound(p6), chain_def_for(EngineType::Algo)),
        At::new(Head::Sound(p6), chain_def_for(EngineType::Modal)),
        At::new(Head::Sound(p6), chain_def_for(EngineType::Tape)),
        At::new(Head::Sound(p6), chain_def_for(EngineType::Va)),
        At::mix(5),
        At::new(Head::Orbit(scene), &chimera_core::ui::orbit::ORBIT_CHAIN),
    ];
    for place in places {
        for node in 0..place.chain.len() {
            let subs = place.at(node, 0).subs().max(1);
            for sub in 0..subs {
                let at = place.at(node, sub);
                let def = at.def();
                // Only the ENV pages take a TYPE suffix.
                let suffixes: &[&str] = if def.name.starts_with("Env") {
                    &["", " / A", " / B"]
                } else {
                    &[""]
                };
                for ((model, suffix, out), closed) in models
                    .iter()
                    .flat_map(|&m| {
                        suffixes.iter().copied().flat_map(move |s| {
                            [DacPair::P1, DacPair::P2, DacPair::P3].map(|o| (m, s, o))
                        })
                    })
                    .flat_map(|x| [None, Some(NEEDS_UPDATE_CELL)].map(|c| (x, c)))
                {
                    let h = header_text(at.head, def, model, FilterKind::Svf, suffix, out, closed);
                    let what = format!("{} / {}{:?}", h.context.as_str(), h.name.as_str(), h.warn);
                    assert!(header_fits(&h), "{what}");
                    // TAPE's model page is named after its machine (tape_pages_test).
                    if h.warn.is_none() && def.id != TAPE_MODEL.id {
                        let full = if def.id == MODAL_EXC.id {
                            EXCITER_NAMES[model as usize].len()
                        } else {
                            def.name.len()
                        };
                        assert_eq!(h.name.as_str().len(), full + suffix.len(), "{what}");
                    }
                }
            }
        }
    }
}

/// The OUT warning's page names: whole where they fit, short where not.
#[test]
fn the_out_warning_keeps_short_names_whole() {
    let sends = At::mix(1).at(1, 0);
    let h = header_text(
        sends.head,
        sends.def(),
        ResonatorMode::String,
        FilterKind::Svf,
        "",
        DacPair::P3,
        None,
    );
    assert_eq!((h.name.as_str(), h.warn), ("SENDS", Some("OUT P3")));
    let alg = At::sound(1);
    let h = header_text(
        alg.head,
        alg.def(),
        ResonatorMode::String,
        FilterKind::Svf,
        "",
        DacPair::P2,
        None,
    );
    assert_eq!((h.name.as_str(), h.warn), ("ALG", Some("OUT P2")));
}

/// The warning is drawn in WARN, right of the name, and replaces the load.
#[test]
fn the_out_warning_draws_in_warn_and_hides_the_load() {
    let mut fb = Fb::new();
    header(
        &mut fb,
        "PART 2 · MIX",
        "SENDS",
        Some("OUT P2"),
        HeaderStatus {
            load_pct: 50,
            ..HeaderStatus::default()
        },
    );
    let warn = |fb: &Fb| {
        (0..28)
            .flat_map(|y| (160..222).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.at(x, y) == theme::WARN)
            .count()
    };
    assert!(warn(&fb) > 20, "OUT P2 in WARN");
    assert!(
        (0..28).all(|y| (100..160).all(|x| fb.at(x, y) != theme::MID)),
        "no CPU readout"
    );
    assert_eq!(fb.oob, 0);
}

/// `·` is drawn as a dot, and measured as drawn.
#[test]
fn the_middle_dot_draws_and_measures() {
    use chimera_core::ui::draw::{text_tracked, text_width};
    let mut fb = Fb::new();
    let s = "PART 2 · MIX";
    let adv = text_tracked(&mut fb, &theme::FONT_LABEL, s, 12, 20, theme::MID, 1);
    assert_eq!(adv, text_width(&theme::FONT_LABEL, s, 1));
    let x = 12 + text_width(&theme::FONT_LABEL, "PART 2 ", 1);
    assert_eq!(fb.at(x, 16), theme::MID, "the dot");
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
    let flt = At::sound(0).at(2, 0); // FLT of ALG OSC FLT AMP MOD
    let mut fb = Fb::new();
    dungeon_map::draw(
        &mut fb,
        flt.chain,
        flt.page(),
        ResonatorMode::String,
        Pages::Stack,
        0,
    );
    let (pill, other) = (node_x(2, 5), node_x(0, 5));
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
    let env = At::sound(0).at(4, 1); // MOD: MOD, ENV, LFO
    let mut fb = Fb::new();
    dungeon_map::draw(
        &mut fb,
        env.chain,
        env.page(),
        ResonatorMode::String,
        Pages::Stack,
        0,
    );
    let x = node_x(4, 5) - 8;
    let lit_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x, lit_row), theme::ACCENT, "ENV lit");
    let first_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x - 2, first_row), theme::MID, "MOD ring");
}

#[test]
fn the_map_draws_only_in_its_band_on_every_chain() {
    for place in [At::sound(0), At::mix(0)] {
        let chain_id = place.chain.name;
        for node in 0..place.chain.len() {
            let subs = place.at(node, 0).subs();
            for (sub, pages) in (0..subs.max(1)).flat_map(|s| Pages::ALL.map(|p| (s, p))) {
                let at = place.at(node, sub);
                let mut fb = Fb::new();
                dungeon_map::draw(
                    &mut fb,
                    at.chain,
                    at.page(),
                    ResonatorMode::String,
                    pages,
                    0,
                );
                assert!(
                    fb.px[..theme::MAP_TOP as usize * 240]
                        .iter()
                        .all(|&p| p == 0),
                    "{chain_id:?} {node} {sub} {pages:?}"
                );
                assert_eq!(fb.oob, 0, "{chain_id:?} {node} {sub} {pages:?}");
            }
        }
    }
}

/// Spec addendum: ALGO is the Algo engine's home: a fresh Part lands on
/// it. Another Part's button from a sound page keeps the page (#957, ADR
/// 0301); its button again opens its mixer (ADR 0057).
#[test]
fn a_fresh_algo_part_lands_on_the_algo_page() {
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::ALGO_ALG;
    use chimera_core::ui::nav::PageAt;
    let mut ui = UiState::new();
    assert_eq!(
        ui.project().part(PartId::ALL[0]).sound.engine(),
        EngineType::Algo
    );
    assert_eq!(ui.page_def().id, ALGO_ALG.id);
    assert_eq!(
        ui.location(),
        Location::pages(PartId::ALL[0], ALGO_CHAIN.home())
    );
    feed(&mut ui, Input::press(ButtonId::Plus));
    let next = |p: usize| Location::pages(PartId::ALL[p], PageAt::of(1, 0));
    assert_eq!(ui.location(), next(0));
    feed(&mut ui, Input::press(ButtonId::B2));
    assert_eq!(ui.location(), next(1), "the page kept");
    feed(&mut ui, Input::press(ButtonId::B1));
    assert_eq!(ui.location(), next(0));
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

/// STACK's node line on every chain: the 34 px pill and the untracked
/// labels beside it never touch, whichever is current. Extents are
/// half-open, so `end == start` is touching.
#[test]
fn the_stack_nodes_never_overlap() {
    let w = |l: &str| chimera_core::ui::draw::text_width(&theme::FONT_LABEL, l, 0);
    for chain in chimera_core::ui::block_registry::ALL_CHAINS.iter() {
        let n = chain.blocks.len();
        for cur in 0..n {
            let ext: Vec<(i32, i32)> = (0..n)
                .map(|i| {
                    let b = &chain.blocks[i];
                    let x = node_x(i, n);
                    let w = if i == cur {
                        theme::PILL_W
                    } else {
                        w(b.map.unwrap_or(b.def.short))
                    };
                    (x - w / 2, x - w / 2 + w)
                })
                .collect();
            for p in ext.windows(2) {
                assert!(
                    p[0].1 < p[1].0,
                    "{} node {cur} current: {ext:?}",
                    chain.name
                );
            }
        }
    }
}

/// ROW's node line: the pill (at least 34, its bold label + 10) and the
/// tracked labels beside it never touch, on any chain, whichever is current.
/// Extents are half-open, so `end == start` is touching (#646).
#[test]
fn the_row_nodes_never_overlap() {
    let t = theme::LABEL_TRACKING;
    for chain in chimera_core::ui::block_registry::ALL_CHAINS.iter() {
        let n = chain.blocks.len();
        let label = |i: usize| chain.blocks[i].map.unwrap_or(chain.blocks[i].def.short);
        for cur in 0..n {
            let ext: Vec<(i32, i32)> = (0..n)
                .map(|i| {
                    let x = node_x(i, n);
                    let w = if i == cur {
                        theme::PILL_W.max(
                            chimera_core::ui::draw::text_width(
                                &theme::FONT_LABEL_BOLD,
                                label(i),
                                t,
                            ) + theme::ROW_PILL_PAD,
                        )
                    } else {
                        chimera_core::ui::draw::text_width(&theme::FONT_LABEL, label(i), t)
                    };
                    (x - w / 2, x - w / 2 + w)
                })
                .collect();
            for w in ext.windows(2) {
                assert!(
                    w[0].1 < w[1].0,
                    "{} node {cur} current: {ext:?}",
                    chain.name
                );
            }
        }
    }
}

/// Spec § UI: EDIT reaches every OSC sub-page, and the map scrolls so the
/// current one is always drawn lit (STACK; ROW underlines it).
#[test]
fn every_osc_sub_page_is_reachable_and_lit_on_the_map() {
    let mut ui = UiState::new();
    screen::stack_map(&mut ui);
    to_osc(&mut ui);
    let block = &ALGO_CHAIN.blocks[osc_node()];
    // The dot's centre column; its fill draws over the trunk beneath it.
    let x = node_x(osc_node(), ALGO_CHAIN.len()) - 8;
    for sub in 0..block.sub_page_count() {
        if sub > 0 {
            feed(&mut ui, Input::press(ButtonId::Edit));
        }
        assert_eq!(ui.page_def().id, block.active_def(sub).id);
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
        screen::set_pages(&mut ui, Pages::Row);
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        let lit = (theme::ROW_L..theme::ROW_R).any(|x| fb.at(x, theme::ROW_Y + 3) == theme::ACCENT);
        assert!(lit, "ROW underlines sub-page {sub}");
        screen::stack_map(&mut ui);
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(
        ui.page_def().id,
        block.active_def(0).id,
        "EDIT wraps from the last to the first (ADR 0081)"
    );
}

/// Modal's home reads RES; its model page is named after MODEL (the
/// approved mockups: RES · STRING · PIT).
#[test]
fn the_model_page_is_named_after_the_model() {
    use chimera_core::dsp::modal::{MODEL_NAMES, ResonatorMode as M};
    use chimera_core::ui::block_registry::{MODAL_1, MODAL_2, PITCH};
    for m in [M::String, M::Modal, M::Bowed, M::Sympathetic] {
        let label = |def| dungeon_map::page_label(def, m);
        assert_eq!(label(&MODAL_1), "RES");
        assert_eq!(label(&MODAL_2), MODEL_NAMES[m as usize]);
        assert_eq!(label(&PITCH), "PIT");
    }
}

/// A MODEL change redraws the map's sub-row through the dirty render.
#[test]
fn a_model_change_redraws_the_map() {
    use chimera_core::params::EngineType;
    use chimera_hal::EncoderId;
    let mut ui = UiState::new();
    screen::load_init(&mut ui, EngineType::Modal);
    feed(&mut ui, Input::press(ButtonId::Plus)); // EXC → RES
    settle(&mut ui);
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    feed(&mut ui, Input::turn(EncoderId::A, 3)); // MODEL → SYMP
    settle(&mut ui);
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    let mut full = Fb::new();
    ui.render_with_audio(&mut full, &perf, None, &scope);
    let band = theme::MAP_TOP as usize * 240..;
    assert!(fb.px[band.clone()] == full.px[band], "stale map");
}

/// EXC's header is named after the model's exciter; its map node reads EXC.
#[test]
fn the_exciter_page_is_named_after_the_exciter() {
    use chimera_core::dsp::modal::{EXCITER_NAMES, ResonatorMode as M};
    use chimera_core::ui::block_registry::{MODAL_EXC, MODAL_PLUCK_CHAIN};
    for m in [M::String, M::Modal, M::Bowed, M::Sympathetic] {
        let h = header_text(
            Head::Sound(PartId::ALL[0]),
            &MODAL_EXC,
            m,
            FilterKind::Svf,
            "",
            P1,
            None,
        );
        assert_eq!(h.name.as_str(), EXCITER_NAMES[m as usize], "{m:?}");
        assert_eq!(dungeon_map::page_label(&MODAL_EXC, m), "EXC");
    }
    let node = &MODAL_PLUCK_CHAIN.blocks[0];
    assert_eq!(node.def.id, MODAL_EXC.id);
    assert_eq!(node.map.unwrap_or(node.def.short), "EXC");
}

/// FLT › MOD on DIRTY speaks CLEAN's cells, so its header names DIRTY;
/// CLEAN's and the 2040's read FILTER MOD.
#[test]
fn dirty_filter_mod_names_dirty() {
    use chimera_core::ui::block_registry::FILTER_MOD;
    let name = |k| {
        let h = header_text(
            Head::Sound(PartId::ALL[0]),
            &FILTER_MOD,
            ResonatorMode::String,
            k,
            "",
            P1,
            None,
        );
        String::from(h.name.as_str())
    };
    assert_eq!(name(FilterKind::Dirty), "DIRTY MOD");
    assert_eq!(name(FilterKind::Svf), "FILTER MOD");
    assert_eq!(name(FilterKind::K2040), "FILTER MOD");
}
