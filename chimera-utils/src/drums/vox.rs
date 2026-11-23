// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! VOX (0.4 prototype, second cut, owner to judge by ear): beatboxed
//! drums built consonant first. A beatboxed drum is a pressure event and
//! a noise event, a few milliseconds of each, with at most a breath of
//! vowel after; so each machine models the mouth's action, not a voice:
//! the release's pressure pulse, its burst, the fricative that follows,
//! the cavity a click rings, from VA NOISE, CLEAN's filters, DRUM's
//! two-pole modes, VA's pulse oscillator and the FX slot's SOFT. No
//! formant operators; TRACT (the tongue's and lips' place, the vowel's
//! colour) moves the resonances and takes a modulation offset per hit.
//! Everything moves a sample at a time; nothing steps per block.
//!
//! Phonetics and acoustics used as data (figures, never code):
//! - Proctor, Bresch, Byrd, Nayak & Narayanan 2013, "Paralinguistic
//!   mechanisms of production in human beatboxing: a real-time MRI study",
//!   JASA 133(2) 1043: the kick is a bilabial ejective [p'] (glottis shut,
//!   larynx pumped: a pressure pop, no voice), the hats alveolar ejectives
//!   [t'], the snares labial/dorsal, the inward K a pulmonic ingressive
//!   (an inhale through a velar/lateral closure).
//! - Flanagan 1972, *Speech Analysis, Synthesis and Perception*, ch. 3;
//!   Stevens 1998, *Acoustic Phonetics*, ch. 3: the lips radiate the
//!   derivative of the volume flow, so a release's flow pulse is heard as
//!   a positive lobe and a shallow negative one.
//! - Blumstein & Stevens 1979, JASA 66(4) 1001; Stevens 1998 ch. 7:
//!   burst spectra: labial diffuse-falling (energy low, under ~1.5–2 kHz),
//!   alveolar diffuse-rising (peak ≥ ~3.5–4 kHz), velar compact (one
//!   peak ~1.5–3 kHz that follows the next vowel's F2), velars often with
//!   multiple bursts; bursts last ~5–20 ms, velar longest.
//! - Jongman, Wayland & Wong 2000, JASA 108(3) 1252; Hughes & Halle 1956,
//!   JASA 28(2) 303: fricative spectra: /s/ peaked ~6–8 kHz with little
//!   under ~3.5 kHz, /ʃ/ peaked lower (~2.5–4 kHz), /f/ flat and diffuse
//!   (~1.5–8 kHz+) and much weaker than the sibilants.
//! - Howell & Rosen 1983, JASA 73(3) 976: affricate rise times short
//!   (~30 ms), fricatives' long (~100+ ms): "ts"/"ch" snap, "sss" swells.
//! - Fujimura 1962, JASA 34(12) 1865: the /m/ murmur: first nasal
//!   resonance ~250–300 Hz, little energy above ~500 Hz.
//! - Ladefoged & Traill 1994, J. Phonetics 22, 33; Ladefoged & Maddieson
//!   1996, *The Sounds of the World's Languages*, ch. 8: clicks are
//!   very short, loud transients ringing the cavity between the tongue's
//!   two closures; the alveolar click lower and hollow (~1–2 kHz), the
//!   palatal sharper and higher (~2.5–3.5 kHz). Ch. 7: bilabial trills
//!   run ~20–35 Hz.

use super::machines::{Id, Knobs, Machine, drive, spec};
use super::parts::*;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::{fast_sin_turns, fast_tanh};

// --------------------------------------------------------------- parts

/// A gesture's level a sample: silent for `delay`, a linear rise over
/// `att`, a `hold`, then an exponential fall (`t60`).
#[derive(Clone, Copy)]
struct Gesture {
    wait: u32,
    rise: f32,
    inc: f32,
    hold: u32,
    fall: Decay,
}

impl Gesture {
    fn new(delay: f32, att: f32, hold: f32, t60: f32) -> Self {
        Self {
            wait: (delay * FS) as u32,
            rise: if att <= 0.0 { 1.0 } else { 0.0 },
            inc: 1.0 / (att * FS).max(1.0),
            hold: (hold * FS) as u32,
            fall: Decay::t60(t60),
        }
    }

    /// Still sounding (or yet to).
    fn live(&self) -> bool {
        self.rise < 1.0 || self.hold > 0 || self.fall.v > 1e-5
    }

    #[inline]
    fn next(&mut self) -> f32 {
        if self.wait > 0 {
            self.wait -= 1;
            0.0
        } else if self.rise < 1.0 {
            self.rise = (self.rise + self.inc).min(1.0);
            self.rise
        } else if self.hold > 0 {
            self.hold -= 1;
            1.0
        } else {
            self.fall.next()
        }
    }
}

/// A release's radiated pressure: flow u = x² e^(−x) (x = t/τ, a smooth
/// onset), heard as du/dx = x(2 − x) e^(−x): a positive lobe 2τ long,
/// then a shallow negative one of the same area (Flanagan 1972 ch. 3).
/// Peak 1.
#[derive(Clone, Copy)]
struct Puff {
    x: f32,
    dx: f32,
    e: f32,
    k: f32,
}

