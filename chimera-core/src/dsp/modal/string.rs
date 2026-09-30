//! The Karplus-Strong string, the project owner's own code from their
//! Carcosa firmware for the Ambika, relicensed here under MIT (ADR 0032).

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use super::body::{Body, BodyMix};
use super::dispersion::{DISPERSION_STAGES, Dispersion};
use super::loop_parts::{Allpass1, LoopGain, MIN_LINE, Release, split};
use crate::dsp::xorshift_noise;
use crate::hw::SAMPLE_RATE;

// ── Karplus-Strong delay line (from the owner's Carcosa firmware) ───

/// String ring length (ADR 0040, 0056): G1 (MIDI 31, 979.6 samples at
/// 48 kHz) needs a 979-sample line and two more for the low-pass, so G1
/// and above play in tune; lower notes clamp.
pub const MAX_STRING_DELAY: usize = 981;

/// The longest line: the ring less the low-pass's two taps.
const MAX_LINE: usize = MAX_STRING_DELAY - 2;

/// Parameters for `KsString::tick_full`, built once per render block (not
/// per sample) at each call site.
#[derive(Clone, Copy)]
pub(super) struct KsRenderParams {
    /// The loop low-pass's side taps (`lp_coeff`).
    pub lp: f32,
    /// The loop's gain per pass.
    pub gain: LoopGain,
    pub ens_rate: f32,
    pub ens_depth: f32,
    pub ens_mix: f32,
}

/// A fresh line's ring, before its first `set_period`.
const INIT_LEN: usize = 100;
/// What a fresh line's first `clear` writes.
pub(super) const FRESH_CLEAR_BYTES: usize = INIT_LEN * size_of::<f32>();
/// The most any line's `clear` writes: its whole ring.
pub(super) const RING_BYTES: usize = MAX_STRING_DELAY * size_of::<f32>();

/// A fresh line's pluck noise.
const NOISE_SEED: u32 = 0x8765_4321;

/// The third ensemble head's fixed offset from the second.
const ENS_SPREAD: f32 = 0.3;

