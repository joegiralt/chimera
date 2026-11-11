//! DRUM's world batch (owner, 2026-11-11: "lets do all the modal/lpg
//! filter ones"), host prototypes for the audition page: five BODY
//! machines on a mode bank whose pitch moves a sample at a time (BAR, PAN,
//! GONG, TABLA) or on MODAL's dispersion stage (SPRING), and two of a new
//! LPG family: LPG BONGO (an oscillator pinged through the Plaits-port
//! gate) and FILTER PING (a struck resonant filter at CLEAN, LADDER or
//! 2040). Built from our parts only: DRUM's two-pole modes and mallet
//! pulse, BODY HAND's hand exciter, MODAL's `Allpass1`, the LPG port, the
//! filter TYPEs, VA NOISE, the FX slot's SOFT and FOLD. PITCH is the note
//! the cell shows (MIDI 0–127), so each machine plays chromatically.
//!
//! Acoustics used as data (ratios and behaviour, never code):
//! - Fletcher & Rossing 1998, *The Physics of Musical Instruments* (2nd
//!   ed.), ch. 19: marimba bars undercut to 1 : 3.9–4 : 9.2–10, the
//!   vibraphone's 1 : 4 : 10, the tube under each bar reinforcing the
//!   fundamental, the vibraphone motor's discs modulating that tube's
//!   radiation (tremolo); ch. 2: the clamped-free bar (a kalimba tine)
//!   at 1 : 6.267 : 17.55 : 34.39 (Euler–Bernoulli, β = 1.875, 4.694,
//!   7.855, 10.996); ch. 3: the free circular plate's modes (2,0) (0,1)
//!   (3,0) (1,1) (4,0) ... at 1 : 1.73 : 2.33 : 3.91 : 4.11 ... (Leissa
//!   1969, *Vibration of Plates*, NASA SP-160, ν ≈ 0.33; the upper four
//!   approximate); ch. 18: the ideal membrane's Bessel ratios 1 : 1.594 :
//!   2.136 : 2.296 : 2.653 : 2.918 : 3.156 : 3.501.
//! - Raman 1934, "The Indian musical drums", Proc. Indian Acad. Sci. A 1,
//!   179: the tabla's loaded head brings its first nine modes into five
//!   near-harmonic groups, 1 : 2 : 3 : 4 : 5.
//! - Rossing, Hampton & Hansen 2000, "Vibrational mode shapes in
//!   Caribbean steelpans", JASA 108(2) 803; Rossing & Hansen 2007,
//!   "Vibrational modes of the Hang" (ISMA): note areas tuned with their
//!   octave and twelfth (1 : 2 : 3); the Hang's Helmholtz "Gu" low under
//!   the notes. Achong 1996, J. Sound Vib. 197(4) 471: the note area's
//!   quadratic mode coupling moves the fundamental's energy into the
//!   octave after the strike (BLOOM here, a sample-rate y₀² push).
//! - Fletcher 1985, "Nonlinear frequency shifts in quasispherical-cap
//!   shells: pitch glide in Chinese gongs", JASA 78(6) 2069; Rossing &
//!   Fletcher 1983 JASA 73(1) 345: an opera gong's pitch rises with its
//!   amplitude, so it falls as the ring decays; a tam-tam's energy swells
//!   into its upper modes after the strike (SWELL).
//! - Rayleigh, *The Theory of Sound* §233: a thin ring's bending modes at
//!   n(n²−1)/√(n²+1), n = 2.. (1 : 2.83 : 5.42 : 8.77 : 12.9 : 17.7);
//!   Inácio, Henrique & Antunes 2006, "The dynamics of Tibetan singing
//!   bowls", Acta Acustica 92, 637: measured bowls near 1 : 2.78 : 5.17 :
//!   8.14, each mode a doublet split by the bowl's asymmetry (the beats),
//!   rubbing exciting them by friction.
//! - Välimäki, Parker & Abel 2010, "Parametric spring reverberation
//!   effect", JAES 58(7/8) 547; Parker 2011, DAFx-11, "Efficient
//!   dispersion generation structures for spring reverb emulation": a
//!   spring's chirps from a cascade of identical first-order allpasses
//!   in a feedback delay loop, low frequencies arriving last.
//!
//! Everything moves a sample at a time: the mode banks' coefficients ramp
//! across each block to its end's pitch, the sweeps and envelopes are
//! per-sample, the filters ramp their `g` per sample as the engine does
//! (FILTER PING's sweep sets its end each block). The LPG's vactrol steps
//! every 12 samples with its gain ramped between, as the port does.

use super::machines::{Hand, Id, Knobs, Machine, NoiseLayer, Stroke, drive, spec};
use super::parts::*;
use super::{Out, add_at, knob_map, knobs, run_hit};
use chimera_core::dsp::fast_sin_turns;
use chimera_core::dsp::filter::{FilterKind, FilterMode};
use chimera_core::dsp::lpg::{self, Drive, Level, Lpg, Onset};
use chimera_core::dsp::modal::Allpass1;
use chimera_core::dsp::va::osc::Unit;
use chimera_core::dsp::voice_fx::fold;
use chimera_core::{MidiNote, Velocity};
use serde_json::json;

/// This batch's machines.
pub const IDS: [Id; 7] = [
    Id::BodyBar,
    Id::BodyPan,
    Id::BodyGong,
    Id::BodyTabla,
    Id::BodySpring,
    Id::LpgBongo,
    Id::FilterPing,
];

