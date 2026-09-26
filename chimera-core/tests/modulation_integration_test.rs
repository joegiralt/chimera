use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_core::params::{DriveParams, EngineType, FilterParams};
use chimera_core::ui::mod_grid::MatrixState;
use chimera_core::{MidiNote, Velocity};

use chimera_hal::BLOCK_SIZE;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const DRIVE: ParamAddr = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
const MORPH: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
const OP1_LEVEL: ParamAddr = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::LEVEL);

fn rms(buf: &[f32]) -> f32 {
    let sum: f32 = buf.iter().map(|s| s * s).sum();
    (sum / buf.len() as f32).sqrt()
}

/// Operator 1 alone on the triangle.
fn tri() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::TRI);
    p
}

#[test]
fn voice_render_with_empty_mod_state() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    let params = tri();

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut output = [0.0f32; BLOCK_SIZE];
    voice.render(&mut output, &params, &empty_mod);

    // Should produce sound normally
    let level = rms(&output);
    assert!(
        level > 0.001,
        "voice with empty mod state should still produce sound, rms={}",
        level
    );
}

#[test]
fn voice_render_with_mod_offset_changes_filter() {
    // Render two voices identically, except one has an LFO modulating filter cutoff.
    let mut voice_dry = Voice::new(chimera_hal::SAMPLE_RATE);
    let mut voice_mod = Voice::new(chimera_hal::SAMPLE_RATE);
    let mut params = tri();
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

    voice_dry.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );
    voice_mod.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

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
    assert!(info.is_some(), "primed param should return Some");
}

#[test]
fn mod_bar_amount_reflects_matrix() {
    use chimera_core::mod_path::ModDestRegistry;

    let mut matrix = MatrixState::new();
    matrix.num_sources = 2;

    // Prime a destination via registry
    let mut registry = ModDestRegistry::new();
    registry.add(MORPH, *b"TSTaPrm\0").unwrap();
    matrix.rebuild_dests_from_registry(&registry);

    // Set amounts from two sources
    matrix.amounts[0][0] = 64;
    matrix.amounts[1][0] = 32;

    let info = matrix.mod_info_for(MORPH);
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
    registry.add(OP1_LEVEL, *b"O1 LEVEL").unwrap();
    registry.add(DRIVE, *b"FLT Freq").unwrap();

    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);

    assert_eq!(matrix.num_dests, 2);

    let dest0 = matrix.dests[0].as_ref().unwrap();
    assert_eq!(dest0.addr, OP1_LEVEL);
    assert_eq!(dest0.label_str(), "O1 LEVEL");

    let dest1 = matrix.dests[1].as_ref().unwrap();
    assert_eq!(dest1.addr, DRIVE);
    assert_eq!(dest1.label_str(), "FLT Freq");
}

/// Review Focus 1: the Algo engine keeps the amp envelope off the VCA, but
/// the envelope still drives the ENV mod source.
#[test]
fn env_source_moves_on_an_algo_sound() {
    let mut params = ParamSnapshot::for_engine(EngineType::Algo);
    params.filter.cutoff = 8000.0;
    params.filter.mode = 2;
    let mut registry = chimera_core::mod_path::ModDestRegistry::new();
    registry.add(CUTOFF, *b"FLTCUT\0\0").unwrap();
    let mut routed = ModState::from_registry(&registry, 2);
    routed.set_amount(0, 0, -100); // ENV → cutoff
    let render = |ms: &ModState| {
        let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
        voice.note_on(
            MidiNote::new(60).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );
        let mut out = [0.0f32; BLOCK_SIZE];
        let mut all = Vec::new();
        for _ in 0..16 {
            voice.render(&mut out, &params, ms);
            all.extend_from_slice(&out);
        }
        all
    };
    let (dry, wet) = (render(&ModState::new()), render(&routed));
    let diff: f32 = dry.iter().zip(&wet).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 0.01, "the ENV route changed nothing ({diff})");
}
