//! A pad's sound (ADR 0269): its FILTER, its AMP, one mod envelope, one
//! LFO and four routes, laid over the voice's `played` snapshot. Every
//! value a newtype or an enum, built clamped or as an `Option`.

use chimera_midi::value::U7;

use super::types::{Hz, Kit, Machine, PadId};
use crate::block::{DiskCode, ParamId, ParamSpec, find_spec};
use crate::dsp::Unit;
use crate::dsp::filter::{FilterKind, FilterMode, K2040Voice};
use crate::dsp::lfo::{LFO_SPECS, LfoParams, LfoShape};
use crate::dsp::lpg;
use crate::dsp::modulator::{EnvSpeed, EnvType, HoldPos};
use crate::hw::Cost;
use crate::params::{EnvParams, FILTER_SPECS, FilterParams, ParamSnapshot};

/// `v` as a U7, past 127 held to it.
const fn u7(v: u8) -> U7 {
    match U7::new(v) {
        Some(u) => u,
        None => U7::MAX,
    }
}

/// 0..=1, both ends exact: `v / 127`.
fn unit7(v: U7) -> Unit {
    Unit::new(f32::from(v.get()) / 127.0)
}

/// An envelope stage's position, 0..=127 on SPEED's range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvPos(U7);

impl EnvPos {
    pub const ZERO: EnvPos = EnvPos(U7::ZERO);

    pub const fn new(v: U7) -> EnvPos {
        EnvPos(v)
    }

    pub const fn get(self) -> U7 {
        self.0
    }

    pub fn unit(self) -> Unit {
        unit7(self.0)
    }
}

/// A decay: a time, or at 127 none (the level holds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvDecay {
    Time(EnvPos),
    Infinite,
}

impl From<U7> for EnvDecay {
    fn from(v: U7) -> EnvDecay {
        if v == U7::MAX {
            EnvDecay::Infinite
        } else {
            EnvDecay::Time(EnvPos(v))
        }
    }
}

impl EnvDecay {
    /// `Time` at 127 is `Infinite`: one value, one code.
    const fn norm(self) -> EnvDecay {
        match self {
            EnvDecay::Time(EnvPos(v)) if v.get() == 127 => EnvDecay::Infinite,
            d => d,
        }
    }

    /// The cell's position: `Infinite` 127.
    pub const fn code(self) -> U7 {
        match self {
            EnvDecay::Time(p) => p.0,
            EnvDecay::Infinite => U7::MAX,
        }
    }
}

/// A pad's AHD envelope: its AMP's and its mod envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadEnv {
    attack: EnvPos,
    hold: EnvPos,
    decay: EnvDecay,
    speed: EnvSpeed,
}

impl PadEnv {
    /// At 1 from the hit on, for good: a slice-1 pad's AMP.
    pub const OPEN: PadEnv = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Infinite,
        EnvSpeed::Med,
    );

    pub const fn new(attack: EnvPos, hold: EnvPos, decay: EnvDecay, speed: EnvSpeed) -> PadEnv {
        PadEnv {
            attack,
            hold,
            decay: decay.norm(),
            speed,
        }
    }

    pub const fn attack(&self) -> EnvPos {
        self.attack
    }

    pub const fn hold(&self) -> EnvPos {
        self.hold
    }

    pub const fn decay(&self) -> EnvDecay {
        self.decay
    }

    pub const fn speed(&self) -> EnvSpeed {
        self.speed
    }

    pub fn set_attack(&mut self, p: EnvPos) {
        self.attack = p;
    }

    pub fn set_hold(&mut self, p: EnvPos) {
        self.hold = p;
    }

    pub fn set_decay(&mut self, d: EnvDecay) {
        self.decay = d.norm();
    }

    pub fn set_speed(&mut self, s: EnvSpeed) {
        self.speed = s;
    }

    /// No attack, no hold, no decay: a constant 1.
    pub fn is_open(&self) -> bool {
        *self == PadEnv::OPEN
    }

    /// As Envelope A runs it: AHD one-shot, sustain 0 and release the
    /// decay. An infinite decay is a sustain at 1 (the level holds).
    pub fn env_params(&self) -> EnvParams {
        self.params_at(self.places())
    }

    /// A, H and D as 0..1 places (k / 127); an infinite decay 1.
    fn places(&self) -> [f32; 3] {
        let d = self.decay.code();
        [self.attack.0, self.hold.0, d].map(|v| unit7(v).get())
    }

    /// `env_params` with A, H and D at the places `p`; an infinite decay
    /// holds whatever its place.
    fn params_at(&self, [attack, hold, d]: [f32; 3]) -> EnvParams {
        let (decay, sustain) = match self.decay {
            EnvDecay::Time(_) => (d, 0.0),
            EnvDecay::Infinite => (1.0, 1.0),
        };
        EnvParams {
            attack,
            hold,
            decay,
            sustain,
            release: decay,
            env_type: EnvType::A,
            speed: self.speed,
            hold_pos: HoldPos::Ahdsr,
            ..EnvParams::default()
        }
    }
}

/// A pad's FILTER: the FLT front's cells. MODE is always one of KIND's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadFilter {
    kind: FilterKind,
    mode: FilterMode,
    voice: K2040Voice,
    cutoff: Hz,
    resonance: Unit,
    morph: Unit,
}

impl PadFilter {
    /// The `FilterParams` ids a pad holds: KIND, MODE, VOICE, CUTOFF,
    /// RESONANCE, MORPH.
    pub const IDS: [ParamId; 6] = [
        FilterParams::KIND,
        FilterParams::MODE,
        FilterParams::VOICE,
        FilterParams::CUTOFF,
        FilterParams::RESONANCE,
        FilterParams::MORPH,
    ];

    /// The pad's cells of `f`.
    pub fn of(f: &FilterParams) -> PadFilter {
        PadFilter {
            kind: f.kind(),
            mode: f.mode(),
            voice: f.voice,
            cutoff: Hz::new(f.cutoff),
            resonance: Unit::new(f.resonance),
            morph: Unit::new(f.morph),
        }
    }

