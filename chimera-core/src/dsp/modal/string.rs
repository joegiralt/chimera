//! The Karplus-Strong string, the project owner's own code from their
//! Carcosa firmware for the Ambika, relicensed here under MIT (ADR 0032).

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

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

/// A fresh line's loop, before its first `set_freq`.
const INIT_LEN: usize = 100;
/// What a fresh line's first `clear` writes.
pub(super) const FRESH_CLEAR_BYTES: usize = INIT_LEN * size_of::<f32>();
/// The most any line's `clear` writes: its whole ring.
pub(super) const RING_BYTES: usize = MAX_STRING_DELAY * size_of::<f32>();

/// The third ensemble head's fixed offset from the second.
const ENS_SPREAD: f32 = 0.3;

/// A delay line and its dirty extent: every sample at or past `dirty`
/// reads 0.0, so a clear zeros `[0, dirty)` only. `dirty` covers the
/// loop and the write position (`dirty ≥ delay_len`, `dirty > write_pos`):
/// every store lands below it, and only `set_freq`, `clear` and
/// `ring_push` move it.
pub(super) struct KsString {
    buffer: [f32; MAX_STRING_DELAY],
    write_pos: usize,
    delay_len: usize,
    dirty: usize,
    ens_lfo_phase: u32,
    noise_state: u32,
}

impl KsString {
    pub(super) fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the 3.9 KB `[f32]` buffer is
        // zero-filled (zero bytes are 0.0) and every other field is written
        // once by value before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).buffer).write_bytes(0, 1);
            addr_of_mut!((*p).write_pos).write(0);
            addr_of_mut!((*p).delay_len).write(INIT_LEN);
            addr_of_mut!((*p).dirty).write(INIT_LEN);
            addr_of_mut!((*p).ens_lfo_phase).write(0);
            addr_of_mut!((*p).noise_state).write(0x8765_4321);
            slot.assume_init_mut()
        }
    }

    pub(super) fn set_freq(&mut self, freq: f32, sample_rate: u32) {
        let period = sample_rate as f32 / freq;
        self.delay_len = (period as usize).clamp(2, MAX_STRING_DELAY - 1);
        self.dirty = self.dirty.max(self.delay_len);
    }

    pub(super) fn delay_len(&self) -> usize {
        self.delay_len
    }

    pub(super) fn write_pos(&self) -> usize {
        self.write_pos
    }

    /// Excite the string at `freq` (Carcosa's Trigger).
    /// excitation: 0=noise, 1=click, 2=bright, 3=dark
    /// The line is cleared first, before the new loop's length widens its
    /// extent: the clear writes `clear_bytes`, and the old note's samples
    /// past the new loop are never read back, even by a pitch drop that
    /// lengthens it.
    pub(super) fn trigger(
        &mut self,
        (freq, sample_rate): (f32, u32),
        amplitude: f32,
        excitation: u8,
        color: f32,
        position: f32,
    ) {
        self.clear();
        self.set_freq(freq, sample_rate);
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
            self.buffer[i] = sample;
        }

        // Pluck position: comb notch at position harmonics
        if position > 0.03 {
            let notch_period = ((self.delay_len as f32 * position) as usize).max(2);
            if notch_period < self.delay_len {
                for i in 0..self.delay_len - notch_period {
                    self.buffer[i] = (self.buffer[i] + self.buffer[i + notch_period]) * 0.5;
                }
            }
        }

        // Excitation color: low-pass filter passes (lower color = darker)
        let filter_passes = ((1.0 - color) * 7.0) as usize;
        for _ in 0..filter_passes {
            for i in 1..self.delay_len {
                self.buffer[i] = (self.buffer[i] + self.buffer[i - 1]) * 0.5;
            }
        }

        self.write_pos = 0;
        self.ens_lfo_phase = 0;
    }

    /// Zeros the whole ring, not just the loop: a note starts on a silent
    /// line, even once a pitch drop lengthens it. Only `[0, dirty)` can
    /// hold anything, so only it is written: a line last played high
    /// clears in a fraction of the ring.
    pub(super) fn clear(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        cleared::add(self.clear_bytes());
        self.buffer[..self.dirty].fill(0.0);
        self.dirty = self.delay_len.max(self.write_pos + 1);
    }

    /// What the next `clear` writes.
    pub(super) fn clear_bytes(&self) -> usize {
        self.dirty * size_of::<f32>()
    }

    /// Silent, writing from the start again.
    pub(super) fn restart(&mut self) {
        self.clear();
        self.write_pos = 0;
    }

    /// Bowed's ring, the whole buffer: the sample `delay_len` behind the
    /// write position.
    pub(super) fn ring_tap(&self) -> f32 {
        self.buffer[(self.write_pos + MAX_STRING_DELAY - self.delay_len) % MAX_STRING_DELAY]
    }

    /// Bowed's ring: stores `x` and steps on round the whole buffer.
    pub(super) fn ring_push(&mut self, x: f32) {
        self.buffer[self.write_pos] = x;
        self.write_pos = (self.write_pos + 1) % MAX_STRING_DELAY;
        self.dirty = self.dirty.max(self.write_pos + 1);
    }

    /// The line and its dirty extent: for the tests.
    #[cfg(test)]
    pub(super) fn line(&self) -> (&[f32; MAX_STRING_DELAY], usize) {
        (&self.buffer, self.dirty)
    }

    /// The next clear zeros the whole ring, as before the dirty extent:
    /// the tests' reference.
    #[cfg(test)]
    pub(super) fn soil(&mut self) {
        self.dirty = MAX_STRING_DELAY;
    }

    /// Damps the loop, each sample by 0.2 `passes` times: read and written
    /// once, the same f32 arithmetic as a pass at a time.
    pub(super) fn damp(&mut self, passes: u32) {
        for x in &mut self.buffer[..self.delay_len] {
            for _ in 0..passes {
                *x *= 0.2;
            }
        }
    }

    /// Full render with all the KS+ features. See `KsRenderParams` for
    /// the field meanings.
    #[inline]
    pub(super) fn tick_full(&mut self, p: &KsRenderParams) -> f32 {
        // Read position: one ahead of write
        let read_pos = (self.write_pos + 1) % self.delay_len;
        let current = self.buffer[read_pos];
        let next = self.buffer[(read_pos + 1) % self.delay_len];
        let mut filtered = lowpass(p, current, next);

        // Stiffness: mix with a sample from +7 offset (allpass-like dispersion)
        if p.stiffness > 0.01 {
            let stiff_pos = (read_pos + 7) % self.delay_len;
            let stiff_sample = self.buffer[stiff_pos];
            filtered = filtered * (1.0 - p.stiffness) + stiff_sample * p.stiffness;
        }

        // Body resonance: comb filter at half-delay
        if p.body > 0.03 {
            let body_pos = (read_pos + self.delay_len / 2) % self.delay_len;
            let body_sample = self.buffer[body_pos];
            filtered = filtered * (1.0 - p.body * 0.5) + body_sample * p.body * 0.5;
        }

        // Feedback boost for sustain (adds energy back, fights decay)
        // Only at high values does it approach infinite sustain.
        if p.feedback > 0.01 {
            filtered += filtered * p.feedback * 0.3;
            filtered = filtered.clamp(-1.5, 1.5);
        }

        // Write back
        self.buffer[read_pos] = filtered;
        self.write_pos = read_pos;

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

            let head2 = self.buffer[p2];
            let head3 = self.buffer[p3];

            output = filtered * (1.0 - p.ens_mix) + (head2 + head3) * 0.5 * p.ens_mix;
        }

        output
    }

    /// `tick_full` for a sympathetic string, which has no body, stiffness,
    /// feedback or ensemble, and which `input` excites at its write
    /// position. The last tick's output waits in `pending` and is stored
    /// with this tick's input: one store a sample instead of a store, a
    /// load and a store again, bit-identical to storing it, then adding the
    /// input in place.
    #[inline]
    pub(super) fn tick_coupled(
        &mut self,
        p: &KsRenderParams,
        input: f32,
        pending: &mut f32,
    ) -> f32 {
        debug_assert!(p.stiffness <= 0.01 && p.body <= 0.03 && p.feedback <= 0.01);
        debug_assert!(p.ens_mix <= 0.01 || p.ens_depth <= 0.01);
        let wp = self.write_pos;
        let injected = *pending + input;
        let read_pos = (wp + 1) % self.delay_len;
        let next_pos = (read_pos + 1) % self.delay_len;
        let current = self.buffer[read_pos];
        // A two-sample loop reads its write position back.
        let next = if next_pos == wp {
            injected
        } else {
            self.buffer[next_pos]
        };
        let filtered = lowpass(p, current, next);
        self.buffer[wp] = injected;
        *pending = filtered;
        self.write_pos = read_pos;
        filtered
    }
}

