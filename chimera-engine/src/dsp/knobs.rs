//! A Part's knobs (ADR 0112, after 0107 and 0111): every Sound parameter
//! whose spec declares `Smoothing::Knobs` glides here, once a block, toward
//! its stored value before the Part's voices read it: a one-pole of the
//! param's τ on its 0..1 place (`ParamSpec::normalize`), so linear in its
//! `Scale` (CUTOFF in octaves). The FREQ and Q
//! columns' route amounts glide here too (ENV AMT, KEYBD, FM1–3, QM). A
//! voice adds its routes' offsets after, so an envelope, an LFO or a tap
//! keeps its snap.
//!
//! What changed reaches the audio as a `KnobDelta` (ADR 0115): the UI side
//! (`KnobPublisher`) diffs each Part's knobs at a publish, and the audio
//! reads and glides exactly the knobs it names, nothing else. An FX, mix
//! or ORBIT edit names none; a knob turn, one.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::LOG2_E;

use crate::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use crate::block::{GlideMs, ParamSpec};
use crate::dsp::algo::engine::AlgoLive;
use crate::dsp::algo::math::exp2;
use crate::hw::Cost;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModState};
use crate::params::ParamSnapshot;
use crate::preset::Sound;
use crate::project::Loads;
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

/// Some of `KNOBS`, a bit each, and the FREQ and Q route columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KnobSet(u128);

/// The route columns' bit.
const ROUTES: u128 = 1 << 127;

const _: () = assert!(KNOB_COUNT < 127);

impl KnobSet {
    pub const EMPTY: Self = Self(0);
    /// Every knob and the routes: a new Sound.
    pub const ALL: Self = Self(((1u128 << KNOB_COUNT) - 1) | ROUTES);

    fn put(&mut self, i: usize, on: bool) {
        let b = 1u128 << i;
        self.0 = if on { self.0 | b } else { self.0 & !b };
    }

    fn has(self, i: usize) -> bool {
        self.0 & (1 << i) != 0
    }

    fn union(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }

    fn minus(self, o: Self) -> Self {
        Self(self.0 & !o.0)
    }

    fn routes(self) -> bool {
        self.0 & ROUTES != 0
    }

    /// Its knobs, without the routes.
    pub fn len(self) -> u32 {
        (self.0 & !ROUTES).count_ones()
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Its first `n` knobs, and the routes once its knobs all fit.
    fn first(self, n: u32) -> Self {
        let mut out = self.0 & ROUTES;
        let mut bits = self.0 & !ROUTES;
        for _ in 0..n {
            if bits == 0 {
                break;
            }
            out |= bits & bits.wrapping_neg();
            bits &= bits - 1;
        }
        if bits != 0 {
            out &= !ROUTES;
        }
        Self(out)
    }

    fn iter(self) -> impl Iterator<Item = usize> {
        let mut bits = self.0 & !ROUTES;
        core::iter::from_fn(move || {
            (bits != 0).then(|| {
                let i = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                i
            })
        })
    }
}

/// Publishes a delta remembers: an audio block that missed more takes
/// every knob (it never happens at the UI's rate; it stays exact if it
/// does).
pub const DELTA_DEPTH: usize = 4;

/// What changed in a Part's knobs, publish by publish (ADR 0115): the
/// sets of the last `DELTA_DEPTH` publishes that changed any, numbered,
/// and the stamp of the Sound they describe. Only `KnobPublisher` makes a
/// non-empty one: the one way a knob change reaches the audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KnobDelta {
    seq: u32,
    sets: [KnobSet; DELTA_DEPTH],
    of: Stamp,
}

impl KnobDelta {
    /// What changed since publish `seen`.
    fn since(&self, seen: u32) -> KnobSet {
        let n = self.seq.wrapping_sub(seen) as usize;
        match self.sets.get(..n) {
            Some(sets) => sets.iter().fold(KnobSet::EMPTY, |a, &s| a.union(s)),
            None => KnobSet::ALL,
        }
    }

    fn push(&mut self, s: KnobSet) {
        self.sets.copy_within(..DELTA_DEPTH - 1, 1);
        self.sets[0] = s;
        self.seq = self.seq.wrapping_add(1);
    }
}