/// PITCH: the note the cell shows.
fn hz(k: f32) -> f32 {
    440.0 * ((k * 127.0 - 69.0) / 12.0).exp2()
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Glides `N` sets of `[f32; M]` along `x` in 0..N−1.
fn glide<const M: usize>(sets: &[[f32; M]], x: f32) -> [f32; M] {
    let top = sets.len() - 1;
    let x = x.clamp(0.0, top as f32);
    let i = (x as usize).min(top.saturating_sub(1));
    let t = x - i as f32;
    let j = (i + 1).min(top);
    core::array::from_fn(|n| sets[i][n] + t * (sets[j][n] - sets[i][n]))
}

// ------------------------------------------------------------ the bank

/// DRUM's two-pole modes (as `parts::Modes`: impulse-normalised, each at
/// its law's amplitude) whose coefficients ramp a sample at a time to
/// where `aim` points them by the block's end, so a glide never steps.
/// `push` feeds a mode directly (couplings).
struct Ring<const N: usize> {
    ratio: [f32; N],
    amp: [f32; N],
    t60: [f32; N],
    /// Per mode: `2r·cos w`, `r²`, input gain now, and their steps.
    c: [[f32; 3]; N],
    d: [[f32; 3]; N],
    y: [[f32; 2]; N],
    push: [f32; N],
    left: usize,
    fresh: bool,
}

impl<const N: usize> Ring<N> {
    fn new() -> Self {
        Self {
            ratio: [1.0; N],
            amp: [0.0; N],
            t60: [0.1; N],
            c: [[0.0; 3]; N],
            d: [[0.0; 3]; N],
            y: [[0.0; 2]; N],
            push: [0.0; N],
            left: 0,
            fresh: true,
        }
    }

    fn target(&self, i: usize, f0: f32) -> [f32; 3] {
        let f = (f0 * self.ratio[i]).max(10.0);
        let on = f < 0.45 * FS && self.amp[i] != 0.0;
        let w = core::f32::consts::TAU * f.min(0.45 * FS) / FS;
        let r = per_sample(self.t60[i]);
        [
            2.0 * r * w.cos(),
            r * r,
            if on { self.amp[i] * w.sin() } else { 0.0 },
        ]
    }

    /// `2π f / FS`'s sine at mode `i` (couplings scale by it).
    fn sin_w(&self, i: usize, f0: f32) -> f32 {
        (core::f32::consts::TAU * (f0 * self.ratio[i]).min(0.45 * FS) / FS).sin()
    }

    /// Every mode at `f0` by the block's end (a fresh bank starts there).
    fn aim(&mut self, f0: f32) {
        for i in 0..N {
            let t = self.target(i, f0);
            if self.fresh {
                self.c[i] = t;
                self.d[i] = [0.0; 3];
            } else {
                self.d[i] = core::array::from_fn(|j| (t[j] - self.c[i][j]) / B as f32);
            }
        }
        self.left = if self.fresh { 0 } else { B };
        self.fresh = false;
    }

    /// A sample: `x` into every mode; mode 0's output and the rest's.
    #[inline]
    fn tick(&mut self, x: f32) -> (f32, f32) {
        if self.left > 0 {
            self.left -= 1;
            for (c, d) in self.c.iter_mut().zip(&self.d) {
                c[0] += d[0];
                c[1] += d[1];
                c[2] += d[2];
            }
        }
        let mut out = [0.0f32; 2];
        for i in 0..N {
            let c = self.c[i];
            let y = &mut self.y[i];
            let v = c[0] * y[0] - c[1] * y[1] + c[2] * x + self.push[i];
            y[1] = y[0];
            y[0] = v;
            self.push[i] = 0.0;
            out[(i > 0) as usize] += v;
        }
        (out[0], out[1])
    }
}

/// DRUM's mallet: a half-sine force pulse of area 1 lasting the contact,
/// and a breath of low-passed noise for the stick's texture.
struct Mallet {
    n: u32,
    len: u32,
    grit: f32,
    lp: f32,
    rng: Lcg,
}

impl Mallet {
    fn new(contact: f32, grit: f32, seed: u32) -> Self {
        Self {
            n: 0,
            len: ((contact * FS) as u32).max(1),
            grit,
            lp: 0.0,
            rng: Lcg(seed ^ 0x3A11),
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        let n = self.n;
        if n > 2 * self.len {
            return 0.0;
        }
        self.n += 1;
        let mut x = 0.0;
        if n < self.len {
            let l = self.len as f32;
            x = core::f32::consts::FRAC_PI_2 / l * fast_sin_turns(0.5 * (n as f32 + 0.5) / l);
        }
        if self.grit > 0.0 {
            self.lp += 0.4 * (self.rng.next() - self.lp);
            x += 0.08 * self.grit * self.lp;
        }
        x
    }
}

// ---------------------------------------------------------- BODY (modes)

/// Pitch motion, semitones over the note, as a function of the time since
/// the hit: a static press, a strike bend falling away, a swoop rising in,
/// a gong's amplitude glide falling with the ring.
#[derive(Clone, Copy, Default)]
struct Motion {
    press: f32,
    /// Depth, time constant (s).
    bend: (f32, f32),
    /// Depth, rise time (s).
    swoop: (f32, f32),
    /// Depth, the ring's amplitude time constant (s): falls as amplitude².
    glide: (f32, f32),
}

impl Motion {
    fn at(&self, t: f32) -> f32 {
        let mut st = self.press;
        if self.bend.0 != 0.0 {
            st += self.bend.0 * (-t / self.bend.1).exp();
        }
        if self.swoop.0 != 0.0 {
            st += self.swoop.0 * smooth(t / self.swoop.1);
        }
        if self.glide.0 != 0.0 {
            st += self.glide.0 * (-2.0 * t / self.glide.1).exp();
        }
        st
    }

    fn moves(&self) -> bool {
        self.bend.0 != 0.0 || self.swoop.0 != 0.0 || self.glide.0 != 0.0
    }
}

/// A rubbed strike (the bowl's stick round the rim): noise band-passed at
/// the fundamental under a swelling, held, fading envelope.
struct Rub {
    hiss: Hiss,
    f: Flt,
    hz: f32,
    level: f32,
    /// Rise, hold, fall in samples.
    shape: (f32, f32, f32),
}

impl Rub {
    fn env(&self, n: f32) -> f32 {
        let (r, h, f) = self.shape;
        if n < r {
            smooth(n / r)
        } else if n < r + h {
            1.0
        } else {
            1.0 - smooth((n - r - h) / f)
        }
    }
}

/// The tam-tam's swell: noise pushed into the upper modes under a slow
/// rise and fall after the strike.
struct Swell {
    rng: Lcg,
    amount: f32,
    from: usize,
    gain: Vec<f32>,
    rise: Decay,
    fall: Decay,
}

/// The bar's buzz: what a rattle (a mirliton, a kalimba's ring) adds when
/// the ring's swing passes its gap: the dead-zone's excess, high-passed.
struct Buzz {
    amount: f32,
    gap: f32,
    f: Flt,
    rng: Lcg,
}

/// The struck body every BODY mode machine plays: a mode bank, its
/// strike (mallet and/or hand, or a rub), its pitch motion, and the
/// extras a machine turns on.
struct Body<const N: usize> {
    ring: Ring<N>,
    f0: f32,
    n: u32,
    motion: Motion,
    /// A mod route's lane on the pitch (PRESS), semitones a sample.
    lane: Option<Vec<f32>>,
    mallet: Option<(Mallet, f32)>,
    hand: Option<Hand>,
    rub: Option<Rub>,
    swell: Option<Swell>,
    buzz: Option<Buzz>,
    /// The note area's y₀² into mode 1 (BLOOM), scaled by mode 1's sin w.
    bloom: f32,
    /// Depth, phase, phase step a sample: the motor's tremolo on mode 0
    /// (the tube's radiation), a quarter of it on the rest.
    trem: Option<(f32, f32, f32)>,
    vel: f32,
    gain: f32,
}

impl<const N: usize> Body<N> {
    fn new(ring: Ring<N>, f0: f32, motion: Motion, vel: f32) -> Self {
        let mut b = Self {
            ring,
            f0,
            n: 0,
            motion,
            lane: None,
            mallet: None,
            hand: None,
            rub: None,
            swell: None,
            buzz: None,
            bloom: 0.0,
            trem: None,
            vel,
            gain: 0.9,
        };
        let f = b.pitch(0, 1.0);
        b.ring.aim(f);
        b
    }

    fn pitch(&self, n: u32, fm: f32) -> f32 {
        let mut st = self.motion.at(n as f32 / FS);
        if let Some(l) = &self.lane {
            st += l.get(n as usize).or(l.last()).copied().unwrap_or(0.0);
        }
        self.f0 * (st / 12.0).exp2() * fm
    }
}

impl<const N: usize> Machine for Body<N> {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let m = fm.map_or(1.0, |b| {
            (1.0 + 4.0 * b.iter().sum::<f32>() / B as f32).max(0.01)
        });
        let end = self.n + B as u32;
        let mut retune = self.motion.moves() || self.lane.is_some() || fm.is_some();
        let (hf, heard) = self
            .hand
            .as_mut()
            .map_or(([0.0; B], [0.0; B]), |h| h.block());
        // The hand left on the head damps it (BODY HAND's law).
        if let Some(h) = &mut self.hand
            && let Some((at, upper, all)) = h.damp
            && at < end
        {
            h.damp = None;
            for (i, t) in self.ring.t60.iter_mut().enumerate() {
                *t *= all * if i >= 2 { upper } else { 1.0 };
            }
            retune = true;
        }
        if retune {
            let f = self.pitch(end - 1, m);
            self.ring.aim(f);
        }
        let mut rub = [0.0; B];
        if let Some(r) = &mut self.rub {
            let noise = r.hiss.block();
            for (i, x) in rub.iter_mut().enumerate() {
                *x = noise[i] * r.env((self.n + i as u32) as f32) * r.level;
            }
            r.f.run(&mut rub, r.hz);
        }
        let s1 = if self.bloom > 0.0 {
            self.bloom * self.ring.sin_w(1, self.f0)
        } else {
            0.0
        };
        let mut raw = [0.0; B];
        for (i, o) in out.iter_mut().enumerate() {
            let mut x = hf[i] + rub[i];
            if let Some((ml, w)) = &mut self.mallet {
                x += *w * ml.next();
            }
            if let Some(s) = &mut self.swell {
                let e = s.amount * (1.0 - s.rise.next()) * s.fall.next();
                if e > 1e-6 {
                    for j in s.from..N {
                        self.ring.push[j] += e * s.gain[j] * s.rng.next();
                    }
                }
            }
            if s1 != 0.0 {
                let y0 = self.ring.y[0][0];
                self.ring.push[1] += s1 * y0 * y0;
            }
            let (y0, rest) = self.ring.tick(x);
            let y = match &mut self.trem {
                Some((d, ph, inc)) => {
                    let u = 0.5 - 0.5 * fast_sin_turns(*ph + 0.25);
                    *ph += *inc;
                    if *ph >= 1.0 {
                        *ph -= 1.0;
                    }
                    y0 * (1.0 - *d * u) + rest * (1.0 - 0.25 * *d * u)
                }
                None => y0 + rest,
            };
            raw[i] = y;
            *o = self.vel * (y + heard[i]);
        }
        if let Some(bz) = &mut self.buzz {
            let mut b = [0.0; B];
            for (x, y) in b.iter_mut().zip(&raw) {
                let e = y - y.clamp(-bz.gap, bz.gap);
                *x = e * (0.7 + 0.3 * bz.rng.next());
            }
            bz.f.run(&mut b, 1800.0);
            for (o, x) in out.iter_mut().zip(&b) {
                *o += self.vel * bz.amount * x;
            }
        }
        self.n = end;
        for o in out.iter_mut() {
            *o = self.gain * (*o).tanh();
        }
    }
}

/// Mode `i`'s amplitude: `ratio^−slope` times a weight.
fn law<const N: usize>(ring: &mut Ring<N>, w: [f32; N], slope: f32, t60: f32, tilt: f32) {
    for i in 0..N {
        let r = ring.ratio[i].max(0.05);
        ring.amp[i] = w[i] * r.powf(-slope);
        ring.t60[i] = t60 * r.max(1.0).powf(-tilt);
    }
}

// BAR's three bars: ratios (four bending modes and two weak torsional or
// lateral ones), weights, the fundamental's T60 at C4 (s) and how it
// shortens up the range, the upper modes' tilt, the tube's lift on the
// fundamental, the contact's scale.
const BAR_RATIO: [[f32; 6]; 3] = [
    // Marimba: 1 : 3.9 : 9.2 (F&R ch. 19), the 4th bending mode untuned.
    [1.0, 3.9, 9.2, 15.4, 2.9, 6.4],
    // Vibraphone: 1 : 4 : 10.
    [1.0, 4.0, 10.0, 17.6, 2.7, 6.6],
    // Kalimba tine: clamped-free, 1 : 6.267 : 17.55 : 34.39.
    [1.0, 6.267, 17.55, 34.39, 3.3, 11.0],
];
const BAR_W: [[f32; 6]; 3] = [
    [1.0, 0.6, 0.35, 0.12, 0.05, 0.04],
    [1.0, 0.5, 0.25, 0.1, 0.06, 0.05],
    [1.0, 0.35, 0.12, 0.05, 0.08, 0.04],
];
/// T60 at C4, the fall up the range (exponent), tilt, tube lift, contact.
const BAR_LAW: [[f32; 5]; 3] = [
    [0.9, 0.7, 1.1, 1.5, 1.0],
    [4.5, 0.5, 0.6, 1.3, 1.0],
    [2.2, 0.5, 1.3, 1.1, 1.6],
];

/// BODY BAR: PITCH · DECAY · HARDNESS · BAR (MARIMBA → VIBES → KALIMBA,
/// gliding) · TREMOLO · BUZZ | RATE · POSITION.
fn bar(k: Knobs, vel: f32, seed: u32) -> Body<6> {
    let x = 2.0 * k[3];
    let ratio = glide(&BAR_RATIO, x);
    let mut w = glide(&BAR_W, x);
    let [t60c, fall, tilt, lift, cmul] = glide(&BAR_LAW, x);
    let f0 = hz(k[0]);
    // POSITION: at the centre the antisymmetric modes (2nd, 4th) sleep.
    let pos = k[7];
    w[1] *= lin(pos, 0.12, 1.0);
    w[3] *= lin(pos, 0.12, 1.0);
    w[0] *= lift * lin(pos, 1.0, 0.75);
    let mut ring = Ring::new();
    ring.ratio = ratio;
    let t60 = t60c * (f0 / 261.6).powf(-fall) * expo(k[1], 0.25, 4.0);
    law(&mut ring, w, lin(k[2], 1.3, 0.3), t60, tilt);
    ring.t60[0] *= lin(lift, 1.0, 1.25);
    let mut b = Body::new(ring, f0, Motion::default(), vel);
    let contact = expo(1.0 - k[2], 0.000_25, 0.004) * cmul;
    b.mallet = Some((Mallet::new(contact, 0.15 * k[2], seed), 1.0));
    if k[4] > 0.0 {
        let rate = expo(k[6], 1.5, 12.0);
        let ph = Lcg(seed ^ 0x7E40).unit();
        b.trem = Some((0.9 * k[4], ph, rate / FS));
    }
    if k[5] > 0.0 {
        b.buzz = Some(Buzz {
            amount: 4.0 * k[5],
            gap: 0.06,
            f: Flt::clean(FilterMode::Hp24, 1800.0, 0.3),
            rng: Lcg(seed ^ 0xB22),
        });
    }
    b.gain = 0.85;
    b
}

// PAN's eight slots: the note area's fundamental, octave, twelfth and
// double octave, a skirt mode, the Hang's Gu (its ratio set per hit from
// a fixed ~95 Hz), and two neighbouring note areas (a fifth under, a
// fourth over) ringing in sympathy.
const PAN_RATIO: [f32; 8] = [1.0, 2.0, 3.0, 4.0, 2.41, 0.0, 2.0 / 3.0, 4.0 / 3.0];
/// Steel then handpan: weights.
const PAN_W: [[f32; 8]; 2] = [
    [1.0, 0.7, 0.4, 0.15, 0.1, 0.0, 1.0, 1.0],
    [1.0, 0.55, 0.3, 0.06, 0.02, 0.35, 1.0, 1.0],
];
/// T60 at C4, its fall up the range, tilt, contact (s).
const PAN_LAW: [[f32; 4]; 2] = [[1.4, 0.4, 0.5, 0.0007], [3.5, 0.3, 0.6, 0.0016]];

/// BODY PAN: PITCH · DECAY · HARDNESS · SHELL (STEEL → HANDPAN) ·
/// OVERTONES · HAND | BLOOM · SYMPATHY.
fn pan(k: Knobs, vel: f32, seed: u32) -> Body<8> {
    let x = k[3];
    let mut w = glide(&PAN_W, x);
    let [t60c, fall, tilt, contact] = glide(&PAN_LAW, x);
    let f0 = hz(k[0]);
    let mut ring = Ring::new();
    ring.ratio = PAN_RATIO;
    ring.ratio[5] = 95.0 / f0;
    let ov = 2.0 * k[4];
    for wi in &mut w[1..4] {
        *wi *= ov;
    }
    w[6] *= 0.12 * k[7];
    w[7] *= 0.08 * k[7];
    let t60 = t60c * (f0 / 261.6).powf(-fall) * expo(k[1], 0.25, 4.0);
    law(&mut ring, w, lin(k[2], 0.8, 0.2), t60, tilt);
    // The Gu, a Helmholtz cavity: damped, its own length.
    ring.t60[5] = 0.5;
    ring.amp[5] = w[5];
    for t in &mut ring.t60[6..8] {
        *t = 0.6 * t60;
    }
    let mut b = Body::new(ring, f0, Motion::default(), vel);
    let hand = k[5];
    b.mallet = Some((
        Mallet::new(contact * expo(1.0 - k[2], 0.4, 2.5), 0.1, seed),
        1.0 - hand,
    ));
    if hand > 0.0 {
        let mut rng = Lcg(seed.wrapping_mul(2_654_435_761) ^ 0x9A4);
        b.hand = Some(Hand::new(Stroke::Open, hand, k[2], vel, &mut rng, seed));
    }
    b.bloom = 0.0025 * k[6];
    b.gain = 0.85;
    b
}

const GONG_N: usize = 12;
// GONG's plate: the free circular plate's first twelve modes (Leissa).
const PLATE: [f32; GONG_N] = [
    1.0, 1.729, 2.328, 3.906, 4.112, 6.294, 6.711, 7.331, 8.833, 10.07, 11.67, 13.31,
];
/// The bowl's six ring modes (Rayleigh), each a doublet; `BEAT` splits
/// the pairs.
const RING_MODES: [f32; 6] = [1.0, 2.83, 5.42, 8.77, 12.87, 17.7];

/// BODY GONG: PITCH · DECAY · HARDNESS · SHAPE (PLATE → BOWL) · GLIDE ·
/// RUB | SWELL · BEAT.
fn gong(k: Knobs, vel: f32, seed: u32) -> Body<GONG_N> {
    let f0 = hz(k[0]);
    let x = k[3];
    // The bowl's doublets: beat 0.2 to 3 Hz at the fundamental, wider up.
    let split = lin(k[7], 0.2, 3.0);
    let bowl: [f32; GONG_N] = core::array::from_fn(|i| {
        let r = RING_MODES[i / 2];
        if i % 2 == 0 {
            r
        } else {
            r + split * (1.0 + 0.5 * (i / 2) as f32) / f0
        }
    });
    let plate: [f32; GONG_N] =
        core::array::from_fn(|i| PLATE[i] * (1.0 + 0.0015 * k[7] * (i % 2) as f32));
    let mut ring = Ring::new();
    ring.ratio = glide(&[plate, bowl], x);
    let pw: [f32; GONG_N] = core::array::from_fn(|i| if i == 0 { 1.0 } else { 0.8 });
    let bw: [f32; GONG_N] = core::array::from_fn(|i| if i % 2 == 0 { 1.0 } else { 0.8 });
    let w = glide(&[pw, bw], x);
    let t60 = lin(x, 4.0, 9.0) * expo(k[1], 0.15, 2.0);
    law(&mut ring, w, lin(k[2], 1.5, 0.4), t60, lin(x, 0.45, 0.7));
    let tau = ring.t60[0] / 6.9;
    // GLIDE: up to four semitones over the note at the strike, falling
    // with the ring's amplitude squared (Fletcher 1985).
    let motion = Motion {
        glide: (4.0 * k[4] * vel, tau),
        ..Motion::default()
    };
    let mut b = Body::new(ring, f0, motion, vel);
    let rub = k[5];
    b.mallet = Some((
        Mallet::new(expo(1.0 - k[2], 0.0006, 0.012), 0.2, seed),
        1.0 - rub,
    ));
    if rub > 0.0 {
        b.rub = Some(Rub {
            hiss: Hiss::new(seed ^ 0x2B),
            f: Flt::clean(FilterMode::Bp12, f0, 0.85),
            hz: f0,
            level: 0.006 * rub,
            shape: (0.6 * FS, 1.4 * FS, 1.2 * FS),
        });
    }
    if k[6] > 0.0 {
        let gain = (0..GONG_N)
            .map(|i| 0.003 * b.ring.sin_w(i, f0) * b.ring.amp[i].sqrt())
            .collect();
        b.swell = Some(Swell {
            rng: Lcg(seed ^ 0x5E11),
            amount: k[6] * vel,
            from: 3,
            gain,
            rise: Decay::tau(0.25),
            fall: Decay::tau(0.9),
        });
    }
    b.gain = 0.8;
    b
}

const TABLA_N: usize = 8;
/// Dayan (Raman's loaded head, degenerate pairs a hair apart), bayan
/// (half loaded), talking drum (the unloaded membrane's Bessel ratios).
const TABLA_RATIO: [[f32; TABLA_N]; 3] = [
    [1.0, 2.0, 3.0, 3.012, 4.0, 4.018, 5.0, 5.03],
    [1.0, 1.8, 2.57, 2.66, 3.33, 3.46, 4.08, 4.27],
    [1.0, 1.594, 2.136, 2.296, 2.653, 2.918, 3.156, 3.501],
];
const TABLA_W: [f32; TABLA_N] = [1.0, 0.8, 0.6, 0.4, 0.4, 0.3, 0.25, 0.2];

/// BODY TABLA: PITCH · DECAY · HAND · DRUM (DAYAN → BAYAN → TALKING) ·
/// STROKE (TUN, NA, TE) · PRESS | SWOOP · SWOOP TIME. PRESS is the palm's
/// static pressure and a mod destination (`lane`, semitones a sample).
fn tabla(k: Knobs, vel: f32, seed: u32, lane: Option<Vec<f32>>) -> Body<TABLA_N> {
    let x = 2.0 * k[3];
    let f0 = hz(k[0]);
    let mut ring = Ring::new();
    ring.ratio = glide(&TABLA_RATIO, x);
    let mut w = TABLA_W;
    // STROKE: TUN (centre, open), NA (the rim with a finger resting on
    // the syahi's edge: the fundamental damped, the harmonics ring), TE
    // (closed, the fingers stay).
    let (st, keep0, bright) = match (k[4] * 3.0) as u32 {
        0 => (Stroke::Open, 1.0, 0.4),
        1 => (Stroke::Open, 0.15, 1.0),
        _ => (Stroke::Mute, 1.0, 0.5),
    };
    w[0] *= keep0;
    // Harmonic heads ring long in their overtones; the bare head less.
    let t60 = lin(x / 2.0, 1.1, 0.6) * expo(k[1], 0.25, 4.0);
    law(
        &mut ring,
        w,
        lin(bright, 1.0, 0.5),
        t60,
        lin(x / 2.0, 0.35, 1.0),
    );
    if keep0 < 1.0 {
        ring.t60[0] *= 0.2;
    }
    let mut rng = Lcg(seed.wrapping_mul(2_654_435_761) ^ 0x7AB1);
    let hand = k[2];
    let motion = Motion {
        press: 7.0 * k[5],
        bend: (0.6 * hand * vel * lin(rng.unit(), 0.5, 1.0), 0.03),
        swoop: (12.0 * k[6], expo(k[7], 0.03, 1.0)),
        ..Motion::default()
    };
    let mut b = Body::new(ring, f0, motion, vel);
    b.lane = lane;
    // The talking drum's curved stick under HAND 0.
    b.mallet = Some((Mallet::new(0.0012, 0.2, seed), 1.0 - hand));
    if hand > 0.0 {
        b.hand = Some(Hand::new(st, hand, bright, vel, &mut rng, seed));
    }
    b.gain = 0.9;
    b
}

// --------------------------------------------------------------- SPRING

const STAGES: usize = 32;
const LINE: usize = 4096;

/// One spring: a feedback delay loop through a cascade of MODAL's
/// first-order allpasses (`Allpass1`, η < 0: lows delayed most, so each
/// pass is a falling chirp), damped by a one-pole low-pass.
struct Coil {
    line: Vec<f32>,
    w: usize,
    len: usize,
    ap: [Allpass1; STAGES],
    lp: f32,
    a: f32,
    g: f32,
}

impl Coil {
    fn new(f: f32, eta: f32, t60: f32, damp_hz: f32) -> Self {
        let len = ((FS / f) as usize).clamp(2, LINE - 1);
        let mut ap = [Allpass1::default(); STAGES];
        for s in &mut ap {
            s.set(eta);
        }
        let dc = STAGES as f32 * (1.0 - eta) / (1.0 + eta);
        let lp_len = len as f32 + dc;
        Self {
            line: vec![0.0; LINE],
            w: 0,
            len,
            ap,
            lp: 0.0,
            a: 1.0 - (-core::f32::consts::TAU * damp_hz / FS).exp(),
            g: 10f32.powf(-3.0 * lp_len / (t60 * FS)),
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let back = self.line[(self.w + LINE - self.len) & (LINE - 1)];
        let mut v = x + self.g * back;
        for s in &mut self.ap {
            v = s.process(v);
        }
        self.lp += self.a * (v - self.lp);
        self.line[self.w] = self.lp;
        self.w = (self.w + 1) & (LINE - 1);
        v
    }
}

/// BODY SPRING: PITCH · DECAY · CHIRP · SHAKE · TUBE · DAMP | TONE ·
/// SPRINGS. A struck or shaken spring (Välimäki, Parker & Abel's
/// structure), into the thunder tube's head (four membrane modes) by
/// TUBE, a second spring by SPRINGS.
struct Spring {
    coils: Vec<(Coil, f32)>,
    n: u32,
    /// The excitation: samples, a strike's pulse or a shake's noise.
    shake: u32,
    jolts: [(u32, f32); 6],
    rng: Lcg,
    tube: Option<(Ring<4>, f32)>,
    tone: Flt,
    tone_hz: f32,
    vel: f32,
    gain: f32,
}

fn spring(k: Knobs, vel: f32, seed: u32) -> Spring {
    let f = hz(k[0]);
    // CHIRP: each stage's DC delay from 1.5 to ~66 samples (the chirp's
    // spread over 32 stages, ~1 to ~44 ms).
    let eta = expo(1.0 - k[2], 0.03, 0.4) - 1.0;
    let t60 = expo(k[1], 0.2, 6.0);
    let damp = expo(1.0 - k[5], 900.0, 14_000.0);
    let mut coils = vec![(Coil::new(f, eta, t60, damp), 1.0)];
    if k[7] > 0.0 {
        coils.push((Coil::new(f * 0.73, eta * 0.97, t60 * 1.2, damp), k[7]));
    }
    let mut rng = Lcg(seed ^ 0x5915);
    let shake = (expo(k[3], 0.0003, 0.8) * FS) as u32;
    let jolts = core::array::from_fn(|i| {
        let at = if i == 0 {
            0
        } else {
            (rng.unit() * shake as f32) as u32
        };
        (
            at,
            if i == 0 {
                1.0
            } else {
                lin(rng.unit(), 0.3, 1.0)
            },
        )
    });
    let tube = (k[4] > 0.0).then(|| {
        let mut r = Ring::<4>::new();
        r.ratio = [1.0, 1.594, 2.136, 2.296];
        r.amp = [1.0, 0.6, 0.4, 0.3];
        r.t60 = [0.35, 0.25, 0.2, 0.18];
        r.aim((2.5 * f).clamp(50.0, 400.0));
        (r, k[4])
    });
    Spring {
        coils,
        n: 0,
        shake,
        jolts,
        rng,
        tube,
        tone: Flt::clean(FilterMode::Lp12, 8000.0, 0.0),
        tone_hz: expo(k[6], 300.0, 14_000.0),
        vel,
        gain: 0.8,
    }
}

impl Spring {
    /// The excitation at sample `n`: a 0.3 ms strike at SHAKE 0, a run of
    /// noise jolts as it rises.
    fn excite(&mut self, n: u32) -> f32 {
        if n >= self.shake.max(16) {
            return 0.0;
        }
        if self.shake < 16 {
            return if n < 8 {
                fast_sin_turns(0.5 * (n as f32 + 0.5) / 8.0) * 5.0
            } else {
                0.0
            };
        }
        let mut e = 0.0;
        for &(at, g) in &self.jolts {
            if n >= at {
                e += g * (-((n - at) as f32) / (0.02 * FS)).exp();
            }
        }
        let win = fast_sin_turns(0.5 * n as f32 / self.shake as f32);
        0.06 * self.rng.next() * e * win
    }
}

impl Machine for Spring {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        for o in out.iter_mut() {
            let x = self.excite(self.n);
            self.n += 1;
            let mut y = 0.0;
            for (c, g) in &mut self.coils {
                y += *g * c.tick(x);
            }
            if let Some((r, mix)) = &mut self.tube {
                let (a, b) = r.tick(y);
                y = (1.0 - *mix) * y + *mix * 6.0 * (a + b);
            }
            *o = y;
        }
        self.tone.run(out, self.tone_hz);
        for o in out.iter_mut() {
            *o = self.gain * (self.vel * *o).tanh();
        }
    }
}

// ------------------------------------------------------------ LPG BONGO

/// FM's ratios, stepped.
const BONGO_RATIOS: [f32; 8] = [0.5, 1.0, 1.5, 2.0, 2.76, 3.0, 3.5, 5.4];

/// LPG BONGO: PITCH · DECAY · COLOR · STRIKE · FM · TONE | RATIO · BEND.
/// The West Coast bongo: an oscillator (sine, through triangle, into
/// FOLD) phase-modulated by a second at RATIO, its index falling with the
/// hit, pinged through the Plaits-port LPG (DECAY its decay, COLOR its
/// colour, STRIKE the ping's height with a thump of the ping heard).
struct Bongo {
    ph: f32,
    mph: f32,
    f0: f32,
    ratio: f32,
    index: f32,
    index_env: Decay,
    bend: f32,
    bend_env: Decay,
    tone: f32,
    thump: (u32, f32),
    n: u32,
    lpg: Lpg,
    knobs: lpg::Knobs,
    hit: Velocity,
    note: MidiNote,
    gain: f32,
}

fn bongo(k: Knobs, vel: f32) -> Bongo {
    let strike = lin(k[3], 0.3, 1.0);
    let colour = k[2];
    let mut lpg = Lpg::NEW;
    lpg.strike(Onset::Fresh);
    let (short, tail) = lpg::fall(
        Unit::new(k[1]),
        lpg::Colour::of(Unit::new(1.0 - colour)),
        FS,
    );
    let _ = tail;
    Bongo {
        ph: 0.0,
        mph: 0.0,
        f0: hz(k[0]),
        ratio: BONGO_RATIOS[((k[6] * 8.0) as usize).min(7)],
        index: 3.0 * k[4] * k[4] * vel,
        index_env: Decay::tau((short * 30.0).clamp(0.01, 0.4)),
        bend: 7.0 * k[7] * vel,
        bend_env: Decay::tau(0.025),
        tone: k[5],
        thump: ((0.0015 * FS) as u32, 0.6 * k[3] * vel),
        n: 0,
        lpg,
        // COLOR is Plaits': the gate's LPG knob is 1 − COLOR; STRIKE 1 so
        // the ping alone drives it (no route holds a drum).
        knobs: lpg::Knobs::of([1.0 - colour, k[1], 1.0]),
        hit: Velocity::new((vel * strike * 127.0).round().clamp(1.0, 127.0) as u8).unwrap(),
        note: MidiNote::clamped((k[0] * 127.0) as u8),
        gain: 0.9,
    }
}

impl Machine for Bongo {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let fold_amt = ((self.tone - 0.5) * 2.0).max(0.0);
        let tri_amt = (self.tone * 2.0).min(1.0);
        for (i, o) in out.iter_mut().enumerate() {
            let m = fm.map_or(1.0, |b| (1.0 + 4.0 * b[i]).max(0.0));
            let f = self.f0 * (self.bend * self.bend_env.next() / 12.0).exp2() * m;
            self.mph += f * self.ratio / FS;
            self.mph -= self.mph.floor();
            self.ph += f / FS;
            self.ph -= self.ph.floor();
            let pm = self.index * self.index_env.next() * fast_sin_turns(self.mph) * 0.16;
            let p = (self.ph + pm).rem_euclid(1.0);
            let sine = fast_sin_turns(p);
            let tri = 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs();
            let mut y = sine + tri_amt * (tri - sine);
            if fold_amt > 0.0 {
                y = fold::curve(y, [fold_amt, 0.5, fold::UNITY_LEVEL]);
            }
            let (len, g) = self.thump;
            if self.n < len {
                y += g * fast_sin_turns(0.5 * (self.n as f32 + 0.5) / len as f32);
            }
            self.n += 1;
            *o = 0.8 * y;
        }
        self.lpg.block(
            out,
            &Drive {
                level: Level::Open,
                knobs: self.knobs,
                hit: self.hit,
                note: self.note,
                fs: FS,
            },
        );
        for o in out.iter_mut() {
            *o *= self.gain;
        }
    }
}

