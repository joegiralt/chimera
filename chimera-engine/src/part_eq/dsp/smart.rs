//! SMART EQ's block (ADR 0296): bank 2's, inside the Part EQ's S4 group
//! (`*7part_eq3dsp*`), run by the EQ's door entry after LO CUT and TILT.
//!
//! Each Part has four fixed band-passes (`Band`) in parallel on its post-TILT bus. A band's unity-peak band-pass
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
use crate::dsp::Stereo;
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::{Db, Unit, clampf};
use crate::hw::MAX_PARTS;
use crate::instrument::Sides;
use crate::part_eq::Live;
use crate::part_eq::smart::{
    Amount, BANDS, Band, BandTaps, Blocks, Bq, Caps, CutDb, CutView, Cuts, Dip, Hears, Kick, Level,
    PartKind, PartSmart, Power, Rate, Returned, Role, SMART_VIEW, SmartEq, SmartState, StillCut,
    Sv,
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
/// No control: kicks closer than `HOLD_NONE_S` hold the dip instead.
const KICK_RELEASE_S: f32 = 0.1;
/// Kicks whose onsets come this close (seconds) or closer hold the dip
/// between them whole, for `HOLD_OF_GAP` of their gap: 16ths at 170 BPM
/// are 88 ms apart, less than the release, and would flutter the bass at
/// 11 Hz...
const HOLD_FULL_S: f32 = 0.1;
/// ...and from this far apart not at all: each kick dips and lets go.
/// Between the two the hold fades, so no gap is a cliff (a hard edge at
/// 150 ms was 5.6 dB of flutter one millisecond past it).
const HOLD_NONE_S: f32 = 0.22;
/// A kick's onset is its key hitting after quiet, or rising by this much
/// (power, 2 dB) while still high: a kick over the last one's body...
const ONSET_RISE: f32 = 1.585;
/// ...no sooner than this after the last (seconds): one kick's attack is
/// one onset.
const REFRACT_S: f32 = 0.02;
/// How long past a dense kick's onset its dip is held, of the gap to the
/// one before: a late hit is still covered.
const HOLD_OF_GAP: f32 = 1.5;
/// A hit is the key within this of its recent peak: a kick's attack and
/// the top of its body, not its tail (a 150 ms 808 falls 6 dB in about
/// 100 ms). A kit's LOW that stays up (a held 50 Hz sub, an 808 two
/// seconds long) is a hit for as long as it holds within this: the dip
/// stays down with it, a duck for the tail's whole length. Meant (owner
/// 2026-11-20): the tail counts.
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
const CLEAR_THREAT: Threat = Threat::new(Db::new(12.0), Db::new(0.0));
/// In LOW and LOW MID a quiet ringing tail still clouds a louder Part
/// (owner, 2026-11-19: "muddy", MODAL's tails the worst): it yields fully
/// up to 12 dB under, and some to 30 dB under.
const MUD_THREAT: Threat = Threat::new(Db::new(30.0), Db::new(12.0));

/// A band's masking reach (`threat`): from `none` dB under the higher
/// Part, rising `slope` a dB to full.
#[derive(Clone, Copy)]
struct Threat {
    none: f32,
    slope: f32,
}

impl Threat {
    /// None from `none` under, full from `full` under.
    const fn new(none: Db, full: Db) -> Threat {
        assert!(none.get() > full.get());
        Threat {
            none: none.get(),
            slope: 1.0 / (none.get() - full.get()),
        }
    }
}
/// AMOUNT's curve (owner, 2026-11-19: "Deep is like 75 % of the slider",
/// and the top keeps going past good, so there is always more to reach
/// for): how deep BACK yields under FRONT at four AMOUNTs, one rank apart
/// 2/3 of each. 64 is polite (4 dB one rank, 6 two, everywhere); 95 is
/// DEEP (LOW and LOW MID 8 and 12 dB, the kick's dip 10 and 15); 127 is
/// OVER, provisional until the owner hears it (14 and 21 dB, the kick 18
/// and 27, HIGH MID and HIGH 6 and 9). Re-tune here: `CAPS` follows.
pub const KNOTS: [(Amount, Caps); 4] = [
    (amount(0), caps(0.0, 0.0, 0.0)),
    (amount(64), caps(6.0, 6.0, 6.0)),
    (amount(95), caps(12.0, 6.0, 15.0)),
    (amount(127), caps(21.0, 9.0, 27.0)),
];

const fn amount(code: u8) -> Amount {
    match Amount::from_code(code) {
        Some(a) => a,
        None => panic!("an AMOUNT is 0..=127"),
    }
}

const fn caps(mud: f32, clear: f32, kick: f32) -> Caps {
    assert!(mud <= CutDb::MAX.get() && clear <= CutDb::MAX.get() && kick <= CutDb::MAX.get());
    Caps {
        mud: CutDb::new(mud),
        clear: CutDb::new(clear),
        kick: CutDb::new(kick),
    }
}

/// A monotone cubic through `KNOTS` (Fritsch and Carlson's: each knot's
/// slope the harmonic mean of its two chords', 0 where they differ in
/// sign or one is flat), at AMOUNT `x`: smooth, and never past a knot's
/// value between knots, so more AMOUNT is never a shallower cut.
const fn curve(ys: [f32; 4], x: f32) -> f32 {
    let xs = [
        KNOTS[0].0.code() as f32,
        KNOTS[1].0.code() as f32,
        KNOTS[2].0.code() as f32,
        KNOTS[3].0.code() as f32,
    ];
    let mut chord = [0.0f32; 3];
    let mut i = 0;
    while i < 3 {
        chord[i] = (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]);
        i += 1;
    }
    // The ends take their one chord's slope.
    let mut slope = [chord[0], 0.0, 0.0, chord[2]];
    i = 1;
    while i < 3 {
        if chord[i - 1] * chord[i] > 0.0 {
            slope[i] = 2.0 * chord[i - 1] * chord[i] / (chord[i - 1] + chord[i]);
        }
        i += 1;
    }
    i = 0;
    while i < 2 && x > xs[i + 1] {
        i += 1;
    }
    let h = xs[i + 1] - xs[i];
    let t = (x - xs[i]) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * ys[i]
        + (t3 - 2.0 * t2 + t) * h * slope[i]
        + (-2.0 * t3 + 3.0 * t2) * ys[i + 1]
        + (t3 - t2) * h * slope[i + 1]
}

