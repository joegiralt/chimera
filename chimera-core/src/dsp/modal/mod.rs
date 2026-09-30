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

pub use chords::{CHORD_COUNT, CHORD_GLIDE_TAU, CHORDS, chord_of, fold};
pub use params::*;
pub use string::MAX_STRING_DELAY;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use super::{fast_tanh, xorshift_noise};
use crate::dsp::dc_blocker::DcBlocker;
use crate::dsp::glide::Glide;
use crate::hw::{Cost, SAMPLE_RATE};
use crate::in_place::{in_place_enum, uninit_at};
use crate::sym_alloc::{Lease, SYM_SLOTS, SymAlloc, SymSlot};
use crate::voice_alloc::VoiceIdx;
use chords::period_ratios;
use core::f32::consts::TAU;
use ensemble::{Ensemble, rate_hz};
use loop_parts::{DC_HZ, LoopGain, RELEASE_SAMPLES, Release, damped};
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
    /// The force `force` eases to: FORCE's at the note's velocity, read
    /// every block, then 0 at note-off.
    force_to: f32,
    /// Force shed a sample at note-off: the note's force over `RELEASE_SAMPLES`.
    lift: f32,
    /// The bow's velocity, easing to SPEED × `BOW_SPEED`.
    bow_vel: f32,
    /// `0.5 + 0.5·velocity`, latched at note-on (`bow_force`).
    vel_scale: f32,
    /// On the string: note-on to note-off.
    bowing: bool,
    /// Samples pushed since note-on, for `KsString::ring_tap`.
    written: u32,
    /// The lifted bow's ramp.
    release: Release,
    /// The loop's smoothing.
    hair: BowHair,
    /// BRIGHT's one-pole on the output.
    tone: f32,
    /// How much of `unlocked`'s move the loop takes (`grip`).
    grip: f32,
    /// The friction curve's slope, easing to FORCE's (`friction_slope`).
    slope: f32,
    slope_to: f32,
    /// The last block's bow point, samples back (0 at a fresh note), and
    /// its comb's weight.
    back: f32,
    comb: f32,
}

crate::in_place::field_list!(BowedString => BowedString {
    string, force, force_to, lift, bow_vel, vel_scale, bowing, written, release, hair, tone, grip, slope, slope_to, back, comb,
});

/// One pool slot (exclusive-state spec § 4.2): the seven strings a
/// Sympathetic note's main string sets ringing. The main string is the
/// voice's (`SympatheticVoice`).
pub struct SympatheticSet {
    strings: [KsString; NUM_SYMPATHETIC],
    /// The chord STRUCTURE last stepped to (`chord_of`).
    chord: u8,
    /// Its periods over the main string's, unfolded.
    ratios: [f32; NUM_SYMPATHETIC],
    /// Each string's period, gliding to the chord's.
    glides: [Glide; NUM_SYMPATHETIC],
    /// The main string's period at the last retune, samples.
    main: f32,
    /// COUPLE's and HALO's gains, latched at note-on.
    coupling: f32,
    level: f32,
    /// Each sympathetic string's last output, not yet stored: it is stored
    /// with the next sample's coupled input (`KsString::tick_coupled`).
    pending: [f32; NUM_SYMPATHETIC],
}