/// The UI side of the knobs (ADR 0115): each Part's knob values and FREQ
/// and Q route amounts as last published, and its delta. A Part whose
/// Sound's stamp hasn't moved is not diffed: a publish costs a compare a
/// Part, and an edit one diff of the edited Part, on the UI's time. All
/// zeros: `.bss`, no flash copy.
pub struct KnobPublisher {
    parts: [Published; crate::hw::MAX_PARTS],
}

#[derive(Clone, Copy)]
struct Published {
    of: Stamp,
    loads: Loads,
    v: [u32; KNOB_COUNT],
    routes: [[i8; MAX_MOD_SOURCES]; 2],
    delta: KnobDelta,
}

impl Default for KnobPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl KnobPublisher {
    pub const fn new() -> Self {
        Self {
            parts: [Published {
                of: Stamp::NONE,
                loads: Loads::NONE,
                v: [0; KNOB_COUNT],
                routes: [[0; MAX_MOD_SOURCES]; 2],
                delta: KnobDelta {
                    seq: 0,
                    sets: [KnobSet::EMPTY; DELTA_DEPTH],
                    of: Stamp::NONE,
                },
            }; crate::hw::MAX_PARTS],
        }
    }

    /// Part `i`'s delta for `sound`, `loads` its Sound loads: the knobs
    /// whose values differ from the last publish's, every one for a new
    /// Sound.
    pub fn publish(&mut self, i: usize, sound: &Tracked<Sound>, loads: Loads) -> KnobDelta {
        let s = &mut self.parts[i];
        if s.of == sound.stamp() && s.loads == loads {
            return s.delta;
        }
        let new = s.of == Stamp::NONE || s.loads != loads;
        let mut d = if new { KnobSet::ALL } else { KnobSet::EMPTY };
        for (j, k) in KNOBS.iter().enumerate() {
            let p = &sound.params;
            let b = p
                .block(k.addr.block)
                .map_or(0, |b| b.get(k.addr.param).to_bits());
            if b != s.v[j] {
                (s.v[j], d) = (b, d.union(KnobSet(1 << j)));
            }
        }
        let m = &sound.mod_state;
        for (c, col) in COLS.into_iter().enumerate() {
            for src in 0..MAX_MOD_SOURCES {
                let a = if src < m.num_sources() {
                    m.fixed_amount(src, col)
                } else {
                    0
                };
                if a != s.routes[c][src] {
                    (s.routes[c][src], d) = (a, d.union(KnobSet(ROUTES)));
                }
            }
        }
        (s.of, s.loads) = (sound.stamp(), loads);
        if !d.is_empty() {
            s.delta.push(d);
        }
        s.delta.of = sound.stamp();
        s.delta
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
/// them this block. `UNPRIMED`, every knob waits to be read and lands, and
/// a voice rendered on its own plays the stored values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// Each of `KNOBS` as its 0..1 place (`ParamSpec::normalize`): near 1,
    /// not ~10 as `log2` Hz, so an f32 step never stalls short of `LAND`.
    v: [f32; KNOB_COUNT],
    moving: KnobSet,
    /// Changed, not yet read: a silent Part's, past its block's budget.
    pending: KnobSet,
    /// Land, not glide, what is pending: a new Sound's.
    landing: bool,
    /// The FREQ and Q columns' amounts by source, as `amount / 127`.
    routes: [[f32; MAX_MOD_SOURCES]; 2],
    routes_moving: bool,
    /// The last publish taken, and the stamps of what it described: a
    /// value written past the publisher (a test's, the bench's) has
    /// another, and is read whole.
    seq: u32,
    seen: (Stamp, Stamp),
    /// Knobs read, wrapping: for the tests.
    reads: u32,
    /// The sample rate and the KNOB and STAGE coefficients at it: worked
    /// out once, not an `exp2` a knob a block.
    k: (u32, [f32; 2]),
}

impl Knobs {
    /// What a sounding Part's knobs cost, cycles a sample, billed once a
    /// Part on one of its voices (ADR 0112, 0115). Estimated from the
    /// thumb build's instructions, not benched. Each block: the delta's
    /// check (~40 cycles), two knobs gliding (~300) and a Sound's 66 read
    /// at a load spread over a UI frame of 12 blocks or more (~6,000 / 12
    /// = 500): 840 / 64 = 13.1. A knob turn reads one knob; an FX, mix or
    /// ORBIT edit, none.
    pub const COST: Cost = Cost(14);

