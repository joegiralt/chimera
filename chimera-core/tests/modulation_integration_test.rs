use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::{ModState, MAX_MOD_SOURCES};
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::mod_grid::MatrixState;

use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

fn rms(buf: &[f32]) -> f32 {
    let sum: f32 = buf.iter().map(|s| s * s).sum();
    (sum / buf.len() as f32).sqrt()
}

#[test]
fn voice_render_with_empty_mod_state() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new();
    let params = ParamSnapshot::default();

    voice.note_on(60, 100, &params, SAMPLE_RATE);

    let mut output = [0.0f32; BLOCK_SIZE];
    voice.render(&mut output, &params, &empty_mod, SAMPLE_RATE);

    // Should produce sound normally
    let level = rms(&output);
    assert!(level > 0.001, "voice with empty mod state should still produce sound, rms={}", level);
}

#[test]
fn voice_render_with_mod_offset_changes_filter() {
    // Render two voices identically, except one has an LFO modulating filter cutoff.
    let mut voice_dry = Voice::new();
    let mut voice_mod = Voice::new();
    let mut params = ParamSnapshot::default();
    params.filter.cutoff.set(2000.0);
    params.filter.mode = 2; // LP4

    // Set up LFO: fast rate so it clearly modulates within a few blocks
    params.lfo.rate = 10.0;
    params.lfo.depth = 1.0;
    params.lfo.shape = 0; // sine

    // Dry: no modulation
    let empty_mod = ModState::new();

    // Modulated: LFO (source 1) -> filter cutoff (block 2, param 0)
    let mut mod_state = ModState::new();
    mod_state.num_sources = 2; // source 0 = env, source 1 = LFO
    mod_state.num_dests = 1;
    mod_state.dests[0] = (2, 0); // filter cutoff
    mod_state.amounts[1][0] = 100; // LFO -> cutoff at high amount

    voice_dry.note_on(60, 100, &params, SAMPLE_RATE);
    voice_mod.note_on(60, 100, &params, SAMPLE_RATE);

    let mut out_dry = [0.0f32; BLOCK_SIZE];
    let mut out_mod = [0.0f32; BLOCK_SIZE];

    // Render several blocks to let modulation develop
    for _ in 0..16 {
        voice_dry.render(&mut out_dry, &params, &empty_mod, SAMPLE_RATE);
        voice_mod.render(&mut out_mod, &params, &mod_state, SAMPLE_RATE);
    }

    // Compare outputs: they should differ due to filter cutoff modulation
    let mut diff_sum = 0.0f32;
    for i in 0..BLOCK_SIZE {
        diff_sum += (out_dry[i] - out_mod[i]).abs();
    }

    assert!(
        diff_sum > 0.001,
        "modulated voice should differ from dry voice, diff_sum={}",
        diff_sum
    );
}

#[test]
fn mod_bar_shows_when_enabled() {
    let mut matrix = MatrixState::new();
    // Not enabled yet
    assert!(
        matrix.mod_info_for_param(1, 0).is_none(),
        "disabled param should return None"
    );

    // Enable block 1, param 0
    matrix.set_mod_enabled(1, 0, true);
    let info = matrix.mod_info_for_param(1, 0);
    assert!(
        info.is_some(),
        "enabled param should return Some"
    );
}

#[test]
fn mod_bar_amount_reflects_matrix() {
    let mut matrix = MatrixState::new();
    matrix.num_sources = 2;

    // Enable a destination
    matrix.set_mod_enabled(0, 1, true);

    // Set amounts from two sources
    matrix.amounts[0][0] = 64;
    matrix.amounts[1][0] = 32;

    let info = matrix.mod_info_for_param(0, 1);
    assert!(info.is_some(), "enabled param should have mod info");

    let amount = info.unwrap();
    // Total = (64 + 32) / 127 = 96/127 ~= 0.756
    let expected = (64.0 + 32.0) / 127.0;
    assert!(
        (amount - expected).abs() < 0.01,
        "mod_info amount should reflect sum: expected {} got {}",
        expected,
        amount
    );
}

#[test]
fn matrix_state_rebuild_sources() {
    use chimera_core::ui::block_def::BlockDef;
    use chimera_core::ui::page::{CellIcon, PageLayout, ValFmt};

    static ENV_DEF: BlockDef = BlockDef {
        name: "Env",
        short: "ENV",
        layout: PageLayout::BigViz,
        viz: chimera_core::ui::block_def::VizType::Adsr,
        params: [
            chimera_core::ui::block_def::ParamSlot { label: "Atk", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Dec", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Sus", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Rel", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
        ],
    };

    static LFO_DEF: BlockDef = BlockDef {
        name: "LFO",
        short: "LFO",
        layout: PageLayout::BigViz,
        viz: chimera_core::ui::block_def::VizType::None,
        params: [
            chimera_core::ui::block_def::ParamSlot { label: "Rate", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Shape", format: ValFmt::Int(4), icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
        ],
    };

    let sub_pages: &[&BlockDef] = &[&ENV_DEF, &LFO_DEF];

    let mut matrix = MatrixState::new();
    matrix.rebuild_sources(sub_pages);

    assert_eq!(matrix.num_sources, 2);
    assert_eq!(matrix.sources[0].as_ref().unwrap().name, "Env");
    assert_eq!(matrix.sources[1].as_ref().unwrap().name, "LFO");
}

#[test]
fn matrix_state_rebuild_dests_from_chain() {
    use chimera_core::ui::block_def::{BlockDef, ChainBlock};
    use chimera_core::ui::page::{CellIcon, PageLayout, ValFmt};

    static PIZZA_DEF: BlockDef = BlockDef {
        name: "Pizza",
        short: "PIZ",
        layout: PageLayout::BigViz,
        viz: chimera_core::ui::block_def::VizType::WaveformPreview,
        params: [
            chimera_core::ui::block_def::ParamSlot { label: "Shape", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Crush", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Level", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
        ],
    };

    static FILTER_DEF: BlockDef = BlockDef {
        name: "Filter",
        short: "FLT",
        layout: PageLayout::BigViz,
        viz: chimera_core::ui::block_def::VizType::FilterResponse,
        params: [
            chimera_core::ui::block_def::ParamSlot { label: "Freq", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "Reso", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
            chimera_core::ui::block_def::ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None },
        ],
    };

    let chain = [
        ChainBlock { def: &PIZZA_DEF, sub_pages: &[] },
        ChainBlock { def: &FILTER_DEF, sub_pages: &[] },
    ];

    let mut matrix = MatrixState::new();
    // Enable pizza shape (block 0, param 0) and filter freq (block 1, param 0)
    matrix.mod_enabled |= 1 << (0 * 6 + 0); // block 0, param 0
    matrix.mod_enabled |= 1 << (1 * 6 + 0); // block 1, param 0

    matrix.rebuild_dests_from_chain(&chain);

    assert_eq!(matrix.num_dests, 2);

    let dest0 = matrix.dests[0].as_ref().unwrap();
    assert_eq!(dest0.block_idx, 0);
    assert_eq!(dest0.param_idx, 0);
    assert_eq!(dest0.block_short, "PIZ");
    assert_eq!(dest0.param_label, "Shape");

    let dest1 = matrix.dests[1].as_ref().unwrap();
    assert_eq!(dest1.block_idx, 1);
    assert_eq!(dest1.param_idx, 0);
    assert_eq!(dest1.block_short, "FLT");
    assert_eq!(dest1.param_label, "Freq");
}
