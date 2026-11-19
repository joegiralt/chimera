//! SMART EQ's block (ADR 0296): bank 2's, inside the Part EQ's S4 group
//! (`*7part_eq3dsp*`), run by the EQ's door entry after LO CUT and TILT.
//!
//! Each Part has four fixed band-passes (`Band`), TPT state-variable
//! filters in parallel on its post-TILT bus. A band's unity-peak band-pass
//! `bp` is both what SMART EQ hears (its block mean square) and what the
//! band cuts: `y = x + Σ d · bp`. The cuts are decided from every Part's
//! levels and roles (`slow_rule` a band a block in turn, `kick_rule` every
//! block), follow their targets in dB, and their dips (`dips`: each band's
//! share of its neighbours' skirts taken off, so the bands' sum is the cut
//! asked) are lerped sample by sample across the next block.
//!
//! A Part being cut runs its bands at the full rate and is heard through
//! them. A Part only listened to (FRONT, or nothing to yield to) is read,
//! never written, through the same bands decimated (`thin`): LOW and LOW
//! MID at a quarter of the rate, HIGH MID and HIGH at half. Only the bus
//! is ever listened to; a DRUM pad's side takes the bands that cut, and
//! no others.

use chimera_hal::BLOCK_SIZE;

use super::tan_small;
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::{Db, Unit, clampf};
use crate::hw::MAX_PARTS;
use crate::instrument::Sides;
use crate::part_eq::Live;
use crate::part_eq::smart::{
    BANDS, Band, BandTaps, Blocks, CutDb, CutView, Dip, Hears, Kick, Level, PartKind, PartSmart,
    Power, Role, SMART_VIEW, SmartEq, SmartState,
};
use crate::project::PartId;

/// Each band's Q: a two-octave bandwidth, neighbours crossing at −3 dB.
const Q: f32 = 2.0 / 3.0;
/// The level follower rises on power with this time constant, seconds...
const LEVEL_RISE_S: f32 = 0.05;
/// ...and falls with this one: a long MODAL tail or a pad's release is
/// heard as the sustained energy it is, not as peaks gone by.
const LEVEL_FALL_S: f32 = 0.5;
/// The kick's key (a DRUM Part's LOW low-pass energy) falls with this
/// time constant, seconds; it rises at once. Short enough to fall 12 dB
/// within a kick's body.
const KEY_FALL_S: f32 = 0.02;
/// The kick's dip deepens with this time constant, seconds: two blocks,
/// inside the kick's first cycle at 50 Hz.
const KICK_ATTACK_S: f32 = 0.003;
/// ...and lets go with this one (owner's range 60–150 ms) once the hit
/// has passed: over a kick's body, gone before the next kick of a bar in
/// four (`a_front_kick_dips…` holds it under 30 % by 240 ms at 120 BPM).
/// No control: kicks closer than `HOLD_UNDER` hold the dip instead.
const KICK_RELEASE_S: f32 = 0.1;
/// Kicks whose onsets come closer than this (seconds) hold the dip
/// between them, steady, for `HOLD_OF_GAP` of their gap: 16ths at 170 BPM
/// are 88 ms apart, less than the release, and would flutter the bass at
/// 11 Hz. Slower kicks each dip and let go.
const HOLD_UNDER_S: f32 = 0.15;
/// How long past a dense kick's onset its dip is held, of the gap to the
/// one before: a late hit is still covered.
const HOLD_OF_GAP: f32 = 1.5;
/// A hit is the key within this of its recent peak: the kick's attack and
/// the top of its body, not its tail (a 150 ms 808 falls 6 dB in about
/// 100 ms), so the dip is a dip, never a duck.
const HIT_SPAN: Db = Db::new(6.0);
/// A key this far into `HIT_SPAN` is hitting; under `HIT_OFF` it has let
/// go (an onset is the change).
const HIT_ON: f32 = 0.5;
const HIT_OFF: f32 = 0.25;
/// A cut deepens with this time constant, seconds.
const ATTACK_S: f32 = 0.5;
/// A cut lets go with this one, seconds.
const RELEASE_S: f32 = 2.0;
/// Under this a band is silent (dBFS): `Level` counts from here.
pub const SILENCE: Db = Db::new(-66.0);
/// This far over `SILENCE` a band is fully active.
const ACTIVE_SPAN: Db = Db::new(24.0);
/// How far under a higher Part a lower one masks it: not at all from
/// `none` under, fully from `full` under. Above the mud a lower Part
/// masks only near the higher one's level.
const CLEAR_THREAT: Threat = Threat {
    none: Db::new(12.0),
    full: Db::new(0.0),
};
/// In LOW and LOW MID a quiet ringing tail still clouds a louder Part
/// (owner, 2026-11-19: "muddy", MODAL's tails the worst): it yields fully
/// up to 12 dB under, and some to 30 dB under.
const MUD_THREAT: Threat = Threat {
    none: Db::new(30.0),
    full: Db::new(12.0),
};