    /// Lays the pad's cells over `f`; the rest of `f` stays.
    pub fn apply(&self, f: &mut FilterParams) {
        f.set_kind_raw(self.kind);
        f.set_mode(self.mode);
        f.voice = self.voice;
        f.cutoff = self.cutoff.get();
        f.resonance = self.resonance.get();
        f.morph = self.morph.get();
    }

    pub const fn kind(&self) -> FilterKind {
        self.kind
    }

    pub const fn mode(&self) -> FilterMode {
        self.mode
    }

    pub const fn voice(&self) -> K2040Voice {
        self.voice
    }

    pub const fn cutoff(&self) -> Hz {
        self.cutoff
    }

    pub const fn resonance(&self) -> Unit {
        self.resonance
    }

    pub const fn morph(&self) -> Unit {
        self.morph
    }

    /// KIND, MODE kept if the kind has it (`FilterParams::set_kind`'s rule).
    pub fn set_kind(&mut self, k: FilterKind) {
        self.kind = k;
        if !k.modes().contains(&self.mode) {
            self.mode = k.default_mode();
        }
    }

    /// Sets `m` if the kind has it; returns whether it did.
    pub fn set_mode(&mut self, m: FilterMode) -> bool {
        let ok = self.kind.modes().contains(&m);
        if ok {
            self.mode = m;
        }
        ok
    }

    pub fn set_voice(&mut self, v: K2040Voice) {
        self.voice = v;
    }

    pub fn set_cutoff(&mut self, c: Hz) {
        self.cutoff = c;
    }

    pub fn set_resonance(&mut self, r: Unit) {
        self.resonance = r;
    }

    pub fn set_morph(&mut self, m: Unit) {
        self.morph = m;
    }
}

/// The pad AMP's MODE: ONE-SHOT ignores note-off; GATED releases its
/// decay at note-off (ADR 0269). Codes fixed by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AmpMode {
    #[default]
    OneShot,
    Gated,
}

impl AmpMode {
    pub const ALL: [AmpMode; 2] = [AmpMode::OneShot, AmpMode::Gated];

    pub const fn code(self) -> u8 {
        match self {
            AmpMode::OneShot => 1,
            AmpMode::Gated => 2,
        }
    }

    /// The code's frozen ident (disk_codes_v1's `C` rows).
    pub const fn disk_ident(self) -> &'static str {
        match self {
            AmpMode::OneShot => "ONE_SHOT",
            AmpMode::Gated => "GATED",
        }
    }

    pub const fn from_code(c: u8) -> Option<AmpMode> {
        match c {
            1 => Some(AmpMode::OneShot),
            2 => Some(AmpMode::Gated),
            _ => None,
        }
    }
}

/// A pad's AMP: its envelope and the Part AMP's VEL, LPG, DECAY, STRIKE.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadAmp {
    env: PadEnv,
    vel: Unit,
    gate: lpg::Knobs,
    mode: AmpMode,
}

impl PadAmp {
    pub const fn new(env: PadEnv, vel: Unit, gate: lpg::Knobs, mode: AmpMode) -> PadAmp {
        PadAmp {
            env,
            vel,
            gate,
            mode,
        }
    }

    pub const fn env(&self) -> &PadEnv {
        &self.env
    }

    pub fn env_mut(&mut self) -> &mut PadEnv {
        &mut self.env
    }

    pub const fn vel(&self) -> Unit {
        self.vel
    }

    /// The kernel's level at velocity `v` (ADR 0269): 1 − VEL·(1 − v),
    /// as v + (1 − VEL)·(1 − v), exact at both ends: VEL 1 is v (slice
    /// 1's), VEL 0 is 1 at any velocity.
    pub fn level(&self, v: crate::Velocity) -> Unit {
        let v = v.unit();
        Unit::new(v + (1.0 - self.vel.get()) * (1.0 - v))
    }

    pub const fn gate(&self) -> lpg::Knobs {
        self.gate
    }

    pub const fn mode(&self) -> AmpMode {
        self.mode
    }

    pub fn set_vel(&mut self, v: Unit) {
        self.vel = v;
    }

    pub fn set_gate(&mut self, g: lpg::Knobs) {
        self.gate = g;
    }

    pub fn set_mode(&mut self, m: AmpMode) {
        self.mode = m;
    }
}

/// The pad LFO's MODE: free-running or restarted at each hit
/// (`LfoParams::sync` 0 and 1). Codes fixed by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LfoMode {
    #[default]
    Free,
    Trig,
}

impl LfoMode {
    pub const ALL: [LfoMode; 2] = [LfoMode::Free, LfoMode::Trig];

    pub const fn code(self) -> u8 {
        match self {
            LfoMode::Free => 1,
            LfoMode::Trig => 2,
        }
    }

    /// The code's frozen ident (disk_codes_v1's `C` rows).
    pub const fn disk_ident(self) -> &'static str {
        match self {
            LfoMode::Free => "FREE",
            LfoMode::Trig => "TRIG",
        }
    }

    pub const fn from_code(c: u8) -> Option<LfoMode> {
        match c {
            1 => Some(LfoMode::Free),
            2 => Some(LfoMode::Trig),
            _ => None,
        }
    }
}

/// A pad's LFO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadLfo {
    wave: LfoShape,
    speed: U7,
    depth: U7,
    phase: U7,
    mode: LfoMode,
}

impl PadLfo {
    /// SINE, free, full depth, SPEED 6 (~0.95 Hz on RATE's line).
    pub const DEFAULT: PadLfo =
        PadLfo::new(LfoShape::Sine, u7(6), U7::MAX, U7::ZERO, LfoMode::Free);

    pub const fn new(wave: LfoShape, speed: U7, depth: U7, phase: U7, mode: LfoMode) -> PadLfo {
        PadLfo {
            wave,
            speed,
            depth,
            phase,
            mode,
        }
    }

    pub const fn wave(&self) -> LfoShape {
        self.wave
    }

    pub const fn speed(&self) -> U7 {
        self.speed
    }

