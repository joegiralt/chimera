//! The reverb's settings (reverb TYPEs spec § Parameters): one block of
//! semantic slots, read per TYPE. `knobs` maps them to what the engine
//! plays; the engines and the panel's value text share it.

use core::f32::consts::LOG2_10;

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::algo::math::log2;
use crate::dsp::clampf;
use crate::dsp::modulator::law::Law;
use crate::dsp::rev::parts::{lg2, pow2};
use crate::dsp::rev::sync::{
    DECAY_BEATS, Division, FoldState, GATE_LENGTH, HALL_PRE, REVERSE_LENGTH, ROOM_PRE, Tempo, fold,
    index_of, p_of,
};
use crate::dsp::rev::{bloom, room};
use crate::dsp::ring::{self, RingControls, STAGES, size_step, stage_len};
use crate::ui::glyph::{CompositeId, FocusGlyph};

/// A reverb TYPE, in screen order (spec § The TYPE list). Its disk code
/// is `DiskCode`'s, never its place here. Zero bytes are QUAD.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum ReverbType {
    #[default]
    Quad,
    Room,
    Hall,
    Bloom,
    Gate,
    Reverse,
}

impl ReverbType {
    pub const ALL: [ReverbType; 6] = [
        ReverbType::Quad,
        ReverbType::Room,
        ReverbType::Hall,
        ReverbType::Bloom,
        ReverbType::Gate,
        ReverbType::Reverse,
    ];
    pub const NAMES: [&'static str; 6] = ["QUAD", "ROOM", "HALL", "BLOOM", "GATE", "REVERSE"];

    /// Its place on screen.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The TYPE at screen place `v`, clamped; NaN is QUAD.
    pub fn at(v: f32) -> Self {
        Self::ALL[clampf(v, 0.0, 5.0) as usize]
    }
}

/// Which lengths follow the tempo (spec § Tempo sync). Zero bytes are OFF.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum RevSync {
    #[default]
    Off,
    /// PRE-DLY or LENGTH in note values.
    Time,
    /// As TIME, and HALL's and BLOOM's DECAY in beats.
    All,
}

