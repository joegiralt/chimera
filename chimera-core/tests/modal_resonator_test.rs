//! Modal 2's resonators (spec 2026-09-29-modal-2-resonators § Tests).
mod common;

use chimera_core::block::Block;
use chimera_core::block::ParamKind;
use chimera_core::dsp::modal::{BankModes, MODAL_SPECS, ModalParams, ResonatorMode, reads};
use chimera_core::dsp::note_to_freq;
use chimera_hal::BLOCK_SIZE;
use common::{
    SR, assert_stable, fundamental_hz, play_modal, play_modal_at, play_modal_bare, rms_diff,
};

const MODES: [ResonatorMode; 4] = [
    ResonatorMode::String,
    ResonatorMode::Modal,
    ResonatorMode::Bowed,
    ResonatorMode::Sympathetic,
];

/// A bow's steady limit cycle wobbles about 0.2 % between seconds; a
/// runaway grows far past this. Its render is deterministic (no noise).
const BOW_MARGIN: f32 = 1.02;

/// Each model, each setting at its min and max, C2 held 30 s: bounded, no
/// growth, no DC at the output. Bowed also at C3: its C2 is silent until #206's fix.
#[test]
fn every_model_is_stable_at_every_extreme() {
    let sr = SR as usize;
    std::thread::scope(|scope| {
        for mode in MODES {
            scope.spawn(move || {
                let (notes, margin): (&[u8], f32) = match mode {
                    ResonatorMode::Bowed => (&[36, 48], BOW_MARGIN),
                    _ => (&[36], 1.001),
                };
                for s in MODAL_SPECS.iter().filter(|s| s.id != ModalParams::MODE) {
                    for (&note, v) in notes.iter().flat_map(|n| [(n, s.min), (n, s.max)]) {
                        let mut p = ModalParams {
                            mode,
                            ..Default::default()
                        };
                        p.set(s.id, v);
                        let out = play_modal(&p, note, 30 * sr / BLOCK_SIZE, 0);
                        let label = format!("{mode:?} {note} {} = {v}", s.label);
                        assert_stable(&out, 4.0, margin, &label);
                    }
                }
            });
        }
    });
}

/// STRING and the SYMP main string (halo bare), G1 to C7, DAMP longest and
/// BRIGHT brightest, BODY off (its interim half-delay comb kills the odd
/// partials until Task 8): the fundamental within ±2 cents, and at G1 and C3
/// partials 2 to 4 within ±5 cents of its multiples. Whole-sample tuning
/// fails the first; a DC blocker in the loop, the second.
#[test]
fn strings_are_in_tune() {
    let blocks = 3 * SR as usize / BLOCK_SIZE;
    std::thread::scope(|scope| {
        for mode in [ResonatorMode::String, ResonatorMode::Sympathetic] {
            scope.spawn(move || {
                let p = ModalParams {
                    mode,
                    damp: 1.0,
                    bright: 1.0,
                    body: 0.0,
                    ..Default::default()
                };
                for n in 31..=96 {
                    let out = match mode {
                        ResonatorMode::Sympathetic => play_modal_bare(&p, n, 100, blocks, 0),
                        _ => play_modal_at(&p, n, 100, blocks, 0),
                    };
                    let f0 = note_to_freq(n) as f64;
                    let s = &out[SR as usize / 4..SR as usize * 5 / 4];
                    let f1 = fundamental_hz(s, f0);
                    let cents = 1200.0 * (f1 / f0).log2();
                    assert!(cents.abs() < 2.0, "{mode:?} note {n}: {cents:+.2} cents");
                    // Harmonic, not just f0: partials 2..4 on multiples of it.
                    if matches!(n, 31 | 48) {
                        for k in 2..=4 {
                            let h = k as f64 * f1;
                            let off = 1200.0 * (fundamental_hz(s, h) / h).log2();
                            assert!(off.abs() < 5.0, "{mode:?} note {n} h{k}: {off:+.2} cents");
                        }
                    }
                }
            });
        }
    });
}

/// Held until Task 9: today's LFO moves the heads under a sample a second.
const INAUDIBLE_UNTIL_T9: &[(ResonatorMode, chimera_core::block::ParamId)] =
    &[(ResonatorMode::String, ModalParams::ENS_RATE)];

/// Per model, from every continuous setting at 0.5 and MODES at 32, note
/// 48 held 1 s: each setting at its min, middle and max. A setting the
/// model reads changes the sound; one it ignores changes no bit. The
/// middle, as POS's ends can null alike.
#[test]
fn live_knobs_move_dimmed_knobs_do_not() {
    let blocks = SR as usize / BLOCK_SIZE;
    for mode in MODES {
        let mut base = ModalParams {
            mode,
            modes: BankModes::M32,
            ..Default::default()
        };
        for s in MODAL_SPECS
            .iter()
            .filter(|s| s.kind == ParamKind::Continuous)
        {
            base.set(s.id, 0.5);
        }
        for s in MODAL_SPECS.iter().filter(|s| s.id != ModalParams::MODE) {
            if INAUDIBLE_UNTIL_T9.contains(&(mode, s.id)) {
                continue;
            }
            let [lo, mid, hi] = [s.min, s.quantize((s.min + s.max) * 0.5), s.max].map(|v| {
                let mut p = base;
                p.set(s.id, v);
                play_modal(&p, 48, blocks, 0)
            });
            let same =
                |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
            if reads(mode, s.id) {
                let moved = [(&lo, &mid), (&mid, &hi), (&lo, &hi)]
                    .iter()
                    .map(|(a, b)| rms_diff(a, b))
                    .fold(0.0, f32::max);
                assert!(moved > 1e-3, "{mode:?} {}: live but inaudible", s.label);
            } else {
                assert!(
                    same(&lo, &hi) && same(&lo, &mid),
                    "{mode:?} {}: dimmed but heard",
                    s.label
                );
            }
        }
    }
}

/// SYMP's STRUCTURE tunes the halo only: bare, it changes no bit of the
/// main string; with a halo, it changes the sound.
#[test]
fn symp_structure_tunes_only_the_halo() {
    let blocks = SR as usize / BLOCK_SIZE;
    let at = |structure: f32| ModalParams {
        mode: ResonatorMode::Sympathetic,
        structure,
        ..Default::default()
    };
    let bare = [0.0, 1.0].map(|s| play_modal_bare(&at(s), 48, 100, blocks, 0));
    assert!(
        bare[0]
            .iter()
            .zip(&bare[1])
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "bare: STRUCTURE moved the main string"
    );
    let full = [0.0, 1.0].map(|s| play_modal(&at(s), 48, blocks, 0));
    assert!(
        rms_diff(&full[0], &full[1]) > 1e-3,
        "halo: STRUCTURE inaudible"
    );
}