crate::in_place::field_list!(SympatheticSet => SympatheticSet {
    strings, chord, ratios, glides, main, coupling, level, pending,
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
    /// The note's loudest output sample: silence is judged against it.
    peak: f32,
    /// On a string model's output, not in its loop, where its phase would
    /// detune the upper partials.
    dc: DcBlocker,
    macros: Macros,
    /// The note's first block is to come: it snaps `macros` to its
    /// modulated values and shapes the pluck at its POS.
    shape_pending: bool,
    /// A re-strike's first block is to come: it adds the pluck at the
    /// eased POS; nothing snaps.
    restruck: bool,
}

crate::in_place::field_list!(ModalEngine => ModalEngine {
    model, frequency, pitch, tuned, active, silence_counter, peak, dc, macros, shape_pending,
    restruck,
});

// A `ModalEngine` is its largest model plus the fields every model shares
// (`frequency`, `pitch`, `tuned`, `active`, `silence_counter`, `peak`,
// `dc`, `macros`, `shape_pending`, `restruck`), never the sum of models. The slot's
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
    let shared = size_of::<(f32, f32, f32, bool, u32, f32, DcBlocker, Macros, bool, bool)>();
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
    /// (ADR 0052), plus 10 % (a saving taken at 90 %), rounded up to 10.
    /// Per-block work is spread over the block's 64 samples, a ring's
    /// wrap-free spans at the bench's notes (2.2 a block); a `powf` or a
    /// `set_period` is taken as about 130 instructions (ADR 0056, Costs).
    ///
    /// STRING, 390 − 69, rounded up to 330: 99 instructions a sample to
    /// 34 (the string's spans, 21 a sample, the three-tap low-pass, four
    /// dispersion allpasses and the tuning allpass in registers; the output
    /// blocker), −86 cycles; DAMP's two `powf`s a block, 8, and the
    /// dispersion's re-split each block STRUCTURE glides, 8, billed always,
    /// +18. BODY and the ensemble bill apart.
    pub const COST_STRING: Cost = Cost(330);
    /// The one-loop bow (ADR 0064), counted in the thumbv7em release build
    /// (`render_bowed` kept out of line so it can be): its per-sample loop,
    /// traced on the bowed path with both `fast_tanh`s off their clamps,
    /// 256 instructions (the easing, the release, both taps' low-passes and
    /// the bow point's lerp, their bounds checks, the smoothing and the
    /// push); a moved POS's per-sample split, 14 more: 270. Against the
    /// one-loop bow's benched 620 at 143 instructions and two `tanhf` bodies
    /// (135), with the two `vdiv.f32`s at 14 cycles: 620 + (270 − 143 −
    /// 135) × 1.46 × 1.1 + 2 × (14 − 1.46) × 1.1 = 634.8, rounded up to 640.
    pub const COST_BOWED: Cost = Cost(640);
    /// 809 (benched, ADR 0054) − 271, rounded up to 540: 419 instructions a
    /// sample to 187, −306 cycles. Each halo string runs its block in
    /// spans, 19 a sample (42 before, 87 at task 11); the main string 15;
    /// the coupling, the mix and the `tanhf` dispatch 32. Ten `powf`s a
    /// block for the loop gains, +35.
    pub const COST_SYMPATHETIC: Cost = Cost(540);
    /// The resonator bank: this plus `COST_MODE` per mode. Unchanged: its
    /// sample loop is as benched (MDL RES /VOICE 1,865; less the chain's 57,
    /// 1,808 at 32 modes against 1,900 billed), and the macros' easing is a few operations a block.
    /// Dropping modes past Nyquist can only save.
    pub const COST_BANK: Cost = Cost(460);
    pub const COST_MODE: Cost = Cost(45);

    /// More on STRING and SYMP with BODY above 0: three band-passes over
    /// the block, their state in registers, 47 instructions a sample.
    pub const BODY: Cost = Cost(80);
    /// More on STRING and SYMP with ENS MIX above 0: two interpolated heads
    /// and the LFO's rotation, 82 instructions a sample on STRING (65 on
    /// SYMP's main string).
    pub const ENSEMBLE: Cost = Cost(140);

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
    /// gliding (`Glide`, ADR 0062). A host estimate as `PITCH`, until the
    /// bench's SYM LFO row reads it, taking a `exp2f`, `log2f` or
    /// `set_period` as 130 instructions: a gliding block's seven ticks
    /// (an `exp2f` and a few operations each) and seven `set_period`s,
    /// 7 × 265 / 64 = 29 a sample; a chord step, which a routed STRUCTURE
    /// may take any block, seven `log2f`s and folds, 7 × 140 / 64 = 15.3;
    /// the halo's render unchanged. (29 + 15.3) × 1.46 × 1.1 = 71.1,
    /// rounded up to 80.
    pub const CHORD: Cost = Cost(80);

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
            addr_of_mut!((*p).peak).write(0.0);
            addr_of_mut!((*p).dc).write(DcBlocker::new(DC_HZ, SAMPLE_RATE));
            addr_of_mut!((*p).macros).write(Macros::of(&ModalParams::default()));
            addr_of_mut!((*p).shape_pending).write(false);
            addr_of_mut!((*p).restruck).write(false);
            slot.assume_init_mut()
        }
    }

    /// The model this engine plays: its last note-on's, else the one it
    /// was built for.
    pub fn mode(&self) -> ResonatorMode {
        self.model.mode()
    }

    /// Its model's `out_gain`, for the voice's VCA.
    pub fn out_gain(&self) -> f32 {
        out_gain(self.mode())
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

    /// STRING's chain STRUCTURE and line length: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn string_line(&self) -> Option<(f32, usize)> {
        match &self.model {
            ModelSlot::String(v) => Some(v.line()),
            _ => None,
        }
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
        // The sounding note struck again: added to what rings, nothing
        // cleared (the owner's UAT, 2026-09-30). BANK's strike always adds.
        let restrike = self.active && freq / sample_rate as f32 == self.frequency;
        self.frequency = freq / sample_rate as f32;
        let freq = self.pitched(freq);
        self.tuned = self.pitch;
        let bank_freq = self.pitched(self.frequency);
        // A re-strike's macros ease on.
        if !restrike {
            self.macros = Macros::of(params);
        }
        let m = &self.macros;

        match &mut self.model {
            ModelSlot::Bank(bank) => {
                bank.resolution = params.modes.count();
                bank.compute_filters(m, bank_freq);
                if !restrike {
                    bank.cos_osc.init(params::beta(m.pos, params::END));
                }
                let burst_ms = 2.0 + params.burst * 4.0;
                bank.burst_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
                // As loud a strike at every pitch: the modes' gain rises
                // with f0, so the burst falls with it, C3 as it was.
                bank.burst_amp =
                    vel * params.excite * BURST_AT_C3 / (bank_freq * sample_rate as f32);
                bank.burst_lp = 0.0;
            }
            ModelSlot::String(v) if restrike => v.restrike(vel * params.excite, params.color),
            ModelSlot::String(v) => {
                v.pluck(
                    (freq, sample_rate),
                    Some(m.structure),
                    vel * params.excite,
                    (
                        params.body,
                        ensemble(params, rate_hz(params.ens_rate), sample_rate),
                    ),
                    params.color,
                );
            }
            ModelSlot::Bowed(b) => {
                b.grip = grip(bow_force(params.force, vel), params.speed);
                // A re-strike sets the bow back on the string as it rings.
                if !restrike {
                    b.string.clear();
                    (b.hair, b.tone, b.back) = (BowHair::REST, 0.0, 0.0);
                    b.force = bow_force(params.force, vel);
                    (b.bow_vel, b.written) = (params.speed * BOW_SPEED, 0);
                }
                b.tune(freq, sample_rate);
                b.force_to = bow_force(params.force, vel);
                b.slope_to = friction_slope(params.force);
                if !restrike {
                    b.slope = b.slope_to;
                }
                b.lift = 0.0;
                b.vel_scale = 0.5 + 0.5 * vel;
                b.bowing = true;
                b.release = Release::HELD;
            }
            ModelSlot::Sympathetic(v) if restrike => {
                // The halo rings on, on its chord.
                v.main.restrike(vel * params.excite, params.color);
            }
            ModelSlot::Sympathetic(v) => {
                // STRUCTURE tunes the halo only: the main string is not stiff.
                let ens = ensemble(params, rate_hz(SYMP_ENS_RATE), sample_rate);
                v.main.pluck(
                    (freq, sample_rate),
                    None,
                    vel * params.excite,
                    (params.body, ens),
                    params.color,
                );
                if let Some(set) = pool.halo(&v.halo) {
                    set.note_on(sample_rate as f32 / freq, chord_of(m.structure), params);
                }
            }
        }

        if restrike {
            self.restruck = true;
        } else {
            self.shape_pending = true;
            self.dc.reset();
            self.peak = 0.0;
        }
        self.active = true;
        self.silence_counter = 0;
    }

    /// The strings follow a changed pitch ratio (a divide per string,
    /// `ModalEngine::PITCH`), STRING's dispersion a moved STRUCTURE
    /// (gliding, `StringVoice::tune`), and SYMP's halo `chord`, the
    /// un-eased STRUCTURE's (gliding, `SympatheticSet::retune`). A note's
    /// first block (`snap`) snaps both.
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
                v.tune(freq, sample_rate, structure, snap)
            }
            ModelSlot::String(_) => {}
            ModelSlot::Bowed(b) if pitched => b.tune(freq, sample_rate),
            ModelSlot::Bowed(_) => {}
            ModelSlot::Sympathetic(m) => {
                if pitched {
                    m.main.tune(freq, sample_rate, 0.0, false);
                }
                if let Some(set) = pool.halo(&m.halo) {
                    set.retune(sample_rate as f32 / freq, chord, (pitched, snap));
                }
            }
        }
    }

    /// Note-off stops the exciter; the resonator rings on DAMP (the
    /// owner's UAT, 2026-09-30): a string and SYMP's halo ring until their
    /// T60 or a steal. Bowed's bow lifts, its loop ramping to DAMP's ring.
    /// Nothing scales a buffer (#51).
    pub fn note_off(&mut self, _pool: &mut SymPool) {
        let f = self.pitched(self.frequency);
        if let ModelSlot::Bowed(b) = &mut self.model {
            b.lift = b.force / RELEASE_SAMPLES as f32;
            b.force_to = 0.0;
            b.bowing = false;
            // The free ring has no lock to lean against: its own period.
            b.grip = 0.0;
            b.tune(f * SAMPLE_RATE as f32, SAMPLE_RATE);
            let (_, ring) = lifted(&self.macros, (f * SAMPLE_RATE as f32, TAU * f));
            b.release.lift(BOW_GAIN, ring);
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
            if core::mem::take(&mut self.restruck) {
                let pos = self.macros.pos;
                match &mut self.model {
                    ModelSlot::String(s) => s.shape(pos),
                    ModelSlot::Sympathetic(v) => v.main.shape(pos),
                    ModelSlot::Bank(_) | ModelSlot::Bowed(_) => {}
                }
            }
        }
        let m = self.macros;

        let bank_freq = self.pitched(self.frequency);
        // The strings' f0 in Hz, for their loop gains.
        let f0 = bank_freq * sample_rate as f32;
        let w0 = core::f32::consts::TAU * bank_freq;
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
                // POS glides its weights over the block; a note's first takes it whole.
                let place = params::beta(m.pos, params::END);
                if first {
                    bank.cos_osc.init(place);
                } else {
                    bank.cos_osc.glide(place, BLOCK_SIZE as u32);
                }
                render_modal(bank, output);
                bank.burst_remaining > 0
            }
            ModelSlot::String(v) => {
                v.set_ensemble();
                render_string(v, output, &m, (f0, w0));
                false
            }
            ModelSlot::Bowed(b) => {
                // FORCE and SPEED are live while bowed, as the macros are.
                if b.bowing {
                    b.force_to = params.force * b.vel_scale;
                    b.slope_to = friction_slope(params.force);
                }
                render_bowed(b, output, &m, (f0, w0, params.speed * BOW_SPEED));
                // Never freed while bowed, however low its note (#206).
                b.force > 0.0
            }
            ModelSlot::Sympathetic(v) => {
                let v = &mut **v;
                v.main.set_ensemble();
                let set = pool.halo(&v.halo);
                render_sympathetic(&mut v.main, set, output, &m, (f0, w0));
                false
            }
        };
        // Every model's DC stops here, BANK's tanh's too; silence is judged
        // on what is heard, after the blocker.
        for s in output.iter_mut() {
            *s = self.dc.process(*s);
            max_level = max_level.max(libm::fabsf(*s));
        }

        self.peak = self.peak.max(max_level);
        // Silent 60 dB under the note's peak, as DAMP's T60 counts: a quiet
        // high note rings as long as a loud low one. Never under
        // `SILENT_FLOOR`.
        if max_level <= (self.peak * SILENT_REL).max(SILENT_FLOOR) && !exciting {
            self.silence_counter += 1;
            if self.silence_counter > 10 {
                self.active = false;
            }
        } else {
            self.silence_counter = 0;
        }
    }
}

