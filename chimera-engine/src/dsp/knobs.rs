//! A Part's knobs (ADR 0112, after 0107 and 0111): every Sound parameter
//! whose spec declares `Smoothing::Knobs` glides here, once a block, toward
//! its stored value before the Part's voices read it: a one-pole of the
//! param's τ on its 0..1 place (`ParamSpec::normalize`), so linear in its
//! `Scale` (CUTOFF in octaves). The FREQ and Q
//! columns' route amounts glide here too (ENV AMT, KEYBD, FM1–3, QM). A
//! voice adds its routes' offsets after, so an envelope, an LFO or a tap
//! keeps its snap.
//!
//! What changed reaches the audio as a `KnobDelta` (ADR 0115, 0116): the
//! UI side (`KnobPublisher`) diffs each Part's knobs at a publish and
//! sends each change as a move, from and to; the audio stores the target
//! and glides, and never reads the Sound for a knob. An FX, mix or ORBIT
//! edit sends none; a knob turn, one; a new Sound none (it lands).

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::LOG2_E;

use crate::addr::{BlockRef, Blocks, ParamAddr};
use crate::block::{GlideMs, ParamSpec};
use crate::dsp::algo::engine::AlgoLive;
use crate::dsp::algo::math::exp2;
use crate::hw::Cost;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModState};
use crate::params::ParamSnapshot;
use crate::preset::Sound;
use crate::project::Loads;
use crate::tracked::{Stamp, Tracked};

/// A param the knobs glide: where it is, its spec and its τ, and its read
/// plan: where its value lies in a `ParamSnapshot`, as what, and how it is
/// placed on 0..1, worked out at compile time, so a read on the audio
/// thread is a load and a multiply (no dispatch, no spec walk, no divide).
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub addr: ParamAddr,
    pub spec: &'static ParamSpec,
    pub tau: GlideMs,
    /// Its field's offset in `ParamSnapshot`, and its type.
    at: u16,
    kind: Kind,
    /// Its place is `(v − lo) × k` (`k` = 1 / span); in octaves (CUTOFF,
    /// the one), its spec's `normalize`.
    lo: f32,
    k: f32,
    octaves: bool,
}

/// How a knob's value is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    F32,
    U8,
    I8,
}

impl Knob {
    /// Its 0..1 place in `p`: `spec.normalize(get)`, read straight from
    /// its field.
    #[inline]
    pub fn read(&self, p: &ParamSnapshot) -> f32 {
        self.place(self.value(p))
    }

    /// Its value in `p`, as `get` gives it: straight from its field.
    #[inline]
    pub fn value(&self, p: &ParamSnapshot) -> f32 {
        let base = core::ptr::from_ref(p).cast::<u8>();
        // SAFETY: `at` is `offset_of!` a field of `kind`'s type in
        // `ParamSnapshot` (`plan`), so the read is in bounds, aligned for
        // that type (the field's own offset) and of an initialised value.
        unsafe {
            let f = base.add(self.at as usize);
            match self.kind {
                Kind::F32 => f.cast::<f32>().read(),
                Kind::U8 => f32::from(f.read()),
                Kind::I8 => f32::from(f.cast::<i8>().read()),
            }
        }
    }

    /// Value `v`'s 0..1 place, as `spec.normalize` places it.
    #[inline]
    pub fn place(&self, v: f32) -> f32 {
        if self.octaves {
            self.spec.normalize(v)
        } else {
            (v - self.lo) * self.k
        }
    }
}

