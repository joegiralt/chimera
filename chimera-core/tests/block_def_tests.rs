use chimera_core::ui::block_registry;
use chimera_core::ui::page::PageLayout;

#[test]
fn filter_block_params() {
    use chimera_core::ui::view::{SlotCtx, view};
    let def = &block_registry::FILTER;
    let ctx = SlotCtx::read(
        &chimera_core::params::ParamSnapshot::default(),
        chimera_core::addr::Op::A,
    );
    assert_eq!(view(def, 1, &ctx).label(), "CUTOFF");
    assert_eq!(view(def, 2, &ctx).label(), "RES");
    assert_eq!(def.layout, PageLayout::BigViz);
}

#[test]
fn chain_active_def_resolves() {
    let chain = &block_registry::ALGO_CHAIN;
    assert_eq!(chain.active_def(0, 0).unwrap().name, "Algorithm");
    assert_eq!(chain.active_def(3, 0).unwrap().name, "Filter");
    assert!(chain.active_def(99, 0).is_none());
}

#[test]
fn modal_pluck_chain() {
    let chain = &block_registry::MODAL_PLUCK_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Exciter");
    assert_eq!(chain.blocks[0].sub_page_count(), 0);
    assert_eq!(chain.blocks[1].def.name, "Modal");
    assert_eq!(chain.blocks[1].sub_page_count(), 3); // primary + Modal-2 + Pitch
    let map: Vec<_> = chain
        .blocks
        .iter()
        .map(|b| b.map.unwrap_or(b.def.short))
        .collect();
    assert_eq!(map, ["EXC", "RES", "FLT", "AMP", "MOD"]);
}

#[test]
fn mixer_channel_strip_chain() {
    let chain = &block_registry::MIXER_CHANNEL_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Part");
    assert_eq!(chain.blocks[1].def.name, "Sends");
    // TAPE before MASTER only with `master-tape` (ADR 0055).
    let len = if cfg!(feature = "master-tape") { 7 } else { 6 };
    assert_eq!(chain.len(), len);
    assert_eq!(chain.blocks[len - 1].def.name, "Master");
}

#[test]
fn demo_chain_has_5_blocks_then_the_glyph_pages() {
    let chain = &block_registry::DEMO_CHAIN;
    assert!(
        chain.blocks[5..]
            .iter()
            .all(|b| b.def.name.starts_with("Glyph: "))
    );
    assert_eq!(chain.blocks[5].def.name, "Glyph: Arc");
    assert_eq!(chain.blocks[0].def.name, "Waves");
    assert_eq!(chain.blocks[2].def.name, "Motion");
    assert_eq!(chain.blocks[3].def.name, "FM Icons");
    assert_eq!(chain.blocks[4].def.name, "Matrix");
    assert_eq!(chain.blocks[4].def.layout, PageLayout::Matrix);
}

#[test]
fn about_has_the_audio_sub_page() {
    let about = &chimera_core::ui::settings::leaves::ABOUT_LEAF.blocks[0];
    assert_eq!(about.def.name, "About");
    assert_eq!(about.sub_pages.len(), 1);
    assert_eq!(about.sub_pages[0].name, "Audio");
    assert_eq!(about.sub_pages[0].id, 41);
}

#[test]
fn algo_chain_is_alg_osc_then_the_voice_chain() {
    let chain = &block_registry::ALGO_CHAIN;
    let labels: Vec<&str> = chain
        .blocks
        .iter()
        .map(|b| b.map.unwrap_or(b.def.short))
        .collect();
    assert_eq!(labels, ["ALG", "OSC", "DRV", "FLT", "AMP", "MOD"]);
    assert_eq!(chain.active_def(1, 0).unwrap().name, "Wave");
    assert!(chain.blocks[1].sub_pages.iter().any(|d| d.name == "Level"));
    assert_eq!(chain.blocks[5].sub_pages.len(), 7);
}

/// Lists and the Sound rung show `NO_PAGE`: no chain's page shares its id,
/// and it binds nothing.
#[test]
fn no_page_id_is_reserved() {
    use chimera_core::ui::block_def::SlotBinding;
    use chimera_core::ui::{NO_PAGE, NO_PAGE_ID};
    for chain in block_registry::ALL_CHAINS {
        for (n, b) in chain.blocks.iter().enumerate() {
            for s in 0..b.sub_page_count().max(1) {
                let def = chain.active_def(n, s).unwrap();
                assert_ne!(def.id, NO_PAGE_ID, "{}", def.name);
            }
        }
    }
    assert_eq!(NO_PAGE.id, NO_PAGE_ID);
    assert!(
        NO_PAGE
            .params
            .iter()
            .all(|p| p.binding == SlotBinding::Empty)
    );
}