// ----------------------------------------------------------- FILTER PING

/// FILTER PING: PITCH · DECAY · SWEEP · DRIVE · NOISE · TYPE | SWEEP TIME
/// · WIDTH. A short pulse (and a breath of noise) strikes one of our
/// filters near self-oscillation: CLEAN's band-pass, LADDER's or the
/// 2040's four-pole low-pass. DECAY sets its RES (its ring's length),
/// SWEEP drops the cutoff from up to two octaves over, DRIVE pushes the
/// strike into the filter's own saturation and the FX slot's SOFT.
struct Ping {
    f: Flt,
    hz0: f32,
    sweep: f32,
    tau: f32,
    pulse: u32,
    amp: f32,
    noise: Option<NoiseLayer>,
    drive: f32,
    /// The TYPE's output level (each rings at its own height).
    level: f32,
    n: u32,
    /// Holds the hit's end where RES rings on (CLEAN past 0.95).
    env: Decay,
    gain: f32,
}

/// TYPE's three: the filter, its mode, RES at DECAY 0 and 1 (each ring
/// at E3 from ~36 ms to ~2.7 s on CLEAN, ~0.1 to ~1.4 s on the four-poles,
/// measured: `ring_lengths`), output level (each peaks near −6 dBFS).
const PING_TYPES: [(FilterKind, FilterMode, f32, f32, f32); 3] = [
    (FilterKind::Svf, FilterMode::Bp12, 0.80, 0.948, 2.5),
    (FilterKind::Ladder, FilterMode::Lp24, 0.75, 0.935, 18.0),
    (FilterKind::K2040, FilterMode::Lp24, 0.75, 0.935, 18.0),
];