impl RevSync {
    pub const ALL: [RevSync; 3] = [RevSync::Off, RevSync::Time, RevSync::All];
    pub const NAMES: [&'static str; 3] = ["OFF", "TIME", "ALL"];

    pub fn at(v: f32) -> Self {
        Self::ALL[clampf(v, 0.0, 2.0) as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbParams {
    pub kind: ReverbType,
    /// How long it lasts: QUAD TIME, DECAY, TAIL, REGEN.
    pub time: f32,
    /// In-loop damping: QUAD's and HALL's DAMP.
    pub damping: f32,
    /// How big or long: SIZE, LENGTH, SWELL.
    pub size: f32,
    /// Return level.
    pub mix: f32,
    /// Input brightness (BLOOM: the in-loop damper).
    pub tone: f32,
    /// DIFFUSE, DENSITY.
    pub diffuse: f32,
    /// PRE-DLY.
    pub predelay: f32,
    /// The burst's curve: GATE SHAPE, REVERSE CURVE.
    pub shape: f32,
    /// The random wander's depth (§ SMEAR).
    pub smear: f32,
    /// The wander's speed.
    pub rate: f32,
    pub sync: RevSync,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            kind: ReverbType::Quad,
            time: 0.5,
            damping: 0.3,
            size: 0.5,
            mix: 0.0,
            tone: 0.3,
            diffuse: 0.4,
            predelay: 0.0,
            shape: 0.2,
            smear: 0.0,
            rate: 0.5,
            sync: RevSync::Off,
        }
    }
}

/// The slots a TYPE change writes, in `KnobMemory`'s order.
const KNOB_SLOTS: [ParamId; 9] = [
    ReverbParams::TIME,
    ReverbParams::DAMPING,
    ReverbParams::SIZE,
    ReverbParams::TONE,
    ReverbParams::DIFFUSE,
    ReverbParams::PREDELAY,
    ReverbParams::SHAPE,
    ReverbParams::SMEAR,
    ReverbParams::RATE,
];

/// Each TYPE's starting knobs (spec § Each TYPE's knobs), by `KNOB_SLOTS`,
/// in screen order. QUAD writes only TIME, DAMP, SIZE, SMEAR and RATE.
/// ROOM's are MidiVerb II program 5's (Medium Bright .6), BLOOM's program
/// 45's (Bloom 1), each fitted on the Rust render (ADR 0095), with SMEAR
/// 0.3.
const START: [[f32; 9]; 6] = [
    [0.5, 0.3, 0.5, 0.3, 0.4, 0.0, 0.2, 0.0, 0.5],
    [0.702, 0.346, 0.619, 0.0, 0.0, 0.077, 0.72, 0.3, 0.5],
    [0.45, 0.3, 0.65, 0.3, 0.4, 0.15, 0.2, 0.35, 0.4],
    [0.948, 0.3, 0.705, 0.578, 0.165, 0.132, 0.2, 0.3, 0.4],
    [0.3, 0.3, 0.55, 0.2, 0.7, 0.0, 0.2, 0.0, 0.5],
    [0.0, 0.3, 0.5, 0.0, 0.8, 0.0, 0.67, 0.0, 0.5],
];

/// Whether `t` writes `KNOB_SLOTS[i]` on a TYPE change.
fn writes(t: ReverbType, i: usize) -> bool {
    t != ReverbType::Quad || matches!(i, 0 | 1 | 2 | 7 | 8)
}

/// Each TYPE's last knobs, in RAM only (spec § A TYPE change loads that
/// TYPE's knobs): a slot is `round(p · 65 534) + 1`, and 0 in a TYPE's
/// first slot marks it not yet visited, so zero bytes are a clear memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnobMemory {
    slots: [[u16; 9]; 6],
}

const _: () = assert!(core::mem::size_of::<KnobMemory>() == 108);

const KNOB_ONE: f32 = 65_534.0;

impl KnobMemory {
    pub const fn new() -> Self {
        Self { slots: [[0; 9]; 6] }
    }

    /// Keep `p`'s knobs as its TYPE's.
    pub fn store(&mut self, p: &ReverbParams) {
        let slots = &mut self.slots[p.kind.index()];
        for (s, &id) in slots.iter_mut().zip(&KNOB_SLOTS) {
            *s = (unit(p.get(id)) * KNOB_ONE + 1.5) as u16;
        }
    }

    /// `t`'s knobs as last left; `None` before `t` was left.
    pub fn recall(&self, t: ReverbType) -> Option<[f32; 9]> {
        let s = &self.slots[t.index()];
        (s[0] != 0).then(|| s.map(|v| (v - 1) as f32 / KNOB_ONE))
    }

    /// Forget every TYPE: a project load.
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

impl Default for KnobMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// `p` in 0..=1; NaN reads 0.
fn unit(p: f32) -> f32 {
    clampf(p, 0.0, 1.0)
}

/// QUAD's ring runs at 24 kHz on the chip: its table's samples, in ms.
const RING_MS: f32 = 1000.0 / 24_000.0;

/// What the engine plays, from the knobs (spec § Each TYPE's knobs):
/// QUAD's laws, or the TYPE's `Ranges` row (HALL's through its plate
/// zone).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineKnobs {
    /// SIZE or LENGTH: the loop's round trip, or the burst, ms.
    pub length_ms: f32,
    /// TIME as RT60, s: QUAD's and TAIL's; REVERSE's from REGEN at its
    /// LENGTH (0 without regen).
    pub rt60_s: f32,
    /// The in-loop damper, Hz.
    pub damp_hz: f32,
    /// SMEAR's depth D, ms, as it plays.
    pub smear_ms: f32,
    pub rate_hz: f32,
    /// TONE: the input one-pole's corner, Hz.
    pub tone_hz: f32,
    /// GATE's TAIL gain scale, `min(1, p/0.05)`: 0 is the fast gate.
    pub tail: f32,
    /// REVERSE's feedback per pass, 0 → 0.708.
    pub regen: f32,
    /// DENSITY as taps, 6 → 12.
    pub taps: f32,
    /// DIFFUSE's diffuser and stage-allpass g; DENSITY's pre-diffuser g.
    pub diffuse_g: f32,
    /// PRE-DLY, ms.
    pub predelay_ms: f32,
    /// The burst's curve end to end, dB: GATE falls (≤ 0), REVERSE rises.
    pub shape_db: f32,
    /// The curve rises to the end (REVERSE).
    pub rising: bool,
}

/// How TIME maps on a row's TYPE.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeLaw {
    /// A LOOP TYPE's DECAY: RT60 `exp` over the range.
    Decay { rt60_s: Exp },
    /// GATE's TAIL: RT60 `exp` over the range, its gain faded in over
    /// the knob's first `fade` (TAIL 0 is exactly the fast gate).
    Tail { rt60_s: Exp, fade: f32 },
    /// REVERSE's REGEN: feedback `lin` 0 → `max`.
    Regen { max: f32 },
}

