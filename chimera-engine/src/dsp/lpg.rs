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
//! - **STRIKE** pings the gate at each note-on (`ProcessPing`), rising by
//!   STRIKE × the note's velocity (never down to it), then falling back
//!   through DECAY. Over the routes it is an accent on the envelope, as
//!   LxD's STRIKE: the two sum into the vactrol, saturating at 1, and the
//!   gate never falls below the envelope. With no route (`Law::Ping`) the
//!   level is × (1 − STRIKE), so at 1 the strike alone drives it, as
//!   Plaits' with LEVEL unpatched. A note on a voice at rest starts it
//!   shut, as Plaits' from rest.
//!
//! The cadence runs across the voice's blocks, so a 64-sample block holds
//! five or six steps. Every loop is bank 2's (`voice::gate::kern`).

use crate::dsp::va::osc::Unit;
use crate::dsp::{clampf, fast_exp2};
use crate::hw::{BLOCK_SIZE, Cost};
use crate::{MidiNote, Velocity};

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
const FULL: Unit = Unit::new(1.0);
const NONE: Unit = Unit::new(0.0);

/// The gate's bill while it runs (`tools/loop_count.py` on the
/// `filter-probes` ELF, `lpg_probe`; ADR 0195). The routed loop, unrolled
/// 4×, is 127 instructions a time round (31.75 a sample), the unrouted 68
/// (17); the block's 654 (10.2 a sample) with its step, ~122 instructions
/// and 2 `vdiv`, taken 5⅓ times a block (8.3 more, 0.21 `vdiv` a sample
/// with the block's own): ceil5(50.2 × 1.606 + 12 × 0.21) = 85 routed, 60
/// not. The unit reads the routed gate 95 over ENV 2 on the VCA, the
/// unrouted 72 over 1 OP (2026-10-22, ROUTING bench, rev V at 480 MHz,
/// #735): billed 95 either way.
pub const LPG_COST: Cost = Cost(95);

/// AMP's LPG, DECAY and STRIKE as the gate reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Knobs {
    pub lpg: Unit,
    pub decay: Unit,
    pub strike: Unit,
}

impl Knobs {
    /// `OutParams::gate`'s `[LPG, DECAY, STRIKE]`.
    #[inline(always)]
    pub const fn of(k: [f32; 3]) -> Knobs {
        Knobs {
            lpg: Unit::new(k[0]),
            decay: Unit::new(k[1]),
            strike: Unit::new(k[2]),
        }
    }

    /// STRIKE at 1 under `Law::Ping`: the strike alone drives the gate,
    /// the level none.
    #[inline(always)]
    pub fn strike_alone(self, law: Law) -> bool {
        law == Law::Ping && self.strike.get() >= 1.0
    }
}

/// Plaits' COLOR, `1 − LPG`: the bleed past the filter and the cutoff's
/// floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour(f32);

impl Colour {
    #[inline(always)]
    pub fn of(lpg: Unit) -> Colour {
        Colour(1.0 - lpg.get())
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A key-gated note's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Held,
    Up,
}

/// What the gate's level follows.
#[derive(Clone, Copy, Debug)]
pub enum Level<'a> {
    /// The VCA routes' gain a sample, × AMP's VEL at the note.
    Routes(&'a Buf, Unit),
    /// No route on an engine the key ends (TAPE, VA): the key.
    Key(Key),
    /// No route on an engine whose own envelopes shape and hold the note
    /// (ALGO's carriers): open, held like a route.
    Carriers,
    /// No route on an engine that rings by itself (MODAL): open.
    Open,
}

/// How a strike meets the level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Law {
    /// The level holds the gate (the routes): the strike is an accent on
    /// it, `1 − (1 − level)(1 − strike)`, and the fall ends at the level.
    Accent,
    /// Nothing holds it (no route): the level gives way as STRIKE rises,
    /// × (1 − STRIKE), and the strike adds to it (Plaits' ping).
    Ping,
}