/// Knob `a`'s field: its offset in `ParamSnapshot` and its type. A knob
/// param it doesn't know fails to compile (`KNOBS` is built with it), and
/// `the_read_plan_reads_what_get_does` holds each against its block's
/// `get`.
const fn plan(a: ParamAddr) -> (usize, Kind) {
    use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
    use crate::dsp::lfo::LfoParams;
    use crate::dsp::modal::ModalParams;
    use crate::dsp::tape_engine::TapeEngineParams as T;
    use crate::dsp::va::VaParams as V;
    use crate::dsp::voice_fx::params::VoiceFxParams as X;
    use crate::params::{EnvParams, FilterParams, OutParams, PitchParams};
    use core::mem::{offset_of, size_of};
    type P = ParamSnapshot;
    let id = a.param.0;
    let (at, kind) = match a.block {
        BlockRef::Modal => {
            let f = if id == ModalParams::STRUCTURE.0 {
                offset_of!(ModalParams, structure)
            } else if id == ModalParams::BRIGHT.0 {
                offset_of!(ModalParams, bright)
            } else if id == ModalParams::DAMP.0 {
                offset_of!(ModalParams, damp)
            } else if id == ModalParams::POS.0 {
                offset_of!(ModalParams, pos)
            } else if id == ModalParams::FORCE.0 {
                offset_of!(ModalParams, force)
            } else if id == ModalParams::SPEED.0 {
                offset_of!(ModalParams, speed)
            } else {
                panic!("a MODAL knob without a field")
            };
            (offset_of!(P, modal) + f, Kind::F32)
        }
        BlockRef::Algo if id == AlgoParams::MORPH.0 => (offset_of!(P, algo.morph), Kind::U8),
        BlockRef::Algo if id == AlgoParams::VOWEL.0 => (offset_of!(P, algo.vowel), Kind::U8),
        BlockRef::AlgoOp(op) => {
            let (f, kind) = if id == AlgoOpParams::FINE.0 {
                (offset_of!(AlgoOpParams, fine), Kind::U8)
            } else if id == AlgoOpParams::DETUNE.0 {
                (offset_of!(AlgoOpParams, detune), Kind::I8)
            } else if id == AlgoOpParams::LEVEL.0 {
                (offset_of!(AlgoOpParams, level), Kind::U8)
            } else if id == AlgoOpParams::FEEDBACK.0 {
                (offset_of!(AlgoOpParams, feedback), Kind::U8)
            } else if id == AlgoOpParams::VELOCITY.0 {
                (offset_of!(AlgoOpParams, velocity), Kind::U8)
            } else if id == AlgoOpParams::FRMT.0 {
                (offset_of!(AlgoOpParams, frmt), Kind::U8)
            } else if id == AlgoOpParams::BW.0 {
                (offset_of!(AlgoOpParams, bw), Kind::U8)
            } else {
                panic!("an ALGO op knob without a field")
            };
            let op = offset_of!(P, algo.ops) + op.index() * size_of::<AlgoOpParams>();
            (op + f, kind)
        }
        BlockRef::Filter => {
            let f = if id == FilterParams::CUTOFF.0 {
                offset_of!(FilterParams, cutoff)
            } else if id == FilterParams::RESONANCE.0 {
                offset_of!(FilterParams, resonance)
            } else if id == FilterParams::LEVEL.0 {
                offset_of!(FilterParams, level)
            } else if id == FilterParams::STAB.0 {
                offset_of!(FilterParams, stability)
            } else if id == FilterParams::FB.0 {
                offset_of!(FilterParams, fb)
            } else {
                panic!("a filter knob without a field")
            };
            (offset_of!(P, filter) + f, Kind::F32)
        }
        BlockRef::VoiceFx => {
            let f = if id == X::P1.0 {
                offset_of!(X, pots)
            } else if id == X::P2.0 {
                offset_of!(X, pots) + size_of::<f32>()
            } else if id == X::P3.0 {
                offset_of!(X, pots) + 2 * size_of::<f32>()
            } else if id == X::MIX.0 {
                offset_of!(X, mix)
            } else {
                panic!("an FX knob without a field")
            };
            (offset_of!(P, fx) + f, Kind::F32)
        }
        BlockRef::Env(s) => {
            let f = if id == EnvParams::SUSTAIN.0 {
                offset_of!(EnvParams, sustain)
            } else if id == EnvParams::SHAPE.0 {
                offset_of!(EnvParams, func.shape)
            } else {
                panic!("an ENV knob without a field")
            };
            let e = offset_of!(P, envelopes) + s.index() * size_of::<EnvParams>();
            (e + f, Kind::F32)
        }
        BlockRef::Lfo(s) => {
            let f = if id == LfoParams::DEPTH.0 {
                offset_of!(LfoParams, depth)
            } else if id == LfoParams::SHAPE_B.0 {
                offset_of!(LfoParams, func.shape)
            } else {
                panic!("an LFO knob without a field")
            };
            let l = offset_of!(P, lfos) + s.index() * size_of::<LfoParams>();
            (l + f, Kind::F32)
        }
        BlockRef::Out => {
            let f = if id == OutParams::VOLUME.0 {
                offset_of!(OutParams, volume)
            } else if id == OutParams::VCA_VEL.0 {
                offset_of!(OutParams, vca_vel)
            } else {
                panic!("an OUT knob without a field")
            };
            (offset_of!(P, out) + f, Kind::F32)
        }
        BlockRef::Pitch if id == PitchParams::FINE.0 => (offset_of!(P, pitch.fine), Kind::F32),
        BlockRef::TapeEngine => {
            let f = if id == T::POSITION.0 {
                offset_of!(T, position)
            } else if id == T::MOTION.0 {
                offset_of!(T, motion)
            } else if id == T::TAPE.0 {
                offset_of!(T, tape)
            } else if id == T::AGE.0 {
                offset_of!(T, age)
            } else if id == T::SIZE.0 {
                offset_of!(T, size)
            } else {
                panic!("a TAPE knob without a field")
            };
            (offset_of!(P, tape) + f, Kind::F32)
        }
        BlockRef::Va => {
            let f = if id == V::WAVE1.0 {
                offset_of!(V, wave)
            } else if id == V::WAVE2.0 {
                offset_of!(V, wave) + size_of::<f32>()
            } else if id == V::AMOUNT.0 {
                offset_of!(V, amount)
            } else if id == V::SUB.0 {
                offset_of!(V, sub)
            } else if id == V::STAB.0 {
                offset_of!(V, stability)
            } else if id == V::FINE1.0 {
                offset_of!(V, fine)
            } else if id == V::FINE2.0 {
                offset_of!(V, fine) + size_of::<f32>()
            } else if id == V::SHAPE1.0 {
                offset_of!(V, shape)
            } else if id == V::SHAPE2.0 {
                offset_of!(V, shape) + size_of::<f32>()
            } else {
                panic!("a VA knob without a field")
            };
            (offset_of!(P, va) + f, Kind::F32)
        }
        _ => panic!("a knob on a block without a read plan"),
    };
    (at, kind)
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
        at: 0,
        kind: Kind::F32,
        lo: 0.0,
        k: 0.0,
        octaves: false,
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
                let (addr, spec) = (ParamAddr::new(r, specs[i].id), &specs[i]);
                let (at, kind) = plan(addr);
                assert!(at <= u16::MAX as usize);
                let span = spec.max - spec.min;
                t[n] = Knob {
                    addr,
                    spec,
                    tau,
                    at: at as u16,
                    kind,
                    lo: spec.min,
                    k: if span == 0.0 { 0.0 } else { 1.0 / span },
                    octaves: matches!(spec.scale, crate::block::Scale::Octaves),
                };
                n += 1;
            }
            i += 1;
        }
        b += 1;
    }
    t
}

