// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ALGO's block through the door (ADR 0212): bank 1's own path on a host
//! and the host's open door (`--features bank2,test-support`) both play
//! what ALGO played before the move, bit for bit; closed, an ALGO note is
//! silent and ends. The door is a process-wide static, so the tests take
//! turns.

use std::sync::Mutex;

use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::factory::{FACTORY_LEN, factory_sound};
use chimera_engine::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_engine::note_queue::{NoteEvent, NoteKind};
use chimera_engine::params::EngineType;
use chimera_engine::preset::Sound;
use chimera_engine::scope::{ScopeWriter, scope_buffer};
use chimera_engine::{MidiChannel, MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;

static DOOR: Mutex<()> = Mutex::new(());

/// Part 1's bus, hashed (FNV-1a over the bits), over a chord of `s` held
/// 200 blocks, a re-strike, then 200 of tail, every LEVEL set to `lift`
/// at block 150 if given; and the voices sounding after the first block.
fn render(s: &Sound, lift: Option<u8>) -> (u64, usize) {
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = s.params.clone();
    *shared.parts[0].mod_state = s.mod_state.clone();
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let (w, _) = Box::leak(Box::new(scope_buffer())).split();
    let mut scope = ScopeWriter::new(w);
    let ev = |n: u8, kind| NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(n).unwrap(),
        kind,
    };
    let notes = [36u8, 55, 64, 79];
    let (mut h, mut after_one) = (0xcbf2_9ce4_8422_2325u64, 0);
    for b in 0..400 {
        if b == 0 {
            for n in notes {
                inst.handle(ev(n, NoteKind::On(Velocity::new(100).unwrap())), &shared);
            }
        }
        if let Some(level) = lift.filter(|_| b == 150) {
            for op in shared.parts[0].params.algo.ops.iter_mut() {
                op.level = level;
            }
        }
        if b == 100 {
            inst.handle(
                ev(notes[0], NoteKind::On(Velocity::new(60).unwrap())),
                &shared,
            );
        }
        if b == 200 {
            for n in notes {
                inst.handle(ev(n, NoteKind::Off), &shared);
            }
        }
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        if b == 0 {
            after_one = inst.sounding();
        }
        for byte in inst
            .part_bus(0)
            .iter()
            .flat_map(|v| v.to_bits().to_le_bytes())
        {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    (h, after_one)
}

/// ALGO INIT and the built-in Sounds that play ALGO.
fn sounds() -> Vec<Sound> {
    let mut v = vec![Sound::init(EngineType::Algo)];
    v.extend(
        (0..FACTORY_LEN)
            .filter_map(factory_sound)
            .filter(|s| s.params.engine() == EngineType::Algo),
    );
    v
}

/// Taken on main before the move (bc20ae7c), bank 1's own path.
const BEFORE: &[u64] = &[
    0xa5740946fc1344fd,
    0xd5e9f3445e81d5de,
    0x33898a23f5f362fa,
    0x7e5886d3543041a3,
    0x898a71c53977288e,
    0x85d874a693bbc522,
    0x9a0526a4b6fe7d7c,
    0x0066615f9d116c31,
    0x225fe1d97e44b25d,
    0xeff5872cf3961cc6,
    0xc3f2e7b223030f63,
    0xba05fb22df25ae86,
    0xe5aea6c48967f8e6,
];

fn hashes() -> Vec<u64> {
    sounds().iter().map(|s| render(s, None).0).collect()
}

#[test]
fn algo_plays_as_before_the_move() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.open_host();
    let h = hashes();
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.close();
    // With the master tape's bus (`master-tape`, 1,494) CHOIR's and VOX's
    // four notes no longer fit beside their re-strike's tail since their
    // bills rose to the unit's worst blocks (ADR 0262): their allocation,
    // not their DSP, differs.
    let mut before = BEFORE.to_vec();
    if cfg!(feature = "master-tape") {
        (before[9], before[12]) = (0x87d1_49c6_8125_5ba1, 0x4a9f_e144_9fdd_07f1);
    }
    assert_eq!(h, before);
}

#[cfg(all(feature = "bank2", feature = "test-support"))]
#[test]
fn algo_with_the_door_closed_is_silent_and_ends() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!chimera_engine::bank2::DOOR_CELL.is_open());
    let silent = {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for _ in 0..400 * chimera_hal::BLOCK_SIZE * 4 {
            // each sample is +0: its bits xor nothing in
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    for s in sounds() {
        assert_eq!(render(&s, None), (silent, 0));
    }
}

/// Operators silent at note-on while their envelopes decay, lit mid-note:
/// a silent op's envelope skips only while it holds (`skip_held`), so a
/// decaying one keeps its steps and is heard where it had got to.
fn decaying() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    for (i, op) in s.params.algo.ops.iter_mut().enumerate() {
        let i = i as u8;
        (op.ar, op.d1r, op.d1l, op.d2r) = (12 + i, 4 + i, 10, 3);
        op.level = if i == 0 { 99 } else { 0 };
    }
    s
}

/// Taken on main before the move (5ec76841).
const DECAYING_BEFORE: u64 = 0x87d5_2bb9_d182_925b;

#[test]
fn a_silent_op_lit_mid_decay_plays_as_before() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.open_host();
    let h = render(&decaying(), Some(99)).0;
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.close();
    assert_eq!(h, DECAYING_BEFORE, "{h:#018x}");
}
