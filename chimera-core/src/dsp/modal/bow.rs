//! Bowed: two delay lines, bow to bridge and bow to nut, meeting at the
//! bow. After J. O. Smith's digital-waveguide bowed string (Physical Audio
//! Signal Processing, CCRMA, "Bowed Strings"). Chimera's own code: no STK
//! code, constant or table.

use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use super::loop_parts::{Allpass1, LoopGain, RELEASE_SAMPLES, Release};
use super::params::{EASE, Macros, t60};
use super::string::{KsString, loop_at};
use crate::hw::SAMPLE_RATE;
use crate::in_place::uninit_at;

/// The two ends' filters' delay, samples: two centre taps each. Off the line.
pub(super) const ENDS_DELAY: f32 = 4.0;
/// POS 0's and POS 1's bow position, as a fraction of the string from the bridge.
pub(super) const BETA_MIN: f32 = 0.06;
pub(super) const BETA_MAX: f32 = 0.5;
/// The friction curve's half-width over the bow's velocity at force 0.5
/// (`width4`): about 2 is Helmholtz motion. Measured, with the constants
/// below, in the window every Bowed test passes in (2.6 to 2.75); past
/// it the middle or a corner period-doubles.
pub(super) const BOW_WIDTH: f32 = 2.7;
/// Each end's filter's side taps (the three-tap, squared), bridge and nut:
/// the ripples between the bow and either end round off. `|H| ≤ 1` for up to 0.5.
pub(super) const END_C: f32 = 0.5;
/// The bridge's gain per pass while bowed: the ripples decay, nothing
/// swells. A T60 of 1.7 s at C3; measured, 0.97 to 0.975 pass.
pub(super) const BOW_LOSS: f32 = 0.97;
/// BRIGHT's output low-pass, its corner over f0: 0's, and 1's over 0's.
pub(super) const TONE_LO: f32 = 2.0;
pub(super) const TONE_SPAN: f32 = 32.0;
/// The curve widens towards the bridge below `REACH_TO`, by `REACH_BRIDGE`
/// at `BETA_MIN`, where the Helmholtz motion needs more force; and towards
/// the middle above `REACH_FROM`, by `REACH_MID` at `BETA_MAX`, where a
/// narrow curve drags the slow slip and the string slips twice a period.
const REACH_TO: f32 = 0.126;
const REACH_BRIDGE: f32 = 0.5;
const REACH_FROM: f32 = 0.3;
const REACH_MID: f32 = 0.25;
/// Blocks between whole-sample steps of the split: a bow moving faster
/// Doppler-shifts the waves it reflects, and the pitch with them.
pub(super) const BOW_SLEW: u8 = 8;
/// The output's level: the v1 Bowed patch's C3 within ±1 dB of the old
/// bow's. Its held RMS at 1.0 read 0.334 against the old 0.381: 1.14.
pub(super) const BOW_OUT: f32 = 1.14;
/// FORCE's and SPEED's easing a sample: about the macros' `EASE` a block.
const BOW_EASE: f32 = EASE / BLOCK_SIZE as f32;

/// POS's bow position β.
pub(super) fn beta(pos: f32) -> f32 {
    // Not `clamp`, which passes NaN.
    let pos = if pos >= 0.0 { pos.min(1.0) } else { 0.0 };
    BETA_MIN + (BETA_MAX - BETA_MIN) * pos
}

/// The bridge line's whole samples for a loop line of `d`, in [1, d − 1]:
/// β of the whole loop (the line, both ends' filters and about a sample of
/// allpass), less the bridge side's filter and allpass, so POS is the same
/// bow position at every note.
pub(super) fn bridge_len(pos: f32, d: usize) -> usize {
    let side = 0.5 * ENDS_DELAY + 1.0;
    let s = beta(pos) * (d as f32 + ENDS_DELAY + 1.0) - side;
    // `max` first: `as` saturates, and a short loop's side can be past it.
    ((s + 0.5).max(1.0) as usize)
        .min(d.saturating_sub(1))
        .max(1)
}