/// A note is silent this far under its peak: −60 dB, DAMP's T60.
const SILENT_REL: f32 = 0.001;
/// And under this whatever its peak: −120 dB.
const SILENT_FLOOR: f32 = 1e-6;

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
            addr_of_mut!((*p).lift).write(0.0);
            addr_of_mut!((*p).bow_vel).write(0.0);
            addr_of_mut!((*p).vel_scale).write(0.0);
            addr_of_mut!((*p).bowing).write(false);
            addr_of_mut!((*p).written).write(0);
            addr_of_mut!((*p).release).write(Release::HELD);
            addr_of_mut!((*p).hair).write(BowHair::REST);
            addr_of_mut!((*p).tone).write(0.0);
            addr_of_mut!((*p).grip).write(0.0);
            addr_of_mut!((*p).slope).write(BOW_SLOPE);
            addr_of_mut!((*p).slope_to).write(BOW_SLOPE);
            addr_of_mut!((*p).back).write(0.0);
            addr_of_mut!((*p).comb).write(0.0);
            slot.assume_init_mut()
        }
    }

    /// The loop for `freq`: half its period, less the smoothing's delay,
    /// the allpass exact at `freq`; the smoothing's make-up at `freq`.
    fn tune(&mut self, freq: f32, sample_rate: u32) {
        let (period, _, w) = string::loop_at(freq, sample_rate);
        self.hair = self.hair.tuned(freq, w);
        let period = period + self.grip * (unlocked(period) - period);
        self.string.set_period(0.5 * period, BowHair::DELAY, w);
    }
}