    pub const fn depth(&self) -> U7 {
        self.depth
    }

    pub const fn phase(&self) -> U7 {
        self.phase
    }

    pub const fn mode(&self) -> LfoMode {
        self.mode
    }

    pub fn set_wave(&mut self, w: LfoShape) {
        self.wave = w;
    }

    pub fn set_speed(&mut self, s: U7) {
        self.speed = s;
    }

    pub fn set_depth(&mut self, d: U7) {
        self.depth = d;
    }

    pub fn set_phase(&mut self, p: U7) {
        self.phase = p;
    }

    pub fn set_mode(&mut self, m: LfoMode) {
        self.mode = m;
    }

    /// As `Lfo` runs it: SPEED on LFO RATE's range.
    pub fn lfo_params(&self) -> LfoParams {
        self.params_at([self.speed, self.depth, self.phase].map(|v| unit7(v).get()))
    }

    /// `lfo_params` with SPEED, DEPTH and PHASE at the 0..1 places `p`.
    fn params_at(&self, [speed, depth, phase]: [f32; 3]) -> LfoParams {
        LfoParams {
            rate: LFO_SPECS[LfoParams::RATE.0 as usize].denormalize(speed),
            shape: self.wave.disk_code(),
            sync: match self.mode {
                LfoMode::Free => 0,
                LfoMode::Trig => 1,
            },
            phase_offset: phase,
            depth,
            ..LfoParams::default()
        }
    }
}

/// What a route reads: the pad's mod envelope, LFO and velocity, or the
/// Part's shared LFOs 1–3. Codes fixed by hand, 0 never used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSource {
    ModEnv,
    Lfo,
    Vel,
    Lfo1,
    Lfo2,
    Lfo3,
}

impl RouteSource {
    pub const ALL: [RouteSource; 6] = [
        RouteSource::ModEnv,
        RouteSource::Lfo,
        RouteSource::Vel,
        RouteSource::Lfo1,
        RouteSource::Lfo2,
        RouteSource::Lfo3,
    ];

    pub const fn code(self) -> u8 {
        match self {
            RouteSource::ModEnv => 1,
            RouteSource::Lfo => 2,
            RouteSource::Vel => 3,
            RouteSource::Lfo1 => 4,
            RouteSource::Lfo2 => 5,
            RouteSource::Lfo3 => 6,
        }
    }

    /// −1..1, the LFOs; else 0..1.
    pub const fn is_bipolar(self) -> bool {
        matches!(
            self,
            RouteSource::Lfo | RouteSource::Lfo1 | RouteSource::Lfo2 | RouteSource::Lfo3
        )
    }

    /// Its place in `ALL` and in `RouteValues`.
    pub const fn index(self) -> usize {
        self.code() as usize - 1
    }

    /// The code's frozen ident (disk_codes_v1's `C` rows).
    pub const fn disk_ident(self) -> &'static str {
        match self {
            RouteSource::ModEnv => "MOD_ENV",
            RouteSource::Lfo => "LFO",
            RouteSource::Vel => "VEL",
            RouteSource::Lfo1 => "LFO1",
            RouteSource::Lfo2 => "LFO2",
            RouteSource::Lfo3 => "LFO3",
        }
    }

    pub const fn from_code(c: u8) -> Option<RouteSource> {
        match c {
            1 => Some(RouteSource::ModEnv),
            2 => Some(RouteSource::Lfo),
            3 => Some(RouteSource::Vel),
            4 => Some(RouteSource::Lfo1),
            5 => Some(RouteSource::Lfo2),
            6 => Some(RouteSource::Lfo3),
            _ => None,
        }
    }
}

/// What a route moves. Codes fixed by hand, 0 never used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteDest {
    Cutoff,
    Reso,
    Volume,
    LfoSpeed,
    LfoDepth,
}

impl RouteDest {
    pub const ALL: [RouteDest; 5] = [
        RouteDest::Cutoff,
        RouteDest::Reso,
        RouteDest::Volume,
        RouteDest::LfoSpeed,
        RouteDest::LfoDepth,
    ];

    pub const fn code(self) -> u8 {
        match self {
            RouteDest::Cutoff => 1,
            RouteDest::Reso => 2,
            RouteDest::Volume => 3,
            RouteDest::LfoSpeed => 4,
            RouteDest::LfoDepth => 5,
        }
    }

    /// The code's frozen ident (disk_codes_v1's `C` rows).
    pub const fn disk_ident(self) -> &'static str {
        match self {
            RouteDest::Cutoff => "CUTOFF",
            RouteDest::Reso => "RESO",
            RouteDest::Volume => "VOLUME",
            RouteDest::LfoSpeed => "LFO_SPEED",
            RouteDest::LfoDepth => "LFO_DEPTH",
        }
    }

    pub const fn from_code(c: u8) -> Option<RouteDest> {
        match c {
            1 => Some(RouteDest::Cutoff),
            2 => Some(RouteDest::Reso),
            3 => Some(RouteDest::Volume),
            4 => Some(RouteDest::LfoSpeed),
            5 => Some(RouteDest::LfoDepth),
            _ => None,
        }
    }

    /// It moves the filter.
    pub const fn is_filter(self) -> bool {
        matches!(self, RouteDest::Cutoff | RouteDest::Reso)
    }
}

/// A route's amount, −127..=127.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(i8);

impl Amount {
    pub const ZERO: Amount = Amount(0);

    pub const fn new(a: i8) -> Option<Amount> {
        if a >= -127 { Some(Amount(a)) } else { None }
    }

    /// `a` held to −127..=127.
    pub const fn clamped(a: i8) -> Amount {
        Amount(if a < -127 { -127 } else { a })
    }

    pub const fn get(self) -> i8 {
        self.0
    }
}

/// One of a pad's four routes, 0..=3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteId(u8);

/// Routes a pad.
const ROUTES: usize = 4;

impl RouteId {
    pub const ALL: [RouteId; ROUTES] = [RouteId(0), RouteId(1), RouteId(2), RouteId(3)];

