//! Rebuilds build in place (ADR 0051): engine and model switches run on a
//! thread whose whole stack is one `ModalEngine`, so a by-value engine on
//! the stack overflows it. Its own binary: an overflow aborts the process.
mod common;
use common::{SR, tri};

use chimera_core::dsp::modal::{ModalEngine, ResonatorMode};
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

fn modal(mode: ResonatorMode) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = mode;
    p
}

#[test]
fn rebuild_fits_a_small_stack() {
    let t = std::thread::Builder::new()
        .stack_size(size_of::<ModalEngine>())
        .spawn(|| {
            let mut raw = Box::<Voice>::new_uninit();
            Voice::init_in_place(&mut raw, SR);
            // SAFETY: `init_in_place` built a valid voice in the box.
            let mut v = unsafe { raw.assume_init() };
            let sounds = [
                tri(),
                modal(ResonatorMode::String),
                modal(ResonatorMode::Sympathetic),
                tri(),
            ];
            let mods = ModState::new();
            let mut b = [0.0f32; BLOCK_SIZE];
            v.note_on(MidiNote::A4, Velocity::DEFAULT, &sounds[0]);
            for p in &sounds {
                for _ in 0..4 {
                    v.render(&mut b, p, &mods);
                }
            }
            v.rebuilds()
        })
        .unwrap();
    assert_eq!(
        t.join().ok(),
        Some(3),
        "each of the three switches rebuilds once"
    );
}