impl Level<'_> {
    #[inline(always)]
    pub const fn law(&self) -> Law {
        match self {
            Level::Routes(..) | Level::Carriers => Law::Accent,
            Level::Key(_) | Level::Open => Law::Ping,
        }
    }
}

/// What drives the gate this block.
#[derive(Clone, Copy, Debug)]
pub struct Drive<'a> {
    pub level: Level<'a>,
    pub knobs: Knobs,
    /// The note's velocity: × the strike's peak.
    pub hit: Velocity,
    /// The note, for the strike's rise (Plaits').
    pub note: MidiNote,
    pub fs: f32,
}

/// A note-on the gate has yet to see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Onset {
    /// On a voice at rest: the gate starts shut, as Plaits' from rest.
    Fresh,
    /// On a sounding voice: the strike rises from where the vactrol is.
    Again,
}

/// Whether the gate ran last block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// It did not: the next block starts where the plain VCA is.
    Idle,
    Running,
}

/// A value ramped across one step, a sample at a time.
#[derive(Clone, Copy, Debug)]
struct StepRamp {
    at: f32,
    inc: f32,
}

impl StepRamp {
    const fn hold(at: f32) -> StepRamp {
        StepRamp { at, inc: 0.0 }
    }

    /// Across the next step to `to`.
    #[inline(always)]
    fn aim(&mut self, to: f32) {
        self.inc = (to - self.at) * (1.0 / f32::from(STEP));
    }

    #[inline(always)]
    fn next(&mut self) -> f32 {
        self.at += self.inc;
        self.at
    }
}

/// The gate: the SVF's two states, the vactrol, the gain's and the
/// bleed's ramps, this step's coefficients and the step's samples left;
/// the key's level; the last block's LPG and STRIKE.
#[derive(Clone, Copy, Debug)]
pub struct Lpg {
    s1: f32,
    s2: f32,
    vac: f32,
    gain: StepRamp,
    g: f32,
    h: f32,
    /// The bleed ramps across a step, so a fast LPG never steps.
    bleed: StepRamp,
    key: Unit,
    /// The routes' level as the step ramps it, and the weight of the
    /// routes' own detail over that ramp: DECAY under its fade-in follows
    /// the routes a sample at a time, not a step late.
    lh: StepRamp,
    detail: f32,
    last: (Unit, Unit),
    left: u8,
    /// A strike is rising (Plaits' `ramp_up_`).
    rising: bool,
    pending: Option<Onset>,
    phase: Phase,
}

/// One block's run: LPG and STRIKE `(from, to)`, the vactrol's fade-in,
/// the fall's two coefficients a step and a strike's rise.
#[derive(Clone, Copy, Debug)]
struct Run {
    a: (Unit, Unit),
    s: (Unit, Unit),
    w: f32,
    short: f32,
    tail: f32,
    attack: f32,
}

/// Plaits' fall a step at `decay` and `colour`, rate `fs`: the short decay
/// and the tail (voice.cc).
#[inline(always)]
pub fn fall(decay: Unit, colour: Colour, fs: f32) -> (f32, f32) {
    let (d, hf) = (decay.get(), colour.get());
    let step = f32::from(STEP) / fs;
    let short = 200.0 * step * fast_exp2(-8.0 * d);
    let tail = 20.0 * step * fast_exp2(-6.0 * d + hf) - short;
    (short, tail)
}

/// Plaits' bleed past the filter at state `v`.
#[inline(always)]
fn bleed_of(v: f32, colour: Colour) -> f32 {
    let hf = colour.get();
    let t = 1.0 - v;
    let t2 = t * t;
    (t2 + (1.0 - t2) * hf) * hf * hf
}

/// voice.cc's compressed LEVEL `1.3·l/(0.3 + l)`, weighed in by LPG.
#[inline(always)]
fn compress(l: f32, lpg: Unit) -> f32 {
    l + lpg.get() * (1.3 * l / (0.3 + l) - l)
}

