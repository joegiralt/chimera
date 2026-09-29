//! The Karplus-Strong string, the project owner's own code from their
//! Carcosa firmware for the Ambika, relicensed here under MIT (ADR 0032).

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use super::q16::{Q16, StepBudget, Store};
use crate::in_place::uninit_at;

// ── Karplus-Strong delay line (from the owner's Carcosa firmware) ───

/// String delay-line length (ADR 0040): the period of G1 (MIDI 31, 49.0 Hz)
/// at 48 kHz is 979 samples, so G1 and above play at their exact period;
/// lower notes clamp to 983 samples (~48.8 Hz). Sized so eight voices fit D2.
pub const MAX_STRING_DELAY: usize = 984;

/// Parameters for `KsString::tick_full`, built once per render block (not
/// per sample) at each call site.
/// damping: 0..1 (lowpass coefficient)
/// decay: 0..1 (AC attenuation rate)
/// body: 0..1 (half-delay comb resonance)
/// stiffness: 0..1 (allpass dispersion for bell character)
/// feedback: 0..1 (sustain boost)
/// ens_rate/ens_depth/ens_mix: ensemble chorus parameters
#[derive(Clone, Copy)]
pub struct KsRenderParams {
    pub damping: f32,
    pub decay: f32,
    pub body: f32,
    pub stiffness: f32,
    pub feedback: f32,
    pub ens_rate: f32,
    pub ens_depth: f32,
    pub ens_mix: f32,
}

/// The third ensemble head's fixed offset from the second.
const ENS_SPREAD: f32 = 0.3;

/// A string, its delay line stored as `S`.
pub(super) struct KsString<S: Store = Q16> {
    pub(super) line: S,
    pub(super) write_pos: usize,
    pub(super) delay_len: usize,
    ens_lfo_phase: u32,
    noise_state: u32,
}

