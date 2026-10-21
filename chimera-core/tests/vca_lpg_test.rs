//! The VCA's low-pass gate (ADR 0195): LPG, DECAY and STRIKE at 0 leave
//! every Sound as it was, bit for bit.

mod common;

use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::va::VaMode;
use chimera_core::modulation::{FixedCol, ModSource, ModState};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::rig::Rig;

const SR: u32 = chimera_hal::SAMPLE_RATE;

/// One Sound's run: C3 at 100 for 40 blocks, re-struck at 40 for 40 (the
/// VEL ramp, #483), key up, 60 blocks of tail; once uneased, once glided.
fn play(p: &ParamSnapshot, m: &ModState, h: &mut u64) {
    for glided in [false, true] {
        let mut r = Rig::new(SR);
        let mut b = [0.0f32; BLOCK_SIZE];
        let mut out = Vec::new();
        for vel in [100, 40] {
            r.note_on(MidiNote::new(48).unwrap(), Velocity::new(vel).unwrap(), p);
            for _ in 0..40 {
                if glided {
                    r.render_glided(&mut b, p, m);
                } else {
                    r.render(&mut b, p, m);
                }
                out.extend_from_slice(&b);
            }
        }
        r.note_off();
        for _ in 0..60 {
            r.render(&mut b, p, m);
            out.extend_from_slice(&b);
        }
        assert!(out.iter().any(|&x| x != 0.0), "a run sounds");
        for s in out {
            for byte in s.to_bits().to_le_bytes() {
                *h = (*h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
}

/// The matrices a Sound's VCA sees: none, ENV 1 at 127 (TAPE's and VA's
/// INIT), and ENV 1 with LFO 1 at 64 beside it.
fn matrices(engine: EngineType) -> [ModState; 3] {
    let base = Sound::init(engine).mod_state;
    let (env, lfo) = (ModSource::Env1.index(), ModSource::Lfo1.index());
    let mut none = base.clone();
    none.clear_fixed(env, FixedCol::Vca);
    let mut routed = base.clone();
    routed.set_fixed(env, FixedCol::Vca, 127);
    let mut both = routed.clone();
    both.set_fixed(lfo, FixedCol::Vca, 64);
    [none, routed, both]
}

/// Every engine's INIT through each VCA matrix, VEL at 1 and ½, under
/// CLEAN, the 2040, DIRTY, LADDER and SEM; then every VA MODE. The
/// constant is main's before the gate (ADR 0195).
#[test]
fn the_gate_at_zero_is_todays_vca_bit_for_bit() {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for e in [
        EngineType::Algo,
        EngineType::Modal,
        EngineType::Tape,
        EngineType::Va,
    ] {
        for m in matrices(e) {
            for kind in [
                FilterKind::Svf,
                FilterKind::K2040,
                FilterKind::Dirty,
                FilterKind::Ladder,
                FilterKind::Sem,
            ] {
                for vel in [1.0, 0.5] {
                    let mut p = Sound::init(e).params;
                    p.lfos[0].depth = 1.0;
                    p.filter.set_kind(kind);
                    p.filter.cutoff = 2_000.0;
                    p.out.vca_vel = vel;
                    play(&p, &m, &mut h);
                }
            }
        }
    }
    for mode in VaMode::ALL {
        let mut p = Sound::init(EngineType::Va).params;
        p.va.mode = mode;
        p.va.amount = 0.6;
        play(&p, &matrices(EngineType::Va)[1], &mut h);
    }
    assert_eq!(h, 0xf54a_71e5_c050_585d, "hash {h:#018x}");
}