/// An `exp` law's range, `lo` and log2(`hi`/`lo`) worked out at compile
/// time: `lo·(hi/lo)^p`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exp {
    pub lo: f32,
    pub hi: f32,
    pub log2_ratio: f32,
}

impl Exp {
    pub const fn new(lo: f32, hi: f32) -> Self {
        Self {
            lo,
            hi,
            log2_ratio: log2(hi / lo),
        }
    }
}

/// A TYPE's knob ranges (spec § Each TYPE's knobs): `exp` laws for the
/// pairs read low → high, `lin` for the rest. One law function, `knobs`,
/// reads every row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ranges {
    /// LENGTH, ms, `exp`; `lin` over the same ends when `length_lin`.
    pub length_ms: Exp,
    pub length_lin: bool,
    pub time: TimeLaw,
    /// TONE's input corner, bright → dark, Hz, `exp`.
    pub tone_hz: Exp,
    /// DENSITY's taps, `lin`.
    pub taps: (f32, f32),
    /// DIFFUSE's or DENSITY's allpass g, `lin`.
    pub diffuse_g: (f32, f32),
    /// PRE-DLY, ms, `lin`.
    pub predelay_ms: (f32, f32),
    /// SHAPE or CURVE, dB, `lin`: negative falls. ROOM's LOOPS, θ in
    /// degrees.
    pub shape_db: (f32, f32),
    pub rising: bool,
    /// SMEAR's depth at 1, ms, `lin`, capped at `smear_cap` of the loop
    /// (§ The SMEAR bound; the decorrelators sit in none: ∞).
    pub smear_ms: f32,
    pub smear_cap: f32,
    /// The in-loop damper's corner, Hz, `exp` (0: none).
    pub damp_hz: Exp,
    /// RATE, Hz, `exp`.
    pub rate_hz: Exp,
}

pub const GATE: Ranges = Ranges {
    length_ms: Exp::new(40.0, 600.0),
    length_lin: false,
    time: TimeLaw::Tail {
        rt60_s: Exp::new(0.15, 0.8),
        fade: 0.05,
    },
    tone_hz: Exp::new(10_000.0, 2_500.0),
    taps: (6.0, 12.0),
    diffuse_g: (0.5, 0.7),
    predelay_ms: (0.0, 0.0),
    shape_db: (0.0, -18.0),
    rising: false,
    smear_ms: 1.0,
    smear_cap: f32::INFINITY,
    damp_hz: Exp {
        lo: 0.0,
        hi: 0.0,
        log2_ratio: 0.0,
    },
    rate_hz: Exp::new(0.1, 2.0),
};

pub const REVERSE: Ranges = Ranges {
    length_ms: Exp::new(150.0, 800.0),
    time: TimeLaw::Regen { max: 0.708 },
    shape_db: (12.0, 30.0),
    rising: true,
    ..GATE
};

