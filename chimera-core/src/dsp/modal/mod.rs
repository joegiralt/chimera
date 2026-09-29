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
mod q16;
mod rings;
mod string;

pub use params::*;
pub use q16::{Exp, Q16, StepBudget, Store};
pub use string::{KsRenderParams, MAX_STRING_DELAY};

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::hw::Cost;
use crate::in_place::{in_place_enum, uninit_at};
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
struct BowedString<S: Store> {
    string: KsString<S>,
    force: f32,
}

crate::in_place::field_list!(BowedString<Q16> => BowedString { string, force });

/// The main string and the seven it sets ringing.
struct SympatheticStrings<S: Store> {
    main: KsString<S>,
    strings: [KsString<S>; NUM_SYMPATHETIC],
    /// Each sympathetic string's ratio to the main one, set at note-on.
    ratios: [f32; NUM_SYMPATHETIC],
}

crate::in_place::field_list!(SympatheticStrings<Q16> => SympatheticStrings { main, strings, ratios });

in_place_enum! {
    /// The one model an engine holds: the variant is the mode.
    #[expect(
        clippy::large_enum_variant,
        reason = "one model per engine, built in place: boxing needs a heap"
    )]
    enum ModelSlot<S: Store> {
        Bank(ModalBank) => rebuild_bank, init_bank;
        String(KsString<S>) => rebuild_string, init_string;
        Bowed(BowedString<S>) => rebuild_bowed, init_bowed;
        Sympathetic(SympatheticStrings<S>) => rebuild_sympathetic, init_sympathetic;
    }
}

impl<S: Store> ModelSlot<S> {
    fn init_in_place(slot: &mut MaybeUninit<Self>, mode: ResonatorMode) -> &mut Self {
        match mode {
            // SAFETY: `ModalBank::init_in_place` writes every field.
            ResonatorMode::Modal => unsafe { Self::init_bank(slot, ModalBank::init_in_place) },
            // SAFETY: `KsString::init_in_place` writes every field.
            ResonatorMode::String => unsafe { Self::init_string(slot, KsString::init_in_place) },
            // SAFETY: `BowedString::init_in_place` writes every field.
            ResonatorMode::Bowed => unsafe { Self::init_bowed(slot, BowedString::init_in_place) },
            // SAFETY: `SympatheticStrings::init_in_place` writes every field.
            ResonatorMode::Sympathetic => unsafe {
                Self::init_sympathetic(slot, SympatheticStrings::init_in_place)
            },
        }
    }