/// The peak of x(2 − x)e^(−x), at x = 2 − √2.
const PUFF_PEAK: f32 = 0.461_2;

impl Puff {
    /// The first lobe `f`'s half cycle: τ = 1/(4f).
    fn at(f: f32) -> Self {
        Self::tau(0.25 / f)
    }

    fn tau(tau: f32) -> Self {
        let dx = 1.0 / (tau * FS);
        Self {
            x: 0.0,
            dx,
            e: 1.0,
            k: (-dx).exp(),
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        if self.x > 30.0 {
            return 0.0;
        }
        let v = self.x * (2.0 - self.x) * self.e * (1.0 / PUFF_PEAK);
        self.x += self.dx;
        self.e *= self.k;
        v
    }
}

/// Turbulence's slow wobble: white through two one-poles at ~35 Hz, as a
/// gain 1 ± about `depth`·0.3.
struct Flutter {
    a: f32,
    b: f32,
    rng: Lcg,
    depth: f32,
}

impl Flutter {
    const C: f32 = core::f32::consts::TAU * 35.0 / FS;

    fn new(seed: u32, depth: f32) -> Self {
        Self {
            a: 0.0,
            b: 0.0,
            rng: Lcg(seed.wrapping_mul(2_654_435_761) ^ 0x0f17),
            depth,
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        self.a += Self::C * (self.rng.next() - self.a);
        self.b += Self::C * (self.a - self.b);
        (1.0 + self.depth * 14.0 * self.b).max(0.0)
    }
}

/// White noise's power through a two-pole mode at `f` Hz and `t60` with
/// DRUM's input gain (sin w): its RMS gain, so a band can be set by level.
fn white_gain(f: f32, t60: f32) -> f32 {
    let w = core::f32::consts::TAU * f / FS;
    let r = per_sample(t60);
    let (r2, c) = (r * r, w.cos());
    let p = w.sin().powi(2) * (1.0 + r2) / ((1.0 - r2) * ((1.0 + r2).powi(2) - 4.0 * r2 * c * c));
    p.sqrt()
}

/// Two-pole modes at absolute `hz`, each `t60` long. `white`: each mode's
/// `amp` is its RMS for unit white noise in (a fricative's band);
/// otherwise its peak for a unit impulse (a struck cavity).
fn modes<const N: usize>(hz: [f32; N], t60: [f32; N], amp: [f32; N], white: bool) -> Modes<N> {
    let mut m = Modes::<N>::new();
    m.ratio = hz;
    m.t60 = t60;
    m.amp = core::array::from_fn(|i| {
        if white {
            amp[i] / white_gain(hz[i], t60[i])
        } else {
            amp[i]
        }
    });
    m.set(1.0);
    m
}

/// The FX slot's SOFT (TONE ½, unity LEVEL) at a fixed DRIVE: its
/// pre-gain and output gain worked out once, the curve a sample.
#[derive(Clone, Copy)]
struct Soft {
    pre: f32,
    post: f32,
}

impl Soft {
    fn new(d: f32) -> Self {
        Self {
            pre: 1.0 + 8.0 * d,
            // The curve's ceiling: its unity LEVEL's gain.
            post: 0.9 * drive(1.0e3, 0.0),
        }
    }

    #[inline]
    fn tick(self, x: f32) -> f32 {
        self.post * fast_tanh(self.pre * x)
    }
}

// ---------------------------------------------------------------- kick

/// The lips flapping: a flow pulse (`Puff` of `tau`) every period, the
/// period and sizes jittered, each flap `shrink` times the last (1 holds);
/// the last flap's tail sounds under the next.
struct Flaps {
    period: f32,
    left: f32,
    p: [(Puff, f32); 2],
    cur: usize,
    tau: f32,
    jitter: f32,
    rng: Lcg,
    size: f32,
    shrink: f32,
}

impl Flaps {
    /// Flapping at `f` Hz, the first flap `delay` seconds in.
    fn new(f: f32, tau: f32, jitter: f32, shrink: f32, delay: f32, seed: u32) -> Self {
        let mut p = Puff::tau(tau);
        p.x = 31.0;
        Self {
            period: FS / f,
            left: delay * FS,
            p: [(p, 0.0); 2],
            cur: 0,
            tau,
            jitter,
            rng: Lcg(seed.wrapping_mul(747_796_405) | 1),
            size: 1.0,
            shrink,
        }
    }

