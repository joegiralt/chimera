//! Spec § Testing "Modulatable is true": for every address whose spec says
//! `modulatable: true`, an LFO route changes the rendered output. Keeps the
//! flag from lying.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// A base sound in which `block` is audible (spec: engine params use their
/// own engine; drive/folder > 0; amp env and FM use Pizza or FM).
fn recipe(block: BlockRef) -> ParamSnapshot {
    let engine = match block {
        BlockRef::Fm | BlockRef::FmOp(_) => EngineType::Fm,
        BlockRef::Modal => EngineType::Modal,
        _ => EngineType::Pizza, // Pizza, AmpEnv, Out, Drive, Filter, Folder
    };
    let mut p = ParamSnapshot::for_engine(engine);
    p.lfo.rate = 5.0; // swings both ways within the render
    match block {
        BlockRef::Fm | BlockRef::FmOp(_) => {
            p.fm.algorithm = 7; // every operator is a carrier
            for op in p.fm.operators.iter_mut() {
                op.level = 99.0;
            }
        }
        BlockRef::Drive => p.drive.drive = 0.5,
        BlockRef::Filter => p.filter.cutoff = 2000.0,
        BlockRef::Folder => p.folder.fold = 0.5,
        _ => {}
    }
    p
}

fn lfo_route(addr: ParamAddr) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(addr, *b"TEST\0\0\0\0").expect("modulatable");
    let mut ms = ModState::from_registry(&reg, 2);
    ms.set_amount(1, 0, 127);
    ms
}

fn render(params: &ParamSnapshot, mod_state: &ModState) -> Vec<f32> {
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, params);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..200 {
        if b == 100 {
            voice.note_off();
        }
        voice.render(&mut block, params, mod_state);
        out.extend_from_slice(&block);
    }
    out
}

#[test]
fn every_modulatable_param_audibly_changes_output() {
    let mut checked = 0;
    for block in BlockRef::ALL {
        for spec in block.specs() {
            let addr = ParamAddr::new(block, spec.id);
            if !addr.modulatable() {
                continue;
            }
            let base = recipe(block);
            let dry = render(&base, &ModState::new());
            let wet = render(&base, &lfo_route(addr));
            let diff = dry
                .iter()
                .zip(&wet)
                .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(
                diff > 1e-4,
                "{block:?}.{}: LFO route changes nothing (max diff {diff})",
                spec.label
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 25);
}