impl<S: Store> KsString<S> {
    pub(super) fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the line is built in place
        // (`Store` is sealed: each `init_in_place` writes every field) and
        // every other field is written once by value, before
        // `assume_init_mut`.
        unsafe {
            S::init_in_place(uninit_at(addr_of_mut!((*p).line)));
            addr_of_mut!((*p).write_pos).write(0);
            addr_of_mut!((*p).delay_len).write(100);
            addr_of_mut!((*p).ens_lfo_phase).write(0);
            addr_of_mut!((*p).noise_state).write(0x8765_4321);
            slot.assume_init_mut()
        }
    }

    pub(super) fn set_freq(&mut self, freq: f32, sample_rate: u32) {
        let period = sample_rate as f32 / freq;
        self.delay_len = (period as usize).clamp(2, MAX_STRING_DELAY - 1);
    }

    /// Excite the string (Carcosa's Trigger).
    /// excitation: 0=noise, 1=click, 2=bright, 3=dark
    pub(super) fn trigger(&mut self, amplitude: f32, excitation: u8, color: f32, position: f32) {
        self.line.restart();
        // Fill delay line based on excitation type
        let mut prev = 0.0_f32;
        for i in 0..self.delay_len {
            let sample = match excitation % 4 {
                1 => {
                    // Click: short impulse
                    if i < 4 { amplitude } else { 0.0 }
                }
                2 => {
                    // Bright noise
                    let n1 = xorshift_noise(&mut self.noise_state);
                    let n2 = xorshift_noise(&mut self.noise_state);
                    (n1 * 0.5 + n2 * 0.25) * amplitude
                }
                3 => {
                    // Dark noise: average with previous
                    let n = xorshift_noise(&mut self.noise_state) * amplitude;
                    prev = (n + prev) * 0.5;
                    prev
                }
                _ => {
                    // White noise
                    xorshift_noise(&mut self.noise_state) * amplitude
                }
            };
            self.line.store(i, sample);
        }

        // Pluck position: comb notch at position harmonics
        if position > 0.03 {
            let notch_period = ((self.delay_len as f32 * position) as usize).max(2);
            if notch_period < self.delay_len {
                for i in 0..self.delay_len - notch_period {
                    let v = (self.line.load(i) + self.line.load(i + notch_period)) * 0.5;
                    self.line.store(i, v);
                }
            }
        }

        // Excitation color: low-pass filter passes (lower color = darker)
        let filter_passes = ((1.0 - color) * 7.0) as usize;
        for _ in 0..filter_passes {
            for i in 1..self.delay_len {
                let v = (self.line.load(i) + self.line.load(i - 1)) * 0.5;
                self.line.store(i, v);
            }
        }

        self.write_pos = 0;
        self.ens_lfo_phase = 0;
    }

    /// Damps the loop, each sample by 0.2 `passes` times: read and written
    /// once, the same f32 arithmetic as a pass at a time.
    pub(super) fn damp(&mut self, passes: u32) {
        for i in 0..self.delay_len {
            let mut x = self.line.load(i);
            for _ in 0..passes {
                x *= 0.2;
            }
            self.line.store(i, x);
        }
    }

    /// Full render with all the KS+ features. See `KsRenderParams` for
    /// the field meanings. A wrap may step the line's exponent, if the
    /// voice's `budget` for the block allows.
    #[inline]
    pub(super) fn tick_full(&mut self, p: &KsRenderParams, budget: &mut StepBudget) -> f32 {
        // Read position: one ahead of write
        let read_pos = (self.write_pos + 1) % self.delay_len;
        let current = self.line.load(read_pos);
        let next = self.line.load((read_pos + 1) % self.delay_len);

        // KS low-pass averaging: blend between current and next sample.
        // Higher coeff = more averaging = darker sound.
        // damping=0 (bright): coeff=0.05 (barely any filtering)
        // damping=1 (dark): coeff=0.5 (heavy filtering, fast decay)
        let coeff = 0.05 + p.damping * 0.45;
        let mut filtered = current * (1.0 - coeff) + next * coeff;

        // The 2-point average inherently decays the signal.
        // Apply a per-sample gain < 1.0 to control decay time.
        // decay=0 → gain=0.9990 (very long ring, ~7 seconds)
        // decay=1 → gain=0.9900 (short pluck, ~100ms)
        let gain = 0.999 - p.decay * 0.009;
        filtered *= gain;

        // Stiffness: mix with a sample from +7 offset (allpass-like dispersion)
        if p.stiffness > 0.01 {
            let stiff_pos = (read_pos + 7) % self.delay_len;
            let stiff_sample = self.line.load(stiff_pos);
            filtered = filtered * (1.0 - p.stiffness) + stiff_sample * p.stiffness;
        }

        // Body resonance: comb filter at half-delay
        if p.body > 0.03 {
            let body_pos = (read_pos + self.delay_len / 2) % self.delay_len;
            let body_sample = self.line.load(body_pos);
            filtered = filtered * (1.0 - p.body * 0.5) + body_sample * p.body * 0.5;
        }

        // Feedback boost for sustain (adds energy back, fights decay)
        // Only at high values does it approach infinite sustain.
        if p.feedback > 0.01 {
            filtered += filtered * p.feedback * 0.3;
            filtered = filtered.clamp(-1.5, 1.5);
        }

        // Write back
        let wrapped = read_pos <= self.write_pos;
        self.line.store(read_pos, filtered);
        self.write_pos = read_pos;
        if wrapped {
            self.line.wrapped(budget);
        }

        // Ensemble: three read heads with LFO detuning
        let mut output = filtered;
        if p.ens_mix > 0.01 && p.ens_depth > 0.01 {
            let lfo_inc = ((p.ens_rate + 0.01) * 1000.0) as u32;
            self.ens_lfo_phase = self.ens_lfo_phase.wrapping_add(lfo_inc);

            // Unipolar triangle: 0..1..0
            let lfo_raw = (self.ens_lfo_phase >> 16) as i16;
            let lfo_val = if self.ens_lfo_phase & 0x80000000 != 0 {
                -(lfo_raw as f32 / 32768.0)
            } else {
                lfo_raw as f32 / 32768.0
            };

            let offset2 = (lfo_val * p.ens_depth * self.delay_len as f32 * 0.05) as i32;
            let offset3 = -offset2 + (ENS_SPREAD * self.delay_len as f32 * 0.02) as i32;

            let p2 = ((read_pos as i32 + offset2).rem_euclid(self.delay_len as i32)) as usize;
            let p3 = ((read_pos as i32 + offset3).rem_euclid(self.delay_len as i32)) as usize;

            let head2 = self.line.load(p2);
            let head3 = self.line.load(p3);

            output = filtered * (1.0 - p.ens_mix) + (head2 + head3) * 0.5 * p.ens_mix;
        }

        output
    }
}

crate::in_place::field_list!(KsString => KsString { line, write_pos, delay_len, ens_lfo_phase, noise_state });

#[inline]
pub(super) fn xorshift_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as i32) as f32 / i32::MAX as f32
}