fn ping(k: Knobs, vel: f32, seed: u32) -> Ping {
    let (kind, mode, lo, hi, level) = PING_TYPES[((k[5] * 3.0) as usize).min(2)];
    let d = k[1];
    let res = lin(1.0 - (1.0 - d) * (1.0 - d), lo, hi);
    let hz0 = hz(k[0]);
    let drive = expo(k[3], 1.0, 8.0);
    Ping {
        f: Flt::new(kind, mode, hz0 * (2.0 * k[2]).exp2(), res),
        hz0,
        sweep: 2.0 * k[2],
        tau: expo(k[6], 0.004, 0.3),
        pulse: (expo(k[7], 0.000_2, 0.004) * FS).max(1.0) as u32,
        amp: vel,
        // CLEAN's band-pass rings quieter as its Q rises.
        level: level
            * if kind == FilterKind::Svf {
                1.0 + 1.7 * d
            } else {
                1.0
            },
        noise: (k[4] > 0.0).then(|| {
            NoiseLayer::new(
                seed,
                expo(k[4], 0.01, 0.08),
                FilterMode::Bp12,
                hz0 * 4.0,
                0.1,
                0.3 * k[4] * vel,
            )
        }),
        drive,
        n: 0,
        env: Decay::t60(expo(d, 0.2, 4.0)),
        gain: 0.9,
    }
}