/// Some of `KNOBS`, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KnobSet(u128);

const _: () = assert!(KNOB_COUNT < 128);

impl KnobSet {
    pub const EMPTY: Self = Self(0);

    fn put(&mut self, i: usize, on: bool) {
        let b = 1u128 << i;
        self.0 = if on { self.0 | b } else { self.0 & !b };
    }

    fn has(self, i: usize) -> bool {
        self.0 & (1 << i) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
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

/// One knob's change at a publish: where its place was and where it goes
/// (ADR 0116), exactly as the read plan places them. `from == to` lands it
/// there (a jog, which moves itself).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KnobMove {
    knob: u8,
    from: f32,
    to: f32,
}

/// A move's `knob` for the FREQ and Q route columns: read them again.
const ROUTES: u8 = u8::MAX;

/// Moves a delta remembers. A block that missed more lands every knob on
/// its stored value, as a new Sound does: a publish of more changes than
/// this at once, which only a bulk write makes, does the same. Six: a
/// delta of 80 B in each of `AudioShared`'s three slots a Part.
pub const MOVES: usize = 6;

/// What changed in a Part's knobs (ADR 0115, 0116): its last `MOVES`
/// moves, counted, and the stamp of the Sound they lead to. Only
/// `KnobPublisher` makes a non-empty one: the one way a knob change
/// reaches the audio, which then reads no knob.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KnobDelta {
    count: u32,
    moves: [KnobMove; MOVES],
    of: Stamp,
}

impl KnobDelta {
    /// Moves pushed so far, wrapping; an overflow adds `MOVES + 1`: for
    /// the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The moves since `seen` was the count, oldest first; `None` past the
    /// window.
    fn since(&self, seen: u32) -> Option<impl Iterator<Item = KnobMove> + '_> {
        let n = self.count.wrapping_sub(seen);
        (n as usize <= MOVES).then(|| (seen..self.count).map(|c| self.moves[c as usize % MOVES]))
    }

    fn push(&mut self, m: KnobMove) {
        self.moves[self.count as usize % MOVES] = m;
        self.count = self.count.wrapping_add(1);
    }

    /// Past the window, so the audio lands every knob.
    fn overflow(&mut self) {
        self.count = self.count.wrapping_add(MOVES as u32 + 1);
    }
}

