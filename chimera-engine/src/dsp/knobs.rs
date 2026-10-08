//! A Part's knobs (ADR 0112, after 0107 and 0111): every Sound parameter
//! whose spec declares `Smoothing::Knobs` glides here, once a block, toward
//! its stored value before the Part's voices read it: a one-pole of the
//! param's τ on its 0..1 place (`ParamSpec::normalize`), so linear in its
//! `Scale` (CUTOFF in octaves). The FREQ and Q
//! columns' route amounts glide here too (ENV AMT, KEYBD, FM1–3, QM). A
//! voice adds its routes' offsets after, so an envelope, an LFO or a tap
//! keeps its snap.
//!
//! A settled Part costs two stamp compares a block: the knobs read the
//! Sound again only when its `Stamp` moves, and glide only what moves.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::LOG2_E;

use crate::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use crate::block::{GlideMs, ParamSpec};
use crate::dsp::algo::engine::AlgoLive;
use crate::dsp::algo::math::exp2;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModState};
use crate::params::ParamSnapshot;
use crate::tracked::{Stamp, Tracked};

/// A param the knobs glide: where it is, its spec and its τ.
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub addr: ParamAddr,
    pub spec: &'static ParamSpec,
    pub tau: GlideMs,
}

/// Every param a Sound declares `Smoothing::Knobs`, in `BlockRef::ALL`'s
/// order: the registry is the list, nothing is wired by hand.
pub static KNOBS: [Knob; KNOB_COUNT] = knobs();
pub const KNOB_COUNT: usize = count();

const fn count() -> usize {
    let (mut n, mut b) = (0, 0);
    while b < BlockRef::ALL.len() {
        let r = BlockRef::ALL[b];
        let specs = r.specs();
        let mut i = 0;
        while r.in_sound() && i < specs.len() {
            if specs[i].motion().knob().is_some() {
                n += 1;
            }
            i += 1;
        }
        b += 1;
    }
    n
}

const fn knobs() -> [Knob; KNOB_COUNT] {
    let first = &crate::params::FILTER_SPECS[0];
    let mut t = [Knob {
        addr: ParamAddr::new(BlockRef::Filter, first.id),
        spec: first,
        tau: GlideMs::KNOB,
    }; KNOB_COUNT];
    let (mut n, mut b) = (0, 0);
    while b < BlockRef::ALL.len() {
        let r = BlockRef::ALL[b];
        let specs = r.specs();
        let mut i = 0;
        while r.in_sound() && i < specs.len() {
            if let Some(tau) = specs[i].motion().knob() {
                // `Knobs` keeps a coefficient for each of these two.
                assert!(tau.ms() == GlideMs::KNOB.ms() || tau.ms() == GlideMs::STAGE.ms());
                t[n] = Knob {
                    addr: ParamAddr::new(r, specs[i].id),
                    spec: &specs[i],
                    tau,
                };
                n += 1;
            }
            i += 1;
        }
        b += 1;
    }
    t
}

/// The knobs on their way, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Moving(u128);

const _: () = assert!(KNOB_COUNT <= u128::BITS as usize);

impl Moving {
    fn put(&mut self, i: usize, on: bool) {
        let b = 1u128 << i;
        self.0 = if on { self.0 | b } else { self.0 & !b };
    }

    fn has(self, i: usize) -> bool {
        self.0 & (1 << i) != 0
    }

    fn iter(self) -> impl Iterator<Item = usize> {
        let mut bits = self.0;
        core::iter::from_fn(move || {
            (bits != 0).then(|| {
                let i = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                i
            })
        })
    }
}

/// How the knobs move this block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// Glide, at this sample rate.
    Glide(u32),
    /// Land on the stored values: an offline render that drives a knob
    /// block by block (the factory tapes' MORPH sweep) plays it exactly.
    Land,
}

/// `tau`'s place in `Knobs::k`.
fn tau_ix(tau: GlideMs) -> usize {
    usize::from(tau == GlideMs::STAGE)
}

/// The one-pole's coefficient once a block at `tau`.
pub fn coeff(tau: GlideMs, sample_rate: u32) -> f32 {
    exp2(-LOG2_E * BLOCK_SIZE as f32 / (tau.secs() * sample_rate as f32))
}

/// The columns whose route amounts glide: FREQ and Q.
const COLS: [FixedCol; 2] = [FixedCol::Freq, FixedCol::Q];

/// Closer than this to its target place, a knob lands on it: 1e-5 of a
/// span, 1e-4 octave of CUTOFF (0.12 c).
const LAND: f32 = 1e-5;

