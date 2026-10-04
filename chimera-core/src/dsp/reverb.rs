//! The reverb's settings (reverb TYPEs spec § Parameters): one block of
//! semantic slots, read per TYPE. Every knob maps to its quantity through
//! `knobs`, which the engines and the panel's value text share.

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::clampf;
use crate::dsp::rev::sync::{
    DECAY_BEATS, Division, FoldState, GATE_LENGTH, HALL_PRE, REVERSE_LENGTH, ROOM_PRE, Tempo, fold,
    list_index,
};
use crate::dsp::ring::{self, RingControls, STAGES, size_step, stage_len};
use crate::ui::glyph::{CompositeId, FocusGlyph};

/// A reverb TYPE, in screen order (spec § The TYPE list). Its disk code
/// is `DiskCode`'s, never its place here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

/// Which lengths follow the tempo (spec § Tempo sync).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

    /// As `t` reads it: QUAD takes none, and ROOM reads ALL as TIME.
    pub fn under(self, t: ReverbType) -> Self {
        match (t, self) {
            (ReverbType::Quad, _) => RevSync::Off,
            (ReverbType::Room, RevSync::All) => RevSync::Time,
            (_, s) => s,
        }
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
const START: [[f32; 9]; 6] = [
    [0.5, 0.3, 0.5, 0.3, 0.4, 0.0, 0.2, 0.0, 0.5],
    [0.5, 0.3, 0.5, 0.3, 0.4, 0.1, 0.2, 0.3, 0.5],
    [0.45, 0.3, 0.65, 0.3, 0.4, 0.15, 0.2, 0.35, 0.4],
    [0.6, 0.3, 0.7, 0.35, 0.5, 0.0, 0.2, 0.3, 0.4],
    [0.3, 0.3, 0.55, 0.2, 0.7, 0.0, 0.2, 0.0, 0.5],
    [0.0, 0.3, 0.5, 0.0, 0.8, 0.0, 0.67, 0.0, 0.5],
];

/// Whether `t` writes `KNOB_SLOTS[i]` on a TYPE change.
fn writes(t: ReverbType, i: usize) -> bool {
    t != ReverbType::Quad || matches!(i, 0 | 1 | 2 | 7 | 8)
}

/// Each TYPE's last knobs, in RAM only (spec § A TYPE change loads that
/// TYPE's knobs): a slot is `round(p · 65 534)`, and `u16::MAX` in a
/// TYPE's first slot marks it not yet visited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnobMemory {
    slots: [[u16; 9]; 6],
}

const _: () = assert!(core::mem::size_of::<KnobMemory>() == 108);

const UNVISITED: u16 = u16::MAX;
const KNOB_ONE: f32 = 65_534.0;

impl KnobMemory {
    pub const fn new() -> Self {
        Self {
            slots: [[UNVISITED; 9]; 6],
        }
    }

    /// Keep `p`'s knobs as its TYPE's.
    pub fn store(&mut self, p: &ReverbParams) {
        let slots = &mut self.slots[p.kind.index()];
        for (s, &id) in slots.iter_mut().zip(&KNOB_SLOTS) {
            *s = (unit(p.get(id)) * KNOB_ONE + 0.5) as u16;
        }
    }