/// The UI side of the knobs (ADR 0115, 0116): each Part's knob values and
/// FREQ and Q route amounts as last published, and its delta. A Part
/// whose Sound's stamp hasn't moved is not diffed: a publish costs a
/// compare a Part, and an edit one diff of the edited Part, reading and
/// placing its knobs on the UI's time. All zeros: `.bss`, no flash copy.
pub struct KnobPublisher {
    parts: [Published; crate::hw::MAX_PARTS],
}

#[derive(Clone, Copy)]
struct Published {
    of: Stamp,
    loads: Loads,
    v: [u32; KNOB_COUNT],
    /// The knobs its Sound moves itself (SCRUB's POSITION): a move to them
    /// lands, and a change of it is a move.
    own: KnobSet,
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
                own: KnobSet::EMPTY,
                routes: [[0; MAX_MOD_SOURCES]; 2],
                delta: KnobDelta {
                    count: 0,
                    moves: [KnobMove {
                        knob: 0,
                        from: 0.0,
                        to: 0.0,
                    }; MOVES],
                    of: Stamp::NONE,
                },
            }; crate::hw::MAX_PARTS],
        }
    }

    /// Part `i`'s delta for `sound`, `loads` its Sound loads: a move for
    /// each knob whose value differs from the last publish's. A new Sound
    /// sends none: the audio lands it (its `loads` moved).
    pub fn publish(&mut self, i: usize, sound: &Tracked<Sound>, loads: Loads) -> KnobDelta {
        let s = &mut self.parts[i];
        if s.of == sound.stamp() && s.loads == loads {
            return s.delta;
        }
        let new = s.of == Stamp::NONE || s.loads != loads;
        let p = &sound.params;
        let (mut moves, mut n) = ([KnobMove::default(); MOVES + 1], 0);
        let mut own = KnobSet::EMPTY;
        for (j, k) in KNOBS.iter().enumerate() {
            let v = k.value(p);
            own.put(j, p.moves_itself(k.addr));
            let flips = own.has(j) != s.own.has(j);
            if !new && (v.to_bits() != s.v[j] || flips) {
                let to = k.place(v);
                let from = if own.has(j) {
                    to
                } else {
                    k.place(f32::from_bits(s.v[j]))
                };
                moves[n.min(MOVES)] = KnobMove {
                    knob: j as u8,
                    from,
                    to,
                };
                n += 1;
            }
            s.v[j] = v.to_bits();
        }
        let m = &sound.mod_state;
        let mut routes = false;
        for (was, col) in s.routes.iter_mut().zip(COLS) {
            for (src, was) in was.iter_mut().enumerate() {
                let a = if src < m.num_sources() {
                    m.fixed_amount(src, col)
                } else {
                    0
                };
                routes |= a != *was;
                *was = a;
            }
        }
        if routes && !new {
            moves[n.min(MOVES)] = KnobMove {
                knob: ROUTES,
                from: 0.0,
                to: 0.0,
            };
            n += 1;
        }
        (s.of, s.loads, s.own) = (sound.stamp(), loads, own);
        if n > MOVES {
            s.delta.overflow();
        } else {
            for m in &moves[..n] {
                s.delta.push(*m);
            }
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

/// Closer than this to its target, the bend lands: a hundredth of a cent.
const BEND_LAND: f32 = 1e-4;

/// Which moving paths a Part's gliding knobs feed (`Knobs::gliding`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gliding {
    pub filter: bool,
    pub fx: bool,
}

/// A Part's knobs on their way to the stored values, as its voices play
/// them this block. A knob not moving plays its stored value; one moving
/// glides from where it was toward its target, both from the publisher's
/// moves (ADR 0116), so the audio reads no knob. `UNPRIMED`, nothing
/// moves. The fields a settled block reads come first.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Knobs {
    /// Something moves, or the route columns wait to be read: a settled
    /// Part has neither, and a block of it is three compares (`ease`).
    busy: bool,
    /// The last delta count taken, and the stamps of what it led to: a
    /// value written past the publisher (a test's, the bench's) has
    /// another, and is read whole.
    seen: u32,
    stamps: (Stamp, Stamp),
    moving: KnobSet,
    /// The knobs whose place `v` holds: a value read past the publisher
    /// glides from it. A new Sound clears it.
    known: KnobSet,
    /// The route columns are read (`routes` holds them); until then the
    /// stored amounts play.
    routes_read: bool,
    routes_moving: bool,
    /// Each of `KNOBS` as its 0..1 place, and where it goes.
    v: [f32; KNOB_COUNT],
    t: [f32; KNOB_COUNT],
    /// The FREQ and Q columns' amounts by source, as `amount / 127`.
    routes: [[f32; MAX_MOD_SOURCES]; 2],
    /// Moves taken and knobs read past the publisher, wrapping: for the
    /// tests.
    reads: u32,
    /// The sample rate and the KNOB and STAGE coefficients at it: worked
    /// out once, not an `exp2` a knob a block.
    k: (u32, [f32; 2]),
    /// The pitch bend now, semitones: the wheel's reach, glided (not a
    /// knob of the Sound).
    bend: f32,
}

