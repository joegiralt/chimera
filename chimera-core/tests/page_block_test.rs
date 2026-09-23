//! Page encoders, snap and display after moving onto `Block` specs.
//! Parity tests pin today's step sizes (plan § Encoder step audit).

use chimera_core::params::ParamSnapshot;
use chimera_core::ui::page::{PageId, ValFmt};

#[test]
fn pizza_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Pizza.apply_encoder(0, 3, &mut p);
    assert_eq!(p.pizza.shape, 0.5 + 3.0 * (1.0 / 128.0));
    PageId::Pizza.apply_encoder(2, 127, &mut p);
    assert_eq!(p.pizza.level, 1.0);
}

/// Spec § Intended behavior changes: shift-snap now works on Pizza.
#[test]
fn pizza_snap_now_works() {
    let mut p = ParamSnapshot::default();
    PageId::Pizza.snap_encoder(1, 1, &mut p);
    assert_eq!(p.pizza.crush, 100.0 / 127.0);
}

#[test]
fn pizza_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Pizza.read_values(&p), [0.5, 0.0, 0.8, 0.0, 0.0, 0.0]);
}

// ── Modal (Task 3) ───────────────────────────────────────────────────

use chimera_core::dsp::modal::ResonatorMode;

#[test]
fn modal_float_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::EngineModal1.apply_encoder(1, 2, &mut p);
    assert_eq!(p.modal.excite, 0.8 + 2.0 * (1.0 / 128.0));
    PageId::EngineModal2.apply_encoder(5, -1, &mut p);
    assert_eq!(p.modal.ks_ens_mix, 0.0);
}

/// Plan D3: the MODE encoder now reaches Sympathetic (was clamped at Bowed).
#[test]
fn modal_mode_reaches_sympathetic() {
    let mut p = ParamSnapshot::default();
    p.modal.mode = ResonatorMode::Bowed;
    PageId::EngineModal1.apply_encoder(0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    PageId::EngineModal1.apply_encoder(0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
}

/// Review Focus 3: snap on an Enum lands on a valid choice.
#[test]
fn modal_mode_snap_lands_on_integer() {
    let mut p = ParamSnapshot::default();
    PageId::EngineModal1.snap_encoder(0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    PageId::EngineModal1.snap_encoder(0, -1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::String);
}

/// Plan D19: MODE displays mode/3 (mode 2 used to display as "3").
#[test]
fn modal_mode_display_is_true_value() {
    let mut p = ParamSnapshot::default();
    p.modal.mode = ResonatorMode::Bowed;
    assert_eq!(PageId::EngineModal1.read_values(&p)[0], 2.0 / 3.0);
}

/// Plan D5: BODY is a 0..1 float, displayed Uni.
#[test]
fn modal2_body_is_uni() {
    assert_eq!(chimera_core::ui::block_registry::MODAL_2.params[0].format, ValFmt::Uni);
}

// ── Drive (Task 4) ───────────────────────────────────────────────────

#[test]
fn drive_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Drive.apply_encoder(0, 5, &mut p);
    assert_eq!(p.drive.drive, 5.0 * ((1.0 - 0.0) / 128.0));
    PageId::DemoWaves.apply_encoder(1, -2, &mut p);
    assert_eq!(p.drive.tone, 0.5 - 2.0 * (1.0 / 128.0));
}

#[test]
fn drive_snap_uses_spec_format() {
    let mut p = ParamSnapshot::default();
    // TONE is Bi: from the centre, the next point up is +43 (107/127).
    PageId::Drive.snap_encoder(1, 1, &mut p);
    assert_eq!(p.drive.tone, 107.0 / 127.0);
}

#[test]
fn drive_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Drive.read_values(&p), [0.0, 0.5, 1.0, 0.0, 0.0, 0.0]);
}

// ── Filter (Task 5) ──────────────────────────────────────────────────

#[test]
fn filter_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Filter.apply_encoder(0, -1, &mut p);
    assert_eq!(p.filter.cutoff, 20000.0 - (20000.0 - 20.0) / 128.0);
    PageId::Filter.apply_encoder(4, 1, &mut p);
    assert_eq!(p.filter.env_amount, (1.0 - -1.0) / 128.0);
    PageId::DemoShapes.apply_encoder(4, 3, &mut p);
    assert_eq!(p.filter.resonance, 3.0 / 128.0);
}

#[test]
fn filter_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Filter.read_values(&p), [1.0, 0.0, 0.0, 0.0, 0.5, 0.0]);
}

// ── Folder (Task 6) ──────────────────────────────────────────────────

#[test]
fn folder_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Folder.apply_encoder(0, 4, &mut p);
    assert_eq!(p.folder.fold, 4.0 / 128.0);
    PageId::DemoWaves.apply_encoder(3, -1, &mut p);
    assert_eq!(p.folder.symmetry, 0.5 - 1.0 / 128.0);
    assert_eq!(PageId::Folder.read_values(&p)[..3], [4.0 / 128.0, 0.5 - 1.0 / 128.0, 0.5]);
}

// ── Envelopes (Task 7) ───────────────────────────────────────────────

#[test]
fn env_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Vca.apply_encoder(0, 1, &mut p);
    assert_eq!(p.envelopes[0].attack, 0.01 + (10.0 - 0.001) / 128.0);
    PageId::Vca.apply_encoder(2, -1, &mut p);
    assert_eq!(p.envelopes[0].sustain, 0.7 - 1.0 / 128.0);
    PageId::DemoMotion.apply_encoder(4, 1, &mut p);
    assert_eq!(p.envelopes[1].attack, 0.01 + (10.0 - 0.001) / 128.0);
    PageId::EnvAux.apply_encoder(3, -128, &mut p);
    assert_eq!(p.envelopes[2].release, 0.001);
}

