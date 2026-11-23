// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's engine slot (plan Task 4): a voice slot holds a hit's kernel,
//! `EngineType::Drum` has its disk code, a DRUM Sound starts on INIT's
//! shared blocks.

mod common;

use chimera_core::Velocity;
use chimera_core::block::DiskCode;
use chimera_core::dsp::algo::engine::AlgoLive;
use chimera_core::dsp::drum::engine::DrumEngine;
use chimera_core::dsp::drum::types::{
    ChokeGroup, DrumHit, HitSeed, KnobSet, Machine, Pad, PadId, Pan, SampleRate, Transpose,
};
use chimera_core::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use chimera_core::dsp::modal::{ModalEngine, SymPool};
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::source_table::GateView;
use chimera_core::taps::TapRows;
use chimera_hal::BLOCK_SIZE;
use chimera_midi::value::U7;
use common::SR;

fn kick() -> DrumHit {
    let pad = Pad::new(
        Machine::ToneKick,
        KnobSet::defaults(Machine::ToneKick),
        U7::MAX,
        Pan::CENTRE,
        ChokeGroup::Off,
        Retrig::Cut,
    );
    DrumHit::new(PadId::ALL[0], pad, Transpose::ZERO)
}

/// Renders `blocks` blocks of `slot`; the peak, and the block it went idle
/// in, if it did.
fn play(slot: &mut EngineSlot, blocks: usize) -> (f32, Option<usize>) {
    let p = ParamSnapshot::for_engine(EngineType::Drum);
    let live = AlgoLive::from_params(&p.algo);
    let mut sym = SymPool::boxed();
    let none = GateView::empty();
    let mut out = [0.0f32; BLOCK_SIZE];
    let mut peak = 0.0f32;
    for b in 0..blocks {
        let mut ctx = EngineCtx {
            sym: &mut sym,
            streams: None,
            src: &none,
            part: chimera_core::project::PartId::ALL[0],
            tune: 0.0,
        };
        slot.render(
            &mut out,
            &p,
            &live,
            SR,
            &mut ctx,
            &mut TapRows::new([false; 2]),
        );
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
        if !slot.is_active() {
            return (peak, Some(b));
        }
    }
    (peak, None)
}

#[test]
fn drum_engine_fits_a_modal_slot() {
    assert!(size_of::<DrumEngine>() <= size_of::<ModalEngine>());
}

#[test]
fn a_drum_slot_rests_silent() {
    let mut slot = EngineSlot::new(SlotKind::Drum);
    assert_eq!(slot.kind(), SlotKind::Drum);
    assert!(!slot.is_active());
    assert_eq!(play(&mut slot, 4), (0.0, Some(0)));
}

#[test]
fn a_struck_kick_sounds_then_ends_within_3_s() {
    let mut slot = EngineSlot::new(SlotKind::Drum);
    let sr = SampleRate::new(SR).expect("a rate");
    slot.drum()
        .expect("a drum slot")
        .strike(&kick(), Velocity::DEFAULT, HitSeed::new(1), sr);
    assert!(slot.is_active());
    let three_s = 3 * SR as usize / BLOCK_SIZE;
    let (peak, ended) = play(&mut slot, three_s);
    assert!(peak > 0.1, "peak {peak}");
    assert!(ended.is_some(), "still sounding after 3 s");
}

#[test]
fn a_hit_keeps_its_pad() {
    let mut slot = EngineSlot::new(SlotKind::Drum);
    let sr = SampleRate::new(SR).expect("a rate");
    let d = slot.drum().expect("a drum slot");
    assert_eq!(d.hit(), None);
    d.strike(&kick(), Velocity::DEFAULT, HitSeed::new(1), sr);
    assert_eq!(d.hit(), Some(kick()));
}

#[test]
fn tonal_pitch_moves_the_kick_both_ways() {
    // Zero crossings over the first 50 ms: an octave up roughly doubles them.
    let crossings = |st: i8| {
        let mut slot = EngineSlot::new(SlotKind::Drum);
        let sr = SampleRate::new(SR).expect("a rate");
        let k = kick();
        let hit = DrumHit::new(k.pad(), k.params(), Transpose::new(st).expect("in range"));
        slot.drum()
            .expect("a drum slot")
            .strike(&hit, Velocity::DEFAULT, HitSeed::new(1), sr);
        let p = ParamSnapshot::for_engine(EngineType::Drum);
        let live = AlgoLive::from_params(&p.algo);
        let mut sym = SymPool::boxed();
        let none = GateView::empty();
        let mut out = [0.0f32; BLOCK_SIZE];
        let mut last = 0.0f32;
        let mut n = 0;
        // Skip the sweep's first 20 ms, count the next 100 ms.
        for b in 0..(120 * SR as usize / 1000 / BLOCK_SIZE) {
            let mut ctx = EngineCtx {
                sym: &mut sym,
                streams: None,
                src: &none,
                part: chimera_core::project::PartId::ALL[0],
                tune: 0.0,
            };
            slot.render(
                &mut out,
                &p,
                &live,
                SR,
                &mut ctx,
                &mut TapRows::new([false; 2]),
            );
            for &x in &out {
                if b * BLOCK_SIZE > 20 * SR as usize / 1000 && (x >= 0.0) != (last >= 0.0) {
                    n += 1;
                }
                last = x;
            }
        }
        n as f32
    };
    let ratio = crossings(12) / crossings(0);
    assert!((1.7..2.3).contains(&ratio), "octave ratio {ratio}");
    let down = crossings(0) / crossings(-12);
    assert!((1.7..2.3).contains(&down), "octave down ratio {down}");
}

#[test]
fn a_plain_note_on_a_drum_slot_starts_nothing() {
    let mut slot = EngineSlot::new(SlotKind::Drum);
    let p = ParamSnapshot::for_engine(EngineType::Drum);
    let mut sym = SymPool::boxed();
    let none = GateView::empty();
    let mut ctx = EngineCtx {
        sym: &mut sym,
        streams: None,
        src: &none,
        part: chimera_core::project::PartId::ALL[0],
        tune: 0.0,
    };
    slot.note_on(
        chimera_core::MidiNote::A4,
        Velocity::DEFAULT,
        &p,
        SR,
        None,
        &mut ctx,
    );
    assert!(!slot.is_active());
}

#[test]
fn drum_round_trips_its_disk_code() {
    let c = EngineType::Drum.disk_code();
    assert_eq!(c, 4);
    assert_eq!(EngineType::Drum.disk_ident(), "DRUM");
    assert_eq!(EngineType::from_disk_code(c), Some(EngineType::Drum));
    assert_eq!(SlotKind::Drum.engine(), EngineType::Drum);
    assert_eq!(
        SlotKind::of(&ParamSnapshot::for_engine(EngineType::Drum)),
        SlotKind::Drum
    );
}

#[test]
fn a_drum_sound_starts_on_init_blocks() {
    let d = ParamSnapshot::for_engine(EngineType::Drum);
    let init = ParamSnapshot::for_engine(EngineType::Algo);
    assert_eq!(d.engine(), EngineType::Drum);
    // The blocks derive no `PartialEq`: compared as printed.
    let shown = |p: &ParamSnapshot| format!("{:?} {:?} {:?}", p.filter, p.envelopes, p.out);
    assert_eq!(shown(&d), shown(&init));
}
