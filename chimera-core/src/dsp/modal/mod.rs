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
// The Karplus-Strong string (`string.rs`) is the project owner's own code,
// from their Carcosa firmware for the Ambika, relicensed here under MIT
// (ADR 0032).

//! Modal, the physical-modelling engine (ADR 0004): a modal resonator bank
//! (`rings`) and Karplus-Strong strings (`string`) in four models.

mod params;
mod rings;
mod string;

pub use params::*;
pub use string::{KsRenderParams, MAX_STRING_DELAY};

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::hw::Cost;
use crate::in_place::{in_place_enum, uninit_at};
use crate::sym_alloc::{Lease, SYM_SLOTS, SymAlloc};
use rings::{CosineOsc, Svf, stiffness_from_structure};
use string::{KsString, xorshift_noise};

pub const MAX_MODES: usize = 48;

// ── Modal Engine (with String and Bowed modes) ──────────────────────

const NUM_SYMPATHETIC: usize = 7;

/// The resonator bank (`ResonatorMode::Modal`) and the noise burst that
/// strikes it.
struct ModalBank {
    filters: [Svf; MAX_MODES],
    cos_osc: CosineOsc,
    resolution: usize,
    /// Samples of burst left; the note sounds while any are.
    burst_remaining: usize,
    burst_amp: f32,
    noise_state: u32,
    /// The burst's one-pole lowpass.
    burst_lp: f32,
}

crate::in_place::field_list!(ModalBank => ModalBank {
    filters, cos_osc, resolution, burst_remaining, burst_amp, noise_state, burst_lp,
});

/// The bowed string and the bow's force on it, 0 once the bow lifts.
struct BowedString {
    string: KsString,
    force: f32,
}

crate::in_place::field_list!(BowedString => BowedString { string, force });

/// One pool slot (exclusive-state spec § 4.2): the seven strings a
/// Sympathetic note's main string sets ringing. The main string is the
/// voice's (`SympatheticVoice`).
pub struct SympatheticSet {
    strings: [KsString; NUM_SYMPATHETIC],
    /// Each sympathetic string's ratio to the main one, set at note-on.
    ratios: [f32; NUM_SYMPATHETIC],
    /// Each sympathetic string's last output, not yet stored: it is stored
    /// with the next sample's coupled input (`KsString::tick_coupled`).
    pending: [f32; NUM_SYMPATHETIC],
}

crate::in_place::field_list!(SympatheticSet => SympatheticSet { strings, ratios, pending });

/// Sympathetic's sets, one per slot of `SymAlloc`, which lends them to
/// voices (ADR 0054). One per `Instrument`, in D2 beside the voices.
pub struct SymPool {
    alloc: SymAlloc,
    sets: [SympatheticSet; SYM_SLOTS],
}

crate::in_place::field_list!(SymPool => SymPool { alloc, sets });

/// Sympathetic's voice side: the main string, and the lease on the set it
/// sets ringing. A Sympathetic voice can't exist without a slot.
struct SympatheticVoice {
    main: KsString,
    lease: Lease,
}

crate::in_place::field_list!(SympatheticVoice => SympatheticVoice { main, lease });

// Sympathetic never sizes the voice (spec § 4.2): Bowed or String does.
const _: () =
    assert!(size_of::<SympatheticVoice>() <= max(size_of::<BowedString>(), size_of::<KsString>()));

const fn max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

/// Host sizes of the private model types, for `memory_budget_test`.
#[cfg(any(test, feature = "test-support"))]
pub mod layout {
    use core::mem::{align_of, size_of};

    pub const MODEL_SLOT: usize = size_of::<super::ModelSlot>();
    pub const MODEL_SLOT_ALIGN: usize = align_of::<super::ModelSlot>();
    pub const BOWED: usize = size_of::<super::BowedString>();
    pub const SYMPATHETIC_VOICE: usize = size_of::<super::SympatheticVoice>();
}

/// The model a `ModalEngine` is built to play. Sympathetic takes the lease
/// on its set by value: no lease, no Sympathetic.
pub enum Model {
    Bank,
    String,
    Bowed,
    Sympathetic(Lease),
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
        String(KsString) => rebuild_string, init_string;
        Bowed(BowedString) => rebuild_bowed, init_bowed;
        Sympathetic(SympatheticVoice) => rebuild_sympathetic, init_sympathetic;
    }
}

