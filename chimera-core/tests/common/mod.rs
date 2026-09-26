//! Fixed render harness shared by the refactor lock (`golden_test.rs`) and the
//! sanity gate (`sanity_test.rs`).
//! Spec: docs/superpowers/specs/2026-09-23-engine-refactor-design.md § Testing.
//!
//! Harness: fresh `Voice` per case (RNG seeds are per instance), 48 kHz,
//! note 60 vel 100 on, ON_BLOCKS blocks, note off, OFF_BLOCKS blocks.
#![allow(dead_code)]

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams, FmOpParams, ParamSnapshot};
use chimera_core::preset::{ChainType, Sound};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

pub fn scope_writer() -> ScopeWriter {
    let (w, _unread) = Box::leak(Box::new(scope_buffer())).split();
    ScopeWriter::new(w)
}

pub const SR: u32 = 48_000;
pub const NOTE: u8 = 60;
pub const VEL: u8 = 100;
pub const ON_BLOCKS: usize = 200;
pub const OFF_BLOCKS: usize = 200;
pub const TOTAL_SAMPLES: usize = (ON_BLOCKS + OFF_BLOCKS) * BLOCK_SIZE;
/// LFO rate for every modulated case. At the 1 Hz default the LFO stays
/// positive for the first 0.5 s, so a route to a param already at its max
/// (cutoff 20 kHz, FM op A level 99) would clamp and never be exercised.
pub const MOD_LFO_RATE: f32 = 5.0;
/// Matrix amount (−127..=127) for every modulated case.
pub const MOD_AMOUNT: i8 = 64;

/// Every golden case. `name()` is the key in `golden_test.rs::GOLDENS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    PizzaInit,
    PizzaLfoCutoff,
    FmInit,
    FmLfoCutoff,
    FmLfoOpALevel,
    /// FM init params with the FM init sound's own `ModState`. Since Task 22
    /// dropped the pre-wire, `Sound::init` no longer seeds any destinations,
    /// so this is empty like `FmInit`'s `ModState::new()` — kept as its own
    /// case for golden continuity (the only case Task 22 re-recorded).
    FmInitPatchMod,
    ModalInit,
    ModalLfoCutoff,
    VaInit,
    /// Pizza init; engine switched to Modal at block ON_BLOCKS / 2 (mid-note).
    PizzaToModalSwitch,
}

impl Case {
    pub const ALL: [Case; 10] = [
        Case::PizzaInit,
        Case::PizzaLfoCutoff,
        Case::FmInit,
        Case::FmLfoCutoff,
        Case::FmLfoOpALevel,
        Case::FmInitPatchMod,
        Case::ModalInit,
        Case::ModalLfoCutoff,
        Case::VaInit,
        Case::PizzaToModalSwitch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Case::PizzaInit => "pizza_init",
            Case::PizzaLfoCutoff => "pizza_lfo_cutoff",
            Case::FmInit => "fm_init",
            Case::FmLfoCutoff => "fm_lfo_cutoff",
            Case::FmLfoOpALevel => "fm_lfo_op_a_level",
            Case::FmInitPatchMod => "fm_init_patch_mod",
            Case::ModalInit => "modal_init",
            Case::ModalLfoCutoff => "modal_lfo_cutoff",
            Case::VaInit => "va_init",
            Case::PizzaToModalSwitch => "pizza_to_modal_switch",
        }
    }
}

/// Init params per engine: the chain's `Sound::init` params for the three
/// real engines; defaults with `engine = Va` for Va (it has no chain).
pub fn init_params(engine: EngineType) -> ParamSnapshot {
    match engine {
        EngineType::Pizza => Sound::init(ChainType::PizzaPoly).params,
        EngineType::Fm => Sound::init(ChainType::Fm).params,
        EngineType::Modal => Sound::init(ChainType::Modal).params,
        EngineType::Va => ParamSnapshot::for_engine(EngineType::Va),
    }
}

/// Filter cutoff — the same semantic address on every chain.
pub const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
/// FM operator A level.
pub const OP_A_LEVEL: ParamAddr = ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::LEVEL);

/// One LFO (source 1) route at MOD_AMOUNT to `dest`; env is source 0 so
/// `num_sources >= 2` and the LFO runs.
pub fn lfo_route(dest: ParamAddr) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(dest, *b"GOLDEN\0\0")
        .expect("golden destination must be modulatable");
    let mut ms = ModState::from_registry(&reg, 2);
    ms.set_amount(1, 0, MOD_AMOUNT);
    ms
}

