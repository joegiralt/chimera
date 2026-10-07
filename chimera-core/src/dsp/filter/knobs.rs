//! A Part's filter knobs, eased once a block before its voices read them
//! (CLAUDE.md: never snap; ADR 0107). CUTOFF glides in octaves, RES and the
//! FREQ and Q columns' amounts (ENV AMT, KEYBD, FM1–3, QM) linearly, each a
//! one-pole of `KNOB_EASE_S`. A voice adds its routes' offsets after, so an
//! envelope, an LFO or a tap keeps its snap, and every TYPE gets the glide:
//! it is before the filter, not in it.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::LOG2_E;

use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::modulator::law::CUTOFF_HZ;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModState};
use crate::params::FilterParams;

/// The knobs' time constant, seconds: a lone detent spreads over ~90 ms
/// (3τ), and a turn trails the hand by 30 ms.
pub const KNOB_EASE_S: f32 = 0.03;

/// The one-pole's coefficient once a block at `KNOB_EASE_S`.
pub fn knob_coeff(sample_rate: u32) -> f32 {
    exp2(-LOG2_E * BLOCK_SIZE as f32 / (KNOB_EASE_S * sample_rate as f32))
}

/// The columns `Knobs` eases: FREQ and Q.
const COLS: [FixedCol; 2] = [FixedCol::Freq, FixedCol::Q];

/// Closer than this to its target, a knob lands on it: 1e-4 octave of
/// CUTOFF (0.12 c), 1e-5 of RES or an amount.
const LAND: f32 = 1e-5;

/// A Part's knobs on their way to the stored values, as its voices play
/// them this block. `UNPRIMED`, the first block lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// CUTOFF as its place on `CUTOFF_HZ` in octaves, 0..1 (near 1, not ~10
    /// as `log2` Hz, so an f32 step never stalls short of `LAND`), and RES.
    knobs: [f32; 2],
    /// The FREQ and Q columns' amounts by source, as `amount / 127`.
    routes: [[f32; MAX_MOD_SOURCES]; 2],
    /// CUTOFF in Hz, from `knobs[0]`.
    cutoff: f32,
    primed: bool,
}

impl Knobs {
    /// Unprimed: the first block lands. `default()`, as a const.
    pub const UNPRIMED: Self = Self {
        knobs: [0.0; 2],
        routes: [[0.0; MAX_MOD_SOURCES]; 2],
        cutoff: 0.0,
        primed: false,
    };

    /// The stored knobs, uneased: a voice rendered on its own.
    pub fn stored(p: &FilterParams, m: &ModState) -> Self {
        let mut k = Self::UNPRIMED;
        k.ease(p, m, 0.0);
        k
    }

    /// The next `ease` lands: a Sound loaded under a sounding voice.
    pub fn land(&mut self) {
        self.primed = false;
    }

    /// One block toward `p` and `m`'s knobs, `k` from `knob_coeff`; 0
    /// lands them (nothing hears a silent Part move).
    pub fn ease(&mut self, p: &FilterParams, m: &ModState, k: f32) {
        let k = if self.primed { k } else { 0.0 };
        self.primed = true;
        let c = log2(p.cutoff / CUTOFF_HZ.min) / CUTOFF_HZ.oct;
        glide(&mut self.knobs[0], c, k);
        glide(&mut self.knobs[1], p.resonance, k);
        for (v, col) in self.routes.iter_mut().zip(COLS) {
            glide_column(v, m, col, k);
        }
        // Landed, the stored value itself.
        self.cutoff = if self.knobs[0] == c {
            p.cutoff
        } else {
            CUTOFF_HZ.min * exp2(self.knobs[0] * CUTOFF_HZ.oct)
        };
    }

    /// CUTOFF and RES set to the eased ones.
    pub fn apply(&self, p: &mut FilterParams) {
        (p.cutoff, p.resonance) = (self.cutoff, self.knobs[1]);
    }

    /// Fixed column `c`'s offset from the sources `v`: FREQ and Q by the
    /// eased amounts, VCA and FOLD by `m`'s.
    #[inline(never)]
    pub fn fixed_sum(&self, m: &ModState, c: FixedCol, v: &[f32; MAX_MOD_SOURCES]) -> f32 {
        match COLS.iter().position(|&x| x == c) {
            Some(i) => self.routes[i]
                .iter()
                .zip(v)
                .filter(|(a, _)| **a != 0.0)
                .map(|(a, x)| a * x)
                .sum(),
            None => m.fixed_sum(c, v),
        }
    }
}

/// Column `col`'s amounts one block toward `m`'s. Out of line, called
/// once a column.
#[inline(never)]
fn glide_column(v: &mut [f32; MAX_MOD_SOURCES], m: &ModState, col: FixedCol, k: f32) {
    // `m`'s sources only: one loop, not eight copies; a source it lacks
    // routes nothing.
    let (live, absent) = v.split_at_mut(m.num_sources().min(MAX_MOD_SOURCES));
    for (s, v) in live.iter_mut().enumerate() {
        // `amount_scale`'s value: the same IEEE divide.
        glide(v, f32::from(m.fixed_amount(s, col).max(-127)) / 127.0, k);
    }
    absent.fill(0.0);
}

/// `v` one block of the one-pole toward `t`, landing within `LAND`. A NaN
/// lands too, so a bad target never sticks once it is fixed. Out of line:
/// called 18 times.
#[inline(never)]
fn glide(v: &mut f32, t: f32, k: f32) {
    let d = k * (*v - t);
    *v = if d.abs() < LAND || d.is_nan() {
        t
    } else {
        t + d
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cutoff_jump_glides_in_octaves_and_lands_exactly() {
        let k = knob_coeff(48_000);
        let m = ModState::new();
        let mut p = FilterParams::default();
        p.cutoff = 250.0;
        let mut e = Knobs::UNPRIMED;
        e.ease(&p, &m, k);
        assert_eq!(e.cutoff, 250.0);
        p.cutoff = 1000.0;
        e.ease(&p, &m, k);
        let first = e.cutoff;
        // Two octaves × (1 − k) of the way, in octaves.
        let want = 250.0 * exp2(2.0 * (1.0 - k));
        assert!((first / want - 1.0).abs() < 1e-3, "{first} vs {want}");
        let mut last = first;
        for _ in 0..400 {
            e.ease(&p, &m, k);
            last = e.cutoff;
        }
        assert_eq!(last, 1000.0);
    }

    #[test]
    fn a_nan_target_never_sticks() {
        let (k, m) = (knob_coeff(48_000), ModState::new());
        let mut p = FilterParams::default();
        p.resonance = 0.4;
        let mut e = Knobs::UNPRIMED;
        e.ease(&p, &m, k);
        p.resonance = f32::NAN;
        e.ease(&p, &m, k);
        p.resonance = 0.5;
        e.ease(&p, &m, k);
        assert_eq!(e.knobs[1], 0.5);
    }

    #[test]
    fn k_zero_lands_and_stored_is_the_stored() {
        let mut m = ModState::with_sources(MAX_MOD_SOURCES);
        m.set_fixed(0, FixedCol::Freq, 127);
        let mut p = FilterParams::default();
        p.resonance = 0.4;
        let s = Knobs::stored(&p, &m);
        assert_eq!((s.cutoff, s.knobs[1]), (p.cutoff, 0.4));
        let v = [0.5; MAX_MOD_SOURCES];
        assert_eq!(
            s.fixed_sum(&m, FixedCol::Freq, &v),
            m.fixed_sum(FixedCol::Freq, &v)
        );
    }
}
