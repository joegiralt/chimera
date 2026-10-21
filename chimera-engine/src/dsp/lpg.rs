//! LPG: Plaits' low-pass gate (ADR 0195), ported from Mutable Instruments
//! Plaits, `plaits/dsp/fx/low_pass_gate.h` (LowPassGate), the LPGEnvelope
//! of `plaits/dsp/envelope.h`, the decay and colour mapping of
//! `plaits/dsp/voice.cc` and stmlib's `Svf` (`dsp/filter.h`,
//! FREQUENCY_DIRTY), as the LPG lab ported them first
//! (`chimera-utils/src/lpg_gate.rs`, branch `lpg-circuit-lab`).
//!
//! Copyright 2014–2016 Emilie Gillet. MIT licence:
//!
//!   Permission is hereby granted, free of charge, to any person obtaining a
//!   copy of this software and associated documentation files (the
//!   "Software"), to deal in the Software without restriction, including
//!   without limitation the rights to use, copy, modify, merge, publish,
//!   distribute, sublicense, and/or sell copies of the Software, and to
//!   permit persons to whom the Software is furnished to do so, subject to
//!   the following conditions:
//!
//!   The above copyright notice and this permission notice shall be
//!   included in all copies or substantial portions of the Software.
//!
//!   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//!   OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//!   MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//!   IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//!   CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//!   TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//!   SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//!
//! In Chimera it is the VCA's: the VCA stays a VCA and three knobs turn it
//! into Plaits' gate, all three 0 a plain VCA. One state, the "vactrol",
//! does three jobs every 12 samples (Plaits' block): it is the gain, it
//! sets the cutoff `(0.003 + 0.3·v⁴ + 0.04·COLOR)·fs`, and its fall slows
//! as it falls. The gain ramps per sample across each 12; the SVF (Q 0.4)
//! holds its coefficients for them, as Plaits does.
//!
//! - **LPG** is Plaits' COLOR inverted (`COLOR = 1 − LPG`): 0 bleeds the
//!   gated signal straight past the filter, a VCA; 1 is all filter, the
//!   darkest gate. It also weighs in voice.cc's compressed LEVEL.
//! - **DECAY** is Plaits' decay. Its first 1/64 fades the vactrol in: at 0
//!   the state is the level each step, and the routes' detail between the
//!   steps rides on it, so the gain is the routes' own, sample for sample.
//! - **STRIKE** pings the gate at each note-on (`ProcessPing`), the ping
//!   rising to the level plus STRIKE × VEL; the level is × (1 − STRIKE), so
//!   at 1 the strike alone drives it.
//!
//! The cadence runs across the voice's blocks, so a 64-sample block holds
//! five or six steps. Every loop is bank 2's (`voice::gate::kern`).

use crate::dsp::{clampf, fast_exp2};
use crate::hw::{BLOCK_SIZE, Cost};

type Buf = [f32; BLOCK_SIZE];

/// Plaits' block, the vactrol's step: 12 samples.
pub const STEP: u8 = 12;
/// The vactrol's rise a step, Plaits'.
const ATTACK: f32 = 0.6;
/// The SVF's damping, 1/Q at Plaits' Q 0.4.
const R: f32 = 1.0 / 0.4;
/// stmlib's FREQUENCY_DIRTY: `tan(πf) ≈ f·(π + a·f²)`.
const PI: f32 = core::f32::consts::PI;
const TAN_A: f32 = 3.736e-1 * PI * PI * PI;
/// The gate is shut under −80 dB of gain: a voice it alone holds ends.
pub const SHUT: f32 = 1e-4;
/// DECAY at and above 1/64 is Plaits' decay as built; below, the vactrol
/// fades in from none.
const FADE_IN: f32 = 64.0;
/// A key-gated note with no route (TAPE, VA): after key up its level falls
/// to 0 over the voice's 128-sample fade, a step at a time.
const KEY_FALL: f32 = STEP as f32 / 128.0;

/// The gate's bill while it runs (`tools/loop_count.py` on the
/// `filter-probes` ELF, `lpg_probe`; ADR 0195). The routed loop, unrolled
/// 4×, is 127 instructions a time round (31.75 a sample), the unrouted 68
/// (17); the block's 654 (10.2 a sample) with its step, ~122 instructions
/// and 2 `vdiv`, taken 5⅓ times a block (8.3 more, 0.21 `vdiv` a sample
/// with the block's own): ceil5(50.2 × 1.606 + 12 × 0.21) = 85 routed, 60
/// not. Billed 85 either way.
pub const LPG_COST: Cost = Cost(85);

/// What drives the gate this block.
#[derive(Clone, Copy, Debug)]
pub struct Drive<'a> {
    /// The VCA routes' gain a sample, `None` with no route.
    pub level: Option<&'a Buf>,
    /// AMP's VEL at the note: × the routes and the strike's peak.
    pub vel: f32,
    /// A key-gated engine with no route: the key is the level (held).
    pub key: Option<bool>,
    pub lpg: f32,
    pub decay: f32,
    pub strike: f32,
    /// The note, for the strike's rise (Plaits'), and the rate.
    pub note: f32,
    pub fs: f32,
}