    pub const fn new(i: u8) -> Option<RouteId> {
        if (i as usize) < ROUTES {
            Some(RouteId(i))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// 1..=4, as shown.
    pub const fn number(self) -> u8 {
        self.0 + 1
    }
}

/// A route: `source` moves `dest` by `amount`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadRoute {
    source: RouteSource,
    dest: RouteDest,
    amount: Amount,
}

impl PadRoute {
    /// MOD ENV → CUTOFF at 0: moves nothing.
    pub const OFF: PadRoute = PadRoute::new(RouteSource::ModEnv, RouteDest::Cutoff, Amount::ZERO);

    pub const fn new(source: RouteSource, dest: RouteDest, amount: Amount) -> PadRoute {
        PadRoute {
            source,
            dest,
            amount,
        }
    }

    pub const fn source(&self) -> RouteSource {
        self.source
    }

    pub const fn dest(&self) -> RouteDest {
        self.dest
    }

    pub const fn amount(&self) -> Amount {
        self.amount
    }

    pub fn set_source(&mut self, s: RouteSource) {
        self.source = s;
    }

    pub fn set_dest(&mut self, d: RouteDest) {
        self.dest = d;
    }

    pub fn set_amount(&mut self, a: Amount) {
        self.amount = a;
    }
}

/// A pad's four routes, reached by `RouteId` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadRoutes([PadRoute; ROUTES]);

impl PadRoutes {
    pub const OFF: PadRoutes = PadRoutes([PadRoute::OFF; ROUTES]);

    pub const fn new(r: [PadRoute; ROUTES]) -> PadRoutes {
        PadRoutes(r)
    }

    pub const fn get(&self, i: RouteId) -> PadRoute {
        self.0[i.index()]
    }

    pub fn set(&mut self, i: RouteId, r: PadRoute) {
        self.0[i.index()] = r;
    }

    pub const fn route(&self, i: RouteId) -> &PadRoute {
        &self.0[i.index()]
    }

    pub fn route_mut(&mut self, i: RouteId) -> &mut PadRoute {
        &mut self.0[i.index()]
    }

    /// A route with an amount moves CUTOFF or RESO.
    pub fn moves_filter(&self) -> bool {
        self.0
            .iter()
            .any(|r| r.amount != Amount::ZERO && r.dest.is_filter())
    }

    /// A route with an amount moves `d`.
    pub fn moves(&self, d: RouteDest) -> bool {
        self.0
            .iter()
            .any(|r| r.amount != Amount::ZERO && r.dest == d)
    }

    /// No route has an amount: the pad moves nothing.
    pub fn is_off(&self) -> bool {
        self.0.iter().all(|r| r.amount == Amount::ZERO)
    }

    /// Each destination's sum, a × its source's value (a = amount / 127);
    /// VOLUME the product of each route's gain from unity (ADR 0269): the
    /// source as 0..1 (a bipolar LFO's (v + 1) / 2), a > 0 opening with it,
    /// 1 − a·(1 − v), a < 0 ducking, 1 − |a|·v; never over 1.
    pub fn offsets(&self, v: &RouteValues) -> RouteOffsets {
        self.offsets_at(&self.amounts(), v)
    }

    /// Each route's a = amount / 127.
    pub fn amounts(&self) -> [f32; ROUTES] {
        self.0.map(|r| f32::from(r.amount.0) / 127.0)
    }

    /// A route with an amount in `a` moves a destination `dest` admits.
    pub fn moves_at(&self, a: &[f32; ROUTES], dest: impl Fn(RouteDest) -> bool) -> bool {
        self.0.iter().zip(a).any(|(r, a)| *a != 0.0 && dest(r.dest))
    }

    /// `offsets` with each route's a from `a`: a glided AMOUNT.
    pub fn offsets_at(&self, a: &[f32; ROUTES], v: &RouteValues) -> RouteOffsets {
        let mut o = [0.0f32; 5];
        let mut gain = 1.0f32;
        for (r, &a) in self.0.iter().zip(a).filter(|(_, a)| **a != 0.0) {
            let x = v.0[r.source.index()];
            if r.dest == RouteDest::Volume {
                let u = Unit::new(if r.source.is_bipolar() {
                    (x + 1.0) / 2.0
                } else {
                    x
                })
                .get();
                gain *= if a > 0.0 {
                    1.0 - a * (1.0 - u)
                } else {
                    1.0 + a * u
                };
            } else {
                o[r.dest.code() as usize - 1] += a * x;
            }
        }
        let [cutoff, reso, _, lfo_speed, lfo_depth] = o;
        RouteOffsets {
            cutoff,
            reso,
            volume: Unit::new(gain),
            lfo_speed,
            lfo_depth,
        }
    }
}

/// What the routes read this block, by `RouteSource::index`: the mod
/// envelope 0..1, the LFOs −1..1, VEL 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteValues([f32; 6]);

impl RouteValues {
    pub const fn new(v: [f32; 6]) -> RouteValues {
        RouteValues(v)
    }
}

/// The routes' sums by destination, in each one's offset units
/// (`apply_offset`'s), and VOLUME's gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteOffsets {
    cutoff: f32,
    reso: f32,
    volume: Unit,
    lfo_speed: f32,
    lfo_depth: f32,
}

impl RouteOffsets {
    /// No route: nothing moved, VOLUME at unity.
    pub const NONE: RouteOffsets = RouteOffsets {
        cutoff: 0.0,
        reso: 0.0,
        volume: Unit::ONE,
        lfo_speed: 0.0,
        lfo_depth: 0.0,
    };

    pub const fn cutoff(&self) -> f32 {
        self.cutoff
    }

    pub const fn reso(&self) -> f32 {
        self.reso
    }

    /// VOLUME's gain, 0..=1.
    pub const fn volume(&self) -> Unit {
        self.volume
    }

    pub const fn lfo_speed(&self) -> f32 {
        self.lfo_speed
    }

    pub const fn lfo_depth(&self) -> f32 {
        self.lfo_depth
    }
}