impl Knobs {
    /// What a sounding Part's knobs cost each block, cycles a sample,
    /// billed once a Part on one of its voices (ADR 0112, 0116): a move
    /// taken and two knobs gliding, ~325 cycles a block on the M7 (host
    /// 23 rdtsc, × 14.3, the unit's measured ratio for this path at
    /// bbd63cd7): 5.1, rounded up.
    pub const COST: Cost = Cost(6);

    /// Every Part's settled knobs each block, reserved once beside the bus
    /// (ADR 0116): six Parts' three compares and the loop, ~860 cycles a
    /// block on the M7 (host 60 rdtsc × 14.3): 13.4, rounded up. With
    /// `COST`, the 20 cycles a sample ADR 0040's costliest six leave beside
    /// the master tape. A bulk write (six knobs on every Part in a block,
    /// up to ~7k) runs in the headroom, as a setting's ease does (ADR
    /// 0061); more than `MOVES` lands.
    pub const RESERVE: Cost = Cost(14);

    pub const UNPRIMED: Self = Self {
        busy: true,
        seen: 0,
        stamps: (Stamp::NONE, Stamp::NONE),
        moving: KnobSet::EMPTY,
        known: KnobSet::EMPTY,
        routes_read: false,
        routes_moving: false,
        v: [0.0; KNOB_COUNT],
        t: [0.0; KNOB_COUNT],
        routes: [[0.0; MAX_MOD_SOURCES]; 2],
        reads: 0,
        k: (0, [0.0; 2]),
        bend: 0.0,
    };

    /// The pitch bend now, in semitones, added to every voice's pitch.
    #[inline]
    pub fn bend(&self) -> f32 {
        self.bend
    }

    /// One block of the bend toward `to` semitones at the KNOB coefficient
    /// `k`, or landed with none; within `BEND_LAND` it lands.
    pub fn ease_bend(&mut self, to: f32, k: Option<f32>) {
        match k {
            Some(k) => {
                glide(&mut self.bend, to, k, BEND_LAND);
            }
            None => self.bend = to,
        }
    }

    /// A new Sound (its Part's `loads` moved): every knob lands on its
    /// stored value, at once.
    pub fn land(&mut self) {
        (self.moving, self.known) = (KnobSet::EMPTY, KnobSet::EMPTY);
        (self.routes_read, self.busy) = (false, true);
    }

    /// Nothing moves.
    pub fn settled(&self) -> bool {
        self.moving.is_empty() && !self.routes_moving
    }

    /// The per-voice paths the gliding knobs put on their moving loops, for
    /// the bill: a glide runs them every block until it lands, where a
    /// step ran them once.
    #[inline]
    pub fn gliding(&self) -> Gliding {
        if self.moving.is_empty() {
            return Gliding::default();
        }
        self.gliding_of_moving()
    }

    #[inline(never)]
    fn gliding_of_moving(&self) -> Gliding {
        use crate::params::FilterParams;
        let mut g = Gliding::default();
        for i in self.moving.iter() {
            let (b, p) = (KNOBS[i].addr.block, KNOBS[i].addr.param);
            g.filter |= b == BlockRef::Filter
                && (p == FilterParams::CUTOFF || p == FilterParams::RESONANCE);
            g.fx |= b == BlockRef::VoiceFx;
        }
        g
    }

    /// Moves taken and knobs read so far, wrapping: for the tests.
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// Settled, with nothing new from `d` or written past it: a block of
    /// it does nothing.
    pub fn quiet(
        &self,
        (p, m): (&Tracked<ParamSnapshot>, &Tracked<ModState>),
        d: &KnobDelta,
    ) -> bool {
        !self.busy && d.count == self.seen && (p.stamp(), m.stamp()) == self.stamps
    }