/// The loop's filter: a two-point average and a gain below 1.
#[inline]
fn lowpass(p: &KsRenderParams, current: f32, next: f32) -> f32 {
    // KS low-pass averaging: blend between current and next sample.
    // Higher coeff = more averaging = darker sound.
    // damping=0 (bright): coeff=0.05 (barely any filtering)
    // damping=1 (dark): coeff=0.5 (heavy filtering, fast decay)
    let coeff = 0.05 + p.damping * 0.45;
    let filtered = current * (1.0 - coeff) + next * coeff;

    // The 2-point average inherently decays the signal.
    // Apply a per-sample gain < 1.0 to control decay time.
    // decay=0 → gain=0.9990 (very long ring, ~7 seconds)
    // decay=1 → gain=0.9900 (short pluck, ~100ms)
    let gain = 0.999 - p.decay * 0.009;
    filtered * gain
}

crate::in_place::field_list!(KsString => KsString { buffer, write_pos, delay_len, dirty, ens_lfo_phase, noise_state });

#[inline]
pub(super) fn xorshift_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as i32) as f32 / i32::MAX as f32
}

/// Bytes `KsString::clear` has written on this thread: for the tests.
#[cfg(any(test, feature = "test-support"))]
pub(super) mod cleared {
    extern crate std;
    use core::cell::Cell;

    std::thread_local!(static BYTES: Cell<usize> = const { Cell::new(0) });

    pub(in super::super) fn add(n: usize) {
        BYTES.with(|b| b.set(b.get() + n));
    }

    /// The bytes cleared since the last call.
    pub fn take() -> usize {
        BYTES.with(|b| b.replace(0))
    }
}
