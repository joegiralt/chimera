//! The voice's FX slot through the whole chain (ADR 0155): a golden per
//! type and place on VA INIT C3 and ALGO INIT C4, and SOFT PRE and FOLD
//! POST against the DRV and fold they replace.

mod common;

use common::*;

use chimera_core::dsp::voice_fx::params::{FxType, Place};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// `p`'s note `note` through the golden harness's hold and release.
fn render(p: &ParamSnapshot, note: u8) -> Vec<f32> {
    let mut voice = Rig::new(SR);
    voice.note_on(MidiNote::new(note).unwrap(), Velocity::new(VEL).unwrap(), p);
    let (mut out, mut block) = (Vec::with_capacity(TOTAL_SAMPLES), [0.0f32; BLOCK_SIZE]);
    for b in 0..ON_BLOCKS + OFF_BLOCKS {
        if b == ON_BLOCKS {
            voice.note_off();
        }
        voice.render(&mut block, p, &ModState::new());
        out.extend_from_slice(&block);
    }
    out
}

/// `engine`'s INIT with the slot at `ty`, `place`, P1 ½ and `mix`; the
/// other pots at their defaults (P2 ½, LEVEL ½).
fn slot(engine: EngineType, ty: FxType, place: Place, mix: f32) -> ParamSnapshot {
    let mut p = Sound::init(engine).params;
    p.fx.set_type(ty);
    (p.fx.place, p.fx.pots[0], p.fx.mix) = (place, 0.5, mix);
    p
}

/// (case, (FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX)).
// Recorded at the slot's wiring (#614, Task 6).
const GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    (
        "va_c3_soft_pre",
        (
            0x4d0135076ed1e842,
            [
                0, 3215478914, 3214582890, 1067408088, 3214684706, 3214678009, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_soft_post",
        (
            0x7cf7a979602c8366,
            [
                0, 3215482388, 3214592158, 1067398931, 3214689496, 3214682795, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fold_pre",
        (
            0xf2c2437c57c93743,
            [
                0, 3213722362, 3211972795, 1067258309, 1047536202, 1048635333, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fold_post",
        (
            0x7f38672d6c6d076c,
            [
                0, 3213724215, 3211973979, 1067254278, 1047492838, 1048613670, 0, 0,
            ],
        ),
    ),
    (
        "algo_c4_soft_pre",
        (
            0x918c2cf0190de88b,
            [
                991314980, 3199664328, 1051945892, 1051226140, 3199465503, 3199457812, 3188181505,
                3174027665,
            ],
        ),
    ),
    (
        "algo_c4_soft_post",
        (
            0x1e996857f64213b7,
            [
                991316302, 3199663800, 1051948290, 1051777810, 3199464395, 3199456679, 3188183348,
                3174028285,
            ],
        ),
    ),
    (
        "algo_c4_fold_pre",
        (
            0xb46c67bc8353b761,
            [
                982927868, 1033606417, 1026113616, 1048522313, 988228026, 1023964401, 3180330905,
                3166061092,
            ],
        ),
    ),
    (
        "algo_c4_fold_post",
        (
            0xf35a5cb2b78d86b2,
            [
                982927785, 1033614918, 1026186046, 1051973052, 989754560, 1024013554, 3180329307,
                3166061000,
            ],
        ),
    ),
    // FUZZ's, recorded at its kernel (#614, Task 11).
    (
        "va_c3_fuzz_pre",
        (
            0x25f35ec17aaf8de6,
            [
                0, 3196583507, 3195809181, 1048883777, 3195244524, 3195235986, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fuzz_post",
        (
            0x7a54be941d203443,
            [
                0, 3196583823, 3195809655, 1048883580, 3195245123, 3195236581, 0, 0,
            ],
        ),
    ),
    (
        "algo_c4_fuzz_pre",
        (
            0x18b3665098d408e1,
            [
                940378252, 3167774530, 1032082417, 3172249448, 3173240141, 3173784984, 3167804953,
                1031523999,
            ],
        ),
    ),
    (
        "algo_c4_fuzz_post",
        (
            0x9c964aed9b7c32cf,
            [
                940379617, 3168372192, 1032082879, 3172246888, 3173239350, 3173783303, 3167811901,
                1031526824,
            ],
        ),
    ),
];

#[test]
fn every_type_and_place_matches_its_golden() {
    let mut got = Vec::new();
    for (engine, note, e) in [
        (EngineType::Va, 48, "va_c3"),
        (EngineType::Algo, 60, "algo_c4"),
    ] {
        for (ty, t) in [
            (FxType::Soft, "soft"),
            (FxType::Fuzz, "fuzz"),
            (FxType::Fold, "fold"),
        ] {
            for (place, pl) in [(Place::Pre, "pre"), (Place::Post, "post")] {
                let out = render(&slot(engine, ty, place, 1.0), note);
                let name: &'static str = format!("{e}_{t}_{pl}").leak();
                got.push((name, (fnv1a(&out), spots(&out))));
            }
        }
    }
    golden::check(GOLDENS, &got);
}

/// The old chain on VA INIT C4, DRIVE ½ (TONE ½, MIX 1) and FOLD ½ (SYM ½,
/// MIX ½), each alone: FNV-1a over the golden harness's render, recorded
/// before the stages were deleted (#614). SOFT PRE and FOLD POST at LEVEL ½
/// are those stages, bit for bit.
const OLD_DRV: u64 = 0xd6bd_8ae7_98d6_3859;
const OLD_FOLD: u64 = 0x3802_70ca_2d1b_f542;

#[test]
fn soft_pre_is_todays_drv_and_fold_post_is_todays_fold() {
    let soft = slot(EngineType::Va, FxType::Soft, Place::Pre, 1.0);
    assert_eq!(fnv1a(&render(&soft, NOTE)), OLD_DRV);
    let fold = slot(EngineType::Va, FxType::Fold, Place::Post, 0.5);
    assert_eq!(fnv1a(&render(&fold, NOTE)), OLD_FOLD);
}