impl Machine for Ping {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let mut x = self.noise.as_mut().map_or([0.0; B], |n| n.block());
        let l = self.pulse as f32;
        for (i, s) in x.iter_mut().enumerate() {
            let n = self.n + i as u32;
            if n < self.pulse {
                // A half-sine of full height: WIDTH widens and softens it.
                *s += fast_sin_turns(0.5 * (n as f32 + 0.5) / l);
            }
            *s *= self.amp * self.drive;
        }
        self.n += B as u32;
        let t = self.n as f32 / FS;
        let m = fm.map_or(1.0, |b| (1.0 + 4.0 * b[B - 1]).max(0.01));
        let hz = self.hz0 * (self.sweep * (-t / self.tau).exp()).exp2() * m;
        self.f.run(&mut x, hz);
        let post = (self.drive - 1.0) / 7.0;
        for (o, v) in out.iter_mut().zip(&x) {
            let u = self.level * self.env.next() * v;
            let y = if post > 0.0 { drive(u, post) } else { u };
            *o = self.gain * y.tanh();
        }
    }
}

// ---------------------------------------------------------------- build

/// A hit; `lane` is TABLA's PRESS as a mod route (semitones a sample).
pub fn build(id: Id, k: Knobs, vel: f32, seed: u32, lane: Option<Vec<f32>>) -> Box<dyn Machine> {
    match id {
        Id::BodyBar => Box::new(bar(k, vel, seed)),
        Id::BodyPan => Box::new(pan(k, vel, seed)),
        Id::BodyGong => Box::new(gong(k, vel, seed)),
        Id::BodyTabla => Box::new(tabla(k, vel, seed, lane)),
        Id::BodySpring => Box::new(spring(k, vel, seed)),
        Id::LpgBongo => Box::new(bongo(k, vel)),
        Id::FilterPing => Box::new(ping(k, vel, seed)),
        _ => unreachable!("not this batch's: {id:?}"),
    }
}