/// A Part's knobs on their way to the stored values, as its voices play
/// them this block. `UNPRIMED`, the next `ease` lands, and a voice rendered
/// on its own plays the stored values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// Each of `KNOBS` as its 0..1 place (`ParamSpec::normalize`): near 1,
    /// not ~10 as `log2` Hz, so an f32 step never stalls short of `LAND`.
    v: [f32; KNOB_COUNT],
    moving: Moving,
    /// The FREQ and Q columns' amounts by source, as `amount / 127`.
    routes: [[f32; MAX_MOD_SOURCES]; 2],
    routes_moving: bool,
    /// The Sound's and its routes' stamps when last read.
    seen: (Stamp, Stamp),
    primed: bool,
    /// The sample rate and the KNOB and STAGE coefficients at it: worked
    /// out once, not an `exp2` a knob a block.
    k: (u32, [f32; 2]),
}

impl Knobs {
    pub const UNPRIMED: Self = Self {
        v: [0.0; KNOB_COUNT],
        moving: Moving(0),
        routes: [[0.0; MAX_MOD_SOURCES]; 2],
        routes_moving: false,
        seen: (Stamp::new(0, 0), Stamp::new(0, 0)),
        primed: false,
        k: (0, [0.0; 2]),
    };

    /// The next `ease` lands: a silent Part, or a Sound loaded under a
    /// sounding voice.
    pub fn land(&mut self) {
        self.primed = false;
    }

    /// One block toward `p`'s and `m`'s stored values. Reads the Sound
    /// only when its stamp moved, and glides only what is moving.
    pub fn ease(&mut self, p: &Tracked<ParamSnapshot>, m: &Tracked<ModState>, pace: Pace) {
        if let Pace::Glide(sr) = pace
            && self.k.0 != sr
        {
            self.k = (sr, [coeff(GlideMs::KNOB, sr), coeff(GlideMs::STAGE, sr)]);
        }
        let k = (self.primed && matches!(pace, Pace::Glide(_))).then_some(self.k.1);
        if k.is_none() || p.stamp() != self.seen.0 {
            self.seen.0 = p.stamp();
            for i in 0..KNOB_COUNT {
                self.step(i, p, k, true);
            }
        } else {
            for i in self.moving.iter() {
                self.step(i, p, k, false);
            }
        }
        if k.is_none() || m.stamp() != self.seen.1 || self.routes_moving {
            self.seen.1 = m.stamp();
            let k = k.map_or(0.0, |k| k[0]);
            self.routes_moving = false;
            for (v, col) in self.routes.iter_mut().zip(COLS) {
                self.routes_moving |= glide_column(v, m, col, k);
            }
        }
        self.primed = true;
    }

    /// Knob `i` one block toward `p`'s at the coefficients `k`; landed at
    /// once with none. A `fresh` read starts it if it differs.
    #[inline(never)]
    fn step(&mut self, i: usize, p: &ParamSnapshot, k: Option<[f32; 2]>, fresh: bool) {
        let knob = &KNOBS[i];
        let t = knob.spec.normalize(
            p.block(knob.addr.block)
                .map_or(0.0, |b| b.get(knob.addr.param)),
        );
        let v = &mut self.v[i];
        let on = match k {
            None => {
                *v = t;
                false
            }
            Some(_) if fresh && !self.moving.has(i) && v.to_bits() == t.to_bits() => false,
            Some(k) => !glide(v, t, k[tau_ix(knob.tau)], LAND),
        };
        self.moving.put(i, on);
    }

    /// The knobs still on their way written over `p`'s stored values, or
    /// `live`'s for ALGO's (its bytes can't hold a value between steps).
    /// The rest are where they landed: stored, bit for bit.
    pub fn apply(&self, p: &mut ParamSnapshot, live: &mut AlgoLive) {
        for i in self.moving.iter() {
            let (a, v) = (KNOBS[i].addr, KNOBS[i].spec.denormalize(self.v[i]));
            if !live.set(a, v)
                && let Some(b) = p.block_mut(a.block)
            {
                b.write(a.param, v);
            }
        }
    }

    /// Fixed column `c`'s offset from the sources `v`: FREQ and Q by the
    /// eased amounts, VCA and FOLD (and all, unprimed) by `m`'s.
    #[inline(never)]
    pub fn fixed_sum(&self, m: &ModState, c: FixedCol, v: &[f32; MAX_MOD_SOURCES]) -> f32 {
        match COLS.iter().position(|&x| x == c).filter(|_| self.primed) {
            Some(i) => self.routes[i]
                .iter()
                .zip(v)
                .filter(|(a, _)| **a != 0.0)
                .map(|(a, x)| a * x)
                .sum(),
            None => m.fixed_sum(c, v),
        }
    }