/// ROOM (ADR 0095 § ROOM v2): SIZE is loop A's round trip, `lin` in
/// SIZE's k 0.4 → 1; DIFFUSE the stage allpasses' g (the diffuser's is
/// fixed); DAMP the loops' one-poles, 10 → 1.5 kHz; LOOPS (SHAPE's slot)
/// the pairs' θ, 0–90°.
pub const ROOM: Ranges = Ranges {
    length_ms: Exp::new(
        room::A_AP_MS + room::K_MIN * room::A_DELAY_MS,
        room::A_AP_MS + room::A_DELAY_MS,
    ),
    length_lin: true,
    time: TimeLaw::Decay {
        rt60_s: Exp::new(0.2, 2.0),
    },
    tone_hz: Exp::new(10_000.0, 2_500.0),
    taps: (0.0, 0.0),
    diffuse_g: (0.45, 0.70),
    predelay_ms: (0.0, 50.0),
    shape_db: (0.0, 90.0),
    rising: false,
    smear_ms: 1.0,
    smear_cap: 0.0072,
    damp_hz: Exp::new(10_000.0, 1_500.0),
    rate_hz: Exp::new(0.1, 1.0),
};

/// BLOOM (ADR 0095 § BLOOM): SWELL is the cascade's length, `exp` over
/// × 0.5–1.2; DECAY its 1 kHz RT60; TONE both one-poles, 10 → 1.5 kHz;
/// DIFFUSE the cascade's g. It has no DAMP.
pub const BLOOM: Ranges = Ranges {
    length_ms: Exp::new(
        bloom::SWELL_LO * bloom::TOTAL_MS,
        bloom::SWELL_HI * bloom::TOTAL_MS,
    ),
    length_lin: false,
    time: TimeLaw::Decay {
        rt60_s: Exp::new(2.0, 12.0),
    },
    tone_hz: Exp::new(10_000.0, 1_500.0),
    diffuse_g: (0.45, 0.65),
    predelay_ms: (0.0, 125.0),
    shape_db: (0.0, 0.0),
    smear_ms: 2.0,
    smear_cap: 0.0024,
    damp_hz: GATE.damp_hz,
    rate_hz: Exp::new(0.05, 0.5),
    ..ROOM
};

/// HALL's loop is its one loop's round trip, down to the plate's 120 ms:
/// SIZE's bottom `PLATE_ZONE` is the plate zone (`plate_pull`). Its DAMP is
/// QUAD's law (`damp_hz`).
pub const HALL: Ranges = Ranges {
    length_ms: Exp::new(120.0, 600.0),
    length_lin: false,
    diffuse_g: (0.5, 0.75),
    shape_db: (0.0, 0.0),
    time: TimeLaw::Decay {
        rt60_s: Exp::new(0.8, 10.0),
    },
    predelay_ms: (0.0, 125.0),
    smear_ms: 2.0,
    smear_cap: 0.0036,
    damp_hz: Exp::new(11_000.0, 1_500.0),
    rate_hz: Exp::new(0.1, 1.5),
    ..ROOM
};

/// HALL's SIZE below this is the plate zone (spec § PLATE is a zone of
/// HALL).
pub const PLATE_ZONE: f32 = 0.15;

/// Whether `p` sits in HALL's plate zone: its SIZE cell reads `PLATE`.
pub fn in_plate_zone(p: &ReverbParams) -> bool {
    p.kind == ReverbType::Hall && unit(p.size) < PLATE_ZONE
}

/// The knobs HALL plays in its plate zone, w = 1 − SIZE/0.15 (1 at SIZE
/// 0): DIFFUSE pulled up by w, TONE and PRE-DLY down by w, toward the
/// plate's dense, bright, immediate corner. The stored knobs never change;
/// above the zone, and on every other TYPE, `p` itself.
pub fn plate_pull(p: &ReverbParams) -> ReverbParams {
    if !in_plate_zone(p) {
        return *p;
    }
    let w = 1.0 - unit(p.size) / PLATE_ZONE;
    let d = unit(p.diffuse);
    ReverbParams {
        diffuse: d + w * (1.0 - d),
        tone: unit(p.tone) * (1.0 - w),
        predelay: unit(p.predelay) * (1.0 - w),
        ..*p
    }
}