    /// The pulse, and whether a flap opened on this sample.
    #[inline]
    fn next(&mut self) -> (f32, bool) {
        let mut open = false;
        if self.left <= 0.0 {
            self.cur ^= 1;
            let s = self.size * (1.0 + 0.3 * self.jitter * self.rng.next());
            self.p[self.cur] = (Puff::tau(self.tau), s);
            self.size *= self.shrink;
            self.left += self.period * (1.0 + 0.15 * self.jitter * self.rng.next());
            open = true;
        }
        self.left -= 1.0;
        let [a, b] = &mut self.p;
        (a.1 * a.0.next() + b.1 * b.0.next(), open)
    }
}

/// The mic's boom of a pop: a sine falling a fifth onto `f0` (linear in
/// Hz, a sample at a time) under an exponential decay.
struct Boom {
    ph: f32,
    f0: f32,
    drop: Decay,
    amp: Decay,
}

impl Boom {
    #[inline]
    fn tick(&mut self) -> f32 {
        self.ph += self.f0 * (1.0 + 0.5 * self.drop.next()) * (1.0 / FS);
        if self.ph >= 1.0 {
            self.ph -= 1.0;
        }
        fast_sin_turns(self.ph) * self.amp.next()
    }
}

/// VOX KICK, "B" / "Bm": lips shut, the larynx pumps the pressure up
/// (Proctor 2013's [p']), the lips part: a low pressure pop (the flow
/// pulse's derivative, its first lobe PITCH's half cycle) and the mic's
/// boom of it, a short labial burst low-passed at TRACT (Blumstein &
/// Stevens: diffuse-falling), LIPS' few flaps of buzz after the release,
/// and an optional "m" hum: a voiced pulse train through the nasal
/// murmur's ~250 Hz resonance (Fujimura 1962).
pub struct Kick {
    air: Hiss,
    puff: Puff,
    boom: Boom,
    burst: Gesture,
    burst_lp: (Flt, f32),
    buzz: Flaps,
    hum: Option<(Pulse, f32, Gesture, Decay, Modes<2>)>,
    /// Pop, boom, burst, lips, hum.
    lv: [f32; 5],
    drv: Soft,
    vel: f32,
}

impl Kick {
    pub fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let f = expo(k[0], 45.0, 100.0);
        let tract = expo(k[6], 700.0, 2500.0);
        let hum = (k[5] > 0.0).then(|| {
            (
                Pulse::new(0.6),
                (1.8 * f).clamp(85.0, 170.0),
                // The "m" after the pop: lips close again, voice starts.
                Gesture::new(0.012, 0.02, 0.0, expo(k[1], 0.12, 1.2)),
                Decay::tau(0.25),
                modes(
                    [250.0, 0.85 * tract + 900.0],
                    [0.025, 0.008],
                    [1.0, 0.12],
                    false,
                ),
            )
        });
        let bz = (1.3 * f).clamp(60.0, 110.0);
        Self {
            air: Hiss::new(seed),
            puff: Puff::at(f),
            boom: Boom {
                ph: 0.0,
                f0: f,
                drop: Decay::tau(0.012),
                amp: Decay::t60(expo(k[1], 0.05, 0.6)),
            },
            burst: Gesture::new(0.0, 0.0002, 0.0, 0.012),
            burst_lp: (Flt::clean(FilterMode::Lp12, tract, 0.15), tract),
            buzz: Flaps::new(bz, 0.07 / bz, 0.3, lin(k[3], 0.3, 0.75), 1.0 / bz, seed),
            hum,
            lv: [1.0 * k[2], 0.8 * k[2], 1.4 * k[4], 0.5 * k[3], 0.04 * k[5]],
            drv: Soft::new(0.6 * k[7] * k[7]),
            vel,
        }
    }
}

impl Machine for Kick {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let mut b = [0.0; B];
        if self.burst.live() {
            b = self.air.block();
            for v in b.iter_mut() {
                *v *= self.burst.next();
            }
            self.burst_lp.0.run(&mut b, self.burst_lp.1);
        }
        let lv = self.lv;
        for (i, o) in out.iter_mut().enumerate() {
            let mut x = lv[0] * self.puff.next()
                + lv[1] * self.boom.tick()
                + lv[2] * b[i]
                + lv[3] * self.buzz.next().0;
            if let Some((p, hz, env, droop, nasal)) = &mut self.hum {
                // Pitch sags a semitone as the breath runs out.
                let h = p.tick(*hz * (0.944 + 0.056 * droop.next()));
                x += lv[4] * nasal.tick(h * env.next());
            }
            *o = self.vel * self.drv.tick(0.6 * x);
        }
    }
}

// --------------------------------------------------------------- snare

/// VOX SNARE, "Pf" / "Psh" / inward K. Outward: a labial burst and a small
/// pop, then the fricative right behind it: "f" flat (~1.5–9 kHz, TRACT
/// low) to "sh" peaked ~2.7–3.6 kHz (TRACT high), turbulence wobbling it.
/// INHALE turns it inward: the burst becomes a sharp velar click (the
/// tongue's back pulled off the velum) and the noise an in-breath that
/// swells, broader and airier.
pub struct Snare {
    air: Hiss,
    flutter: Flutter,
    burst: Gesture,
    burst_lp: (Flt, f32),
    puff: Puff,
    click: Modes<2>,
    struck: bool,
    fric: Gesture,
    hp: (Flt, f32),
    lp: (Flt, f32),
    peak: Modes<2>,
    /// The flat and peaked shares of the fricative.
    mix: (f32, f32),
    /// Burst, pop, click, hiss.
    lv: [f32; 4],
    vel: f32,
}