/// A pad's AMP envelope running, OPEN, with no route.
// ponytail: placeholder bill until the unit bench (#951)
pub const PAD_AMP_COST: Cost = Cost(20);

/// A pad's routes running: AMP, mod envelope, LFO and the ramps.
// ponytail: placeholder bill until the unit bench (#951)
pub const PAD_ROUTED_COST: Cost = Cost(55);

/// A filter as billed: its KIND, and for the 2040 its VOICE. Six of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterShape(u8);

impl FilterShape {
    const ALL: [FilterShape; 6] = [
        FilterShape(0),
        FilterShape(1),
        FilterShape(2),
        FilterShape(3),
        FilterShape(4),
        FilterShape(5),
    ];

    pub const fn of(kind: FilterKind, voice: K2040Voice) -> FilterShape {
        FilterShape(match (kind, voice) {
            (FilterKind::Svf, _) => 0,
            (FilterKind::Dirty, _) => 1,
            (FilterKind::Ladder, _) => 2,
            (FilterKind::Sem, _) => 3,
            (FilterKind::K2040, K2040Voice::Rev2) => 4,
            (FilterKind::K2040, K2040Voice::Exp) => 5,
        })
    }

    pub const fn kind(self) -> FilterKind {
        match self.0 {
            0 => FilterKind::Svf,
            1 => FilterKind::Dirty,
            2 => FilterKind::Ladder,
            3 => FilterKind::Sem,
            _ => FilterKind::K2040,
        }
    }

    pub const fn voice(self) -> K2040Voice {
        if self.0 == 5 {
            K2040Voice::Exp
        } else {
            K2040Voice::Rev2
        }
    }

    const fn bit(self) -> u8 {
        1 << self.0
    }
}

/// What a pad's AMP envelope and routes add a hit (`PadSound::mods`):
/// three bills, so `KitBill`'s field holds any of them. Ordered as their
/// costs are, so the costlier is the `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PadMods {
    None,
    /// The AMP envelope runs (`PAD_AMP_COST`).
    Amp,
    /// A route runs (`PAD_ROUTED_COST`).
    Routed,
}

const _: () = assert!(Cost::ZERO.0 < PAD_AMP_COST.0 && PAD_AMP_COST.0 < PAD_ROUTED_COST.0);

impl PadMods {
    const ALL: [PadMods; 3] = [PadMods::None, PadMods::Amp, PadMods::Routed];

    pub const fn cost(self) -> Cost {
        match self {
            PadMods::None => Cost::ZERO,
            PadMods::Amp => PAD_AMP_COST,
            PadMods::Routed => PAD_ROUTED_COST,
        }
    }
}

/// `KitBill::to_bits`' fields: mods 2 bits, the machine 4 (`Machine::index`
/// + 1, 0 for none), the shapes 6 and the moving shapes 6, the gate 1.
const MODS_BITS: u32 = 0x3;
const MACHINE_BITS: u32 = 0xf;
const _: () = assert!(PadMods::ALL.len() <= MODS_BITS as usize + 1);
const _: () = assert!(Machine::ALL.len() <= MACHINE_BITS as usize);

/// What a DRUM Part's hit is billed for its kit (`Voice::drum_cost`): the
/// filter shapes its pads play, those a pad route moves, its costliest
/// pad's mods and its costliest machine. Any bits are a bill
/// (`from_bits`), so the view needs no build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitBill {
    shapes: u8,
    moving: u8,
    mods: PadMods,
    /// The costliest machine; none before a kit is read.
    machine: Option<Machine>,
    /// A pad's LPG, DECAY or STRIKE above 0: the gate runs.
    gated: bool,
}

impl KitBill {
    /// No pad: the Sound's filter alone, the costliest machine.
    pub const NONE: KitBill = KitBill {
        shapes: 0,
        moving: 0,
        mods: PadMods::None,
        machine: None,
        gated: false,
    };

    /// The filter shapes the pads play, each with whether a pad route
    /// moves it.
    pub fn shapes(self) -> impl Iterator<Item = (FilterShape, bool)> {
        FilterShape::ALL
            .into_iter()
            .filter(move |f| self.shapes & f.bit() != 0)
            .map(move |f| (f, self.moving & f.bit() != 0))
    }

    /// The costliest pad's AMP and routes.
    pub const fn mods(self) -> Cost {
        self.mods.cost()
    }

    /// A pad's gate runs (`lpg::Knobs` above 0).
    pub const fn gated(self) -> bool {
        self.gated
    }

    /// The Part's kernel bill: its costliest machine's `Machine::cost`,
    /// the costliest of all with no kit read.
    pub const fn kernel(self) -> Cost {
        match self.machine {
            Some(m) => m.cost(),
            None => Machine::MAX_COST,
        }
    }

    pub const fn to_bits(self) -> u32 {
        let machine = match self.machine {
            Some(m) => m.index() as u32 + 1,
            None => 0,
        };
        self.mods as u32
            | machine << 4
            | (self.shapes as u32) << 16
            | (self.moving as u32) << 24
            | (self.gated as u32) << 31
    }

    pub const fn from_bits(b: u32) -> KitBill {
        let m = (b >> 4 & MACHINE_BITS) as usize;
        let mods = (b & MODS_BITS) as usize;
        KitBill {
            mods: if mods < PadMods::ALL.len() {
                PadMods::ALL[mods]
            } else {
                PadMods::Routed
            },
            machine: if m >= 1 && m <= Machine::ALL.len() {
                Some(Machine::ALL[m - 1])
            } else {
                None
            },
            shapes: (b >> 16) as u8 & 0x3f,
            moving: (b >> 24) as u8 & 0x3f,
            gated: b >> 31 != 0,
        }
    }
}