    /// `mode`'s model, fresh, in place: the layout test's. A voice rebuilds
    /// its whole slot instead (`Voice::rebuild`, the one counted path).
    #[cfg(test)]
    fn rebuild(&mut self, mode: ResonatorMode) {
        match mode {
            // SAFETY: `ModalBank::init_in_place` writes every field.
            ResonatorMode::Modal => unsafe { self.rebuild_bank(ModalBank::init_in_place) },
            // SAFETY: `KsString::init_in_place` writes every field.
            ResonatorMode::String => unsafe { self.rebuild_string(KsString::init_in_place) },
            // SAFETY: `BowedString::init_in_place` writes every field.
            ResonatorMode::Bowed => unsafe { self.rebuild_bowed(BowedString::init_in_place) },
            // SAFETY: `SympatheticStrings::init_in_place` writes every field.
            ResonatorMode::Sympathetic => unsafe {
                self.rebuild_sympathetic(SympatheticStrings::init_in_place)
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

/// Modal, its strings' delay lines stored as `S` (ADR 0052).
pub struct ModalEngine<S: Store = Q16> {
    /// The model it plays, and all of that model's state.
    model: ModelSlot<S>,
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
const fn models_are_exclusive<S: Store>() -> bool {
    use core::mem::{align_of, size_of};
    let models = [
        size_of::<ModalBank>(),
        size_of::<KsString<S>>(),
        size_of::<BowedString<S>>(),
        size_of::<SympatheticStrings<S>>(),
    ];
    let mut largest = 0;
    let mut i = 0;
    while i < models.len() {
        if models[i] > largest {
            largest = models[i];
        }
        i += 1;
    }
    let align = align_of::<ModalEngine<S>>();
    let shared = size_of::<(f32, f32, f32, bool, bool, u32)>();
    size_of::<ModalEngine<S>>()
        <= (largest.next_multiple_of(align) + align + shared).next_multiple_of(align)
}
const _: () = assert!(models_are_exclusive::<F32>() && models_are_exclusive::<Q16>());

/// The f32 line: the store the 16-bit one is measured against.
type F32 = [f32; MAX_STRING_DELAY];

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
    /// tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(mode: ResonatorMode) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { crate::in_place::by_value(|slot| Self::init_in_place(slot, mode)) }
    }
}

impl<S: Store> ModalEngine<S> {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, mode: ResonatorMode) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the model is built in place,
        // every other field is written once by value, before
        // `assume_init_mut`.
        unsafe {
            ModelSlot::init_in_place(uninit_at(addr_of_mut!((*p).model)), mode);
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

    /// `params.mode` must be the model this engine holds: a voice rebuilds
    /// its slot into another (`Voice::rebuild`, ADR 0051).
    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32) {
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
                b.string.line.clear();
                b.force = vel * params.bow_force;
            }
            ModelSlot::Sympathetic(m) => {
                // Main string gets excitation
                m.main.set_freq(freq, sample_rate);
                m.main.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
                m.ratios = sympathetic_ratios(params.inharm);
                m.tune(freq, sample_rate);
                for sym in m.strings.iter_mut() {
                    // Sympathetic strings start silent — energy comes from main
                    sym.line.restart();
                    for i in 0..sym.delay_len {
                        sym.line.store(i, 0.0);
                    }
                    sym.write_pos = 0;
                }
            }
        }

        self.active = true;
        self.released = false;
        self.silence_counter = 0;
    }