/// A delay line and its dirty extent: every sample at or past `dirty`
/// reads 0.0, so a clear zeros `[0, dirty)` only. The loop reads `delay`
/// behind the last write, round a ring of `ring_len`; the allpass `frac`
/// carries the rest of the period. `MIN_LINE <= delay`,
/// `delay + 2 <= ring_len <= dirty` and `write_pos < ring_len`: every
/// store lands below `dirty`, and only `set_period` and `clear` move it.
pub(super) struct KsString {
    buffer: [f32; MAX_STRING_DELAY],
    /// The last write.
    write_pos: usize,
    ring_len: usize,
    delay: usize,
    dirty: usize,
    ens_lfo_phase: u32,
    noise_state: u32,
    frac: Allpass1,
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
            addr_of_mut!((*p).ring_len).write(INIT_LEN);
            addr_of_mut!((*p).delay).write(INIT_LEN - 2);
            addr_of_mut!((*p).dirty).write(INIT_LEN);
            addr_of_mut!((*p).ens_lfo_phase).write(0);
            addr_of_mut!((*p).noise_state).write(NOISE_SEED);
            addr_of_mut!((*p).frac).write(Allpass1::default());
            slot.assume_init_mut()
        }
    }

    /// A loop of `period` samples, `other` of them in its other parts at
    /// `w`: the line and the allpass (`split`). The ring grows to fit,
    /// its samples kept in order, and never shrinks mid-note.
    pub(super) fn set_period(&mut self, period: f32, other: f32, w: f32) {
        let (mut delay, mut eta) = split(period, other, w);
        if delay > MAX_LINE || period - other < MIN_LINE as f32 + 0.5 {
            // Clamped, the fraction leaves [0.5, 1.5]: held at its edge.
            delay = delay.min(MAX_LINE);
            let frac = (period - other - delay as f32).clamp(0.5, 1.5);
            eta = super::loop_parts::eta_for(frac, w);
        }
        self.delay = delay;
        self.frac.set(eta);
        let need = delay + 2;
        if need > self.ring_len {
            // The oldest samples move up past the new gap, which reads silent.
            let (w, grow) = (self.write_pos + 1, need - self.ring_len);
            self.buffer.copy_within(w..self.ring_len, w + grow);
            self.buffer[w..w + grow].fill(0.0);
            self.ring_len = need;
            self.dirty = self.dirty.max(need);
        }
    }

    /// Tunes the loop to `freq`: the line and the allpass, nothing else
    /// in the loop delaying.
    pub(super) fn tune(&mut self, freq: f32, sample_rate: u32) {
        let (period, other, w) = loop_at(freq, sample_rate);
        self.set_period(period, other, w);
    }

    #[cfg(test)]
    pub(super) fn delay(&self) -> usize {
        self.delay
    }

    /// The sample `k` behind the last write, `k < ring_len`.
    #[inline]
    fn behind(&self, k: usize) -> f32 {
        let i = self.write_pos + self.ring_len - k;
        self.buffer[if i >= self.ring_len {
            i - self.ring_len
        } else {
            i
        }]
    }

    /// Steps the write position on round the ring.
    #[inline]
    fn advance(&mut self) {
        self.write_pos += 1;
        if self.write_pos == self.ring_len {
            self.write_pos = 0;
        }
    }

    /// The loop's next sample, before the gain: the low-pass centred
    /// `delay` back, so it adds no delay.
    #[inline]
    fn lowpass(&self, p: &KsRenderParams) -> f32 {
        let d = self.delay;
        let c = p.lp;
        c * 0.5 * (self.behind(d - 2) + self.behind(d)) + (1.0 - c) * self.behind(d - 1)
    }

    /// Plucks the string with white noise on a loop of `period` samples
    /// (`set_period`), unshaped until `shape`. The line is cleared first,
    /// before the new loop's length widens its extent: the clear writes
    /// `clear_bytes`, and the old note's samples past the new loop are
    /// never read back, even by a pitch drop that lengthens it.
    pub(super) fn excite(&mut self, (period, other, w): (f32, f32, f32), amplitude: f32) {
        self.clear();
        self.set_period(period, other, w);
        let len = self.delay;
        for i in 0..len {
            self.buffer[i] = xorshift_noise(&mut self.noise_state) * amplitude;
        }
        // The oldest sample first.
        self.write_pos = len - 1;
        self.ens_lfo_phase = 0;
    }

    /// Shapes the pluck `excite` wrote, in place, before the loop reads
    /// it: a comb notched at `position`'s harmonics of a loop of `period`
    /// samples, then one smoothing pass (the old colour 0.8).
    pub(super) fn shape(&mut self, position: f32, period: f32) {
        let len = self.delay;
        if position > 0.03 {
            let notch_period = ((period * position) as usize).max(2);
            if notch_period < len {
                for i in 0..len - notch_period {
                    self.buffer[i] = (self.buffer[i] + self.buffer[i + notch_period]) * 0.5;
                }
            }
        }
        for i in 1..len {
            self.buffer[i] = (self.buffer[i] + self.buffer[i - 1]) * 0.5;
        }
    }

    /// Zeros the whole line and starts the shortest ring from the start:
    /// a note starts on a silent line, even once a pitch drop lengthens
    /// it. Only `[0, dirty)` can hold anything, so only it is written: a
    /// line last played high clears in a fraction of the buffer.
    pub(super) fn clear(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        cleared::add(self.clear_bytes());
        self.buffer[..self.dirty].fill(0.0);
        self.write_pos = 0;
        self.delay = MIN_LINE;
        self.ring_len = MIN_LINE + 2;
        self.dirty = self.ring_len;
        self.frac.reset();
    }

    /// What the next `clear` writes.
    pub(super) fn clear_bytes(&self) -> usize {
        self.dirty * size_of::<f32>()
    }

    /// Bowed's ring: the sample `delay` pushes back, or the note's first
    /// until `written` reaches it, so a low note sounds from its first
    /// samples (#206).
    pub(super) fn ring_tap(&self, written: u32) -> f32 {
        self.behind(self.delay.min(written.max(1) as usize) - 1)
    }

    /// Bowed's ring: stores `x` through the allpass, stepping on round it.
    pub(super) fn ring_push(&mut self, x: f32) {
        self.advance();
        self.buffer[self.write_pos] = self.frac.process(x);
    }

    /// The line and its dirty extent: for the tests.
    #[cfg(test)]
    pub(super) fn line(&self) -> (&[f32; MAX_STRING_DELAY], usize) {
        (&self.buffer, self.dirty)
    }

    /// The pluck's noise from its first note's: for the tests.
    #[cfg(test)]
    pub(super) fn reseed(&mut self) {
        self.noise_state = NOISE_SEED;
    }

    /// The next clear zeros the whole ring, as before the dirty extent:
    /// the tests' reference.
    #[cfg(test)]
    pub(super) fn soil(&mut self) {
        self.dirty = MAX_STRING_DELAY;
    }

    /// One pass of the loop, the low-pass, `disp` and the allpass, then
    /// `gain` last, so nothing bypasses it; out through the ensemble.
    #[inline]
    fn tick_full(
        &mut self,
        p: &KsRenderParams,
        gain: LoopGain,
        disp: Option<&mut Dispersion>,
    ) -> f32 {
        let d = self.delay;
        let mut x = self.lowpass(p);
        if let Some(disp) = disp {
            x = disp.process(x);
        }
        let filtered = self.frac.process(x) * gain.get();
        self.advance();
        self.buffer[self.write_pos] = filtered;

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

            let offset2 = (lfo_val * p.ens_depth * d as f32 * 0.05) as i32;
            let offset3 = -offset2 + (ENS_SPREAD * d as f32 * 0.02) as i32;

            // A head `offset` newer than the loop's read, round the loop.
            let head = |offset: i32| self.behind((d as i32 - offset).rem_euclid(d as i32) as usize);
            output =
                filtered * (1.0 - p.ens_mix) + (head(offset2) + head(offset3)) * 0.5 * p.ens_mix;
        }

        output
    }

    /// `tick_full` for a sympathetic string, which has no dispersion,
    /// ensemble or release, and which `input` excites at its write
    /// position. The last tick's output waits in `pending` and is stored
    /// with this tick's input.
    #[inline]
    pub(super) fn tick_coupled(
        &mut self,
        p: &KsRenderParams,
        input: f32,
        pending: &mut f32,
    ) -> f32 {
        debug_assert!(p.ens_mix <= 0.01 || p.ens_depth <= 0.01);
        self.buffer[self.write_pos] = *pending + input;
        let filtered = self.frac.process(self.lowpass(p) * p.gain.get());
        self.advance();
        *pending = filtered;
        filtered
    }
}