/// `kit`'s bill: every pad's filter shape, whether a pad's gate runs, the
/// costliest pad's mods and the costliest machine.
pub fn kit_bill(kit: &Kit) -> KitBill {
    PadId::ALL.iter().fold(KitBill::NONE, |b, &p| {
        let s = kit.pad(p).sound();
        let f = FilterShape::of(s.filter.kind, s.filter.voice).bit();
        let m = s.mods();
        let mc = kit.pad(p).machine();
        KitBill {
            shapes: b.shapes | f,
            moving: b.moving | if s.routes.moves_filter() { f } else { 0 },
            mods: b.mods.max(m),
            machine: match b.machine {
                Some(was) if was.cost() >= mc.cost() => Some(was),
                _ => Some(mc),
            },
            gated: b.gated || s.amp.gate != lpg::Knobs::default(),
        }
    })
}

/// A Part's voice bill split at its DRUM kernel (`Voice::drum_cost`): a
/// hit bills the rest and its own machine, nothing taken off, so no
/// later kit can bring it under its cost. Other engines' have no kernel
/// (`flat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartBill {
    rest: Cost,
    kernel: Cost,
}

/// What a DRUM voice's kernel plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKernel {
    /// A hit sounding or waiting to: its machine.
    Sounding(Machine),
    /// It ended, its key still held: no kernel runs.
    Ended,
}

impl PartBill {
    pub const ZERO: PartBill = PartBill::flat(Cost::ZERO);

    /// `rest` beside a kernel billed `kernel`.
    pub const fn new(rest: Cost, kernel: Cost) -> PartBill {
        PartBill { rest, kernel }
    }

    /// A bill with no kernel: not DRUM.
    pub const fn flat(c: Cost) -> PartBill {
        PartBill::new(c, Cost::ZERO)
    }

    /// `extra` on every voice of the Part (a knob's glide).
    pub fn with(self, extra: Cost) -> PartBill {
        PartBill::new(self.rest + extra, self.kernel)
    }

    /// The Part's voice: the rest at its kit's costliest machine.
    pub fn whole(self) -> Cost {
        self.rest + self.kernel
    }

    /// A hit's voice: the rest and its own kernel.
    pub fn hit(self, k: HitKernel) -> Cost {
        self.rest
            + match k {
                HitKernel::Sounding(m) => m.cost(),
                HitKernel::Ended => Cost::ZERO,
            }
    }
}

/// The values a ringing hit follows (`Places`).
const PLACES: usize = 20;

/// CUTOFF's, RES's and MORPH's specs (`PadFilter::IDS`): what a hit's
/// filter glides on.
static FOLLOWED: [&ParamSpec; 3] = [
    filter_spec(PadFilter::IDS[3]),
    filter_spec(PadFilter::IDS[4]),
    filter_spec(PadFilter::IDS[5]),
];

const fn filter_spec(id: ParamId) -> &'static ParamSpec {
    match find_spec(&FILTER_SPECS, id) {
        Some(s) => s,
        None => panic!("a filter id"),
    }
}

/// A pad sound's continuous values as 0..1 places (ADR 0269), what a
/// ringing hit glides at the KNOB coefficient: CUTOFF, RES and MORPH on
/// their specs (CUTOFF's octaves), VEL and the gate's three as themselves,
/// the AMP's and the mod envelope's A, H and D and LFO SPEED, DEPTH and
/// PHASE as k / 127, each route's AMOUNT as a / 127. A linear spec glides
/// alike on either scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Places([f32; PLACES]);

impl Places {
    const AMP: usize = 3;
    const AMP_ENV: usize = 7;
    const ENV: usize = 10;
    const LFO: usize = 13;
    const ROUTES: usize = 16;

    fn three(&self, at: usize) -> [f32; 3] {
        [self.0[at], self.0[at + 1], self.0[at + 2]]
    }

    /// One block toward `to` at the KNOB coefficient `k`, as a Part's
    /// knob glides; whether every place landed.
    pub fn ease(&mut self, to: &Places, k: f32) -> bool {
        let mut landed = true;
        for (v, t) in self.0.iter_mut().zip(to.0) {
            landed &= crate::dsp::knobs::glide_knob(v, t, k);
        }
        landed
    }

    /// The AMP envelope's A, H and D.
    pub fn amp_env(&self) -> [f32; 3] {
        self.three(Self::AMP_ENV)
    }

    /// The mod envelope's A, H and D.
    pub fn env(&self) -> [f32; 3] {
        self.three(Self::ENV)
    }

    /// LFO SPEED, DEPTH and PHASE.
    pub fn lfo(&self) -> [f32; 3] {
        self.three(Self::LFO)
    }

    /// Each route's a.
    pub fn amounts(&self) -> [f32; ROUTES] {
        core::array::from_fn(|i| self.0[Self::ROUTES + i])
    }
}

/// A pad's sound's bytes (`PadSound::to_bytes`).
pub const PAD_SOUND_BYTES: usize = 57;
pub type PadSoundBytes = [u8; PAD_SOUND_BYTES];

/// A pad's FILTER, AMP, mod envelope, LFO and routes (ADR 0269).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadSound {
    filter: PadFilter,
    amp: PadAmp,
    env: PadEnv,
    lfo: PadLfo,
    routes: PadRoutes,
}

// Every f32 inside is a `Unit` or an `Hz`, never NaN: `==` is an
// equivalence, so `Pad`, `Kit` and `DrumHit` stay `Eq`.
impl Eq for PadSound {}

// 57 B of values; the filter's and AMP's f32s pad it past 60.
const _: () = assert!(core::mem::size_of::<PadSound>() <= 64);

impl PadSound {
    /// DRUM INIT's Sound (CLEAN LP4 open, VEL 1, no gate) with an open
    /// AMP, a short mod envelope, the default LFO and no routes.
    pub const DEFAULT: PadSound = PadSound {
        filter: PadFilter {
            kind: FilterKind::Svf,
            mode: FilterMode::Lp24,
            voice: K2040Voice::Rev2,
            cutoff: Hz::new(20_000.0),
            resonance: Unit::ZERO,
            morph: Unit::ZERO,
        },
        amp: PadAmp::new(
            PadEnv::OPEN,
            Unit::ONE,
            lpg::Knobs::of([0.0; 3]),
            AmpMode::OneShot,
        ),
        env: PadEnv::new(
            EnvPos::ZERO,
            EnvPos::ZERO,
            EnvDecay::Time(EnvPos(u7(64))),
            EnvSpeed::Med,
        ),
        lfo: PadLfo::DEFAULT,
        routes: PadRoutes::OFF,
    };