    /// The strings follow a changed pitch ratio (per block, at a change
    /// only): a divide per string (`ModalEngine::PITCH`).
    fn retune(&mut self, sample_rate: u32) {
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
                m.tune(freq, sample_rate);
            }
        }
    }

    pub fn note_off(&mut self) {
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
                for sym in &mut m.strings {
                    sym.damp(1);
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
    ) {
        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        let mut max_level = 0.0_f32;

        let bank_freq = self.pitched(self.frequency);
        self.retune(sample_rate);
        // Every string of the voice shares it: one exponent step a block.
        let mut budget = StepBudget::one();

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
                render_string(
                    string,
                    output,
                    params,
                    self.released,
                    &mut max_level,
                    &mut budget,
                );
                false
            }
            ModelSlot::Bowed(b) => {
                render_bowed(b, output, params, &mut max_level, &mut budget);
                false
            }
            ModelSlot::Sympathetic(m) => {
                render_sympathetic(
                    m,
                    output,
                    params,
                    self.released,
                    &mut max_level,
                    &mut budget,
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

impl<S: Store> BowedString<S> {
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

impl<S: Store> SympatheticStrings<S> {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the eight strings are built in
        // place and `ratios` is written by value, before `assume_init_mut`.
        unsafe {
            KsString::init_in_place(uninit_at(addr_of_mut!((*p).main)));
            let sym = addr_of_mut!((*p).strings).cast::<KsString<S>>();
            for i in 0..NUM_SYMPATHETIC {
                KsString::init_in_place(uninit_at(sym.add(i)));
            }
            addr_of_mut!((*p).ratios).write([1.0; NUM_SYMPATHETIC]);
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

fn render_string<S: Store>(
    string: &mut KsString<S>,
    output: &mut [f32; BLOCK_SIZE],
    params: &ModalParams,
    released: bool,
    max_level: &mut f32,
    budget: &mut StepBudget,
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
        *s = string.tick_full(&render_params, budget);
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

fn render_bowed<S: Store>(
    b: &mut BowedString<S>,
    output: &mut [f32; BLOCK_SIZE],
    params: &ModalParams,
    max_level: &mut f32,
    budget: &mut StepBudget,
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
        let string_vel = string.line.load(read_pos);

        // Bow friction: stick-slip model.
        // When |delta_v| is small, bow sticks (high friction → energy in).
        // When |delta_v| is large, bow slips (low friction → string rings free).
        let delta_v = bow_vel - string_vel;
        let friction = bow_force * libm::tanhf(delta_v * 8.0);

        let feedback = string_vel * 0.9995 * release_decay + friction * 0.4;

        // Soft-limit to prevent blowup
        let clamped = libm::tanhf(feedback);

        string.line.store(string.write_pos, clamped);
        string.write_pos = (string.write_pos + 1) % MAX_STRING_DELAY;
        if string.write_pos == 0 {
            string.line.wrapped(budget);
        }

        *s = string_vel;
        *max_level = max_level.max(libm::fabsf(*s));
    }
}

fn render_sympathetic<S: Store>(
    m: &mut SympatheticStrings<S>,
    output: &mut [f32; BLOCK_SIZE],
    params: &ModalParams,
    released: bool,
    max_level: &mut f32,
    budget: &mut StepBudget,
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
        let main_out = m.main.tick_full(&main_params, budget);

        // 2. Couple main string output into sympathetic strings
        let sym_input = main_out * coupling;

        // 3. Tick all sympathetic strings, sum their output
        let mut sym_sum = 0.0_f32;
        for sym in &mut m.strings {
            // Inject coupled energy from main string into delay line
            let wp = sym.write_pos;
            sym.line.store(wp, sym.line.load(wp) + sym_input);
            // Tick the sympathetic string (with gentler damping)
            let sym_out = sym.tick_full(&sym_params, budget);
            sym_sum += sym_out;
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
    use std::format;
    use std::vec::Vec;

    use super::*;

    /// In `ModelSlot`'s declaration order: the tag.
    const MODES: [ResonatorMode; 4] = [
        ResonatorMode::Modal,
        ResonatorMode::String,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ];

    fn payload_addr<S: Store>(slot: &ModelSlot<S>) -> *const u8 {
        match slot {
            ModelSlot::Bank(b) => core::ptr::from_ref(b).cast(),
            ModelSlot::String(s) => core::ptr::from_ref(s).cast(),
            ModelSlot::Bowed(b) => core::ptr::from_ref(b).cast(),
            ModelSlot::Sympathetic(m) => core::ptr::from_ref(m).cast(),
        }
    }

    #[test]
    fn model_slot_layout_matches_repr() {
        for (from, first) in MODES.into_iter().enumerate() {
            let mut raw = Box::<ModelSlot<Q16>>::new_uninit();
            // SAFETY: the box is valid for `size_of::<ModelSlot>()` byte writes.
            unsafe {
                raw.as_mut_ptr()
                    .cast::<u8>()
                    .write_bytes(0xA5, size_of::<ModelSlot<Q16>>())
            };
            ModelSlot::init_in_place(&mut raw, first);
            // SAFETY: `init_in_place` built a valid slot in the box.
            let mut slot = unsafe { raw.assume_init() };
            // From each model to every other, and back.
            for k in 0..=MODES.len() {
                let i = (from + k) % MODES.len();
                if k > 0 {
                    slot.rebuild(MODES[i]);
                }
                assert_eq!(slot.mode(), MODES[i]);
                let (tag, payload) = slot.mirror_parts();
                assert_eq!(usize::from(tag), i, "{:?}", MODES[i]);
                assert_eq!(payload_addr(&slot), payload, "{:?}", MODES[i]);
            }
        }
    }

    const SR: u32 = 48_000;
    /// Blocks in a second, and samples in the 10 ms envelope window.
    const SECOND: usize = SR as usize / BLOCK_SIZE;
    const WINDOW: usize = 480;

    fn engine<S: Store>(mode: ResonatorMode) -> Box<ModalEngine<S>> {
        let mut raw = Box::<ModalEngine<S>>::new_uninit();
        ModalEngine::init_in_place(&mut raw, mode);
        // SAFETY: `init_in_place` built a valid engine in the box.
        unsafe { raw.assume_init() }
    }

    fn block<S: Store>(e: &mut ModalEngine<S>, p: &ModalParams) -> [f32; BLOCK_SIZE] {
        let mut out = [0.0; BLOCK_SIZE];
        e.render(&mut out, p, SR);
        out
    }

    /// `note` at velocity 100 for `blocks`, released before `release_block`:
    /// the output and the first block after which the voice is free.
    fn play<S: Store>(
        p: &ModalParams,
        note: u8,
        release_block: Option<usize>,
        blocks: usize,
    ) -> (Vec<f32>, Option<usize>) {
        let mut e = engine::<S>(p.mode);
        e.note_on(note, 100, p, SR);
        let (mut out, mut quiet) = (Vec::with_capacity(blocks * BLOCK_SIZE), None);
        for b in 0..blocks {
            if release_block == Some(b) {
                e.note_off();
            }
            out.extend(block(&mut e, p));
            if quiet.is_none() && !e.is_active() {
                quiet = Some(b);
            }
        }
        (out, quiet)
    }

    fn db(x: f32) -> f32 {
        20.0 * libm::log10f(x)
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    /// The largest envelope gap in dB, over windows where f32 reads above
    /// -60 dBFS.
    fn envelope_gap(q: &[f32], f: &[f32]) -> f32 {
        q.chunks(WINDOW)
            .zip(f.chunks(WINDOW))
            .filter(|(_, f)| peak(f) > 0.001)
            .map(|(q, f)| db(peak(q) / peak(f)).abs())
            .fold(0.0, f32::max)
    }

    /// ADR 0052's error bound: RMS of Q16 − f32 re full scale over the
    /// first second, FDBK 0.
    const ERROR_DBFS: f32 = -90.0;

    /// Sympathetic's own bound, for the high notes whose loops ring loud
    /// (ADR 0052). Its seven lines resonate at 0.65–0.8, so they sit at
    /// `Exp::START` (14), one LSB 6.1e-5 at every level. Each pass rounds
    /// in about 0.29 LSB RMS, and a 0.999 loop builds that power by
    /// 1/(1 − g²) ≈ 500: about 4e-4 a string, times 0.15 · √7 at the mix,
    /// a floor near −76 dBFS (C6 measures −73.9). A limit of 16-bit
    /// storage in that model, not of the exponent's steps.
    const SYMPATHETIC_ERROR_DBFS: f32 = -72.0;

    /// ADR 0052's gate: with FDBK 0, the 16-bit strings play what f32 ones
    /// do, to their error bound over a second, within 0.1 dB of envelope,
    /// and go quiet with them.
    #[test]
    fn strings_i16_match_f32() {
        let mut cases = Vec::new();
        for mode in [
            ResonatorMode::String,
            ResonatorMode::Bowed,
            ResonatorMode::Sympathetic,
        ] {
            for note in [31, 69, 84] {
                for decay in [0.0, 0.3] {
                    // Only the Sympathetic cases that measure below -90.
                    let loud_loops = mode == ResonatorMode::Sympathetic
                        && (note == 84 || (note == 69 && decay == 0.0));
                    let bound = if loud_loops {
                        SYMPATHETIC_ERROR_DBFS
                    } else {
                        ERROR_DBFS
                    };
                    let p = ModalParams {
                        mode,
                        decay,
                        ks_feedback: 0.0,
                        ..Default::default()
                    };
                    cases.push((p, note, bound));
                }
            }
        }
        let ensemble = ModalParams {
            ks_feedback: 0.0,
            ks_ens_mix: 0.5,
            ks_ens_depth: 0.5,
            ..Default::default()
        };
        cases.push((ensemble, 45, ERROR_DBFS));

        let mut misses = Vec::new();

        for (p, note, bound) in cases {
            let release = (p.mode == ResonatorMode::Bowed).then_some(SECOND / 2);
            let (q, q_quiet) = play::<Q16>(&p, note, release, 10 * SECOND);
            let (f, f_quiet) = play::<F32>(&p, note, release, 10 * SECOND);
            let n = SECOND * BLOCK_SIZE;
            let err = q[..n]
                .iter()
                .zip(&f[..n])
                .map(|(q, f)| (q - f) * (q - f))
                .sum::<f32>();
            let err = db(libm::sqrtf(err / n as f32));
            let gap = envelope_gap(&q, &f);
            let case = format!("{:?} {note} decay {}", p.mode, p.decay);
            let quiet = match (q_quiet, f_quiet) {
                (None, None) => true,
                (Some(q), Some(f)) => q.abs_diff(f) <= 15,
                _ => false,
            };
            let row = format!(
                "{case:<24} {err:7.2} dBFS  env {gap:.3} dB  quiet {q_quiet:?} / {f_quiet:?}"
            );
            std::eprintln!("{row}");
            if err > bound || gap > 0.1 || !quiet {
                misses.push(row);
            }
        }
        assert!(misses.is_empty(), "missed the gate: {misses:#?}");
    }

    /// With feedback the loop self-oscillates and amplifies any difference,
    /// so the gate is bounds and envelope, not samples.
    #[test]
    fn strings_i16_with_feedback_stay_bounded() {
        for ks_feedback in [0.2, 1.0] {
            for (mode, note) in [
                (ResonatorMode::String, 69),
                (ResonatorMode::Sympathetic, 57),
            ] {
                let p = ModalParams {
                    mode,
                    ks_feedback,
                    ..Default::default()
                };
                let (q, _) = play::<Q16>(&p, note, None, SECOND);
                let (f, _) = play::<F32>(&p, note, None, SECOND);
                let gap = envelope_gap(&q, &f);
                std::eprintln!("{mode:?} {note} fdbk {ks_feedback}  env {gap:.3} dB");
                assert!(
                    q.iter().all(|y| y.is_finite() && y.abs() <= 1.5),
                    "{mode:?} {ks_feedback}"
                );
                assert!(gap <= 2.0, "{mode:?} {ks_feedback}: envelope {gap} dB");
            }
        }
    }

    /// Samples past a short loop keep the level of the note before: a new
    /// note's restart rescales them, so a pitch drop that reads them back
    /// hears no burst.
    #[test]
    fn pitch_down_after_a_long_tail_reads_no_burst() {
        fn run<S: Store>(check: impl Fn(&ModalEngine<S>)) -> f32 {
            let p = ModalParams {
                decay: 0.0,
                ks_feedback: 0.0,
                ..Default::default()
            };
            let mut e = engine::<S>(p.mode);
            e.note_on(31, 100, &p, SR);
            for _ in 0..3 * SECOND {
                block(&mut e, &p);
            }
            check(&e);
            e.note_on(69, 100, &p, SR);
            block(&mut e, &p);
            e.set_pitch(0.25);
            (0..8).map(|_| peak(&block(&mut e, &p))).fold(0.0, f32::max)
        }
        let q = run::<Q16>(|e| match &e.model {
            ModelSlot::String(s) => {
                assert!(s.line.exp().get() > Exp::START.get(), "the tail stepped up")
            }
            _ => unreachable!(),
        });
        let f = run::<F32>(|_| {});
        assert!((q - f).abs() <= 0.01, "peak {q} / {f}");
    }

    /// Eight short loops wrap every block, but a voice steps at most one
    /// string's exponent a block.
    #[test]
    fn a_short_loop_steps_at_most_once_a_block() {
        let p = ModalParams {
            mode: ResonatorMode::Sympathetic,
            ..Default::default()
        };
        let mut e = engine::<Q16>(p.mode);
        e.note_on(96, 100, &p, SR);
        let exps = |e: &ModalEngine<Q16>| match &e.model {
            ModelSlot::Sympathetic(m) => core::array::from_fn::<_, 8, _>(|i| {
                if i == 0 {
                    m.main.line.exp()
                } else {
                    m.strings[i - 1].line.exp()
                }
            }),
            _ => unreachable!(),
        };
        let mut most = 0;
        for b in 0..2 * SECOND {
            let before = exps(&e);
            block(&mut e, &p);
            let stepped = before
                .iter()
                .zip(exps(&e))
                .filter(|(a, b)| **a != *b)
                .count();
            assert!(stepped <= 1, "block {b}: {stepped} strings stepped");
            most = most.max(stepped);
        }
        assert_eq!(most, 1, "no string ever stepped");
    }
}
