//! Modal 2's resonators (spec 2026-09-29-modal-2-resonators § Tests).
mod common;

use chimera_core::block::Block;
use chimera_core::dsp::modal::{MODAL_SPECS, ModalParams, ResonatorMode};
use chimera_hal::BLOCK_SIZE;
use common::{SR, play_modal, rms};

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