/// `split` one whole sample towards `to`: the glide's step.
pub(super) fn glide(split: usize, to: usize) -> usize {
    match split.cmp(&to) {
        core::cmp::Ordering::Less => split + 1,
        core::cmp::Ordering::Greater => split - 1,
        core::cmp::Ordering::Equal => split,
    }
}

/// The friction curve's width to the fourth: (BOW_WIDTH·bow_vel)⁴ at
/// force 0.5, and in proportion to it, so the width goes as force^¼: in
/// the Helmholtz window at every FORCE, and 0 at force 0.
#[inline]
pub(super) fn width4(force: f32, bow_vel: f32) -> f32 {
    let w = BOW_WIDTH * bow_vel;
    let w2 = w * w;
    w2 * w2 * (2.0 * force)
}

/// The bow's push on the string at a velocity difference `dv`:
/// dv·ρ(dv), ρ = w4 / (w4 + dv⁴ + 1e-20). At most 0.57·w; 0 at w4 = 0.
#[inline]
pub(super) fn push(dv: f32, w4: f32) -> f32 {
    let dv2 = dv * dv;
    dv * (w4 / (w4 + dv2 * dv2 + 1e-20))
}

/// An end's filter: the linear-phase three-tap, squared, and its state.
#[derive(Clone, Copy, Default)]
pub(super) struct Bridge {
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
}

impl Bridge {
    /// c/2·(a + a2) + (1 − c)·a1, twice, shifting each stage's inputs:
    /// linear phase, two samples, |H| ≤ 1 for c ≤ 0.5.
    #[inline]
    pub(super) fn reflect(&mut self, a: f32, c: f32) -> f32 {
        let y = c * 0.5 * (a + self.a2) + (1.0 - c) * self.a1;
        (self.a2, self.a1) = (self.a1, a);
        let z = c * 0.5 * (y + self.b2) + (1.0 - c) * self.b1;
        (self.b2, self.b1) = (self.b1, y);
        z
    }
}

/// The bowed loop's gain per pass, until note-off ramps it down.
fn bow_gain() -> LoopGain {
    LoopGain::new(BOW_LOSS)
}

/// BRIGHT's output low-pass at `f0` Hz: the one-pole's coefficient, 1 open.
pub(super) fn tone(bright: f32, f0: f32) -> f32 {
    // Not `clamp`, which passes NaN.
    let bright = if bright >= 0.0 { bright.min(1.0) } else { 0.0 };
    let fc = f0 * TONE_LO * libm::expf(bright * libm::logf(TONE_SPAN));
    1.0 - libm::expf(-core::f32::consts::TAU * fc / SAMPLE_RATE as f32)
}

/// A block's held gain, BRIGHT's low-pass (`tone`) and the curve's
/// width at the bow (`reach4`).
pub(super) type Block = (LoopGain, f32, f32);

/// The friction curve's width at the split's β, over `BOW_WIDTH`'s, to the
/// fourth: 1 from `REACH_TO` to `REACH_FROM`, wider towards either end.
pub(super) fn reach4(split: usize, d: usize) -> f32 {
    let beta = (split as f32 + 0.5 * ENDS_DELAY + 1.0) / (d as f32 + ENDS_DELAY + 1.0);
    let near = (REACH_TO - beta).max(0.0) * (REACH_BRIDGE / (REACH_TO - BETA_MIN));
    let mid = (beta - REACH_FROM).max(0.0) * (REACH_MID / (BETA_MAX - REACH_FROM));
    let r = 1.0 + near + mid;
    let r2 = r * r;
    r2 * r2
}

/// What a block holds still: its gain, BRIGHT's low-pass, and the bow's targets.
struct Hold {
    held: LoopGain,
    tone: f32,
    /// The curve's width at the bow's position, to the fourth (`reach4`).
    reach4: f32,
    vel_to: f32,
    force_to: f32,
    lift: f32,
    bowing: bool,
}

