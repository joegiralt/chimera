// The modal bank's filter setup follows Mutable Instruments Rings, and
// `rings.rs` holds its SVF, cosine oscillator and stiffness table (ADR 0032):
//
// Copyright 2014-2015 Emilie Gillet.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// `dispersion.rs` follows Rings' `ap_gain` curve, and `chords.rs` holds its
// chord table (`dsp/part.cc`, Copyright 2015 Emilie Gillet), under the
// same notice.
//
// The Karplus-Strong string (`string.rs`) is the project owner's own code,
// from their Carcosa firmware for the Ambika, relicensed here under MIT
// (ADR 0032).

//! Modal, the physical-modelling engine (ADR 0004): a modal resonator bank
//! (`rings`) and Karplus-Strong strings (`string`) in four models.

mod body;
mod chords;
mod dispersion;
mod ensemble;
mod loop_parts;
mod params;
mod rings;
mod string;

pub use chords::{CHORD_COUNT, CHORD_GLIDE_SAMPLES, CHORDS, chord_of, fold};
pub use params::*;
pub use string::MAX_STRING_DELAY;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use super::xorshift_noise;
use crate::hw::{Cost, SAMPLE_RATE};
use crate::in_place::{in_place_enum, uninit_at};
use crate::sym_alloc::{Lease, SYM_SLOTS, SymAlloc, SymSlot};
use crate::voice_alloc::VoiceIdx;
use chords::{GLIDE_STEP, period_ratios};
use core::f32::consts::TAU;
use ensemble::{Ensemble, rate_hz};
use loop_parts::{DcBlocker, LoopGain, RELEASE_SAMPLES, RELEASE_T60, Release};
use rings::{CosineOsc, Svf, stiffness_from_structure};
use string::{FRESH_CLEAR_BYTES, KsRenderParams, KsString, RING_BYTES, StringVoice, lp_coeff};

/// Bytes the string lines' clears have written on this thread, since the
/// last call: for the tests.
#[cfg(any(test, feature = "test-support"))]
pub fn take_cleared_bytes() -> usize {
    string::cleared::take()
}

pub const MAX_MODES: usize = 48;

// ── Modal Engine (with String and Bowed modes) ──────────────────────

const NUM_SYMPATHETIC: usize = 7;

/// A fresh set's periods, before its first note-on.
const INIT_PERIOD: f32 = 100.0;

/// The most a Sympathetic note-on clears: eight whole rings, a slot and a
/// main string last played at or below G1 (`SymPool::note_on_clear`).
pub const SYM_NOTE_ON_CLEAR_MAX: usize = (1 + NUM_SYMPATHETIC) * RING_BYTES;

/// The resonator bank (`ResonatorMode::Modal`) and the noise burst that
/// strikes it.
struct ModalBank {
    filters: [Svf; MAX_MODES],
    cos_osc: CosineOsc,
    /// MODES, latched at note-on: the bill.
    resolution: usize,
    /// How many of them, from the first, lie below 0.49 of the rate: the
    /// ones rendered. The rest are dropped, as Rings does, and rest silent.
    sounding: usize,
    /// Samples of burst left; the note sounds while any are.
    burst_remaining: usize,
    burst_amp: f32,
    noise_state: u32,
    /// The burst's one-pole lowpass.
    burst_lp: f32,
}

crate::in_place::field_list!(ModalBank => ModalBank {
    filters, cos_osc, resolution, sounding, burst_remaining, burst_amp, noise_state, burst_lp,
});

/// The bowed string and the bow's force on it, 0 once the bow lifts.
struct BowedString {
    string: KsString,
    force: f32,
    /// The force `force` slews to: the note-on's, then 0 at note-off.
    force_to: f32,
    /// Samples pushed since note-on, for `KsString::ring_tap`.
    written: u32,
    /// The lifted bow's ramp.
    release: Release,
}

crate::in_place::field_list!(BowedString => BowedString { string, force, force_to, written, release });

/// One pool slot (exclusive-state spec § 4.2): the seven strings a
/// Sympathetic note's main string sets ringing. The main string is the
/// voice's (`SympatheticVoice`).
pub struct SympatheticSet {
    strings: [KsString; NUM_SYMPATHETIC],
    /// The chord STRUCTURE last stepped to (`chord_of`).
    chord: u8,
    /// Its periods over the main string's, unfolded.
    ratios: [f32; NUM_SYMPATHETIC],
    /// The glide's periods, samples: from the last chord's, to this one's
    /// folded.
    from: [f32; NUM_SYMPATHETIC],
    to: [f32; NUM_SYMPATHETIC],
    /// Samples of glide left.
    glide: u32,
    /// COUPLE's and HALO's gains, latched at note-on.
    coupling: f32,
    level: f32,
    /// Each sympathetic string's last output, not yet stored: it is stored
    /// with the next sample's coupled input (`KsString::tick_coupled`).
    pending: [f32; NUM_SYMPATHETIC],
}

crate::in_place::field_list!(SympatheticSet => SympatheticSet {
    strings, chord, ratios, from, to, glide, coupling, level, pending,
});

/// Sympathetic's sets, one per slot of `SymAlloc`, which lends them to
/// voices (ADR 0054). One per `Instrument`, in D2 beside the voices.
pub struct SymPool {
    alloc: SymAlloc,
    sets: [SympatheticSet; SYM_SLOTS],
}

crate::in_place::field_list!(SymPool => SymPool { alloc, sets });

/// Sympathetic's voice side: the main string, and its halo.
struct SympatheticVoice {
    main: StringVoice,
    halo: Halo,
}

crate::in_place::field_list!(SympatheticVoice => SympatheticVoice { main, halo });

/// A Sympathetic note's seven sympathetic lines, fixed when it starts
/// (the owner's rule, 2026-09-29): a slot's set, or none.
///
/// An enum, not `Option<Lease>`: there is no `insert`, `get_or_insert` or
/// `replace` to attach a lease to a sounding note, and `SympatheticVoice`
/// has no setter. Only an in-place rebuild writes a halo, so a bare note
/// never borrows mid-note, and `Full` holds a `Lease`, which only a slot
/// promised to the voice yields: no lease without a slot.
pub enum Halo {
    /// It rings a pool slot's set.
    Full(Lease),
    /// No slot was free when it started: the main string alone.
    Bare,
}

// Sympathetic is String's voice and a lease (spec § 4.2): within an align.
const _: () =
    assert!(size_of::<SympatheticVoice>() <= size_of::<StringVoice>() + align_of::<StringVoice>());

/// Host sizes of the private model types, for `memory_budget_test`.
#[cfg(any(test, feature = "test-support"))]
pub mod layout {
    use core::mem::{align_of, size_of};

    pub const MODEL_SLOT: usize = size_of::<super::ModelSlot>();
    pub const MODEL_SLOT_ALIGN: usize = align_of::<super::ModelSlot>();
    pub const BOWED: usize = size_of::<super::BowedString>();
    pub const STRING: usize = size_of::<super::StringVoice>();
    pub const SYMPATHETIC_VOICE: usize = size_of::<super::SympatheticVoice>();
}

/// The model a `ModalEngine` is built to play. Sympathetic takes its
/// halo by value.
pub enum Model {
    Bank,
    String,
    Bowed,
    Sympathetic(Halo),
}