/// `t`'s row; `None` plays QUAD's laws.
pub fn ranges(t: ReverbType) -> Option<&'static Ranges> {
    match t {
        ReverbType::Room => Some(&ROOM),
        ReverbType::Hall => Some(&HALL),
        ReverbType::Gate => Some(&GATE),
        ReverbType::Reverse => Some(&REVERSE),
        ReverbType::Bloom => Some(&BLOOM),
        ReverbType::Quad => None,
    }
}

/// `lo·(hi/lo)^p`, `p` in 0..=1 (NaN reads 0).
#[inline(never)]
pub fn exp_law(e: Exp, p: f32) -> f32 {
    e.lo * pow2(unit(p) * e.log2_ratio)
}

/// `lo + (hi − lo)·p`, `p` in 0..=1 (NaN reads 0).
pub fn lin((lo, hi): (f32, f32), p: f32) -> f32 {
    lo + (hi - lo) * unit(p)
}

/// QUAD's and HALL's DAMP: `ring::damp_coef`'s cutoff.
pub fn damp_hz(p: f32) -> f32 {
    11_000.0 * libm::powf(1.5 / 11.0, unit(p))
}

/// `p`'s knobs as the engine plays them at `tempo`: its TYPE's row, its
/// synced times in note values (`fold` held across calls), else QUAD's
/// laws as `dsp::ring` runs them (ADR 0028), on the chip's 24 kHz ring.
pub fn knobs(p: &ReverbParams, tempo: Tempo, fold: &mut FoldState) -> EngineKnobs {
    let Some(r) = ranges(p.kind) else {
        return quad_knobs(p);
    };
    let q = plate_pull(p);
    let mut k = row_knobs(r, &q);
    let s = synced(p);
    match s.time {
        Some((id, list)) => {
            let ms = synced_ms(r, id, list, q.get(id), tempo, fold);
            if id == ReverbParams::SIZE {
                k.length_ms = ms;
                if k.regen > 0.0 {
                    k.rt60_s = regen_rt60(ms, k.regen);
                }
            } else {
                k.predelay_ms = ms;
            }
        }
        None => *fold = FoldState::default(),
    }
    if s.beats
        && let TimeLaw::Decay { rt60_s } = r.time
    {
        k.rt60_s = beats_s(rt60_s, q.time, tempo);
    }
    k
}

/// What `p.sync` puts in note values under `p.kind` (spec § Tempo sync):
/// the slot and its divisions, and whether DECAY is in beats. QUAD takes
/// none; ROOM reads ALL as TIME. BLOOM takes HALL's lists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Synced {
    pub time: Option<(ParamId, &'static [Division])>,
    pub beats: bool,
}

pub fn synced(p: &ReverbParams) -> Synced {
    use ReverbParams as P;
    let time = match p.kind {
        ReverbType::Room => (P::PREDELAY, ROOM_PRE),
        ReverbType::Hall | ReverbType::Bloom => (P::PREDELAY, HALL_PRE),
        ReverbType::Gate => (P::SIZE, GATE_LENGTH),
        ReverbType::Reverse => (P::SIZE, REVERSE_LENGTH),
        ReverbType::Quad => {
            return Synced {
                time: None,
                beats: false,
            };
        }
    };
    Synced {
        time: (p.sync != RevSync::Off).then_some(time),
        beats: p.sync == RevSync::All && matches!(p.kind, ReverbType::Hall | ReverbType::Bloom),
    }
}

/// Slot `id`'s ms range on row `r`: LENGTH's or PRE-DLY's.
fn ms_range(r: &Ranges, id: ParamId) -> (f32, f32) {
    if id == ReverbParams::SIZE {
        (r.length_ms.lo, exp_law(r.length_ms, 1.0))
    } else {
        r.predelay_ms
    }
}

/// Slot `id` at `v` as a division at `tempo`, folded into its range.
fn synced_ms(
    r: &Ranges,
    id: ParamId,
    list: &[Division],
    v: f32,
    tempo: Tempo,
    st: &mut FoldState,
) -> f32 {
    let (lo, hi) = ms_range(r, id);
    fold(list[index_of(v, list.len())].ms(tempo), lo, hi, st)
}