/// What a sample moves, in registers across a span.
#[derive(Clone, Copy)]
struct Junction {
    force: f32,
    bow_vel: f32,
    release: Release,
    bridge: Bridge,
    nut: Bridge,
    ap: Allpass1,
    lp: f32,
}

impl Junction {
    /// One sample at the bow, from the nut's return `n` and the bridge's
    /// `a`: the nut line's input, the bridge line's (through the tuning
    /// allpass), and the output.
    #[inline(always)]
    fn sample(&mut self, k: &Hold, n: f32, a: f32) -> (f32, f32, f32) {
        if k.bowing {
            // At its target, no bit moves.
            self.force += BOW_EASE * (k.force_to - self.force);
            self.bow_vel += BOW_EASE * (k.vel_to - self.bow_vel);
        } else if self.force > k.force_to {
            self.force = (self.force - k.lift).max(k.force_to);
        }
        let bow_vel = if self.force > 0.001 {
            self.bow_vel
        } else {
            0.0
        };
        let w4 = width4(self.force, self.bow_vel) * k.reach4;
        let gain = self.release.gain(k.held).get();
        // Both ends invert: upright once a period.
        let bridge = -gain * self.bridge.reflect(a, END_C);
        let nut = -self.nut.reflect(n, END_C);
        let v = push(bow_vel - (bridge + nut), w4);
        let toward = nut + v;
        // BRIGHT: the bridge to the body, outside the loop.
        self.lp += k.tone * (BOW_OUT * toward - self.lp);
        (bridge + v, self.ap.process(toward), self.lp)
    }
}

/// `i` round a ring of `len`, `i <= len`.
#[inline]
fn wrap(i: usize, len: usize) -> usize {
    if i == len { 0 } else { i }
}

/// The cell `k` before `at` round a ring of `len`, `k <= len`.
#[inline]
fn back(at: usize, k: usize, len: usize) -> usize {
    if at >= k { at - k } else { at + len - k }
}

/// The bowed string and the bow's force on it, 0 once the bow lifts.
pub(super) struct BowedString {
    pub(super) string: KsString,
    pub(super) force: f32,
    /// The force `force` eases to: FORCE's at the note's velocity, read
    /// every block, then 0 at note-off.
    pub(super) force_to: f32,
    /// Force shed a sample at note-off: the note's force over `RELEASE_SAMPLES`.
    pub(super) lift: f32,
    /// The bow's velocity, easing to SPEED × `BOW_SPEED`.
    pub(super) bow_vel: f32,
    /// `0.5 + 0.5·velocity`, latched at note-on (`bow_force`).
    pub(super) vel_scale: f32,
    /// On the string: note-on to note-off.
    pub(super) bowing: bool,
    /// The lifted bow's ramp.
    pub(super) release: Release,
    bridge: Bridge,
    /// The nut's filter: the same loss, so neither side of the bow rings on.
    nut: Bridge,
    /// The bridge line's whole samples now; the nut line is `delay − split`.
    split: usize,
    /// BRIGHT's low-pass on the output.
    lp: f32,
    /// Blocks until the split may step again.
    wait: u8,
}

crate::in_place::field_list!(BowedString => BowedString {
    string, force, force_to, lift, bow_vel, vel_scale, bowing, release, bridge, nut, split, lp, wait,
});

