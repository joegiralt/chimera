// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLEAN: the TPT state-variable filter with `sat_c1` on both states, in
//! the feedback loop, so it bends instead of blowing up.
//!
//! Coefficients are set once a block (spec § 2, B1). A steady block, `g`
//! and `k` equal at both ends, divides once for the block; a moving one
//! ramps `g` and `k` and divides exactly every sample. `d` is convex in
//! `g`, so lerping it is wrong by up to 95 % on a one-block jump. Each
//! stage takes its `d`, and never divides. NOTCH2 is not an SVF: four TPT
//! all-passes with the dry, and `G₁` takes the divide.

use super::coef::{g_of, sat_c1};
use super::fed::OpenCall;
use super::level::{FilterMods, LevelRamp, LevelStage, level_gain};
use super::{CLEAN_MODES, Fed, FilterMode, SUB, TapShape, g_at, ramps, step_to, tap_g_range};
use crate::hw::BLOCK_SIZE;
use crate::meter::Clip;
use crate::params::FilterParams;

/// RES's `k`: 2 at RES 0, 0 at 0.95 (the onset, spec § CLEAN's DSP) and
/// −0.105 at RES 1, where CLEAN self-oscillates into `sat_c1`. `k > −2`
/// keeps `1 + kg + g² ≥ 0.997` for every `g`.
#[inline(always)]
pub fn clean_k(res: f32) -> f32 {
    2.0 * (1.0 - res * (1.0 / 0.95))
}

/// The LP4 cascade's second-stage `k₂`: it resonates, but never oscillates
/// on its own.
#[inline(always)]
pub fn clean_k2(k: f32) -> f32 {
    (k * 1.2 + 0.3).max(0.1)
}

/// NOTCH2's feedback, `0.7·RES`, at `k = clean_k(RES)`: linear in `k`, so it
/// ramps with it. 0 at RES 0, 0.7 at RES 1; the Cascadia's PHZR never
/// oscillates, and neither does this: held to ±0.7 whatever RES a route or
/// tap reaches.
#[inline(always)]
pub fn notch2_fb(k: f32) -> f32 {
    crate::dsp::clampf((0.7 * 0.95) * (1.0 - 0.5 * k), -0.7, 0.7)
}

/// `mode`'s output trim at `k`: 1 for `k ≥ 0.5`, linear to its trim at
/// `k ≤ 0`: the middle of its untrimmed RES 1 range over 100 Hz – 8 kHz,
/// brought to −8 dBFS, the middle of the shared band (`OSC_BAND_DB`).
/// NOTCH2 never oscillates: its trim is `√(1 − fb)·½`, which holds its
/// loudness as Q feeds back. Set at each end of a block and ramped across
/// it, as `k` is: one multiply-add and one multiply a sample.
#[inline(always)]
pub fn trim(mode: FilterMode, k: f32) -> f32 {
    // Untrimmed at RES 1, dBFS.
    let osc = match mode {
        FilterMode::Lp6 => 0.3277,  // +0.89 to +2.49
        FilterMode::Lp12 => 0.4756, // −2.35 to −0.74
        FilterMode::Lp24 => 0.3949, // −1.59 to +1.73
        FilterMode::Bp12 => 0.2374, // +3.70 to +5.28
        FilterMode::Bp24 => 0.191,  // +5.00 to +7.76
        // HP12 never reaches CLEAN (`process` runs HP4 for it).
        FilterMode::Hp24 | FilterMode::Hp12 => 0.3367, // +0.62 to +2.29
        FilterMode::Notch => 0.6324,                   // −4.82 to −3.22
        FilterMode::Phaser => return 0.5 * crate::dsp::sqrt(1.0 - notch2_fb(k)),
    };
    let u = crate::dsp::clampf(1.0 - 2.0 * k, 0.0, 1.0);
    1.0 + (osc - 1.0) * u
}

#[derive(Clone, Debug, Default)]
pub struct Clean {
    s1: [f32; 2],
    s2: [f32; 2],
    /// NOTCH2's four all-pass states, and its cascade's last output (the
    /// feedback, one sample late).
    ap: [f32; 4],
    w: f32,
    /// The last block's ends; `None` on a fresh voice, which starts
    /// unramped. One `Option` for the three: they are primed and forgotten
    /// together, 8 B under three (ADR 0101's 2,112 B voice chain).
    last: Option<Ends>,
    /// LEVEL's gain and clipper, before the loop.
    level: LevelStage,
    /// FB's signal, the output a sample late through its one-pole (#393).
    fbp: f32,
}

/// A block's end values of `g`, `k` and LEVEL's gain.
#[derive(Clone, Copy, Debug)]
struct Ends {
    g: f32,
    k: f32,
    pre: f32,
}

