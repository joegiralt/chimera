// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! STABILITY on the 2040 and LADDER (ADR 0168), through the voice: at 1
//! every slot plays today's filter bit for bit; below 1 each slot is its own
//! chip, the same one every time, and the output stays bounded.

mod common;

use chimera_core::dsp::filter::FilterKind;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::project::PartId;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use common::rig::{Rig, Src};

/// VA's saws at STAB 1 (identical oscillators, so only the filter
/// differs) through `kind` at 1.2 kHz, RES `res`, STABILITY `stab`.
fn sound(kind: FilterKind, res: f32, stab: f32) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Va);
    p.filter.set_kind(kind);
    (p.filter.cutoff, p.filter.resonance, p.filter.stability) = (1200.0, res, stab);
    p
}

/// Note 57 on slot `slot` of Part `part`, `blocks` blocks.
fn play(p: &ParamSnapshot, part: usize, slot: usize, blocks: usize) -> Vec<f32> {
    let mut rig = Rig::at(SAMPLE_RATE, VoiceIdx::ALL[slot], Src::None);
    rig.part = PartId::ALL[part];
    rig.note_on(MidiNote::new(57).unwrap(), Velocity::DEFAULT, p);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK_SIZE];
    for _ in 0..blocks {
        rig.render(&mut block, p, &ModState::new());
        out.extend_from_slice(&block);
    }
    out
}

fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|v| v.to_bits()).collect()
}

/// STAB 1: every slot and Part plays the same filter, bit for bit; it is
/// the stored-neutral Sound's, so today's.
#[test]
fn full_stability_is_one_chip_on_every_slot() {
    for kind in [FilterKind::K2040, FilterKind::Ladder] {
        let p = sound(kind, 0.5, 1.0);
        let want = bits(&play(&p, 0, 0, 100));
        for (part, slot) in [(0, 3), (0, 7), (1, 0), (5, 4)] {
            assert_eq!(
                bits(&play(&p, part, slot, 100)),
                want,
                "{kind:?} {part} {slot}"
            );
        }
    }
}

/// Below 1 each slot is its own chip, and the same chip every time.
#[test]
fn each_slot_is_its_own_chip() {
    for kind in [FilterKind::K2040, FilterKind::Ladder] {
        let p = sound(kind, 0.5, 0.5);
        let stable = bits(&play(&sound(kind, 0.5, 1.0), 0, 0, 60));
        let takes: Vec<_> = (0..5).map(|s| bits(&play(&p, 0, s, 60))).collect();
        for (s, t) in takes.iter().enumerate() {
            assert_ne!(*t, stable, "{kind:?} slot {s}");
            assert_eq!(*t, bits(&play(&p, 0, s, 60)), "{kind:?} slot {s} again");
            for u in &takes[..s] {
                assert_ne!(t, u, "{kind:?} slot {s}");
            }
        }
    }
}

/// CLEAN and DIRTY never read it.
#[test]
fn clean_and_dirty_ignore_it() {
    for kind in [FilterKind::Svf, FilterKind::Dirty] {
        let want = bits(&play(&sound(kind, 0.5, 1.0), 0, 2, 40));
        assert_eq!(
            bits(&play(&sound(kind, 0.5, 0.0), 0, 2, 40)),
            want,
            "{kind:?}"
        );
    }
}

/// STAB 0 at RES 1 stays bounded on both kinds and every slot.
#[test]
fn full_spread_at_full_res_is_bounded() {
    for kind in [FilterKind::K2040, FilterKind::Ladder] {
        let p = sound(kind, 1.0, 0.0);
        for slot in 0..chimera_core::hw::MAX_VOICES {
            let out = play(&p, 0, slot, 400);
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(
                peak.is_finite() && peak < 4.0,
                "{kind:?} slot {slot}: {peak}"
            );
        }
    }
}