    pub const fn new(
        filter: PadFilter,
        amp: PadAmp,
        env: PadEnv,
        lfo: PadLfo,
        routes: PadRoutes,
    ) -> PadSound {
        PadSound {
            filter,
            amp,
            env,
            lfo,
            routes,
        }
    }

    /// `p`'s FILTER and AMP cells, the rest `DEFAULT`'s.
    #[optimize(size)]
    pub fn from_sound(p: &ParamSnapshot) -> PadSound {
        PadSound {
            filter: PadFilter::of(&p.filter),
            amp: PadAmp {
                vel: Unit::new(p.out.vca_vel),
                gate: lpg::Knobs::of(p.out.gate),
                ..PadSound::DEFAULT.amp
            },
            ..PadSound::DEFAULT
        }
    }

    /// Its continuous values as places (`Places`).
    #[optimize(size)]
    pub fn places(&self) -> Places {
        let f = &self.filter;
        let a = &self.amp;
        let mut p = [0.0; PLACES];
        let filter = [f.cutoff.get(), f.resonance.get(), f.morph.get()];
        for ((o, v), s) in p.iter_mut().zip(filter).zip(FOLLOWED) {
            *o = s.normalize(v);
        }
        let amp = [a.vel, a.gate.lpg, a.gate.decay, a.gate.strike].map(Unit::get);
        p[Places::AMP..Places::AMP_ENV].copy_from_slice(&amp);
        p[Places::AMP_ENV..Places::ENV].copy_from_slice(&a.env.places());
        p[Places::ENV..Places::LFO].copy_from_slice(&self.env.places());
        let l = &self.lfo;
        let lfo = [l.speed, l.depth, l.phase].map(|v| unit7(v).get());
        p[Places::LFO..Places::ROUTES].copy_from_slice(&lfo);
        p[Places::ROUTES..].copy_from_slice(&self.routes.amounts());
        Places(p)
    }

    /// What a hit struck on `self` plays once `live` is the pad's (ADR
    /// 0269): `self`'s choices (filter KIND, MODE, VOICE, AMP MODE, each
    /// envelope's SPEED, LFO WAVE and MODE, each route's SOURCE and DEST)
    /// with `live`'s continuous values. A decay's hold (127) is a choice
    /// too: a held decay stays held, a timed one stays timed.
    #[optimize(size)]
    pub fn live_target(&self, live: &PadSound) -> PadSound {
        let env = |s: &PadEnv, l: &PadEnv| PadEnv {
            attack: l.attack,
            hold: l.hold,
            decay: match (s.decay, l.decay) {
                (EnvDecay::Time(_), EnvDecay::Time(_)) => l.decay,
                _ => s.decay,
            },
            speed: s.speed,
        };
        let (s, l) = (self, live);
        PadSound {
            filter: PadFilter {
                cutoff: l.filter.cutoff,
                resonance: l.filter.resonance,
                morph: l.filter.morph,
                ..s.filter
            },
            amp: PadAmp {
                env: env(&s.amp.env, &l.amp.env),
                vel: l.amp.vel,
                gate: l.amp.gate,
                mode: s.amp.mode,
            },
            env: env(&s.env, &l.env),
            lfo: PadLfo {
                speed: l.lfo.speed,
                depth: l.lfo.depth,
                phase: l.lfo.phase,
                ..s.lfo
            },
            routes: PadRoutes(core::array::from_fn(|i| PadRoute {
                amount: l.routes.0[i].amount,
                ..s.routes.0[i]
            })),
        }
    }

    /// The FILTER's CUTOFF, RES and MORPH and the AMP's VEL and gate at
    /// `p`'s places: what `overlay` lays on, mid-glide.
    #[optimize(size)]
    pub fn set_places(&mut self, p: &Places) {
        let [c, r, m] = core::array::from_fn(|i| FOLLOWED[i].denormalize(p.0[i]));
        let f = &mut self.filter;
        (f.cutoff, f.resonance, f.morph) = (Hz::new(c), Unit::new(r), Unit::new(m));
        let [vel, lpg, decay, strike] = core::array::from_fn(|i| Unit::new(p.0[Places::AMP + i]));
        self.amp.vel = vel;
        self.amp.gate = lpg::Knobs { lpg, decay, strike };
    }

    /// The AMP envelope's `EnvParams` at `p`'s places.
    pub fn amp_env_at(&self, p: &Places) -> EnvParams {
        self.amp.env.params_at(p.amp_env())
    }

    /// The mod envelope's `EnvParams` at `p`'s places.
    pub fn env_at(&self, p: &Places) -> EnvParams {
        self.env.params_at(p.env())
    }

    /// The LFO's `LfoParams` at `p`'s places.
    pub fn lfo_at(&self, p: &Places) -> LfoParams {
        self.lfo.params_at(p.lfo())
    }

    /// Which of its AMP envelope and routes run.
    pub fn mods(&self) -> PadMods {
        if !self.routes.is_off() {
            PadMods::Routed
        } else if !self.amp.env.is_open() {
            PadMods::Amp
        } else {
            PadMods::None
        }
    }

    /// Lays the FILTER and AMP's gate cells over `m`. VEL is the pad's
    /// own (`PadAmp::level`): a Part VCA route keeps the Sound's.
    pub fn overlay(&self, m: &mut ParamSnapshot) {
        self.filter.apply(&mut m.filter);
        let g = self.amp.gate;
        m.out.gate = [g.lpg.get(), g.decay.get(), g.strike.get()];
    }

    pub const fn filter(&self) -> &PadFilter {
        &self.filter
    }

    pub fn filter_mut(&mut self) -> &mut PadFilter {
        &mut self.filter
    }

    pub const fn amp(&self) -> &PadAmp {
        &self.amp
    }

    pub fn amp_mut(&mut self) -> &mut PadAmp {
        &mut self.amp
    }

