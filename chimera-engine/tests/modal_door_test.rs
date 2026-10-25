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
/// hash and Part 1's bus's.
const BEFORE: &[(&str, u64, u64)] = &[
    ("String init", 0x2945be550b3e07b5, 0x552b36dadf56e7ca),
    ("Modal init", 0x8b5eefc284bee8d5, 0xa4ee49337f312386),
    ("Bowed init", 0x7e5effbe95300b81, 0x810027f802c57e01),
    ("Sympathetic init", 0xf05e3c880ee71391, 0x05070edb756816c9),
    ("res M16 s0 b0.3", 0xc4982421ee9fd159, 0x71691cdb267a201d),
    (
        "res M16 s0 b0.3 high",
        0x20cefc4dcf6fbfd9,
        0x2f98166f8f8fa789,
    ),
    ("res M16 s0.27 b0.9", 0x7ef34c7282930efd, 0xd4a29923398b2f82),
    (
        "res M16 s0.27 b0.9 high",
        0x2c0274c1f550ad2d,
        0xc28ff269519e2225,
    ),
    ("res M16 s0.8 b0.5", 0x49ec3cea65a291b9, 0x9f669161c28b5842),
    (
        "res M16 s0.8 b0.5 high",
        0x6d348c714bbcae99,
        0xa548baf38fd7c592,
    ),
    ("res M16 s1 b1", 0x9bdb84c61d82b489, 0x6de658787d32ba99),
    ("res M16 s1 b1 high", 0x90443b80d29dbc91, 0xd19b6df677692af5),
    ("res M16 moving", 0xd82a2b5e368fc665, 0xcff67f2a61b2165c),
    (
        "res M16 moving bent",
        0x598195a046876bd5,
        0x0df529085d2785e1,
    ),
    ("res M16 bright", 0xc0b379166e628581, 0xee6bda2dc43c09ec),
    ("res M24 s0 b0.3", 0x89105691426e2c65, 0xf692af44b143cce0),
    (
        "res M24 s0 b0.3 high",
        0x8c76a795016e0ee9,
        0xa8c7187cbcb9ca6a,
    ),
    ("res M24 s0.27 b0.9", 0x94f057f937c90ce5, 0x0726c52c483d87f0),
    (
        "res M24 s0.27 b0.9 high",
        0xa8b17c2e4794ee49,
        0xaf9bea25e410a3ac,
    ),
    ("res M24 s0.8 b0.5", 0x50a3eab0bb4bdafd, 0xded25f914625673e),
    (
        "res M24 s0.8 b0.5 high",
        0x6d348c714bbcae99,
        0xa548baf38fd7c592,
    ),
    ("res M24 s1 b1", 0x733eb23898ba36e9, 0x4ba963d8a9e2516e),
    ("res M24 s1 b1 high", 0x90443b80d29dbc91, 0xd19b6df677692af5),
    ("res M24 moving", 0xe01a91f4178ddd5d, 0x9a0a57eb4ec03908),
    (
        "res M24 moving bent",
        0x702d62f9ae2fef45,
        0x6f1756ba6f408900,
    ),
    ("res M24 bright", 0x41b244e17f5b64f5, 0x2845743628955cb1),
    ("res M32 s0 b0.3", 0xac7177165d49dedd, 0x9f9fed68a54dee16),
    (
        "res M32 s0 b0.3 high",
        0x99f701ba2ec3bf21,
        0x67a28c7febfe3ded,
    ),
    ("res M32 s0.27 b0.9", 0x548655e64f5f74ad, 0x1007c52b4f855c04),
    (
        "res M32 s0.27 b0.9 high",
        0xa8b17c2e4794ee49,
        0xaf9bea25e410a3ac,
    ),
    ("res M32 s0.8 b0.5", 0x2e4b295fddfd7991, 0x00a1e935e77a8268),
    (
        "res M32 s0.8 b0.5 high",
        0x6d348c714bbcae99,
        0xa548baf38fd7c592,
    ),
    ("res M32 s1 b1", 0x3db8c095d3fe2085, 0xe5a7ef70cb31bdf1),
    ("res M32 s1 b1 high", 0x90443b80d29dbc91, 0xd19b6df677692af5),
    ("res M32 moving", 0x0b87a948f8ee20c5, 0x4350dc36312cc2dc),
    (
        "res M32 moving bent",
        0x2a88576fff6ef071,
        0x53fe06f09e1e9b4f,
    ),
    ("res M32 bright", 0xb123ce3fcecb5951, 0x361e6e80881df209),
    ("res M48 s0 b0.3", 0xea82d1440e4f0419, 0x715e71e55479f46a),
    (
        "res M48 s0 b0.3 high",
        0x488d9785aa374bd9,
        0x69168cb2aa1b2f9e,
    ),
    ("res M48 s0.27 b0.9", 0xe63cbc14041af2d5, 0xcc1a73b8aa7cf1c4),
    (
        "res M48 s0.27 b0.9 high",
        0xa8b17c2e4794ee49,
        0xaf9bea25e410a3ac,
    ),
    ("res M48 s0.8 b0.5", 0xb17ff063cc1f96d9, 0xeec5550debfc589b),
    (
        "res M48 s0.8 b0.5 high",
        0x6d348c714bbcae99,
        0xa548baf38fd7c592,
    ),
    ("res M48 s1 b1", 0x3db8c095d3fe2085, 0xe5a7ef70cb31bdf1),
    ("res M48 s1 b1 high", 0x90443b80d29dbc91, 0xd19b6df677692af5),
    ("res M48 moving", 0x8db5b46164f001c5, 0x29a25bef40a21cd8),
    (
        "res M48 moving bent",
        0x831356d069304259,
        0x131ffc3af61ff36a,
    ),
    ("res M48 bright", 0x53c50acf6c337071, 0x0f2ad3729a1d1d6e),
    ("String moving", 0x402a7255058060bd, 0x5cadee53687eb150),
    ("String moving bent", 0x92694f1b40fb1335, 0xb785afa916dad2e7),
    ("String long", 0x92a98d981b6c5bdd, 0xc696ce8a284a471b),
    ("String high", 0x377459bbf7ffe955, 0xca3b387165bd73d3),
    ("Bowed moving", 0x2097f24de0efa479, 0xb422150fc049bcd8),
    ("Bowed moving bent", 0xedd349f2577e8915, 0xd154a6d192240ba4),
    ("Bowed long", 0x1c5b8918943adf15, 0x9f7d6c2e6efe2096),
    ("Bowed high", 0x7c8ddd3d86b1ad51, 0x3f5cdbc81155db0e),
    ("Sympathetic moving", 0x15e2a492843f5c89, 0x1d3bd9c7aa682ff2),
    (
        "Sympathetic moving bent",
        0x994d88d2f28edf19,
        0x5665ff83c1d2e9b0,
    ),
    ("Sympathetic long", 0x2de8c24df4eecc49, 0x9d6d167e4a0669d0),
    ("Sympathetic high", 0xae5539d74b0b3875, 0xd16f7d128abe9d8c),
    (
        "Sympathetic structure alone",
        0xa5901a6dc550038d,
        0xa21a8ef98cec172b,
    ),
    (
        "Sympathetic pitch alone",
        0xbc4dd0ee0c7e3eb1,
        0xc98d4625076fa0ea,
    ),
    (
        "String structure alone",
        0xe664e574592131ed,
        0xc9a18268a06ed11e,
    ),
    ("String pitch alone", 0x5ae620719e2de6dd, 0x02ca17684b2c3d36),
    (
        "Bowed structure alone",
        0x7e5effbe95300b81,
        0x810027f802c57e01,
    ),
    ("Bowed pitch alone", 0x07246432160f1b29, 0x23627fbce72ef9ac),
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
