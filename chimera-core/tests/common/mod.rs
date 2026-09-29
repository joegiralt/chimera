//! Fixed render harness shared by the refactor lock (`golden_test.rs`) and the
//! sanity gate (`sanity_test.rs`).
//! Spec: docs/superpowers/specs/2026-09-23-engine-refactor-design.md § Testing.
//!
//! Harness: fresh `Voice` per case (RNG seeds are per instance), 48 kHz,
//! note 60 vel 100 on, ON_BLOCKS blocks, note off, OFF_BLOCKS blocks.
#![allow(dead_code)]

pub mod codec_util;
pub mod golden;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

pub fn scope_writer() -> ScopeWriter {
    let (w, _unread) = Box::leak(Box::new(scope_buffer())).split();
    ScopeWriter::new(w)
}

pub const SR: u32 = chimera_hal::SAMPLE_RATE;
pub const NOTE: u8 = 60;
pub const VEL: u8 = 100;
pub const ON_BLOCKS: usize = 200;
pub const OFF_BLOCKS: usize = 200;
pub const TOTAL_SAMPLES: usize = (ON_BLOCKS + OFF_BLOCKS) * BLOCK_SIZE;
/// LFO rate for every modulated case. At the 1 Hz default the LFO stays
/// positive for the first 0.5 s, so a route to a param already at its max
/// (cutoff 20 kHz) would clamp and never be exercised.
pub const MOD_LFO_RATE: f32 = 5.0;
/// Matrix amount (−127..=127) for every modulated case.
pub const MOD_AMOUNT: i8 = 64;

/// Every golden case. `name()` is the key in `golden_test.rs::GOLDENS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    ModalInit,
    ModalLfoCutoff,
    /// The Algo init Sound: operator 1 on W1 at LEVEL 99, T1.
    AlgoInit,
    /// Algo init with the LFO on filter cutoff: the chain lock Pizza's
    /// `pizza_lfo_cutoff` held.
    AlgoLfoCutoff,
    /// `tx_patch` on T1–T8 (0–7).
    AlgoTx(u8),
    /// `morph_patch` with no modulation.
    AlgoMorphStatic,
    /// `morph_patch` with the LFO sweeping MORPH.
    AlgoMorphSweep,
    /// Algo init; engine switched to Modal at block ON_BLOCKS / 2 (mid-note).
    AlgoToModalSwitch,
    /// Factory Sound `i` (0–7) as the bank builds it, its own matrix included.
    Factory(u8),
}

static TX_NAMES: [&str; 8] = [
    "algo_t1", "algo_t2", "algo_t3", "algo_t4", "algo_t5", "algo_t6", "algo_t7", "algo_t8",
];

static FACTORY_NAMES: [&str; 8] = [
    "factory_0",
    "factory_1",
    "factory_2",
    "factory_3",
    "factory_4",
    "factory_5",
    "factory_6",
    "factory_7",
];

impl Case {
    pub const ALL: [Case; 23] = [
        Case::ModalInit,
        Case::ModalLfoCutoff,
        Case::AlgoInit,
        Case::AlgoLfoCutoff,
        Case::AlgoTx(0),
        Case::AlgoTx(1),
        Case::AlgoTx(2),
        Case::AlgoTx(3),
        Case::AlgoTx(4),
        Case::AlgoTx(5),
        Case::AlgoTx(6),
        Case::AlgoTx(7),
        Case::AlgoMorphStatic,
        Case::AlgoMorphSweep,
        Case::AlgoToModalSwitch,
        Case::Factory(0),
        Case::Factory(1),
        Case::Factory(2),
        Case::Factory(3),
        Case::Factory(4),
        Case::Factory(5),
        Case::Factory(6),
        Case::Factory(7),
    ];

    pub fn name(self) -> &'static str {
        match self {
            Case::ModalInit => "modal_init",
            Case::ModalLfoCutoff => "modal_lfo_cutoff",
            Case::AlgoInit => "algo_init",
            Case::AlgoLfoCutoff => "algo_lfo_cutoff",
            Case::AlgoTx(t) => TX_NAMES[t as usize % 8],
            Case::AlgoMorphStatic => "algo_morph_static",
            Case::AlgoMorphSweep => "algo_morph_sweep",
            Case::AlgoToModalSwitch => "algo_to_modal_switch",
            Case::Factory(i) => FACTORY_NAMES[i as usize % 8],
        }
    }
}

/// Init params per engine: the chain's `Sound::init` params.
pub fn init_params(engine: EngineType) -> ParamSnapshot {
    match engine {
        EngineType::Algo => Sound::init(EngineType::Algo).params,
        EngineType::Modal => Sound::init(EngineType::Modal).params,
    }
}

/// Filter cutoff — the same semantic address on every chain.
pub const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

/// MORPH, the destination of the morph sweep.
pub const MORPH: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);