    /// One block: `d`'s moves since the last taken, each knob's target set
    /// and (moving from where it was) glided toward, or landed while the
    /// Part is silent (`pace` `None`); then what moves glides on.
    pub fn ease(
        &mut self,
        (p, m): (&Tracked<ParamSnapshot>, &Tracked<ModState>),
        d: &KnobDelta,
        pace: Option<Pace>,
    ) {
        let stamps = (p.stamp(), m.stamp());
        if !self.busy && d.count == self.seen && stamps == self.stamps {
            return;
        }
        if let Some(Pace::Glide(sr)) = pace
            && self.k.0 != sr
        {
            self.k = (sr, [coeff(GlideMs::KNOB, sr), coeff(GlideMs::STAGE, sr)]);
        }
        let k = matches!(pace, Some(Pace::Glide(_))).then_some(self.k.1);
        let mut routes = !self.routes_read;
        if stamps == (d.of, d.of) {
            match d.since(self.seen) {
                Some(moves) => {
                    for mv in moves {
                        routes |= mv.knob == ROUTES;
                        self.take(mv, k);
                    }
                }
                // More than the window: every knob lands.
                None => (self.moving, routes) = (KnobSet::EMPTY, true),
            }
        } else if stamps != self.stamps {
            // Written past the publisher (a test's, the bench's; never the
            // shells'): it names no knob, so each is read.
            for i in 0..KNOB_COUNT {
                self.reread(i, p, k);
            }
            routes = true;
        }
        (self.seen, self.stamps) = (d.count, stamps);
        for i in self.moving.iter() {
            let t = self.t[i];
            let k = k.filter(|_| !p.moves_itself(KNOBS[i].addr));
            self.moving.put(i, step(&mut self.v[i], t, k, KNOBS[i].tau));
        }
        if routes || self.routes_moving {
            let k = k.filter(|_| self.routes_read).map_or(0.0, |k| k[0]);
            self.routes_moving = false;
            for (v, col) in self.routes.iter_mut().zip(COLS) {
                self.routes_moving |= glide_column(v, m, col, k);
            }
            self.routes_read = true;
        }
        self.busy = !self.moving.is_empty() || self.routes_moving;
    }

    /// Move `mv` taken: its knob's target set; it glides from where it
    /// was (still moving) or from `from`, or lands with no `k` or when
    /// `from == to`.
    fn take(&mut self, mv: KnobMove, k: Option<[f32; 2]>) {
        let i = usize::from(mv.knob);
        if i >= KNOB_COUNT {
            return;
        }
        let (from, to) = (mv.from, mv.to);
        if k.is_none() || from.to_bits() == to.to_bits() {
            (self.v[i], self.t[i]) = (to, to);
            self.moving.put(i, false);
        } else {
            if !self.moving.has(i) {
                self.v[i] = from;
            }
            self.t[i] = to;
            self.moving.put(i, true);
        }
        self.known.put(i, true);
        self.reads = self.reads.wrapping_add(1);
    }

    /// Knob `i` read from `p`, written past the publisher: it glides from
    /// where it was to there, if its place is known and it differs, else
    /// lands.
    #[inline(never)]
    fn reread(&mut self, i: usize, p: &ParamSnapshot, k: Option<[f32; 2]>) {
        let t = KNOBS[i].read(p);
        self.t[i] = t;
        let glides = k.is_some()
            && self.known.has(i)
            && (self.moving.has(i) || self.v[i].to_bits() != t.to_bits());
        if !glides {
            self.v[i] = t;
        }
        self.moving.put(i, glides);
        self.known.put(i, true);
        self.reads = self.reads.wrapping_add(1);
    }

    /// The knobs still on their way written over `p`'s stored values, or
    /// `live`'s for ALGO's (its bytes can't hold a value between steps).
    /// The rest are where they landed: stored, bit for bit.
    pub fn apply(&self, p: &mut ParamSnapshot, live: &mut AlgoLive) {
        for i in self.moving.iter() {
            let (a, v) = (KNOBS[i].addr, KNOBS[i].spec.denormalize(self.v[i]));
            if !live.set(a, v)
                && let Some(mut b) = p.block_mut(a.block)
            {
                b.write(a.param, v);
            }
        }
    }