/// Slot `id` at `v` unsynced, ms.
fn free_ms(r: &Ranges, id: ParamId, v: f32) -> f32 {
    if id == ReverbParams::SIZE {
        exp_law(r.length_ms, v)
    } else {
        lin(r.predelay_ms, v)
    }
}

/// The stored value whose unsynced ms is `ms`.
fn free_p(r: &Ranges, id: ParamId, ms: f32) -> f32 {
    if id == ReverbParams::SIZE {
        exp_inv(r.length_ms, ms)
    } else {
        let (lo, hi) = r.predelay_ms;
        unit((ms - lo) / (hi - lo))
    }
}

/// `exp_law`'s inverse: the p at which `e` reads `x`.
fn exp_inv(e: Exp, x: f32) -> f32 {
    unit(lg2(x / e.lo) / e.log2_ratio)
}

/// DECAY at `v` in beats at `tempo`, s, clamped to the row's seconds (a
/// decay has no grid to stay on).
fn beats_s(e: Exp, v: f32, tempo: Tempo) -> f32 {
    let b = DECAY_BEATS[index_of(v, DECAY_BEATS.len())] as f32;
    clampf(b * tempo.beat_ms() * 1e-3, e.lo, exp_law(e, 1.0))
}

/// The entry of `n` whose value `at(i)` is nearest `x`; the first on a tie.
fn nearest(n: usize, x: f32, at: impl Fn(usize) -> f32) -> usize {
    (0..n)
        .min_by(|&a, &b| (at(a) - x).abs().total_cmp(&(at(b) - x).abs()))
        .unwrap_or(0)
}

/// −60 dB at 20·log10 `g` a pass of `length_ms`.
fn regen_rt60(length_ms: f32, g: f32) -> f32 {
    length_ms * 1e-3 * -3.0 * LOG2_10 / lg2(g)
}

fn quad_knobs(p: &ReverbParams) -> EngineKnobs {
    let c = p.controls().sanitised();
    let step = size_step(c.size);
    let trip: u32 = (0..STAGES).map(|k| stage_len(step, k) as u32).sum();
    let length_ms = trip as f32 * RING_MS;
    EngineKnobs {
        length_ms,
        rt60_s: ring::rt60(c.time, step, 24_000.0),
        damp_hz: damp_hz(c.damp),
        // `ring::smear_depth`'s law, on the trip already summed.
        smear_ms: unit(p.smear) * 0.0018 * length_ms,
        rate_hz: ring::smear_hz(p.rate),
        ..EngineKnobs::default()
    }
}

/// One row's laws.
fn row_knobs(r: &Ranges, p: &ReverbParams) -> EngineKnobs {
    let length_ms = if r.length_lin {
        lin((r.length_ms.lo, r.length_ms.hi), p.size)
    } else {
        exp_law(r.length_ms, p.size)
    };
    let (rt60_s, tail, regen) = match r.time {
        TimeLaw::Decay { rt60_s } => (exp_law(rt60_s, p.time), 0.0, 0.0),
        TimeLaw::Tail { rt60_s, fade } => (
            exp_law(rt60_s, p.time),
            clampf(unit(p.time) / fade, 0.0, 1.0),
            0.0,
        ),
        TimeLaw::Regen { max } => {
            let g = max * unit(p.time);
            // −60 dB at 20·log10 g a pass.
            let rt = if g > 0.0 {
                regen_rt60(length_ms, g)
            } else {
                0.0
            };
            (rt, 0.0, g)
        }
    };
    EngineKnobs {
        length_ms,
        rt60_s,
        smear_ms: (r.smear_ms * unit(p.smear)).min(r.smear_cap * length_ms),
        rate_hz: exp_law(r.rate_hz, p.rate),
        tone_hz: exp_law(r.tone_hz, p.tone),
        tail,
        regen,
        taps: lin(r.taps, p.diffuse),
        diffuse_g: lin(r.diffuse_g, p.diffuse),
        predelay_ms: lin(r.predelay_ms, p.predelay),
        shape_db: lin(r.shape_db, p.shape),
        rising: r.rising,
        damp_hz: exp_law(r.damp_hz, p.damping),
    }
}

