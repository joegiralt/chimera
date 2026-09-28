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
use crate::in_place::{by_value, uninit_at};
use rings::{CosineOsc, Svf, stiffness_from_structure};
use string::{KsString, xorshift_noise};

pub const MAX_MODES: usize = 48;

// ── Modal Engine (with String and Bowed modes) ──────────────────────

const NUM_SYMPATHETIC: usize = 7;

pub struct ModalEngine {
    filters: [Svf; MAX_MODES],
    cos_osc: CosineOsc,
    resolution: usize,
    // String model (main)
    string: KsString,
    // Sympathetic strings (7 additional resonators)
    sym_strings: [KsString; NUM_SYMPATHETIC],
    // Shared
    frequency: f32,
    active_mode: ResonatorMode,
    released: bool, // true after note_off
    exciter_remaining: usize,
    exciter_amp: f32,
    noise_state: u32,
    exciter_lp: f32,
    active: bool,
    silence_counter: u32,
}

crate::in_place::field_list!(ModalEngine => ModalEngine {
    filters, cos_osc, resolution, string, sym_strings, frequency, active_mode,
    released, exciter_remaining, exciter_amp, noise_state, exciter_lp, active, silence_counter,
});

impl Default for ModalEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalEngine {
    pub const COST: Cost = Cost(390); // measured 2026-09-27, bench, rev V at 480 MHz