/// LP4's open top (#695): over CUTOFF's top third of an octave, from
/// `FROM_HZ` to `TOP_HZ`, `g` lifts to `2^RISE` times itself, so the
/// cascade opens toward Nyquist, where it passes everything; over the
/// band's last quarter, where it lags under 20° at 15 kHz, the dry comes
/// in, so fully open LP4 is its LEVEL'd input, bit for bit. Both are
/// continuous in CUTOFF and ramp across the block as `g` does. Bank 2's
/// (`fed`): with the door closed LP4 plays as it always has.
#[derive(Clone, Copy, Debug)]
pub struct OpenTop {
    lo: f32,
    /// `1/(top − lo)`, the band's width in `g`: `u` multiplies by it
    /// instead of dividing a sample.
    per: f32,
    /// The lifted `g`s where the dry starts and at the top.
    dry_from: f32,
    dry_top: f32,
}

impl OpenTop {
    pub const FROM_HZ: f32 = 15_874.01;
    pub const TOP_HZ: f32 = 20_000.0;
    /// Octaves `g` lifts by at the top.
    pub const RISE: f32 = 4.0;
    /// Where in the band the dry starts.
    pub const DRY_FROM: f32 = 0.75;
    /// The `k` the band damps to at its top: where `trim` reaches 1.
    pub const K_OPEN: f32 = 0.5;

    /// The band at `fs`: two `tan`s and two lifts, taken at compile time
    /// for 48 kHz (`for_rate`); inlined, so another rate's is bank 2's.
    #[inline(always)]
    pub const fn at(fs: u32) -> OpenTop {
        let (lo, top) = (
            g_of(Self::FROM_HZ, fs as f32),
            g_of(Self::TOP_HZ, fs as f32),
        );
        let t = OpenTop {
            lo,
            per: 1.0 / (top - lo),
            dry_from: 0.0,
            dry_top: 0.0,
        };
        OpenTop {
            dry_from: t.lifted(lo + Self::DRY_FROM * (top - lo)),
            dry_top: t.lifted(top),
            ..t
        }
    }

    const AT_48K: OpenTop = OpenTop::at(crate::hw::SAMPLE_RATE);

    /// The band at `fs`: 48 kHz's constant, any other rate's taken.
    #[inline(always)]
    pub fn for_rate(fs: u32) -> OpenTop {
        if fs == crate::hw::SAMPLE_RATE {
            Self::AT_48K
        } else {
            Self::at(fs)
        }
    }

    /// Where `g` lies in the band, 0 to 1.
    #[inline(always)]
    const fn u(&self, g: f32) -> f32 {
        crate::dsp::clampf((g - self.lo) * self.per, 0.0, 1.0)
    }

    /// `k` under `K_OPEN` damped toward it as `u` rises through the band,
    /// so a resonant peak the lift carries toward Nyquist fades instead
    /// of ringing there (#1135's review, minor 4). The trim, linear in `k`
    /// to `K_OPEN`, rises with it to 1 at the top, the dry's level; a `k`
    /// past it, already untrimmed, is kept.
    #[inline(always)]
    fn damped(k: f32, u: f32) -> f32 {
        k.max(k + (Self::K_OPEN - k) * u)
    }

    /// `g` lifted: itself under the band, `2^(RISE·u)` times itself in it.
    #[inline(always)]
    const fn lifted(&self, g: f32) -> f32 {
        self.lifted_at(g, self.u(g))
    }

    /// `g` lifted at band position `u`.
    #[inline(always)]
    const fn lifted_at(&self, g: f32, u: f32) -> f32 {
        if u == 0.0 {
            g
        } else {
            g * crate::dsp::fast_exp2(Self::RISE * u)
        }
    }

    /// The dry's weight at lifted `g`: 0 to `dry_from`, 1 at `dry_top`,
    /// linear in the lifted `g` between.
    #[inline(always)]
    fn weight(&self, g: f32) -> f32 {
        crate::dsp::clampf(
            (g - self.dry_from) / (self.dry_top - self.dry_from),
            0.0,
            1.0,
        )
    }
}

/// The modes that run the second stage: the four-pole cascades.
#[inline(always)]
pub(super) fn cascade(m: FilterMode) -> bool {
    matches!(
        m,
        FilterMode::Lp24 | FilterMode::Bp24 | FilterMode::Hp24 | FilterMode::Hp12
    )
}

/// One block's `(from, to)` for `g`, `k` and the LEVEL gain.
#[derive(Clone, Copy)]
pub(super) struct Ramps {
    g: (f32, f32),
    k: (f32, f32),
    pre: (f32, f32),
}

/// A sample's coefficients: `d = 1/(1 + kg + g²)` for stage 1, and `d2`
/// at `k2` for stage 2, and the output trim. NOTCH2 reads `two_g1 =
/// 2g/(1 + g)` (its all-passes' `2·G₁`) and `fb` instead.
#[derive(Clone, Copy)]
struct Coef {
    g: f32,
    k: f32,
    k2: f32,
    d: f32,
    d2: f32,
    two_g1: f32,
    fb: f32,
    trim: f32,
}