impl Model {
    /// `mode`'s model without a lease: Sympathetic rests as String, its
    /// main string without the set (`SlotKind::resting`).
    pub fn resting(mode: ResonatorMode) -> Self {
        match mode {
            ResonatorMode::Modal => Self::Bank,
            ResonatorMode::String | ResonatorMode::Sympathetic => Self::String,
            ResonatorMode::Bowed => Self::Bowed,
        }
    }
}

in_place_enum! {
    /// The one model an engine holds: the variant is the mode.
    enum ModelSlot {
        Bank(ModalBank) => rebuild_bank, init_bank;
        String(StringVoice) => rebuild_string, init_string;
        Bowed(BowedString) => rebuild_bowed, init_bowed;
        Sympathetic(SympatheticVoice) => rebuild_sympathetic, init_sympathetic;
    }
}

impl ModelSlot {
    fn init_in_place(slot: &mut MaybeUninit<Self>, model: Model) -> &mut Self {
        match model {
            // SAFETY: `ModalBank::init_in_place` writes every field.
            Model::Bank => unsafe { Self::init_bank(slot, ModalBank::init_in_place) },
            // SAFETY: `StringVoice::init_in_place` writes every field.
            Model::String => unsafe { Self::init_string(slot, StringVoice::init_in_place) },
            // SAFETY: `BowedString::init_in_place` writes every field.
            Model::Bowed => unsafe { Self::init_bowed(slot, BowedString::init_in_place) },
            // SAFETY: `SympatheticVoice::init_in_place` writes every field.
            Model::Sympathetic(halo) => unsafe {
                Self::init_sympathetic(slot, |s| SympatheticVoice::init_in_place(s, halo))
            },
        }
    }

    /// `model`, fresh, in place: the layout test's. A voice rebuilds its
    /// whole slot instead (`Voice::rebuild`, the one counted path), which
    /// takes a Sympathetic lease out first; this drops it.
    #[cfg(test)]
    fn rebuild(&mut self, model: Model) {
        match model {
            // SAFETY: `ModalBank::init_in_place` writes every field.
            Model::Bank => unsafe { self.rebuild_bank(ModalBank::init_in_place) },
            // SAFETY: `StringVoice::init_in_place` writes every field.
            Model::String => unsafe { self.rebuild_string(StringVoice::init_in_place) },
            // SAFETY: `BowedString::init_in_place` writes every field.
            Model::Bowed => unsafe { self.rebuild_bowed(BowedString::init_in_place) },
            // SAFETY: `SympatheticVoice::init_in_place` writes every field.
            Model::Sympathetic(halo) => unsafe {
                self.rebuild_sympathetic(|s| SympatheticVoice::init_in_place(s, halo))
            },
        }
    }

    fn mode(&self) -> ResonatorMode {
        match self {
            Self::Bank(_) => ResonatorMode::Modal,
            Self::String(_) => ResonatorMode::String,
            Self::Bowed(_) => ResonatorMode::Bowed,
            Self::Sympathetic(_) => ResonatorMode::Sympathetic,
        }
    }
}

pub struct ModalEngine {
    /// The model it plays, and all of that model's state.
    model: ModelSlot,
    /// The note's frequency per sample, before the pitch offset.
    frequency: f32,
    /// The voice's pitch ratio (`set_pitch`, ADR 0042), and the one the
    /// strings are tuned to.
    pitch: f32,
    tuned: f32,
    active: bool,
    silence_counter: u32,
    /// On a string model's output, not in its loop, where its phase would
    /// detune the upper partials.
    dc: DcBlocker,
    macros: Macros,
    /// The note's first block is to come: it snaps `macros` to its
    /// modulated values and shapes the pluck at its POS.
    shape_pending: bool,
}

crate::in_place::field_list!(ModalEngine => ModalEngine {
    model, frequency, pitch, tuned, active, silence_counter, dc, macros, shape_pending,
});

// A `ModalEngine` is its largest model plus the fields every model shares
// (`frequency`, `pitch`, `tuned`, `active`, `silence_counter`,
// `dc`, `macros`, `shape_pending`), never the sum of models. The slot's
// tag takes one align (`in_place_enum!`).
const fn models_are_exclusive() -> bool {
    use core::mem::{align_of, size_of};
    let models = [
        size_of::<ModalBank>(),
        size_of::<StringVoice>(),
        size_of::<BowedString>(),
        size_of::<SympatheticVoice>(),
    ];
    let mut largest = 0;
    let mut i = 0;
    while i < models.len() {
        if models[i] > largest {
            largest = models[i];
        }
        i += 1;
    }
    let align = align_of::<ModalEngine>();
    let shared = size_of::<(f32, f32, f32, bool, u32, DcBlocker, Macros, bool)>();
    size_of::<ModalEngine>()
        <= (largest.next_multiple_of(align) + align + shared).next_multiple_of(align)
}
const _: () = assert!(models_are_exclusive());

impl ModalEngine {
    /// Cycles/sample per model (ADR 0013), at the chain's LP24. Host
    /// estimates, provisional until the bench's MDL rows (Modal 2 step A,
    /// task 12): the model's last benched bill plus what step A added,
    /// counted as the hot path's instructions in the thumbv7em release
    /// build before (6f8fffc) and after, at 1.46 cycles an instruction
    /// (ADR 0052), plus 10 %, rounded up to 10. Per-block work is spread
    /// over the block's 64 samples; a `powf` or a `set_period` is taken as
    /// about 130 instructions.
    ///
    /// STRING, 390 + 70: 30 instructions a sample (99 to 129: the
    /// three-tap low-pass, four dispersion allpasses, the tuning allpass,
    /// the release and the output blocker), 44 cycles; DAMP's two `powf`s
    /// a block, 8; the dispersion's re-split each block STRUCTURE glides,
    /// 8, billed always. BODY and the ensemble bill apart.
    pub const COST_STRING: Cost = Cost(460);
    /// 620 + 100: 60 instructions a sample (143 to 203: the tuning allpass
    /// on the ring, the tap that follows the write (#206), the release, the
    /// bow's lift and the output blocker), 87 cycles.
    pub const COST_BOWED: Cost = Cost(720);
    /// 809 (benched, ADR 0054) + 560, rounded to 1,370: 323 instructions a
    /// sample (419 to 742), 471 cycles, nearly all in the halo, each
    /// string's tick 42 to 87 (the three-tap low-pass's three wrapped reads
    /// and the tuning allpass); ten `powf`s a block for the loop gains, 32.
    pub const COST_SYMPATHETIC: Cost = Cost(1_370);
    /// The resonator bank: this plus `COST_MODE` per mode. Unchanged: its
    /// sample loop is as benched (MDL RES /VOICE 1,865; less the chain's 57,
    /// 1,808 at 32 modes against 1,900 billed), and the macros' easing is a few operations a block.
    /// Dropping modes past Nyquist can only save.
    pub const COST_BANK: Cost = Cost(460);
    pub const COST_MODE: Cost = Cost(45);

    /// More on STRING and SYMP with BODY above 0: three band-passes, 80
    /// instructions a sample on SYMP's mix (65 on STRING's).
    pub const BODY: Cost = Cost(130);
    /// More on STRING and SYMP with ENS MIX above 0: two interpolated heads
    /// and the LFO's rotation, 94 instructions a sample.
    pub const ENSEMBLE: Cost = Cost(160);