impl ReverbParams {
    /// Off when the mix is below audibility: the return fades out, the ring
    /// runs on (ADR 0061).
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    // ParamId(0) was TYPE and ParamId(5) GRIT: retired, never reused
    // (ADR 0009, reverb TYPEs spec § GRIT is retired).
    pub const TIME: ParamId = ParamId(1);
    pub const DAMPING: ParamId = ParamId(2);
    pub const SIZE: ParamId = ParamId(3);
    pub const MIX: ParamId = ParamId(4);
    pub const TYPE: ParamId = ParamId(6);
    pub const TONE: ParamId = ParamId(7);
    pub const DIFFUSE: ParamId = ParamId(8);
    pub const PREDELAY: ParamId = ParamId(9);
    pub const SHAPE: ParamId = ParamId(10);
    pub const SMEAR: ParamId = ParamId(11);
    pub const RATE: ParamId = ParamId(12);
    pub const SYNC: ParamId = ParamId(13);

    /// What the ring reads.
    pub fn controls(&self) -> RingControls {
        RingControls {
            time: self.time,
            damp: self.damping,
            size: self.size,
        }
    }

    /// TYPE `t` with its knobs: `mem`'s if `t` was left before, else its
    /// starting ones. MIX and SYNC stay: the player's level and clock.
    pub fn with_type(self, t: ReverbType, mem: &KnobMemory) -> Self {
        let knobs = mem.recall(t).unwrap_or(START[t.index()]);
        let mut p = Self { kind: t, ..self };
        for (i, &id) in KNOB_SLOTS.iter().enumerate() {
            if writes(t, i) {
                p.write(id, knobs[i]);
            }
        }
        p
    }

    /// SYNC `s`, its slots converted so the sound stays close (spec §
    /// Toggling SYNC): into note values, the division nearest what plays
    /// at `tempo`; out of them, the value whose ms is the division that
    /// played. ROOM takes ALL as TIME. Decode and CC writes store SYNC
    /// alone.
    pub fn with_sync(self, s: RevSync, tempo: Tempo) -> Self {
        let s = if self.kind == ReverbType::Room && s == RevSync::All {
            RevSync::Time
        } else {
            s
        };
        let mut p = Self { sync: s, ..self };
        let Some(r) = ranges(self.kind) else {
            return p;
        };
        let (from, to) = (synced(&self), synced(&p));
        match (from.time, to.time) {
            (None, Some((id, list))) => {
                let ms = free_ms(r, id, self.get(id));
                let at = |i| synced_ms(r, id, list, p_of(i, list.len()), tempo, &mut FoldState(0));
                p.write(id, p_of(nearest(list.len(), ms, at), list.len()));
            }
            (Some((id, list)), None) => {
                let ms = synced_ms(r, id, list, self.get(id), tempo, &mut FoldState(0));
                p.write(id, free_p(r, id, ms));
            }
            _ => {}
        }
        if let TimeLaw::Decay { rt60_s } = r.time {
            let n = DECAY_BEATS.len();
            match (from.beats, to.beats) {
                (false, true) => {
                    let s = exp_law(rt60_s, self.time);
                    p.time = p_of(nearest(n, s, |i| beats_s(rt60_s, p_of(i, n), tempo)), n);
                }
                (true, false) => p.time = exp_inv(rt60_s, beats_s(rt60_s, self.time, tempo)),
                _ => {}
            }
        }
        p
    }
}

/// Every reverb param but the two word choices shows the reverb's
/// composite (ADR 0067, ADR 0085); TYPE and SYNC take NONE.
const REV: FocusGlyph = FocusGlyph::Composite(CompositeId::Reverb);