/// How deep SMART EQ may cut at each AMOUNT: `KNOTS`' curve, by code.
static CAPS: [Caps; Amount::MAX as usize + 1] = {
    let mut t = [Caps::NONE; Amount::MAX as usize + 1];
    let (k0, k1, k2, k3) = (KNOTS[0].1, KNOTS[1].1, KNOTS[2].1, KNOTS[3].1);
    let mut a = 0;
    while a < t.len() {
        let x = a as f32;
        t[a] = Caps {
            mud: CutDb::new(curve(
                [k0.mud.get(), k1.mud.get(), k2.mud.get(), k3.mud.get()],
                x,
            )),
            clear: CutDb::new(curve(
                [
                    k0.clear.get(),
                    k1.clear.get(),
                    k2.clear.get(),
                    k3.clear.get(),
                ],
                x,
            )),
            kick: CutDb::new(curve(
                [k0.kick.get(), k1.kick.get(), k2.kick.get(), k3.kick.get()],
                x,
            )),
        };
        a += 1;
    }
    t
};

/// How deep `smart` may cut: its AMOUNT's on the curve, nothing off.
#[inline(always)]
pub fn caps_at(smart: SmartEq) -> Caps {
    if smart.listens() {
        CAPS[usize::from(smart.amount().code())]
    } else {
        Caps::NONE
    }
}
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

/// Each band's band-pass at each band's centre, `B_b(f_c)` as
/// `RESPONSE[c][b]`, real and imaginary: measured from the filters as
/// they run at 48 kHz (`smart_eq_dsp_test`'s
/// `print_the_unmix_matrix_s_source`). The bands overlap: a neighbour is
/// 0.14 to 0.18 in phase at a centre and 0.34 a quarter turn off.
const RESPONSE: [[(f32, f32); BANDS]; BANDS] = [
    [
        (0.999_91, 0.013_08),
        (0.135_08, 0.345_10),
        (0.008_72, 0.092_95),
        (0.000_49, 0.022_04),
    ],
    [
        (0.149_50, -0.343_72),
        (0.998_63, 0.052_33),
        (0.137_01, 0.343_86),
        (0.007_78, 0.087_84),
    ],
    [
        (0.018_76, -0.092_93),
        (0.184_80, -0.339_13),
        (1.000_00, -0.000_00),
        (0.123_29, 0.328_77),
    ],
    [
        (0.010_41, -0.022_04),
        (0.049_13, -0.087_66),
        (0.123_29, -0.328_77),
        (1.000_00, 0.000_00),
    ],
];

/// What each band's dip must be for the four bands' sum to be the gain
/// asked at each band's centre, as far as their in-phase parts go: the
/// inverse of `RESPONSE`'s real parts. Raw, four 6 dB dips reach −8.8 dB
/// at 428 Hz; with this the sum is at most 0.05 dB deeper than the
/// deepest cut asked, and up to 0.27 dB (AMOUNT 64) to 0.56 dB (127)
/// over unity between two bands, where a cut's neighbour is lifted
/// (ADR 0296 § 6, `the_bands_sum_against_the_cuts_asked_on_a_fine_grid`).
/// Deep, the neighbours' quarter-turn parts hold a band's centre up: two
/// adjacent bands asked 21 dB reach 10 to 18 dB. What the bands really do at each centre is
/// `heard`, and the pages read that. Rows: the band dipped; columns: the
/// gain asked, LOW to HIGH.
pub const UNMIX: [[f32; BANDS]; BANDS] = [
    [1.020_83, -0.139_97, 0.010_36, -0.000_69],
    [-0.154_14, 1.048_30, -0.143_47, 0.009_61],
    [0.009_86, -0.187_78, 1.041_29, -0.126_92],
    [-0.004_27, -0.026_89, -0.121_44, 1.015_18],
];

/// One block of the Parts' buses (after LO CUT and TILT), as the door hands
/// them over.
pub struct SmartBlock<'a> {
    pub buses: &'a mut [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a mut Sides,
    pub written: &'a [bool; MAX_PARTS],
    pub roles: &'a [Role; MAX_PARTS],
    pub kinds: &'a [PartKind; MAX_PARTS],
    /// The reverb return's role.
    pub wash: Role,
    pub smart: SmartEq,
    pub sample_rate: u32,
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