/// A setting: its name, knobs (0..127, PITCH a MIDI note), what to hear.
pub type Setting = (&'static str, &'static [u8], &'static str);

pub fn settings(id: Id) -> Vec<Setting> {
    match id {
        Id::BodyBar => vec![
            (
                "marimba",
                &[57, 64, 60, 0, 0, 0, 50, 40],
                "A3 marimba: a yarn mallet, the tuned 1 : 3.9 : 9.2 bar, the tube's warm fundamental, a short woody ring",
            ),
            (
                "vibes",
                &[65, 70, 70, 64, 70, 0, 50, 40],
                "F4 vibraphone: the 1 : 4 : 10 aluminium bar ringing long, the motor's tremolo (~4 Hz) on the tube's fundamental",
            ),
            (
                "kalimba",
                &[69, 60, 50, 127, 0, 50, 50, 40],
                "A4 kalimba: a thumbed tine's clamped-free modes (1 : 6.27 : 17.6), a quick plink, the buzz rattling while it swings wide",
            ),
        ],
        Id::BodyPan => vec![
            (
                "steel",
                &[72, 60, 80, 0, 80, 0, 60, 30],
                "C5 steel pan: a rubber-tipped stick, the note's octave and twelfth, the octave blooming in after the strike",
            ),
            (
                "handpan",
                &[57, 75, 40, 127, 70, 110, 40, 40],
                "A3 handpan: a fingertip (BODY HAND's exciter), softer, long, the Gu's low under it, neighbours humming",
            ),
        ],
        Id::BodyGong => vec![
            (
                "gong",
                &[40, 100, 40, 0, 70, 0, 40, 30],
                "E2 opera gong: a soft beater, the plate's dense modes, the pitch sliding down as it rings, a shimmer swelling in",
            ),
            (
                "bowl",
                &[62, 85, 70, 127, 0, 0, 0, 60],
                "D4 singing bowl, struck: the ring's modes in doublets beating slowly, a very long ring",
            ),
            (
                "bowl-rub",
                &[62, 85, 70, 127, 0, 127, 0, 60],
                "the same bowl rubbed: a stick round the rim swelling the fundamental in, held, then let go",
            ),
        ],
        Id::BodyTabla => vec![
            (
                "dayan",
                &[62, 64, 110, 0, 10, 0, 0, 50],
                "D4 dayan, TUN: a fingertip in the centre, the loaded head's harmonic 1 : 2 : 3 : 4 : 5, a ringing pitched drum",
            ),
            (
                "dayan-na",
                &[62, 64, 110, 0, 64, 0, 0, 50],
                "the dayan's NA: the rim struck, the fundamental damped by a resting finger, the bright harmonics ringing",
            ),
            (
                "bayan-swoop",
                &[43, 70, 110, 64, 10, 0, 90, 60],
                "G2 bayan \"ghe\": the open bass stroke, then the palm's heel pressing the pitch up most of an octave",
            ),
            (
                "talking",
                &[50, 70, 20, 127, 10, 20, 100, 50],
                "D3 talking drum: a curved stick on a bare head (Bessel modes), squeezed up as it rings",
            ),
        ],
        Id::BodySpring => vec![
            (
                "boing",
                &[45, 70, 70, 0, 0, 30, 90, 0],
                "a struck spring: falling chirps repeating as the wave runs the spring and back, each smeared more",
            ),
            (
                "thunder",
                &[24, 100, 100, 110, 80, 50, 60, 127],
                "a thunder tube shaken: two long springs into a drum head, a low rolling rumble of chirps",
            ),
        ],
        Id::LpgBongo => vec![
            (
                "classic",
                &[64, 40, 64, 80, 20, 20, 40, 30],
                "the West Coast bongo: a near-sine pinged through the LPG, bright at the hit and darkening as it closes",
            ),
            (
                "mid",
                &[76, 30, 90, 100, 60, 50, 70, 50],
                "higher, triangle, more FM at ratio 2.76 (a bar's): a woody, metallic knock, a little bend",
            ),
            (
                "extreme",
                &[45, 90, 20, 127, 127, 127, 127, 127],
                "low, folded, full FM at 5.4, a big bend and a long dark gate: a gamelan-ish gong-bongo",
            ),
        ],
        Id::FilterPing => vec![
            (
                "classic",
                &[52, 90, 40, 30, 0, 0, 30, 10],
                "CLEAN's band-pass struck near self-oscillation: an 808-style tom, a short fall",
            ),
            (
                "mid",
                &[64, 60, 20, 60, 40, 64, 20, 20],
                "LADDER pinged with a breath of noise and some drive: a conga-like knock",
            ),
            (
                "extreme",
                &[40, 110, 127, 127, 60, 127, 60, 0],
                "the 2040 driven hard, a two-octave dive: a long zap",
            ),
        ],
        _ => unreachable!(),
    }
}

/// Two bars of sixteenths, one token a step: `.` rest, `X<st>` an accent,
/// `x<st>` at 70 %, `<st>` semitones over the setting's PITCH.
fn pattern(id: Id) -> &'static str {
    match id {
        Id::BodyBar => {
            "X0 . x7 . x12 . x7 x10 . x7 x3 . x5 . x7 . X0 . x7 . x12 . x15 . x14 . x12 x10 . x7 x5 ."
        }
        Id::BodyPan => {
            "X0 . x4 x7 . x12 . x7 X5 . x9 . x12 . x9 x7 X0 . x4 x7 . x12 . x14 X12 . x9 . x7 x5 x4 ."
        }
        Id::BodyGong => "X0 . . . . . . . . . . . . . . . . . . . . . . . x-5 . . . . . . .",
        Id::BodyTabla => {
            "X0 . x0 x0 X0 . x0 . X0 . x0 x0 X0 . x0 x0 X0 . x0 x0 X0 . x0 . X0 x0 x0 x0 X0 x0 X0 x0"
        }
        Id::BodySpring => "X0 . . . . . . . x5 . . . . . . . X0 . . . . . . . x-3 . . . x7 . . .",
        Id::LpgBongo => {
            "X0 . x7 x0 . x7 X0 . x0 . x7 . X12 . x7 x0 X0 . x7 x0 . x7 X0 . x3 x5 x7 . X12 x7 x5 x3"
        }
        Id::FilterPing => {
            "X0 . . x5 . . X7 . . x5 . x0 X-5 . x0 . X0 . . x5 . . X7 . x12 x10 x7 x5 X0 x-2 x-5 ."
        }
        _ => unreachable!(),
    }
}