/// `freq`'s loop for `set_period`: its period, nothing else delaying,
/// and its angular frequency.
pub(super) fn loop_at(freq: f32, sample_rate: u32) -> (f32, f32, f32) {
    let w = core::f32::consts::TAU * freq / sample_rate as f32;
    (sample_rate as f32 / freq, 0.0, w)
}

/// The loop low-pass's side taps at `bright`, 1 brightest: the old
/// two-point average's loss at low frequencies, `c·(1 − c)` for its `c`,
/// so old patches keep their tone (0.0475 to 0.25).
pub(super) fn lp_coeff(bright: f32) -> f32 {
    let c = 0.05 + 0.45 * (1.0 - bright);
    c * (1.0 - c)
}

crate::in_place::field_list!(KsString => KsString { buffer, write_pos, ring_len, delay, dirty, ens_lfo_phase, noise_state, frac });

/// STRING's string, and SYMP's main one: the loop, STRUCTURE's
/// dispersion in it, BODY on its output and the note-off's ramp.
pub(super) struct StringVoice {
    pub(super) string: KsString,
    disp: Dispersion,
    /// STRING's: the chain runs. SYMP's main string has none, and skips it.
    stiff: bool,
    /// The STRUCTURE the chain is at, slewing to the eased one.
    structure: f32,
    /// The loop's period, samples, as last tuned.
    period: f32,
    body: Body,
    /// BODY, latched at note-on (spec § 1).
    body_mix: BodyMix,
    release: Release,
}

crate::in_place::field_list!(StringVoice => StringVoice {
    string, disp, stiff, structure, period, body, body_mix, release,
});

/// The longest loop: the whole line, a one-sample fraction and the chain
/// at STRUCTURE 0. A lower note plays this, at every STRUCTURE.
const LONGEST: f32 = (MAX_LINE + 1 + DISPERSION_STAGES) as f32;

/// Samples a block the chain's DC delay, and so the line, may move: a
/// STRUCTURE step glides, not jumps.
const DISP_SLEW: f32 = 2.0;