    /// `t`'s knobs as last left; `None` before `t` was left.
    pub fn recall(&self, t: ReverbType) -> Option<[f32; 9]> {
        let s = &self.slots[t.index()];
        (s[0] != UNVISITED).then(|| s.map(|v| v as f32 / KNOB_ONE))
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

/// `exp(lo, hi, p) = lo·(hi/lo)^p`, p clamped (NaN reads 0). Out of
/// line: a dozen call sites share one body.
#[inline(never)]
fn exp_law(lo: f32, hi: f32, p: f32) -> f32 {
    lo * libm::powf(hi / lo, unit(p))
}

/// The p `exp_law` maps to `x`.
#[inline(never)]
fn exp_inv(lo: f32, hi: f32, x: f32) -> f32 {
    unit(crate::dsp::fast_log2(x / lo) / crate::dsp::fast_log2(hi / lo))
}

/// The p of the `n`-list entry whose `at(i)` is nearest `x`.
#[inline(never)]
fn nearest(n: usize, at: &dyn Fn(usize) -> f32, x: f32) -> f32 {
    let (mut best, mut d) = (0, f32::INFINITY);
    for i in 0..n {
        let e = (at(i) - x).abs();
        if e < d {
            (best, d) = (i, e);
        }
    }
    best as f32 / n.saturating_sub(1).max(1) as f32
}

fn lin(lo: f32, hi: f32, p: f32) -> f32 {
    lo + (hi - lo) * unit(p)
}

/// `p` in 0..=1; NaN reads 0.
fn unit(p: f32) -> f32 {
    clampf(p, 0.0, 1.0)
}

/// QUAD's ring runs at 24 kHz on the chip: its table's samples, in ms.
const RING_MS: f32 = 1000.0 / 24_000.0;

/// HALL's plate zone: SIZE 0 → 0.15 (spec § PLATE is a zone of HALL).
pub const PLATE_ZONE: f32 = 0.15;

/// A TYPE's ranges (spec § Each TYPE's knobs): lengths in ms, times in s,
/// frequencies in Hz. A pair is a law's (p 0, p 1) ends.
struct Ranges {
    /// SIZE, LENGTH, SWELL: exp.
    len: (f32, f32),
    /// DECAY, TAIL: exp.
    decay: (f32, f32),
    /// TONE: exp, bright to dark.
    tone: (f32, f32),
    /// DIFFUSE's g, DENSITY's pre-diffuser g: lin.
    diffuse: (f32, f32),
    /// PRE-DLY's top: lin from 0.
    pre: f32,
    /// SMEAR's top, ms: lin from 0.
    smear: f32,
    /// SMEAR's cap, a share of the round trip (∞: outside every loop).
    cap: f32,
    /// RATE: exp.
    rate: (f32, f32),
    /// SYNC's lists: PRE-DLY's, or LENGTH's.
    pre_list: &'static [Division],
    len_list: &'static [Division],
}

const ROOM: Ranges = Ranges {
    len: (100.0, 200.0),
    decay: (0.2, 1.5),
    tone: (10_000.0, 2_500.0),
    diffuse: (0.5, 0.75),
    pre: 50.0,
    smear: 1.0,
    cap: 0.0072,
    rate: (0.1, 1.0),
    pre_list: ROOM_PRE,
    len_list: &[],
};
const HALL: Ranges = Ranges {
    len: (120.0, 600.0),
    decay: (0.8, 10.0),
    smear: 2.0,
    cap: 0.0036,
    pre: 125.0,
    rate: (0.1, 1.5),
    pre_list: HALL_PRE,
    ..ROOM
};
const BLOOM: Ranges = Ranges {
    len: (250.0, 700.0),
    decay: (2.0, 12.0),
    tone: (10_000.0, 1_500.0),
    diffuse: (0.45, 0.65),
    cap: 0.0024,
    rate: (0.05, 0.5),
    ..HALL
};
const GATE: Ranges = Ranges {
    len: (40.0, 600.0),
    decay: (0.15, 0.8),
    diffuse: (0.5, 0.7),
    pre: 0.0,
    cap: f32::INFINITY,
    rate: (0.1, 2.0),
    pre_list: &[],
    len_list: GATE_LENGTH,
    ..ROOM
};
const REVERSE: Ranges = Ranges {
    len: (150.0, 800.0),
    len_list: REVERSE_LENGTH,
    ..GATE
};

fn ranges(t: ReverbType) -> Option<&'static Ranges> {
    Some(match t {
        ReverbType::Quad => return None,
        ReverbType::Room => &ROOM,
        ReverbType::Hall => &HALL,
        ReverbType::Bloom => &BLOOM,
        ReverbType::Gate => &GATE,
        ReverbType::Reverse => &REVERSE,
    })
}

/// REGEN's top: −3 dB a pass.
const REGEN_MAX: f32 = 0.708;
/// TAIL fades in over its first 5 % (spec § BURST engine).
const TAIL_FADE: f32 = 0.05;

/// What a TYPE's engine plays, from its knobs (spec § Each TYPE's knobs):
/// every quantity after its law, its cap, the plate zone and SYNC. A
/// field the TYPE lacks reads 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineKnobs {
    /// SIZE, LENGTH or SWELL: the loop's round trip, or the burst, ms.
    pub length_ms: f32,
    /// DECAY or TAIL as RT60, s. The engine floors it at its `t_min`.
    pub rt60_s: f32,
    /// TAIL's gain scale, `min(1, TAIL/0.05)` (GATE).
    pub tail: f32,
    /// REGEN's feedback gain, 0..0.708 (REVERSE).
    pub regen: f32,
    /// TONE's cutoff, Hz (BLOOM: the in-loop damper).
    pub tone_hz: f32,
    /// DIFFUSE's allpass g (GATE, REVERSE: the pre-diffuser's).
    pub diffuse_g: f32,
    /// DENSITY's taps, 6..12 (GATE, REVERSE).
    pub taps: f32,
    /// The in-loop damper, Hz (QUAD, HALL).
    pub damp_hz: f32,
    pub predelay_ms: f32,
    /// GATE's fall (≤ 0) or REVERSE's rise, dB.
    pub shape_db: f32,
    /// SMEAR's depth D, ms, as it plays: under its cap.
    pub smear_ms: f32,
    pub rate_hz: f32,
}