/// `(step, semitones, velocity)` of a pattern's tokens.
fn steps(p: &str) -> Vec<(usize, f32, f32)> {
    p.split_whitespace()
        .enumerate()
        .filter_map(|(i, t)| {
            let (v, st) = match t.as_bytes()[0] {
                b'X' => (1.0, &t[1..]),
                b'x' => (0.7, &t[1..]),
                _ => return None,
            };
            Some((i, st.parse::<f32>().unwrap_or(0.0), v))
        })
        .collect()
}

/// `k` with PITCH moved `st` semitones.
fn transposed(id: Id, k: &[u8], st: f32) -> Knobs {
    let mut kn = knobs(id, k);
    kn[0] = (kn[0] + st / 127.0).clamp(0.0, 1.0);
    kn
}

/// How long a hit may ring when rendered alone.
fn max_s(id: Id) -> f32 {
    match id {
        Id::BodyGong => 10.0,
        _ => 6.0,
    }
}

/// A pattern's steps at `bpm` with each hit's semitones, into `out`.
fn play(id: Id, k: &[u8], p: &str, bpm: f32, gain: f32, seed: u32, out: &mut Vec<f32>) {
    let step = 60.0 / bpm / 4.0;
    for (i, st, vel) in steps(p) {
        let h = run_hit(
            build(
                id,
                transposed(id, k, st),
                vel,
                seed.wrapping_add(i as u32 * 7919),
                None,
            ),
            max_s(id).min(4.0),
        );
        add_at(out, (i as f32 * step * FS) as usize, &h, gain);
    }
}

pub fn run(out: &mut Out) -> Result<(), String> {
    for id in IDS {
        let s = spec(id);
        let slug = s.name.to_ascii_lowercase().replace(' ', "-");
        let sets = settings(id);
        for (si, (name, k, listen)) in sets.iter().enumerate() {
            let x = run_hit(build(id, knobs(id, k), 1.0, 1 + si as u32, None), max_s(id));
            let row = json!({
                "kind": "hit", "machine": s.name, "family": s.family, "kernel": s.kernel,
                "setting": name, "knobs": knob_map(id, k), "velocity": 127, "listen_for": listen,
            });
            out.write(&format!("{slug}_{name}.wav"), &x, row)?;
        }
        let (name, k, _) = sets[0];
        let p = pattern(id);
        let mut x = Vec::new();
        play(id, k, p, 120.0, 0.8, 11, &mut x);
        x.resize(x.len().max((4.0 * FS) as usize), 0.0);
        let row = json!({
            "kind": "pattern", "machine": s.name, "family": s.family, "kernel": s.kernel,
            "setting": name, "knobs": knob_map(id, k), "steps": p, "bpm": 120,
            "listen_for": "the first setting in a two-bar groove, each step's number the semitones over its PITCH (the pad played chromatically from C1 up); accents (X) at full velocity, others at 70 %",
        });
        out.write(&format!("{slug}_pattern.wav"), &x, row)?;
    }
    press_demo(out)?;
    groove(out)
}

/// A route's value, semitones, at a time since the hit.
type Route = fn(f32) -> f32;

/// TABLA's PRESS as a mod destination: four bayan strokes, the palm's
/// pressure from a route (an envelope's rise, an LFO's wobble, a pressure
/// gesture down and up), a sample at a time.
fn press_demo(out: &mut Out) -> Result<(), String> {
    let id = Id::BodyTabla;
    let k: &[u8] = &[43, 70, 110, 64, 10, 0, 0, 50];
    let n = (1.0 * FS) as usize;
    let lanes: [(&str, Route); 4] = [
        ("none", |_| 0.0),
        ("envelope", |t| 9.0 * smooth(t / 0.15)),
        ("lfo", |t| 2.5 * (1.0 - fast_sin_turns(6.0 * t + 0.25))),
        ("gesture", |t| {
            7.0 * smooth(t / 0.08) * (1.0 - smooth((t - 0.3) / 0.25))
        }),
    ];
    let mut x = Vec::new();
    let mut names = Vec::new();
    for (j, (name, f)) in lanes.iter().enumerate() {
        let lane: Vec<f32> = (0..n).map(|i| f(i as f32 / FS)).collect();
        let h = run_hit(build(id, knobs(id, k), 1.0, 40 + j as u32, Some(lane)), 1.0);
        add_at(&mut x, j * n, &h, 1.0);
        names.push(*name);
    }
    out.write(
        "body-tabla_press-mod.wav",
        &x,
        json!({
            "kind": "demo", "machine": "BODY TABLA", "family": "BODY", "kernel": "modes",
            "setting": "bayan, PRESS routed", "knobs": knob_map(id, k), "routes": names,
            "listen_for": "four bayan strokes a second apart, PRESS moved by a route a sample at a time: none, an envelope pressing up 9 semitones in 150 ms, a 6 Hz LFO wobble, a press-and-release gesture; never a step",
        }),
    )
}

const WORLD_BPM: f32 = 100.0;

fn bars(b: &str, n: usize) -> String {
    vec![b; n].join(" ")
}