/// Mode `M`'s coefficients at `g` and `k`: a divide a stage, and only
/// those `M` reads.
#[inline(always)]
fn coef<const M: u8>(g: f32, k: f32, trim: f32) -> Coef {
    let d_at = |k: f32| 1.0 / (1.0 + k * g + g * g);
    let mut c = Coef {
        g,
        k,
        k2: k,
        d: 0.0,
        d2: 0.0,
        two_g1: 0.0,
        fb: 0.0,
        trim,
    };
    match CLEAN_MODES[M as usize] {
        FilterMode::Lp24 | FilterMode::Bp24 | FilterMode::Hp24 | FilterMode::Hp12 => {
            // The second stage less resonant, so the cascade doesn't blow up.
            c.k2 = clean_k2(k);
            c.d = d_at(k);
            c.d2 = d_at(c.k2);
        }
        FilterMode::Phaser => {
            c.two_g1 = 2.0 * g / (1.0 + g);
            c.fb = notch2_fb(k);
        }
        FilterMode::Lp6 | FilterMode::Lp12 | FilterMode::Bp12 | FilterMode::Notch => c.d = d_at(k),
    }
    c
}

impl Clean {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block through LEVEL and the mode: the block's `clip_amount`
    /// for the LED.
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Clip {
        let r = self.prelude(buf, p, fs, mods);
        // LP4 builds its taps' shape behind the door, or for the closed
        // door's path: once a block either way.
        let tap = || TapShape::new(mods, tap_g_range(fs), clean_k);
        let lp4 = p.mode() == FilterMode::Lp24;
        let t = if lp4 { None } else { tap() };
        // The mode and the loop matched once a block: each is its own loop.
        match p.mode() {
            FilterMode::Lp6 => self.run::<0>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Lp12 => self.run::<1>(buf, r, t, mods.glide, mods.fb, fs),
            // Through its open top in bank 2 (#695); closed, the plain path,
            // a held block on the moving loop: the same bits as `steady`'s
            // (equal ends, `held`'s cascade `tick`'s arithmetic), and bank 1
            // keeps no held copy for a path only the closed door plays.
            FilterMode::Lp24 => {
                if !self.open_top(buf, r, mods, fs) {
                    self.closed_lp4(buf, r, tap(), mods.glide, mods.fb, fs)
                }
            }
            FilterMode::Bp12 => self.run::<3>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Bp24 => self.run::<4>(buf, r, t, mods.glide, mods.fb, fs),
            // DIRTY's HP2 isn't CLEAN's; were it handed one, HP4.
            FilterMode::Hp24 | FilterMode::Hp12 => {
                self.run::<5>(buf, r, t, mods.glide, mods.fb, fs)
            }
            FilterMode::Notch => self.run::<6>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Phaser => self.run::<7>(buf, r, t, mods.glide, mods.fb, fs),
        }
        self.settle()
    }

    /// A block's end: a NaN or infinite input poisons the states for good,
    /// so the next block starts from rest; LEVEL's clip.
    fn settle(&mut self) -> Clip {
        let ap: f32 = self.ap.iter().sum();
        if !(self.s1[0] + self.s1[1] + self.s2[0] + self.s2[1] + ap + self.w).is_finite() {
            self.rest();
        }
        self.level.clip()
    }

    /// This block's ramps, and LEVEL's stage run on `buf`.
    #[inline(always)]
    pub(super) fn prelude(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Ramps {
        let r = self.ramps(p, fs);
        self.level.run(buf, LevelRamp::new(r.pre, mods));
        r
    }

    /// LEVEL's clipper runs next block (`LevelStage::running`).
    pub fn level_running(&self) -> bool {
        self.level.running()
    }

    /// The raw states, for the bound tests: `[s1, s2]` of each stage, then
    /// NOTCH2's four all-passes and its feedback.
    #[cfg(feature = "test-support")]
    pub fn states(&self) -> [f32; 9] {
        let [a, b, c, d] = self.ap;
        [
            self.s1[0], self.s1[1], self.s2[0], self.s2[1], a, b, c, d, self.w,
        ]
    }

    /// This block's ramps, stored as the next block's start.
    #[inline(always)]
    pub(super) fn ramps(&mut self, p: &FilterParams, fs: u32) -> Ramps {
        let to = Ends {
            g: g_of(p.cutoff, fs as f32),
            k: clean_k(p.resonance),
            pre: level_gain(p.level),
        };
        let from = self.last.replace(to).unwrap_or(to);
        Ramps {
            g: (from.g, to.g),
            k: (from.k, to.k),
            pre: (from.pre, to.pre),
        }
    }

    #[inline(always)]
    fn run<const M: u8>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        t: Option<TapShape>,
        glide: bool,
        fb: Fed,
        fs: u32,
    ) {
        let still = t.is_none() && !fb.on();
        if still && r.g.0 == r.g.1 && r.k.0 == r.k.1 {
            self.steady::<M>(buf, r);
        } else if still && glide {
            self.stepped::<M>(buf, r);
        } else if !(fb.on() && self.fed_play(M, buf, r, t.as_ref(), fb, tap_g_range(fs))) {
            self.moving::<M>(buf, r, t.as_ref());
        }
        if !fb.on() {
            self.fbp = 0.0;
        }
    }