    pub const UNPRIMED: Self = Self {
        v: [0.0; KNOB_COUNT],
        moving: KnobSet::EMPTY,
        pending: KnobSet::ALL,
        landing: true,
        routes: [[0.0; MAX_MOD_SOURCES]; 2],
        routes_moving: false,
        seq: 0,
        seen: (Stamp::NONE, Stamp::NONE),
        reads: 0,
        k: (0, [0.0; 2]),
    };

    /// A new Sound (its Part's `loads` moved): every knob read and landed.
    pub fn land(&mut self) {
        (self.pending, self.landing) = (KnobSet::ALL, true);
    }

    /// Knobs read so far, wrapping: for the tests.
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// One block: the knobs `d` says changed read, the rest left where
    /// they are, and those on their way glided toward `p`'s and `m`'s
    /// stored values. A silent Part (`pace` `None`) lands them instead,
    /// reading at most `budget` knobs, which it takes from; the rest wait.
    pub fn ease(
        &mut self,
        (p, m): (&Tracked<ParamSnapshot>, &Tracked<ModState>),
        d: &KnobDelta,
        pace: Option<Pace>,
        budget: &mut u32,
    ) {
        if let Some(Pace::Glide(sr)) = pace
            && self.k.0 != sr
        {
            self.k = (sr, [coeff(GlideMs::KNOB, sr), coeff(GlideMs::STAGE, sr)]);
        }
        let stamps = (p.stamp(), m.stamp());
        let new = if stamps == (d.of, d.of) {
            d.since(self.seq)
        } else if stamps != self.seen {
            KnobSet::ALL
        } else {
            KnobSet::EMPTY
        };
        (self.seq, self.seen) = (d.seq, stamps);
        let todo = self.pending.union(new);
        let now = match pace {
            Some(_) => todo,
            None => todo.first(*budget),
        };
        *budget = budget.saturating_sub(now.len());
        self.pending = todo.minus(now);
        let glide = !self.landing && matches!(pace, Some(Pace::Glide(_)));
        let k = glide.then_some(self.k.1);
        for i in now.iter() {
            self.step(i, p, k, true);
        }
        for i in self.moving.minus(now).iter() {
            self.step(i, p, k, false);
        }
        if now.routes() || self.routes_moving {
            let k = k.map_or(0.0, |k| k[0]);
            self.routes_moving = false;
            for (v, col) in self.routes.iter_mut().zip(COLS) {
                self.routes_moving |= glide_column(v, m, col, k);
            }
        }
        self.reads = self.reads.wrapping_add(now.len());
        self.landing &= !self.pending.is_empty();
    }