/// A band's masking reach (`threat`): dB under the higher Part.
#[derive(Clone, Copy)]
struct Threat {
    none: Db,
    full: Db,
}
/// Above the mud a lower Part yields this share of its LOW and LOW MID
/// cut: the mud is where yielding matters most.
const CLEAR_WEIGHT: f32 = 0.75;
/// A band's skirt at the centre of the band `n` away (two octaves each):
/// `−10 log10(1 + Q² (4ⁿ − 4⁻ⁿ)²)` dB at Q 2/3, for n = 1, 2, 3 (index 0
/// unused). What a tone at a neighbour's centre puts in this band.
const SKIRT: [Db; BANDS] = [
    Db::new(0.0),
    Db::new(-8.60),
    Db::new(-20.56),
    Db::new(-32.60),
];
/// A Part's band is its own sound by this much excess over what its
/// other bands' skirts put there: none at 0, all from here.
const OWN_SPAN: Db = Db::new(6.0);
/// A cut under this lands on 0 (dB).
const LAND_DB: f32 = 0.01;
/// A filter's memory under this is silence: flushed to 0 before it can
/// fall subnormal (−300 dB).
const FLUSH: f32 = 1e-15;
/// 10·log10(2): dB of power per octave.
const DB_PER_LOG2_POWER: f32 = 3.010_3;
/// dB of amplitude to a gain's log2: `log2(10) / 20`.
const DB_TO_LOG2: f32 = core::f32::consts::LOG2_10 / 20.0;

/// What each band's dip must be for the four bands' sum to be the gain
/// asked at each band's centre: the inverse of `A[c][b] = Re BP_b(f_c)`,
/// the real part of band `b`'s band-pass at band `c`'s centre (Q 2/3,
/// bilinear at 48 kHz; `1 / (1 + jQ(r − 1/r))`, `r` the warped ratio).
/// The bands overlap (a neighbour is 0.138 at a centre), so four 6 dB
/// dips summed raw reach −8.8 dB at 428 Hz; with this the sum is never
/// more than 0.03 dB deeper than the deepest cut asked, within 0.6 dB of
/// each at its centre, and never over +0.25 dB (`smart_eq_dsp_test`
/// tries every combination). Rows: the band dipped; columns: the gain
/// asked, LOW to HIGH.
pub const UNMIX: [[f32; BANDS]; BANDS] = [
    [1.019_49, -0.142_01, 0.010_66, -0.000_71],
    [-0.142_01, 1.039_00, -0.142_29, 0.009_53],
    [0.010_66, -0.142_29, 1.035_00, -0.126_50],
    [-0.000_71, 0.009_53, -0.126_50, 1.015_52],
];

/// One block of the Parts' buses (after LO CUT and TILT), as the door hands
/// them over.
pub struct SmartBlock<'a> {
    pub buses: &'a mut [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a mut Sides,
    pub written: &'a [bool; MAX_PARTS],
    pub roles: &'a [Role; MAX_PARTS],
    pub kinds: &'a [PartKind; MAX_PARTS],
    pub smart: SmartEq,
    pub sample_rate: u32,
}

/// A band filter's coefficients at a rate (Zavalishin's TPT SVF): `a1 =
/// 1 / (1 + g (g + k))`, `a2 = g a1`, `a3 = g a2`, `k = 1/Q`.
#[derive(Clone, Copy)]
struct Svf {
    a1: f32,
    a2: f32,
    a3: f32,
}