    /// `p`'s model, as the voice plays it from its next note-on.
    pub fn cost(p: &ModalParams) -> Cost {
        let extras = || Self::extras((p.body > 0.0, p.ens_mix > 0.0));
        match p.mode {
            ResonatorMode::String => Self::COST_STRING + extras(),
            ResonatorMode::Bowed => Self::COST_BOWED,
            ResonatorMode::Sympathetic => Self::COST_SYMPATHETIC + extras(),
            ResonatorMode::Modal => Self::bank_cost(p.modes.count()),
        }
    }

    fn bank_cost(modes: usize) -> Cost {
        Cost(Self::COST_BANK.0 + Self::COST_MODE.0 * modes as u32)
    }

    /// BODY's and the ensemble's bill, each if it runs.
    fn extras((body, ens): (bool, bool)) -> Cost {
        let on = |runs, c: Cost| if runs { c } else { Cost::ZERO };
        on(body, Self::BODY) + on(ens, Self::ENSEMBLE)
    }

    /// What the sounding note costs: its note-on's model, MODES, BODY and
    /// ensemble.
    pub fn playing_cost(&self) -> Option<Cost> {
        self.active.then(|| match &self.model {
            ModelSlot::Bank(b) => Self::bank_cost(b.resolution),
            ModelSlot::String(v) => Self::COST_STRING + Self::extras(v.runs()),
            ModelSlot::Bowed(_) => Self::COST_BOWED,
            ModelSlot::Sympathetic(v) => Self::COST_SYMPATHETIC + Self::extras(v.main.runs()),
        })
    }

    /// More with a route into PITCH or FINE: the strings re-split every
    /// block. A host estimate, as the `COST_*`: SYMP's eight `set_period`s
    /// a block and the halo's octave fold, 18 instructions a sample (STRING's
    /// re-split with its dispersion, about 6). It was 12 before fractional
    /// tuning (ADR 0042).
    pub const PITCH: Cost = Cost(30);

    /// More on SYMP with a route into STRUCTURE, which can keep the halo
    /// gliding: seven `set_period`s every `GLIDE_STEP` (28 a block) and
    /// the glide's lerp, about 60 instructions a sample, and seven
    /// `exp2f`s a chord step. A host estimate, as `PITCH`, until the
    /// bench's SYM LFO row reads it (task 12).
    pub const CHORD: Cost = Cost(100);