/// Params + ModState for a case (the switch's second half is in `render_case`).
pub fn setup(case: Case) -> (ParamSnapshot, ModState) {
    let with_lfo = |engine: EngineType, dest: ParamAddr| {
        let mut p = init_params(engine);
        p.lfo.rate = MOD_LFO_RATE;
        (p, lfo_route(dest))
    };
    match case {
        Case::PizzaInit => (init_params(EngineType::Pizza), ModState::new()),
        Case::PizzaLfoCutoff => with_lfo(EngineType::Pizza, CUTOFF),
        Case::FmInit => (init_params(EngineType::Fm), ModState::new()),
        Case::FmLfoCutoff => with_lfo(EngineType::Fm, CUTOFF),
        Case::FmLfoOpALevel => with_lfo(EngineType::Fm, OP_A_LEVEL),
        Case::FmInitPatchMod => {
            let sound = Sound::init(ChainType::Fm);
            (sound.params, sound.mod_state)
        }
        Case::ModalInit => (init_params(EngineType::Modal), ModState::new()),
        Case::ModalLfoCutoff => with_lfo(EngineType::Modal, CUTOFF),
        Case::VaInit => (init_params(EngineType::Va), ModState::new()),
        Case::PizzaToModalSwitch => (init_params(EngineType::Pizza), ModState::new()),
    }
}

/// Render the fixed harness for one case. Returns TOTAL_SAMPLES samples.
pub fn render_case(case: Case) -> Vec<f32> {
    let (params, mod_state) = setup(case);
    // Pizza→Modal: from block ON_BLOCKS / 2 the same (default) params with
    // the engine switched — i.e. the Modal init params.
    let switched = init_params(EngineType::Modal);
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(
        MidiNote::new(NOTE).unwrap(),
        Velocity::new(VEL).unwrap(),
        &params,
    );
    let mut out = Vec::with_capacity(TOTAL_SAMPLES);
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..ON_BLOCKS + OFF_BLOCKS {
        if b == ON_BLOCKS {
            voice.note_off();
        }
        let p = if case == Case::PizzaToModalSwitch && b >= ON_BLOCKS / 2 {
            &switched
        } else {
            &params
        };
        voice.render(&mut block, p, &mod_state);
        out.extend_from_slice(&block);
    }
    out
}

/// The same harness through `Instrument` (instrument-core spec § Testing):
/// the case on part 1 (MIDI channel 1), other parts silent, sends 0.
/// Returns part 1's mono bus — the sum of its voices before pan and level.
pub fn render_case_through_instrument(case: Case) -> Vec<f32> {
    let (params, mod_state) = setup(case);
    let mut shared = AudioShared::default();
    shared.parts[0].params = params;
    shared.parts[0].mod_state = mod_state;
    let mut switched = shared.clone();
    switched.parts[0].params = init_params(EngineType::Modal);
    let mut inst = Box::new(Instrument::new(
        chimera_hal::SAMPLE_RATE,
        chimera_core::hw::SampleBudget::for_cpu(chimera_core::hw::CPU_HZ_REV_V),
    ));
    let mut fx = Box::new(FxBus::new());
    let mut dac = [[0.0f32; BLOCK_SIZE * 2]; DAC_PAIRS];
    let event = |kind| NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(NOTE).unwrap(),
        kind,
    };
    let mut out = Vec::with_capacity(TOTAL_SAMPLES);
    let mut scope = scope_writer();
    for b in 0..ON_BLOCKS + OFF_BLOCKS {
        let s = if case == Case::PizzaToModalSwitch && b >= ON_BLOCKS / 2 {
            &switched
        } else {
            &shared
        };
        if b == 0 {
            inst.handle(event(NoteKind::On(Velocity::new(VEL).unwrap())), s);
        }
        if b == ON_BLOCKS {
            inst.handle(event(NoteKind::Off), s);
        }
        inst.render(&mut fx, &mut dac, s, &mut scope);
        out.extend_from_slice(inst.part_bus(0));
    }
    out
}

/// FNV-1a 64 over the little-endian bytes of each sample's `f32::to_bits`.
pub fn fnv1a(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Spot-check indices: start, attack, sustain, around note-off, release, end.
pub const SPOT_IDX: [usize; 8] = [0, 1_000, 5_000, 10_000, 12_799, 12_800, 19_200, 25_599];

pub fn spots(samples: &[f32]) -> [u32; 8] {
    SPOT_IDX.map(|i| samples[i].to_bits())
}

/// Whether an engine produces sound from its default params. Exhaustive on
/// purpose: adding an `EngineType` variant fails to compile here until its
/// expectation is written (spec § Testing "Engines").
pub fn expects_sound(e: EngineType) -> bool {
    match e {
        EngineType::Pizza | EngineType::Fm | EngineType::Modal => true,
        EngineType::Va => false,
    }
}