impl Snare {
    pub fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let s = expo(k[7], 0.8, 1.25);
        let (tr, inh) = (k[3], k[6]);
        let pk = lin(inh, expo(tr, 3600.0, 2700.0), 3800.0) * s;
        let q = lin(inh, 0.0022, 0.0012);
        Self {
            air: Hiss::new(seed),
            flutter: Flutter::new(seed, k[5] * (0.5 + 0.6 * inh)),
            burst: Gesture::new(0.0, 0.0002, 0.0, 0.012),
            burst_lp: (Flt::clean(FilterMode::Lp12, 2600.0 * s, 0.1), 2600.0 * s),
            puff: Puff::at(110.0 * s),
            click: modes([1800.0 * s, 3100.0 * s], [0.014, 0.008], [1.0, 0.5], false),
            struck: false,
            fric: Gesture::new(
                lin(inh, 0.004, 0.003),
                lin(inh, 0.007, 0.03),
                lin(k[2], 0.0, 0.06),
                expo(k[2], 0.05, 0.4),
            ),
            hp: (
                Flt::clean(FilterMode::Hp24, 1400.0 * s, 0.0),
                lin(tr.max(inh), 1400.0, 2000.0) * s,
            ),
            lp: (Flt::clean(FilterMode::Lp12, 9500.0 * s, 0.0), 9500.0 * s),
            peak: modes([pk, 1.6 * pk], [q, 0.8 * q], [1.0, 0.35], true),
            mix: (
                (1.0 - tr) * (1.0 - 0.5 * inh) + 0.4 * inh,
                tr * (1.0 - inh) + 0.5 * inh,
            ),
            lv: [
                1.3 * k[0] * (1.0 - inh),
                0.7 * k[4] * (1.0 - inh),
                0.8 * k[0] * inh,
                k[1] * (0.9 + 0.3 * tr),
            ],
            vel,
        }
    }
}

impl Machine for Snare {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let n = self.air.block();
        let mut b: Blk = core::array::from_fn(|i| n[i] * self.burst.next());
        self.burst_lp.0.run(&mut b, self.burst_lp.1);
        let mut f: Blk = core::array::from_fn(|i| n[i] * self.fric.next() * self.flutter.next());
        self.hp.0.run(&mut f, self.hp.1);
        let r: Blk = core::array::from_fn(|i| self.peak.tick(f[i]));
        self.lp.0.run(&mut f, self.lp.1);
        let lv = self.lv;
        for (i, o) in out.iter_mut().enumerate() {
            let x = if self.struck { 0.0 } else { 1.0 };
            self.struck = true;
            let s = lv[0] * b[i]
                + lv[1] * self.puff.next()
                + lv[2] * self.click.tick(x)
                + lv[3] * (self.mix.0 * f[i] + self.mix.1 * r[i]);
            *o = self.vel * Soft::new(0.03).tick(0.75 * s);
        }
    }
}

// ----------------------------------------------------------------- hat

/// VOX HAT, "T" / "Ts" / "Ch" / open "tsss": the tongue tip's release, an
/// alveolar burst (noise over ~2.8 kHz, peaked ~4.2 kHz: diffuse-rising)
/// with a small pop, then the sibilant: noise through the front cavity's
/// peak, ~7 kHz for "s" (TRACT low) to ~3.6 kHz for "sh"/"ch" (TRACT
/// high), nothing much under it; EDGE the teeth's sharpness (the peak's
/// Q); DECAY from a snapped closed "ts" to a swelling open "tsss".
pub struct Hat {
    air: Hiss,
    flutter: Flutter,
    burst: Gesture,
    burst_hp: (Flt, f32),
    tbump: Modes<1>,
    puff: Puff,
    sib: Gesture,
    sib_hp: (Flt, f32),
    peak: Modes<2>,
    flat: f32,
    breath: (Flt, f32),
    /// Burst, pop, hiss, breath.
    lv: [f32; 4],
    vel: f32,
}

impl Hat {
    pub fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let s = expo(k[6], 0.8, 1.25);
        let dec = k[2];
        let fs = expo(k[3], 7200.0, 3600.0) * s;
        let q = lin(k[5], 0.0006, 0.0035);
        Self {
            air: Hiss::new(seed),
            flutter: Flutter::new(seed, 0.6 * k[4]),
            burst: Gesture::new(0.0, 0.0001, 0.0, 0.005),
            burst_hp: (Flt::clean(FilterMode::Hp24, 2800.0 * s, 0.0), 2800.0 * s),
            tbump: modes([4200.0 * s], [0.0015], [0.8], true),
            puff: Puff::at(320.0 * s),
            sib: Gesture::new(
                0.003,
                lin(dec, 0.002, 0.015),
                0.25 * dec * dec,
                expo(dec, 0.03, 0.9),
            ),
            sib_hp: (Flt::clean(FilterMode::Hp24, 0.6 * fs, 0.0), 0.6 * fs),
            peak: modes([fs, 1.5 * fs], [q, q], [1.0, 0.35], true),
            flat: 0.3 * (1.0 - k[5]),
            breath: (Flt::clean(FilterMode::Lp12, 2500.0, 0.0), 2500.0),
            lv: [0.9 * k[0], 0.35 * k[7], 0.8 * k[1], 0.3 * k[4]],
            vel,
        }
    }
}

