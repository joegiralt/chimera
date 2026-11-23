// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! LFO DEPTH's routes through the door (ADR 0250): open, INIT's wheel
//! opens LFO 1 DEPTH; closed, DEPTH takes no routes and plays as stored.
//! The host builds its own door (`open_host`); the door is a process-wide
//! static, so the tests take turns.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::addr::{BlockRef, ParamAddr};
use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::dsp::lfo::LfoParams;
use chimera_engine::dsp::modulator::LfoSlot;
use chimera_engine::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_engine::note_queue::{Expr, ExprEvent, NoteEvent, NoteKind};
use chimera_engine::params::EngineType;
use chimera_engine::preset::Sound;
use chimera_engine::scope::{ScopeWriter, scope_buffer};
use chimera_engine::{MidiChannel, MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;
use chimera_midi::value::U7;

static DOOR: Mutex<()> = Mutex::new(());

/// The LFO 1 DEPTH VA INIT's held voice plays with the wheel full up.
fn depth_with_the_wheel_up() -> f32 {
    let s = Sound::init(EngineType::Va);
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = s.params;
    *shared.parts[0].mod_state = s.mod_state;
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let (w, _) = Box::leak(Box::new(scope_buffer())).split();
    let mut scope = ScopeWriter::new(w);
    let ch = MidiChannel::new(0).unwrap();
    inst.handle(
        NoteEvent {
            channel: ch,
            note: MidiNote::new(60).unwrap(),
            kind: NoteKind::On(Velocity::new(100).unwrap()),
        },
        &shared,
    );
    inst.expr(ExprEvent {
        channel: ch,
        expr: Expr::Wheel(U7::new(127).unwrap()),
    });
    for _ in 0..400 {
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
    }
    let depth = ParamAddr::new(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH);
    let slots = inst.allocator().slots();
    let v = (0..slots.len())
        .find(|&v| slots[v].held())
        .expect("a held voice");
    inst.played(depth)[v]
}

#[test]
fn the_wheel_opens_depth_through_the_door() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    let d = depth_with_the_wheel_up();
    DOOR_CELL.close();
    assert!((d - 1.0).abs() < 1e-3, "{d}");
}

#[test]
fn closed_depth_takes_no_routes() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    assert_eq!(depth_with_the_wheel_up(), 0.0);
}