/// `tan x` to x¹¹, for a band's centre at a decimated rate (HIGH at half
/// the rate is 0.84 rad, where `tan_small`'s x⁷ is 0.5 % short; this is
/// within 0.06 %): inlined here, not libm's `tanf` in bank 1.
#[inline(always)]
fn tan_mid(x: f32) -> f32 {
    let x2 = x * x;
    let x9 = x2 * x2 * x2 * x2 * x;
    tan_small(x) + x9 * (62.0 / 2835.0 + x2 * (1382.0 / 155_925.0))
}

impl Svf {
    /// Band `b` run at `rate` Hz.
    #[inline(always)]
    fn at(b: Band, rate: f32) -> Svf {
        let k = 1.0 / Q;
        let g = tan_mid(core::f32::consts::PI * b.centre().get() / rate);
        let a1 = 1.0 / (1.0 + g * (g + k));
        Svf {
            a1,
            a2: g * a1,
            a3: g * g * a1,
        }
    }
}

/// How far the listen-only bands are decimated: LOW and LOW MID, then
/// HIGH MID and HIGH.
const THIN_LOW: usize = 4;
const THIN_HIGH: usize = 2;
const _: () = assert!(BLOCK_SIZE.is_multiple_of(THIN_LOW) && BLOCK_SIZE.is_multiple_of(THIN_HIGH));

/// The four bands' filters, full and decimated, and the block's time
/// constants at the rate.
#[derive(Clone, Copy)]
struct Rate {
    svf: [Svf; BANDS],
    thin: [Svf; BANDS],
    /// One-pole coefficients a block: the level's rise and fall, the
    /// key's fall, the kick dip's and the cut's attack and release.
    rise: f32,
    fall: f32,
    key_fall: f32,
    kick_attack: f32,
    kick_release: f32,
    attack: f32,
    release: f32,
    /// `HOLD_UNDER_S` in blocks.
    hold_under: Blocks,
}

/// `exp(−block / (τ fs))`, a block's one-pole pole.
#[inline(always)]
fn pole(tau_s: f32, sample_rate: u32) -> f32 {
    exp2(-core::f32::consts::LOG2_E * BLOCK_SIZE as f32 / (tau_s * sample_rate as f32))
}

impl Rate {
    #[inline(always)]
    fn at(sample_rate: u32) -> Rate {
        let fs = sample_rate as f32;
        let thin_rate = |b: Band| {
            let by = if b.muddy() { THIN_LOW } else { THIN_HIGH };
            fs / by as f32
        };
        Rate {
            svf: Band::ALL.map(|b| Svf::at(b, fs)),
            thin: Band::ALL.map(|b| Svf::at(b, thin_rate(b))),
            rise: pole(LEVEL_RISE_S, sample_rate),
            fall: pole(LEVEL_FALL_S, sample_rate),
            key_fall: pole(KEY_FALL_S, sample_rate),
            kick_attack: pole(KICK_ATTACK_S, sample_rate),
            kick_release: pole(KICK_RELEASE_S, sample_rate),
            attack: pole(ATTACK_S, sample_rate),
            release: pole(RELEASE_S, sample_rate),
            hold_under: Blocks::of((HOLD_UNDER_S * fs / BLOCK_SIZE as f32) as u16),
        }
    }
}

/// The four bands' dips for the cuts `c`: each band's `G − 1`, unmixed
/// (`UNMIX`) so their sum is the gain asked. No cut is no dip, exactly.
#[inline(always)]
pub fn dips(c: &[CutDb; BANDS]) -> [f32; BANDS] {
    let g: [f32; BANDS] = core::array::from_fn(|b| {
        if c[b] == CutDb::ZERO {
            0.0
        } else {
            exp2(-c[b].get() * DB_TO_LOG2) - 1.0
        }
    });
    core::array::from_fn(|b| {
        let u = &UNMIX[b];
        u[0] * g[0] + u[1] * g[1] + u[2] * g[2] + u[3] * g[3]
    })
}

/// What a block's bus put in each band (`Σ v1²` a sample heard, before
/// `k²`) and in LOW's low-pass, the kick's key.
#[derive(Clone, Copy)]
struct Energy {
    bands: [f32; BANDS],
    key: f32,
}