    /// Knob `addr`'s 0..1 place this block, and whether it is still on its
    /// way: for the tests.
    pub fn place(&self, addr: ParamAddr) -> Option<(f32, bool)> {
        let i = KNOBS.iter().position(|k| k.addr == addr)?;
        Some((self.v[i], self.moving.has(i)))
    }
}

/// Column `col`'s amounts one block toward `m`'s; whether any still moves.
/// Out of line, called once a column.
#[inline(never)]
fn glide_column(v: &mut [f32; MAX_MOD_SOURCES], m: &ModState, col: FixedCol, k: f32) -> bool {
    // `m`'s sources only: one loop, not eight copies; a source it lacks
    // routes nothing.
    let (live, absent) = v.split_at_mut(m.num_sources().min(MAX_MOD_SOURCES));
    let mut moving = false;
    for (s, v) in live.iter_mut().enumerate() {
        // `amount_scale`'s value: the same IEEE divide.
        let t = f32::from(m.fixed_amount(s, col).max(-127)) / 127.0;
        moving |= !glide(v, t, k, LAND);
    }
    absent.fill(0.0);
    moving
}

/// `v` one block of the one-pole toward `t`, landing within `land`;
/// whether it landed. A NaN lands too, so a bad target never sticks once
/// it is fixed.
fn glide(v: &mut f32, t: f32, k: f32, land: f32) -> bool {
    let d = k * (*v - t);
    *v = if d.abs() < land || d.is_nan() {
        t
    } else {
        t + d
    };
    v.to_bits() == t.to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Block;
    use crate::params::FilterParams;

    fn cutoff() -> ParamAddr {
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)
    }

    fn rig() -> (Tracked<ParamSnapshot>, Tracked<ModState>, Knobs) {
        let p = Tracked::new(ParamSnapshot::default(), Stamp::new(0, 0));
        let m = Tracked::new(ModState::new(), Stamp::new(0, 0));
        let mut k = Knobs::UNPRIMED;
        k.ease(&p, &m, Pace::Glide(48_000));
        (p, m, k)
    }

    /// The cutoff a voice is handed.
    fn played(e: &Knobs, p: &ParamSnapshot) -> f32 {
        let mut q = p.clone();
        let mut live = AlgoLive::from_params(&q.algo);
        e.apply(&mut q, &mut live);
        q.filter.get(FilterParams::CUTOFF)
    }

    #[test]
    fn the_registry_is_the_list() {
        for k in &KNOBS {
            assert!(k.addr.block.in_sound());
            assert_eq!(k.addr.spec().map(|s| s.ident), Some(k.spec.ident));
        }
        assert!(KNOBS.iter().any(|k| k.addr == cutoff()));
    }

    #[test]
    fn a_cutoff_jump_glides_in_octaves_and_lands_exactly() {
        let (mut p, m, mut e) = rig();
        p.filter.cutoff = 250.0;
        e.land();
        e.ease(&p, &m, Pace::Glide(48_000));
        assert_eq!(played(&e, &p), 250.0);
        p.filter.cutoff = 1000.0;
        e.ease(&p, &m, Pace::Glide(48_000));
        let first = played(&e, &p);
        // Two octaves × (1 − k) of the way, in octaves.
        let want = 250.0 * exp2(2.0 * (1.0 - coeff(GlideMs::KNOB, 48_000)));
        assert!((first / want - 1.0).abs() < 1e-3, "{first} vs {want}");
        for _ in 0..400 {
            e.ease(&p, &m, Pace::Glide(48_000));
        }
        assert_eq!(played(&e, &p), 1000.0);
        assert_eq!(e.moving, Moving(0));
    }

    #[test]
    fn a_nan_target_never_sticks() {
        let (mut p, m, mut e) = rig();
        let res = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
        p.filter.resonance = f32::NAN;
        e.ease(&p, &m, Pace::Glide(48_000));
        p.filter.resonance = 0.5;
        e.ease(&p, &m, Pace::Glide(48_000));
        assert_eq!(e.place(res), Some((0.5, false)));
    }

    #[test]
    fn settled_reads_nothing_and_applies_nothing() {
        let (p, m, mut e) = rig();
        let before = e;
        e.ease(&p, &m, Pace::Glide(48_000));
        assert_eq!(e, before);
        assert_eq!(played(&e, &p), p.filter.cutoff);
    }

    #[test]
    fn unprimed_sums_the_stored_routes() {
        let mut m = ModState::with_sources(MAX_MOD_SOURCES);
        m.set_fixed(0, FixedCol::Freq, 127);
        let v = [0.5; MAX_MOD_SOURCES];
        assert_eq!(
            Knobs::UNPRIMED.fixed_sum(&m, FixedCol::Freq, &v),
            m.fixed_sum(FixedCol::Freq, &v)
        );
    }
}