impl StringVoice {
    pub(super) fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the string is built in place
        // and the rest written by value, before `assume_init_mut`.
        unsafe {
            KsString::init_in_place(crate::in_place::uninit_at(addr_of_mut!((*p).string)));
            addr_of_mut!((*p).disp).write(Dispersion::default());
            addr_of_mut!((*p).stiff).write(false);
            addr_of_mut!((*p).structure).write(0.0);
            addr_of_mut!((*p).period).write(INIT_LEN as f32);
            addr_of_mut!((*p).body).write(Body::new(SAMPLE_RATE));
            addr_of_mut!((*p).body_mix).write(Body::mix(0.0));
            addr_of_mut!((*p).release).write(Release::HELD);
            slot.assume_init_mut()
        }
    }

    /// `freq`'s loop, stiff no longer than `LONGEST`, at `self.structure`: the
    /// dispersion set, and its phase delay at f0 the line's to give back,
    /// so the pitch holds.
    fn dispersed(&mut self, freq: f32, sample_rate: u32) -> (f32, f32, f32) {
        let (period, _, w) = loop_at(freq, sample_rate);
        if !self.stiff {
            self.period = period;
            return (period, 0.0, w);
        }
        // Clamped, the chain's delay comes off the whole line, not on top.
        let (period, w) = if period > LONGEST {
            (LONGEST, core::f32::consts::TAU / LONGEST)
        } else {
            (period, w)
        };
        self.period = period;
        let a = Dispersion::coeff(self.structure, period);
        self.disp.set(a);
        (period, Dispersion::phase_delay(a, w), w)
    }

    /// A note-on: plucks `freq`, at `structure` if stiff (STRING) or with
    /// no chain (`None`, SYMP's main string); BODY latched at `body`.
    pub(super) fn pluck(
        &mut self,
        (freq, sample_rate): (f32, u32),
        structure: Option<f32>,
        amplitude: f32,
        body: f32,
    ) {
        self.stiff = structure.is_some();
        self.structure = structure.unwrap_or(0.0);
        let l = self.dispersed(freq, sample_rate);
        self.string.excite(l, amplitude);
        self.disp.reset();
        self.body.reset();
        self.body_mix = Body::mix(body);
        self.release = Release::HELD;
    }

    /// Shapes the pluck at `position` of the loop's period (`KsString::shape`).
    pub(super) fn shape(&mut self, position: f32) {
        self.string.shape(position, self.period);
    }

    /// Retunes to `freq`, the chain a step towards `structure`, mid-note.
    pub(super) fn tune(&mut self, freq: f32, sample_rate: u32, structure: f32) {
        if self.stiff {
            self.structure = Dispersion::slew(self.structure, structure, self.period, DISP_SLEW);
        }
        let (period, other, w) = self.dispersed(freq, sample_rate);
        self.string.set_period(period, other, w);
    }

    /// The chain has not reached `structure`: `tune` again next block.
    pub(super) fn gliding(&self, structure: f32) -> bool {
        self.stiff && self.structure != structure
    }

    /// Note-off: the loop gain ramps from `held` to `to` (`Release`).
    pub(super) fn release(&mut self, held: LoopGain, to: LoopGain) {
        self.release.start(held, to);
    }

    /// The string's next sample, before BODY.
    #[inline]
    pub(super) fn tick(&mut self, p: &KsRenderParams) -> f32 {
        let gain = self.release.gain(p.gain);
        let disp = if self.stiff {
            Some(&mut self.disp)
        } else {
            None
        };
        self.string.tick_full(p, gain, disp)
    }

    /// `x` through BODY, outside the loop.
    #[inline]
    pub(super) fn colour(&mut self, x: f32) -> f32 {
        self.body.process(x, self.body_mix)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::dsp::modal::loop_parts::allpass_phase_delay;
    use crate::dsp::note_to_freq;
    use crate::in_place::by_value;

    /// G1 to C8, STRUCTURE 0 to 1: the line stays in `[MIN_LINE, MAX_LINE]`,
    /// and the line, the allpass and the dispersion add to the period at
    /// f0. G1 at 1 gives the chain its most.
    #[test]
    fn dispersion_stays_in_tune_and_in_bounds() {
        // SAFETY: `init_in_place` writes every field.
        let mut v = unsafe { by_value(StringVoice::init_in_place) };
        for note in [31, 48, 84, 108] {
            let freq = note_to_freq(note);
            for s in [0.0, 0.5, 1.0] {
                v.pluck((freq, 48_000), Some(s), 1.0, 0.0);
                let (period, _, w) = loop_at(freq, 48_000);
                let d = v.string.delay();
                assert!((MIN_LINE..=MAX_LINE).contains(&d), "{note} {s}: {d}");
                let a = Dispersion::coeff(s, period);
                let total = d as f32
                    + allpass_phase_delay(v.string.frac.eta(), w)
                    + Dispersion::phase_delay(a, w);
                assert!(
                    (total - period).abs() < 1e-2,
                    "{note} {s}: {total} vs {period}"
                );
            }
        }
    }
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