    /// LP4 with the door closed (#695): `run::<2>` less `steady`.
    #[inline(always)]
    fn closed_lp4(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        t: Option<TapShape>,
        glide: bool,
        fb: Fed,
        fs: u32,
    ) {
        if t.is_none() && !fb.on() && glide && (r.g.0 != r.g.1 || r.k.0 != r.k.1) {
            self.stepped::<2>(buf, r);
        } else if !(fb.on() && self.fed_play(2, buf, r, t.as_ref(), fb, tap_g_range(fs))) {
            self.moving::<2>(buf, r, t.as_ref());
        }
        if !fb.on() {
            self.fbp = 0.0;
        }
    }

    /// LP4's block through its open top, whole, in bank 2 (#695,
    /// `fed::OpenTop::call`). False while the door is closed: the plain
    /// path plays it.
    #[inline(never)]
    fn open_top(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        mods: &FilterMods,
        fs: u32,
    ) -> bool {
        super::fed::through(|d| {
            (d.open)(OpenCall {
                clean: self,
                buf,
                r,
                mods,
                fs,
            })
        })
        .is_some()
    }

    /// FB's block (ADR 0190): `moving`'s arrays, built here, and the loop
    /// in bank 2 (`fed::Clean::fed`). False while the door is closed.
    fn fed_play(
        &mut self,
        m: u8,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
        fb: Fed,
        gr: (f32, f32),
    ) -> bool {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let (mut g, mut k) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut g, &mut k, (r.g.0, g_step), (r.k.0, k_step), false);
        if let Some(t) = tap {
            t.apply(&mut g, &mut k);
        }
        let mode = CLEAN_MODES[usize::from(m.min(7))];
        let t = (trim(mode, r.k.0), trim(mode, r.k.1));
        super::fed::through(|d| (d.clean)(self, m, buf, &g, &k, t, fb, gr)).is_some()
    }

    /// `g` and `k` held: their divides once a block.
    #[inline(always)]
    pub(super) fn steady<const M: u8>(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps) {
        let c = coef::<M>(r.g.1, r.k.1, trim(CLEAN_MODES[M as usize], r.k.1));
        self.held::<M>(buf, &c);
    }

    /// `buf` through `tick` at `c`. A cascade's second stage runs a sample
    /// behind its first, two chains the FPU overlaps (#793): each value is
    /// `tick`'s arithmetic, so the same bits.
    #[inline(always)]
    fn held<const M: u8>(&mut self, buf: &mut [f32], c: &Coef) {
        let mode = CLEAN_MODES[M as usize];
        let Some((first, rest)) = buf.split_first_mut().filter(|_| cascade(mode)) else {
            for s in buf.iter_mut() {
                *s = self.tick::<M>(*s, c);
            }
            return;
        };
        let pick = |(lp, bp, hp): (f32, f32, f32)| match mode {
            FilterMode::Lp24 => lp,
            FilterMode::Bp24 => bp,
            _ => hp,
        };
        let mut a = pick(self.stage(*first, c.g, c.k, c.d, 0));
        let mut prev = first;
        for s in rest {
            let next = pick(self.stage(*s, c.g, c.k, c.d, 0));
            *prev = self.second::<M>(a, c);
            (a, prev) = (next, s);
        }
        *prev = self.second::<M>(a, c);
    }

    /// A cascade's second stage on its first's output, trimmed: `tick`'s.
    #[inline(always)]
    fn second<const M: u8>(&mut self, a: f32, c: &Coef) -> f32 {
        let (lp, bp, hp) = self.stage(a, c.g, c.k2, c.d2, 1);
        let y = match CLEAN_MODES[M as usize] {
            FilterMode::Lp24 => lp,
            FilterMode::Bp24 => bp * 2.0,
            _ => hp,
        };
        y * c.trim
    }

    /// A knob glide: `g`, `k` and the trim step `STEPS` times to this
    /// block's values, each sub-block the steady loop (ADR 0117).
    #[inline(never)]
    pub(super) fn stepped<const M: u8>(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps) {
        let mode = CLEAN_MODES[M as usize];
        let t = (trim(mode, r.k.0), trim(mode, r.k.1));
        for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let c = coef::<M>(step_to(r.g, j), step_to(r.k, j), step_to(t, j));
            self.held::<M>(sub, &c);
        }
    }

    /// `g` and `k` ramp to this block's values, reached on the last sample
    /// (#53), each sample's through the taps if any (`TapShape`), and every
    /// sample divides exactly. The trim stays on the ramp's `k`.
    #[inline(always)]
    pub(super) fn moving<const M: u8>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
    ) {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let (mut g, mut k) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut g, &mut k, (r.g.0, g_step), (r.k.0, k_step), false);
        if let Some(t) = tap {
            t.apply(&mut g, &mut k);
        }
        let mode = CLEAN_MODES[M as usize];
        let t = (trim(mode, r.k.0), trim(mode, r.k.1));
        let t_step = (t.1 - t.0) / n;
        for (i, s) in buf.iter_mut().enumerate() {
            let c = coef::<M>(g[i], k[i], g_at(t.0, t_step, i));
            *s = self.tick::<M>(*s, &c);
        }
    }

    /// Forget the last `g`, `k` and LEVEL: the next block starts unramped,
    /// and LEVEL's clipper where its gate is.
    pub fn hold(&mut self) {
        self.last = None;
        self.level = LevelStage::default();
    }

    /// A fresh voice's filter: at rest and unramped. Self-oscillation never
    /// decays, so a voice ended mid-ring would start its next note from it.
    pub fn reset(&mut self) {
        self.rest();
        self.hold();
    }

    fn rest(&mut self) {
        (self.s1, self.s2, self.ap, self.w, self.fbp) = ([0.0; 2], [0.0; 2], [0.0; 4], 0.0, 0.0);
    }

    /// Still ringing: a state above −80 dB.
    pub fn rings(&self) -> bool {
        let m = |a: f32, b: f32| a.abs().max(b.abs());
        let ap = self.ap.iter().fold(self.w.abs(), |a, &s| m(a, s));
        m(m(m(self.s1[0], self.s1[1]), m(self.s2[0], self.s2[1])), ap) > 1e-4
    }

    /// The last block's `g` (`None`: no ramp next block). Test-only: nothing
    /// in the audio path needs to read the ramp state back.
    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        self.last.map(|e| e.g)
    }

    /// `mode`'s outputs for a warm start (spec § 1), `(lp, bp)` of stage 0,
    /// read from its states; NOTCH2's from its first all-pass, a one-pole
    /// low-pass, and no `bp`.
    pub fn outputs(&self, mode: FilterMode) -> (f32, f32) {
        match mode {
            FilterMode::Phaser => (self.ap[0], 0.0),
            _ => (self.s2[0], self.s1[0]),
        }
    }

    /// A warm start: both stages' states from `lp` and `bp`.
    pub fn seed(&mut self, lp: f32, bp: f32) {
        (self.s1, self.s2) = ([bp; 2], [lp; 2]);
    }

    /// Entering NOTCH2: each all-pass at stage 0's `lp`, its DC steady
    /// state (spec § 1). The feedback starts from rest: seeding it at its
    /// own DC state, `lp/(1 − fb)`, lands 4.5 % off at 1 kHz against 0.6.
    pub fn seed_notch2(&mut self) {
        self.ap = [self.s2[0]; 4];
    }
    /// A MODE change's warm start from `from` to `to`: stage 0 runs alike
    /// in every SVF mode, so only what `to` reads beyond it starts anew
    /// (`enter`); leaving NOTCH2, both stages start from its `lp`.
    pub(super) fn shift(&mut self, from: FilterMode, to: FilterMode) {
        if from == FilterMode::Phaser {
            self.seed(self.ap[0], 0.0);
        }
        self.enter(to);
    }

    /// Entering `mode` with stage 0 warm: a cascade's stage 2 at its own
    /// input's DC steady state (`lp` and `bp` of stage 0's `lp`, `bp` or
    /// `hp`, the last near 0 at DC), NOTCH2's all-passes at `lp`.
    pub(super) fn enter(&mut self, mode: FilterMode) {
        match mode {
            FilterMode::Lp24 => self.set_stage2([self.s1[0], self.s2[0]]),
            FilterMode::Bp24 => self.set_stage2([0.0, self.s1[0]]),
            FilterMode::Hp24 | FilterMode::Hp12 => self.set_stage2([0.0; 2]),
            FilterMode::Phaser => self.seed_notch2(),
            FilterMode::Lp6 | FilterMode::Lp12 | FilterMode::Bp12 | FilterMode::Notch => {}
        }
    }

    /// Stage 2's states: what a cascade-to-cascade crossfade carries for
    /// the old mode, whose stage 2 runs on another input.
    pub(super) fn stage2(&self) -> [f32; 2] {
        [self.s1[1], self.s2[1]]
    }

    pub(super) fn set_stage2(&mut self, [a, b]: [f32; 2]) {
        (self.s1[1], self.s2[1]) = (a, b);
    }

    /// After a crossfade block: the states `mode` leaves alone take the old
    /// mode's run (`old`), so it runs on from them next block.
    pub(super) fn adopt(&mut self, old: &Clean, mode: FilterMode) {
        if mode == FilterMode::Phaser {
            (self.s1, self.s2) = (old.s1, old.s2);
            return;
        }
        (self.ap, self.w) = (old.ap, old.w);
        if !cascade(mode) {
            self.set_stage2(old.stage2());
        }
    }

    /// The states `mode` doesn't use zeroed (spec § 1: clean the next time
    /// they are entered).
    pub(super) fn zero_unused(&mut self, mode: FilterMode) {
        if mode == FilterMode::Phaser {
            (self.s1, self.s2) = ([0.0; 2], [0.0; 2]);
            return;
        }
        (self.ap, self.w) = ([0.0; 4], 0.0);
        if !cascade(mode) {
            self.set_stage2([0.0; 2]);
        }
    }

    /// One sample, trimmed (`trim`).
    #[inline(always)]
    fn tick<const M: u8>(&mut self, x: f32, c: &Coef) -> f32 {
        self.untrimmed::<M>(x, c) * c.trim
    }

    #[inline(always)]
    fn untrimmed<const M: u8>(&mut self, x: f32, c: &Coef) -> f32 {
        match CLEAN_MODES[M as usize] {
            // LP1: `(1 + s)/(s² + ks + 1)`, the one-pole `1/(1 + s)` at
            // `k = 2`; Q peaks it by `√2/k` at FREQ.
            FilterMode::Lp6 => {
                let (lp, bp, _) = self.stage(x, c.g, c.k, c.d, 0);
                lp + bp
            }
            FilterMode::Lp12 => self.stage(x, c.g, c.k, c.d, 0).0,
            FilterMode::Lp24 => {
                let lp = self.stage(x, c.g, c.k, c.d, 0).0;
                self.stage(lp, c.g, c.k2, c.d2, 1).0
            }
            // BP boosted for presence.
            FilterMode::Bp12 => self.stage(x, c.g, c.k, c.d, 0).1 * 2.0,
            FilterMode::Bp24 => {
                let bp = self.stage(x, c.g, c.k, c.d, 0).1;
                self.stage(bp, c.g, c.k2, c.d2, 1).1 * 2.0
            }
            FilterMode::Hp24 | FilterMode::Hp12 => {
                let hp = self.stage(x, c.g, c.k, c.d, 0).2;
                self.stage(hp, c.g, c.k2, c.d2, 1).2
            }
            // NOTCH1, the Cascadia's NT2: `(1 + s²/4)/(s² + ks + 1)`, a zero
            // at 2 × FREQ, the treble −12 dB.
            FilterMode::Notch => {
                let (lp, _, hp) = self.stage(x, c.g, c.k, c.d, 0);
                lp + 0.25 * hp
            }
            // NOTCH2, the Cascadia's PHZR (the SSM2040 data sheet's Fig. 10,
            // with Q as feedback): dry plus four all-passes, `(1 + A)·u`,
            // zero where A turns −180° and −540°: 0.414 × and 2.414 × FREQ.
            // The dry is `u`, after the feedback node, so the notches stay
            // exact at every Q; `w` is a sample late, so no loop to solve.
            FilterMode::Phaser => {
                let u = x + c.fb * self.w;
                let mut v = u;
                for s in &mut self.ap {
                    // The TPT one-pole: `t = (v − s)·G₁`, `lp = s + t`, and
                    // the new state `s′ = s + 2t`. Its all-pass `2·lp − v`
                    // is `s′ − (v − s)`: four operations, not six.
                    let e = v - *s;
                    *s += e * c.two_g1;
                    v = *s - e;
                }
                self.w = v;
                u + v
            }
        }
    }

    /// One SVF stage, `d = 1/(1 + kg + g²)` given: `(lp, bp, hp)`.
    #[inline(always)]
    fn stage(&mut self, x: f32, g: f32, k: f32, d: f32, i: usize) -> (f32, f32, f32) {
        let v1 = sat_c1(self.s1[i]);
        let v2 = sat_c1(self.s2[i]);
        let hp = (x - (k + g) * v1 - v2) * d;
        let bp = g * hp + v1;
        let lp = g * bp + v2;
        self.s1[i] = bp + g * hp;
        self.s2[i] = lp + g * bp;
        (lp, bp, hp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::SAMPLE_RATE;

    /// The moving loop on a steady block is the steady loop: the same
    /// coefficients, per sample instead of once.
    #[test]
    fn steady_and_moving_agree_on_a_steady_block() {
        let mut p = FilterParams::default();
        (p.cutoff, p.resonance) = (1500.0, 0.6);
        let saw = |b: usize| -> [f32; BLOCK_SIZE] {
            core::array::from_fn(|i| 0.8 * (((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0))
        };
        let mut a = Clean::new();
        for b in 0..3 {
            a.process(&mut saw(b), &p, SAMPLE_RATE, &FilterMods::NONE);
        }
        let mut m = a.clone();
        let (mut s, mut v) = (saw(3), saw(3));
        let r = a.ramps(&p, SAMPLE_RATE);
        a.steady::<2>(&mut s, r);
        let r = m.ramps(&p, SAMPLE_RATE);
        m.moving::<2>(&mut v, r, None);
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.01);
        for (i, (a, b)) in s.iter().zip(&v).enumerate() {
            assert!((a - b).abs() <= 1e-6 * peak, "sample {i}: {a} vs {b}");
        }
    }
    /// LP4 with the door closed plays `run::<2>`'s bits (#695): a held
    /// block on the moving loop, glided or not, moving and RES 1 blocks.
    #[test]
    fn closed_lp4_is_run_to_the_bit() {
        let saw = |b: usize| -> [f32; BLOCK_SIZE] {
            core::array::from_fn(|i| 0.8 * (((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0))
        };
        let mut p = FilterParams::default();
        for res in [0.0, 0.6, 1.0] {
            for glide in [false, true] {
                let (mut a, mut b) = (Clean::new(), Clean::new());
                for blk in 0..12 {
                    // Held, then a jump, then held again.
                    (p.cutoff, p.resonance) = (
                        if (4..8).contains(&blk) {
                            6_000.0
                        } else {
                            700.0
                        },
                        res,
                    );
                    let (mut x, mut y) = (saw(blk), saw(blk));
                    let r = a.ramps(&p, SAMPLE_RATE);
                    a.run::<2>(&mut x, r, None, glide, Fed::OFF, SAMPLE_RATE);
                    let r = b.ramps(&p, SAMPLE_RATE);
                    b.closed_lp4(&mut y, r, None, glide, Fed::OFF, SAMPLE_RATE);
                    let bits = |v: [f32; BLOCK_SIZE]| v.map(f32::to_bits);
                    assert_eq!(bits(x), bits(y), "RES {res} glide {glide} block {blk}");
                }
            }
        }
    }

    /// `held`'s cascades a sample apart are `tick` a sample at a time, to
    /// the bit, states too: driven hard, so `sat_c1` bends, and at self-
    /// oscillation.
    #[test]
    fn held_is_tick_to_the_bit() {
        fn check<const M: u8>() {
            for (g, k) in [(0.05, 1.4), (0.7, 0.2), (2.5, -0.1)] {
                let c = coef::<M>(g, k, trim(CLEAN_MODES[M as usize], k));
                let mut x: [f32; BLOCK_SIZE] =
                    core::array::from_fn(|i| 3.0 * (((i * 37) % 23) as f32 / 11.0 - 1.0));
                let (mut a, mut b) = (Clean::new(), Clean::new());
                let mut y = x;
                for _ in 0..4 {
                    a.held::<M>(&mut x, &c);
                    for s in y.iter_mut() {
                        *s = b.tick::<M>(*s, &c);
                    }
                    let bits = |v: [f32; BLOCK_SIZE]| v.map(f32::to_bits);
                    assert_eq!(bits(x), bits(y), "mode {M} g {g} k {k}");
                    let st = |c: &Clean| {
                        [
                            c.s1[0], c.s1[1], c.s2[0], c.s2[1], c.ap[0], c.ap[1], c.ap[2], c.ap[3],
                            c.w,
                        ]
                        .map(f32::to_bits)
                    };
                    assert_eq!(st(&a), st(&b));
                }
            }
        }
        check::<0>();
        check::<1>();
        check::<2>();
        check::<3>();
        check::<4>();
        check::<5>();
        check::<6>();
        check::<7>();
    }
}

/// FB's loop (ADR 0190), linked into bank 2 with `bank2` (`filter::fed`).
mod fed {
    use super::super::{Fed, g_at};
    use super::{Clean, coef};
    use crate::hw::BLOCK_SIZE;

    impl Clean {
        /// `moving`'s loop on `g` and `k`, each sample's `g` through the FB
        /// signal; one loop for every mode, `m` read a sample (FB's bill
        /// covers it), the trim ramped across `t`.
        #[inline(never)]
        #[allow(clippy::too_many_arguments)]
        pub(in crate::dsp::filter) fn fed(
            &mut self,
            m: u8,
            buf: &mut [f32; BLOCK_SIZE],
            g: &[f32; BLOCK_SIZE],
            k: &[f32; BLOCK_SIZE],
            t: (f32, f32),
            fb: Fed,
            gr: (f32, f32),
        ) {
            let t_step = (t.1 - t.0) / BLOCK_SIZE as f32;
            for (i, s) in buf.iter_mut().enumerate() {
                let gi = fb.g(g[i], i, self.fbp, gr);
                let tr = g_at(t.0, t_step, i);
                *s = match m {
                    0 => self.tick::<0>(*s, &coef::<0>(gi, k[i], tr)),
                    1 => self.tick::<1>(*s, &coef::<1>(gi, k[i], tr)),
                    2 => self.tick::<2>(*s, &coef::<2>(gi, k[i], tr)),
                    3 => self.tick::<3>(*s, &coef::<3>(gi, k[i], tr)),
                    4 => self.tick::<4>(*s, &coef::<4>(gi, k[i], tr)),
                    5 => self.tick::<5>(*s, &coef::<5>(gi, k[i], tr)),
                    6 => self.tick::<6>(*s, &coef::<6>(gi, k[i], tr)),
                    _ => self.tick::<7>(*s, &coef::<7>(gi, k[i], tr)),
                };
                fb.feed(&mut self.fbp, *s);
            }
        }
    }
}

/// LP4's open top (#695), bank 2's through `FedDoor::open`: its own
/// module, so it links in an S4 group (`clean::open`), not FB's in S0-S3.
mod open {
    use super::super::fed::OpenCall;
    use super::super::{Fed, TapShape, g_at, ramps, tap_g_range};
    use super::{CLEAN_MODES, Clean, OpenTop, Ramps, clean_k, coef, trim};
    use crate::hw::BLOCK_SIZE;

    impl OpenTop {
        /// The door's one entry (#695): LP4's block on the raw ramps
        /// `c.r`. A block whose `g` moves per sample (a tap, FB, or a
        /// sweep that reaches the band) lifts, damps and weighs each
        /// sample's `g` after the taps and FB (`moved`); a held or glided
        /// one its ends, the dry mixed by their weights.
        #[inline(never)]
        pub(in crate::dsp::filter) fn call(c: OpenCall<'_, '_>) {
            let OpenCall {
                clean,
                buf,
                r,
                mods,
                fs,
            } = c;
            let top = OpenTop::for_rate(fs);
            let gr = tap_g_range(fs);
            let t = TapShape::new(mods, gr, clean_k);
            let raw = r.g;
            let sweeps = raw.0 != raw.1 && raw.0.max(raw.1) > top.lo;
            if t.is_some() || mods.fb.on() || sweeps {
                clean.moved(&top, buf, r, t.as_ref(), mods.fb, gr);
                return;
            }
            let (u0, u1) = (top.u(raw.0), top.u(raw.1));
            let r = Ramps {
                g: (top.lifted(raw.0), top.lifted(raw.1)),
                k: (OpenTop::damped(r.k.0, u0), OpenTop::damped(r.k.1, u1)),
                ..r
            };
            let w = (top.weight(r.g.0), top.weight(r.g.1));
            if w == (0.0, 0.0) {
                clean.run::<2>(buf, r, None, mods.glide, Fed::OFF, fs);
            } else {
                let dry = *buf;
                clean.run::<2>(buf, r, None, mods.glide, Fed::OFF, fs);
                Clean::open(&dry, buf, w);
            }
        }
    }

    impl Clean {
        /// LP4's moving block through its open top (#695): each sample's
        /// `g` after the taps and FB lifted, its `k` damped and its dry
        /// weighed, so modulation and sweeps reach the top (#1135's
        /// review, I1). Under the band it is `moving::<2>`'s, or FB's
        /// `fed`, bit for bit.
        fn moved(
            &mut self,
            top: &OpenTop,
            buf: &mut [f32; BLOCK_SIZE],
            r: Ramps,
            tap: Option<&TapShape>,
            fb: Fed,
            gr: (f32, f32),
        ) {
            let n = BLOCK_SIZE as f32;
            let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
            let (mut g, mut k) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
            ramps(&mut g, &mut k, (r.g.0, g_step), (r.k.0, k_step), false);
            if let Some(t) = tap {
                t.apply(&mut g, &mut k);
            }
            let mode = CLEAN_MODES[2];
            let tr = (trim(mode, r.k.0), trim(mode, r.k.1));
            let t_step = (tr.1 - tr.0) / n;
            let on = fb.on();
            for (i, s) in buf.iter_mut().enumerate() {
                let gi = if on {
                    fb.g(g[i], i, self.fbp, gr)
                } else {
                    g[i]
                };
                // A tap or FB moving `g` up keeps the band's lift (the ramp's
                // place in it), toward the dry; moving it down takes the band
                // back as CUTOFF would, so it reaches as deep as under it.
                let gb = g_at(r.g.0, g_step, i);
                let u = top.u(gb.min(gi));
                let x = *s;
                *s = if u <= 0.0 {
                    self.tick::<2>(x, &coef::<2>(gi, k[i], g_at(tr.0, t_step, i)))
                } else {
                    let (gl, ki) = (top.lifted_at(gi, u), OpenTop::damped(k[i], u));
                    let w = top.weight(gl);
                    // The trim is the ramp's, as under the band, and moves
                    // only by the band's damping of the ramp's `k`: 0 at the
                    // band's edge, so a tap's RES crossing it never steps
                    // the gain (#1135's re-review).
                    let kr = g_at(r.k.0, k_step, i);
                    let tri = g_at(tr.0, t_step, i)
                        + (trim(mode, OpenTop::damped(kr, u)) - trim(mode, kr));
                    self.tick::<2>(x, &coef::<2>(gl, ki, tri)) * (1.0 - w) + x * w
                };
                if on {
                    fb.feed(&mut self.fbp, *s);
                }
            }
            if !on {
                self.fbp = 0.0;
            }
        }

        /// LP4 near the top (#695): `wet·(1 − w) + dry·w`, `w` ramped
        /// across the block as `g` is, landing on `w.1` on its last
        /// sample; at 1, the dry exactly.
        #[inline(always)]
        fn open(dry: &[f32; BLOCK_SIZE], wet: &mut [f32; BLOCK_SIZE], w: (f32, f32)) {
            let step = (w.1 - w.0) / BLOCK_SIZE as f32;
            for (i, (y, &x)) in wet.iter_mut().zip(dry).enumerate() {
                let wi = g_at(w.0, step, i);
                *y = *y * (1.0 - wi) + x * wi;
            }
        }
    }
}