/// Four TX-style operators on T1–T8 (`t` 0–7); operators 5 and 6 silent.
pub fn tx_patch(alg: AlgoId) -> ParamSnapshot {
    let mut p = init_params(EngineType::Algo);
    (p.algo.alg_a, p.algo.alg_b) = (alg.get(), alg.get());
    let ops = [
        (WaveId::W1, 4, 99, 0),
        (WaveId::W1, 8, 70, 0),
        (WaveId::W2, 4, 80, 0),
        (WaveId::W1, 13, 60, 4),
    ];
    for (o, (wave, coarse, level, feedback)) in p.algo.ops.iter_mut().zip(ops) {
        (o.wave, o.coarse, o.level, o.feedback) = (wave.get(), coarse, level, feedback);
        (o.d1r, o.d1l, o.d2r) = (6, 10, 2);
    }
    p
}

/// Six sines at ratios 1–6 between A1 and A17, MORPH halfway.
pub fn morph_patch() -> ParamSnapshot {
    let mut p = init_params(EngineType::Algo);
    (p.algo.alg_a, p.algo.alg_b, p.algo.morph) = (AlgoId::A1.get(), AlgoId::A17.get(), 64);
    for (i, o) in p.algo.ops.iter_mut().enumerate() {
        (o.coarse, o.level) = ([4, 8, 10, 13, 16, 19][i], 80);
    }
    p
}

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

/// Params + ModState for a case.
pub fn setup(case: Case) -> (ParamSnapshot, ModState) {
    let with_lfo = |engine: EngineType, dest: ParamAddr| {
        let mut p = init_params(engine);
        p.lfos[0].rate = MOD_LFO_RATE;
        (p, lfo_route(dest))
    };
    match case {
        Case::ModalInit => (init_params(EngineType::Modal), ModState::new()),
        Case::ModalLfoCutoff => with_lfo(EngineType::Modal, CUTOFF),
        Case::AlgoInit => (init_params(EngineType::Algo), ModState::new()),
        Case::AlgoLfoCutoff => with_lfo(EngineType::Algo, CUTOFF),
        Case::AlgoTx(t) => (tx_patch(AlgoId::clamped(t)), ModState::new()),
        Case::AlgoMorphStatic => (morph_patch(), ModState::new()),
        Case::AlgoMorphSweep => {
            let mut p = morph_patch();
            p.lfos[0].rate = MOD_LFO_RATE;
            (p, lfo_route(MORPH))
        }
        Case::AlgoToModalSwitch => (init_params(EngineType::Algo), ModState::new()),
        Case::Factory(i) => {
            let s = chimera_core::factory::factory_sound(i as usize).expect("factory sound");
            (s.params, s.mod_state)
        }
    }
}

/// Render the fixed harness for one case. Returns TOTAL_SAMPLES samples.
pub fn render_case(case: Case) -> Vec<f32> {
    let (params, mod_state) = setup(case);
    // Algo→Modal: from block ON_BLOCKS / 2 the Modal init params.
    let switched = init_params(EngineType::Modal);
    let switch = (case == Case::AlgoToModalSwitch).then_some(&switched);
    render_with(&params, &mod_state, switch)
}

/// The fixed harness for any Sound's params and matrix.
pub fn render_sound(params: &ParamSnapshot, mods: &ModState) -> Vec<f32> {
    render_with(params, mods, None)
}

/// `switch`: the params from block ON_BLOCKS / 2 on.
fn render_with(
    params: &ParamSnapshot,
    mod_state: &ModState,
    switch: Option<&ParamSnapshot>,
) -> Vec<f32> {
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(
        MidiNote::new(NOTE).unwrap(),
        Velocity::new(VEL).unwrap(),
        params,
    );
    let mut out = Vec::with_capacity(TOTAL_SAMPLES);
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..ON_BLOCKS + OFF_BLOCKS {
        if b == ON_BLOCKS {
            voice.note_off();
        }
        let p = match switch {
            Some(s) if b >= ON_BLOCKS / 2 => s,
            _ => params,
        };
        voice.render(&mut block, p, mod_state);
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
        let s = if case == Case::AlgoToModalSwitch && b >= ON_BLOCKS / 2 {
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
        EngineType::Algo | EngineType::Modal => true,
    }
}

/// Operator 1 alone on the triangle.
pub fn tri() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::TRI);
    p
}

/// Goertzel: the magnitude of `buf` at the bin nearest `hz`.
pub fn goertzel(buf: &[f32], hz: f32, sample_rate: u32) -> f32 {
    let n = buf.len() as f32;
    let k = (hz * n / sample_rate as f32).round();
    let coeff = 2.0 * libm::cosf(2.0 * core::f32::consts::PI * k / n);
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in buf {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    libm::sqrtf((s1 * s1 + s2 * s2 - coeff * s1 * s2).abs()) / n
}

/// Root mean square, summed in f64.
pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
}

#[allow(unused_imports)] // each test binary uses some of these
pub use chimera_core::scope::peak;