/// `from` to `to` at `t`, 0..1.
#[inline(always)]
fn lerp((from, to): (Unit, Unit), t: f32) -> Unit {
    Unit::new(from.get() + (to.get() - from.get()) * t)
}

impl Lpg {
    pub const NEW: Lpg = Lpg {
        s1: 0.0,
        s2: 0.0,
        vac: 0.0,
        gain: StepRamp::hold(0.0),
        g: 0.0,
        h: 0.0,
        bleed: StepRamp::hold(0.0),
        key: NONE,
        lh: StepRamp::hold(0.0),
        detail: 0.0,
        last: (NONE, NONE),
        left: 0,
        rising: false,
        pending: None,
        phase: Phase::Idle,
    };

    /// At rest at LPG `lpg` and STRIKE `strike`, as Plaits' gate before
    /// its first note.
    fn shut(lpg: Unit, strike: Unit) -> Lpg {
        Lpg {
            phase: Phase::Running,
            last: (lpg, strike),
            bleed: StepRamp::hold(bleed_of(0.0, Colour::of(lpg))),
            ..Lpg::NEW
        }
    }

    /// Where the plain VCA is at level `l`, so turning the gate on never
    /// steps: the gain the level, the bleed whole, LPG and STRIKE ramping
    /// in from 0, and no ring.
    fn at_vca(l: f32, key: Unit) -> Lpg {
        Lpg {
            vac: l,
            gain: StepRamp::hold(l),
            lh: StepRamp::hold(l),
            key,
            bleed: StepRamp::hold(1.0),
            phase: Phase::Running,
            ..Lpg::NEW
        }
    }

    /// A note-on: the next block strikes, if STRIKE is up.
    #[inline(always)]
    pub fn strike(&mut self, onset: Onset) {
        self.pending = Some(onset);
    }

    /// A block the gate did not run: the next one starts where the VCA
    /// is.
    #[inline(always)]
    pub fn idle(&mut self) {
        (self.phase, self.pending) = (Phase::Idle, None);
    }

    /// Open: rising from a strike, or above −80 dB.
    pub fn open(&self) -> bool {
        self.rising || self.vac > SHUT
    }

    /// The vactrol: the gain the step ramps to.
    pub fn level(&self) -> Unit {
        Unit::new(self.vac)
    }

    /// The level at sample `i`: the routes × VEL, the key's, or open.
    #[inline(always)]
    fn input(&mut self, d: &Drive, i: usize) -> f32 {
        match d.level {
            Level::Routes(g, vel) => clampf(g[i], 0.0, 1.0) * vel.get(),
            Level::Key(k) => {
                self.key = match k {
                    Key::Held => FULL,
                    Key::Up => Unit::new(self.key.get() - KEY_FALL),
                };
                self.key.get()
            }
            Level::Carriers | Level::Open => 1.0,
        }
    }

