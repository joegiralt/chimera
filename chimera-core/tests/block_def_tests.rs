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
fn kick_chain_has_3_blocks() {
    let chain = &block_registry::KICK_CHAIN;
    assert_eq!(chain.len(), 3);
    assert_eq!(chain.blocks[0].def.name, "Noise");
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
    assert_eq!(chain.blocks[0].def.name, "Modal");
    assert_eq!(chain.blocks[0].sub_page_count(), 2); // primary + Modal-2
}

/// FX diet spec § UI: the Mix map reads MIX · CHR · DLY · REV · TAPE · MST;
/// EFX keeps its id and name, only its short label is REV.
#[test]
fn mix_chain_is_mix_chr_dly_rev_tape_mst() {
    let chain = &block_registry::MIX_CHAIN;
    let shorts: Vec<&str> = chain.blocks.iter().map(|b| b.def.short).collect();
    assert_eq!(shorts, ["MIX", "CHR", "DLY", "REV", "TAPE", "MST"]);
    assert_eq!(chain.blocks[0].def.name, "Mixer");
    assert_eq!(
        (chain.blocks[3].def.id, chain.blocks[3].def.name),
        (16, "Reverb")
    );
    assert_eq!(chain.blocks[5].def.name, "Master");
}

#[test]
fn mixer_channel_strip_chain() {
    let chain = &block_registry::MIXER_CHANNEL_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Part");
    assert_eq!(chain.blocks[1].def.name, "Sends");
    assert_eq!(chain.len(), 7);
    assert_eq!(chain.blocks[6].def.name, "Master");
}

#[test]
fn system_chain_has_5_blocks() {
    let chain = &block_registry::SYSTEM_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "MIDI Setup");
    assert_eq!(chain.blocks[4].def.name, "About");
    assert_eq!(chain.len(), 5);
}

#[test]
fn demo_chain_has_5_blocks() {
    let chain = &block_registry::DEMO_CHAIN;
    assert_eq!(chain.blocks.len(), 5);
    assert_eq!(chain.blocks[0].def.name, "Waves");
    assert_eq!(chain.blocks[2].def.name, "Motion");
    assert_eq!(chain.blocks[3].def.name, "FM Icons");
    assert_eq!(chain.blocks[4].def.name, "Matrix");
    assert_eq!(chain.blocks[4].def.layout, PageLayout::Matrix);
    assert_eq!(chain.len(), 5);
}

#[test]
fn about_has_the_audio_sub_page() {
    let about = &block_registry::SYSTEM_CHAIN.blocks[4];
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