impl BowedString {
    pub(super) fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
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
            addr_of_mut!((*p).release).write(Release::HELD);
            addr_of_mut!((*p).bridge).write(Bridge::default());
            addr_of_mut!((*p).nut).write(Bridge::default());
            addr_of_mut!((*p).split).write(1);
            addr_of_mut!((*p).lp).write(0.0);
            addr_of_mut!((*p).wait).write(0);
            slot.assume_init_mut()
        }
    }

    /// Note-on: the line cleared and tuned (`ENDS_DELAY` off the period),
    /// the split snapped to POS, the bridge filter zeroed.
    pub(super) fn start(&mut self, freq: f32, sample_rate: u32, pos: f32) {
        self.string.clear();
        self.tune(freq, sample_rate);
        self.place(pos);
        (self.bridge, self.nut, self.lp) = (Bridge::default(), Bridge::default(), 0.0);
        self.wait = 0;
    }

    /// The split at `pos` at once: at note-on, and at a note's first
    /// block, before a sample sounds.
    pub(super) fn place(&mut self, pos: f32) {
        self.split = bridge_len(pos, self.string.delay());
    }

    /// A pitch move: `set_period(period, ENDS_DELAY, w)`, the split clamped to the new line.
    pub(super) fn tune(&mut self, freq: f32, sample_rate: u32) {
        let (period, _, w) = loop_at(freq, sample_rate);
        self.string.set_period(period, ENDS_DELAY, w);
        self.split = self.split.min(self.string.delay() - 1).max(1);
    }

    /// The block's held gain (lifted, DAMP's, which `Release::gain` never
    /// lets rise; bowed, `BOW_LOSS`) and BRIGHT's low-pass (`tone`).
    pub(super) fn block(&self, m: &Macros, f0: f32) -> Block {
        let held = if self.bowing {
            bow_gain()
        } else {
            LoopGain::from_t60(t60(m.damp), f0)
        };
        (
            held,
            tone(m.bright, f0),
            reach4(self.split, self.string.delay()),
        )
    }

    /// Note-off: the bow lifts, shedding its force over `RELEASE_SAMPLES`,
    /// and the gain ramps from the bowed gain to `held`, DAMP's.
    pub(super) fn lift(&mut self, held: LoopGain) {
        self.lift = self.force / RELEASE_SAMPLES as f32;
        self.force_to = 0.0;
        self.bowing = false;
        self.release.start(bow_gain(), held);
    }

    fn hold(&self, (held, tone, reach4): Block, vel_to: f32) -> Hold {
        Hold {
            held,
            tone,
            reach4,
            vel_to,
            force_to: self.force_to,
            lift: self.lift,
            bowing: self.bowing,
        }
    }

    fn junction(&self) -> Junction {
        Junction {
            force: self.force,
            bow_vel: self.bow_vel,
            release: self.release,
            bridge: self.bridge,
            nut: self.nut,
            ap: Allpass1::default(),
            lp: self.lp,
        }
    }

    fn keep(&mut self, j: Junction) {
        (self.force, self.bow_vel, self.release) = (j.force, j.bow_vel, j.release);
        (self.bridge, self.nut, self.lp) = (j.bridge, j.nut, j.lp);
    }

    /// A block: in spans where neither the write nor the splice wraps; the
    /// split steps at sample 0, one block in `BOW_SLEW`; the block's gain, BRIGHT's low-pass, and
    /// FORCE's and SPEED's targets computed once.
    pub(super) fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        m: &Macros,
        (f0, vel_to): (f32, f32),
    ) {
        let k = self.hold(self.block(m, f0), vel_to);
        let mut i = 0;
        if self.glides() {
            let s = glide(self.split, bridge_len(m.pos, self.string.delay()));
            if s != self.split {
                out[0] = self.step(&k, s);
                i = 1;
            }
        }
        self.run(&k, &mut out[i..]);
    }

    /// Whether this block's first sample may step the split: one block in
    /// `BOW_SLEW`.
    pub(super) fn glides(&mut self) -> bool {
        let now = self.wait == 0;
        self.wait = if now { BOW_SLEW - 1 } else { self.wait - 1 };
        now
    }

    /// One sample, the split stepped to `to` first, a whole sample at
    /// most: `+1` re-reads the last bridge return and drops the newest
    /// nut-line sample; `−1` writes the nut line's input into both cells,
    /// repeating it. The sum holds, and neither line reads the other's.
    fn step(&mut self, k: &Hold, to: usize) -> f32 {
        let (s, mut j) = (self.split, self.junction());
        let (ring, wp, d, ap) = self.string.guide();
        j.ap = *ap;
        let len = ring.len();
        let ws = wrap(*wp + 1, len);
        let ai = back(ws, to, len);
        let a = if to > s { j.bridge.a1 } else { ring[ai] };
        let (into_nut, into_bridge, y) = j.sample(k, ring[back(ws, d, len)], a);
        ring[ai] = into_nut;
        if to < s {
            ring[back(ws, s, len)] = into_nut;
        }
        ring[ws] = into_bridge;
        (*wp, *ap) = (ws, j.ap);
        self.split = to;
        self.keep(j);
        y
    }

    /// `step`'s sample, the split held, over `out`: in spans where no
    /// index wraps, the junction in registers.
    fn run(&mut self, k: &Hold, out: &mut [f32]) {
        let (s, mut j) = (self.split, self.junction());
        let (ring, wp, d, ap) = self.string.guide();
        j.ap = *ap;
        let len = ring.len();
        let ring = Cell::from_mut(ring).as_slice_of_cells();
        let mut ws = wrap(*wp + 1, len);
        let (mut ai, mut ni) = (back(ws, s, len), back(ws, d, len));
        let mut i = 0;
        while i < out.len() {
            let m = (out.len() - i).min(len - ws).min(len - ai).min(len - ni);
            let cells = ring[ws..ws + m].iter().zip(&ring[ai..ai + m]);
            let span = cells.zip(&ring[ni..ni + m]).zip(&mut out[i..i + m]);
            for (((w, a), n), o) in span {
                let (into_nut, into_bridge, y) = j.sample(k, n.get(), a.get());
                a.set(into_nut);
                w.set(into_bridge);
                *o = y;
            }
            (ws, ai, ni) = (wrap(ws + m, len), wrap(ai + m, len), wrap(ni + m, len));
            i += m;
        }
        (*wp, *ap) = (if ws == 0 { len - 1 } else { ws - 1 }, j.ap);
        self.keep(j);
    }

    /// One sample of `render`, for the tests: bit for bit the same, at
    /// the block's held gain, BRIGHT's low-pass and bow velocity target;
    /// `to`, at a step point, the split's target.
    #[cfg(test)]
    pub(super) fn tick(&mut self, b: Block, vel_to: f32, to: Option<usize>) -> f32 {
        let k = self.hold(b, vel_to);
        let to = to.map_or(self.split, |t| glide(self.split, t));
        self.step(&k, to)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::boxed::Box;
    use std::format;
    use std::vec::Vec;

    use super::*;
    use crate::dsp::modal::ModalParams;
    use crate::dsp::modal::loop_parts::MIN_LINE;
    use crate::dsp::note_to_freq;
    use crate::in_place::by_value;

    fn bowed() -> Box<BowedString> {
        // SAFETY: `init_in_place` writes every field.
        Box::new(unsafe { by_value(BowedString::init_in_place) })
    }

    #[test]
    fn the_bow_table_sticks_at_rest_and_slips_away() {
        // w = 0.15.
        let w4 = width4(0.5, 0.15 / BOW_WIDTH);
        let rho = |dv: f32| push(dv, w4) / dv;
        assert!((rho(1e-4) - 1.0).abs() < 1e-6, "{}", rho(1e-4));
        for dv in [0.15, -0.15] {
            assert!((rho(dv) - 0.5).abs() < 1e-6, "{dv}: {}", rho(dv));
        }
        for dv in [0.6, -0.6] {
            assert!(rho(dv) < 0.01, "{dv}: {}", rho(dv));
        }
    }

    #[test]
    fn the_push_is_bounded_and_finite() {
        for force in [0.0, 0.001, 0.5, 1.0] {
            let w4 = width4(force, 0.3);
            let w = libm::powf(w4, 0.25);
            for i in -10_000..=10_000 {
                let dv = i as f32 * 1e-3;
                let p = push(dv, w4);
                assert!(p.is_finite(), "{force} {dv}");
                assert!(p.abs() <= 0.5700 * w, "{force} {dv}: {p}");
            }
        }
        assert_eq!(push(0.0, 0.0), 0.0);
        for x in [1e-9, 0.3, -2.0, 10.0] {
            assert_eq!(push(x, 0.0), 0.0, "{x}");
        }
    }

    #[test]
    fn the_bridge_filter_is_linear_phase_and_lossless_at_dc() {
        let impulse = |c: f32| {
            let mut b = Bridge::default();
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0].map(|a| b.reflect(a, c))
        };
        // The three-tap, squared: centred two samples on.
        assert_eq!(impulse(0.5), [0.0625, 0.25, 0.375, 0.25, 0.0625, 0.0]);
        assert_eq!(impulse(0.0), [0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        for c in [0.0, 0.25, 0.5] {
            let mut b = Bridge::default();
            let mut y = 0.0;
            for _ in 0..6 {
                y = b.reflect(0.7, c);
            }
            assert!((y - 0.7).abs() < 1e-7, "{c}: {y}");
        }
    }

    #[test]
    fn the_split_fits_the_line() {
        for d in MIN_LINE..=979 {
            let mut last = 0;
            for i in 0..=64 {
                let s = bridge_len(i as f32 / 64.0, d);
                assert!((1..d).contains(&s), "{d} {i}: {s}");
                assert!(s >= last, "{d} {i}: {s} < {last}");
                last = s;
            }
            let s = bridge_len(f32::NAN, d);
            assert!((1..d).contains(&s), "{d} NaN: {s}");
        }
    }

    #[test]
    fn glide_steps_one_sample() {
        assert_eq!(glide(10, 14), 11);
        assert_eq!(glide(10, 7), 9);
        assert_eq!(glide(10, 10), 10);
    }

    /// A free string (force 0) of a period whole samples at `pos`, BRIGHT
    /// open: the allpass a unit delay, each end's filter centred one
    /// sample on. One impulse on the bridge line, at the last write.
    /// A loop line of `d`'s whole period: the ends' filters and a unit allpass.
    fn period_of(d: usize) -> usize {
        d + 1 + ENDS_DELAY as usize
    }

    fn free_string(d: usize, pos: f32) -> Box<BowedString> {
        let mut b = bowed();
        let period = period_of(d) as f32;
        b.string.clear();
        b.string
            .set_period(period, ENDS_DELAY, core::f32::consts::TAU / period);
        assert_eq!(b.string.delay(), d);
        b.split = bridge_len(pos, d);
        let (ring, wp, _, _) = b.string.guide();
        ring[*wp] = 1.0;
        b
    }

    /// Each return in `out`, a period `p` apart from the first, gathered
    /// over the period about it: its centre and what it sums to.
    fn returns(out: &[f32], p: usize) -> Vec<(f32, f32)> {
        let first = (0..p + 16)
            .max_by(|&i, &j| out[i].total_cmp(&out[j]))
            .unwrap();
        let mut r = Vec::new();
        let mut at = first;
        while at + p / 2 < out.len() {
            let span = at.saturating_sub(p / 2)..at + p / 2;
            let sum: f32 = out[span.clone()].iter().sum();
            let m: f32 = span.map(|i| i as f32 * out[i]).sum();
            r.push((m / sum, sum));
            at += p;
        }
        r
    }

    /// Returns centred a period apart, each upright, `LoopGain::MAX` of the
    /// one before: the ends' filters are linear phase, lossless at DC.
    fn once_a_period(out: &[f32], d: usize, label: &str) {
        let p = period_of(d);
        let r = returns(out, p);
        assert!(r.len() >= 3, "{label}: {r:?}");
        for w in r.windows(2) {
            assert!(w[0].1 > 0.0, "{label}: {r:?}");
            let gap = w[1].0 - w[0].0;
            assert!((gap - p as f32).abs() < 1e-2, "{label}: {r:?}");
            let g = w[1].1 / w[0].1;
            assert!((g - LoopGain::MAX).abs() < 1e-4, "{label}: {g}");
        }
    }

    /// The pitch law: two inversions a period, so one period, not two.
    #[test]
    fn an_impulse_comes_back_upright_once_a_period() {
        for d in [100, 979] {
            for pos in [0.0, 0.15, 0.5, 1.0] {
                let mut b = free_string(d, pos);
                let out: Vec<f32> = (0..5 * period_of(d))
                    .map(|_| b.tick((LoopGain::TOP, 1.0, 1.0), 0.0, None))
                    .collect();
                once_a_period(&out, d, &format!("{d} {pos}"));
            }
        }
    }

    /// A step each way, the impulse mid-way down the bridge line for the
    /// first and the nut line for the second: the loop keeps its length.
    #[test]
    fn a_splice_step_keeps_the_loop_length() {
        for d in [200, 979] {
            let p = period_of(d);
            let mut b = free_string(d, 0.5);
            let s = b.split;
            let head: Vec<f32> = (0..p + 16)
                .map(|_| b.tick((LoopGain::TOP, 1.0, 1.0), 0.0, None))
                .collect();
            let c0 = (0..head.len())
                .max_by(|&i, &j| head[i].total_cmp(&head[j]))
                .unwrap();
            let mut out = head;
            // From an output the impulse is in the bridge line `s + 1`
            // samples, then in the nut line.
            for i in p + 16..6 * p {
                let at = (i + p - c0) % p;
                let to = if at == s / 2 {
                    Some(b.split + 1)
                } else if at == s + (d - s) / 2 {
                    Some(b.split - 1)
                } else {
                    None
                };
                out.push(b.tick((LoopGain::TOP, 1.0, 1.0), 0.0, to));
            }
            once_a_period(&out, d, &format!("{d}"));
        }
    }

    /// `render`'s spans are `tick` bit for bit: C3, G1 and the top, POS
    /// gliding, FORCE easing, through a note-off.
    #[test]
    fn render_is_tick_bit_for_bit() {
        const SPEED: f32 = 0.5 * super::super::BOW_SPEED;
        for note in [48, 31, 96, 127] {
            let freq = note_to_freq(note);
            let f0 = freq;
            let (mut fast, mut slow) = (bowed(), bowed());
            for b in [&mut fast, &mut slow] {
                b.start(freq, 48_000, 0.2);
                b.force = 0.4;
                b.force_to = 0.4;
                b.bow_vel = SPEED;
                b.vel_scale = 1.0;
                b.bowing = true;
            }
            for block in 0..40 {
                let p = ModalParams {
                    pos: if block < 10 { 0.2 } else { 0.9 },
                    bright: 0.3,
                    damp: 0.4,
                    ..Default::default()
                };
                let m = Macros::of(&p);
                let force = if block < 5 { 0.4 } else { 0.9 };
                for b in [&mut fast, &mut slow] {
                    if block == 30 {
                        let (held, ..) = b.block(&m, f0);
                        b.lift(held);
                    }
                    if b.bowing {
                        b.force_to = force * b.vel_scale;
                    }
                }
                let k = slow.block(&m, f0);
                let to = slow
                    .glides()
                    .then(|| bridge_len(m.pos, slow.string.delay()));
                let mut a = [0.0; BLOCK_SIZE];
                fast.render(&mut a, &m, (f0, SPEED));
                let b: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                    let step = if i == 0 { to } else { None };
                    slow.tick(k, SPEED, step)
                });
                assert_eq!(
                    a.map(f32::to_bits),
                    b.map(f32::to_bits),
                    "{note}: block {block}"
                );
            }
            let d = fast.string.delay();
            assert!(d < 8 || fast.split > bridge_len(0.2, d), "{note}: glided");
        }
    }
}