// ── LFO (Task 8) ─────────────────────────────────────────────────────

#[test]
fn lfo_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Lfo.apply_encoder(0, 2, &mut p);
    assert_eq!(p.lfo.rate, 1.0 + 2.0 * 0.15);
    PageId::Lfo.apply_encoder(1, 9, &mut p);
    assert_eq!(p.lfo.shape, 4);
    PageId::Lfo.apply_encoder(2, 1, &mut p);
    assert_eq!(p.lfo.sync, 1);
    PageId::Lfo.apply_encoder(5, 3, &mut p);
    assert_eq!(p.lfo.offset, 3.0 * (1.0 / 128.0) * 2.0);
}

/// Spec § Intended behavior changes: shift-snap now works on the LFO page.
#[test]
fn lfo_snap_now_works() {
    let mut p = ParamSnapshot::default();
    PageId::Lfo.snap_encoder(1, 1, &mut p);
    assert_eq!(p.lfo.shape, 4);
}

/// Plan D19: RATE displays over its real range 0.01..20.
#[test]
fn lfo_rate_display_uses_range() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Lfo.read_values(&p)[0], (1.0 - 0.01) / (20.0 - 0.01));
}

// ── FM (Task 9) ──────────────────────────────────────────────────────

/// The selected operator is a process-wide static until Task 20, so every
/// assertion that depends on it lives in this one test.
#[test]
fn fm_pages_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::FmOp.apply_encoder(0, 1, &mut p); // select op B
    PageId::FmOp.apply_encoder(2, 5, &mut p);
    assert_eq!(p.fm.operators[1].level, 5.0);
    PageId::FmOp.apply_encoder(4, -9, &mut p);
    assert_eq!(p.fm.operators[1].detune, -7);
    PageId::FmRatio.apply_encoder(4, 1, &mut p); // FINE of the selected op
    assert_eq!(p.fm.operators[1].fine, 1);
    assert_eq!(PageId::FmOp.read_values(&p)[0], 1.0 / 3.0);
    // Review Focus 3: snapping a Stepped level lands on an integer.
    PageId::FmOp.snap_encoder(2, 1, &mut p);
    assert_eq!(p.fm.operators[1].level, 78.0); // 99 * 100/127 = 77.95 → 78
    PageId::FmOp.apply_encoder(0, -1, &mut p); // back to op A
}

#[test]
fn fm_fixed_op_pages_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::FmAlg.apply_encoder(0, 9, &mut p);
    assert_eq!(p.fm.algorithm, 7);
    PageId::FmRatio.apply_encoder(2, 1, &mut p);
    assert_eq!(p.fm.operators[2].coarse, 5);
    PageId::FmEnv3.apply_encoder(2, -1, &mut p);
    assert_eq!(p.fm.operators[2].decay1_level, 14);
    PageId::DemoFm.apply_encoder(3, 2, &mut p);
    assert_eq!(p.fm.operators[2].feedback, 2.0);
}

/// Review Focus 3: snap on Stepped params lands on integers in range.
#[test]
fn fm_snap_lands_on_integers() {
    let mut p = ParamSnapshot::default();
    PageId::FmRatio.snap_encoder(0, 1, &mut p);
    assert_eq!(p.fm.operators[0].coarse, 63);
    PageId::FmEnv1.snap_encoder(0, -1, &mut p);
    assert_eq!(p.fm.operators[0].attack_rate, 0);
}

/// Plan D4: RR spans 0..=15 (today's encoder stopped at 1).
#[test]
fn fm_rr_reaches_zero() {
    let mut p = ParamSnapshot::default();
    PageId::FmEnv2.apply_encoder(4, -20, &mut p);
    assert_eq!(p.fm.operators[1].release_rate, 0);
}

// ── Out (Task 10) ────────────────────────────────────────────────────

#[test]
fn out_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Mixer.apply_encoder(0, -8, &mut p);
    assert_eq!(p.out.volume, 0.8 - 8.0 / 128.0);
    PageId::Master.apply_encoder(1, 1, &mut p);
    assert_eq!(p.out.pan, 2.0 / 128.0);
    PageId::FmAlg.apply_encoder(2, 1, &mut p);
    assert_eq!(p.out.volume, 0.8 - 8.0 / 128.0 + 1.0 / 128.0);
}

/// The mixer page keeps its placeholder bars for unbound slots.
#[test]
fn mixer_read_values_keep_placeholders() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Mixer.read_values(&p), [0.8, 0.5, 0.5, 0.0, 0.5, 0.0]);
}

// ── FX (Task 11) ─────────────────────────────────────────────────────

#[test]
fn fx_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Delay.apply_encoder(0, 2, &mut p);
    assert_eq!(p.delay.time_ms, 375.0 + 2.0 * 8.0);
    PageId::Chorus.apply_encoder(0, 5, &mut p);
    assert_eq!(p.chorus.mode, 3);
    PageId::MixReverb.apply_encoder(0, 5, &mut p);
    assert_eq!(p.reverb.reverb_type, 2);
    PageId::Efx.apply_encoder(4, 1, &mut p);
    assert_eq!(p.reverb.mix, 1.0 / 128.0);
}
