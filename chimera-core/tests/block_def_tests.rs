use chimera_core::ui::block_def::*;
use chimera_core::ui::block_registry;
use chimera_core::ui::page::PageLayout;

#[test]
fn pizza_poly_chain_has_5_blocks() {
    let chain = &block_registry::PIZZA_POLY_CHAIN;
    assert_eq!(chain.len(), 5);
    assert_eq!(chain.blocks[0].def.name, "Pizza");
    assert_eq!(chain.blocks[2].def.name, "Filter");
    assert_eq!(chain.blocks[4].def.name, "Mod Matrix");
    // Mod matrix has 2 sub-pages: Envelope + LFO
    assert_eq!(chain.blocks[4].sub_pages.len(), 2);
    assert_eq!(chain.blocks[4].sub_pages[0].name, "Envelope");
    assert_eq!(chain.blocks[4].sub_pages[1].name, "LFO");
}

#[test]
fn filter_block_params() {
    let def = &block_registry::FILTER;
    assert_eq!(def.params[0].label, "CUTOFF");
    assert_eq!(def.params[1].label, "RESO");
    assert_eq!(def.layout, PageLayout::BigViz);
}

#[test]
fn kick_chain_has_3_blocks() {
    let chain = &block_registry::KICK_CHAIN;
    assert_eq!(chain.len(), 3);
    assert_eq!(chain.blocks[0].def.name, "Noise");
}

#[test]
fn chain_active_def_resolves() {
    let chain = &block_registry::PIZZA_POLY_CHAIN;
    assert_eq!(chain.active_def(0, 0).unwrap().name, "Pizza");
    assert_eq!(chain.active_def(2, 0).unwrap().name, "Filter");
    assert!(chain.active_def(99, 0).is_none());
}

#[test]
fn modal_pluck_chain() {
    let chain = &block_registry::MODAL_PLUCK_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Modal");
    assert_eq!(chain.blocks[0].sub_page_count(), 2); // primary + Modal-2
}

#[test]
fn mix_chain_has_5_blocks() {
    let chain = &block_registry::MIX_CHAIN;
    assert_eq!(chain.len(), 5);
    assert_eq!(chain.blocks[0].def.name, "Mixer");
    assert_eq!(chain.blocks[4].def.name, "Master");
}

#[test]
fn mixer_channel_strip_chain() {
    let chain = &block_registry::MIXER_CHANNEL_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Channel");
    assert_eq!(chain.blocks[1].def.name, "MIDI");
    assert_eq!(chain.blocks[2].def.name, "EQ");
    assert_eq!(chain.blocks[3].def.name, "Sends");
    assert_eq!(chain.len(), 4);
}

#[test]
fn system_chain_has_5_blocks() {
    let chain = &block_registry::SYSTEM_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "MIDI Setup");
    assert_eq!(chain.blocks[4].def.name, "About");
    assert_eq!(chain.len(), 5);
}

#[test]
fn fm_chain_has_5_blocks() {
    let chain = &block_registry::FM_CHAIN;
    assert_eq!(chain.len(), 5);
    assert_eq!(chain.blocks[0].def.name, "4opFM");
    assert_eq!(chain.blocks[1].def.name, "Drive");
    assert_eq!(chain.blocks[2].def.name, "Filter");
    assert_eq!(chain.blocks[3].def.name, "Folder");
    assert_eq!(chain.blocks[4].def.name, "Mod Matrix");
    // FM engine block has 2 sub-pages: Operator + Ratios
    assert_eq!(chain.blocks[0].sub_pages.len(), 2);
    assert_eq!(chain.blocks[0].sub_pages[0].name, "Operator");
    assert_eq!(chain.blocks[0].sub_pages[1].name, "Ratios");
    assert_eq!(chain.blocks[0].sub_page_count(), 3); // primary + 2 subs
}

#[test]
fn fm_chain_resolves_sub_pages() {
    let chain = &block_registry::FM_CHAIN;
    assert_eq!(chain.active_def(0, 0).unwrap().name, "4opFM");
    assert_eq!(chain.active_def(0, 1).unwrap().name, "Operator");
    assert_eq!(chain.active_def(0, 2).unwrap().name, "Ratios");
}

#[test]
fn demo_chain_has_4_blocks() {
    let chain = &block_registry::DEMO_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Waves");
    assert_eq!(chain.blocks[2].def.name, "Motion");
    assert_eq!(chain.blocks[3].def.name, "Matrix");
    assert_eq!(chain.blocks[3].def.layout, PageLayout::Matrix);
    assert_eq!(chain.len(), 4);
}