impl Machine for Hat {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let n = self.air.block();
        let mut b: Blk = core::array::from_fn(|i| n[i] * self.burst.next());
        self.burst_hp.0.run(&mut b, self.burst_hp.1);
        let e: Blk = core::array::from_fn(|_| self.sib.next() * self.flutter.next());
        let mut f: Blk = core::array::from_fn(|i| n[i] * e[i]);
        let mut br = f;
        self.sib_hp.0.run(&mut f, self.sib_hp.1);
        self.breath.0.run(&mut br, self.breath.1);
        let lv = self.lv;
        for (i, o) in out.iter_mut().enumerate() {
            let t = b[i] + self.tbump.tick(b[i]);
            let s = self.flat * f[i] + self.peak.tick(f[i]);
            let x = lv[0] * t + lv[1] * self.puff.next() + lv[2] * s + lv[3] * br[i];
            *o = self.vel * Soft::new(0.03).tick(0.7 * x);
        }
    }
}

// --------------------------------------------------------------- click

/// VOX CLICK: a tongue click and a "K". The click: suction between the
/// tongue's two closures breaks, one impulse rings that cavity (PITCH:
/// ~1 kHz hollow alveolar "tok" to ~3.5 kHz palatal "tick"; HOLLOW the
/// rounded lips' bigger cavity and its low pop), a hair of edge noise.
/// The K: a velar burst through one compact peak at TRACT (~1.4–3 kHz, the
/// next vowel's F2), a second weaker burst 11 ms on, then AIR's
/// aspiration through the same peaks: the vowel's colour, briefly.
pub struct Click {
    air: Hiss,
    cavity: Modes<3>,
    struck: bool,
    edge: Gesture,
    edge_hp: Flt,
    bursts: (Gesture, Gesture),
    asp: Gesture,
    k_hp: Flt,
    velar: Modes<2>,
    /// Click, edge, burst, aspiration.
    lv: [f32; 4],
    drv: Soft,
    vel: f32,
}

impl Click {
    pub fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let fc = expo(k[0], 900.0, 3500.0);
        let ring = expo(k[2], 0.004, 0.04);
        let hol = k[6];
        let fk = expo(k[5], 1400.0, 3000.0);
        Self {
            air: Hiss::new(seed),
            cavity: modes(
                [fc, fc * lin(hol, 1.7, 1.35), lin(k[0], 420.0, 700.0)],
                [ring, 0.6 * ring, lin(hol, 0.01, 0.03)],
                [1.0, 0.5, 1.2 * hol],
                false,
            ),
            struck: false,
            edge: Gesture::new(0.0, 0.0, 0.0, 0.0015),
            edge_hp: Flt::clean(FilterMode::Hp24, 3000.0, 0.0),
            bursts: (
                Gesture::new(0.0, 0.0003, 0.0, 0.018),
                Gesture::new(0.011, 0.0003, 0.0, 0.01),
            ),
            asp: Gesture::new(0.016, 0.005, 0.0, expo(k[4], 0.03, 0.15)),
            k_hp: Flt::clean(FilterMode::Hp24, 600.0, 0.0),
            velar: modes([fk, 1.9 * fk], [0.004, 0.003], [1.0, 0.35], true),
            lv: [0.9 * k[1], 0.25 * k[1], 0.9 * k[3], 0.5 * k[4]],
            drv: Soft::new(0.5 * k[7] * k[7]),
            vel,
        }
    }
}

impl Machine for Click {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let n = self.air.block();
        let mut e: Blk = core::array::from_fn(|i| n[i] * self.edge.next());
        self.edge_hp.run(&mut e, 3000.0);
        let lv = self.lv;
        let mut kk: Blk = core::array::from_fn(|i| {
            let b = self.bursts.0.next() + 0.5 * self.bursts.1.next();
            n[i] * (lv[2] * b + lv[3] * self.asp.next())
        });
        self.k_hp.run(&mut kk, 600.0);
        for (i, o) in out.iter_mut().enumerate() {
            let x = if self.struck { 0.0 } else { 1.0 };
            self.struck = true;
            let s =
                lv[0] * self.cavity.tick(x) + lv[1] * e[i] + self.velar.tick(kk[i]) + 0.12 * kk[i];
            *o = self.vel * self.drv.tick(0.65 * s);
        }
    }
}

// ---------------------------------------------------------------- roll