    /// Knob `i` one block toward `p`'s at the coefficients `k`; landed at
    /// once with none. A `fresh` read starts it if it differs.
    #[inline(never)]
    fn step(&mut self, i: usize, p: &ParamSnapshot, k: Option<[f32; 2]>, fresh: bool) {
        let knob = &KNOBS[i];
        let k = k.filter(|_| !p.moves_itself(knob.addr));
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
        match COLS
            .iter()
            .position(|&x| x == c)
            .filter(|_| !self.pending.routes())
        {
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
    use crate::params::{EngineType, FilterParams};

    fn cutoff() -> ParamAddr {
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)
    }

    const GLIDE: Option<Pace> = Some(Pace::Glide(48_000));

    /// One playing block of `p` and `m` written in place (no publisher):
    /// read whole when their stamps move.
    fn ease(e: &mut Knobs, p: &Tracked<ParamSnapshot>, m: &Tracked<ModState>) {
        e.ease((p, m), &KnobDelta::default(), GLIDE, &mut 0);
    }

    fn rig() -> (Tracked<ParamSnapshot>, Tracked<ModState>, Knobs) {
        let p = Tracked::fresh(ParamSnapshot::default());
        let m = Tracked::fresh(ModState::new());
        let mut k = Knobs::UNPRIMED;
        ease(&mut k, &p, &m);
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
        ease(&mut e, &p, &m);
        assert_eq!(played(&e, &p), 250.0);
        p.filter.cutoff = 1000.0;
        ease(&mut e, &p, &m);
        let first = played(&e, &p);
        // Two octaves × (1 − k) of the way, in octaves.
        let want = 250.0 * exp2(2.0 * (1.0 - coeff(GlideMs::KNOB, 48_000)));
        assert!((first / want - 1.0).abs() < 1e-3, "{first} vs {want}");
        for _ in 0..400 {
            ease(&mut e, &p, &m);
        }
        assert_eq!(played(&e, &p), 1000.0);
        assert_eq!(e.moving, KnobSet::EMPTY);
    }

    #[test]
    fn a_nan_target_never_sticks() {
        let (mut p, m, mut e) = rig();
        let res = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
        p.filter.resonance = f32::NAN;
        ease(&mut e, &p, &m);
        p.filter.resonance = 0.5;
        ease(&mut e, &p, &m);
        assert_eq!(e.place(res), Some((0.5, false)));
    }

    #[test]
    fn settled_reads_nothing_and_applies_nothing() {
        let (p, m, mut e) = rig();
        let before = e;
        ease(&mut e, &p, &m);
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

    /// The publisher names what changed and nothing else; an unwritten
    /// Sound, nothing; a new one, everything.
    #[test]
    fn a_delta_names_only_what_changed() {
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Algo));
        let first = pb.publish(0, &s, Loads::NONE);
        assert_eq!(first.since(0), KnobSet::ALL);
        assert_eq!(pb.publish(0, &s, Loads::NONE), first, "unwritten");
        s.params.filter.cutoff = 500.0;
        let d = pb.publish(0, &s, Loads::NONE);
        let i = KNOBS.iter().position(|k| k.addr == cutoff()).unwrap();
        assert_eq!(d.since(first.seq), KnobSet(1 << i));
        let _ = &mut *s;
        assert_eq!(
            pb.publish(0, &s, Loads::NONE).seq,
            d.seq,
            "written, unchanged"
        );
        s.mod_state.set_fixed(0, FixedCol::Q, 64);
        let r = pb.publish(0, &s, Loads::NONE);
        assert_eq!(r.since(d.seq), KnobSet(ROUTES));
        // Past the window, everything.
        assert_eq!(
            r.since(r.seq.wrapping_sub(DELTA_DEPTH as u32 + 1)),
            KnobSet::ALL
        );
    }

    /// The audio reads the knobs a delta names, one each, and none else.
    #[test]
    fn the_audio_reads_only_what_a_delta_names() {
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Algo));
        let mut e = Knobs::UNPRIMED;
        let publish = |pb: &mut KnobPublisher, s: &Tracked<Sound>| {
            let d = pb.publish(0, s, Loads::NONE);
            let (p, m) = (
                Tracked::new(s.params.clone(), d.of),
                Tracked::new(s.mod_state.clone(), d.of),
            );
            (p, m, d)
        };
        let (p, m, d) = publish(&mut pb, &s);
        e.ease((&p, &m), &d, GLIDE, &mut 0);
        let base = e.reads();
        assert_eq!(base, KNOB_COUNT as u32);
        e.ease((&p, &m), &d, GLIDE, &mut 0);
        assert_eq!(e.reads(), base, "settled");
        s.params.filter.cutoff = 500.0;
        let (p, m, d) = publish(&mut pb, &s);
        e.ease((&p, &m), &d, GLIDE, &mut 0);
        assert_eq!(e.reads(), base + 1, "one knob");
        assert_eq!(e.place(cutoff()).map(|x| x.1), Some(true));
    }

    /// A silent Part reads what changed within its budget, and lands it.
    #[test]
    fn a_silent_part_reads_within_its_budget() {
        let p = Tracked::fresh(ParamSnapshot::default());
        let m = Tracked::fresh(ModState::new());
        let mut e = Knobs::UNPRIMED;
        let mut budget = 10;
        e.ease((&p, &m), &KnobDelta::default(), None, &mut budget);
        assert_eq!((e.reads(), budget), (10, 0));
        for _ in 0..KNOB_COUNT {
            e.ease((&p, &m), &KnobDelta::default(), None, &mut 10);
        }
        assert_eq!(e.reads(), KNOB_COUNT as u32);
        assert!(e.pending.is_empty() && !e.landing);
    }
}
