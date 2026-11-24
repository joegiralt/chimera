// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

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
/// other pots at their defaults (P2 ½, LEVEL ½). The filter at STABILITY
/// 1, as the goldens were recorded: the slot is what they pin.
fn slot(engine: EngineType, ty: FxType, place: Place, mix: f32) -> ParamSnapshot {
    let mut p = Sound::init(engine).params;
    p.filter.stability = 1.0;
    p.fx.set_type(ty);
    (p.fx.place, p.fx.pots[0], p.fx.mix) = (place, 0.5, mix);
    p
}

/// (case, (FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX)).
// Recorded at the slot's wiring (#614, Task 6); SOFT and FOLD rows
// re-recorded in Task 14 for their `MAKE_UP` (level-matched wet).
const GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    (
        "va_c3_soft_pre",
        (
            0x96a84a659bb7ad36,
            [
                0, 3206715266, 3205849718, 1058664374, 3205948070, 3205941600, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_soft_post",
        (
            0x3f3fd1d7bc9db678,
            [
                0, 3206718618, 3205858658, 1058655533, 3205952690, 3205946217, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fuzz_pre",
        (
            0xec9e435365a823c3,
            [
                0, 3205377212, 3204698625, 1057667708, 3204358665, 3204349738, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fuzz_post",
        (
            0x56ceec1a9de0d554,
            [
                0, 3205377558, 3204698861, 1057667475, 3204359271, 3204350338, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fold_pre",
        (
            0xabcc24c709319a96,
            [
                0, 3208746636, 3206944242, 1062657782, 1042563082, 1043355471, 0, 0,
            ],
        ),
    ),
    (
        "va_c3_fold_post",
        (
            0xd2aae62932b7925b,
            [
                0, 3208749173, 3206945051, 1062652282, 1042533496, 1043325912, 0, 0,
            ],
        ),
    ),
    (
        "algo_c4_soft_pre",
        (
            0x065a7ce979cf4c40,
            [
                997697488, 3190863970, 1043148590, 1043167494, 3190673504, 3190666051, 3180131583,
                3166830462,
            ],
        ),
    ),
    (
        "algo_c4_soft_post",
        (
            0x065a7ce979cf4c40,
            [
                997697488, 3190863970, 1043148590, 1043167494, 3190673504, 3190666051, 3180131583,
                3166830462,
            ],
        ),
    ),
    (
        "algo_c4_fuzz_pre",
        (
            0xe93ffbc100fdd4ef,
            [
                964398755, 3176488806, 1040913796, 3181573199, 3182544346, 3183106308, 3177542865,
                1040356689,
            ],
        ),
    ),
    (
        "algo_c4_fuzz_post",
        (
            0xe93ffbc100fdd4ef,
            [
                964398755, 3176488806, 1040913796, 3181573199, 3182544346, 3183106308, 3177542865,
                1040356689,
            ],
        ),
    ),
    (
        "algo_c4_fold_pre",
        (
            0xcdc4707b99ce07a1,
            [
                992961183, 1032105664, 1021325272, 1038066645, 1015489504, 1026241282, 3176454419,
                3163276402,
            ],
        ),
    ),
    (
        "algo_c4_fold_post",
        (
            0xcdc4707b99ce07a1,
            [
                992961183, 1032105664, 1021325272, 1038066645, 1015489504, 1026241282, 3176454419,
                3163276402,
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
/// were those stages bit for bit; Task 14 re-recorded both for the
/// `MAKE_UP` of SOFT (0.483) and FOLD (0.684), which level-match the wet.
const OLD_DRV: u64 = 0x5db4_3ba4_e2df_eb02;
const OLD_FOLD: u64 = 0x9574_eaed_bbb6_a7ae;

#[test]
fn soft_pre_is_todays_drv_and_fold_post_is_todays_fold() {
    let soft = slot(EngineType::Va, FxType::Soft, Place::Pre, 1.0);
    assert_eq!(fnv1a(&render(&soft, NOTE)), OLD_DRV);
    let fold = slot(EngineType::Va, FxType::Fold, Place::Post, 0.5);
    assert_eq!(fnv1a(&render(&fold, NOTE)), OLD_FOLD);
}