impl Sv {
    /// Band `b` run at `rate` Hz.
    #[inline(always)]
    fn at(b: Band, rate: f32) -> Sv {
        // `sin` of the half angle from its tangent: no libm.
        let t = tan_mid(core::f32::consts::PI * b.centre().get() / rate);
        Sv {
            g: 2.0 * t * crate::dsp::algo::math::inv_sqrt(1.0 + t * t),
        }
    }
}

impl Bq {
    /// Band `b` run at `rate` Hz: RBJ's constant-peak band-pass, `α =
    /// sin ω / 2Q`, normalised; `sin` and `cos` from the half angle's
    /// tangent.
    #[inline(always)]
    fn at(b: Band, rate: f32) -> Bq {
        let t = tan_mid(core::f32::consts::PI * b.centre().get() / rate);
        let n = 1.0 / (1.0 + t * t);
        let (sin, cos) = (2.0 * t * n, (1.0 - t * t) * n);
        let alpha = sin / (2.0 * Q);
        let a0 = 1.0 / (1.0 + alpha);
        Bq {
            b: alpha * a0,
            a1: -2.0 * cos * a0,
            a2: (1.0 - alpha) * a0,
        }
    }
}

/// The bands a pair of filters runs: LOW with LOW MID, HIGH MID with HIGH.
const LOW_PAIR: [Band; 2] = [Band::Low, Band::LowMid];
const HIGH_PAIR: [Band; 2] = [Band::HighMid, Band::High];

/// How far the listen-only bands are decimated: LOW and LOW MID, then
/// HIGH MID and HIGH.
const THIN_LOW: usize = 4;
const THIN_HIGH: usize = 2;
const _: () = assert!(BLOCK_SIZE.is_multiple_of(THIN_LOW) && BLOCK_SIZE.is_multiple_of(THIN_HIGH));

/// `exp(−block / (τ fs))`, a block's one-pole pole.
#[inline(always)]
fn pole(tau_s: f32, sample_rate: u32) -> f32 {
    exp2(-core::f32::consts::LOG2_E * BLOCK_SIZE as f32 / (tau_s * sample_rate as f32))
}

impl Rate {
    /// Out of line: run once a wake, or when the rate changes.
    #[inline(never)]
    fn at(sample_rate: u32) -> Rate {
        let fs = sample_rate as f32;
        let blocks = |s: f32| Blocks::of((s * fs / BLOCK_SIZE as f32) as u16);
        Rate {
            hz: sample_rate,
            low: LOW_PAIR.map(|b| Sv::at(b, fs)),
            high: HIGH_PAIR.map(|b| Bq::at(b, fs)),
            thin_low: LOW_PAIR.map(|b| Sv::at(b, fs / THIN_LOW as f32)),
            thin_high: HIGH_PAIR.map(|b| Bq::at(b, fs / THIN_HIGH as f32)),
            rise: Unit::new(pole(LEVEL_RISE_S, sample_rate)),
            fall: Unit::new(pole(LEVEL_FALL_S, sample_rate)),
            key_fall: Unit::new(pole(KEY_FALL_S, sample_rate)),
            kick_attack: Unit::new(pole(KICK_ATTACK_S, sample_rate)),
            kick_release: Unit::new(pole(KICK_RELEASE_S, sample_rate)),
            attack: Unit::new(pole(ATTACK_S, sample_rate)),
            release: Unit::new(pole(RELEASE_S, sample_rate)),
            hold_full: blocks(HOLD_FULL_S),
            hold_none: blocks(HOLD_NONE_S),
            refract: blocks(REFRACT_S),
        }
    }
}

/// `G − 1` of a cut of `c` dB, `10^(−c/20) − 1`. No cut is 0 exactly.
#[inline(always)]
fn gain_less_one(c: CutDb) -> f32 {
    /// dB of amplitude to a gain's log2: `log2(10) / 20`.
    const DB_TO_LOG2: f32 = core::f32::consts::LOG2_10 / 20.0;
    if c == CutDb::ZERO {
        0.0
    } else {
        exp2(-c.get() * DB_TO_LOG2) - 1.0
    }
}

/// The four bands' dips for the cuts `c`: each band's `G − 1`, unmixed
/// (`UNMIX`) so their sum is the gain asked. No cut is no dip, exactly.
#[inline(always)]
pub fn dips(c: &[CutDb; BANDS]) -> [f32; BANDS] {
    // Plain loops: `array::from_fn`'s instance for these is core's own,
    // linked outside this group (in bank 1) when it is not inlined.
    let mut g = [0.0f32; BANDS];
    for (g, c) in g.iter_mut().zip(c) {
        *g = gain_less_one(*c);
    }
    let mut d = [0.0f32; BANDS];
    for (d, u) in d.iter_mut().zip(&UNMIX) {
        *d = u[0] * g[0] + u[1] * g[1] + u[2] * g[2] + u[3] * g[3];
    }
    d
}

/// What the four bands' dips `d` really cut at each band's centre: the
/// sum `1 + Σ d_b · B_b(f_c)`'s size, as a cut. What the pages read, so a
/// readout is the cut heard there, however the bands stack.
#[inline(always)]
pub fn heard(d: &[f32; BANDS]) -> [CutDb; BANDS] {
    let mut cut = [CutDb::ZERO; BANDS];
    for (cut, row) in cut.iter_mut().zip(&RESPONSE) {
        let (mut re, mut im) = (1.0f32, 0.0f32);
        for (d, (r, i)) in d.iter().zip(row) {
            re += d * r;
            im += d * i;
        }
        *cut = CutDb::new(-DB_PER_LOG2_POWER * log2(re * re + im * im));
    }
    cut
}