/// QUAD's and HALL's DAMP: `ring::damp_coef`'s cutoff.
pub fn damp_hz(p: f32) -> f32 {
    exp_law(11_000.0, 1_500.0, p)
}

/// The plate zone's pull at HALL `size`: 1 at 0, none from 0.15.
pub fn plate_pull(size: f32) -> f32 {
    clampf(1.0 - unit(size) / PLATE_ZONE, 0.0, 1.0)
}

/// `p`'s knobs as `p.kind`'s engine plays them at `tempo`. A synced time
/// folds by `fold` (one per TYPE: none syncs two folding lengths).
#[inline(never)]
pub fn knobs(p: &ReverbParams, tempo: Tempo, st: &mut FoldState) -> EngineKnobs {
    let Some(r) = ranges(p.kind) else {
        return quad_knobs(p);
    };
    let sync = p.sync.under(p.kind);
    let synced = |list: &[Division], v: f32| list[list_index(v, list.len())].ms(tempo);
    let length_ms = match sync {
        RevSync::Time | RevSync::All if !r.len_list.is_empty() => {
            fold(synced(r.len_list, p.size), r.len.0, r.len.1, st)
        }
        _ => exp_law(r.len.0, r.len.1, p.size),
    };
    let w = if p.kind == ReverbType::Hall {
        plate_pull(p.size)
    } else {
        0.0
    };
    let pre_ms = match sync {
        RevSync::Time | RevSync::All if !r.pre_list.is_empty() => {
            fold(synced(r.pre_list, p.predelay), 0.0, r.pre, st)
        }
        _ => lin(0.0, r.pre, p.predelay),
    };
    let rt60_s = match (sync, p.kind) {
        (RevSync::All, ReverbType::Hall | ReverbType::Bloom) => {
            let beats = DECAY_BEATS[list_index(p.time, DECAY_BEATS.len())];
            clampf(
                beats as f32 * tempo.beat_ms() / 1000.0,
                r.decay.0,
                r.decay.1,
            )
        }
        (_, ReverbType::Reverse) => 0.0,
        _ => exp_law(r.decay.0, r.decay.1, p.time),
    };
    let burst = matches!(p.kind, ReverbType::Gate | ReverbType::Reverse);
    let diffuse = unit(p.diffuse);
    EngineKnobs {
        length_ms,
        rt60_s,
        tail: match p.kind {
            ReverbType::Gate => (unit(p.time) / TAIL_FADE).min(1.0),
            _ => 0.0,
        },
        regen: match p.kind {
            ReverbType::Reverse => lin(0.0, REGEN_MAX, p.time),
            _ => 0.0,
        },
        tone_hz: exp_law(r.tone.0, r.tone.1, unit(p.tone) * (1.0 - w)),
        diffuse_g: lin(r.diffuse.0, r.diffuse.1, diffuse + w * (1.0 - diffuse)),
        taps: if burst {
            lin(6.0, 12.0, p.diffuse)
        } else {
            0.0
        },
        damp_hz: match p.kind {
            ReverbType::Hall => damp_hz(p.damping),
            _ => 0.0,
        },
        predelay_ms: pre_ms * (1.0 - w),
        shape_db: match p.kind {
            ReverbType::Gate => -lin(0.0, 18.0, p.shape),
            ReverbType::Reverse => lin(12.0, 30.0, p.shape),
            _ => 0.0,
        },
        smear_ms: lin(0.0, r.smear, p.smear).min(r.cap * length_ms),
        rate_hz: exp_law(r.rate.0, r.rate.1, p.rate),
    }
}

