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
        FilterMode::Phaser => return 0.5 * libm::sqrtf(1.0 - notch2_fb(k)),
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
        let t = TapShape::new(mods, tap_g_range(fs), clean_k);
        // The mode and the loop matched once a block: each is its own loop.
        match p.mode() {
            FilterMode::Lp6 => self.run::<0>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Lp12 => self.run::<1>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Lp24 => self.run::<2>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Bp12 => self.run::<3>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Bp24 => self.run::<4>(buf, r, t, mods.glide, mods.fb, fs),
            // DIRTY's HP2 isn't CLEAN's; were it handed one, HP4.
            FilterMode::Hp24 | FilterMode::Hp12 => {
                self.run::<5>(buf, r, t, mods.glide, mods.fb, fs)
            }
            FilterMode::Notch => self.run::<6>(buf, r, t, mods.glide, mods.fb, fs),
            FilterMode::Phaser => self.run::<7>(buf, r, t, mods.glide, mods.fb, fs),
        }
        // A NaN or infinite input poisons the states for good: start the
        // next block from rest.
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
        for s in buf.iter_mut() {
            *s = self.tick::<M>(*s, &c);
        }
    }

    /// A knob glide: `g`, `k` and the trim step `STEPS` times to this
    /// block's values, each sub-block the steady loop (ADR 0117).
    #[inline(never)]
    pub(super) fn stepped<const M: u8>(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps) {
        let mode = CLEAN_MODES[M as usize];
        let t = (trim(mode, r.k.0), trim(mode, r.k.1));
        for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let c = coef::<M>(step_to(r.g, j), step_to(r.k, j), step_to(t, j));
            for s in sub {
                *s = self.tick::<M>(*s, &c);
            }
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
