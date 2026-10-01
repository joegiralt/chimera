//! Direction A header and map (UI refresh spec § Shared components 1, 5).

mod screen;

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::part::DacPair::{self, P1};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, ChainDef2};
use chimera_core::ui::block_registry::ALGO_CHAIN;
use chimera_core::ui::block_registry::{DEMO_CHAIN, MIXER_CHANNEL_CHAIN};
use chimera_core::ui::components::{Head, header, header_fits, header_text};
use chimera_core::ui::dungeon_map::{self, node_x};
use chimera_core::ui::nav::{Location, PageAt};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
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
    let h = header_text(at.head, at.def(), ResonatorMode::String, "", P1);
    (h.context.as_str().to_string(), h.name.as_str().to_string())
}

#[test]
fn header_names_context_and_page() {
    let p1 = At::sound(0);
    assert_eq!(texts(p1), ("PART 1 · SOUND".into(), "ALGORITHM".into()));
    assert_eq!(
        texts(p1.at(3, 0)),
        ("PART 1 · SOUND".into(), "FILTER".into())
    );
    let m2 = At::mix(1);
    assert_eq!(texts(m2), ("PART 2 · MIX".into(), "PART".into()));
    assert_eq!(texts(m2.at(1, 0)), ("PART 2 · MIX".into(), "SENDS".into()));
    assert_eq!(texts(m2.at(2, 0)), ("PART 2 · MIX".into(), "CHORUS".into()));
    let tuning = At::new(
        Head::Settings,
        &chimera_core::ui::settings::leaves::TUNING_LEAF,
    );
    assert_eq!(texts(tuning), ("SETTINGS".into(), "TUNING".into()));
}

#[test]
fn header_dot_shows_only_while_sounding_and_stays_in_the_band() {
    for sounding in [false, true] {
        let mut fb = Fb::new();
        header(&mut fb, "PART 1", "WAVE", None, sounding, 0);
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
    header(&mut fb, "PART 1", "WAVE", None, false, 85);
    let alert = (0..28)
        .flat_map(|y| (120..225).map(move |x| (x, y)))
        .any(|(x, y)| fb.at(x, y) == theme::ALERT);
    assert!(alert, "85 % is drawn in the alert colour");
}

/// Every page's header text fits left of the dot, or of the OUT warning
/// (and inside FmtBuf's 32 bytes). Without a warning every name is whole;
/// with one, a long name falls back to its short form (ADR 0057).
#[test]
fn every_header_fits() {
    use chimera_core::dsp::modal::{EXCITER_NAMES, ResonatorMode as M};
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::MODAL_EXC;
    use chimera_core::ui::nav::chain_def_for;
    let models = [M::String, M::Modal, M::Bowed, M::Sympathetic];
    let p6 = PartId::ALL[5];
    let mut places = vec![
        At::new(Head::Sound(p6), chain_def_for(EngineType::Algo)),
        At::new(Head::Sound(p6), chain_def_for(EngineType::Modal)),
        At::mix(5),
    ];
    places.extend(
        chimera_core::ui::block_registry::ALL_CHAINS[3..]
            .iter()
            .map(|&c| At::new(Head::Settings, c)),
    );
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
                for (model, suffix, out) in models.iter().flat_map(|&m| {
                    suffixes.iter().copied().flat_map(move |s| {
                        [DacPair::P1, DacPair::P2, DacPair::P3].map(|o| (m, s, o))
                    })
                }) {
                    let h = header_text(at.head, def, model, suffix, out);
                    let what = format!("{} / {}{:?}", h.context.as_str(), h.name.as_str(), h.warn);
                    assert!(header_fits(&h), "{what}");
                    if h.warn.is_none() {
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
        "",
        DacPair::P3,
    );
    assert_eq!((h.name.as_str(), h.warn), ("SENDS", Some("OUT P3")));
    let alg = At::sound(1);
    let h = header_text(alg.head, alg.def(), ResonatorMode::String, "", DacPair::P2);
    assert_eq!((h.name.as_str(), h.warn), ("ALG", Some("OUT P2")));
}

/// The warning is drawn in WARN, right of the name, and replaces the load.
#[test]
fn the_out_warning_draws_in_warn_and_hides_the_load() {
    let mut fb = Fb::new();
    header(&mut fb, "PART 2 · MIX", "SENDS", Some("OUT P2"), false, 50);
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
    let flt = At::sound(0).at(3, 0); // FLT of OSC ALG DRV FLT FLD MOD
    let mut fb = Fb::new();
    dungeon_map::draw(&mut fb, flt.chain, flt.page(), ResonatorMode::String, 0);
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
    let env = At::sound(0).at(5, 1); // MOD: MOD, ENV, LFO
    let mut fb = Fb::new();
    dungeon_map::draw(&mut fb, env.chain, env.page(), ResonatorMode::String, 0);
    let x = node_x(5, 6) - 8;
    let lit_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x, lit_row), theme::ACCENT, "ENV lit");
    let first_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x - 2, first_row), theme::MID, "MOD ring");
}

#[test]
fn the_map_draws_only_in_its_band_on_every_chain() {
    for place in [
        At::sound(0),
        At::mix(0),
        At::new(Head::Settings, &DEMO_CHAIN),
    ] {
        let chain_id = place.chain.name;
        for node in 0..place.chain.len() {
            let subs = place.at(node, 0).subs();
            for sub in 0..subs.max(1) {
                let at = place.at(node, sub);
                let mut fb = Fb::new();
                dungeon_map::draw(&mut fb, at.chain, at.page(), ResonatorMode::String, 0);
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
/// and so does entering another Part's chain (its button again opens its
/// mixer, ADR 0057).
#[test]
fn a_fresh_algo_part_lands_on_the_algo_page() {
    use chimera_core::params::EngineType;
    use chimera_core::ui::block_registry::ALGO_ALG;
    let mut ui = UiState::new();
    assert_eq!(
        ui.project().part(PartId::ALL[0]).sound.engine(),
        EngineType::Algo
    );
    assert_eq!(ui.page_def().id, ALGO_ALG.id);
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::B2));
    let home = |p: usize| Location::pages(PartId::ALL[p], ALGO_CHAIN.home());
    assert_eq!(ui.location(), home(1));
    assert_eq!(ui.page_def().id, ALGO_ALG.id);
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::B1));
    assert_eq!(ui.location(), home(0));
    assert_eq!(ui.page_def().id, ALGO_ALG.id);
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
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(
        ui.page_def().id,
        block.active_def(block.sub_page_count() - 1).id,
        "EDIT stops at the last"
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
        let h = header_text(Head::Sound(PartId::ALL[0]), &MODAL_EXC, m, "", P1);
        assert_eq!(h.name.as_str(), EXCITER_NAMES[m as usize], "{m:?}");
        assert_eq!(dungeon_map::page_label(&MODAL_EXC, m), "EXC");
    }
    let node = &MODAL_PLUCK_CHAIN.blocks[0];
    assert_eq!(node.def.id, MODAL_EXC.id);
    assert_eq!(node.map.unwrap_or(node.def.short), "EXC");
}