impl ModelSlot {
    fn init_in_place(slot: &mut MaybeUninit<Self>, model: Model) -> &mut Self {
        match model {
            // SAFETY: `ModalBank::init_in_place` writes every field.
            Model::Bank => unsafe { Self::init_bank(slot, ModalBank::init_in_place) },
            // SAFETY: `KsString::init_in_place` writes every field.
            Model::String => unsafe { Self::init_string(slot, KsString::init_in_place) },
            // SAFETY: `BowedString::init_in_place` writes every field.
            Model::Bowed => unsafe { Self::init_bowed(slot, BowedString::init_in_place) },
            // SAFETY: `SympatheticVoice::init_in_place` writes every field.
            Model::Sympathetic(lease) => unsafe {
                Self::init_sympathetic(slot, |s| SympatheticVoice::init_in_place(s, lease))
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
            // SAFETY: `KsString::init_in_place` writes every field.
            Model::String => unsafe { self.rebuild_string(KsString::init_in_place) },
            // SAFETY: `BowedString::init_in_place` writes every field.
            Model::Bowed => unsafe { self.rebuild_bowed(BowedString::init_in_place) },
            // SAFETY: `SympatheticVoice::init_in_place` writes every field.
            Model::Sympathetic(lease) => unsafe {
                self.rebuild_sympathetic(|s| SympatheticVoice::init_in_place(s, lease))
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
    released: bool, // true after note_off
    active: bool,
    silence_counter: u32,
}

crate::in_place::field_list!(ModalEngine => ModalEngine {
    model, frequency, pitch, tuned, released, active, silence_counter,
});

// A `ModalEngine` is its largest model plus the fields every model shares
// (`frequency`, `pitch`, `tuned`, `released`, `active`, `silence_counter`),
// never the sum of models. The slot's tag takes one align (`in_place_enum!`).
const fn models_are_exclusive() -> bool {
    use core::mem::{align_of, size_of};
    let models = [
        size_of::<ModalBank>(),
        size_of::<KsString>(),
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
    let shared = size_of::<(f32, f32, f32, bool, bool, u32)>();
    size_of::<ModalEngine>()
        <= (largest.next_multiple_of(align) + align + shared).next_multiple_of(align)
}
const _: () = assert!(models_are_exclusive());

impl ModalEngine {
    /// Cycles/sample per model (ADR 0013), at the chain's LP24. String is
    /// measured (bench `MODAL`, 2026-09-27, rev V at 480 MHz). The others
    /// are provisional until the bench's MDL rows read them (#49): the
    /// emulator's count over String's (1.36 cycles an instruction, 38 an
    /// I- or D-cache miss), scaled by String's bench/emulator ratio (1.07)
    /// and rounded up about 10 %. Emulator, per voice: String 235
    /// instructions a sample, 78 misses a block; Bowed 340, 115;
    /// Sympathetic 801, 174; the resonator bank 1,106, 148 at 32 modes and
    /// 1,539, 169 at 48.
    pub const COST_STRING: Cost = Cost(390);
    /// Estimated 565.
    pub const COST_BOWED: Cost = Cost(620);
    /// Estimated 1,274.
    pub const COST_SYMPATHETIC: Cost = Cost(1_400);
    /// The resonator bank: this plus `COST_MODE` per mode. Estimated 1,703
    /// at 32 modes and 40 a mode; billed 1,900 at 32.
    pub const COST_BANK: Cost = Cost(460);
    pub const COST_MODE: Cost = Cost(45);

    /// `p`'s model, as the voice plays it from its next note-on.
    pub fn cost(p: &ModalParams) -> Cost {
        match p.mode {
            ResonatorMode::String => Self::COST_STRING,
            ResonatorMode::Bowed => Self::COST_BOWED,
            ResonatorMode::Sympathetic => Self::COST_SYMPATHETIC,
            ResonatorMode::Modal => {
                Cost(Self::COST_BANK.0 + Self::COST_MODE.0 * resolution(p) as u32)
            }
        }
    }
    /// More with a route into PITCH or FINE: `retune`'s eight divides a
    /// block, the per-block `fast_exp2` and the retune's I-cache lines.
    /// Provisional, pending a bench row (#182): the emulator's Sympathetic row
    /// with an LFO on PITCH less the unrouted one: 3 instructions a sample
    /// and 9 I-cache misses a block, per voice (2026-09-28). Billed at 12.
    pub const PITCH: Cost = Cost(12);

    /// An idle engine set to play `mode`, by value, through the stack:
    /// tests only. Sympathetic borrows a slot of `pool` for voice 0.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_in(pool: &mut SymPool, mode: ResonatorMode) -> Self {
        let model = match mode {
            ResonatorMode::Sympathetic => {
                let v0 = crate::voice_alloc::VoiceIdx::ALL[0];
                Model::Sympathetic(pool.alloc.lend(v0).expect("a free slot"))
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
            addr_of_mut!((*p).released).write(false);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).silence_counter).write(0);
            slot.assume_init_mut()
        }
    }

    /// The model this engine plays: its last note-on's, else the one it
    /// was built for.
    pub fn mode(&self) -> ResonatorMode {
        self.model.mode()
    }

    /// The model the sounding note plays, set at its note-on.
    pub fn playing(&self) -> Option<ResonatorMode> {
        self.active.then(|| self.model.mode())
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
            ModelSlot::Sympathetic(m) => Some(&mut m.lease),
            ModelSlot::Bank(_) | ModelSlot::String(_) | ModelSlot::Bowed(_) => None,
        }
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

        match &mut self.model {
            ModelSlot::Bank(bank) => {
                bank.compute_filters(params, bank_freq);
                bank.cos_osc.init(params.position);
                let burst_ms = 2.0 + params.excite * 4.0;
                bank.burst_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
                bank.burst_amp = vel * params.excite;
                bank.burst_lp = 0.0;
            }
            ModelSlot::String(string) => {
                string.set_freq(freq, sample_rate);
                string.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
            }
            ModelSlot::Bowed(b) => {
                b.string.set_freq(freq, sample_rate);
                b.string.clear();
                b.force = vel * params.bow_force;
            }
            ModelSlot::Sympathetic(m) => {
                let set = pool.set(&m.lease);
                // Main string gets excitation
                m.main.set_freq(freq, sample_rate);
                m.main.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
                set.ratios = sympathetic_ratios(params.inharm);
                set.tune(freq, sample_rate);
                for sym in set.strings.iter_mut() {
                    // Sympathetic strings start silent — energy comes from
                    // main. A handed-over slot carries nothing of its last
                    // note (spec § 4.8).
                    sym.clear();
                    sym.write_pos = 0;
                }
                set.pending = [0.0; NUM_SYMPATHETIC];
            }
        }

        self.active = true;
        self.released = false;
        self.silence_counter = 0;
    }

    /// The strings follow a changed pitch ratio (per block, at a change
    /// only): a divide per string (`ModalEngine::PITCH`).
    fn retune(&mut self, sample_rate: u32, pool: &mut SymPool) {
        if self.pitch == self.tuned {
            return;
        }
        self.tuned = self.pitch;
        let freq = self.pitched(self.frequency * sample_rate as f32);
        match &mut self.model {
            ModelSlot::Bank(_) => {}
            ModelSlot::String(string) => string.set_freq(freq, sample_rate),
            ModelSlot::Bowed(b) => b.string.set_freq(freq, sample_rate),
            ModelSlot::Sympathetic(m) => {
                m.main.set_freq(freq, sample_rate);
                pool.set(&m.lease).tune(freq, sample_rate);
            }
        }
    }

    pub fn note_off(&mut self, pool: &mut SymPool) {
        self.released = true;
        match &mut self.model {
            ModelSlot::String(string) => {
                // Dampen the buffer heavily
                string.damp(3);
            }
            ModelSlot::Bowed(b) => {
                // Stop the bow — zero exciter, heavily dampen string
                b.force = 0.0;
                b.string.damp(5);
            }
            ModelSlot::Bank(_) => {}
            ModelSlot::Sympathetic(m) => {
                m.main.damp(1);
                let set = pool.set(&m.lease);
                for (sym, pending) in set.strings.iter_mut().zip(&mut set.pending) {
                    sym.damp(1);
                    // Its write position's sample, were it stored.
                    if sym.write_pos < sym.delay_len {
                        *pending *= 0.2;
                    }
                }
            }
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

        let bank_freq = self.pitched(self.frequency);
        self.retune(sample_rate, pool);

        // Whether the model is still exciting itself: silent or not, the
        // note sounds on.
        let exciting = match &mut self.model {
            ModelSlot::Bank(bank) => {
                // Recompute filters every block (Rings does this — allows live parameter changes)
                bank.compute_filters(params, bank_freq);
                bank.cos_osc.init(params.position);
                render_modal(bank, output, &mut max_level);
                bank.burst_remaining > 0
            }
            ModelSlot::String(string) => {
                render_string(string, output, params, self.released, &mut max_level);
                false
            }
            ModelSlot::Bowed(b) => {
                render_bowed(b, output, params, &mut max_level);
                false
            }
            ModelSlot::Sympathetic(m) => {
                let m = &mut **m;
                let set = pool.set(&m.lease);
                render_sympathetic(
                    &mut m.main,
                    set,
                    output,
                    params,
                    self.released,
                    &mut max_level,
                );
                false
            }
        };

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
            addr_of_mut!((*p).burst_remaining).write(0);
            addr_of_mut!((*p).burst_amp).write(0.0);
            addr_of_mut!((*p).noise_state).write(0x1234_5678);
            addr_of_mut!((*p).burst_lp).write(0.0);
            slot.assume_init_mut()
        }
    }

    /// Configure filters — called every render block (not just note_on).
    /// Matches Rings' ComputeFilters().
    fn compute_filters(&mut self, params: &ModalParams, frequency: f32) {
        let num = resolution(params);
        self.resolution = num;

        // Q from decay (Rings-style range).
        // At partial_freq=0.003 (130Hz): mode_q = 1 + 0.003 * q
        //   decay=0:   q=500,    mode_q=2.5  (short ping)
        //   decay=0.5: q=50000,  mode_q=151  (nice ring)
        //   decay=1:   q=500000, mode_q=1501 (long sustain)
        let mut q = 500.0 * libm::powf(10.0, params.decay * 3.0); // 500..500,000

        // Stiffness from structure/inharm
        let mut stiffness = stiffness_from_structure(params.inharm);

        // Brightness → q_loss per mode (Rings formula)
        let structure = params.inharm;
        let bright_atten = {
            let x = 1.0 - structure;
            let x2 = x * x;
            x2 * x2 * x2 * x2
        };
        let brightness = params.brightness * (1.0 - 0.2 * bright_atten);
        let mut q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;
        let q_loss_damping_rate = structure * (2.0 - structure) * 0.1;

        let mut harmonic = frequency;
        let mut stretch_factor = 1.0_f32;

        for filter in self.filters.iter_mut().take(num) {
            let partial_freq = (harmonic * stretch_factor).min(0.49);

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
    }
}

impl BowedString {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the string is built in place
        // and `force` written by value, before `assume_init_mut`.
        unsafe {
            KsString::init_in_place(uninit_at(addr_of_mut!((*p).string)));
            addr_of_mut!((*p).force).write(0.0);
            slot.assume_init_mut()
        }
    }
}

impl SympatheticVoice {
    fn init_in_place(slot: &mut MaybeUninit<Self>, lease: Lease) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the main string is built in
        // place and the lease (one byte) written by value, before
        // `assume_init_mut`.
        unsafe {
            KsString::init_in_place(uninit_at(addr_of_mut!((*p).main)));
            addr_of_mut!((*p).lease).write(lease);
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

    pub(crate) fn alloc_mut(&mut self) -> &mut SymAlloc {
        &mut self.alloc
    }

    /// The set `lease` names. It borrows the pool and the lease, so no
    /// other set is reachable meanwhile and the lease can't go back.
    fn set(&mut self, lease: &Lease) -> &mut SympatheticSet {
        &mut self.sets[lease.slot().index()]
    }
}

impl SympatheticSet {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the seven strings are built
        // in place and `ratios` and `pending` are written by value, before
        // `assume_init_mut`.
        unsafe {
            let sym = addr_of_mut!((*p).strings).cast::<KsString>();
            for i in 0..NUM_SYMPATHETIC {
                KsString::init_in_place(uninit_at(sym.add(i)));
            }
            addr_of_mut!((*p).ratios).write([1.0; NUM_SYMPATHETIC]);
            addr_of_mut!((*p).pending).write([0.0; NUM_SYMPATHETIC]);
            slot.assume_init_mut()
        }
    }

    /// The sympathetic strings at their note-on ratios to `freq`.
    fn tune(&mut self, freq: f32, sample_rate: u32) {
        for (sym, r) in self.strings.iter_mut().zip(self.ratios) {
            sym.set_freq(freq * r, sample_rate);
        }
    }
}

fn render_modal(bank: &mut ModalBank, output: &mut [f32; BLOCK_SIZE], max_level: &mut f32) {
    let num = bank.resolution;
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

        // Sum to mono, scale up, soft-limit
        *s = libm::tanhf(odd + even) * 2.0;
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

fn render_string(
    string: &mut KsString,
    output: &mut [f32; BLOCK_SIZE],
    params: &ModalParams,
    released: bool,
    max_level: &mut f32,
) {
    let (fb, body, stiff, decay) = if released {
        (0.0, 0.0, 0.0, 0.8_f32.max(params.decay)) // fast decay on release
    } else {
        (
            params.ks_feedback,
            params.ks_body,
            params.ks_stiffness,
            params.decay,
        )
    };
    let render_params = KsRenderParams {
        damping: params.brightness,
        decay,
        body,
        stiffness: stiff,
        feedback: fb,
        ens_rate: params.ks_ens_rate,
        ens_depth: params.ks_ens_depth,
        ens_mix: params.ks_ens_mix,
    };
    for s in output.iter_mut() {
        *s = string.tick_full(&render_params);
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

fn render_bowed(
    b: &mut BowedString,
    output: &mut [f32; BLOCK_SIZE],
    params: &ModalParams,
    max_level: &mut f32,
) {
    let (string, exciter_amp) = (&mut b.string, b.force);
    let bow_vel = if exciter_amp > 0.001 {
        params.bow_velocity * 0.3
    } else {
        0.0
    };
    let bow_force = exciter_amp * 4.0;
    // When bow is released, apply decay
    let release_decay = if exciter_amp < 0.001 { 0.995 } else { 1.0 };

    for s in output.iter_mut() {
        // Read from delay line
        let read_pos = (string.write_pos + MAX_STRING_DELAY - string.delay_len) % MAX_STRING_DELAY;
        let string_vel = string.buffer[read_pos];

        // Bow friction: stick-slip model.
        // When |delta_v| is small, bow sticks (high friction → energy in).
        // When |delta_v| is large, bow slips (low friction → string rings free).
        let delta_v = bow_vel - string_vel;
        let friction = bow_force * libm::tanhf(delta_v * 8.0);

        let feedback = string_vel * 0.9995 * release_decay + friction * 0.4;

        // Soft-limit to prevent blowup
        let clamped = libm::tanhf(feedback);

        string.buffer[string.write_pos] = clamped;
        string.write_pos = (string.write_pos + 1) % MAX_STRING_DELAY;

        *s = string_vel;
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

fn render_sympathetic(
    main: &mut KsString,
    set: &mut SympatheticSet,
    output: &mut [f32; BLOCK_SIZE],
    params: &ModalParams,
    released: bool,
    max_level: &mut f32,
) {
    let (fb, body, stiff) = if released {
        (0.0, 0.0, 0.0)
    } else {
        (params.ks_feedback, params.ks_body, params.ks_stiffness)
    };
    let decay = if released {
        0.8_f32.max(params.decay)
    } else {
        params.decay
    };

    // Coupling gain: how much main string feeds into sympathetic
    let coupling = 0.025; // Rings uses 0.2 / num_strings

    let main_params = KsRenderParams {
        damping: params.brightness,
        decay,
        body,
        stiffness: stiff,
        feedback: fb,
        ens_rate: params.ks_ens_rate,
        ens_depth: params.ks_ens_depth,
        ens_mix: params.ks_ens_mix,
    };
    let sym_params = KsRenderParams {
        damping: params.brightness * 0.7, // darker
        decay: decay * 0.5,               // slower decay
        body: 0.0,
        stiffness: 0.0,
        feedback: 0.0, // no body/stiff/feedback
        ens_rate: 0.0,
        ens_depth: 0.0,
        ens_mix: 0.0, // no ensemble
    };

    for s in output.iter_mut() {
        // 1. Main string tick
        let main_out = main.tick_full(&main_params);

        // 2. Couple main string output into sympathetic strings
        let sym_input = main_out * coupling;

        // 3. Tick all sympathetic strings, sum their output
        let mut sym_sum = 0.0_f32;
        for (sym, pending) in set.strings.iter_mut().zip(&mut set.pending) {
            // Inject coupled energy from the main string at the write
            // position, and tick with gentler damping.
            sym_sum += sym.tick_coupled(&sym_params, sym_input, pending);
        }

        // 4. Mix: main + sympathetic
        let mixed = main_out + sym_sum * 0.15;
        *s = libm::tanhf(mixed);
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

use super::note_to_freq;

/// The bank's mode count: `num_modes`, even, at most `MAX_MODES`.
fn resolution(p: &ModalParams) -> usize {
    (p.num_modes as usize).min(MAX_MODES) & !1
}

/// The sympathetic strings' ratios to the main one: harmonics/intervals
/// spread by `inharm`, 0 unison, 1 a wide harmonic series.
fn sympathetic_ratios(inharm: f32) -> [f32; NUM_SYMPATHETIC] {
    let intervals = [0.0, 12.0, 7.02, 12.0, 19.02, 24.0, 7.02];
    intervals.map(|st| libm::powf(2.0, st * inharm / 12.0))
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
                Model::Sympathetic(SymAlloc::new().lend(VoiceIdx::ALL[0]).expect("free"))
            }
            other => Model::resting(other),
        }
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
                Model::Sympathetic(pool.alloc.lend(VoiceIdx::ALL[0]).expect("free"))
            }
            other => Model::resting(other),
        };
        let mut raw = Box::<ModalEngine>::new_uninit();
        ModalEngine::init_in_place(&mut raw, model);
        // SAFETY: `init_in_place` built a valid engine in the box.
        unsafe { raw.assume_init() }
    }

    /// Every sample of a line past the loop reads silent.
    fn tail_is_silent(s: &KsString) -> bool {
        s.buffer[s.delay_len..].iter().all(|&x| x == 0.0)
    }

    /// A note-on's worst case, pinned: Sympathetic clears its eight lines,
    /// one memset of 3,936 B each, whatever the last note left in them.
    #[test]
    fn note_on_clears_every_line_at_a_fixed_cost() {
        assert_eq!(
            (1 + NUM_SYMPATHETIC) * size_of::<[f32; MAX_STRING_DELAY]>(),
            31_488
        );
        let p = ModalParams {
            mode: ResonatorMode::Sympathetic,
            decay: 0.0,
            ks_feedback: 0.0,
            ..Default::default()
        };
        let mut pool = SymPool::boxed();
        let mut e = engine(&mut pool, p.mode);
        e.note_on(31, 100, &p, SR, &mut pool);
        for _ in 0..SECOND {
            let mut out = [0.0; BLOCK_SIZE];
            e.render(&mut out, &p, SR, &mut pool);
        }
        let lines = |e: &ModalEngine, pool: &SymPool| match &e.model {
            ModelSlot::Sympathetic(m) => core::iter::once(&m.main)
                .chain(&pool.sets[m.lease.slot().index()].strings)
                .map(|s| (s.buffer.iter().any(|&x| x != 0.0), tail_is_silent(s)))
                .collect::<Vec<_>>(),
            _ => unreachable!(),
        };
        assert!(
            lines(&e, &pool).iter().all(|(rings, _)| *rings),
            "every line rang"
        );
        e.note_on(96, 100, &p, SR, &mut pool);
        for (i, (_, silent)) in lines(&e, &pool).into_iter().enumerate() {
            assert!(silent, "line {i}");
        }
    }
}