/// The share of `unlocked`'s move a bow of this effective force (FORCE
/// at the note's velocity, `bow_force`) and SPEED takes, as measured:
/// all from INIT's v100, 0.447, to FORCE 0.5 at v127, 0.55; none at 0.3
/// and under, where the stick-slip is too soft to lock, nor at 0.8 and
/// over, whose windows are others; none at SPEED 0.1, where a slow bow
/// already runs sharp, all from INIT's 0.5.
fn grip(force: f32, speed: f32) -> f32 {
    let f = ((force - 0.3) / (0.447 - 0.3)).min((0.8 - force) / (0.8 - 0.55));
    f.clamp(0.0, 1.0) * ((speed - 0.1) / 0.4).clamp(0.0, 1.0)
}

/// Periods under this, samples (C6 up), lean toward a whole number: the
/// stick-slip locks a period within `UNDER` below or `OVER` above one to
/// it, up to 6.3 cents off.
const LOCKS_UNDER: f32 = 48.0;
/// The lock's reach either side of a whole period, samples, measured at
/// C6 to C7 (a request swept in cents against the pitch heard).
const UNDER: f32 = 0.18;
const OVER: f32 = 0.33;
/// Nearer a whole period than this the lock is heard under 4 cents: left.
const NEAR: f32 = 0.05;

/// `period` moved to the edge of a whole period's lock when inside it:
/// there the stick-slip is pulled back toward `period`, which it then
/// plays within 1 cent (measured: C6 +2 cents asked, D6 +2, G#6 +4, A6
/// −3, C7 +8). Longer periods, and those nearly whole, as they are.
fn unlocked(period: f32) -> f32 {
    if period >= LOCKS_UNDER {
        return period;
    }
    let n = libm::roundf(period);
    let d = period - n;
    if d > NEAR && d < OVER {
        n + OVER
    } else if d < -NEAR && d > -UNDER {
        n - UNDER
    } else {
        period
    }
}