/// The reverb runs outside `Voice`: nothing is modulatable. Labels are the
/// static ones; each TYPE's panel names its slots (`ui::reverb_panel`).
pub static REVERB_SPECS: [ParamSpec; 12] = [
    ParamSpec::choice(6, "TYPE", ValFmt::Names(&ReverbType::NAMES), 5.0, 0.0)
        .ident("TYPE")
        .glyph(FocusGlyph::None),
    ParamSpec::continuous(1, "TIME", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false)
        .ident("TIME")
        .glyph(REV),
    ParamSpec::continuous(2, "DAMP", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false)
        .ident("DAMP")
        .glyph(REV),
    // GATE's LENGTH moves 2 % a tick; QUAD's panel ticks a table step.
    ParamSpec::continuous(3, "SIZE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false)
        .ident("SIZE")
        .glyph(REV),
    ParamSpec::continuous(
        4,
        "MIX",
        ValFmt::Law(Law::Pct),
        0.0,
        1.0,
        0.0,
        1.0 / 128.0,
        false,
    )
    .ident("MIX")
    .glyph(REV),
    ParamSpec::continuous(7, "TONE", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false)
        .ident("TONE")
        .glyph(REV),
    ParamSpec::continuous(8, "DIFFUSE", ValFmt::Uni, 0.0, 1.0, 0.4, 1.0 / 128.0, false)
        .ident("DIFFUSE")
        .short("DIFF")
        .glyph(REV),
    ParamSpec::continuous(9, "PRE-DLY", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false)
        .ident("PREDELAY")
        .short("PRE")
        .glyph(REV),
    ParamSpec::continuous(10, "SHAPE", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false)
        .ident("SHAPE")
        .glyph(REV),
    ParamSpec::continuous(11, "SMEAR", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false)
        .ident("SMEAR")
        .glyph(REV),
    ParamSpec::continuous(12, "RATE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false)
        .ident("RATE")
        .glyph(REV),
    ParamSpec::choice(13, "SYNC", ValFmt::Names(&RevSync::NAMES), 2.0, 0.0)
        .ident("SYNC")
        .glyph(FocusGlyph::None),
];

impl Block for ReverbParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &REVERB_SPECS
    }

    // Out of line: the trait's defaults each call them.
    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::TYPE => self.kind.index() as f32,
            Self::TIME => self.time,
            Self::DAMPING => self.damping,
            Self::SIZE => self.size,
            Self::MIX => self.mix,
            Self::TONE => self.tone,
            Self::DIFFUSE => self.diffuse,
            Self::PREDELAY => self.predelay,
            Self::SHAPE => self.shape,
            Self::SMEAR => self.smear,
            Self::RATE => self.rate,
            Self::SYNC => self.sync as u8 as f32,
            _ => 0.0,
        }
    }

    /// TYPE stores only the TYPE: a load or a CC keeps the stored knobs
    /// (`with_type` is the UI's).
    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::TYPE => self.kind = ReverbType::at(v),
            Self::TIME => self.time = v,
            Self::DAMPING => self.damping = v,
            Self::SIZE => self.size = v,
            Self::MIX => self.mix = v,
            Self::TONE => self.tone = v,
            Self::DIFFUSE => self.diffuse = v,
            Self::PREDELAY => self.predelay = v,
            Self::SHAPE => self.shape = v,
            Self::SMEAR => self.smear = v,
            Self::RATE => self.rate = v,
            Self::SYNC => self.sync = RevSync::at(v),
            _ => {}
        }
    }

    /// The default's body, out of line: `nudge` and `snap` would each
    /// inline it, and the twelve-way `write` with it.
    #[inline(never)]
    fn set(&mut self, id: ParamId, v: f32) {
        if let Some(s) = self.spec(id) {
            self.write(id, s.quantize(v));
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::TYPE => Some(self.kind.disk_code()),
            Self::SYNC => Some(self.sync.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::TYPE => Some(self.kind.disk_ident()),
            Self::SYNC => Some(self.sync.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::TYPE => apply_code(ReverbType::from_disk_code(code), |t| self.kind = t),
            Self::SYNC => apply_code(RevSync::from_disk_code(code), |s| self.sync = s),
            _ => false,
        }
    }
}