/// QUAD's laws, as `dsp::ring` runs them (ADR 0028).
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

/// The division a synced slot plays, if `id` follows the tempo under
/// `p`'s TYPE and SYNC, with its list.
pub fn synced_list(p: &ReverbParams, id: ParamId) -> Option<&'static [Division]> {
    let r = ranges(p.kind)?;
    match (p.sync.under(p.kind), id) {
        (RevSync::Off, _) => None,
        (_, ReverbParams::SIZE) if !r.len_list.is_empty() => Some(r.len_list),
        (_, ReverbParams::PREDELAY) if !r.pre_list.is_empty() => Some(r.pre_list),
        _ => None,
    }
}

/// DECAY walks beats: HALL and BLOOM under SYNC ALL.
pub fn decay_in_beats(p: &ReverbParams) -> bool {
    p.sync.under(p.kind) == RevSync::All && matches!(p.kind, ReverbType::Hall | ReverbType::Bloom)
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

    /// SYNC `s`, its slots converted so the sound stays close at `tempo`
    /// (spec § Toggling SYNC): into a division, the nearest to the time
    /// that played; out of one, the time it played.
    pub fn with_sync(self, s: RevSync, tempo: Tempo) -> Self {
        let Some(r) = ranges(self.kind) else {
            return self;
        };
        let to = Self { sync: s, ..self };
        let (from, into) = (self.sync.under(self.kind), to.sync.under(self.kind));
        if from == into {
            return to;
        }
        let was = knobs(&self, tempo, &mut FoldState::default());
        let mut p = to;
        let ms = |list: &'static [Division]| move |i: usize| list[i].ms(tempo);
        if !r.len_list.is_empty() {
            p.size = match into {
                RevSync::Off => exp_inv(r.len.0, r.len.1, was.length_ms),
                _ => nearest(r.len_list.len(), &ms(r.len_list), was.length_ms),
            };
        }
        if !r.pre_list.is_empty() {
            p.predelay = match into {
                RevSync::Off => unit(was.predelay_ms / r.pre),
                _ => nearest(r.pre_list.len(), &ms(r.pre_list), was.predelay_ms),
            };
        }
        if decay_in_beats(&self) != decay_in_beats(&p) {
            p.time = if decay_in_beats(&p) {
                let beat_s = tempo.beat_ms() / 1000.0;
                let at = |i: usize| DECAY_BEATS[i] as f32 * beat_s;
                nearest(DECAY_BEATS.len(), &at, was.rt60_s)
            } else {
                exp_inv(r.decay.0, r.decay.1, was.rt60_s)
            };
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
    ParamSpec::continuous(4, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false)
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
