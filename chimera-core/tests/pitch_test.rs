//! Voice pitch as a matrix destination (#162, ADR 0042): PITCH and FINE on
//! every engine's PIT page.

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot, PitchParams};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{SR, goertzel, init_params, lfo_route};

const PITCH: ParamAddr = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
const FINE: ParamAddr = ParamAddr::new(BlockRef::Pitch, PitchParams::FINE);

/// Note 60 held for `blocks` blocks.
fn render(p: &ParamSnapshot, mods: &ModState, blocks: usize) -> Vec<f32> {
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut blk = [0.0f32; BLOCK_SIZE];
    for _ in 0..blocks {
        v.render(&mut blk, p, mods);
        out.extend_from_slice(&blk);
    }
    out
}

fn with_pitch(engine: EngineType, pitch: f32, fine: f32) -> ParamSnapshot {
    let mut p = init_params(engine);
    (p.pitch.pitch, p.pitch.fine) = (pitch, fine);
    p
}

/// The strongest bin between 100 Hz and 4 kHz (5.9 Hz bins).
fn peak_hz(x: &[f32]) -> f32 {
    let w = &x[256..256 + 8192];
    let bin = SR as f32 / w.len() as f32;
    (17..=700)
        .map(|k| k as f32 * bin)
        .max_by(|&a, &b| goertzel(w, a, SR).total_cmp(&goertzel(w, b, SR)))
        .unwrap()
}

#[test]
fn pitch_plus_12_doubles_the_frequency_on_every_engine() {
    for engine in EngineType::ALL {
        let none = ModState::new();
        let at_0 = peak_hz(&render(&with_pitch(engine, 0.0, 0.0), &none, 200));
        let at_12 = peak_hz(&render(&with_pitch(engine, 12.0, 0.0), &none, 200));
        let ratio = at_12 / at_0;
        assert!(
            (ratio - 2.0).abs() < 0.03,
            "{engine:?}: {at_0} Hz → {at_12} Hz"
        );
    }
}

#[test]
fn fine_plus_100_is_pitch_plus_1() {
    for engine in EngineType::ALL {
        let none = ModState::new();
        let fine = render(&with_pitch(engine, 0.0, 100.0), &none, 50);
        let semi = render(&with_pitch(engine, 1.0, 0.0), &none, 50);
        let flat = render(&with_pitch(engine, 0.0, 0.0), &none, 50);
        assert_eq!(fine, semi, "{engine:?}");
        assert_ne!(fine, flat, "{engine:?}");
    }
}

/// Zero crossings (rising) per window of `w` samples.
fn crossings(x: &[f32], w: usize) -> Vec<usize> {
    x.chunks_exact(w)
        .map(|c| c.windows(2).filter(|p| p[0] < 0.0 && p[1] >= 0.0).count())
        .collect()
}

#[test]
fn an_lfo_on_pitch_is_vibrato() {
    let mut p = init_params(EngineType::Algo);
    p.lfos[0].rate = 2.0;
    let flat = render(&p, &ModState::new(), 400);
    let vib = render(&p, &lfo_route(PITCH), 400);
    // 1200-sample windows (25 ms) across 0.5 s: two LFO cycles.
    let still = crossings(&flat[2400..], 1200);
    let moving = crossings(&vib[2400..], 1200);
    let span = |c: &[usize]| c.iter().max().unwrap() - c.iter().min().unwrap();
    assert!(span(&still) <= 1, "unrouted pitch holds: {still:?}");
    assert!(span(&moving) >= 4, "routed pitch moves: {moving:?}");
    // Periodic: at 2 Hz the window counts rise and fall more than once.
    let turns = moving
        .windows(3)
        .filter(|t| (t[1] > t[0]) != (t[2] > t[1]) && t[1] != t[0] && t[2] != t[1])
        .count();
    assert!(turns >= 2, "{moving:?}");
}

#[test]
fn pitch_and_fine_are_modulatable_voice_destinations() {
    assert!(PITCH.modulatable());
    assert!(FINE.modulatable());
    assert!(BlockRef::Pitch.voice_reads());
}

#[test]
fn the_offset_laws_are_semitones_and_cents() {
    // ±127 is ±24 semitones on PITCH and ±100 cents on FINE.
    let (pitch, fine) = (PITCH.spec().unwrap(), FINE.spec().unwrap());
    assert_eq!(pitch.offset(0.0, 1.0), 24.0);
    assert_eq!(pitch.offset(0.0, -0.5), -12.0);
    assert_eq!(pitch.offset(20.0, 1.0), 24.0, "clamped");
    assert_eq!(fine.offset(0.0, 1.0), 100.0);
    assert_eq!(fine.offset(-50.0, 0.25), -25.0);
    assert_eq!(
        (pitch.min, pitch.max, fine.min, fine.max),
        (-24.0, 24.0, -100.0, 100.0)
    );
}

/// A route moves every Modal model's pitch mid-note (the resonators per
/// block, the strings when the ratio changes) to where the same stored
/// offset puts it.
#[test]
fn a_route_to_pitch_retunes_every_modal_model() {
    use chimera_core::dsp::modal::ResonatorMode;
    use chimera_core::mod_path::ModDestRegistry;
    use chimera_core::modulation::{MAX_MOD_SOURCES, ModSource, amount_scale};
    let mut reg = ModDestRegistry::new();
    reg.add(PITCH, *b"PITCH\0\0\0").unwrap();
    let mut up = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    up.set_amount(ModSource::Vel.index(), 0, 127);
    let st = amount_scale(127) * Velocity::DEFAULT.unit() * 24.0;
    let bin = SR as f32 / 8192.0;
    for mode in [
        ResonatorMode::Modal,
        ResonatorMode::String,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ] {
        let mut p = init_params(EngineType::Modal);
        p.modal.mode = mode;
        let flat = peak_hz(&render(&p, &ModState::new(), 200));
        let routed = peak_hz(&render(&p, &up, 200));
        let mut stored = p.clone();
        stored.pitch.pitch = st;
        let stored = peak_hz(&render(&stored, &ModState::new(), 200));
        assert!(
            (routed - stored).abs() <= bin,
            "{mode:?}: routed {routed} Hz, stored {stored} Hz"
        );
        assert!((routed - flat).abs() > bin, "{mode:?}: {flat} Hz unmoved");
    }
}
