use chimera_core::{MidiNote, Velocity};
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::{ModState, MAX_MOD_SOURCES};
use chimera_core::params::ParamSnapshot;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::params::{DriveParams, FilterParams};
use chimera_core::ui::mod_grid::MatrixState;

use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const DRIVE: ParamAddr = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
const CRUSH: ParamAddr = ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH);
const SHAPE: ParamAddr = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);

fn rms(buf: &[f32]) -> f32 {
    let sum: f32 = buf.iter().map(|s| s * s).sum();
    (sum / buf.len() as f32).sqrt()
}

#[test]
fn voice_render_with_empty_mod_state() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    let params = ParamSnapshot::default();

    voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);

    let mut output = [0.0f32; BLOCK_SIZE];
    voice.render(&mut output, &params, &empty_mod);

    // Should produce sound normally
    let level = rms(&output);
    assert!(level > 0.001, "voice with empty mod state should still produce sound, rms={}", level);
}

#[test]
fn voice_render_with_mod_offset_changes_filter() {
    // Render two voices identically, except one has an LFO modulating filter cutoff.
    let mut voice_dry = Voice::new(chimera_hal::SAMPLE_RATE);
    let mut voice_mod = Voice::new(chimera_hal::SAMPLE_RATE);
    let mut params = ParamSnapshot::default();
    params.filter.cutoff = 2000.0;
    params.filter.mode = 2; // LP4

    // Set up LFO: fast rate so it clearly modulates within a few blocks
    params.lfo.rate = 10.0;
    params.lfo.depth = 1.0;
    params.lfo.shape = 0; // sine

    // Dry: no modulation
    let empty_mod = ModState::new();

    // Modulated: LFO (source 1) -> filter cutoff
    let mut registry = chimera_core::mod_path::ModDestRegistry::new();
    registry.add(CUTOFF, *b"FLTCUT\0\0").unwrap();
    let mut mod_state = ModState::from_registry(&registry, 2); // env, LFO
    mod_state.set_amount(1, 0, 100); // LFO -> cutoff at high amount

    voice_dry.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);
    voice_mod.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);

    let mut out_dry = [0.0f32; BLOCK_SIZE];
    let mut out_mod = [0.0f32; BLOCK_SIZE];

    // Render several blocks to let modulation develop
    for _ in 0..16 {
        voice_dry.render(&mut out_dry, &params, &empty_mod);
        voice_mod.render(&mut out_mod, &params, &mod_state);
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
fn mod_bar_shows_when_primed() {
    use chimera_core::mod_path::ModDestRegistry;

    let mut matrix = MatrixState::new();
    // Not primed yet
    assert!(
        matrix.mod_info_for(DRIVE).is_none(),
        "un-primed param should return None"
    );

    // Prime block 1, param 0 via registry
    let mut registry = ModDestRegistry::new();
    registry.add(DRIVE, *b"B1 Prm0\0").unwrap();
    matrix.rebuild_dests_from_registry(&registry);

    let info = matrix.mod_info_for(DRIVE);
    assert!(
        info.is_some(),
        "primed param should return Some"
    );
}

#[test]
fn mod_bar_amount_reflects_matrix() {
    use chimera_core::mod_path::ModDestRegistry;

    let mut matrix = MatrixState::new();
    matrix.num_sources = 2;

    // Prime a destination via registry
    let mut registry = ModDestRegistry::new();
    registry.add(CRUSH, *b"TSTaPrm\0").unwrap();
    matrix.rebuild_dests_from_registry(&registry);

    // Set amounts from two sources
    matrix.amounts[0][0] = 64;
    matrix.amounts[1][0] = 32;

    let info = matrix.mod_info_for(CRUSH);
    assert!(info.is_some(), "primed param should have mod info");

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
    let mut matrix = MatrixState::new();
    matrix.rebuild_sources(&["Env", "LFO"]);

    assert_eq!(matrix.num_sources, 2);
    assert_eq!(matrix.sources[0].as_ref().unwrap().name, "Env");
    assert_eq!(matrix.sources[1].as_ref().unwrap().name, "LFO");
}

#[test]
fn matrix_state_rebuild_dests_from_registry() {
    use chimera_core::mod_path::ModDestRegistry;

    let mut registry = ModDestRegistry::new();
    registry.add(SHAPE, *b"PIZShape").unwrap();
    registry.add(DRIVE, *b"FLT Freq").unwrap();

    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);

    assert_eq!(matrix.num_dests, 2);

    let dest0 = matrix.dests[0].as_ref().unwrap();
    assert_eq!(dest0.addr, SHAPE);
    assert_eq!(dest0.label_str(), "PIZShape");

    let dest1 = matrix.dests[1].as_ref().unwrap();
    assert_eq!(dest1.addr, DRIVE);
    assert_eq!(dest1.label_str(), "FLT Freq");
}