    /// An idle engine set to play `mode`, by value, through the stack:
    /// tests only. Sympathetic borrows a slot of `pool` for voice 0.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_in(pool: &mut SymPool, mode: ResonatorMode) -> Self {
        let model = match mode {
            ResonatorMode::Sympathetic => {
                let v0 = crate::voice_alloc::VoiceIdx::ALL[0];
                assert!(pool.alloc.place(v0), "a free slot");
                Model::Sympathetic(Halo::Full(pool.alloc.lend(v0).expect("promised")))
            }
            other => Model::resting(other),
        };
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { crate::in_place::by_value(|slot| Self::init_in_place(slot, model)) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, model: Model) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the model is built in place,
        // every other field is written once by value, before
        // `assume_init_mut`.
        unsafe {
            ModelSlot::init_in_place(uninit_at(addr_of_mut!((*p).model)), model);
            addr_of_mut!((*p).frequency).write(220.0 / 48000.0);
            addr_of_mut!((*p).pitch).write(1.0);
            addr_of_mut!((*p).tuned).write(1.0);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).silence_counter).write(0);
            addr_of_mut!((*p).dc).write(DcBlocker::new(SAMPLE_RATE));
            addr_of_mut!((*p).macros).write(Macros::of(&ModalParams::default()));
            addr_of_mut!((*p).shape_pending).write(false);
            slot.assume_init_mut()
        }
    }

    /// The model this engine plays: its last note-on's, else the one it
    /// was built for.
    pub fn mode(&self) -> ResonatorMode {
        self.model.mode()
    }

    /// The voice's pitch ratio, for the next `note_on` or `render`: the
    /// resonators' and strings' frequency, per block.
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
    }

    /// `f` under the pitch ratio; untouched at 1 (the goldens).
    fn pitched(&self, f: f32) -> f32 {
        if self.pitch == 1.0 { f } else { f * self.pitch }
    }

    /// The lease a Sympathetic engine holds: for `EngineSlot::rebuild`,
    /// which moves it out before the payload is overwritten.
    pub(crate) fn lease_mut(&mut self) -> Option<&mut Lease> {
        match &mut self.model {
            ModelSlot::Sympathetic(m) => match &mut m.halo {
                Halo::Full(l) => Some(l),
                Halo::Bare => None,
            },
            ModelSlot::Bank(_) | ModelSlot::String(_) | ModelSlot::Bowed(_) => None,
        }
    }

    /// Sympathetic without a halo.
    pub fn is_bare(&self) -> bool {
        matches!(&self.model, ModelSlot::Sympathetic(m) if matches!(m.halo, Halo::Bare))
    }

    /// The halo's set in `pool`, if any: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    fn halo_in<'a>(&self, pool: &'a SymPool) -> Option<&'a SympatheticSet> {
        match &self.model {
            ModelSlot::Sympathetic(m) => match &m.halo {
                Halo::Full(l) => Some(&pool.sets[l.slot().index()]),
                Halo::Bare => None,
            },
            _ => None,
        }
    }

    /// The halo strings' periods, samples, as last split: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn halo_periods(&self, pool: &SymPool) -> Option<[f32; NUM_SYMPATHETIC]> {
        self.halo_in(pool).map(SympatheticSet::periods)
    }

    /// The halo strings' lines, `(delay, ring_len)`: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn halo_lines(&self, pool: &SymPool) -> Option<[(usize, usize); NUM_SYMPATHETIC]> {
        self.halo_in(pool)
            .map(|s| s.strings.each_ref().map(|k| (k.delay(), k.ring_len())))
    }

    /// `params.mode` must be the model this engine holds: a voice rebuilds
    /// its slot into another (`Voice::rebuild`, ADR 0051). Sympathetic
    /// reads its set from `pool`.
    pub fn note_on(
        &mut self,
        note: u8,
        velocity: u8,
        params: &ModalParams,
        sample_rate: u32,
        pool: &mut SymPool,
    ) {
        debug_assert_eq!(params.mode, self.mode());
        let vel = velocity as f32 / 127.0;
        let freq = note_to_freq(note);
        self.frequency = freq / sample_rate as f32;
        let freq = self.pitched(freq);
        self.tuned = self.pitch;
        let bank_freq = self.pitched(self.frequency);
        self.macros = Macros::of(params);
        let m = &self.macros;

        match &mut self.model {
            ModelSlot::Bank(bank) => {
                bank.resolution = params.modes.count();
                bank.compute_filters(m, bank_freq);
                bank.cos_osc.init(m.pos);
                let burst_ms = 2.0 + params.excite * 4.0;
                bank.burst_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
                bank.burst_amp = vel * params.excite;
                bank.burst_lp = 0.0;
            }
            ModelSlot::String(v) => {
                v.pluck(
                    (freq, sample_rate),
                    Some(m.structure),
                    vel * params.excite,
                    (
                        params.body,
                        ensemble(params, rate_hz(params.ens_rate), sample_rate),
                    ),
                );
            }
            ModelSlot::Bowed(b) => {
                b.string.clear();
                b.string.tune(freq, sample_rate);
                b.force = vel * BOW_FORCE;
                b.force_to = b.force;
                b.written = 0;
                b.release = Release::HELD;
            }
            ModelSlot::Sympathetic(v) => {
                // STRUCTURE tunes the halo only: the main string is not stiff.
                let ens = ensemble(params, rate_hz(SYMP_ENS_RATE), sample_rate);
                v.main.pluck(
                    (freq, sample_rate),
                    None,
                    vel * params.excite,
                    (params.body, ens),
                );
                if let Some(set) = pool.halo(&v.halo) {
                    set.note_on(sample_rate as f32 / freq, chord_of(m.structure), params);
                }
            }
        }

        self.shape_pending = true;
        self.dc.reset();
        self.active = true;
        self.silence_counter = 0;
    }

    /// The strings follow a changed pitch ratio (a divide per string,
    /// `ModalEngine::PITCH`), STRING's dispersion a moved STRUCTURE
    /// (gliding, `StringVoice::tune`), and SYMP's halo `chord`, the
    /// un-eased STRUCTURE's (gliding, `SympatheticSet::retune`; a note's
    /// first block snaps it).
    fn retune(
        &mut self,
        sample_rate: u32,
        pool: &mut SymPool,
        (moved, chord, snap): (bool, usize, bool),
    ) {
        let pitched = self.pitch != self.tuned;
        self.tuned = self.pitch;
        let freq = self.pitched(self.frequency * sample_rate as f32);
        let structure = self.macros.structure;
        match &mut self.model {
            ModelSlot::Bank(_) => {}
            ModelSlot::String(v) if pitched || moved || v.gliding(structure) => {
                v.tune(freq, sample_rate, structure)
            }
            ModelSlot::String(_) => {}
            ModelSlot::Bowed(b) if pitched => b.string.tune(freq, sample_rate),
            ModelSlot::Bowed(_) => {}
            ModelSlot::Sympathetic(m) => {
                if pitched {
                    m.main.tune(freq, sample_rate, 0.0);
                }
                if let Some(set) = pool.halo(&m.halo) {
                    set.retune(sample_rate as f32 / freq, chord, (pitched, snap));
                }
            }
        }
    }

    /// The string's loop gain ramps to a `RELEASE_T60` ring over
    /// `RELEASE_SAMPLES`, and Bowed's as the bow lifts. Nothing scales a
    /// buffer (#51). The bank and SYMP's halo ring on: sympathetic strings
    /// ring until touched (ADR 0054).
    pub fn note_off(&mut self, _pool: &mut SymPool) {
        // As `render` rings them: f0 in Hz, T60 from the eased DAMP.
        let f0 = self.pitched(self.frequency) * SAMPLE_RATE as f32;
        let to = LoopGain::from_t60(RELEASE_T60, f0);
        let held = LoopGain::from_t60(t60(self.macros.damp), f0);
        match &mut self.model {
            ModelSlot::String(v) => v.release(held, to),
            ModelSlot::Bowed(b) => {
                b.force_to = 0.0;
                b.release.start(BOW_GAIN, to);
            }
            ModelSlot::Bank(_) => {}
            ModelSlot::Sympathetic(m) => m.main.release(held, to),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        sample_rate: u32,
        pool: &mut SymPool,
    ) {
        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        let mut max_level = 0.0_f32;

        // A note's first block takes its modulated macros whole: nothing
        // sounds yet. Then they ease.
        let (was, to) = (self.macros, Macros::of(params));
        let first = core::mem::take(&mut self.shape_pending);
        if first {
            self.macros = to;
            // Before any retune: the pluck is the note-on's length.
            match &mut self.model {
                ModelSlot::String(s) => s.shape(to.pos),
                ModelSlot::Sympathetic(v) => v.main.shape(to.pos),
                ModelSlot::Bank(_) | ModelSlot::Bowed(_) => {}
            }
        } else {
            self.macros.ease(&to);
        }
        let m = self.macros;

        let bank_freq = self.pitched(self.frequency);
        // The strings' f0 in Hz, for their loop gains.
        let f0 = bank_freq * sample_rate as f32;
        // The chord steps on the un-eased STRUCTURE: its glide is the easing.
        let chord = chord_of(to.structure);
        self.retune(
            sample_rate,
            pool,
            (m.structure != was.structure, chord, first),
        );

        // Whether the model is still exciting itself: silent or not, the
        // note sounds on.
        let exciting = match &mut self.model {
            ModelSlot::Bank(bank) => {
                // Recompute filters every block (Rings does this — allows live parameter changes)
                bank.compute_filters(&m, bank_freq);
                bank.cos_osc.init(m.pos);
                render_modal(bank, output, &mut max_level);
                bank.burst_remaining > 0
            }
            ModelSlot::String(v) => {
                v.set_ensemble();
                render_string(v, output, &m, f0);
                false
            }
            ModelSlot::Bowed(b) => {
                render_bowed(b, output);
                // Never freed while bowed, however low its note (#206).
                b.force > 0.0
            }
            ModelSlot::Sympathetic(v) => {
                let v = &mut **v;
                v.main.set_ensemble();
                let set = pool.halo(&v.halo);
                render_sympathetic(&mut v.main, set, output, &m, (f0, sample_rate));
                false
            }
        };
        // A string's silence is judged on what is heard: after the blocker.
        if !matches!(self.model, ModelSlot::Bank(_)) {
            for s in output.iter_mut() {
                *s = self.dc.process(*s);
                max_level = max_level.max(libm::fabsf(*s));
            }
        }

        if max_level < 0.001 && !exciting {
            self.silence_counter += 1;
            if self.silence_counter > 10 {
                self.active = false;
            }
        } else {
            self.silence_counter = 0;
        }
    }
}

impl ModalBank {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every field (the largest,
        // `filters`, is 960 B) is written once by value, before
        // `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).filters).write(core::array::from_fn(|_| Svf::new()));
            addr_of_mut!((*p).cos_osc).write(CosineOsc::new());
            addr_of_mut!((*p).resolution).write(0);
            addr_of_mut!((*p).sounding).write(0);
            addr_of_mut!((*p).burst_remaining).write(0);
            addr_of_mut!((*p).burst_amp).write(0.0);
            addr_of_mut!((*p).noise_state).write(0x1234_5678);
            addr_of_mut!((*p).burst_lp).write(0.0);
            slot.assume_init_mut()
        }
    }

    /// Configure filters — called every render block (not just note_on).
    /// Matches Rings' ComputeFilters().
    fn compute_filters(&mut self, m: &Macros, frequency: f32) {
        let num = self.resolution;

        // Q from DAMP (Rings-style range).
        // At partial_freq=0.003 (130Hz): mode_q = 1 + 0.003 * q
        //   damp=0:   q=500,    mode_q=2.5  (short ping)
        //   damp=0.5: q=50000,  mode_q=151  (nice ring)
        //   damp=1:   q=500000, mode_q=1501 (long sustain)
        let mut q = 500.0 * libm::powf(10.0, m.damp * 3.0); // 500..500,000

        let structure = m.structure;
        let mut stiffness = stiffness_from_structure(structure);

        // Brightness → q_loss per mode (Rings formula)
        let bright_atten = {
            let x = 1.0 - structure;
            let x2 = x * x;
            x2 * x2 * x2 * x2
        };
        let brightness = m.bright * (1.0 - 0.2 * bright_atten);
        let mut q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;
        let q_loss_damping_rate = structure * (2.0 - structure) * 0.1;

        let mut harmonic = frequency;
        let mut stretch_factor = 1.0_f32;

        let mut sounding = num;
        for (i, filter) in self.filters.iter_mut().take(num).enumerate() {
            let partial_freq = harmonic * stretch_factor;
            if partial_freq >= 0.49 {
                sounding = i;
                break;
            }

            // Per-mode Q (Rings: 1.0 + partial_freq * q)
            let mode_q = 1.0 + partial_freq * q;
            filter.set(partial_freq, mode_q);

            // Accumulate stiffness with decay for negative values
            stretch_factor += stiffness;
            if stretch_factor < 0.1 {
                stretch_factor = 0.1;
            } // never go negative
            if stiffness < 0.0 {
                stiffness *= 0.93;
            } // decay negative stiffness

            // Q decays across modes (Rings: q *= q_loss)
            q *= q_loss;
            q_loss += q_loss_damping_rate * (1.0 - q_loss);

            harmonic += frequency;
        }
        // Dropped modes go silent: one that returns as STRUCTURE falls restarts from rest.
        for f in &mut self.filters[sounding..self.sounding.max(sounding)] {
            f.reset();
        }
        self.sounding = sounding;
    }
}