    /// Fixed column `c`'s offset from the sources `v`: FREQ and Q by the
    /// eased amounts, VCA and FOLD (and all, unread) by `m`'s.
    #[inline(never)]
    pub fn fixed_sum(&self, m: &ModState, c: FixedCol, v: &[f32; MAX_MOD_SOURCES]) -> f32 {
        match COLS
            .iter()
            .position(|&x| x == c)
            .filter(|_| self.routes_read)
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
    /// way: for the tests. A knob not moving plays its stored value; its
    /// place is the last it was told.
    pub fn place(&self, addr: ParamAddr) -> Option<(f32, bool)> {
        let i = KNOBS.iter().position(|k| k.addr == addr)?;
        Some((self.v[i], self.moving.has(i)))
    }
}

/// `v` one block toward `t` at `k` (the KNOB and STAGE coefficients, by
/// `tau`), or landed with none; whether it still moves.
fn step(v: &mut f32, t: f32, k: Option<[f32; 2]>, tau: GlideMs) -> bool {
    match k {
        None => {
            *v = t;
            false
        }
        Some(k) => !glide(v, t, k[tau_ix(tau)], LAND),
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
    use crate::addr::BlockRead;
    use crate::block::Block;
    use crate::params::{EngineType, FilterParams};

    fn cutoff() -> ParamAddr {
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)
    }

    fn index(a: ParamAddr) -> usize {
        KNOBS.iter().position(|k| k.addr == a).unwrap()
    }

    const GLIDE: Option<Pace> = Some(Pace::Glide(48_000));

    /// One playing block of `p` and `m` written in place (no publisher):
    /// read whole when their stamps move.
    fn ease(e: &mut Knobs, p: &Tracked<ParamSnapshot>, m: &Tracked<ModState>) {
        e.ease((p, m), &KnobDelta::default(), GLIDE);
    }

    fn rig() -> (Tracked<ParamSnapshot>, Tracked<ModState>, Knobs) {
        let p = Tracked::fresh(ParamSnapshot::default());
        let m = Tracked::fresh(ModState::new());
        let mut k = Knobs::UNPRIMED;
        ease(&mut k, &p, &m);
        (p, m, k)
    }

    /// `s` published: the audio's copy, stamped, and its delta.
    fn publish(
        pb: &mut KnobPublisher,
        s: &Tracked<Sound>,
        l: Loads,
    ) -> (Tracked<ParamSnapshot>, Tracked<ModState>, KnobDelta) {
        let d = pb.publish(0, s, l);
        let p = Tracked::new(s.params.clone(), d.of);
        (p, Tracked::new(s.mod_state.clone(), d.of), d)
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

    /// Written past the publisher, a cutoff jump glides in octaves and
    /// lands exactly.
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

    /// The publisher sends a move for what changed, from where it was to
    /// where it goes, and nothing else; an unwritten Sound, nothing; a new
    /// one, nothing (it lands).
    #[test]
    fn a_delta_moves_only_what_changed() {
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Algo));
        s.params.filter.cutoff = 250.0;
        let first = pb.publish(0, &s, Loads::NONE);
        assert_eq!(
            first.since(0).map(|m| m.count()),
            Some(0),
            "a first publish lands"
        );
        assert_eq!(pb.publish(0, &s, Loads::NONE), first, "unwritten");
        s.params.filter.cutoff = 1000.0;
        let d = pb.publish(0, &s, Loads::NONE);
        let k = &KNOBS[index(cutoff())];
        let want = KnobMove {
            knob: index(cutoff()) as u8,
            from: k.place(250.0),
            to: k.place(1000.0),
        };
        assert_eq!(
            d.since(first.count)
                .unwrap()
                .collect::<alloc::vec::Vec<_>>(),
            [want]
        );
        let _ = &mut *s;
        assert_eq!(
            pb.publish(0, &s, Loads::NONE).count,
            d.count,
            "written, unchanged"
        );
        s.mod_state.set_fixed(0, FixedCol::Q, 64);
        let r = pb.publish(0, &s, Loads::NONE);
        assert_eq!(
            r.since(d.count)
                .unwrap()
                .map(|m| m.knob)
                .collect::<alloc::vec::Vec<_>>(),
            [ROUTES]
        );
        s.params.filter.cutoff = 500.0;
        let l = pb.publish(0, &s, Loads::NONE.next());
        assert_eq!(l.count, r.count, "a new Sound: no moves");
    }