/// VOX ROLL, "brrr": a lip trill as a bass. The lips flap at PITCH (a
/// trill's ~20–35 Hz up to a tensed ~80 Hz); every opening a flow pulse
/// (LIPS: sharper and shorter) and a puff of noise low-passed at TRACT;
/// JITTER loosens the period and the flaps' sizes; HUM voices it two
/// octaves up through an "uh"-ish pair of resonances; AIR is the breath
/// under it all.
pub struct Roll {
    air: Hiss,
    flutter: Flutter,
    env: Gesture,
    flaps: Flaps,
    puffs: Gesture,
    puff_t60: f32,
    lp: (Flt, f32),
    hum: Option<(Pulse, f32, Modes<2>)>,
    /// Flap, puff noise, hum, breath.
    lv: [f32; 4],
    drv: Soft,
    vel: f32,
}

impl Roll {
    pub fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let f = expo(k[0], 20.0, 80.0);
        let tract = expo(k[5], 500.0, 2200.0);
        let hum = (k[3] > 0.0).then(|| {
            let c = lin(k[5], 0.85, 1.2);
            (
                Pulse::new(0.6),
                (4.0 * f).clamp(80.0, 240.0),
                modes([550.0 * c, 1150.0 * c], [0.02, 0.012], [1.0, 0.5], false),
            )
        });
        let tau = lin(k[2], 0.22, 0.07) / f;
        Self {
            air: Hiss::new(seed),
            flutter: Flutter::new(seed, 0.5 * k[4]),
            env: Gesture::new(0.0, 0.012, lin(k[1], 0.03, 0.4), expo(k[1], 0.1, 1.5)),
            flaps: Flaps::new(f, tau, k[6], 1.0, 0.0, seed),
            puffs: Gesture::new(0.0, 0.0, 0.0, 0.0),
            puff_t60: (2.0 * tau).clamp(0.003, 0.012),
            lp: (Flt::clean(FilterMode::Lp12, tract, 0.1), tract),
            hum,
            lv: [0.75, 0.5 * (0.3 + k[2]), 0.05 * k[3], 0.2 * k[4]],
            drv: Soft::new(0.6 * k[7] * k[7]),
            vel,
        }
    }
}

impl Machine for Roll {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let n = self.air.block();
        let mut flap = [0.0; B];
        let mut nz = [0.0; B];
        let mut ev = [0.0; B];
        let lv = self.lv;
        for i in 0..B {
            let (p, open) = self.flaps.next();
            if open {
                self.puffs = Gesture::new(0.0, 0.0002, 0.0, self.puff_t60);
            }
            let e = self.env.next();
            ev[i] = e;
            flap[i] = e * p;
            nz[i] = e * n[i] * (lv[1] * self.puffs.next() + lv[3] * self.flutter.next());
        }
        self.lp.0.run(&mut nz, self.lp.1);
        for (i, o) in out.iter_mut().enumerate() {
            let mut x = lv[0] * flap[i] + nz[i];
            if let Some((p, hz, m)) = &mut self.hum {
                x += lv[2] * m.tick(p.tick(*hz) * ev[i]);
            }
            *o = self.vel * self.drv.tick(0.75 * x);
        }
    }
}

// ------------------------------------------------------------ builder

/// Which knob is TRACT, the mod destination.
pub fn tract_knob(id: Id) -> usize {
    match id {
        Id::VoxKick => 6,
        Id::VoxSnare | Id::VoxHat => 3,
        _ => 5,
    }
}

/// A VOX hit, TRACT moved by `tract` (knob units, a route latched at the
/// hit: accent, random, an LFO's value).
pub fn build(id: Id, mut k: Knobs, vel: f32, seed: u32, tract: f32) -> Box<dyn Machine> {
    let t = tract_knob(id);
    k[t] = (k[t] + tract).clamp(0.0, 1.0);
    match id {
        Id::VoxKick => Box::new(Kick::new(k, vel, seed)),
        Id::VoxSnare => Box::new(Snare::new(k, vel, seed)),
        Id::VoxHat => Box::new(Hat::new(k, vel, seed)),
        Id::VoxClick => Box::new(Click::new(k, vel, seed)),
        Id::VoxRoll => Box::new(Roll::new(k, vel, seed)),
        _ => panic!("{id:?} is not VOX"),
    }
}

// ------------------------------------------------------------ renders

use super::vox_formant;
use super::{Out, add_at, knob_map, knobs, run_hit, settings};
use serde_json::{Value, json};

pub fn run(out: &mut Out) -> Result<(), String> {
    before(out)?;
    groove(out)?;
    demo(out)
}

/// The first cut's classic hits, as rendered before, for the A/B.
fn before(out: &mut Out) -> Result<(), String> {
    for id in [Id::VoxKick, Id::VoxSnare, Id::VoxHat, Id::VoxClick] {
        let (name, k, listen) = vox_formant::settings(id)[0];
        let x = run_hit(
            vox_formant::build_vox(id, knobs(id, k), 1.0, 1, vox_formant::VowelMod::default()),
            6.0,
        );
        let m = spec(id)
            .name
            .trim_start_matches("VOX ")
            .to_ascii_lowercase();
        out.write(
            &format!("vox-before_{m}.wav"),
            &x,
            json!({
                "kind": "before", "machine": spec(id).name, "family": "VOX", "kernel": "vox (formant first cut)",
                "setting": name, "knobs_first_cut": k, "velocity": 127,
                "listen_for": format!("the first cut's classic {listen} (ALGO FRMT/NOIS, vowel first) against vox-{m}_classic.wav"),
            }),
        )?;
    }
    Ok(())
}