/// The bowed loop's smoothing, a binomial on what the bow pushes: linear
/// phase, so every partial is delayed alike, and the stick-slip's corner,
/// spread over a few samples, times the period between them. Without it
/// the period locked to whole samples, 26 cents sharp at C7. `[1, 2, 1]/4`
/// under `NARROW_HZ`, where it is enough, so low notes keep the one-loop
/// bow's edge; `[1, 6, 15, 20, 15, 6, 1]/64` from `WIDE_HZ`; between them
/// a crossfade, both centred 3 samples back, so no neighbouring notes
/// step. Its gain at f0 is made up, so DAMP sets the ring.
#[derive(Clone, Copy)]
struct BowHair {
    /// The last six samples pushed, newest first.
    past: [f32; 6],
    /// The seven taps' share.
    wide: f32,
    /// Over its gain at f0.
    norm: f32,
}

impl BowHair {
    const REST: Self = Self {
        past: [0.0; 6],
        wide: 0.0,
        norm: 1.0,
    };
    /// Its delay, samples.
    const DELAY: f32 = 3.0;
    /// The crossfade's ends, Hz: A4 to D#5.
    const NARROW_HZ: f32 = 440.0;
    const WIDE_HZ: f32 = 622.0;
    /// The least gain at f0 made up: seven taps' at C7.
    const LEAST: f32 = 0.94;

    /// For `freq` Hz, `w` rad/sample, its history kept.
    fn tuned(self, freq: f32, w: f32) -> Self {
        let half = 0.5 * (1.0 + libm::cosf(w));
        let t = (libm::log2f(freq / Self::NARROW_HZ)
            / libm::log2f(Self::WIDE_HZ / Self::NARROW_HZ))
        .clamp(0.0, 1.0);
        let wide = t * t * (3.0 - 2.0 * t);
        let gain = (1.0 - wide) * half + wide * half * half * half;
        Self {
            wide,
            // Past C7 the loop is too short to make up: it rings shorter.
            norm: 1.0 / gain.max(Self::LEAST),
            ..self
        }
    }

    /// Its gain at `w` rad/sample, made up: for the tests.
    #[cfg(test)]
    fn response(&self, w: f32) -> f32 {
        let half = 0.5 * (1.0 + libm::cosf(w));
        ((1.0 - self.wide) * half + self.wide * half * half * half) * self.norm
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let [a, b, c, d, e, f] = self.past;
        self.past = [x, a, b, c, d, e];
        let narrow = (b + 2.0 * c + d) * 0.25;
        let wide = (x + f + 6.0 * (a + e) + 15.0 * (b + d) + 20.0 * c) * (1.0 / 64.0);
        (narrow + self.wide * (wide - narrow)) * self.norm
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
            addr_of_mut!((*p).glides)
                .write([Glide::new(INIT_PERIOD, CHORD_GLIDE_TAU); NUM_SYMPATHETIC]);
            addr_of_mut!((*p).main).write(INIT_PERIOD);
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
        self.glides = self
            .ratios
            .map(|r| Glide::new(fold(period * r), CHORD_GLIDE_TAU));
        self.main = period;
        self.split();
        self.coupling = 0.1 * params.couple;
        self.level = 0.6 * params.halo;
        self.pending = [0.0; NUM_SYMPATHETIC];
    }

    /// Per block, on a main string of `period` samples: a new `chord`
    /// glides each string from where it is (`CHORD_GLIDE_TAU`), by the
    /// table's interval, or round the fold by the least move where that
    /// does not fit the line; a note's first block (`snap`) takes it whole.
    /// A pitch change moves a gliding set's targets, and a resting set at
    /// once.
    fn retune(&mut self, period: f32, chord: usize, (pitched, snap): (bool, bool)) {
        let stepped = chord != self.chord as usize;
        if !stepped && !pitched {
            return;
        }
        let (was, gliding) = (self.ratios, self.glides.iter().any(Glide::gliding));
        let moved = period / self.main;
        self.main = period;
        if stepped {
            self.chord = chord as u8;
            self.ratios = period_ratios(chord);
        }
        for ((g, r), w) in self.glides.iter_mut().zip(self.ratios).zip(was) {
            let folded = fold(period * r);
            if snap {
                g.toward(folded);
                g.snap();
            } else if stepped {
                g.toward(octave_near(folded, g.value() * r / w));
            } else {
                // Under an octave's move, with the pitch from where it is,
                // in the octave it is in while its line fits (a glide or the
                // fold may have left it over the fold's pick), at most one
                // over it. A larger jump re-voices the chord at the fold's:
                // kept where they were, strings a unison apart summed
                // coherently, 4 dB up.
                let near = moved > 0.5 && moved < 2.0;
                g.toward(if near {
                    octave_near(folded, fold(g.target() * moved))
                } else {
                    folded
                });
                if !gliding {
                    g.snap();
                }
            }
        }
        self.split();
    }