impl BowedString {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the string is built in place
        // and the rest written by value, before `assume_init_mut`.
        unsafe {
            KsString::init_in_place(uninit_at(addr_of_mut!((*p).string)));
            addr_of_mut!((*p).force).write(0.0);
            addr_of_mut!((*p).force_to).write(0.0);
            addr_of_mut!((*p).written).write(0);
            addr_of_mut!((*p).release).write(Release::HELD);
            slot.assume_init_mut()
        }
    }
}

impl SympatheticVoice {
    fn init_in_place(slot: &mut MaybeUninit<Self>, halo: Halo) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the main string is built in
        // place and the halo (two bytes) written by value, before
        // `assume_init_mut`.
        unsafe {
            StringVoice::init_in_place(uninit_at(addr_of_mut!((*p).main)));
            addr_of_mut!((*p).halo).write(halo);
            slot.assume_init_mut()
        }
    }
}

impl SymPool {
    /// Built once, where the `Instrument` lives: the sets in place, the
    /// allocator by value.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; each set is built in place and
        // the allocator (a few bytes) written by value, before
        // `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).alloc).write(SymAlloc::new());
            let sets = addr_of_mut!((*p).sets).cast::<SympatheticSet>();
            for i in 0..SYM_SLOTS {
                SympatheticSet::init_in_place(uninit_at(sets.add(i)));
            }
            slot.assume_init_mut()
        }
    }

    /// A pool on the heap, built in place: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn boxed() -> alloc::boxed::Box<Self> {
        let mut raw = alloc::boxed::Box::<Self>::new_uninit();
        Self::init_in_place(&mut raw);
        // SAFETY: `init_in_place` built a valid pool in the box.
        unsafe { raw.assume_init() }
    }

    pub fn alloc(&self) -> &SymAlloc {
        &self.alloc
    }

    /// A Sympathetic note starts on `voice`: `SymAlloc::place`.
    pub fn place(&mut self, voice: VoiceIdx) -> bool {
        self.alloc.place(voice)
    }

    pub(crate) fn alloc_mut(&mut self) -> &mut SymAlloc {
        &mut self.alloc
    }

    /// The bytes a Sympathetic note-on on `voice`, whose engine is
    /// `engine`, clears (spec § 4.8): its main string and its set, each
    /// line up to its dirty extent. An engine already Sympathetic keeps
    /// its halo, unless it is bare and a slot is now promised to `voice`;
    /// any other is rebuilt, its main string fresh, and lent that promise
    /// if any. At most `SYM_NOTE_ON_CLEAR_MAX`.
    pub fn note_on_clear(&self, engine: Option<&ModalEngine>, voice: VoiceIdx) -> usize {
        let set = |s: SymSlot| -> usize {
            self.sets[s.index()]
                .strings
                .iter()
                .map(KsString::clear_bytes)
                .sum()
        };
        let promise = self.alloc.promise_of(voice);
        match engine.map(|e| &e.model) {
            Some(ModelSlot::Sympathetic(m)) => match &m.halo {
                Halo::Full(l) => m.main.string.clear_bytes() + set(l.slot()),
                Halo::Bare if promise.is_none() => m.main.string.clear_bytes(),
                Halo::Bare => FRESH_CLEAR_BYTES + promise.map_or(0, set),
            },
            _ => FRESH_CLEAR_BYTES + promise.map_or(0, set),
        }
    }

    /// The set `halo` rings, if any. It borrows the pool and the lease, so
    /// no other set is reachable meanwhile and the lease can't go back.
    fn halo(&mut self, halo: &Halo) -> Option<&mut SympatheticSet> {
        match halo {
            Halo::Full(lease) => Some(&mut self.sets[lease.slot().index()]),
            Halo::Bare => None,
        }
    }
}

impl SympatheticSet {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the seven strings are built
        // in place and every other field is written by value, before
        // `assume_init_mut`.
        unsafe {
            let sym = addr_of_mut!((*p).strings).cast::<KsString>();
            for i in 0..NUM_SYMPATHETIC {
                KsString::init_in_place(uninit_at(sym.add(i)));
            }
            addr_of_mut!((*p).chord).write(0);
            addr_of_mut!((*p).ratios).write([1.0; NUM_SYMPATHETIC]);
            addr_of_mut!((*p).from).write([INIT_PERIOD; NUM_SYMPATHETIC]);
            addr_of_mut!((*p).to).write([INIT_PERIOD; NUM_SYMPATHETIC]);
            addr_of_mut!((*p).glide).write(0);
            addr_of_mut!((*p).coupling).write(0.0);
            addr_of_mut!((*p).level).write(0.0);
            addr_of_mut!((*p).pending).write([0.0; NUM_SYMPATHETIC]);
            slot.assume_init_mut()
        }
    }

    /// A note-on on a main string of `period` samples: the strings
    /// cleared, each ring sized for the longest folded period any chord
    /// gives it, so no glide grows one mid-note, then tuned to `chord`.
    fn note_on(&mut self, period: f32, chord: usize, params: &ModalParams) {
        let ratios: [[f32; NUM_SYMPATHETIC]; CHORD_COUNT] = core::array::from_fn(period_ratios);
        for (i, sym) in self.strings.iter_mut().enumerate() {
            // Sympathetic strings start silent — energy comes from main. A
            // handed-over slot carries nothing of its last note (spec
            // § 4.8). Cleared before the retune, so the clear is
            // `SymPool::note_on_clear`'s.
            sym.clear();
            let longest = ratios
                .iter()
                .map(|r| fold(period * r[i]))
                .fold(0.0, f32::max);
            sym.set_period(longest, 0.0, TAU / longest);
        }
        self.chord = chord as u8;
        self.ratios = ratios[chord];
        self.to = self.ratios.map(|r| fold(period * r));
        self.from = self.to;
        self.glide = 0;
        self.split();
        self.coupling = 0.1 * params.couple;
        self.level = 0.6 * params.halo;
        self.pending = [0.0; NUM_SYMPATHETIC];
    }

    /// Per block, on a main string of `period` samples: a new `chord`
    /// starts a glide from where the strings are, which `glide_step`
    /// walks, or on a note's first block (`snap`) takes it whole; a pitch
    /// change moves its target.
    fn retune(&mut self, period: f32, chord: usize, (pitched, snap): (bool, bool)) {
        let stepped = chord != self.chord as usize;
        if !stepped && !pitched {
            return;
        }
        let now = self.periods();
        if stepped {
            self.chord = chord as u8;
            self.ratios = period_ratios(chord);
            self.glide = if snap { 0 } else { CHORD_GLIDE_SAMPLES };
        }
        self.to = self.ratios.map(|r| fold(period * r));
        if self.glide == 0 {
            self.split();
            return;
        }
        // Rebased so `periods()` is still `now`: nothing jumps.
        let g = self.glide as f32 / CHORD_GLIDE_SAMPLES as f32;
        self.from = core::array::from_fn(|i| self.to[i] + (now[i] - self.to[i]) / g);
    }

    /// Every `GLIDE_STEP` samples: a gliding set's next step.
    #[inline]
    fn glide_step(&mut self) {
        if self.glide > 0 {
            self.glide = self.glide.saturating_sub(GLIDE_STEP as u32);
            self.split();
        }
    }

    /// The strings' periods now, samples: `to` past the glide.
    fn periods(&self) -> [f32; NUM_SYMPATHETIC] {
        if self.glide == 0 {
            return self.to;
        }
        let t = 1.0 - self.glide as f32 / CHORD_GLIDE_SAMPLES as f32;
        core::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * t)
    }

    /// Each string's line and allpass at `periods`: no dispersion.
    fn split(&mut self) {
        let periods = self.periods();
        for (sym, p) in self.strings.iter_mut().zip(periods) {
            sym.set_period(p, 0.0, TAU / p);
        }
    }
}