    pub fn new() -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(Self::init_in_place) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the eight strings are built in
        // place, every other field (the largest, `filters`, is 960 B) is
        // written once by value, before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).filters).write(core::array::from_fn(|_| Svf::new()));
            addr_of_mut!((*p).cos_osc).write(CosineOsc::new());
            addr_of_mut!((*p).resolution).write(0);
            KsString::init_in_place(uninit_at(addr_of_mut!((*p).string)));
            let sym = addr_of_mut!((*p).sym_strings).cast::<KsString>();
            for i in 0..NUM_SYMPATHETIC {
                KsString::init_in_place(uninit_at(sym.add(i)));
            }
            addr_of_mut!((*p).frequency).write(220.0 / 48000.0);
            addr_of_mut!((*p).active_mode).write(ResonatorMode::Modal);
            addr_of_mut!((*p).released).write(false);
            addr_of_mut!((*p).exciter_remaining).write(0);
            addr_of_mut!((*p).exciter_amp).write(0.0);
            addr_of_mut!((*p).noise_state).write(0x1234_5678);
            addr_of_mut!((*p).exciter_lp).write(0.0);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).silence_counter).write(0);
            slot.assume_init_mut()
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32) {
        self.active_mode = params.mode;
        let vel = velocity as f32 / 127.0;
        let freq = note_to_freq(note);
        self.frequency = freq / sample_rate as f32;

        match self.active_mode {
            ResonatorMode::Modal => {
                self.compute_filters(params, self.frequency);
                self.cos_osc.init(params.position);
                let burst_ms = 2.0 + params.excite * 4.0;
                self.exciter_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
                self.exciter_amp = vel * params.excite;
                self.exciter_lp = 0.0;
            }
            ResonatorMode::String => {
                self.string.set_freq(freq, sample_rate);
                self.string.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
            }
            ResonatorMode::Bowed => {
                self.string.set_freq(freq, sample_rate);
                for s in self.string.buffer.iter_mut() {
                    *s = 0.0;
                }
                self.exciter_amp = vel * params.bow_force;
            }
            ResonatorMode::Sympathetic => {
                // Main string gets excitation
                self.string.set_freq(freq, sample_rate);
                self.string.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
                // Sympathetic strings tuned to harmonics/intervals
                // inharm controls spread: 0=unison, 1=wide harmonic series
                let intervals = [0.0, 12.0, 7.02, 12.0, 19.02, 24.0, 7.02];
                for (i, sym) in self.sym_strings.iter_mut().enumerate() {
                    let detune = intervals[i] * params.inharm;
                    let sym_freq = freq * semitones_to_ratio(detune);
                    sym.set_freq(sym_freq, sample_rate);
                    // Sympathetic strings start silent — energy comes from main
                    for s in sym.buffer[..sym.delay_len].iter_mut() {
                        *s = 0.0;
                    }
                    sym.write_pos = 0;
                }
            }
        }

        self.active = true;
        self.released = false;
        self.silence_counter = 0;
    }

    pub fn note_off(&mut self) {
        self.released = true;
        match self.active_mode {
            ResonatorMode::String => {
                // Dampen the buffer heavily
                for _ in 0..3 {
                    for i in 0..self.string.delay_len {
                        self.string.buffer[i] *= 0.2;
                    }
                }
            }
            ResonatorMode::Bowed => {
                // Stop the bow — zero exciter, heavily dampen string
                self.exciter_amp = 0.0;
                for _ in 0..5 {
                    for i in 0..self.string.delay_len {
                        self.string.buffer[i] *= 0.2;
                    }
                }
            }
            ResonatorMode::Modal => {}
            ResonatorMode::Sympathetic => {
                for i in 0..self.string.delay_len {
                    self.string.buffer[i] *= 0.2;
                }
                for sym in &mut self.sym_strings {
                    for i in 0..sym.delay_len {
                        sym.buffer[i] *= 0.2;
                    }
                }
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Configure filters — called every render block (not just note_on).
    /// Matches Rings' ComputeFilters().
    fn compute_filters(&mut self, params: &ModalParams, frequency: f32) {
        let num = (params.num_modes as usize).min(MAX_MODES) & !1;
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

    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        _sample_rate: u32,
    ) {
        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        let mut max_level = 0.0_f32;

        // Recompute filters every block (Rings does this — allows live parameter changes)
        if self.active_mode == ResonatorMode::Modal {
            self.compute_filters(params, self.frequency);
            self.cos_osc.init(params.position);
        }

        match self.active_mode {
            ResonatorMode::String => self.render_string(output, params, &mut max_level),
            ResonatorMode::Modal => self.render_modal(output, &mut max_level),
            ResonatorMode::Bowed => self.render_bowed(output, params, &mut max_level),
            ResonatorMode::Sympathetic => self.render_sympathetic(output, params, &mut max_level),
        }

        if max_level < 0.001 && self.exciter_remaining == 0 {
            self.silence_counter += 1;
            if self.silence_counter > 10 {
                self.active = false;
            }
        } else {
            self.silence_counter = 0;
        }
    }

    fn render_modal(&mut self, output: &mut [f32; BLOCK_SIZE], max_level: &mut f32) {
        let num = self.resolution;
        for s in output.iter_mut() {
            let excite = if self.exciter_remaining > 0 {
                self.exciter_remaining -= 1;
                let env = (self.exciter_remaining as f32 / 200.0).min(1.0);
                let raw = xorshift_noise(&mut self.noise_state) * self.exciter_amp * env;
                self.exciter_lp += 0.4 * (raw - self.exciter_lp);
                self.exciter_lp
            } else {
                0.0
            };

            // Rings scales external audio input by 0.125. Our internal exciter
            // is already at the right level — no additional scaling needed.
            let input = excite;

            let mut odd = 0.0_f32;
            let mut even = 0.0_f32;
            self.cos_osc.start();

            let mut i = 0;
            while i + 1 < num {
                odd += self.cos_osc.next() * self.filters[i].process_bp(input);
                even += self.cos_osc.next() * self.filters[i + 1].process_bp(input);
                i += 2;
            }

            // Sum to mono, scale up, soft-limit
            *s = libm::tanhf(odd + even) * 2.0;
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_string(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        max_level: &mut f32,
    ) {
        let (fb, body, stiff, decay) = if self.released {
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
            *s = self.string.tick_full(&render_params);
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_bowed(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        max_level: &mut f32,
    ) {
        let bow_vel = if self.exciter_amp > 0.001 {
            params.bow_velocity * 0.3
        } else {
            0.0
        };
        let bow_force = self.exciter_amp * 4.0;
        // When bow is released, apply decay
        let release_decay = if self.exciter_amp < 0.001 { 0.995 } else { 1.0 };

        for s in output.iter_mut() {
            // Read from delay line
            let read_pos = (self.string.write_pos + MAX_STRING_DELAY - self.string.delay_len)
                % MAX_STRING_DELAY;
            let string_vel = self.string.buffer[read_pos];

            // Bow friction: stick-slip model.
            // When |delta_v| is small, bow sticks (high friction → energy in).
            // When |delta_v| is large, bow slips (low friction → string rings free).
            let delta_v = bow_vel - string_vel;
            let friction = bow_force * libm::tanhf(delta_v * 8.0);

            let feedback = string_vel * 0.9995 * release_decay + friction * 0.4;

            // Soft-limit to prevent blowup
            let clamped = libm::tanhf(feedback);

            self.string.buffer[self.string.write_pos] = clamped;
            self.string.write_pos = (self.string.write_pos + 1) % MAX_STRING_DELAY;

            *s = string_vel;
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_sympathetic(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        max_level: &mut f32,
    ) {
        let released = self.released;
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
            let main_out = self.string.tick_full(&main_params);

            // 2. Couple main string output into sympathetic strings
            let sym_input = main_out * coupling;

            // 3. Tick all sympathetic strings, sum their output
            let mut sym_sum = 0.0_f32;
            for sym in &mut self.sym_strings {
                // Inject coupled energy from main string into delay line
                let wp = sym.write_pos;
                sym.buffer[wp] += sym_input;
                // Tick the sympathetic string (with gentler damping)
                let sym_out = sym.tick_full(&sym_params);
                sym_sum += sym_out;
            }

            // 4. Mix: main + sympathetic
            let mixed = main_out + sym_sum * 0.15;
            *s = libm::tanhf(mixed);
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }
}

use super::note_to_freq;

fn semitones_to_ratio(semitones: f32) -> f32 {
    libm::powf(2.0, semitones / 12.0)
}
