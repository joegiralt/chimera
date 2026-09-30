//! The Karplus-Strong string, the project owner's own code from their
//! Carcosa firmware for the Ambika, relicensed here under MIT (ADR 0032).

use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use super::body::{Body, BodyMix};
use super::dispersion::{DISPERSION_STAGES, Dispersion};
use super::ensemble::{ENS_HEADS, Ensemble};
use super::loop_parts::{Allpass1, LoopGain, MIN_LINE, split};
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
}

/// A fresh line's ring, before its first `set_period`.
const INIT_LEN: usize = 100;
/// What a fresh line's first `clear` writes.
pub(super) const FRESH_CLEAR_BYTES: usize = INIT_LEN * size_of::<f32>();
/// The most any line's `clear` writes: its whole ring.
pub(super) const RING_BYTES: usize = MAX_STRING_DELAY * size_of::<f32>();

/// A fresh line's pluck noise.
const NOISE_SEED: u32 = 0x8765_4321;

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

    pub(super) fn delay(&self) -> usize {
        self.delay
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn ring_len(&self) -> usize {
        self.ring_len
    }

    /// The sample `k` behind the last write, `k < ring_len`.
    #[cfg(test)]
    #[inline]
    fn behind(&self, k: usize) -> f32 {
        let i = self.write_pos + self.ring_len - k;
        self.buffer[if i >= self.ring_len {
            i - self.ring_len
        } else {
            i
        }]
    }

    /// The line `delay` behind the last write, linearly interpolated:
    /// `delay + 1 < ring_len`.
    #[cfg(test)]
    pub(super) fn read_frac(&self, delay: f32) -> f32 {
        let i = delay as usize;
        let f = delay - i as f32;
        self.behind(i) + f * (self.behind(i + 1) - self.behind(i))
    }

    /// Steps the write position on round the ring.
    #[cfg(test)]
    #[inline]
    fn advance(&mut self) {
        self.write_pos += 1;
        if self.write_pos == self.ring_len {
            self.write_pos = 0;
        }
    }

    /// The loop's next sample, before the gain: the low-pass centred
    /// `delay` back, so it adds no delay. The spans' reference.
    #[cfg(test)]
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
    }

    /// Shapes the pluck `excite` wrote, in place, before the loop reads
    /// it: plucked at `beta` of the string (`comb`), then `passes`
    /// smoothing passes (`color_passes`), then its mean taken out.
    pub(super) fn shape(&mut self, beta: f32, passes: usize) {
        let len = self.delay;
        comb(&mut self.buffer[..len], beta);
        for _ in 0..passes {
            for i in 1..len {
                self.buffer[i] = (self.buffer[i] + self.buffer[i - 1]) * 0.5;
            }
        }
        // A pluck's mean rings as the loop's 0 Hz mode for the whole T60,
        // and a halo string's comb gains it by 1 / (1 − g): the drift SYMP
        // put under the output blocker. None.
        let line = &mut self.buffer[..len];
        let mean = line.iter().sum::<f32>() / len as f32;
        for x in line {
            *x -= mean;
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

    /// Bowed's waveguide on the ring (`bow.rs`): the ring in use
    /// (`ring_len` cells), the last write, the loop's line and its tuning allpass.
    pub(super) fn guide(&mut self) -> (&mut [f32], &mut usize, usize, &mut Allpass1) {
        let ring = &mut self.buffer[..self.ring_len];
        (ring, &mut self.write_pos, self.delay, &mut self.frac)
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
    /// `gain` last, so nothing bypasses it. The spans' reference.
    #[cfg(test)]
    fn tick_full(
        &mut self,
        p: &KsRenderParams,
        gain: LoopGain,
        disp: Option<&mut Dispersion>,
    ) -> f32 {
        let mut x = self.lowpass(p);
        if let Some(disp) = disp {
            x = disp.process(x);
        }
        let filtered = self.frac.process(x) * gain.get();
        self.advance();
        self.buffer[self.write_pos] = filtered;
        filtered
    }

    /// `tick_full` for a sympathetic string, which has no dispersion,
    /// or ensemble, and which `input` excites at its write
    /// position. The last tick's output waits in `pending` and is stored
    /// with this tick's input. `run_coupled`'s reference.
    #[cfg(test)]
    pub(super) fn tick_coupled(
        &mut self,
        p: &KsRenderParams,
        input: f32,
        pending: &mut f32,
    ) -> f32 {
        self.buffer[self.write_pos] = *pending + input;
        let filtered = self.frac.process(self.lowpass(p) * p.gain.get());
        self.advance();
        *pending = filtered;
        filtered
    }

    /// `tick_coupled` over `input`, each output added to `out`'s sample:
    /// the same arithmetic, in spans where neither the write nor the
    /// newest tap wraps, the two older taps carried from the sample before.
    pub(super) fn run_coupled(
        &mut self,
        p: &KsRenderParams,
        input: &[f32],
        pending: &mut f32,
        out: &mut [f32],
    ) {
        let (half, mid, gain) = (p.lp * 0.5, 1.0 - p.lp, p.gain.get());
        let (d, len) = (self.delay, self.ring_len);
        let ring = Cell::from_mut(&mut self.buffer[..len]).as_slice_of_cells();
        let mut wp = self.write_pos;
        // The newest tap, `d − 2` behind the write: at `d` 2, the sample
        // just written.
        let mut cp = (wp + len + 2 - d) % len;
        let (mut a, mut b) = (behind_in(ring, wp, d), behind_in(ring, wp, d - 1));
        let (mut ap, mut pend) = (self.frac, *pending);
        let mut k = 0;
        while k < input.len() {
            let m = (input.len() - k).min(len - wp).min(len - cp);
            let io = input[k..k + m].iter().zip(&mut out[k..k + m]);
            for ((w, t), (x, o)) in ring[wp..wp + m].iter().zip(&ring[cp..cp + m]).zip(io) {
                w.set(pend + x);
                let c = t.get();
                pend = ap.process((half * (c + a) + mid * b) * gain);
                *o += pend;
                (a, b) = (b, c);
            }
            (wp, cp) = (wrap(wp + m, len), wrap(cp + m, len));
            k += m;
        }
        (self.write_pos, self.frac, *pending) = (wp, ap, pend);
    }
}

/// A pluck at `beta` of the string, on one period of the line: `(x[i] +
/// x[i + n]) / 2` round it for `n = β·len / 2`, so harmonic k is
/// `|cos πkβ/2|` of the noise's. Its nulls are a pluck's at the odd
/// multiples of 1/β: none at the end (β 0 passes the noise whole), and at
/// the middle the 2nd, 6th, 10th…; the fundamental never. In place: each
/// cycle of `i → i + n` walked in order, its first sample kept for its
/// last.
fn comb(line: &mut [f32], beta: f32) {
    let len = line.len();
    let n = (beta * 0.5 * len as f32 + 0.5) as usize;
    if n == 0 || n >= len {
        return;
    }
    let (mut a, mut b) = (len, n);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    for start in 0..a {
        let first = line[start];
        let mut i = start;
        loop {
            let j = if i + n >= len { i + n - len } else { i + n };
            let next = if j == start { first } else { line[j] };
            line[i] = (line[i] + next) * 0.5;
            if j == start {
                break;
            }
            i = j;
        }
    }
}

/// `i` round a ring of `len`, `i <= len`.
#[inline]
fn wrap(i: usize, len: usize) -> usize {
    if i == len { 0 } else { i }
}

/// Where `ring`'s sample `k` behind `at` is, `k < ring.len()`.
#[inline]
fn behind_at(ring: &[Cell<f32>], at: usize, k: usize) -> usize {
    let i = at + ring.len() - k;
    if i >= ring.len() { i - ring.len() } else { i }
}

/// `StringVoice::run`'s state, held in registers across a block.
struct Held {
    half: f32,
    mid: f32,
    gain: f32,
    mix: f32,
    d: usize,
    /// The low-pass's two older taps, oldest first.
    taps: (f32, f32),
    ap: Allpass1,
    disp: Dispersion,
    ens: Ensemble,
}

impl Held {
    /// `out.len()` samples writing from `ws` and reading the newest tap
    /// from `cp`, neither wrapping. `FAST`: the heads not wrapping.
    #[inline(always)]
    fn span<const STIFF: bool, const ENS: bool, const FAST: bool>(
        &mut self,
        ring: &[Cell<f32>],
        (ws, cp): (usize, usize),
        out: &mut [f32],
    ) {
        let m = out.len();
        let (mut a, mut b) = self.taps;
        let span = ring[ws..ws + m].iter().zip(&ring[cp..cp + m]);
        for (at, ((w, t), o)) in (ws..).zip(span.zip(out)) {
            let c = t.get();
            let mut x = self.half * (c + a) + self.mid * b;
            if STIFF {
                x = self.disp.process(x);
            }
            let dry = self.ap.process(x) * self.gain;
            w.set(dry);
            (a, b) = (b, c);
            *o = if ENS {
                let heads = self.ens.head_delays(self.d);
                self.ens.advance();
                let read = |&h| {
                    if FAST {
                        read_frac_up(ring, at, h)
                    } else {
                        read_frac_in(ring, at, h)
                    }
                };
                let wet = heads.iter().map(read).sum::<f32>();
                dry + self.mix * (wet * (1.0 / ENS_HEADS as f32) - dry)
            } else {
                dry
            };
        }
        self.taps = (a, b);
    }
}

/// `ring`'s sample `k` behind `at`.
#[inline]
fn behind_in(ring: &[Cell<f32>], at: usize, k: usize) -> f32 {
    ring[behind_at(ring, at, k)].get()
}

/// `read_frac_in` where the taps don't wrap: `delay + 2 <= at`.
#[inline]
fn read_frac_up(ring: &[Cell<f32>], at: usize, delay: f32) -> f32 {
    let i = delay as usize;
    let f = delay - i as f32;
    let x = ring[at - i].get();
    x + f * (ring[at - i - 1].get() - x)
}

/// `KsString::read_frac` on `ring` behind `at`: the older tap is the
/// newer's neighbour, so one wrap serves both.
#[inline]
fn read_frac_in(ring: &[Cell<f32>], at: usize, delay: f32) -> f32 {
    let i = delay as usize;
    let f = delay - i as f32;
    let j = behind_at(ring, at, i);
    let x = ring[j].get();
    let older = ring[if j == 0 { ring.len() - 1 } else { j - 1 }].get();
    x + f * (older - x)
}

/// `freq`'s loop for `set_period`: its period, nothing else delaying,
/// and its angular frequency.
pub(super) fn loop_at(freq: f32, sample_rate: u32) -> (f32, f32, f32) {
    let w = core::f32::consts::TAU * freq / sample_rate as f32;
    (sample_rate as f32 / freq, 0.0, w)
}

/// The pluck's smoothing passes at COLOR, 1 brightest: the old `ks_color`
/// law. The old hidden 0.8 is one pass.
pub(super) fn color_passes(color: f32) -> usize {
    (((1.0 - color) * 7.0) as usize).min(7)
}

/// The loop low-pass's side taps at `bright`, 1 brightest: the old
/// two-point average's loss at low frequencies, `c·(1 − c)` for its `c`,
/// so old patches keep their tone (0.0475 to 0.25).
pub(super) fn lp_coeff(bright: f32) -> f32 {
    let c = 0.05 + 0.45 * (1.0 - bright);
    c * (1.0 - c)
}

crate::in_place::field_list!(KsString => KsString { buffer, write_pos, ring_len, delay, dirty, noise_state, frac });

/// STRING's string, and SYMP's main one: the loop, STRUCTURE's
/// dispersion in it, and BODY on its output.
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
    /// The ensemble and ENS MIX, latched at note-on (spec § 1); MIX 0 is off.
    ens: Ensemble,
    ens_mix: f32,
    /// COLOR's smoothing passes, latched at note-on.
    passes: u8,
}

crate::in_place::field_list!(StringVoice => StringVoice {
    string, disp, stiff, structure, period, body, body_mix, ens, ens_mix, passes,
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
            addr_of_mut!((*p).ens).write(Ensemble::default());
            addr_of_mut!((*p).ens_mix).write(0.0);
            addr_of_mut!((*p).passes).write(1);
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
    /// no chain (`None`, SYMP's main string); BODY, the ensemble at its
    /// MIX, and COLOR's passes, latched.
    pub(super) fn pluck(
        &mut self,
        (freq, sample_rate): (f32, u32),
        structure: Option<f32>,
        amplitude: f32,
        (body, (ens_mix, ens)): (f32, (f32, Ensemble)),
        color: f32,
    ) {
        self.stiff = structure.is_some();
        self.structure = structure.unwrap_or(0.0);
        let l = self.dispersed(freq, sample_rate);
        self.string.excite(l, amplitude);
        self.disp.reset();
        self.body.reset();
        self.body_mix = Body::mix(body);
        self.ens = ens;
        self.ens_mix = ens_mix;
        self.passes = color_passes(color) as u8;
    }

    /// Whether BODY and the ensemble run, as latched: what the note bills.
    pub(super) fn runs(&self) -> (bool, bool) {
        (self.body_mix.runs(), self.ens_mix > 0.0)
    }

    /// Per block, after any retune: the heads sized to the line in use.
    pub(super) fn set_ensemble(&mut self) {
        if self.ens_mix > 0.0 {
            self.ens.set(self.string.delay);
        }
    }

    /// Shapes the pluck at POS's β (`params::beta`, from the end), at COLOR's passes
    /// (`KsString::shape`).
    pub(super) fn shape(&mut self, pos: f32) {
        self.string.shape(
            super::params::beta(pos, super::params::END),
            self.passes.into(),
        );
    }

    /// Retunes to `freq`, the chain a step towards `structure`, mid-note;
    /// on a note's first block (`snap`) all the way.
    pub(super) fn tune(&mut self, freq: f32, sample_rate: u32, structure: f32, snap: bool) {
        if self.stiff {
            self.structure = if snap {
                structure
            } else {
                Dispersion::slew(self.structure, structure, self.period, DISP_SLEW)
            };
        }
        let (period, other, w) = self.dispersed(freq, sample_rate);
        self.string.set_period(period, other, w);
    }

    /// The chain has not reached `structure`: `tune` again next block.
    pub(super) fn gliding(&self, structure: f32) -> bool {
        self.stiff && self.structure != structure
    }

    /// The chain's STRUCTURE and the line's length: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn line(&self) -> (f32, usize) {
        (self.structure, self.string.delay)
    }

    /// The string's next sample, and the ensemble's heads on its line,
    /// before BODY: `render`'s reference.
    #[cfg(test)]
    pub(super) fn tick(&mut self, p: &KsRenderParams) -> f32 {
        let gain = p.gain;
        let disp = if self.stiff {
            Some(&mut self.disp)
        } else {
            None
        };
        let dry = self.string.tick_full(p, gain, disp);
        let mix = self.ens_mix;
        if mix <= 0.0 {
            return dry;
        }
        let heads = self.ens.head_delays(self.string.delay);
        self.ens.advance();
        let wet = heads.iter().map(|&o| self.string.read_frac(o)).sum::<f32>();
        dry + mix * (wet * (1.0 / ENS_HEADS as f32) - dry)
    }

    /// `tick` over `out`, in spans (`run`).
    pub(super) fn render(&mut self, p: &KsRenderParams, out: &mut [f32]) {
        match (self.stiff, self.ens_mix > 0.0) {
            (false, false) => self.run::<false, false>(p, out),
            (true, false) => self.run::<true, false>(p, out),
            (false, true) => self.run::<false, true>(p, out),
            (true, true) => self.run::<true, true>(p, out),
        }
    }

    /// `tick`, with the chain if `STIFF` and the heads if `ENS`: the same
    /// arithmetic, in spans where neither the write nor the newest tap
    /// wraps, the two older taps carried from the sample before.
    fn run<const STIFF: bool, const ENS: bool>(&mut self, p: &KsRenderParams, out: &mut [f32]) {
        let s = &mut self.string;
        let (d, len, last) = (s.delay, s.ring_len, s.write_pos);
        let ring = Cell::from_mut(&mut s.buffer[..len]).as_slice_of_cells();
        let mut h = Held {
            half: p.lp * 0.5,
            mid: 1.0 - p.lp,
            gain: p.gain.get(),
            mix: self.ens_mix,
            d,
            taps: (behind_in(ring, last, d), behind_in(ring, last, d - 1)),
            ap: s.frac,
            disp: self.disp,
            ens: self.ens,
        };
        let mut ws = wrap(last + 1, len);
        // The newest tap, `d − 2` behind the last write: at `d` 2, that write.
        let mut cp = (last + len + 2 - d) % len;
        let reach = h.ens.reach(d);
        let mut k = 0;
        while k < out.len() {
            let mut m = (out.len() - k).min(len - ws).min(len - cp);
            // The heads' wrap, while the write is within `reach` of the
            // ring's start, runs apart in `span`'s slow form.
            let near = ENS && ws < reach;
            if near {
                m = m.min(reach - ws);
            }
            let out = &mut out[k..k + m];
            if near {
                h.span::<STIFF, ENS, false>(ring, (ws, cp), out);
            } else {
                h.span::<STIFF, ENS, true>(ring, (ws, cp), out);
            }
            (ws, cp) = (wrap(ws + m, len), wrap(cp + m, len));
            k += m;
        }
        s.write_pos = if ws == 0 { len - 1 } else { ws - 1 };
        (s.frac, self.disp, self.ens) = (h.ap, h.disp, h.ens);
    }

    /// `buf` through BODY, outside the loop, in place.
    pub(super) fn colour(&mut self, buf: &mut [f32]) {
        self.body.process_block(buf, self.body_mix);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::dsp::modal::loop_parts::allpass_phase_delay;
    use crate::dsp::note_to_freq;
    use crate::in_place::by_value;
    use std::boxed::Box;

    fn voice() -> Box<StringVoice> {
        // SAFETY: `init_in_place` writes every field.
        Box::new(unsafe { by_value(StringVoice::init_in_place) })
    }

    /// `line`'s harmonic `k` of its length, magnitude.
    fn bin(line: &[f32], k: usize) -> f64 {
        let n = line.len() as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &x) in line.iter().enumerate() {
            let ph = std::f64::consts::TAU * (k * i) as f64 / n;
            re += x as f64 * libm::cos(ph);
            im += x as f64 * libm::sin(ph);
        }
        libm::sqrt(re * re + im * im)
    }

    /// The pluck's comb is `|cos πkβ/2|` on harmonic k of the line,
    /// exactly, from G1's line to C7's. Stepped by POS 1/8, the 2nd
    /// harmonic over the 1st, `cos πβ / cos πβ/2`, falls at every step
    /// from 1 at the end to nothing at the middle; C7's 18 samples
    /// resolve quarters.
    #[test]
    fn the_pluck_comb_runs_from_the_end_to_the_middle() {
        let mut state = NOISE_SEED;
        for len in [979, 362, 18] {
            let noise: std::vec::Vec<f32> = (0..len).map(|_| xorshift_noise(&mut state)).collect();
            let mut last = f64::INFINITY;
            let by = if len > 18 { 1 } else { 2 };
            for step in (0..=8).step_by(by) {
                let beta = super::super::params::beta(step as f32 / 8.0, 0.0);
                let mut line = noise.clone();
                comb(&mut line, beta);
                let n = libm::floor(beta as f64 * 0.5 * len as f64 + 0.5);
                let at = |k: usize| {
                    libm::fabs(libm::cos(
                        core::f64::consts::PI * (k as f64) * n / len as f64,
                    ))
                };
                for k in [1, 2, 3] {
                    let got = bin(&line, k) / bin(&noise, k);
                    assert!(
                        (got - at(k)).abs() < 1e-4,
                        "{len} POS {step}/8 h{k}: {got} {}",
                        at(k)
                    );
                }
                let ratio = at(2) / at(1);
                assert!(ratio < last, "{len} POS {step}/8: {ratio} after {last}");
                last = ratio;
            }
            // A whole-sample comb: C7's middle is 5/18, not 1/4.
            let null = if len > 18 { 0.02 } else { 0.3 };
            assert!(last < null, "{len}: the middle's 2nd over 1st {last}");
        }
    }

    /// A pluck leaves no mean on the line: nothing for the loop's 0 Hz mode
    /// to ring, nor a halo's comb to gain.
    #[test]
    fn a_pluck_is_zero_mean() {
        for (note, color) in [(24, 0.0), (60, 0.8), (108, 1.0)] {
            let mut v = voice();
            let ens = Ensemble::new(0.0, 3.0, 48_000);
            v.pluck(
                (note_to_freq(note), 48_000),
                None,
                1.0,
                (0.0, (0.0, ens)),
                color,
            );
            v.shape(0.2);
            let (line, len) = (v.string.line().0, v.string.delay);
            let mean = line[..len].iter().sum::<f32>() / len as f32;
            assert!(mean.abs() < 1e-6, "note {note}: {mean}");
        }
    }

    /// `render`'s spans are `tick` bit for bit: every chain and ensemble
    /// case, from the shortest line to the longest.
    #[test]
    fn render_is_tick_bit_for_bit() {
        let p = KsRenderParams {
            lp: lp_coeff(0.4),
            gain: LoopGain::new(0.998),
        };
        for note in [20, 31, 60, 96, 127] {
            let freq = note_to_freq(note);
            for (structure, ens_mix) in
                [(None, 0.0), (Some(0.7), 0.0), (None, 0.5), (Some(1.0), 1.0)]
            {
                let (mut fast, mut slow) = (voice(), voice());
                for v in [&mut fast, &mut slow] {
                    let ens = Ensemble::new(1.0, 3.0, 48_000);
                    v.pluck((freq, 48_000), structure, 1.0, (0.3, (ens_mix, ens)), 0.8);
                    v.shape(0.2);
                }
                for block in 0..200 {
                    fast.set_ensemble();
                    slow.set_ensemble();
                    let mut a = [0.0; 64];
                    fast.render(&p, &mut a);
                    let b: [f32; 64] = core::array::from_fn(|_| slow.tick(&p));
                    let bits = |x: [f32; 64]| x.map(f32::to_bits);
                    assert_eq!(
                        bits(a),
                        bits(b),
                        "{note} {structure:?} {ens_mix}: block {block}"
                    );
                }
            }
        }
    }

    /// A drifted LFO, never renormalised, clamps its heads: the spans read
    /// in range and match `tick` bit for bit.
    #[test]
    fn a_drifted_ensemble_never_reads_past_its_reach() {
        let p = KsRenderParams {
            lp: lp_coeff(0.4),
            gain: LoopGain::new(0.998),
        };
        for note in [31, 60, 96] {
            let (mut fast, mut slow) = (voice(), voice());
            for v in [&mut fast, &mut slow] {
                let ens = Ensemble::new(1.0, 6.0, 48_000);
                v.pluck(
                    (note_to_freq(note), 48_000),
                    Some(0.5),
                    1.0,
                    (0.0, (1.0, ens)),
                    0.8,
                );
                v.set_ensemble();
                v.ens.drift(1e6);
            }
            for block in 0..100 {
                let mut a = [0.0; 64];
                fast.render(&p, &mut a);
                let b: [f32; 64] = core::array::from_fn(|_| slow.tick(&p));
                assert_eq!(
                    a.map(f32::to_bits),
                    b.map(f32::to_bits),
                    "{note}: block {block}"
                );
            }
        }
    }

    /// `run_coupled` is `tick_coupled` bit for bit, in runs of any length,
    /// the period gliding between runs as a chord change does.
    #[test]
    fn run_coupled_is_tick_coupled_bit_for_bit() {
        let p = KsRenderParams {
            lp: lp_coeff(0.8),
            gain: LoopGain::new(0.999),
        };
        let input: [f32; 64] = core::array::from_fn(|i| libm::sinf(i as f32 * 0.37));
        for period in [3.2, 4.5, 23.7, 979.6] {
            let (mut fast, mut slow) = (voice(), voice());
            // The ring sized for the longest, as a halo note-on does.
            let tune = |s: &mut KsString, p: f32| s.set_period(p, 0.0, core::f32::consts::TAU / p);
            for s in [&mut fast.string, &mut slow.string] {
                s.clear();
                tune(s, period);
            }
            let (mut pf, mut ps) = (0.0, 0.0);
            for (block, len) in [64, 16, 1, 7, 64, 3]
                .into_iter()
                .cycle()
                .take(300)
                .enumerate()
            {
                // Down a fifth and back, a step a run.
                let glide = period * (1.0 - 0.33 * (block % 20) as f32 / 20.0);
                tune(&mut fast.string, glide);
                tune(&mut slow.string, glide);
                let mut a = [0.0; 64];
                fast.string
                    .run_coupled(&p, &input[..len], &mut pf, &mut a[..len]);
                let b: [f32; 64] = core::array::from_fn(|i| {
                    if i < len {
                        slow.string.tick_coupled(&p, input[i], &mut ps)
                    } else {
                        0.0
                    }
                });
                assert_eq!(
                    a.map(f32::to_bits),
                    b.map(f32::to_bits),
                    "{period}: block {block}"
                );
            }
        }
    }

    /// COLOR's passes are the old `ks_color` law: 0.8 the old one pass,
    /// 0 seven, 1 none, never more as COLOR rises.
    #[test]
    fn color_passes_are_the_old_law() {
        assert_eq!(color_passes(0.8), 1);
        assert_eq!(color_passes(0.0), 7);
        assert_eq!(color_passes(1.0), 0);
        let mut last = usize::MAX;
        for i in 0..=128 {
            let n = color_passes(i as f32 / 128.0);
            assert!(n <= last && n <= 7, "{i}/128: {n}");
            last = n;
        }
    }

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
                v.pluck(
                    (freq, 48_000),
                    Some(s),
                    1.0,
                    (0.0, (0.0, Ensemble::default())),
                    0.8,
                );
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