/// One VOX hit, TRACT moved by `tract`.
fn hit_t(id: Id, set: usize, vel: f32, seed: u32, tract: f32) -> Vec<f32> {
    let k = knobs(id, settings(id)[set].1);
    run_hit(build(id, k, vel, seed, tract), 3.0)
}

/// A lane of the groove: pad, machine, setting, 64 steps, level.
type Lane = (&'static str, Id, usize, &'static str, f32);

const GROOVE: [Lane; 7] = [
    (
        "KICK",
        Id::VoxKick,
        0,
        "X.....x...X.....X.....x.X..x....X.....x...X.....X.....x.X.x.x...",
        0.68,
    ),
    (
        "SNARE",
        Id::VoxSnare,
        0,
        "....X.......X.......X...............X.......X.......X.......X...",
        0.56,
    ),
    (
        "INWARD",
        Id::VoxSnare,
        2,
        "............................X..x............................x.xx",
        0.56,
    ),
    (
        "CLOSED",
        Id::VoxHat,
        0,
        "x.X.x.X.x.X.x...x.X.x.X.x.X.x...x.X.x.X.x.X.x...x.X.x.X.x.X.x...",
        0.4,
    ),
    (
        "OPEN",
        Id::VoxHat,
        2,
        "..............x...............x...............x.................",
        0.36,
    ),
    (
        "CLICK",
        Id::VoxClick,
        0,
        "..x.......x.......x.......x.x.....x.......x.......x.....x.x.....",
        0.44,
    ),
    (
        "ROLL",
        Id::VoxRoll,
        0,
        "..........................................................X.....",
        0.48,
    ),
];

const GROOVE_BPM: f32 = 96.0;
/// The swing: every second sixteenth at 60 % of the pair.
const SWING: f32 = 0.6;
/// Accent's route to TRACT and the random route's depth, knob units.
const ACCENT_TRACT: f32 = 0.12;
const RANDOM_TRACT: f32 = 0.12;

fn swung(i: usize, bpm: f32) -> usize {
    let step = 15.0 / bpm;
    let t = (i / 2) as f32 * 2.0 * step + if i % 2 == 1 { 2.0 * SWING * step } else { 0.0 };
    (t * FS) as usize
}

/// Four bars of beatbox, VOX only: swung, accents at full velocity (and
/// TRACT nudged), every hit a random TRACT nudge; the open hat choked by
/// the next closed one.
fn groove(out: &mut Out) -> Result<(), String> {
    let mut rng = Lcg(0x5eed_b0c5);
    let closed: Vec<usize> = GROOVE[3]
        .3
        .char_indices()
        .filter(|(_, c)| *c != '.')
        .map(|(i, _)| i)
        .collect();
    let mut x = Vec::new();
    let mut pads = Vec::new();
    for (lane, id, set, steps, gain) in GROOVE {
        for (i, c) in steps.chars().enumerate() {
            let (vel, acc) = match c {
                'X' => (1.0, ACCENT_TRACT),
                'x' => (0.7, 0.0),
                _ => continue,
            };
            let tract = acc + RANDOM_TRACT * rng.next();
            let mut h = hit_t(id, set, vel, 700 + i as u32, tract);
            if lane == "OPEN"
                && let Some(&j) = closed.iter().find(|&&j| j > i)
            {
                let cut = swung(j, GROOVE_BPM) - swung(i, GROOVE_BPM);
                let fade = (0.005 * FS) as usize;
                for (n, v) in h.iter_mut().enumerate().skip(cut) {
                    *v *= 1.0 - ((n - cut) as f32 / fade as f32).min(1.0);
                }
            }
            add_at(&mut x, swung(i, GROOVE_BPM), &h, gain);
        }
        let k = settings(id)[set].1;
        pads.push(json!({"pad": lane, "machine": spec(id).name, "setting": settings(id)[set].0, "knobs": knob_map(id, k), "level": gain, "steps": steps}));
    }
    x.resize(x.len().max(swung(64, GROOVE_BPM)), 0.0);
    out.write(
        "vox-groove_beatbox.wav",
        &x,
        json!({
            "kind": "vox-demo", "machine": "VOX KIT", "family": "VOX", "setting": "beatbox groove",
            "knobs": {}, "pads": pads, "bpm": GROOVE_BPM, "swing": SWING,
            "tract_routes": {"accent_to_tract": ACCENT_TRACT, "random_to_tract": RANDOM_TRACT},
            "listen_for": "four bars of beatbox from the VOX machines alone, swung 60 %: does it read as a mouth? \"B\" kicks, \"Pf\" snares with inward K snares in the fills, \"ts\" hats and an open \"tsss\" choked by the next closed one, tongue clicks, a lip roll at the end; accents louder, every hit's TRACT nudged at random so repeats don't sound looped",
        }),
    )
}