/// What a block's bus put in each band (its unity-peak band-pass's mean
/// square) and in LOW's low-pass, the kick's key.
#[derive(Clone, Copy)]
struct Energy {
    bands: [Power; BANDS],
    key: Power,
}

impl Energy {
    const NONE: Energy = Energy {
        bands: [Power::ZERO; BANDS],
        key: Power::ZERO,
    };
}

/// A block's dips for a pair of bands: each one's start and its step a
/// sample, so the block ends on the new dip.
#[derive(Clone, Copy)]
struct Ramp {
    at: [f32; 2],
    step: [f32; 2],
}

impl Ramp {
    #[inline(always)]
    fn of(pair: [Band; 2], from: &[Dip; BANDS], to: &[Dip; BANDS]) -> Ramp {
        let n = BLOCK_SIZE as f32;
        let (f, t) = (
            pair.map(|b| from[b.index()].get()),
            pair.map(|b| to[b.index()].get()),
        );
        Ramp {
            at: f,
            step: [(t[0] - f[0]) / n, (t[1] - f[1]) / n],
        }
    }

    /// Unity at both ends: nothing to add.
    #[inline(always)]
    fn rests(&self) -> bool {
        self.at == [0.0; 2] && self.step == [0.0; 2]
    }
}

/// LOW and LOW MID over a block, `BY` samples averaged a step (1: the
/// full rate; over 1 a boxcar whose first null is the decimated rate).
/// `WRITE` adds each band's `d · k · bp` to `x` (the full rate only);
/// `LISTEN` gives the bands' mean squares and LOW's low-pass's.
/// Two bands at a time: a pair's memory and coefficients stay in
/// registers, and two independent recurrences hide each other's FPU
/// latency.
#[inline(always)]
fn low_pair<const BY: usize, const WRITE: bool, const LISTEN: bool>(
    x: &mut [f32],
    input: &[f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    f: &[Sv; 2],
    ramp: Ramp,
    heard: &mut Energy,
) {
    const { assert!(BY == 1 || !WRITE) };
    let k = 1.0 / Q;
    let [l, m] = LOW_PAIR.map(Band::index);
    // `s1` the band-pass, `s2` the low-pass.
    let (mut bp, mut lp) = ([taps[l].s1, taps[m].s1], [taps[l].s2, taps[m].s2]);
    let (mut d, dd) = (ramp.at.map(|v| v * k), ramp.step.map(|v| v * k));
    let (mut e, mut key) = ([0.0f32; 2], 0.0f32);
    let (steps, _) = input.as_chunks::<BY>();
    for (j, c) in steps.iter().enumerate() {
        let v = if BY == 1 {
            c[0]
        } else {
            c.iter().sum::<f32>() * (1.0 / BY as f32)
        };
        let mut add = 0.0f32;
        for i in 0..2 {
            lp[i] += f[i].g * bp[i];
            let hp = v - lp[i] - k * bp[i];
            bp[i] += f[i].g * hp;
            if LISTEN {
                e[i] += bp[i] * bp[i];
            }
            if WRITE {
                d[i] += dd[i];
                add += d[i] * bp[i];
            }
        }
        if LISTEN {
            key += lp[0] * lp[0];
        }
        if WRITE {
            x[j] += add;
        }
    }
    (taps[l], taps[m]) = (
        BandTaps {
            s1: bp[0],
            s2: lp[0],
        },
        BandTaps {
            s1: bp[1],
            s2: lp[1],
        },
    );
    if LISTEN {
        let n = (k * k) / steps.len() as f32;
        (heard.bands[l], heard.bands[m]) = (Power::new(e[0] * n), Power::new(e[1] * n));
        heard.key = Power::new(key / steps.len() as f32);
    }
}

/// HIGH MID and HIGH over a block, as `low_pair`.
#[inline(always)]
fn high_pair<const BY: usize, const WRITE: bool, const LISTEN: bool>(
    x: &mut [f32],
    input: &[f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    f: &[Bq; 2],
    ramp: Ramp,
    heard: &mut Energy,
) {
    const { assert!(BY == 1 || !WRITE) };
    let [h, t] = HIGH_PAIR.map(Band::index);
    let (mut s1, mut s2) = ([taps[h].s1, taps[t].s1], [taps[h].s2, taps[t].s2]);
    let (mut d, dd) = (ramp.at, ramp.step);
    let mut e = [0.0f32; 2];
    let (steps, _) = input.as_chunks::<BY>();
    for (j, c) in steps.iter().enumerate() {
        let v = if BY == 1 {
            c[0]
        } else {
            c.iter().sum::<f32>() * (1.0 / BY as f32)
        };
        let mut add = 0.0f32;
        for i in 0..2 {
            let bx = f[i].b * v;
            let y = bx + s1[i];
            s1[i] = s2[i] - f[i].a1 * y;
            s2[i] = -bx - f[i].a2 * y;
            if LISTEN {
                e[i] += y * y;
            }
            if WRITE {
                d[i] += dd[i];
                add += d[i] * y;
            }
        }
        if WRITE {
            x[j] += add;
        }
    }
    (taps[h], taps[t]) = (
        BandTaps {
            s1: s1[0],
            s2: s2[0],
        },
        BandTaps {
            s1: s1[1],
            s2: s2[1],
        },
    );
    if LISTEN {
        let n = 1.0 / steps.len() as f32;
        (heard.bands[h], heard.bands[t]) = (Power::new(e[0] * n), Power::new(e[1] * n));
    }
}

/// A cut Part's bus through its four bands at the full rate: every band
/// hears the block as it came in, adds its dip (lerped from `from` to
/// `to` across the block) low to high, and is heard as it goes.
#[inline(always)]
fn full(
    x: &mut [f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    r: &Rate,
    from: &[Dip; BANDS],
    to: &[Dip; BANDS],
) -> Energy {
    let input = *x;
    let mut heard = Energy::NONE;
    let (lo, hi) = (Ramp::of(LOW_PAIR, from, to), Ramp::of(HIGH_PAIR, from, to));
    low_pair::<1, true, true>(x, &input, taps, &r.low, lo, &mut heard);
    high_pair::<1, true, true>(x, &input, taps, &r.high, hi, &mut heard);
    heard
}

/// A listened Part's bus through its four bands, decimated and never
/// written: LOW and LOW MID at a quarter of the rate and HIGH MID and
/// HIGH at half, on coefficients for that rate. What folds down is 20 dB
/// under by the boxcar and lands under the skirts `own` already allows
/// for.
#[inline(never)]
fn thin(x: &[f32; BLOCK_SIZE], taps: &mut [BandTaps; BANDS], r: &Rate) -> Energy {
    let mut heard = Energy::NONE;
    let none = Ramp {
        at: [0.0; 2],
        step: [0.0; 2],
    };
    // Never written: the output it is handed is a scrap.
    let mut scrap = [0.0f32; 0];
    low_pair::<THIN_LOW, false, true>(&mut scrap, x, taps, &r.thin_low, none, &mut heard);
    high_pair::<THIN_HIGH, false, true>(&mut scrap, x, taps, &r.thin_high, none, &mut heard);
    heard
}

/// The bands as they run, at fixed dips (`dips`' or raw), over `x` in
/// whole blocks: for the tests' response measures.
#[cfg(any(test, feature = "test-support"))]
pub fn probe(d: &[f32; BANDS], x: &mut [f32], sample_rate: u32) {
    let r = Rate::at(sample_rate);
    let d = d.map(Dip::new);
    let mut taps = EMPTY;
    let (blocks, _) = x.as_chunks_mut::<BLOCK_SIZE>();
    for b in blocks {
        full(b, &mut taps, &r, &d, &d);
    }
}

/// A cut Part's side through the pairs that cut, and no others: the same
/// dips as its bus, so the cut is the same on left and right. A pair at
/// unity at both ends of the block is not run and its memory rests; the
/// side is never listened to. One copy, for a Part's side and the reverb
/// return's left and right.
#[inline(never)]
fn side_bands(
    x: &mut [f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    r: &Rate,
    from: &[Dip; BANDS],
    to: &[Dip; BANDS],
) {
    let input = *x;
    let mut none = Energy::NONE;
    let (lo, hi) = (Ramp::of(LOW_PAIR, from, to), Ramp::of(HIGH_PAIR, from, to));
    if lo.rests() {
        for b in LOW_PAIR {
            taps[b.index()] = BandTaps::ZERO;
        }
    } else {
        low_pair::<1, true, false>(x, &input, taps, &r.low, lo, &mut none);
    }
    if hi.rests() {
        for b in HIGH_PAIR {
            taps[b.index()] = BandTaps::ZERO;
        }
    } else {
        high_pair::<1, true, false>(x, &input, taps, &r.high, hi, &mut none);
    }
}

/// A band's memory all under `FLUSH` to 0, and a NaN's or an inf's to
/// rest. One test a band: a NaN fails it, and an inf's next block is one.
#[inline(always)]
fn tidy(taps: &mut [BandTaps; BANDS]) {
    for t in taps.iter_mut() {
        let size = t.s1.abs() + t.s2.abs();
        // A NaN is in no range.
        if !(FLUSH..=f32::MAX).contains(&size) {
            *t = BandTaps::ZERO;
        }
    }
}

/// A power as the rule reads it: dB over `SILENCE`.
#[inline(always)]
fn level(p: Power) -> Level {
    Level::over_silence(DB_PER_LOG2_POWER * log2(p.get()) - SILENCE.get())
}

/// How active a band at `l` is: 0 silent, 1 playing.
#[inline(always)]
fn active(l: Level) -> f32 {
    const PER_DB: f32 = 1.0 / ACTIVE_SPAN.get();
    clampf(l.over() * PER_DB, 0.0, 1.0)
}

/// How much a Part at `lower` can mask one at `higher` in a band with
/// reach `t`.
#[inline(always)]
fn threat(lower: Level, higher: Level, t: Threat) -> f32 {
    let under = higher.over() - lower.over();
    clampf((t.none - under) * t.slope, 0.0, 1.0)
}

/// How much of band `b` is a Part's own sound, not its other bands'
/// energy through `b`'s skirt (`SKIRT`): a pure 100 Hz bass is LOW's
/// alone, though LOW MID hears it 8.6 dB down.
#[inline(always)]
fn own(levels: &[Level; BANDS], b: Band) -> f32 {
    let mut leak = f32::MIN;
    for o in Band::ALL {
        if o != b {
            leak = leak.max(levels[o.index()].over() + SKIRT[o.index().abs_diff(b.index())].get());
        }
    }
    const PER_DB: f32 = 1.0 / OWN_SPAN.get();
    clampf((levels[b.index()].over() - leak) * PER_DB, 0.0, 1.0)
}

/// How much a DRUM Part's kick is hitting now: its key within `HIT_SPAN`
/// of its recent peak, the peak itself active. A snare on the same Part
/// barely moves LOW's low-pass.
#[inline(always)]
fn hit(key: Level, peak: Level) -> f32 {
    const PER_DB: f32 = 1.0 / HIT_SPAN.get();
    let under = peak.over() - key.over();
    active(peak) * clampf((HIT_SPAN.get() - under) * PER_DB, 0.0, 1.0)
}

/// How far each role outranks each (`Role::depth_over`), by code: the
/// rule's inner loop reads a table, not a match.
const DEPTH: [[f32; 3]; 3] = {
    let mut t = [[0.0; 3]; 3];
    let mut q = 0;
    while q < 3 {
        let mut p = 0;
        while p < 3 {
            t[q][p] = Role::ALL[q].depth_over(Role::ALL[p]).get();
            p += 1;
        }
        q += 1;
    }
    t
};
const _: () = assert!(Role::Front.code() == 0 && Role::Mid.code() == 1 && Role::Back.code() == 2);

/// How far `higher` outranks `lower`: 0 at or under it.
#[inline(always)]
fn depth(higher: Role, lower: Role) -> f32 {
    DEPTH[usize::from(higher.code())][usize::from(lower.code())]
}

/// Each Part's role and kind, and the reverb return's role.
struct Roles<'a> {
    roles: &'a [Role; MAX_PARTS],
    kinds: &'a [PartKind; MAX_PARTS],
    wash: Role,
}

impl Roles<'_> {
    #[inline(always)]
    fn role(&self, p: PartId) -> Role {
        self.roles[p.index()]
    }

    #[inline(always)]
    fn kind(&self, p: PartId) -> PartKind {
        self.kinds[p.index()]
    }
}

/// A value a Part, read by `PartId`.
#[derive(Clone, Copy)]
struct PerPart<T>([T; MAX_PARTS]);

impl<T: Copy> PerPart<T> {
    /// `f` of each Part, from `fill` (a plain loop: see `dips`).
    #[inline(always)]
    fn of(fill: T, mut f: impl FnMut(PartId) -> T) -> PerPart<T> {
        let mut a = [fill; MAX_PARTS];
        for p in PartId::ALL {
            a[p.index()] = f(p);
        }
        PerPart(a)
    }

    #[inline(always)]
    fn at(&self, p: PartId) -> T {
        self.0[p.index()]
    }
}

/// How hard the Parts over a member of `role` press on it in a band,
/// 0 to 1: the most of `press(q)` by how far `q` outranks it.
#[inline(always)]
fn pressed(roles: &Roles<'_>, role: Role, mut press: impl FnMut(PartId) -> f32) -> f32 {
    let mut most = 0.0f32;
    for q in PartId::ALL {
        let d = depth(roles.role(q), role);
        if d > 0.0 {
            most = most.max(d * press(q));
        }
    }
    most
}

/// The slow rule for band `b` (ADR 0296 § 3): each member's cut target
/// there, the deepest any higher-role Part asks, by AMOUNT's `caps`. `q`
/// presses on `p` where each is active and the band is each one's own
/// sound, and `p` is loud enough there to mask `q`. In LOW a DRUM `q`
/// presses only through the kick's dip (`kick_rule`).
#[inline(always)]
fn slow_rule(st: &mut SmartState, b: Band, roles: &Roles<'_>, caps: Caps) {
    let (reach, cap) = if b.muddy() {
        (MUD_THREAT, caps.mud)
    } else {
        (CLEAR_THREAT, caps.clear)
    };
    // What each Part sounds like in the band: its level, and how much it
    // is there with its own sound (a kit's LOW is its kick's rule's).
    let at = PerPart::of((Level::SILENT, 0.0f32), |p| {
        let h = &st.part(p).heard;
        let keyed = b == Band::Low && roles.kind(p) == PartKind::Drum;
        let there = if keyed {
            0.0
        } else {
            active(h[b.index()]) * own(h, b)
        };
        (h[b.index()], there)
    });
    let want = |heard: &[Level; BANDS], role: Role| {
        let (l, a) = (heard[b.index()], active(heard[b.index()]) * own(heard, b));
        if a == 0.0 {
            return CutDb::ZERO;
        }
        let press = pressed(roles, role, |q| {
            let (lq, aq) = at.at(q);
            aq * threat(l, lq, reach)
        });
        cap.share(Unit::new(a * press))
    };
    for p in PartId::ALL {
        let m = st.part_mut(p);
        m.wants(b, want(&m.heard, roles.role(p)));
    }
    // The reverb's return yields as a Part of its role, to every Part
    // over it; nothing yields to it.
    let w = &mut st.wash.cuts;
    w.wants(b, want(&w.heard, roles.wash));
}

impl PartSmart {
    /// The slow rule's target for band `b`.
    #[inline(always)]
    fn wants(&mut self, b: Band, want: CutDb) {
        self.want[b.index()] = want;
        if want != CutDb::ZERO {
            self.cuts = Cuts::Move;
        }
    }

    /// A block's step of the cuts toward what the rules want: deeper at
    /// the attack's pace, shallower at the release's. Whether there was
    /// anything to step; at rest there is not.
    #[inline(always)]
    fn step(&mut self, r: &Rate) -> bool {
        let k = self.kick_want;
        if self.cuts == Cuts::Rest && k == CutDb::ZERO {
            return false;
        }
        let mut still = self.dipped != [CutDb::ZERO; BANDS];
        for (c, &want) in self.cut.iter_mut().zip(&self.want) {
            let pole = if want > *c { r.attack } else { r.release };
            *c = c.toward(want, pole, LAND_DB);
            still |= *c != CutDb::ZERO || want != CutDb::ZERO;
        }
        let pole = if k > self.kick {
            r.kick_attack
        } else {
            r.kick_release
        };
        self.kick = self.kick.toward(k, pole, LAND_DB);
        still |= self.kick != CutDb::ZERO;
        self.cuts = if still { Cuts::Move } else { Cuts::Rest };
        true
    }

    /// This block's dips: the last block's, and those of the cuts as they
    /// now stand.
    #[inline(always)]
    fn ramp(&mut self) -> ([Dip; BANDS], [Dip; BANDS]) {
        let from = self.dip;
        let applied = self.applied();
        let to = if applied == self.dipped {
            from
        } else {
            dips(&applied).map(Dip::new)
        };
        (self.dip, self.dipped) = (to, applied);
        (from, to)
    }

    /// A block's levels from what was `heard`: each band's power rises
    /// fast and falls slowly; band `read`'s is read in dB.
    #[inline(always)]
    fn follow(&mut self, heard: &Energy, r: &Rate, read: Band) {
        for (l, ms) in self.level.iter_mut().zip(heard.bands) {
            let pole = if ms > *l { r.rise } else { r.fall };
            *l = Power::new(ms.get() + pole.get() * (l.get() - ms.get()));
        }
        self.heard[read.index()] = level(self.level[read.index()]);
    }

    /// This Part's kick as a key, `blocks` blocks on: how hard it presses,
    /// 0 to 1. Its hit; and between kicks that come closer than the dip
    /// can let go, held: whole to `HOLD_FULL_S` apart, fading to none at
    /// `HOLD_NONE_S`, so a dense run is one dip, not a flutter, and no
    /// gap is a cliff.
    #[inline(always)]
    fn kick_press(&mut self, r: &Rate, blocks: Blocks) -> f32 {
        let (key, peak) = (level(self.key), level(self.key_peak));
        let h = hit(key, peak);
        let was = self.hitting;
        if h >= HIT_ON {
            self.hitting = Kick::Hitting;
        } else if h < HIT_OFF {
            self.hitting = Kick::Quiet;
        }
        // An onset: hitting after quiet, or the key rising again while
        // still high (a kick over the last one's body), once per attack.
        let since = self.since.after(blocks);
        let rising = self.key.get() > ONSET_RISE * self.key_was.get() && since >= r.refract;
        self.key_was = self.key;
        if self.hitting == Kick::Hitting && (was == Kick::Quiet || rising) {
            (self.gap, self.since) = (since, Blocks::ZERO);
        } else {
            self.since = since;
        }
        if self.gap == Blocks::ZERO {
            return h;
        }
        let (gap, full, none) = (
            f32::from(self.gap.get()),
            f32::from(r.hold_full.get()),
            f32::from(r.hold_none.get()),
        );
        let hold = clampf((none - gap) / (none - full), 0.0, 1.0);
        if f32::from(self.since.get()) < HOLD_OF_GAP * gap {
            h.max(hold * active(peak))
        } else {
            h
        }
    }
}

/// The kick's rule (ADR 0296 § 4, owner 2026-11-19): each member's fast
/// LOW dip target, the deepest any higher-role DRUM Part's kick asks.
/// With no kit pressing, none. Run every `blocks` blocks.
#[inline(always)]
fn kick_rule(st: &mut SmartState, r: &Rate, roles: &Roles<'_>, cap: CutDb, blocks: Blocks) {
    let mut any = false;
    // How hard each kit's kick presses, and its LOW's level.
    let press = PerPart::of((0.0f32, Level::SILENT), |q| match roles.kind(q) {
        PartKind::Drum if st.part(q).key_peak != Power::ZERO => {
            let m = st.part_mut(q);
            let p = m.kick_press(r, blocks);
            any |= p > 0.0;
            (p, m.heard[Band::Low.index()])
        }
        PartKind::Drum | PartKind::Tonal => (0.0, Level::SILENT),
    });
    if !any {
        for p in PartId::ALL {
            st.part_mut(p).kick_want = CutDb::ZERO;
        }
        st.wash.cuts.kick_want = CutDb::ZERO;
        return;
    }
    let want = |heard: &[Level; BANDS], role: Role| {
        let l = heard[Band::Low.index()];
        let a = active(l) * own(heard, Band::Low);
        if a == 0.0 {
            return CutDb::ZERO;
        }
        let most = pressed(roles, role, |q| {
            let (pq, lq) = press.at(q);
            pq * threat(l, lq, MUD_THREAT)
        });
        cap.share(Unit::new(a * most))
    };
    for p in PartId::ALL {
        let m = st.part_mut(p);
        m.kick_want = want(&m.heard, roles.role(p));
    }
    // The return's LOW takes the kick's dip as a Part of its role would.
    let w = &mut st.wash.cuts;
    w.kick_want = want(&w.heard, roles.wash);
}

/// The rules' turns, in blocks: the slow rule, then the kick's.
const RULES: Blocks = Blocks::of(2);
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
    if st.rate.hz != b.sample_rate {
        st.rate = Rate::at(b.sample_rate);
    }
    let r = st.rate;
    // One band's dB a block, one Part's view; the slow rule a band every
    // other block and the kick's rule the blocks between, so no block
    // carries both.
    let read = Band::of_turn(st.turn);
    let shown = PartId::ALL[usize::from(st.turn) % MAX_PARTS];
    let lanes = b.buses.iter_mut().zip(b.sides.iter_mut()).zip(b.written);
    for (p, ((bus, side), &written)) in st.parts.iter_mut().zip(lanes) {
        let (from, to) = p.ramp();
        let mut heard = Energy::NONE;
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
            match hears {
                Hears::Thin => {
                    heard = thin(bus, &mut p.mid, &r);
                    p.side = EMPTY;
                }
                Hears::Full => {
                    heard = full(bus, &mut p.mid, &r, &from, &to);
                    match side {
                        // A side is there only while a panned pad sounds
                        // (`Sides`): nothing is run for a silent one.
                        Some(side) => {
                            side_bands(side, &mut p.side, &r, &from, &to);
                            tidy(&mut p.side);
                        }
                        None => p.side = EMPTY,
                    }
                }
            }
            tidy(&mut p.mid);
        } else {
            // Nothing on the bus: the filters empty, the levels fall.
            (p.mid, p.side) = (EMPTY, EMPTY);
            // Long silent (its levels read silent well before their
            // powers reach 0) with nothing to let go: nothing to step.
            if p.level == [Power::ZERO; BANDS] && p.key_peak == Power::ZERO && p.cuts == Cuts::Rest
            {
                continue;
            }
        }
        p.follow(&heard, &r, read);
        // The key rises at once and falls fast; its peak falls slowly.
        let ms = heard.key.get();
        p.key = Power::new(ms.max(ms + r.key_fall.get() * (p.key.get() - ms)));
        p.key_peak = Power::new(ms.max(ms + r.fall.get() * (p.key_peak.get() - ms)));
    }
    let roles = Roles {
        roles: b.roles,
        kinds: b.kinds,
        wash: b.wash,
    };
    let caps = caps_at(b.smart);
    let rules = RULES.get() as u8;
    if st.turn.is_multiple_of(rules) {
        slow_rule(st, Band::of_turn(st.turn / rules), &roles, caps);
    } else {
        kick_rule(st, &r, &roles, caps.kick, RULES);
    }
    st.turn = (st.turn + 1) % SmartState::TURNS;
    let mut any = st.wash.cuts.step(&r);
    for (p, id) in st.parts.iter_mut().zip(PartId::ALL) {
        any |= p.step(&r);
        // The pages read at the screen's pace: a Part a block, in turn.
        if id == shown {
            SMART_VIEW.publish(id, CutView::of(&heard(&p.dip.map(Dip::get))));
        }
    }
    st.still = StillCut::count(st);
    if !listens && !any {
        *st = SmartState::REST;
        for id in PartId::ALL {
            SMART_VIEW.publish(id, CutView::NONE);
        }
        SMART_VIEW.publish_wash(CutView::NONE);
    }
}