/// The gate: the SVF's two states, the vactrol, the gain's ramp, this
/// step's coefficients and the step's samples left; the key's level; the
/// last block's LPG and STRIKE, this block's ramp start.
#[derive(Clone, Copy, Debug)]
pub struct Lpg {
    s1: f32,
    s2: f32,
    vac: f32,
    gain: f32,
    inc: f32,
    g: f32,
    h: f32,
    bleed: f32,
    key: f32,
    /// The routes' level as the step ramps it, its step a sample, and the
    /// weight of the routes' own detail over that ramp: DECAY under its
    /// fade-in follows the routes a sample at a time, not a step late.
    lh: f32,
    linc: f32,
    detail: f32,
    last: [f32; 2],
    left: u8,
    /// A strike is rising (Plaits' `ramp_up_`).
    ramp_up: bool,
    /// A note-on since the last block.
    struck: bool,
    /// The gate ran last block; else the next starts where the VCA is.
    ran: bool,
    /// A fresh note: the next start is shut, so the gate opens as Plaits'.
    cold: bool,
}

/// One block's run: LPG and STRIKE `(from, to)`, the vactrol's fade-in,
/// the fall's two coefficients a step and a strike's rise.
#[derive(Clone, Copy, Debug)]
struct Run {
    a: (f32, f32),
    s: (f32, f32),
    w: f32,
    short: f32,
    tail: f32,
    attack: f32,
}

/// Plaits' fall a step at decay `d` and COLOR `hf`, rate `fs`: the short
/// decay and the tail (voice.cc).
#[inline(always)]
pub fn fall(d: f32, hf: f32, fs: f32) -> (f32, f32) {
    let step = f32::from(STEP) / fs;
    let short = 200.0 * step * fast_exp2(-8.0 * d);
    let tail = 20.0 * step * fast_exp2(-6.0 * d + hf) - short;
    (short, tail)
}

/// voice.cc's compressed LEVEL `1.3·l/(0.3 + l)`, weighed in by LPG `a`.
#[inline(always)]
fn compress(l: f32, a: f32) -> f32 {
    l + a * (1.3 * l / (0.3 + l) - l)
}

impl Lpg {
    pub const NEW: Lpg = Lpg {
        s1: 0.0,
        s2: 0.0,
        vac: 0.0,
        gain: 0.0,
        inc: 0.0,
        g: 0.0,
        h: 0.0,
        bleed: 0.0,
        key: 0.0,
        lh: 0.0,
        linc: 0.0,
        detail: 0.0,
        last: [0.0; 2],
        left: 0,
        ramp_up: false,
        struck: false,
        ran: false,
        cold: false,
    };

    /// A note-on: the next block strikes (if STRIKE is up); a fresh voice's
    /// gate starts shut.
    #[inline(always)]
    pub fn strike(&mut self, fresh: bool) {
        self.struck = true;
        if fresh {
            (self.ran, self.cold) = (false, true);
        }
    }

    /// A block the gate did not run: the next one starts where the VCA
    /// is.
    #[inline(always)]
    pub fn idle(&mut self) {
        (self.ran, self.struck, self.cold) = (false, false, false);
    }

    /// Open: rising from a strike, or above −80 dB.
    pub fn open(&self) -> bool {
        self.ramp_up || self.vac > SHUT
    }

    /// The vactrol: the gain the step ramps to.
    pub fn level(&self) -> f32 {
        self.vac
    }

    /// The level at sample `i`: the routes × VEL, the key's, or open.
    #[inline(always)]
    fn input(&mut self, d: &Drive, i: usize) -> f32 {
        match (d.level, d.key) {
            (Some(g), _) => clampf(g[i], 0.0, 1.0) * d.vel,
            (None, Some(held)) => {
                self.key = if held {
                    1.0
                } else {
                    (self.key - KEY_FALL).max(0.0)
                };
                self.key
            }
            (None, None) => 1.0,
        }
    }