/// TRACT as a mod destination: bar 1 dry, bar 2 a random value per hit,
/// bars 3-4 an LFO's value latched at each hit.
fn demo(out: &mut Out) -> Result<(), String> {
    const BPM: f32 = 120.0;
    const DEPTH: f32 = 0.4;
    const LFO_HZ: f32 = 0.5;
    let lanes: [(Id, usize, &str, f32); 2] = [
        (
            Id::VoxHat,
            0,
            "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.",
            0.7,
        ),
        (
            Id::VoxClick,
            1,
            "...X...X...X...X...X...X...X...X...X...X...X...X...X...X...X...X",
            0.7,
        ),
    ];
    let mut rng = Lcg(0xb0ca1);
    let mut x = Vec::new();
    for (id, set, steps, gain) in lanes {
        for (i, c) in steps.chars().enumerate() {
            let vel = match c {
                'X' => 1.0,
                'x' => 0.75,
                _ => continue,
            };
            let t = i as f32 * 15.0 / BPM;
            let tract = match i / 16 {
                0 => 0.0,
                1 => DEPTH * rng.next(),
                _ => DEPTH * chimera_core::dsp::fast_sin_turns((LFO_HZ * t).fract()),
            };
            let h = hit_t(id, set, vel, 300 + i as u32, tract);
            add_at(&mut x, (t * FS) as usize, &h, gain);
        }
    }
    x.resize(x.len().max((4.0 * 4.0 * 60.0 / BPM * FS) as usize), 0.0);
    let pads: Vec<Value> = lanes
        .iter()
        .map(|(id, set, steps, gain)| json!({"machine": spec(*id).name, "setting": settings(*id)[*set].0, "knobs": knob_map(*id, settings(*id)[*set].1), "level": gain, "steps": steps}))
        .collect();
    out.write(
        "vox-demo_vowel-per-hit.wav",
        &x,
        json!({
            "kind": "vox-demo", "machine": "VOX HAT + VOX CLICK", "family": "VOX", "setting": "TRACT modulated",
            "knobs": {}, "pads": pads, "bpm": BPM,
            "tract_mod": {"bar 1": "none", "bar 2": format!("RANDOM per hit, latched, ±{DEPTH} TRACT"), "bars 3-4": format!("LFO {LFO_HZ} Hz sine, ±{DEPTH} TRACT, latched at each hit")},
            "listen_for": "the same two pads said differently: bar 1 every hit alike; bar 2 each hit a random place (hat between \"ts\" and \"sh\", the K between back \"ko\" and front \"ki\"); bars 3-4 the place sweeping slowly across the hits",
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Id; 5] = [
        Id::VoxKick,
        Id::VoxSnare,
        Id::VoxHat,
        Id::VoxClick,
        Id::VoxRoll,
    ];

    /// Zero crossings a second: where the energy sits, roughly.
    fn zcr(x: &[f32]) -> f32 {
        let n = x
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        n as f32 / (x.len() as f32 / FS)
    }

    /// The first difference's RMS over the signal's: brightness.
    fn bright(x: &[f32]) -> f32 {
        let d: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        (d / x.iter().map(|v| v * v).sum::<f32>()).sqrt()
    }

    #[test]
    fn tract_moves_every_machine() {
        for id in ALL {
            let at = |t: f32| {
                let mut k = knobs(id, settings(id)[0].1);
                // Only TRACT's sources: no pop, boom, hum or flaps.
                match id {
                    Id::VoxKick => (k[2], k[3], k[5], k[4]) = (0.0, 0.0, 0.0, 1.0),
                    Id::VoxClick => (k[1], k[3]) = (0.0, 1.0),
                    _ => {}
                }
                k[tract_knob(id)] = t;
                let x = run_hit(build(id, k, 1.0, 1, 0.0), 1.0);
                bright(&x[..x.len().min((0.08 * FS) as usize)])
            };
            let (lo, hi) = (at(0.0), at(1.0));
            assert!((hi / lo).ln().abs() > 0.1, "{id:?}: {lo} {hi}");
        }
    }

    #[test]
    fn every_setting_stays_under_full_scale() {
        for id in ALL {
            for (name, k, _) in settings(id) {
                let x = run_hit(build(id, knobs(id, k), 1.0, 1, 0.0), 3.0);
                let pk = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                assert!(pk < 1.0 && pk > 0.05, "{id:?} {name}: {pk}");
            }
        }
    }

    #[test]
    fn the_kick_sits_low_and_the_hat_high() {
        let z = |id| {
            zcr(&run_hit(
                build(id, knobs(id, settings(id)[0].1), 1.0, 1, 0.0),
                1.0,
            ))
        };
        assert!(z(Id::VoxKick) < 1500.0 && z(Id::VoxHat) > 6000.0);
    }
}
