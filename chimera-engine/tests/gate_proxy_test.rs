// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The VCA's gate through the door (ADR 0195): open, a gated Sound crosses
//! to bank 2's kernel and is heard as the gate; closed, it plays bit for
//! bit what the plain VCA plays. The host builds its own door
//! (`open_host`); the door is a process-wide static, so the tests take
//! turns.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_engine::note_queue::{NoteEvent, NoteKind};
use chimera_engine::params::EngineType;
use chimera_engine::preset::Sound;
use chimera_engine::scope::{ScopeWriter, scope_buffer};
use chimera_engine::{MidiChannel, MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;

static DOOR: Mutex<()> = Mutex::new(());

/// Part 1's bus over a VA INIT note held 40 blocks, then 40 of tail, with
/// the gate's knobs at `k`.
fn render(k: [f32; 3]) -> Vec<u32> {
    let mut s = Sound::init(EngineType::Va);
    s.params.out.gate = k;
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
    let ev = |kind| NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(48).unwrap(),
        kind,
    };
    let mut out = Vec::new();
    for b in 0..80 {
        if b == 0 {
            inst.handle(ev(NoteKind::On(Velocity::new(100).unwrap())), &shared);
        }
        if b == 40 {
            inst.handle(ev(NoteKind::Off), &shared);
        }
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        out.extend(inst.part_bus(0).iter().map(|v| v.to_bits()));
    }
    out
}

const UP: [f32; 3] = [1.0, 0.6, 0.5];

#[test]
fn the_gate_through_the_door_is_heard() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    let (plain, gate) = (render([0.0; 3]), render(UP));
    DOOR_CELL.close();
    assert_ne!(plain, gate);
    assert!(gate.iter().any(|&b| f32::from_bits(b) != 0.0));
}

#[test]
fn the_gate_with_the_door_closed_is_the_plain_vca() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    assert_eq!(render(UP), render([0.0; 3]));
}
