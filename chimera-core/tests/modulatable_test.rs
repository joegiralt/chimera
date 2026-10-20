//! Spec § Testing "Modulatable is true": for every address whose spec says
//! `modulatable: true`, an LFO route changes the rendered output. Keeps the
//! flag from lying. An ENV slot is heard through its own route into CUTOFF.
mod common;
use common::{Rig, routes};

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::budget::FORMANT_N_UNLINKED;
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::modulator::EnvType;
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::params::{EngineType, EnvParams, FilterParams, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// A base sound in which `addr` is audible: Modal's or VA's own engine, else Algo
/// (every operator heard at ALG A for its own parameters, the triangle for
/// the chain's). An ENV slot takes the TYPE its parameter acts on.
fn recipe(addr: ParamAddr) -> ParamSnapshot {
    let block = addr.block;
    let engine = match block {
        BlockRef::Modal => EngineType::Modal,
        BlockRef::Va => EngineType::Va,
        _ => EngineType::Algo,
    };
    let mut p = ParamSnapshot::for_engine(engine);
    p.lfos[0].rate = 5.0; // swings both ways within the render
    match block {
        BlockRef::Algo | BlockRef::AlgoOp(_) => {
            p.algo.try_set_algs(AlgoId::A1, p.algo.algs().1).unwrap();
            p.algo.try_set_algs(p.algo.algs().0, AlgoId::A17).unwrap();
            p.algo.morph = 64;
            for (i, op) in p.algo.ops.iter_mut().enumerate() {
                (op.level, op.coarse) = (80, [4, 8, 10, 13, 16, 19][i]);
            }
        }
        BlockRef::Modal => {}
        // OSC 2 a fifth up: MIX's AMOUNT then moves between two tones.
        BlockRef::Va => p.va.coarse[1] = 7,
        _ => p.algo = AlgoParams::single(WaveId::TRI),
    }
    match block {
        BlockRef::Filter => {
            p.filter.cutoff = 2000.0;
            // STABILITY acts on the 2040 and LADDER.
            if addr.param == FilterParams::STAB {
                p.filter.set_kind(FilterKind::K2040);
            }
        }
        BlockRef::VoiceFx => {
            p.fx.set_type(FxType::Soft);
            p.fx.pots[0] = 0.5;
        }
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

/// The bases `addr` is checked on: `recipe`'s, or for the formant
/// operators' FRMT and BW, operator *n* as FRMT and again as NOIS, and for
/// VOWEL, operators 1 and 2 FRMT and 3 FRMT if the budget holds three (else
/// NOIS), on A1 (no links).
fn recipes(addr: ParamAddr) -> Vec<ParamSnapshot> {
    let formant = |kinds: &[(usize, OpKind)]| {
        let mut p = recipe(addr);
        p.algo.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
        for &(i, k) in kinds {
            p.algo.try_set_kind(i, k).unwrap();
        }
        p
    };
    match (addr.block, addr.param) {
        (BlockRef::AlgoOp(op), AlgoOpParams::FRMT | AlgoOpParams::BW) => {
            let n = op.index();
            vec![
                formant(&[(n, OpKind::Formant)]),
                formant(&[(n, OpKind::Noise)]),
            ]
        }
        (BlockRef::Algo, AlgoParams::VOWEL) => {
            let third = if FORMANT_N_UNLINKED >= 3 {
                OpKind::Formant
            } else {
                OpKind::Noise
            };
            vec![formant(&[
                (0, OpKind::Formant),
                (1, OpKind::Formant),
                (2, third),
            ])]
        }
        _ => vec![recipe(addr)],
    }
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
            for (case, base) in recipes(addr).iter().enumerate() {
                let repluck = block == BlockRef::Modal;
                let dry = render(base, &routes(addr, 0), repluck);
                let wet = render(base, &routes(addr, 127), repluck);
                let diff = dry
                    .iter()
                    .zip(&wet)
                    .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
                assert!(
                    diff > 1e-4,
                    "{block:?}.{} case {case}: LFO route changes nothing (max diff {diff})",
                    spec.label
                );
            }
            checked += 1;
        }
    }
    // + VCA; PITCH, FINE; Modal's macros; VA's; FRMT and BW per operator,
    // VOWEL; the FX slot's pots and MIX; less DRV's three and the fold's;
    // the filter's STABILITY.
    assert_eq!(checked, 17 + 3 * 5 + 1 + 2 + 4 + 11 + 2 * 6 + 1 + 4 - 6 + 1);
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