    /// One block of the gate on `buf`, in place. Inline: the kernel and
    /// the probe each hold one copy.
    #[inline(always)]
    pub fn block(&mut self, buf: &mut Buf, d: &Drive) {
        let a = clampf(d.lpg, 0.0, 1.0);
        let s = clampf(d.strike, 0.0, 1.0);
        let dec = clampf(d.decay, 0.0, 1.0);
        let struck = self.struck;
        if !self.ran {
            // A fresh note's gate shut; else where the VCA is, so turning
            // the gate on never steps. No ring left either way.
            let cold = self.cold;
            *self = Lpg {
                key: if d.key == Some(false) { 0.0 } else { 1.0 },
                last: [a, s],
                ran: true,
                ..Lpg::NEW
            };
            let l = if cold { 0.0 } else { self.input(d, 0) };
            let u = (1.0 - s) * compress(l, a);
            (self.vac, self.gain, self.lh) = (u, u, l);
        }
        self.struck = false;
        if struck && s > 0.0 {
            self.ramp_up = true;
        }
        let [a0, s0] = core::mem::replace(&mut self.last, [a, s]);
        let (short, tail) = fall(dec, 1.0 - a, d.fs);
        // Plaits' `NoteToFrequency(note) · kBlockSize · 2`, while it rises.
        let attack = if self.ramp_up {
            2.0 * f32::from(STEP) * 440.0 / d.fs * fast_exp2((d.note - 69.0) * (1.0 / 12.0))
        } else {
            0.0
        };
        let r = Run {
            a: (a0, a),
            s: (s0, s),
            w: (dec * FADE_IN).min(1.0),
            short,
            tail,
            attack,
        };
        let mut i = 0;
        while i < BLOCK_SIZE {
            if self.left == 0 {
                self.step(d, i, &r);
            }
            let n = usize::from(self.left).min(BLOCK_SIZE - i);
            let (g, h, b, inc) = (self.g, self.h, self.bleed, self.inc);
            let rg = R + g;
            let mut gain = self.gain;
            match d.level {
                Some(lv) => {
                    let (mut lh, linc, k) = (self.lh, self.linc, self.detail);
                    for (x, l) in buf[i..i + n].iter_mut().zip(&lv[i..i + n]) {
                        gain += inc;
                        lh += linc;
                        let e = clampf(*l, 0.0, 1.0) * d.vel;
                        *x = self.tick(*x * (gain + k * (e - lh)), g, rg, h, b);
                    }
                    self.lh = lh;
                }
                None => {
                    for x in &mut buf[i..i + n] {
                        gain += inc;
                        *x = self.tick(*x * gain, g, rg, h, b);
                    }
                }
            }
            self.gain = gain;
            self.left -= n as u8;
            i += n;
        }
        self.settle();
    }

    /// One of Plaits' steps at sample `i` of the block: the vactrol moves
    /// toward its input (`LPGEnvelope::ProcessLP`, after `ProcessPing`'s
    /// rise on a strike), and the step's gain ramp, cutoff and bleed follow
    /// it. The cutoff and bleed read the state before the move, as Plaits'.
    #[inline(always)]
    fn step(&mut self, d: &Drive, i: usize, r: &Run) {
        let t = i as f32 * (1.0 / BLOCK_SIZE as f32);
        let a = r.a.0 + (r.a.1 - r.a.0) * t;
        let s = r.s.0 + (r.s.1 - r.s.0) * t;
        let hf = 1.0 - a;
        let l = self.input(d, i);
        let u = (1.0 - s) * compress(l, a);
        self.linc = (l - self.lh) * (1.0 / f32::from(STEP));
        self.detail = (1.0 - r.w) * (1.0 - s);
        let input = if self.ramp_up {
            self.vac += r.attack;
            let peak = (u + s * d.vel).min(1.0);
            if self.vac >= peak {
                (self.vac, self.ramp_up) = (peak, false);
            }
            if self.ramp_up { self.vac } else { u }
        } else {
            u
        };
        let v = self.vac;
        let err = input - v;
        let v2 = v * v;
        let v4 = v2 * v2;
        let tl = 1.0 - v;
        let t2 = tl * tl;
        let c = if err > 0.0 {
            ATTACK
        } else {
            r.short + (1.0 - v4) * r.tail
        };
        self.vac = v + (1.0 - r.w * (1.0 - c)) * err;
        let f = 0.003 + 0.3 * v4 + 0.04 * hf;
        let g = f * (PI + TAN_A * f * f);
        self.g = g;
        self.h = 1.0 / (1.0 + R * g + g * g);
        self.bleed = (t2 + (1.0 - t2) * hf) * hf * hf;
        self.inc = (self.vac - self.gain) * (1.0 / f32::from(STEP));
        self.left = STEP;
    }

    /// One sample of `v` (gated) through the SVF at `g`, `r + g`, `h`, and
    /// the bleed `b`.
    #[inline(always)]
    fn tick(&mut self, v: f32, g: f32, rg: f32, h: f32, b: f32) -> f32 {
        let hp = (v - rg * self.s1 - self.s2) * h;
        let bp = g * hp + self.s1;
        self.s1 = g * hp + bp;
        let lp = g * bp + self.s2;
        self.s2 = g * bp + lp;
        lp + (v - lp) * b
    }

    /// A NaN or infinite input left a state so: the next block starts
    /// afresh.
    #[inline(always)]
    fn settle(&mut self) {
        if !(self.s1.is_finite() && self.s2.is_finite() && self.vac.is_finite() && self.lh.is_finite())
        {
            *self = Lpg::NEW;
        }
    }
}

impl Default for Lpg {
    fn default() -> Self {
        Self::NEW
    }
}

/// The gate's block alone, for `tools/loop_count.py`'s count: bank 1 in
/// the `filter-probes` ELF, never called.
#[cfg(any(test, feature = "probe", feature = "filter-probe"))]
#[inline(never)]
pub fn lpg_probe(s: &mut Lpg, buf: &mut Buf, d: &Drive) {
    s.block(buf, d);
}