fn groove(out: &mut Out) -> Result<(), String> {
    let gong = format!("X0 {}", bars(".", 63));
    let dayan = bars("X0 . x0 . x0 x0 . x0 X0 . x0 . x0 x0 x0 .", 3)
        + " X0 x0 x0 x0 X0 . x0 x0 X0 x0 x0 x0 X0 x0 X0 x0";
    let bayan = bars("X0 . . . . . x0 . . . X0 . . . . .", 4);
    let bongo = bars(". . X7 x0 . . x7 . . . X7 x0 . x7 x0 .", 4);
    let ping = bars(". . . . . . . . . . . . X0 . x-5 .", 2)
        + " "
        + &bars(". . . . . . . . . . . . X0 x5 x0 x-5", 2);
    let marimba = bars("X0 . x7 . x12 . x7 . x10 . x7 . x3 . x5 .", 2)
        + " "
        + &bars("X-2 . x5 . x10 . x5 . x8 . x5 . x2 . x3 .", 2);
    let pan = format!(
        "{} . . . . . . . . X12 . x15 . x19 . x15 .  . . . . . . . . X10 . x14 . x17 . x22 x24",
        bars(".", 32)
    );
    let spring = format!("{} x0 . . .  {} X-5 . . .", bars(".", 28), bars(".", 28));
    let shaker = bars("x0 x0 X0 x0", 16);
    let lanes: Vec<(&str, Id, usize, String, f32)> = vec![
        ("GONG", Id::BodyGong, 0, gong, 0.25),
        ("DAYAN", Id::BodyTabla, 0, dayan, 0.4),
        ("BAYAN", Id::BodyTabla, 2, bayan, 0.5),
        ("BONGO", Id::LpgBongo, 0, bongo, 0.3),
        ("CONGA", Id::FilterPing, 1, ping, 0.3),
        ("MARIMBA", Id::BodyBar, 0, marimba, 0.3),
        ("STEEL", Id::BodyPan, 0, pan, 0.2),
        ("SPRING", Id::BodySpring, 0, spring, 0.25),
        ("SHAKER", Id::BodyShaker, 0, shaker, 0.18),
    ];
    let step = 60.0 / WORLD_BPM / 4.0;
    let mut x = Vec::new();
    let mut pads = Vec::new();
    for (pad, id, set, p, gain) in &lanes {
        let (sname, k) = if IDS.contains(id) {
            let (n, k, _) = settings(*id)[*set];
            (n, k)
        } else {
            let (n, k, _) = super::settings(*id)[*set];
            (n, k)
        };
        for (i, st, vel) in steps(p) {
            let seed = 900u32.wrapping_add(i as u32 * 31);
            let h = if IDS.contains(id) {
                run_hit(
                    build(*id, transposed(*id, k, st), vel, seed, None),
                    max_s(*id),
                )
            } else {
                run_hit(super::machines::build(*id, knobs(*id, k), vel, seed), 4.0)
            };
            add_at(&mut x, (i as f32 * step * FS) as usize, &h, *gain);
        }
        pads.push(json!({"pad": pad, "machine": spec(*id).name, "setting": sname, "knobs": knob_map(*id, k), "level": gain, "steps": p}));
    }
    x.resize(x.len().max((64.0 * step * FS) as usize), 0.0);
    out.write(
        "kit_world.wav",
        &x,
        json!({
            "kind": "kit", "machine": "KIT", "family": "KIT", "setting": "world groove",
            "knobs": {}, "pads": pads, "bpm": WORLD_BPM,
            "listen_for": "four bars of the new machines together at their mixer levels: a gong to open, dayan and bayan (its swoop) as the backbone, LPG bongos, FILTER PING congas, a marimba line, steel pan answers, a spring boing, BODY SHAKER as glue",
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(id: Id, set: usize) -> Vec<f32> {
        let (_, k, _) = settings(id)[set];
        run_hit(build(id, knobs(id, k), 1.0, 1, None), max_s(id))
    }

    #[test]
    fn every_hit_is_finite_bounded_and_ends() {
        flush_denormals();
        for id in IDS {
            for s in 0..settings(id).len() {
                let x = render(id, s);
                let pk = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                assert!(x.iter().all(|v| v.is_finite()), "{id:?} {s}");
                assert!(pk < 1.0 && pk > 0.01, "{id:?} {s}: peak {pk}");
                let tail = &x[x.len() - B..];
                let t = tail.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                assert!(
                    t < 0.02,
                    "{id:?} {s}: still {t} at {} s",
                    x.len() as f32 / FS
                );
            }
        }
    }

    /// A ring's coefficients reach `aim`'s target in one block, a sample
    /// at a time.
    #[test]
    fn ring_glides_to_its_aim_in_a_block() {
        let mut r = Ring::<2>::new();
        r.ratio = [1.0, 2.0];
        r.amp = [1.0, 0.5];
        r.t60 = [1.0, 0.5];
        r.aim(200.0);
        r.aim(300.0);
        let mut last = r.c[0][0];
        for _ in 0..B {
            r.tick(0.0);
            assert!(r.c[0][0] < last);
            last = r.c[0][0];
        }
        let t = r.target(0, 300.0);
        assert!((r.c[0][0] - t[0]).abs() < 1e-5);
        r.tick(0.0);
        assert_eq!(r.c[0][0], last, "holds once there");
    }

    /// The bayan's swoop raises its pitch: zero crossings late in the hit
    /// come faster than early.
    #[test]
    fn swoop_rises() {
        let x = render(Id::BodyTabla, 2);
        let rate = |a: f32, b: f32| {
            let s = &x[(a * FS) as usize..(b * FS) as usize];
            s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 / (b - a)
        };
        assert!(rate(0.45, 0.6) > 1.3 * rate(0.02, 0.1));
    }

    /// The level of `f` in `x[a..b]` s (Goertzel, Hann window).
    fn tone(x: &[f32], f: f32, a: f32, b: f32) -> f32 {
        let s = &x[(a * FS) as usize..(b * FS) as usize];
        let w = core::f32::consts::TAU * f / FS;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let n = s.len() as f32;
        for (i, v) in s.iter().enumerate() {
            let h = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n).cos();
            let y = (v * h) as f64 + 2.0 * (w as f64).cos() * s1 - s2;
            (s2, s1) = (s1, y);
        }
        ((s1 * s1 + s2 * s2 - 2.0 * (w as f64).cos() * s1 * s2).sqrt() / n as f64) as f32
    }

    /// The steel pan's octave blooms in: its share of the fundamental
    /// grows after the strike.
    #[test]
    fn octave_blooms() {
        let x = render(Id::BodyPan, 0);
        let f0 = hz(72.0 / 127.0);
        let early = tone(&x, 2.0 * f0, 0.0, 0.03) / tone(&x, f0, 0.0, 0.03);
        let late = tone(&x, 2.0 * f0, 0.1, 0.2) / tone(&x, f0, 0.1, 0.2);
        println!("octave share {early:.3} -> {late:.3}");
        assert!(late > 1.2 * early);
    }

    /// The gong's pitch falls as it rings.
    #[test]
    fn gong_glides_down() {
        let x = render(Id::BodyGong, 0);
        let f0 = hz(40.0 / 127.0);
        let at = |t: f32| {
            (0..=40)
                .map(|c| f0 * ((c as f32 * 10.0) / 1200.0).exp2())
                .max_by(|a, b| tone(&x, *a, t, t + 0.4).total_cmp(&tone(&x, *b, t, t + 0.4)))
                .unwrap()
        };
        let (a, b) = (at(0.0), at(3.0));
        println!("gong {a:.1} Hz -> {b:.1} Hz (note {f0:.1})");
        assert!(a > b * 1.03);
    }

    /// The motor's tremolo moves the vibraphone's level at RATE.
    #[test]
    fn vibes_tremolo() {
        let x = render(Id::BodyBar, 1);
        let env: Vec<f32> = x[..(3.0 * FS) as usize]
            .chunks(480)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / 480.0).sqrt())
            .collect();
        // Detrended by the ring's own fall: the level's wobble a block.
        let r: Vec<f32> = env.windows(2).map(|w| w[1] / w[0].max(1e-9)).collect();
        let dev = r.iter().map(|v| (v - 1.0).abs()).sum::<f32>() / r.len() as f32;
        println!("vibes level wobble {dev:.3} per 10 ms");
        assert!(dev > 0.02);
    }

    /// A spring's pass is a falling chirp: its highs arrive before its
    /// lows (the allpasses delay the lows most), ~1 ms and ~12 ms apart
    /// at CHIRP 70.
    #[test]
    fn spring_chirps_fall() {
        let eta = expo(1.0 - 70.0 / 127.0, 0.03, 0.4) - 1.0;
        let mut c = Coil::new(110.0, eta, 1.0, 14_000.0);
        c.g = 0.0;
        let x: Vec<f32> = (0..(0.05 * FS) as usize)
            .map(|n| c.tick(if n == 0 { 1.0 } else { 0.0 }))
            .collect();
        let when = |f: f32| {
            (0..80)
                .map(|i| i as f32 * 0.0005)
                .max_by(|a, b| tone(&x, f, *a, a + 0.004).total_cmp(&tone(&x, f, *b, b + 0.004)))
                .unwrap()
        };
        let (hi, lo) = (when(3000.0), when(300.0));
        println!("chirp: 3 kHz at {hi:.4} s, 300 Hz at {lo:.4} s");
        assert!(lo > hi + 0.006);
    }

    /// Print each FILTER PING TYPE's ring length over DECAY (calibration).
    #[test]
    #[ignore]
    fn ring_lengths() {
        flush_denormals();
        for t in 0..3 {
            for d in [0u8, 32, 64, 96, 127] {
                let k: Knobs = knobs(Id::FilterPing, &[55, d, 0, 0, 0, t * 60, 0, 10]);
                let mut p = ping(k, 1.0, 1);
                p.env = Decay::t60(1e3);
                let x = run_hit(Box::new(p), 6.0);
                let pk = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                let end = x.iter().rposition(|v| v.abs() > pk * 1e-3).unwrap_or(0);
                println!(
                    "type {t} decay {d}: peak {pk:.3}, -60 dB at {:.3} s",
                    end as f32 / FS
                );
            }
        }
    }
}
