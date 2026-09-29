//! Modal 2's resonators (spec 2026-09-29-modal-2-resonators § Tests).
mod common;

use chimera_core::block::Block;
use chimera_core::dsp::modal::{MODAL_SPECS, ModalParams, ResonatorMode};
use chimera_core::dsp::note_to_freq;
use chimera_hal::BLOCK_SIZE;
use common::{SR, fundamental_hz, play_modal, play_modal_at, play_modal_bare, rms};

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
/// growth, no DC. Bowed also at C3: its C2 is silent until #206's fix.
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
                        let last = &out[out.len() - sr..];
                        let second = &out[sr..2 * sr];
                        assert!(
                            out.iter().all(|x| x.is_finite() && x.abs() <= 4.0),
                            "{mode:?} {note} {} = {v}: bounded",
                            s.label
                        );
                        assert!(
                            rms(last) <= rms(second) * margin + 1e-6,
                            "{mode:?} {note} {} = {v}: grows",
                            s.label
                        );
                        assert!(
                            (last.iter().sum::<f32>() / last.len() as f32).abs() < 1e-3,
                            "{mode:?} {note} {} = {v}: DC",
                            s.label
                        );
                    }
                }
            });
        }
    });
}

/// STRING and the SYMP main string (halo bare), G1 to C7, DAMP longest and
/// BRIGHT brightest, BODY off (its interim half-delay comb kills the odd
/// partials until Task 8): the fundamental within ±2 cents. Whole-sample
/// tuning fails it.
#[test]
fn strings_are_in_tune() {
    let blocks = 3 * SR as usize / BLOCK_SIZE;
    std::thread::scope(|scope| {
        for mode in [ResonatorMode::String, ResonatorMode::Sympathetic] {
            scope.spawn(move || {
                let p = ModalParams {
                    mode,
                    decay: 0.0,
                    brightness: 0.0,
                    ks_body: 0.0,
                    ..Default::default()
                };
                for n in 31..=96 {
                    let out = match mode {
                        ResonatorMode::Sympathetic => play_modal_bare(&p, n, 100, blocks, 0),
                        _ => play_modal_at(&p, n, 100, blocks, 0),
                    };
                    let f0 = note_to_freq(n) as f64;
                    let s = &out[SR as usize / 4..SR as usize * 5 / 4];
                    let cents = 1200.0 * (fundamental_hz(s, f0) / f0).log2();
                    assert!(cents.abs() < 2.0, "{mode:?} note {n}: {cents:+.2} cents");
                }
            });
        }
    });
}