/// The reverb's return through SMART EQ (ADR 0296 § the return), once it
/// is played and before it joins pair 1: its left and right take the cuts
/// the rules set it this block (`run`, earlier in the block), through the
/// pairs that cut and no others; it is heard as their sum, decimated, for
/// the next block's rules. `Returned::Rests` is a reverb at rest: nothing
/// is run or written. At unity it is read, never written.
#[inline(never)]
pub fn run_wash(st: &mut SmartState, wet: &mut Stereo, reverb: Returned, sample_rate: u32) {
    // The rate is the Parts' block's, made there (`run`) earlier.
    if st.live == Live::Bypass || st.rate.hz != sample_rate {
        return;
    }
    let r = st.rate;
    let read = Band::of_turn(st.turn);
    let w = &mut st.wash;
    let (from, to) = w.cuts.ramp();
    let mut heard = Energy::NONE;
    if reverb == Returned::Sounding {
        // Left and right at equal power: a reverb's sides are decorrelated,
        // so their sum at 1/√2 reads as one side does, and as a Part's
        // bus would (a return with both sides alike reads 3 dB over).
        let mut sum = [0.0f32; BLOCK_SIZE];
        for ((s, l), r) in sum.iter_mut().zip(&wet.l).zip(&wet.r) {
            *s = core::f32::consts::FRAC_1_SQRT_2 * (l + r);
        }
        heard = thin(&sum, &mut w.listen, &r);
        tidy(&mut w.listen);
        if from == UNITY && to == UNITY {
            (w.cuts.mid, w.cuts.side) = (EMPTY, EMPTY);
        } else {
            side_bands(&mut wet.l, &mut w.cuts.mid, &r, &from, &to);
            side_bands(&mut wet.r, &mut w.cuts.side, &r, &from, &to);
            tidy(&mut w.cuts.mid);
            tidy(&mut w.cuts.side);
        }
        w.cuts.follow(&heard, &r, read);
    } else {
        (w.cuts.mid, w.cuts.side, w.listen) = (EMPTY, EMPTY, EMPTY);
        if w.cuts.level != [Power::ZERO; BANDS] {
            w.cuts.follow(&heard, &r, read);
        }
    }
    if read == Band::Low {
        SMART_VIEW.publish_wash(CutView::of(&self::heard(&w.cuts.dip.map(Dip::get))));
    }
}