    /// One block of the gate on `buf`, in place. Inline: the kernel and
    /// the probe each hold one copy.
    #[inline(always)]
    pub fn block(&mut self, buf: &mut Buf, d: &Drive) {
        let Knobs {
            lpg: a,
            decay: dec,
            strike: s,
        } = d.knobs;
        let onset = self.pending.take();
        if onset == Some(Onset::Fresh) {
            *self = Lpg::shut(a, s);
        } else if self.phase == Phase::Idle {
            self.key = if matches!(d.level, Level::Key(Key::Up)) {
                NONE
            } else {
                FULL
            };
            let l = self.input(d, 0);
            *self = Lpg::at_vca(l, self.key);
        }
        if onset.is_some() && s.get() > 0.0 {
            self.rising = true;
        }
        let (a0, s0) = core::mem::replace(&mut self.last, (a, s));
        let (short, tail) = fall(dec, Colour::of(a), d.fs);
        // Plaits' `NoteToFrequency(note) · kBlockSize · 2`, while it rises.
        let attack = if self.rising {
            let note = f32::from(d.note.get());
            2.0 * f32::from(STEP) * 440.0 / d.fs * fast_exp2((note - 69.0) * (1.0 / 12.0))
        } else {
            0.0
        };
        let r = Run {
            a: (a0, a),
            s: (s0, s),
            w: (dec.get() * FADE_IN).min(1.0),
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
            let (g, h) = (self.g, self.h);
            let rg = R + g;
            let (mut gain, mut b) = (self.gain, self.bleed);
            match d.level {
                Level::Routes(lv, vel) => {
                    let (mut lh, k) = (self.lh, self.detail);
                    for (x, l) in buf[i..i + n].iter_mut().zip(&lv[i..i + n]) {
                        let (gn, bn, ln) = (gain.next(), b.next(), lh.next());
                        let e = clampf(*l, 0.0, 1.0) * vel.get();
                        *x = self.tick(*x * (gn + k * (e - ln)), g, rg, h, bn);
                    }
                    self.lh = lh;
                }
                Level::Key(_) | Level::Carriers | Level::Open => {
                    for x in &mut buf[i..i + n] {
                        let (gn, bn) = (gain.next(), b.next());
                        *x = self.tick(*x * gn, g, rg, h, bn);
                    }
                }
            }
            (self.gain, self.bleed) = (gain, b);
            self.left -= n as u8;
            i += n;
        }
        self.settle();
    }

    /// One of Plaits' steps at sample `i` of the block: the vactrol moves
    /// toward its input (`LPGEnvelope::ProcessLP`, after `ProcessPing`'s
    /// rise on a strike), and the step's gain ramp, cutoff and bleed follow
    /// it. The cutoff and bleed read the state before the move, as Plaits';
    /// the bleed ramps to its value across the step.
    #[inline(always)]
    fn step(&mut self, d: &Drive, i: usize, r: &Run) {
        let t = i as f32 * (1.0 / BLOCK_SIZE as f32);
        let a = lerp(r.a, t);
        let s = lerp(r.s, t).get();
        let hf = Colour::of(a);
        let l = self.input(d, i);
        let law = d.level.law();
        let c = compress(l, a);
        let u = match law {
            Law::Accent => c,
            Law::Ping => (1.0 - s) * c,
        };
        self.lh.aim(l);
        // Only the routes read it, and they are never `Law::Ping`.
        self.detail = 1.0 - r.w;
        let input = if self.rising {
            let x = s * d.hit.unit();
            let peak = match law {
                Law::Accent => u + x * (1.0 - u),
                Law::Ping => (u + x).min(1.0),
            };
            if self.vac >= peak {
                // Struck again above this strike's peak: no drop to it, the
                // fall takes it from here (Plaits' peak is 1, never under).
                self.rising = false;
            } else {
                self.vac += r.attack;
                if self.vac >= peak {
                    (self.vac, self.rising) = (peak, false);
                }
            }
            if self.rising { self.vac } else { u }
        } else {
            u
        };
        let v = self.vac;
        let err = input - v;
        let v2 = v * v;
        let v4 = v2 * v2;
        let c = if err > 0.0 {
            ATTACK
        } else {
            r.short + (1.0 - v4) * r.tail
        };
        self.vac = v + (1.0 - r.w * (1.0 - c)) * err;
        let f = 0.003 + 0.3 * v4 + 0.04 * hf.get();
        let g = f * (PI + TAN_A * f * f);
        self.g = g;
        self.h = 1.0 / (1.0 + R * g + g * g);
        self.bleed.aim(bleed_of(v, hf));
        self.gain.aim(self.vac);
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
        if !(self.s1.is_finite()
            && self.s2.is_finite()
            && self.vac.is_finite()
            && self.lh.at.is_finite())
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
