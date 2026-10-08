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
        let base = core::ptr::from_ref(p).cast::<u8>();
        // SAFETY: `at` is `offset_of!` a field of `kind`'s type in
        // `ParamSnapshot` (`plan`), so the read is in bounds, aligned for
        // that type (the field's own offset) and of an initialised value.
        let v = unsafe {
            let f = base.add(self.at as usize);
            match self.kind {
                Kind::F32 => f.cast::<f32>().read(),
                Kind::U8 => f32::from(f.read()),
                Kind::I8 => f32::from(f.cast::<i8>().read()),
            }
        };
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
    use crate::params::{
        DriveParams, EnvParams, FilterParams, FolderParams, OutParams, PitchParams,
    };
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
            } else {
                panic!("an ALGO op knob without a field")
            };
            let op = offset_of!(P, algo.ops) + op.index() * size_of::<AlgoOpParams>();
            (op + f, kind)
        }
        BlockRef::Drive => {
            let f = if id == DriveParams::DRIVE.0 {
                offset_of!(DriveParams, drive)
            } else if id == DriveParams::TONE.0 {
                offset_of!(DriveParams, tone)
            } else if id == DriveParams::MIX.0 {
                offset_of!(DriveParams, mix)
            } else {
                panic!("a DRIVE knob without a field")
            };
            (offset_of!(P, drive) + f, Kind::F32)
        }
        BlockRef::Filter => {
            let f = if id == FilterParams::CUTOFF.0 {
                offset_of!(FilterParams, cutoff)
            } else if id == FilterParams::RESONANCE.0 {
                offset_of!(FilterParams, resonance)
            } else if id == FilterParams::LEVEL.0 {
                offset_of!(FilterParams, level)
            } else {
                panic!("a filter knob without a field")
            };
            (offset_of!(P, filter) + f, Kind::F32)
        }
        BlockRef::Folder => {
            let f = if id == FolderParams::FOLD.0 {
                offset_of!(FolderParams, fold)
            } else if id == FolderParams::SYMMETRY.0 {
                offset_of!(FolderParams, symmetry)
            } else if id == FolderParams::MIX.0 {
                offset_of!(FolderParams, mix)
            } else {
                panic!("a FOLD knob without a field")
            };
            (offset_of!(P, folder) + f, Kind::F32)
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

    /// Its routes, always (sixteen amounts, cheap, and a column read
    /// late would play its stored amounts at once), and `n` of its knobs
    /// from `from` on, round: so a Sound read in parts reads every knob,
    /// none twice before the rest. The set, and where to go on from.
    fn take(self, n: u32, from: u32) -> (Self, u32) {
        let mut out = self.0 & ROUTES;
        let knobs = self.0 & !ROUTES;
        let mask = (1u128 << from.min(127)) - 1;
        let (mut bits, mut next) = (knobs & !mask, from);
        for _ in 0..n {
            if bits == 0 {
                bits = knobs & mask & !out;
            }
            if bits == 0 {
                break;
            }
            let b = bits.isolate_lowest_one();
            (out, bits, next) = (out | b, bits & !b, b.trailing_zeros() + 1);
        }
        (Self(out), next % KNOB_COUNT as u32)
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
    /// The knobs its Sound moves itself (SCRUB's POSITION): the one input
    /// to a knob's motion that isn't a knob, so a change names them.
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
        let mut own = KnobSet::EMPTY;
        for (j, k) in KNOBS.iter().enumerate() {
            let p = &sound.params;
            let b = p
                .block(k.addr.block)
                .map_or(0, |b| b.get(k.addr.param).to_bits());
            if b != s.v[j] {
                (s.v[j], d) = (b, d.union(KnobSet(1 << j)));
            }
            own.put(j, p.moves_itself(k.addr));
        }
        d = d.union(KnobSet(own.0 ^ s.own.0));
        s.own = own;
        let m = &sound.mod_state;
        for (was, col) in s.routes.iter_mut().zip(COLS) {
            for (src, was) in was.iter_mut().enumerate() {
                let a = if src < m.num_sources() {
                    m.fixed_amount(src, col)
                } else {
                    0
                };
                if a != *was {
                    (*was, d) = (a, d.union(KnobSet(ROUTES)));
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
/// a voice rendered on its own plays the stored values. The fields a
/// settled block reads come first, so it touches one cache line.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Knobs {
    /// Something waits, moves or lands: a settled Part has none, and a
    /// block of it is three compares (`ease`).
    busy: bool,
    /// The last publish taken, and the stamps of what it described: a
    /// value written past the publisher (a test's, the bench's) has
    /// another, and is read whole.
    seq: u32,
    seen: (Stamp, Stamp),
    /// Each of `KNOBS` as its 0..1 place (`ParamSpec::normalize`): near 1,
    /// not ~10 as `log2` Hz, so an f32 step never stalls short of `LAND`.
    v: [f32; KNOB_COUNT],
    moving: KnobSet,
    /// Changed, not yet read: a silent Part's, past its block's budget.
    pending: KnobSet,
    /// Of `pending`, those that land, not glide, when read: a new
    /// Sound's, and what a silent Part left waiting.
    lands: KnobSet,
    /// The FREQ and Q columns' amounts by source, as `amount / 127`.
    routes: [[f32; MAX_MOD_SOURCES]; 2],
    routes_moving: bool,
    /// Knobs read, wrapping: for the tests.
    reads: u32,
    /// Where the next read of a part-read Sound goes on from.
    cursor: u32,
    /// The sample rate and the KNOB and STAGE coefficients at it: worked
    /// out once, not an `exp2` a knob a block.
    k: (u32, [f32; 2]),
}

impl Knobs {
    /// What a sounding Part's knobs cost each block, cycles a sample,
    /// billed once a Part on one of its voices (ADR 0112, 0115). Estimated
    /// from the thumb build's instructions, not benched: the delta's check
    /// (~40 cycles) and two knobs gliding (~300): 340 / 64 = 5.3. Reading
    /// what changed is `READS`'s.
    pub const COST: Cost = Cost(6);

    /// Knobs read in a block, every Part's together (ADR 0115): a knob
    /// turn reads one, before any landing; a new Sound's 66 land over nine
    /// blocks (~12 ms), playing their stored values meanwhile. Eight, so
    /// the reserve keeps ADR 0040's voices on rev V in every build: the
    /// costliest patch's six beside the master tape leave 20 cycles a
    /// sample, which `COST` and `READS` share (18).
    pub const READ_BUDGET: u32 = 8;

    /// `READ_BUDGET`'s reads, reserved once beside the bus: ~91 cycles a
    /// knob, a whole Sound's read on the unit (~6,000, the STRESS overrun
    /// analysis) / 66, × 8 = 728 / 64 = 11.4.
    pub const READS: Cost = Cost(12);

    pub const UNPRIMED: Self = Self {
        busy: true,
        v: [0.0; KNOB_COUNT],
        moving: KnobSet::EMPTY,
        pending: KnobSet::ALL,
        lands: KnobSet::ALL,
        routes: [[0.0; MAX_MOD_SOURCES]; 2],
        routes_moving: false,
        seq: 0,
        seen: (Stamp::NONE, Stamp::NONE),
        reads: 0,
        cursor: 0,
        k: (0, [0.0; 2]),
    };

    /// A new Sound (its Part's `loads` moved): every knob read and landed.
    pub fn land(&mut self) {
        (self.pending, self.lands, self.busy) = (KnobSet::ALL, KnobSet::ALL, true);
    }

    /// Only landing: a new Sound read in parts, no edit waiting.
    pub fn landing(&self) -> bool {
        !self.pending.is_empty() && self.pending.minus(self.lands).is_empty()
    }

    /// No change waits to be read (a glide may still run).
    pub fn settled(&self) -> bool {
        self.pending.is_empty()
    }

    /// Knobs read so far, wrapping: for the tests.
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// One block: the knobs `d` says changed read, at most `budget` of
    /// them (taken from it; the rest wait), the rest left where they are,
    /// and those on their way glided toward `p`'s and `m`'s stored values.
    /// A silent Part (`pace` `None`), or one landing a new Sound, lands
    /// them instead. A knob that waits plays where it was (`apply`), so
    /// an edit glides a block late, never snaps; a new Sound's plays its
    /// stored value, as a load lands anyway.
    pub fn ease(
        &mut self,
        (p, m): (&Tracked<ParamSnapshot>, &Tracked<ModState>),
        d: &KnobDelta,
        pace: Option<Pace>,
        budget: &mut u32,
    ) {
        let stamps = (p.stamp(), m.stamp());
        // Settled, and nothing new: nothing to do.
        if !self.busy && d.seq == self.seq && stamps == self.seen {
            return;
        }
        if let Some(Pace::Glide(sr)) = pace
            && self.k.0 != sr
        {
            self.k = (sr, [coeff(GlideMs::KNOB, sr), coeff(GlideMs::STAGE, sr)]);
        }
        // A value written past the publisher (a test's, the bench's; never
        // the shells') is read whole at once, past the budget: it names no
        // knob, and spread out it would play stale ones.
        let past = stamps != (d.of, d.of) && stamps != self.seen;
        let new = if stamps == (d.of, d.of) {
            d.since(self.seq)
        } else if past {
            KnobSet::ALL
        } else {
            KnobSet::EMPTY
        };
        let mut whole = u32::MAX;
        let budget = if past { &mut whole } else { budget };
        (self.seq, self.seen) = (d.seq, stamps);
        let todo = self.pending.union(new);
        // No budget left, nothing moving: what changed waits, untouched.
        if *budget == 0 && self.moving.is_empty() && !self.routes_moving {
            self.pending = todo;
            if pace.is_none() {
                self.lands = self.lands.union(new);
            }
            self.busy = !todo.is_empty();
            return;
        }
        // Edits first, so a turn never waits on a landing; then what lands.
        let (edits, from) = todo.minus(self.lands).take(*budget, self.cursor);
        *budget = budget.saturating_sub(edits.len());
        let (lands, from) = todo.minus(edits).take(*budget, from);
        *budget = budget.saturating_sub(lands.len());
        let now = edits.union(lands);
        (self.pending, self.cursor) = (todo.minus(now), from);
        let k = matches!(pace, Some(Pace::Glide(_))).then_some(self.k.1);
        for i in now.iter() {
            let k = k.filter(|_| !self.lands.has(i));
            self.step(i, p, k, true);
        }
        for i in self.moving.minus(now).iter() {
            self.step(i, p, k, false);
        }
        if now.routes() || self.routes_moving {
            let k = k.filter(|_| !self.lands.routes()).map_or(0.0, |k| k[0]);
            self.routes_moving = false;
            for (v, col) in self.routes.iter_mut().zip(COLS) {
                self.routes_moving |= glide_column(v, m, col, k);
            }
        }
        self.reads = self.reads.wrapping_add(now.len());
        // What a silent Part leaves waiting lands when read, as it would
        // have, even once a voice plays it.
        self.lands = self.lands.minus(now);
        if pace.is_none() {
            self.lands = self.lands.union(self.pending);
        }
        self.busy = !(self.pending.is_empty() && self.moving.is_empty()) || self.routes_moving;
    }

    /// Settled, with nothing new from `d` or written past it: a block of
    /// it does nothing.
    pub fn quiet(
        &self,
        (p, m): (&Tracked<ParamSnapshot>, &Tracked<ModState>),
        d: &KnobDelta,
    ) -> bool {
        !self.busy && d.seq == self.seq && (p.stamp(), m.stamp()) == self.seen
    }

    /// Knob `i` one block toward `p`'s at the coefficients `k`; landed at
    /// once with none. A `fresh` read starts it if it differs.
    #[inline(never)]
    fn step(&mut self, i: usize, p: &ParamSnapshot, k: Option<[f32; 2]>, fresh: bool) {
        let knob = &KNOBS[i];
        let k = k.filter(|_| !p.moves_itself(knob.addr));
        let t = knob.read(p);
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

    /// The knobs still on their way, and edits waiting on the read budget,
    /// written over `p`'s stored values, or `live`'s for ALGO's (its bytes
    /// can't hold a value between steps). The rest are where they landed:
    /// stored, bit for bit.
    pub fn apply(&self, p: &mut ParamSnapshot, live: &mut AlgoLive) {
        let waiting = self.pending.minus(self.lands);
        for i in self.moving.union(waiting).iter() {
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

    /// No read budget to speak of.
    fn plenty() -> u32 {
        u32::MAX
    }

    const GLIDE: Option<Pace> = Some(Pace::Glide(48_000));

    /// One playing block of `p` and `m` written in place (no publisher):
    /// read whole when their stamps move.
    fn ease(e: &mut Knobs, p: &Tracked<ParamSnapshot>, m: &Tracked<ModState>) {
        e.ease((p, m), &KnobDelta::default(), GLIDE, &mut plenty());
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
        e.ease((&p, &m), &d, GLIDE, &mut plenty());
        let base = e.reads();
        assert_eq!(base, KNOB_COUNT as u32);
        e.ease((&p, &m), &d, GLIDE, &mut plenty());
        assert_eq!(e.reads(), base, "settled");
        s.params.filter.cutoff = 500.0;
        let (p, m, d) = publish(&mut pb, &s);
        e.ease((&p, &m), &d, GLIDE, &mut plenty());
        assert_eq!(e.reads(), base + 1, "one knob");
        assert_eq!(e.place(cutoff()).map(|x| x.1), Some(true));
    }

    /// A silent Part reads within its budget, and lands it; a write past
    /// the publisher is read whole at once.
    #[test]
    fn a_silent_part_reads_within_its_budget() {
        let mut pb = KnobPublisher::new();
        let s = Tracked::fresh(Sound::init(EngineType::Algo));
        let d = pb.publish(0, &s, Loads::NONE);
        let p = Tracked::new(s.params.clone(), d.of);
        let m = Tracked::new(s.mod_state.clone(), d.of);
        let mut e = Knobs::UNPRIMED;
        let mut budget = 10;
        e.ease((&p, &m), &d, None, &mut budget);
        assert_eq!((e.reads(), budget), (10, 0));
        for _ in 0..KNOB_COUNT {
            e.ease((&p, &m), &d, None, &mut 10);
        }
        assert_eq!(e.reads(), KNOB_COUNT as u32);
        assert!(e.pending.is_empty() && e.lands.is_empty());
        let q = Tracked::fresh(ParamSnapshot::default());
        e.ease((&q, &m), &d, None, &mut 0);
        assert_eq!(
            e.reads(),
            2 * KNOB_COUNT as u32,
            "past the publisher, whole"
        );
    }

    /// The window is exact at its edge: four publishes back, the union of
    /// the four; five, every knob.
    #[test]
    fn the_window_holds_four_publishes() {
        let mut d = KnobDelta::default();
        for i in 0..DELTA_DEPTH {
            d.push(KnobSet(1 << i));
        }
        assert_eq!(d.since(d.seq.wrapping_sub(4)), KnobSet(0b1111));
        assert_eq!(d.since(d.seq.wrapping_sub(1)), KnobSet(1 << 3));
        assert_eq!(d.since(d.seq.wrapping_sub(5)), KnobSet::ALL);
        assert_eq!(d.since(d.seq), KnobSet::EMPTY);
    }

    /// An edit past the block's read budget plays where it was, then
    /// glides when read: never a snap to the new value.
    #[test]
    fn a_waiting_edit_holds_then_glides() {
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Algo));
        s.params.filter.cutoff = 250.0;
        let mut e = Knobs::UNPRIMED;
        let publish = |pb: &mut KnobPublisher, s: &Tracked<Sound>| {
            let d = pb.publish(0, s, Loads::NONE);
            let p = Tracked::new(s.params.clone(), d.of);
            (p, Tracked::new(s.mod_state.clone(), d.of), d)
        };
        let (p, m, d) = publish(&mut pb, &s);
        e.ease((&p, &m), &d, GLIDE, &mut plenty());
        s.params.filter.cutoff = 1000.0;
        let (p, m, d) = publish(&mut pb, &s);
        e.ease((&p, &m), &d, GLIDE, &mut 0);
        // Its place, back in Hz: 250 within a float's step.
        assert!(
            (played(&e, &p) / 250.0 - 1.0).abs() < 1e-6,
            "waits where it was"
        );
        e.ease((&p, &m), &d, GLIDE, &mut plenty());
        let c = played(&e, &p);
        assert!(c > 250.0 && c < 1000.0, "glides: {c}");
    }

    /// A TAPE model turned to SCRUB or away changes how POSITION moves
    /// with no knob changing: the delta names it.
    #[test]
    fn a_model_that_moves_a_knob_itself_names_it() {
        use crate::dsp::tape_engine::{TapeEngineParams, TapeModel};
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Tape));
        let d = pb.publish(0, &s, Loads::NONE);
        s.params.tape.model = TapeModel::Scrub;
        let e = pb.publish(0, &s, Loads::NONE);
        let pos = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::POSITION);
        let i = KNOBS.iter().position(|k| k.addr == pos).unwrap();
        assert_eq!(e.since(d.seq), KnobSet(1 << i));
    }

    /// A Sound read in parts reads every knob once, round from where the
    /// last part stopped, and the routes at once.
    #[test]
    fn a_sound_read_in_parts_reads_each_knob_once() {
        let (mut seen, mut from) = (KnobSet::EMPTY, 0);
        for i in 0..3 {
            let now;
            (now, from) = KnobSet::ALL.minus(seen).take(30, from);
            assert_eq!(now.routes(), i == 0);
            assert_eq!(now.minus(seen).len(), now.len(), "none twice");
            seen = seen.union(now);
        }
        assert_eq!(seen, KnobSet::ALL);
        // From the middle, round.
        let (a, next) = KnobSet::ALL.take(4, 64);
        assert_eq!(a, KnobSet(ROUTES | 0b11 << 64 | 0b11));
        assert_eq!(next, 2);
    }

    /// Landing a new Sound in parts, an edit to a knob it already read
    /// goes first, and glides.
    #[test]
    fn an_edit_goes_before_a_landing() {
        let mut pb = KnobPublisher::new();
        let mut s = Tracked::fresh(Sound::init(EngineType::Modal));
        let publish = |pb: &mut KnobPublisher, s: &Tracked<Sound>, l| {
            let d = pb.publish(0, s, l);
            let p = Tracked::new(s.params.clone(), d.of);
            (p, Tracked::new(s.mod_state.clone(), d.of), d)
        };
        let (p, m, d) = publish(&mut pb, &s, Loads::NONE);
        let mut e = Knobs::UNPRIMED;
        e.ease((&p, &m), &d, GLIDE, &mut plenty());
        e.land();
        e.ease((&p, &m), &d, GLIDE, &mut 8);
        assert_eq!(KNOBS[0].spec.label, "STRUCT");
        s.params.modal.structure = 0.9;
        let (p, m, d) = publish(&mut pb, &s, Loads::NONE);
        e.ease((&p, &m), &d, GLIDE, &mut 1);
        assert_eq!(
            e.place(KNOBS[0].addr).map(|x| x.1),
            Some(true),
            "read, gliding"
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
