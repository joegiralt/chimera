//! Spec § Testing "Modulatable is true": for every address whose spec says
//! `modulatable: true`, an LFO route changes the rendered output. Keeps the
//! flag from lying. An ENV slot is heard through its own route into CUTOFF.
mod common;
use common::{Rig, routes};

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::modulator::EnvType;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::params::{EngineType, EnvParams, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// A base sound in which `addr` is audible: Modal's own engine, else Algo
/// (every operator heard at ALG A for its own parameters, the triangle for
/// the chain's). An ENV slot takes the TYPE its parameter acts on.
fn recipe(addr: ParamAddr) -> ParamSnapshot {
    let block = addr.block;
    let engine = if block == BlockRef::Modal {
        EngineType::Modal
    } else {
        EngineType::Algo
    };
    let mut p = ParamSnapshot::for_engine(engine);
    p.lfos[0].rate = 5.0; // swings both ways within the render
    match block {
        BlockRef::Algo | BlockRef::AlgoOp(_) => {
            p.algo.alg_a = AlgoId::A1.get();
            p.algo.alg_b = AlgoId::A17.get();
            p.algo.morph = 64;
            for (i, op) in p.algo.ops.iter_mut().enumerate() {
                (op.level, op.coarse) = (80, [4, 8, 10, 13, 16, 19][i]);
            }
        }
        BlockRef::Modal => {}
        _ => p.algo = AlgoParams::single(WaveId::TRI),
    }
    match block {
        BlockRef::Drive => p.drive.drive = 0.5,
        BlockRef::Filter => p.filter.cutoff = 2000.0,
        BlockRef::Folder => p.folder.fold = 0.5,
        BlockRef::Env(s) => {
            p.filter.cutoff = 1000.0;
            p.envelopes[s.index()].env_type = match addr.param {
                EnvParams::LEVEL | EnvParams::TIME => EnvType::A,
                _ => EnvType::B,
            };
        }
        _ => {}
    }
    p
}

/// Note 60 held 100 blocks, then released. `repluck`: again at block 50,
/// so Modal's POS is heard at its next pluck.
fn render(params: &ParamSnapshot, mod_state: &ModState, repluck: bool) -> Vec<f32> {
    let mut voice = Rig::new(chimera_hal::SAMPLE_RATE);
    let note = MidiNote::new(60).unwrap();
    voice.note_on(note, Velocity::DEFAULT, params);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..200 {
        if b == 50 && repluck {
            voice.note_on(note, Velocity::DEFAULT, params);
        }
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
            let base = recipe(addr);
            let repluck = block == BlockRef::Modal;
            let dry = render(&base, &routes(addr, 0), repluck);
            let wet = render(&base, &routes(addr, 127), repluck);
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
    assert_eq!(checked, 17 + 3 * 5 + 1 + 2 + 4); // + VCA; PITCH, FINE; Modal's macros
}

/// #188: ENV 2 → MORPH is heard on the INIT Sound, whose ALG B differs
/// from its ALG A.
#[test]
fn env2_into_morph_is_heard_on_init() {
    let p = ParamSnapshot::for_engine(EngineType::Algo);
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let mut reg = ModDestRegistry::new();
    reg.add(morph, *b"ALGMORPH").unwrap();
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    let dry = render(&p, &ms, false);
    ms.set_amount(ModSource::Env2.index(), 0, 127);
    let wet = render(&p, &ms, false);
    let sq = |s: &mut dyn Iterator<Item = f32>| s.map(|x| x * x).sum::<f32>();
    let rel =
        (sq(&mut dry.iter().zip(&wet).map(|(a, b)| a - b)) / sq(&mut dry.iter().copied())).sqrt();
    assert!(rel > 0.1, "relative RMS change {rel}");
}