    /// A block of glide: the gliding strings a step on, re-split.
    fn glide_step(&mut self) {
        if self.glides.iter().any(Glide::gliding) {
            self.glides.iter_mut().for_each(Glide::tick);
            self.split();
        }
    }

    /// The strings' periods now, samples.
    fn periods(&self) -> [f32; NUM_SYMPATHETIC] {
        self.glides.map(|g| g.value())
    }

    /// Each string's line and allpass at `periods`: no dispersion.
    fn split(&mut self) {
        let periods = self.periods();
        for (sym, p) in self.strings.iter_mut().zip(periods) {
            sym.set_period(p, 0.0, TAU / p);
        }
    }
}

fn render_modal(bank: &mut ModalBank, output: &mut [f32; BLOCK_SIZE]) {
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

        // Sum to mono and soft-limit, a quarter of Rings' 2× out: the
        // voice's filter after it stays under its knee (`out_gain` makes up).
        *s = libm::tanhf(odd + even) * 0.5;
    }
}

fn render_string(v: &mut StringVoice, output: &mut [f32; BLOCK_SIZE], m: &Macros, f0: (f32, f32)) {
    v.render(&main_string(m, f0), output);
    v.colour(output);
}

/// The block's STRING or SYMP main string at `f0` Hz, `w` rad/sample:
/// its low-pass and gain. The fundamental
/// rings DAMP's T60 at every pitch (`damped`).
fn main_string(m: &Macros, f0: (f32, f32)) -> KsRenderParams {
    let (lp, gain) = damped(t60(m.damp), f0, lp_coeff(m.bright));
    KsRenderParams { lp, gain }
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

/// Each model's output gain (ADR 0058, 0063), at the voice's VCA: its
/// INIT's C4 at velocity 100 as loud on P1 as ALGO INIT's, the factory
/// median, ±1 dB; STRING's by its peak, its eight-note chord at velocity
/// 127 under 3 dB of the limiter's gain reduction. After the bank's tanh,
/// SYMP's and the voice's filter, so none of them saturates more.
pub const fn out_gain(mode: ResonatorMode) -> f32 {
    match mode {
        ResonatorMode::String => 5.95,
        ResonatorMode::Modal => 11.83,
        ResonatorMode::Bowed => 0.93,
        ResonatorMode::Sympathetic => 4.24,
    }
}

/// C3, Hz: the bank's burst level there is EXCITE's, and falls as 1/f0.
const BURST_AT_C3: f32 = 130.81;

/// SPEED 1's bow velocity; SPEED 0.5 is the old `BOW_VELOCITY · 0.3`.
const BOW_SPEED: f32 = 0.3;
/// FORCE's and SPEED's easing a sample: about the macros' `EASE` a block.
const BOW_EASE: f32 = EASE / BLOCK_SIZE as f32;
/// The bowed loop's gain per pass, until the lift ramps it to DAMP's.
const BOW_GAIN: LoopGain = LoopGain::TOP;
/// BRIGHT 0's low-pass side taps on the bowed loop: the most |H| ≤ 1
/// allows. The bow's stick-slip keeps it gentle.
const BOW_LP: f32 = 0.5;
/// POS over which the bow point's comb fades in from the loop's tap alone.
const BOW_POS_MIN: f32 = 0.03;
/// Loop passes a period: the loop is half the string and inverts each
/// pass, as a string's reflections do, so it sounds f0. The one-loop bow's
/// stick-slip ran a period of two passes, an octave down (UAT 2026-09-30).
const BOW_LOOPS: f32 = 2.0;

/// The friction curve's slope at FORCE 0.5, INIT's: the one-loop bow's.
const BOW_SLOPE: f32 = 8.0;

/// FORCE's friction slope: `BOW_SLOPE` at 0.5, halved at 0 and doubled at
/// 1, so a harder bow grips sharper and its corner, and tone, brighten.
fn friction_slope(force: f32) -> f32 {
    BOW_SLOPE * libm::expf(core::f32::consts::LN_2 * 2.0 * (force - 0.5))
}

/// The bowed loop's low-pass side taps: INIT's BRIGHT's, fixed. BRIGHT in
/// the loop sharpened the stick-slip's corner, darker read brighter.
const BOW_LOOP_LP: f32 = BOW_LP * (1.0 - 0.3);
/// BRIGHT under INIT's 0.3 closes the output's one-pole to this corner
/// at 0, Hz.
const BOW_DARK_HZ: f32 = 1000.0;

/// A lifted bow's loop at `f0` Hz, `w` rad/sample: its low-pass and
/// DAMP's gain, the low-pass's loss at f0 made up (`damped`).
fn lifted(m: &Macros, (f0, w): (f32, f32)) -> (f32, LoopGain) {
    damped(t60(m.damp), (BOW_LOOPS * f0, w), BOW_LOOP_LP)
}

/// BRIGHT on the bow's output: the taps' 3-tap low-pass, `BOW_LP·(1 −
/// BRIGHT)` as the loop's was, and under INIT's 0.3 a one-pole whose pole
/// runs from 0 there to `BOW_DARK_HZ`'s at 0: `(taps' c, one-pole a)`.
fn bow_tone(bright: f32) -> (f32, f32) {
    let open = (bright / 0.3).min(1.0);
    let pole = libm::expf(-TAU * BOW_DARK_HZ / SAMPLE_RATE as f32) * (1.0 - open);
    (BOW_LP * (1.0 - bright), 1.0 - pole)
}

/// A bow point `at` samples back, `at >= 2`: its whole samples and
/// fraction, without a `floorf` (positive, so the cast truncates as floor).
#[inline(always)]
fn split_back(at: f32) -> (usize, f32) {
    let i = at as usize;
    (i, at - i as f32)
}

/// The bow on its string: a half-length loop that inverts each pass
/// (`BOW_LOOPS`) through `BowHair`. DAMP rings the lifted bow, BRIGHT
/// low-passes the output (`bow_tone`), POS combs the output at the bow
/// point, fading in over `BOW_POS_MIN`. The loop and the friction read
/// the one tap, so the pitch holds: a friction reading POS's second tap
/// bows a second loop, which takes the pitch. The tap's place is set once
/// a block.
#[inline(never)]
fn render_bowed(
    b: &mut BowedString,
    output: &mut [f32; BLOCK_SIZE],
    m: &Macros,
    (f0, w0, vel_to): (f32, f32, f32),
) {
    // The lock correction follows the bow as it is now, FORCE and SPEED
    // eased: re-taken each block, the loop re-split when it moves (only
    // where it locks, `LOCKS_UNDER`).
    if b.bowing && f0 * LOCKS_UNDER > SAMPLE_RATE as f32 {
        let g = grip(b.force, b.bow_vel / BOW_SPEED);
        if g != b.grip {
            b.grip = g;
            b.tune(f0, SAMPLE_RATE);
        }
    }
    // Lifted, DAMP's ring, which `Release::gain` never lets rise; bowed, the top.
    let (c, held) = if b.bowing {
        (BOW_LOOP_LP, BOW_GAIN)
    } else {
        lifted(m, (f0, w0))
    };
    let (co, a) = bow_tone(m.bright);
    // The bow point a third of the half-loop from the tap at POS 1: its
    // comb nulls the 3rd, 9th and 15th partials, the string bowed a third
    // of the way along (the loop sounds odd partials only).
    let d = b.string.delay() as f32;
    let back = (d - m.pos * d / 3.0).max(2.0);
    let comb = 0.5 * (m.pos / BOW_POS_MIN).min(1.0);
    // A moved bow point glides across the block, a step a sample; a still
    // one is set once. A note's first block takes it whole.
    let (from, comb_from) = if b.back > 0.0 {
        (b.back, b.comb)
    } else {
        (back, comb)
    };
    (b.back, b.comb) = (back, comb);
    let moving = from != back || comb_from != comb;
    let step = 1.0 / BLOCK_SIZE as f32;
    let mut point = split_back(back);
    let mut comb = comb;
    for (k, s) in output.iter_mut().enumerate() {
        if moving {
            let t = (k + 1) as f32 * step;
            let at = from + t * (back - from);
            point = split_back(at);
            comb = comb_from + t * (b.comb - comb_from);
        }
        if b.bowing {
            // At its target, no bit moves.
            b.force += BOW_EASE * (b.force_to - b.force);
            b.slope += BOW_EASE * (b.slope_to - b.slope);
            b.bow_vel += BOW_EASE * (vel_to - b.bow_vel);
        } else if b.force > b.force_to {
            b.force = (b.force - b.lift).max(b.force_to);
        }
        let bow_vel = if b.force > 0.001 { b.bow_vel } else { 0.0 };
        let gain = b.release.gain(held);
        let (x, xo, y) = b.string.bow_taps(b.written, (c, co), point);
        // The bow point's nulls, heard: outside the loop, so the pitch holds.
        let out = xo + comb * (y - xo);
        b.tone = if a < 1.0 {
            b.tone + a * (out - b.tone)
        } else {
            out
        };
        *s = b.tone;
        // Stick-slip: a small |Δv| sticks (energy in), a large one slips.
        let friction = b.force * 4.0 * fast_tanh((bow_vel - x) * b.slope);
        // Inverted each pass: two passes a period. Bounded: `x` under a
        // gain below 1, a bounded push, then `tanh`; once the bow is off,
        // linear, so DAMP's ring is its T60 at any level.
        let feedback = -x * gain.get() + friction * 0.4;
        let v = if b.force > 0.0 {
            fast_tanh(feedback)
        } else {
            feedback
        };
        if b.written == 0 {
            // A note's first push is its first pass's tap (#206): the
            // smoothing starts as if held at it, not 3 samples of silence.
            b.hair.past = [v; 6];
        }
        let y = b.hair.process(v);
        b.string.ring_push(y);
        b.written = b.written.saturating_add(1);
    }
}

/// Each halo string's T60 over the main string's.
const HALO_T60: f32 = 2.0;

/// SYMP's main string's ensemble rate: ENS RATE is STRING's alone.
const SYMP_ENS_RATE: f32 = 0.3;

/// The main string and, with a halo, the seven it sets ringing; bare, the
/// main string alone.
fn render_sympathetic(
    main: &mut StringVoice,
    set: Option<&mut SympatheticSet>,
    output: &mut [f32; BLOCK_SIZE],
    m: &Macros,
    f0: (f32, f32),
) {
    main.render(&main_string(m, f0), output);
    if let Some(set) = set {
        set.glide_step();
        // Each halo string rings twice the main one's T60, no darker.
        let lp = lp_coeff(halo_bright(m.bright));
        let halo_t60 = HALO_T60 * t60(m.damp);
        let halo = set.periods().map(|p| {
            let hz = (SAMPLE_RATE as f32 / p, core::f32::consts::TAU / p);
            let (lp, gain) = damped(halo_t60, hz, lp);
            KsRenderParams { lp, gain }
        });
        let (coupling, level) = (set.coupling, set.level);
        // The main string drives each halo string at its write position.
        let mut input = [0.0_f32; BLOCK_SIZE];
        for (i, x) in input.iter_mut().zip(output.iter()) {
            *i = x * coupling;
        }
        let mut sum = [0.0_f32; BLOCK_SIZE];
        let halo = set.strings.iter_mut().zip(&mut set.pending).zip(&halo);
        for ((sym, pending), p) in halo {
            sym.run_coupled(p, &input, pending, &mut sum);
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

/// `folded` or the octave above, whichever is nearer `near`: a glide by
/// the chord's interval, not round the fold.
fn octave_near(folded: f32, near: f32) -> f32 {
    if folded > near * core::f32::consts::SQRT_2 {
        folded * 0.5
    } else {
        folded
    }
}

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

    /// The fundamental's gain the DSP runs at C3, the loop's and its
    /// low-pass's, is the old DECAY's gain per pass: the DAMP law and its
    /// v1 inverse agree.
    #[test]
    fn old_decay_gain_survives_at_c3() {
        let w = TAU * 130.81 / 48_000.0;
        for decay in [0.2, 0.3, 0.6, 1.0] {
            let p = ModalParams {
                damp: damp_from_v1_decay(decay),
                ..Default::default()
            };
            let k = main_string(&Macros::of(&p), (130.81, w));
            let got = k.gain.get() * (1.0 - k.lp * (1.0 - libm::cosf(w)));
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

    /// BURST alone sets the strike's length, 2 to 6 ms; EXCITE its level.
    #[test]
    fn bank_burst_is_2_to_6_ms() {
        for excite in [0.2, 1.0] {
            for (burst, want) in [(0.0, 96), (1.0, 288)] {
                let p = ModalParams {
                    mode: ResonatorMode::Modal,
                    excite,
                    burst,
                    ..Default::default()
                };
                let mut pool = SymPool::boxed();
                let mut e = engine(&mut pool, p.mode);
                e.note_on(48, 100, &p, SR, &mut pool);
                let ModelSlot::Bank(b) = &e.model else {
                    unreachable!()
                };
                assert_eq!(b.burst_remaining, want, "EXCITE {excite}, BURST {burst}");
            }
        }
    }

    /// The bow's smoothing has no step in pitch: across any 2 % step from
    /// 300 to 900 Hz its made-up response moves under 0.04 at every
    /// frequency (0.033 at most; the hard switch at 520 Hz moved it 0.38 at
    /// 10.8 kHz), so no timbre jumps between neighbouring notes.
    #[test]
    fn the_bows_smoothing_moves_smoothly_with_pitch() {
        let hair = |f: f32| BowHair::REST.tuned(f, TAU * f / SR as f32);
        let mut f = 300.0;
        while f < 900.0 {
            let (a, b) = (hair(f), hair(f * 1.02));
            for k in 0..64 {
                let w = core::f32::consts::PI * k as f32 / 64.0;
                let d = (a.response(w) - b.response(w)).abs();
                assert!(d < 0.04, "{f} Hz, ω {w}: {d}");
            }
            f *= 1.02;
        }
    }

    /// Bowed writes round its ring, the loop and two; another note's
    /// note-on still starts silent (the same note's re-strike adds).
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
        e.note_on(95, 100, &p, SR, &mut pool);
        let ModelSlot::Bowed(b) = &e.model else {
            unreachable!()
        };
        assert!(b.string.line().0.iter().all(|&x| x == 0.0));
    }
}