    pub const fn env(&self) -> &PadEnv {
        &self.env
    }

    pub fn env_mut(&mut self) -> &mut PadEnv {
        &mut self.env
    }

    pub const fn lfo(&self) -> &PadLfo {
        &self.lfo
    }

    pub fn lfo_mut(&mut self) -> &mut PadLfo {
        &mut self.lfo
    }

    pub const fn routes(&self) -> &PadRoutes {
        &self.routes
    }

    pub fn routes_mut(&mut self) -> &mut PadRoutes {
        &mut self.routes
    }

    /// 57 bytes: filter 15 (KIND, MODE, VOICE, then CUTOFF, RES and MORPH
    /// as f32 LE), AMP 21 (A, H, D, SPEED, MODE, then VEL, LPG, DECAY and
    /// STRIKE as f32 LE), mod env 4 (A, H, D, SPEED), LFO 5 (WAVE, SPEED,
    /// DEPTH, PHASE, MODE), routes 12 (SOURCE, DEST, AMOUNT ×4).
    #[optimize(size)]
    pub fn to_bytes(&self) -> PadSoundBytes {
        let mut b = [0; PAD_SOUND_BYTES];
        let mut w = Writer { b: &mut b, at: 0 };
        let f = &self.filter;
        w.put(&[f.kind.disk_code(), f.mode.disk_code(), f.voice.disk_code()]);
        for v in [f.cutoff.get(), f.resonance.get(), f.morph.get()] {
            w.put(&v.to_le_bytes());
        }
        let a = &self.amp;
        w.env(&a.env);
        w.put(&[a.mode.code()]);
        for v in [a.vel, a.gate.lpg, a.gate.decay, a.gate.strike] {
            w.put(&v.get().to_le_bytes());
        }
        w.env(&self.env);
        let l = &self.lfo;
        w.put(&[
            l.wave.disk_code(),
            l.speed.get(),
            l.depth.get(),
            l.phase.get(),
            l.mode.code(),
        ]);
        for r in self.routes.0 {
            w.put(&[r.source.code(), r.dest.code(), r.amount.get() as u8]);
        }
        b
    }

    /// A sound from `to_bytes`' bytes, `d` the default: an unknown code or
    /// a non-finite float takes `d`'s field, a value past its range is held
    /// to it, a MODE the KIND lacks is `d`'s if the KIND has it, else the
    /// KIND's default. Total: any bytes are a sound.
    #[inline(never)]
    #[optimize(size)]
    pub fn from_bytes(b: PadSoundBytes, d: &PadSound) -> PadSound {
        let mut r = Reader { b: &b, at: 0 };
        let df = &d.filter;
        let kind = FilterKind::from_disk_code(r.byte()).unwrap_or(df.kind);
        let mode = [FilterMode::from_disk_code(r.byte()), Some(df.mode)]
            .into_iter()
            .flatten()
            .find(|m| kind.modes().contains(m))
            .unwrap_or(kind.default_mode());
        let voice = K2040Voice::from_disk_code(r.byte()).unwrap_or(df.voice);
        let filter = PadFilter {
            kind,
            mode,
            voice,
            cutoff: Hz::new(r.f32_or(df.cutoff.get())),
            resonance: Unit::new(r.f32_or(df.resonance.get())),
            morph: Unit::new(r.f32_or(df.morph.get())),
        };
        let da = &d.amp;
        let env = r.env(&da.env);
        let mode = AmpMode::from_code(r.byte()).unwrap_or(da.mode);
        let mut unit = |u: Unit| Unit::new(r.f32_or(u.get()));
        let amp = PadAmp {
            env,
            mode,
            vel: unit(da.vel),
            gate: lpg::Knobs {
                lpg: unit(da.gate.lpg),
                decay: unit(da.gate.decay),
                strike: unit(da.gate.strike),
            },
        };
        let env = r.env(&d.env);
        let dl = &d.lfo;
        let lfo = PadLfo {
            wave: LfoShape::from_disk_code(r.byte()).unwrap_or(dl.wave),
            speed: u7(r.byte()),
            depth: u7(r.byte()),
            phase: u7(r.byte()),
            mode: LfoMode::from_code(r.byte()).unwrap_or(dl.mode),
        };
        let mut routes = d.routes;
        for (o, dr) in routes.0.iter_mut().zip(d.routes.0) {
            *o = PadRoute {
                source: RouteSource::from_code(r.byte()).unwrap_or(dr.source),
                dest: RouteDest::from_code(r.byte()).unwrap_or(dr.dest),
                amount: Amount::clamped(r.byte() as i8),
            };
        }
        PadSound {
            filter,
            amp,
            env,
            lfo,
            routes,
        }
    }
}

/// `to_bytes`' cursor.
struct Writer<'a> {
    b: &'a mut PadSoundBytes,
    at: usize,
}

impl Writer<'_> {
    fn put(&mut self, v: &[u8]) {
        self.b[self.at..self.at + v.len()].copy_from_slice(v);
        self.at += v.len();
    }

    fn env(&mut self, e: &PadEnv) {
        self.put(&[
            e.attack.0.get(),
            e.hold.0.get(),
            e.decay.code().get(),
            e.speed.disk_code(),
        ]);
    }
}

/// `from_bytes`' cursor; reads are in bounds by the layout's sum.
struct Reader<'a> {
    b: &'a PadSoundBytes,
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> u8 {
        let v = self.b[self.at];
        self.at += 1;
        v
    }

    /// The next f32 LE, `d` if it isn't finite.
    fn f32_or(&mut self, d: f32) -> f32 {
        let v = f32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()]);
        if v.is_finite() { v } else { d }
    }

    fn env(&mut self, d: &PadEnv) -> PadEnv {
        PadEnv {
            attack: EnvPos(u7(self.byte())),
            hold: EnvPos(u7(self.byte())),
            decay: EnvDecay::from(u7(self.byte())),
            speed: EnvSpeed::from_disk_code(self.byte()).unwrap_or(d.speed),
        }
    }
}
