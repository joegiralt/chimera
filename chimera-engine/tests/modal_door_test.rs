// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MODAL's block through the door (ADR 0215): bank 1's own path on a host
//! and the host's open door (`--features bank2,test-support`) both play
//! what MODAL played before the move, bit for bit; closed, a MODAL note is
//! silent and ends. The cases also lock #793's MODAL loops: every model,
//! each MODES of the bank, POS, STRUCTURE, DAMP and BRIGHT moving under
//! LFOs, glides, notes high enough to drop modes, and the built-in MODAL
//! Sounds. The door is a process-wide static, so the tests take turns.

use std::sync::Mutex;

use chimera_engine::addr::{BlockRef, ParamAddr};
use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::dsp::modal::{BankModes, ModalParams, ResonatorMode};
use chimera_engine::factory::{FACTORY_LEN, factory_sound};
use chimera_engine::hw::SampleBudget;
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_engine::mod_path::ModDestRegistry;
use chimera_engine::modulation::{MAX_MOD_SOURCES, ModSource, ModState};
use chimera_engine::note_queue::{NoteEvent, NoteKind};
use chimera_engine::params::{EngineType, ParamSnapshot, PitchParams};
use chimera_engine::preset::Sound;
use chimera_engine::scope::{ScopeWriter, scope_buffer};
use chimera_engine::{MidiChannel, MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;

static DOOR: Mutex<()> = Mutex::new(());

const FNV: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv(h: &mut u64, x: f32) {
    for byte in x.to_bits().to_le_bytes() {
        *h = (*h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
}

/// The DAC's every pair and Part 1's bus, each hashed (FNV-1a over the
/// bits), over `notes` struck, the first re-struck at block 100, a new
/// note at 150, all let go at 250, 600 blocks
/// in all; and the voices sounding after the first block.
fn render(s: &Sound, notes: &[u8]) -> (u64, u64, usize) {
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = s.params.clone();
    *shared.parts[0].mod_state = s.mod_state.clone();
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let (w, _) = Box::leak(Box::new(scope_buffer())).split();
    let mut scope = ScopeWriter::new(w);
    let ev = |n: u8, kind| NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(n).unwrap(),
        kind,
    };
    let on = |v: u8| NoteKind::On(Velocity::new(v).unwrap());
    let (mut out, mut bus, mut after_one) = (FNV, FNV, 0);
    for b in 0..600 {
        match b {
            0 => notes
                .iter()
                .for_each(|&n| inst.handle(ev(n, on(100)), &shared)),
            100 => inst.handle(ev(notes[0], on(60)), &shared),
            150 => inst.handle(ev(notes[0] + 7, on(90)), &shared),
            250 => {
                for &n in notes.iter().chain(&[notes[0] + 7]) {
                    inst.handle(ev(n, NoteKind::Off), &shared);
                }
            }
            _ => {}
        }
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        if b == 0 {
            after_one = inst.sounding();
        }
        for pair in dac.out() {
            for &x in pair {
                fnv(&mut out, x);
            }
        }
        for &x in inst.part_bus(0) {
            fnv(&mut bus, x);
        }
    }
    (out, bus, after_one)
}

fn modal(mode: ResonatorMode, f: impl FnOnce(&mut ModalParams)) -> ParamSnapshot {
    let mut p = Sound::init(EngineType::Modal).params;
    p.modal.mode = mode;
    f(&mut p.modal);
    p
}

/// LFO `i` at `rate` Hz, DEPTH 1, routed to MODAL's `param` by `amount`;
/// `routes` gathers them for `mods`.
fn lfo(
    routes: &mut Vec<(ModSource, ParamAddr, i8)>,
    p: &mut ParamSnapshot,
    i: usize,
    rate: f32,
    param: chimera_engine::block::ParamId,
    amount: i8,
) {
    p.lfos[i].rate = rate;
    p.lfos[i].depth = 1.0;
    let src = [ModSource::Lfo1, ModSource::Lfo2, ModSource::Lfo3][i];
    routes.push((src, ParamAddr::new(BlockRef::Modal, param), amount));
}

/// Each route, its column primed.
fn mods(routes: &[(ModSource, ParamAddr, i8)]) -> ModState {
    let mut reg = ModDestRegistry::new();
    for &(_, a, _) in routes {
        assert!(a.modulatable(), "{a:?}");
        reg.add(a, *b"TEST\0\0\0\0").unwrap();
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for &(s, a, amt) in routes {
        let d = ms.find(a).unwrap();
        ms.set_route(s.index(), d, amt);
    }
    ms
}

/// ENV 3 and LFO 1 on PITCH as well: the pitch moves every block.
fn bend(routes: &mut Vec<(ModSource, ParamAddr, i8)>) {
    let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
    routes.push((ModSource::Env3, pitch, 60));
    routes.push((ModSource::Lfo1, pitch, 20));
}

/// Every case: its name, its Sound and its notes.
fn cases() -> Vec<(String, Sound, Vec<u8>)> {
    use ResonatorMode::*;
    let chord = vec![36u8, 55, 64, 79];
    let high = vec![84u8, 96, 103];
    let none = Sound::init(EngineType::Modal).mod_state;
    let sound = |params: ParamSnapshot, mod_state: ModState| Sound {
        params,
        mod_state,
        ..Sound::init(EngineType::Modal)
    };
    let mut v = Vec::new();
    for mode in [String, Modal, Bowed, Sympathetic] {
        v.push((
            format!("{mode:?} init"),
            sound(modal(mode, |_| {}), none.clone()),
            chord.clone(),
        ));
    }
    for modes in [
        BankModes::M16,
        BankModes::M24,
        BankModes::M32,
        BankModes::M48,
    ] {
        for (s, b) in [(0.0, 0.3), (0.27, 0.9), (0.8, 0.5), (1.0, 1.0)] {
            let p = modal(Modal, |m| {
                (m.modes, m.structure, m.bright, m.pos, m.damp) = (modes, s, b, 0.35, 0.7);
            });
            v.push((
                format!("res {modes:?} s{s} b{b}"),
                sound(p.clone(), none.clone()),
                chord.clone(),
            ));
            v.push((
                format!("res {modes:?} s{s} b{b} high"),
                sound(p, none.clone()),
                high.clone(),
            ));
        }
        // POS, STRUCTURE and DAMP moving: weights glide, modes drop and return.
        let mut p = modal(Modal, |m| (m.modes, m.structure, m.pos) = (modes, 0.6, 0.5));
        let mut r = Vec::new();
        lfo(&mut r, &mut p, 0, 3.0, ModalParams::POS, 100);
        lfo(&mut r, &mut p, 1, 0.7, ModalParams::STRUCTURE, 110);
        lfo(&mut r, &mut p, 2, 1.9, ModalParams::DAMP, 60);
        v.push((
            format!("res {modes:?} moving"),
            sound(p.clone(), mods(&r)),
            chord.clone(),
        ));
        bend(&mut r);
        v.push((
            format!("res {modes:?} moving bent"),
            sound(p, mods(&r)),
            chord.clone(),
        ));
        // Only BRIGHT moving: the filters' other inputs still.
        let mut p = modal(Modal, |m| m.modes = modes);
        let mut r = Vec::new();
        lfo(&mut r, &mut p, 0, 2.3, ModalParams::BRIGHT, 90);
        v.push((
            format!("res {modes:?} bright"),
            sound(p, mods(&r)),
            chord.clone(),
        ));
    }
    for mode in [String, Bowed, Sympathetic] {
        let mut p = modal(mode, |m| (m.structure, m.pos, m.ens_mix) = (0.4, 0.3, 0.5));
        let mut r = Vec::new();
        lfo(&mut r, &mut p, 0, 3.0, ModalParams::POS, 90);
        lfo(&mut r, &mut p, 1, 1.1, ModalParams::BRIGHT, 80);
        lfo(&mut r, &mut p, 2, 0.6, ModalParams::DAMP, 70);
        v.push((
            format!("{mode:?} moving"),
            sound(p.clone(), mods(&r)),
            chord.clone(),
        ));
        bend(&mut r);
        v.push((
            format!("{mode:?} moving bent"),
            sound(p, mods(&r)),
            chord.clone(),
        ));
        let p = modal(mode, |m| {
            (m.damp, m.bright, m.halo, m.couple) = (0.95, 0.9, 0.8, 0.7)
        });
        v.push((
            format!("{mode:?} long"),
            sound(p.clone(), none.clone()),
            chord.clone(),
        ));
        v.push((
            format!("{mode:?} high"),
            sound(p, none.clone()),
            high.clone(),
        ));
    }
    for i in 0..FACTORY_LEN {
        if let Some(s) = factory_sound(i).filter(|s| s.params.engine() == EngineType::Modal) {
            v.push((format!("factory {i}"), s, chord.clone()));
        }
    }
    // DAMP and BRIGHT still while only STRUCTURE (SYMP's halo chord
    // glides) or only the pitch moves: the halo's settings follow the
    // periods alone.
    for mode in [Sympathetic, String, Bowed] {
        let mut p = modal(mode, |m| (m.halo, m.couple) = (0.8, 0.7));
        let mut r = Vec::new();
        lfo(&mut r, &mut p, 1, 0.8, ModalParams::STRUCTURE, 120);
        v.push((
            format!("{mode:?} structure alone"),
            sound(p, mods(&r)),
            chord.clone(),
        ));
        let p = modal(mode, |m| (m.halo, m.couple) = (0.8, 0.7));
        let mut r = Vec::new();
        bend(&mut r);
        let mut p = p;
        p.lfos[0].rate = 4.0;
        p.lfos[0].depth = 1.0;
        v.push((
            format!("{mode:?} pitch alone"),
            sound(p, mods(&r)),
            chord.clone(),
        ));
    }
    v
}

fn hashes() -> Vec<(String, u64, u64)> {
    cases()
        .into_iter()
        .map(|(name, s, notes)| {
            let (out, bus, _) = render(&s, &notes);
            (name, out, bus)
        })
        .collect()
}

/// Taken on main before the move (bc20ae7c; the six "alone" rows on
/// 5ec76841, where the first 60 match too), bank 1's own path: the DAC's
/// hash and Part 1's bus's. The four rows whose note-on PITCH is bent
/// (STRING's and SYMP's "moving bent" and "pitch alone") re-taken when the
/// routed pitch re-plucks at its length from the first pluck's noise (#233,
/// ADR 0310).
const BEFORE: &[(&str, u64, u64)] = &[
    ("String init", 0x288b57ec5f5805d9, 0x99e2ab6099b7c094),
    ("Modal init", 0x2da367cf09d191b1, 0xf77ecdd2c8d624a1),
    ("Bowed init", 0xa7d73ff941b2dfdd, 0x809cd7e0d075ab2f),
    ("Sympathetic init", 0xadb112ae258403a1, 0xdc82dcc167a07337),
    ("res M16 s0 b0.3", 0x33dac3ac0837782d, 0xb03b803004a3ebb2),
    (
        "res M16 s0 b0.3 high",
        0x29ad082e1ec5d81d,
        0x36107cc9c43c099b,
    ),
    ("res M16 s0.27 b0.9", 0x3164201564d5df55, 0x713d288d378c2c4f),
    (
        "res M16 s0.27 b0.9 high",
        0x8744cc2aea0a7d09,
        0xce6b52a6cfa53bee,
    ),
    ("res M16 s0.8 b0.5", 0x72af4908ca3b429d, 0x0e4aec1a05fdfa2f),
    (
        "res M16 s0.8 b0.5 high",
        0xe7a58fb205066439,
        0xbf538972e6060510,
    ),
    ("res M16 s1 b1", 0x08363b53fdb74ab1, 0xfe8a09cde76c1430),
    ("res M16 s1 b1 high", 0x20b330f34c6f1501, 0xa42a02e27f5ae0be),
    ("res M16 moving", 0x98588f09b0d8b2a9, 0x96ac785d74db6738),
    (
        "res M16 moving bent",
        0x54c7f2e77113e699,
        0x2b49dceeb79cb821,
    ),
    ("res M16 bright", 0x16ffe95c6631b0b1, 0xd3f604b6fa59539a),
    ("res M24 s0 b0.3", 0xe618ea0e77214045, 0x03e542cced161b18),
    (
        "res M24 s0 b0.3 high",
        0x43275c84029c9aad,
        0x8b9dc0baea7b4163,
    ),
    ("res M24 s0.27 b0.9", 0xa8bffafffe8513c9, 0x18b7bf6880b83180),
    (
        "res M24 s0.27 b0.9 high",
        0x697fe16d5944e2cd,
        0x33de82dc36484035,
    ),
    ("res M24 s0.8 b0.5", 0x863404d87e70218d, 0x59c39961eec4a5e5),
    (
        "res M24 s0.8 b0.5 high",
        0xe7a58fb205066439,
        0xbf538972e6060510,
    ),
    ("res M24 s1 b1", 0x480068bc48850ab9, 0xc7ee5d1376108488),
    ("res M24 s1 b1 high", 0x20b330f34c6f1501, 0xa42a02e27f5ae0be),
    ("res M24 moving", 0x6fc1e17c7ba65c39, 0xdd133f41f8f274b8),
    (
        "res M24 moving bent",
        0x1abb0bbc1a492d15,
        0x56eeeb189310d000,
    ),
    ("res M24 bright", 0x3881c6e0e7e81d21, 0x2c0f47f04691f8c8),
    ("res M32 s0 b0.3", 0x98b225059788ecfd, 0x22dcd4857dc262b7),
    (
        "res M32 s0 b0.3 high",
        0x3225b7f4eb62ee15,
        0x89993e8e9d14ab80,
    ),
    ("res M32 s0.27 b0.9", 0x6526fcbfbeef0411, 0x79b2ab9936cad2c3),
    (
        "res M32 s0.27 b0.9 high",
        0x697fe16d5944e2cd,
        0x33de82dc36484035,
    ),
    ("res M32 s0.8 b0.5", 0x205221667ad745f1, 0x4a6bfa009f40329e),
    (
        "res M32 s0.8 b0.5 high",
        0xe7a58fb205066439,
        0xbf538972e6060510,
    ),
    ("res M32 s1 b1", 0x833153d3c4bc33e9, 0xc95d5e2a9b763f84),
    ("res M32 s1 b1 high", 0x20b330f34c6f1501, 0xa42a02e27f5ae0be),
    ("res M32 moving", 0xdddb9e61c51b1db1, 0x5ed94dd4a83aa7d8),
    (
        "res M32 moving bent",
        0xf3a4a34b537f3b9d,
        0xf5bc3d2f4565233b,
    ),
    ("res M32 bright", 0xa69db0273b832ea9, 0x4b0601a2b40a8c5c),
    ("res M48 s0 b0.3", 0x7bbfacec94abc01d, 0x58fe7b3fc9918fa8),
    (
        "res M48 s0 b0.3 high",
        0x5c084e4ec126fb39,
        0x6373d7035c489a6a,
    ),
    ("res M48 s0.27 b0.9", 0x2d60fbfdbcea7609, 0xaaf13494e1be1cb1),
    (
        "res M48 s0.27 b0.9 high",
        0x697fe16d5944e2cd,
        0x33de82dc36484035,
    ),
    ("res M48 s0.8 b0.5", 0x13d953e069c0fd61, 0x28a626b988cf8a60),
    (
        "res M48 s0.8 b0.5 high",
        0xe7a58fb205066439,
        0xbf538972e6060510,
    ),
    ("res M48 s1 b1", 0x833153d3c4bc33e9, 0xc95d5e2a9b763f84),
    ("res M48 s1 b1 high", 0x20b330f34c6f1501, 0xa42a02e27f5ae0be),
    ("res M48 moving", 0x6ad584f2edbf4dc9, 0xa40ff5b7cb273268),
    (
        "res M48 moving bent",
        0xc2da951a2ca4a999,
        0x283b8f45dce5fc39,
    ),
    ("res M48 bright", 0xbbd164d49c381c5d, 0x62fec3f3f9b5923f),
    ("String moving", 0xa54ae3b0c526ef61, 0x8226bc7da0cfb23a),
    ("String moving bent", 0xeec5047f60758b21, 0xd4fcb800e81140f8),
    ("String long", 0x017374f8f49cee91, 0x895de93073a43ec5),
    ("String high", 0xecba4a562b2d34b1, 0xba9cea0305c3d695),
    ("Bowed moving", 0xf34597b1e19f9391, 0xfc2b4c31f05551a8),
    ("Bowed moving bent", 0xb61f9a796fcaebb5, 0xfc17b40017515856),
    ("Bowed long", 0x7ca0e06c25bfa221, 0x7b94babcf1d8f860),
    ("Bowed high", 0x8acee5f7ad7f2fa1, 0xb9c0b6895f0443eb),
    ("Sympathetic moving", 0x0485ce1bddbf78dd, 0x6c4e5b4024fff054),
    (
        "Sympathetic moving bent",
        0x3335a67029f5f8b1,
        0xd7e32245e6f4e16b,
    ),
    ("Sympathetic long", 0x7145c08c2d931b79, 0x8c55eee782a3cc68),
    ("Sympathetic high", 0x94e7315881032419, 0xfeee624062258f0c),
    (
        "Sympathetic structure alone",
        0x4d7c883ab402ab99,
        0x3d163575e66ed563,
    ),
    (
        "Sympathetic pitch alone",
        0xd13879b579dfd341,
        0x7b90bbde39540c02,
    ),
    (
        "String structure alone",
        0x0d08ac026b8183dd,
        0x4020407293f1928d,
    ),
    ("String pitch alone", 0x5f28568330046ac9, 0x0d822d6b0773cfb0),
    (
        "Bowed structure alone",
        0xa7d73ff941b2dfdd,
        0x809cd7e0d075ab2f,
    ),
    ("Bowed pitch alone", 0x278c8355d26b7b59, 0x2ed8d89c6e7eb548),
];

#[test]
fn modal_plays_as_before_the_move() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.open_host();
    let h = hashes();
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.close();
    if std::env::var_os("MODAL_DOOR_PRINT").is_some() {
        for (name, out, bus) in &h {
            println!("    ({name:?}, 0x{out:016x}, 0x{bus:016x}),");
        }
    }
    let got: Vec<(&str, u64, u64)> = h.iter().map(|(n, a, b)| (n.as_str(), *a, *b)).collect();
    assert_eq!(got, BEFORE);
}

#[cfg(all(feature = "bank2", feature = "test-support"))]
#[test]
fn modal_with_the_door_closed_is_silent_and_ends() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!chimera_engine::bank2::DOOR_CELL.is_open());
    let silent = {
        let mut h = FNV;
        for _ in 0..600 * chimera_hal::BLOCK_SIZE {
            fnv(&mut h, 0.0);
        }
        h
    };
    for (name, s, notes) in cases().into_iter().take(4) {
        let (_, bus, after_one) = render(&s, &notes);
        assert_eq!((bus, after_one), (silent, 0), "{name}");
    }
}