    /// The window holds `MOVES`: that many back, every one; one more, none
    /// (every knob lands). A publish of more changes than that lands too.
    #[test]
    fn the_window_holds_six_moves() {
        let mut d = KnobDelta::default();
        for i in 0..MOVES {
            d.push(KnobMove {
                knob: i as u8,
                from: 0.0,
                to: 1.0,
            });
        }
        let back = d.count.wrapping_sub(MOVES as u32);
        assert_eq!(
            d.since(back)
                .map(|m| m.map(|m| m.knob).collect::<alloc::vec::Vec<_>>()),
            Some((0..MOVES as u8).collect())
        );
        assert!(d.since(back.wrapping_sub(1)).is_none());
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Modal));
        let a = pb.publish(0, &s, Loads::NONE);
        let m = &mut s.params.modal;
        for (i, v) in [
            &mut m.structure,
            &mut m.bright,
            &mut m.damp,
            &mut m.pos,
            &mut m.force,
            &mut m.speed,
        ]
        .into_iter()
        .enumerate()
        {
            *v = 0.03 + 0.11 * i as f32;
        }
        let f = &mut s.params.filter;
        (f.cutoff, f.resonance, f.level) = (317.0, 0.37, 0.41);
        let b = pb.publish(0, &s, Loads::NONE);
        assert!(b.since(a.count).is_none(), "nine changes at once: land");
        assert_eq!(core::mem::size_of::<KnobDelta>(), 80);
    }

    /// The audio takes a move: the knob glides from `from` toward `to`,
    /// reading nothing.
    #[test]
    fn the_audio_takes_only_what_a_delta_moves() {
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Algo));
        s.params.filter.cutoff = 250.0;
        let mut e = Knobs::UNPRIMED;
        let (p, m, d) = publish(&mut pb, &s, Loads::NONE);
        e.ease((&p, &m), &d, GLIDE);
        assert_eq!(e.reads(), 0, "a first publish lands, reading nothing");
        assert_eq!(played(&e, &p), 250.0);
        s.params.filter.cutoff = 1000.0;
        let (p, m, d) = publish(&mut pb, &s, Loads::NONE);
        e.ease((&p, &m), &d, GLIDE);
        assert_eq!(e.reads(), 1, "one move");
        let c = played(&e, &p);
        assert!(c > 250.0 && c < 1000.0, "glides from where it was: {c}");
        for _ in 0..400 {
            e.ease((&p, &m), &d, GLIDE);
        }
        assert_eq!(
            (e.reads(), played(&e, &p)),
            (1, 1000.0),
            "lands, reading nothing more"
        );
    }

    /// A TAPE model turned to SCRUB or away changes how POSITION moves
    /// with no knob changing: a move that lands it.
    #[test]
    fn a_model_that_moves_a_knob_itself_moves_it() {
        use crate::dsp::tape_engine::{TapeEngineParams, TapeModel};
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Tape));
        let d = pb.publish(0, &s, Loads::NONE);
        s.params.tape.model = TapeModel::Scrub;
        let e = pb.publish(0, &s, Loads::NONE);
        let pos = index(ParamAddr::new(
            BlockRef::TapeEngine,
            TapeEngineParams::POSITION,
        ));
        let moves: alloc::vec::Vec<_> = e.since(d.count).unwrap().collect();
        assert_eq!(moves.len(), 1);
        assert_eq!(
            (usize::from(moves[0].knob), moves[0].from == moves[0].to),
            (pos, true)
        );
    }

    /// Each knob's read plan reads what its block's `get` does, placed as
    /// its spec's `normalize` places it (within a float's rounding): every
    /// knob, on every engine's INIT and every factory Sound, and with each
    /// knob set to its range's ends.
    #[test]
    fn the_read_plan_reads_what_get_does() {
        use crate::factory::{FACTORY_LEN, factory_sound};
        let mut sounds: alloc::vec::Vec<ParamSnapshot> =
            [EngineType::Algo, EngineType::Modal, EngineType::Tape]
                .map(|e| Sound::init(e).params)
                .into();
        sounds.extend((0..FACTORY_LEN).filter_map(|i| factory_sound(i).map(|s| s.params)));
        for mut p in sounds {
            for end in [None, Some(0.0), Some(1.0)] {
                for k in &KNOBS {
                    if let Some(x) = end {
                        let v = k.spec.quantize(k.spec.denormalize(x));
                        p.block_mut(k.addr.block).unwrap().set(k.addr.param, v);
                    }
                    let want = k
                        .spec
                        .normalize(p.block(k.addr.block).unwrap().get(k.addr.param));
                    let got = k.read(&p);
                    assert!(
                        (got - want).abs() <= 1e-6,
                        "{:?} {}: {got} vs {want}",
                        k.addr.block,
                        k.spec.label
                    );
                }
            }
        }
    }
}