/// A cut Part's bus through its four bands at the full rate: each band's
/// `k · v1` (the unity-peak band-pass) summed into the output by its dip,
/// lerped from `from` to `to` across the block, and heard as it goes.
/// Two bands at a time over the block: each pair's memory and
/// coefficients fit the M7's 32 float registers (all four interleaved
/// spilled to the stack, 75 instructions a sample), and two independent
/// recurrences hide each other's FPU latency. Every band hears the block
/// as it came in (`input`); the bands add to the output low to high.
#[inline(always)]
fn full(
    x: &mut [f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    r: &Rate,
    from: &[Dip; BANDS],
    to: &[Dip; BANDS],
) -> Energy {
    let k = 1.0 / Q;
    let n = BLOCK_SIZE as f32;
    let input = *x;
    let mut heard = Energy {
        bands: [0.0; BANDS],
        key: 0.0,
    };
    for pair in [[0, 1], [2, 3]] {
        let f = pair.map(|b| r.svf[b]);
        let mut s = pair.map(|b| taps[b]);
        // Each dip's share of `k`, at the block's start and its step a sample.
        let mut d = pair.map(|b| k * from[b].get());
        let dd = pair.map(|b| k * (to[b].get() - from[b].get()) / n);
        let mut e = [0.0f32; 2];
        let mut lp = 0.0f32;
        for (o, &v) in x.iter_mut().zip(input.iter()) {
            let mut add = 0.0f32;
            for i in 0..2 {
                let v3 = v - s[i].s2;
                let v1 = f[i].a1 * s[i].s1 + f[i].a2 * v3;
                let v2 = s[i].s2 + f[i].a2 * s[i].s1 + f[i].a3 * v3;
                s[i].s1 = 2.0 * v1 - s[i].s1;
                s[i].s2 = 2.0 * v2 - s[i].s2;
                e[i] += v1 * v1;
                // LOW's low-pass: the kick's key, under a snare.
                if pair[i] == 0 {
                    lp += v2 * v2;
                }
                d[i] += dd[i];
                add += d[i] * v1;
            }
            *o += add;
        }
        for (i, &b) in pair.iter().enumerate() {
            taps[b] = s[i];
            heard.bands[b] = e[i] / n;
        }
        if pair[0] == 0 {
            heard.key = lp / n;
        }
    }
    heard
}

/// A listened Part's bus through its four bands, decimated and never
/// written: `BY` samples averaged a step (a boxcar, its first null at the
/// decimated rate), LOW and LOW MID at a quarter of the rate and HIGH MID
/// and HIGH at half, on coefficients warped for that rate. What folds
/// down is 20 dB under by the boxcar and lands under the skirts `own`
/// already allows for.
#[inline(always)]
fn thin_pair<const BY: usize>(
    x: &[f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    r: &Rate,
    pair: [usize; 2],
    heard: &mut Energy,
) {
    let f = pair.map(|b| r.thin[b]);
    let mut s = pair.map(|b| taps[b]);
    let mut e = [0.0f32; 2];
    let mut lp = 0.0f32;
    let (steps, _) = x.as_chunks::<BY>();
    for c in steps {
        let v = c.iter().sum::<f32>() * (1.0 / BY as f32);
        for i in 0..2 {
            let v3 = v - s[i].s2;
            let v1 = f[i].a1 * s[i].s1 + f[i].a2 * v3;
            let v2 = s[i].s2 + f[i].a2 * s[i].s1 + f[i].a3 * v3;
            s[i].s1 = 2.0 * v1 - s[i].s1;
            s[i].s2 = 2.0 * v2 - s[i].s2;
            e[i] += v1 * v1;
            if pair[i] == 0 {
                lp += v2 * v2;
            }
        }
    }
    let n = steps.len() as f32;
    for (i, &b) in pair.iter().enumerate() {
        taps[b] = s[i];
        heard.bands[b] = e[i] / n;
    }
    if pair[0] == 0 {
        heard.key = lp / n;
    }
}

/// `thin_pair` on both pairs.
#[inline(always)]
fn thin(x: &[f32; BLOCK_SIZE], taps: &mut [BandTaps; BANDS], r: &Rate) -> Energy {
    let mut heard = Energy {
        bands: [0.0; BANDS],
        key: 0.0,
    };
    thin_pair::<THIN_LOW>(x, taps, r, [0, 1], &mut heard);
    thin_pair::<THIN_HIGH>(x, taps, r, [2, 3], &mut heard);
    heard
}

/// A cut Part's side through the bands that cut, and no others: the same
/// dips as its bus, so the cut is the same on left and right. A band at
/// unity at both ends of the block is not run and its memory rests; the
/// side is never listened to.
#[inline(always)]
fn side_bands(
    x: &mut [f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    r: &Rate,
    from: &[Dip; BANDS],
    to: &[Dip; BANDS],
) {
    let k = 1.0 / Q;
    let n = BLOCK_SIZE as f32;
    let input = *x;
    for b in 0..BANDS {
        if from[b] == Dip::UNITY && to[b] == Dip::UNITY {
            taps[b] = BandTaps::ZERO;
            continue;
        }
        let f = r.svf[b];
        let mut s = taps[b];
        let mut d = k * from[b].get();
        let dd = k * (to[b].get() - from[b].get()) / n;
        for (o, &v) in x.iter_mut().zip(input.iter()) {
            let v3 = v - s.s2;
            let v1 = f.a1 * s.s1 + f.a2 * v3;
            let v2 = s.s2 + f.a2 * s.s1 + f.a3 * v3;
            s.s1 = 2.0 * v1 - s.s1;
            s.s2 = 2.0 * v2 - s.s2;
            d += dd;
            *o += d * v1;
        }
        taps[b] = s;
    }
}

/// Memory under `FLUSH` to 0, a NaN or inf to rest.
#[inline(always)]
fn tidy(taps: &mut [BandTaps; BANDS]) {
    for t in taps.iter_mut() {
        if !(t.s1 + t.s2).is_finite() {
            *t = BandTaps::ZERO;
        }
        if t.s1.abs() < FLUSH {
            t.s1 = 0.0;
        }
        if t.s2.abs() < FLUSH {
            t.s2 = 0.0;
        }
    }
}

/// A power as the rule reads it: dB over `SILENCE`.
#[inline(always)]
fn level(p: Power) -> Level {
    Level::over_silence(Db::new(DB_PER_LOG2_POWER * log2(p.get()) - SILENCE.get()))
}

/// How active a band at `l` is: 0 silent, 1 playing.
#[inline(always)]
fn active(l: Level) -> f32 {
    clampf(l.db().get() / ACTIVE_SPAN.get(), 0.0, 1.0)
}

/// How much a Part at `lower` can mask one at `higher` in a band with
/// reach `t`.
#[inline(always)]
fn threat(lower: Level, higher: Level, t: Threat) -> f32 {
    let under = higher.db().get() - lower.db().get();
    clampf(
        (t.none.get() - under) / (t.none.get() - t.full.get()),
        0.0,
        1.0,
    )
}

/// How much of band `b` is a Part's own sound, not its other bands'
/// energy through `b`'s skirt (`SKIRT`): a pure 100 Hz bass is LOW's
/// alone, though LOW MID hears it 8.6 dB down.
#[inline(always)]
fn own(levels: &[Level; BANDS], b: usize) -> f32 {
    let mut leak = f32::MIN;
    for (o, l) in levels.iter().enumerate() {
        if o != b {
            leak = leak.max(l.db().get() + SKIRT[o.abs_diff(b)].get());
        }
    }
    clampf((levels[b].db().get() - leak) / OWN_SPAN.get(), 0.0, 1.0)
}

/// How much a DRUM Part's kick is hitting now: its key within `HIT_SPAN`
/// of its recent peak, the peak itself active. A snare on the same Part
/// barely moves LOW's low-pass.
#[inline(always)]
fn hit(key: Level, peak: Level) -> f32 {
    let under = peak.db().get() - key.db().get();
    active(peak) * clampf((HIT_SPAN.get() - under) / HIT_SPAN.get(), 0.0, 1.0)
}

/// The slow rule for band `b` (ADR 0296 § 3): each Part's cut target
/// there, the deepest any higher-role Part asks, scaled by AMOUNT
/// (`share`). `q` presses on `p` where each is active and the band is
/// each one's own sound, and `p` is loud enough there to mask `q`. In LOW
/// a DRUM `q` presses only through the kick's dip (`kick_rule`).
#[inline(always)]
fn slow_rule(st: &mut SmartState, b: usize, roles: &Roles<'_>, share: Unit) {
    let (reach, weight) = if Band::ALL[b].muddy() {
        (MUD_THREAT, 1.0)
    } else {
        (CLEAR_THREAT, CLEAR_WEIGHT)
    };
    // What each Part sounds like in the band: its level, and how much it
    // is there with its own sound.
    let at: [(Level, f32); MAX_PARTS] = core::array::from_fn(|p| {
        let h = &st.parts[p].heard;
        (h[b], active(h[b]) * own(h, b))
    });
    let low = b == Band::Low.index();
    for p in 0..MAX_PARTS {
        let (lp, ap) = at[p];
        let mut press = 0.0f32;
        for q in 0..MAX_PARTS {
            let depth = roles.roles[q].depth_over(roles.roles[p]).get();
            let keyed = low && roles.kinds[q] == PartKind::Drum;
            if depth > 0.0 && !keyed {
                let (lq, aq) = at[q];
                press = press.max(depth * aq * ap * threat(lp, lq, reach));
            }
        }
        st.parts[p].want[b] = CutDb::of(Unit::new(share.get() * weight * press));
    }
}

/// Each Part's role and kind.
struct Roles<'a> {
    roles: &'a [Role; MAX_PARTS],
    kinds: &'a [PartKind; MAX_PARTS],
}

impl PartSmart {
    /// This Part's kick as a key, a block on: how hard it presses, 0 to 1.
    /// Its hit, and between kicks that come closer than the dip can let
    /// go (`HOLD_UNDER_S`), held steady: a dense run is one dip, not a
    /// flutter.
    #[inline(always)]
    fn kick_press(&mut self, r: &Rate) -> f32 {
        let (key, peak) = (level(self.key), level(self.key_peak));
        let h = hit(key, peak);
        let was = self.hitting;
        if h >= HIT_ON {
            self.hitting = Kick::Hitting;
        } else if h < HIT_OFF {
            self.hitting = Kick::Quiet;
        }
        if was == Kick::Quiet && self.hitting == Kick::Hitting {
            (self.gap, self.since) = (self.since, Blocks::ZERO);
        } else {
            self.since = self.since.next();
        }
        let dense = self.gap != Blocks::ZERO && self.gap < r.hold_under;
        let held = f32::from(self.since.get()) < HOLD_OF_GAP * f32::from(self.gap.get());
        if dense && held { active(peak) } else { h }
    }

    /// Any band cut or not yet back at unity.
    #[inline(always)]
    fn cutting(&self) -> bool {
        self.cut.iter().any(|c| *c != CutDb::ZERO)
            || self.kick != CutDb::ZERO
            || self.dip.iter().any(|d| *d != Dip::UNITY)
    }
}

/// The kick's rule (ADR 0296 § 4, owner 2026-11-19): each Part's fast LOW
/// dip target, the deepest any higher-role DRUM Part's kick asks.
#[inline(always)]
fn kick_rule(st: &mut SmartState, r: &Rate, roles: &Roles<'_>, share: Unit) -> [CutDb; MAX_PARTS] {
    let low = Band::Low.index();
    let press: [f32; MAX_PARTS] = core::array::from_fn(|q| match roles.kinds[q] {
        PartKind::Drum => st.parts[q].kick_press(r),
        PartKind::Tonal => 0.0,
    });
    core::array::from_fn(|p| {
        let h = &st.parts[p].heard;
        let ap = active(h[low]) * own(h, low);
        let mut most = 0.0f32;
        for q in 0..MAX_PARTS {
            let depth = roles.roles[q].depth_over(roles.roles[p]).get();
            if depth > 0.0 && press[q] > 0.0 {
                let lq = st.parts[q].heard[low];
                most = most.max(depth * press[q] * ap * threat(h[low], lq, MUD_THREAT));
            }
        }
        CutDb::of(Unit::new(share.get() * most))
    })
}

/// A cut toward its target: deeper at `attack`'s pace, shallower at
/// `release`'s (block poles), landing on 0 under `LAND_DB`.
#[inline(always)]
fn follow(c: CutDb, target: CutDb, attack: f32, release: f32) -> CutDb {
    let pole = if target > c { attack } else { release };
    let next = target.get() + pole * (c.get() - target.get());
    if target == CutDb::ZERO && next < LAND_DB {
        CutDb::ZERO
    } else {
        CutDb::new(next)
    }
}

const UNITY: [Dip; BANDS] = [Dip::UNITY; BANDS];
const EMPTY: [BandTaps; BANDS] = [BandTaps::ZERO; BANDS];

/// Every Part's SMART EQ for one block.
#[inline(always)]
pub fn run(st: &mut SmartState, b: SmartBlock<'_>) {
    let listens = b.smart.listens();
    if st.live == Live::Bypass {
        if !listens {
            return;
        }
        *st = SmartState::REST;
        st.live = Live::Running;
    }
    let r = Rate::at(b.sample_rate);
    let k2 = 1.0 / (Q * Q);
    let turn = usize::from(st.turn) % BANDS;
    let lanes = b.buses.iter_mut().zip(b.sides.iter_mut()).zip(b.written);
    for (p, ((bus, side), &written)) in st.parts.iter_mut().zip(lanes) {
        let from = p.dip;
        let applied = p.applied();
        let to = if applied == p.dipped {
            from
        } else {
            dips(&applied).map(Dip::new)
        };
        (p.dip, p.dipped) = (to, applied);
        let mut heard = Energy {
            bands: [0.0; BANDS],
            key: 0.0,
        };
        if written {
            let hears = if from == UNITY && to == UNITY {
                Hears::Thin
            } else {
                Hears::Full
            };
            // The other rate's memory means nothing at this one.
            if hears != p.hears {
                (p.mid, p.hears) = (EMPTY, hears);
            }
            match (hears, side) {
                (Hears::Thin, _) => {
                    heard = thin(bus, &mut p.mid, &r);
                    p.side = EMPTY;
                }
                (Hears::Full, side) => {
                    heard = full(bus, &mut p.mid, &r, &from, &to);
                    match side {
                        // A silent side has nothing to cut.
                        Some(side) if side.iter().any(|v| *v != 0.0) => {
                            side_bands(side, &mut p.side, &r, &from, &to);
                        }
                        Some(_) | None => p.side = EMPTY,
                    }
                }
            }
            tidy(&mut p.mid);
            tidy(&mut p.side);
        } else {
            // Nothing on the bus: the filters empty, the levels fall.
            (p.mid, p.side) = (EMPTY, EMPTY);
        }
        for (l, e) in p.level.iter_mut().zip(heard.bands) {
            let ms = e * k2;
            let pole = if ms > l.get() { r.rise } else { r.fall };
            *l = Power::new(ms + pole * (l.get() - ms));
        }
        // One band's dB a block, in turn: the rule's own pace.
        p.heard[turn] = level(p.level[turn]);
        // The key rises at once and falls fast; its peak falls slowly.
        let ms = heard.key;
        p.key = Power::new(ms.max(ms + r.key_fall * (p.key.get() - ms)));
        p.key_peak = Power::new(ms.max(ms + r.fall * (p.key_peak.get() - ms)));
    }
    let roles = Roles {
        roles: b.roles,
        kinds: b.kinds,
    };
    let share = b.smart.share();
    slow_rule(st, turn, &roles, share);
    st.turn = ((turn + 1) % BANDS) as u8;
    let kick = kick_rule(st, &r, &roles, share);
    let mut any = false;
    for ((p, &k), id) in st.parts.iter_mut().zip(kick.iter()).zip(PartId::ALL) {
        for (c, &want) in p.cut.iter_mut().zip(&p.want) {
            *c = follow(*c, want, r.attack, r.release);
        }
        p.kick = follow(p.kick, k, r.kick_attack, r.kick_release);
        any |= p.cutting();
        SMART_VIEW.publish(id, CutView::of(&p.applied()));
    }
    if !listens && !any {
        *st = SmartState::REST;
        for id in PartId::ALL {
            SMART_VIEW.publish(id, CutView::NONE);
        }
    }
}