fn render_modal(bank: &mut ModalBank, output: &mut [f32; BLOCK_SIZE], max_level: &mut f32) {
    let num = bank.sounding;
    for s in output.iter_mut() {
        let excite = if bank.burst_remaining > 0 {
            bank.burst_remaining -= 1;
            let env = (bank.burst_remaining as f32 / 200.0).min(1.0);
            let raw = xorshift_noise(&mut bank.noise_state) * bank.burst_amp * env;
            bank.burst_lp += 0.4 * (raw - bank.burst_lp);
            bank.burst_lp
        } else {
            0.0
        };

        // Rings scales external audio input by 0.125. Our internal exciter
        // is already at the right level — no additional scaling needed.
        let input = excite;

        let mut odd = 0.0_f32;
        let mut even = 0.0_f32;
        bank.cos_osc.start();

        let mut i = 0;
        while i + 1 < num {
            odd += bank.cos_osc.next() * bank.filters[i].process_bp(input);
            even += bank.cos_osc.next() * bank.filters[i + 1].process_bp(input);
            i += 2;
        }
        if i < num {
            odd += bank.cos_osc.next() * bank.filters[i].process_bp(input);
        }

        // Sum to mono, scale up, soft-limit
        *s = libm::tanhf(odd + even) * 2.0;
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

fn render_string(v: &mut StringVoice, output: &mut [f32; BLOCK_SIZE], m: &Macros, f0: f32) {
    v.render(&main_string(m, f0), output);
    v.colour(output);
}

/// The block's STRING or SYMP main string at `f0` Hz: its held gain,
/// which a release caps.
fn main_string(m: &Macros, f0: f32) -> KsRenderParams {
    KsRenderParams {
        lp: lp_coeff(m.bright),
        gain: LoopGain::from_t60(t60(m.damp), f0),
    }
}

/// A note's ENS MIX and ensemble at `hz`: off at MIX 0.
fn ensemble(params: &ModalParams, hz: f32, sample_rate: u32) -> (f32, Ensemble) {
    if params.ens_mix <= 0.0 {
        return (0.0, Ensemble::default());
    }
    (
        params.ens_mix,
        Ensemble::new(params.ens_depth, hz, sample_rate),
    )
}

/// Bowed's hidden bow, until step B's exciter.
const BOW_VELOCITY: f32 = 0.5;
const BOW_FORCE: f32 = 0.5;
/// The bowed loop's gain per pass, until note-off ramps it down.
const BOW_GAIN: LoopGain = LoopGain::TOP;
/// The bow lifts a full force in `RELEASE_SAMPLES`, a softer one sooner.
const BOW_LIFT: f32 = BOW_FORCE / RELEASE_SAMPLES as f32;

/// Each halo string's T60 over the main string's.
const HALO_T60: f32 = 2.0;

/// SYMP's main string's ensemble rate: ENS RATE is STRING's alone.
const SYMP_ENS_RATE: f32 = 0.3;

fn render_bowed(b: &mut BowedString, output: &mut [f32; BLOCK_SIZE]) {
    for s in output.iter_mut() {
        if b.force > b.force_to {
            b.force = (b.force - BOW_LIFT).max(b.force_to);
        }
        let exciter_amp = b.force;
        let bow_vel = if exciter_amp > 0.001 {
            BOW_VELOCITY * 0.3
        } else {
            0.0
        };
        let bow_force = exciter_amp * 4.0;
        let gain = b.release.gain(BOW_GAIN);

        // Read from delay line
        let string_vel = b.string.ring_tap(b.written);

        // Bow friction: stick-slip model.
        // When |delta_v| is small, bow sticks (high friction → energy in).
        // When |delta_v| is large, bow slips (low friction → string rings free).
        let delta_v = bow_vel - string_vel;
        let friction = bow_force * libm::tanhf(delta_v * 8.0);

        let feedback = string_vel * gain.get() + friction * 0.4;

        // Soft-limit to prevent blowup
        let clamped = libm::tanhf(feedback);

        b.string.ring_push(clamped);
        b.written = b.written.saturating_add(1);

        *s = string_vel;
    }
}

/// The main string and, with a halo, the seven it sets ringing; bare, the
/// main string alone.
fn render_sympathetic(
    main: &mut StringVoice,
    set: Option<&mut SympatheticSet>,
    output: &mut [f32; BLOCK_SIZE],
    m: &Macros,
    (f0, sample_rate): (f32, u32),
) {
    main.render(&main_string(m, f0), output);
    if let Some(set) = set {
        // Each halo string rings twice the main one's T60, no darker.
        let lp = lp_coeff(halo_bright(m.bright));
        let halo_t60 = HALO_T60 * t60(m.damp);
        let halo = set.periods().map(|p| KsRenderParams {
            lp,
            gain: LoopGain::from_t60(halo_t60, sample_rate as f32 / p),
        });
        let (coupling, level) = (set.coupling, set.level);
        // The main string drives each halo string at its write position.
        let mut input = [0.0_f32; BLOCK_SIZE];
        for (i, x) in input.iter_mut().zip(output.iter()) {
            *i = x * coupling;
        }
        let mut sum = [0.0_f32; BLOCK_SIZE];
        // String by string, a glide step at a time; a whole block unless gliding.
        let step = if set.glide > 0 {
            GLIDE_STEP
        } else {
            BLOCK_SIZE
        };
        for (input, sum) in input.chunks(step).zip(sum.chunks_mut(step)) {
            set.glide_step();
            let halo = set.strings.iter_mut().zip(&mut set.pending).zip(&halo);
            for ((sym, pending), p) in halo {
                sym.run_coupled(p, input, pending, sum);
            }
        }
        for (s, h) in output.iter_mut().zip(&sum) {
            *s += h * level;
        }
    }
    main.colour(output);
    for s in output.iter_mut() {
        *s = libm::tanhf(*s);
    }
}

use super::note_to_freq;

/// The halo's BRIGHT: its damping 0.7× the main string's, as today.
fn halo_bright(bright: f32) -> f32 {
    1.0 - 0.7 * (1.0 - bright)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::boxed::Box;
    use std::vec::Vec;

    use super::*;
    use crate::voice_alloc::VoiceIdx;

    /// In `ModelSlot`'s declaration order: the tag.
    const MODES: [ResonatorMode; 4] = [
        ResonatorMode::Modal,
        ResonatorMode::String,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ];

    fn payload_addr(slot: &ModelSlot) -> *const u8 {
        match slot {
            ModelSlot::Bank(b) => core::ptr::from_ref(b).cast(),
            ModelSlot::String(s) => core::ptr::from_ref(s).cast(),
            ModelSlot::Bowed(b) => core::ptr::from_ref(b).cast(),
            ModelSlot::Sympathetic(m) => core::ptr::from_ref(m).cast(),
        }
    }

    /// `mode`'s model; Sympathetic's lease is from a pool of its own.
    fn model(mode: ResonatorMode) -> Model {
        match mode {
            ResonatorMode::Sympathetic => {
                let mut alloc = SymAlloc::new();
                assert!(alloc.place(VoiceIdx::ALL[0]));
                Model::Sympathetic(Halo::Full(alloc.lend(VoiceIdx::ALL[0]).expect("promised")))
            }
            other => Model::resting(other),
        }
    }

    /// The loop gain the DSP runs at C3 is the old DECAY's gain per pass:
    /// the DAMP law and its v1 inverse agree.
    #[test]
    fn old_decay_gain_survives_at_c3() {
        for decay in [0.2, 0.3, 0.6, 1.0] {
            let p = ModalParams {
                damp: damp_from_v1_decay(decay),
                ..Default::default()
            };
            let got = main_string(&Macros::of(&p), 130.81).gain.get();
            let want = 0.999 - 0.009 * decay;
            assert!((got - want).abs() < 1e-5, "DECAY {decay}: {got} vs {want}");
        }
    }

    /// Today's relation: the halo's damping 0.7× the main string's, so it
    /// is no darker.
    #[test]
    fn the_halo_is_no_darker_than_the_main_string() {
        let bright = ModalParams::default().bright;
        assert!(lp_coeff(halo_bright(bright)) <= lp_coeff(bright));
    }

    /// COUPLE and HALO at their defaults give today's fixed 0.025 and 0.15,
    /// latched at note-on.
    #[test]
    fn couple_and_halo_default_to_todays_mix() {
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, ResonatorMode::Sympathetic);
        let mut p = sym_params();
        (p.couple, p.halo) = (0.25, 0.25);
        e.note_on(48, 100, &p, SR, &mut pool);
        let ModelSlot::Sympathetic(m) = &e.model else {
            unreachable!()
        };
        let set = &pool.sets[set_of(m).index()];
        assert_eq!((set.coupling, set.level), (0.025, 0.15));
    }

    #[test]
    fn model_slot_layout_matches_repr() {
        for (from, first) in MODES.into_iter().enumerate() {
            let mut raw = Box::<ModelSlot>::new_uninit();
            // SAFETY: the box is valid for `size_of::<ModelSlot>()` byte writes.
            unsafe {
                raw.as_mut_ptr()
                    .cast::<u8>()
                    .write_bytes(0xA5, size_of::<ModelSlot>())
            };
            ModelSlot::init_in_place(&mut raw, model(first));
            // SAFETY: `init_in_place` built a valid slot in the box.
            let mut slot = unsafe { raw.assume_init() };
            // From each model to every other, and back.
            for k in 0..=MODES.len() {
                let i = (from + k) % MODES.len();
                if k > 0 {
                    slot.rebuild(model(MODES[i]));
                }
                assert_eq!(slot.mode(), MODES[i]);
                let (tag, payload) = slot.mirror_parts();
                assert_eq!(usize::from(tag), i, "{:?}", MODES[i]);
                assert_eq!(payload_addr(&slot), payload, "{:?}", MODES[i]);
            }
        }
    }

    const SR: u32 = 48_000;
    /// Blocks in a second.
    const SECOND: usize = SR as usize / BLOCK_SIZE;

    fn engine(pool: &mut SymPool, mode: ResonatorMode) -> Box<ModalEngine> {
        let model = match mode {
            ResonatorMode::Sympathetic => {
                assert!(pool.alloc.place(VoiceIdx::ALL[0]));
                Model::Sympathetic(Halo::Full(
                    pool.alloc.lend(VoiceIdx::ALL[0]).expect("promised"),
                ))
            }
            other => Model::resting(other),
        };
        let mut raw = Box::<ModalEngine>::new_uninit();
        ModalEngine::init_in_place(&mut raw, model);
        // SAFETY: `init_in_place` built a valid engine in the box.
        unsafe { raw.assume_init() }
    }

    /// Sympathetic's eight lines: the main string, then its set's seven.
    fn lines<'a>(e: &'a ModalEngine, pool: &'a SymPool) -> Vec<&'a KsString> {
        match &e.model {
            ModelSlot::Sympathetic(m) => core::iter::once(&m.main.string)
                .chain(&pool.sets[set_of(m).index()].strings)
                .collect(),
            _ => unreachable!(),
        }
    }

    /// The slot a Sympathetic voice's halo rings.
    fn set_of(m: &SympatheticVoice) -> SymSlot {
        match &m.halo {
            Halo::Full(l) => l.slot(),
            Halo::Bare => panic!("bare"),
        }
    }

    /// Every line's next clear zeros its whole ring: the full clear.
    fn soil(e: &mut ModalEngine, pool: &mut SymPool) {
        let ModelSlot::Sympathetic(m) = &mut e.model else {
            unreachable!()
        };
        m.main.string.soil();
        for s in pool.sets[set_of(m).index()].strings.iter_mut() {
            s.soil();
        }
    }

    /// Floats the next clear writes, over the eight lines.
    fn to_clear(e: &ModalEngine, pool: &SymPool) -> usize {
        lines(e, pool).iter().map(|s| s.line().1).sum()
    }

    fn sym_params() -> ModalParams {
        ModalParams {
            mode: ResonatorMode::Sympathetic,
            ..Default::default()
        }
    }

    /// `blocks` blocks, pitch ratio `pitch` from block `from` on.
    fn play(
        e: &mut ModalEngine,
        pool: &mut SymPool,
        p: &ModalParams,
        blocks: usize,
        (from, pitch): (usize, f32),
    ) -> Vec<u32> {
        let mut bits = Vec::new();
        for b in 0..blocks {
            if b == from {
                e.set_pitch(pitch);
            }
            let mut out = [0.0; BLOCK_SIZE];
            e.render(&mut out, p, SR, pool);
            bits.extend(out.map(f32::to_bits));
        }
        bits
    }

    /// The dirty extent clears what the full ring's clear did: bit for
    /// bit, from a slot last played at the lowest pitch or two octaves
    /// up, with a PITCH drop in each note lengthening every loop past what
    /// the note-on set.
    #[test]
    fn dirty_clear_is_bit_identical_to_the_full_clear() {
        let p = sym_params();
        for last in [0, 43] {
            for (note, drop) in [(96, 0.125), (60, 0.25), (31, 0.5)] {
                let run = |full: bool| {
                    let mut pool = SymPool::boxed();
                    let mut e = engine(&mut pool, p.mode);
                    e.note_on(last, 100, &p, SR, &mut pool);
                    play(&mut e, &mut pool, &p, SECOND / 4, (SECOND / 8, 0.5));
                    e.set_pitch(1.0);
                    if full {
                        soil(&mut e, &mut pool);
                    }
                    e.note_on(note, 100, &p, SR, &mut pool);
                    play(&mut e, &mut pool, &p, SECOND / 2, (SECOND / 8, drop))
                };
                assert!(run(false) == run(true), "last {last}, note {note}");
            }
        }
    }

    /// A slot handed over from a low note plays its next note as a
    /// never-used slot does, and every line past the loop reads silent.
    #[test]
    fn a_reused_slot_plays_nothing_stale() {
        let p = sym_params();
        let fresh = {
            let mut pool = SymPool::boxed();
            let mut e = engine(&mut pool, p.mode);
            e.note_on(84, 100, &p, SR, &mut pool);
            play(&mut e, &mut pool, &p, SECOND, (SECOND / 2, 0.25))
        };
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, p.mode);
        e.note_on(0, 127, &p, SR, &mut pool);
        play(&mut e, &mut pool, &p, SECOND, (0, 1.0));
        e.set_pitch(1.0);
        // The voice's own string plucks fresh noise; the slot is the subject.
        let ModelSlot::Sympathetic(m) = &mut e.model else {
            unreachable!()
        };
        m.main.string.reseed();
        e.note_on(84, 100, &p, SR, &mut pool);
        for (i, s) in lines(&e, &pool).into_iter().enumerate() {
            let (buf, _) = s.line();
            assert!(buf[s.delay()..].iter().all(|&x| x == 0.0), "line {i}");
            if i > 0 {
                assert!(buf.iter().all(|&x| x == 0.0), "set line {i}");
            }
        }
        let reused = play(&mut e, &mut pool, &p, SECOND, (SECOND / 2, 0.25));
        assert!(reused == fresh);
    }

    /// A note-on clears what the lines' last notes wrote, not the ring: a
    /// slot last played high clears a fraction of one last played at the
    /// lowest pitch, which clears at most all eight rings.
    #[test]
    fn the_clear_scales_with_the_dirty_extent() {
        let p = sym_params();
        let ring = (1 + NUM_SYMPATHETIC) * MAX_STRING_DELAY;
        let cleared_after = |last: u8| {
            let mut pool = SymPool::boxed();
            let mut e = engine(&mut pool, p.mode);
            e.note_on(last, 100, &p, SR, &mut pool);
            play(&mut e, &mut pool, &p, SECOND, (0, 1.0));
            to_clear(&e, &pool)
        };
        // A4: every loop at most 110 samples.
        let high = cleared_after(69);
        assert!(high <= (1 + NUM_SYMPATHETIC) * 110, "{high}");
        // MIDI 0: the main string clamps to the whole ring; the halo's
        // fold into it, each over half of it.
        let low = cleared_after(0);
        assert!(low <= ring && 2 * low > ring, "{low}");
        assert!(high * 8 < low);
        // The dirty extent is only ever the clear's upper bound: past it
        // every sample is silent.
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, p.mode);
        e.note_on(0, 100, &p, SR, &mut pool);
        play(&mut e, &mut pool, &p, SECOND, (SECOND / 2, 4.0));
        for (i, s) in lines(&e, &pool).into_iter().enumerate() {
            let (buf, dirty) = s.line();
            assert!(buf[dirty..].iter().all(|&x| x == 0.0), "line {i}");
        }
    }

    /// `SymPool::note_on_clear` is what the note-on then clears, to the
    /// byte: a kept slot and main string after low and high notes and a
    /// PITCH drop, and a fresh engine lent a slot last played low.
    #[test]
    fn note_on_clear_is_what_the_note_on_clears() {
        let p = sym_params();
        let v0 = VoiceIdx::ALL[0];
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, p.mode);
        for (note, pitch) in [(0, 1.0), (96, 1.0), (60, 0.25), (31, 1.0), (84, 1.0)] {
            e.set_pitch(1.0);
            let want = pool.note_on_clear(Some(&e), v0);
            let _ = take_cleared_bytes();
            e.note_on(note, 100, &p, SR, &mut pool);
            assert_eq!(take_cleared_bytes(), want, "note {note}");
            assert!(want <= SYM_NOTE_ON_CLEAR_MAX);
            play(&mut e, &mut pool, &p, SECOND / 8, (1, pitch));
        }
        // Voice 1, unplaced, plays bare: a fresh main string. Placed, it
        // is promised a slot never played: eight fresh lines.
        let v1 = VoiceIdx::ALL[1];
        assert_eq!(pool.note_on_clear(None, v1), FRESH_CLEAR_BYTES);
        assert!(pool.alloc.place(v1));
        let fresh = (1 + NUM_SYMPATHETIC) * FRESH_CLEAR_BYTES;
        assert_eq!(pool.note_on_clear(None, v1), fresh);
    }

    /// The modes a bank renders, after its last `compute_filters`.
    fn rendered(b: &ModalBank) -> &[Svf] {
        &b.filters[..b.sounding]
    }

    /// No mode rings at or past 0.49 of the rate: the bank drops them, as
    /// Rings does, not clamps them.
    #[test]
    fn no_bank_mode_reaches_nyquist() {
        let p = ModalParams {
            mode: ResonatorMode::Modal,
            structure: 1.0,
            modes: BankModes::M48,
            ..Default::default()
        };
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, p.mode);
        e.note_on(96, 127, &p, SR, &mut pool);
        let mut out = [0.0; BLOCK_SIZE];
        e.render(&mut out, &p, SR, &mut pool);
        let ModelSlot::Bank(b) = &e.model else {
            unreachable!()
        };
        let ceiling = rings::tan_approx(0.49);
        assert!(!rendered(b).is_empty());
        for (i, f) in rendered(b).iter().enumerate() {
            assert!(f.g() < ceiling, "mode {i}: g {}", f.g());
        }
    }

    /// Bowed writes round its ring, the loop and two; its note-on still
    /// starts silent.
    #[test]
    fn bowed_clears_the_ring_it_wrote() {
        let p = ModalParams {
            mode: ResonatorMode::Bowed,
            ..Default::default()
        };
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, p.mode);
        e.note_on(96, 100, &p, SR, &mut pool);
        play(&mut e, &mut pool, &p, SECOND / 4, (0, 1.0));
        let ModelSlot::Bowed(b) = &mut e.model else {
            unreachable!()
        };
        let (buf, dirty) = b.string.line();
        assert_eq!(dirty, b.string.delay() + 2);
        assert!(buf.iter().any(|&x| x != 0.0));
        e.note_on(96, 100, &p, SR, &mut pool);
        let ModelSlot::Bowed(b) = &e.model else {
            unreachable!()
        };
        assert!(b.string.line().0.iter().all(|&x| x == 0.0));
    }
}
