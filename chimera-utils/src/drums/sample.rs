// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sample layer and COMBINE (design note § The voice): clips we
//! synthesise ourselves, stored at 16 kHz as 12-bit, 8-bit or a 4-bit
//! IMA-style ADPCM, read once through by TAPE's Hermite (`read::hermite`)
//! FORWARD, REVERSE or PING-PONG, then joined to the machine by a MODE.

use super::machines::Machine;
use super::parts::*;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::tape_engine::read::hermite;
use chimera_core::dsp::va::VaMode;
use chimera_core::dsp::va::combine::{Crusher, Peaks, combine};
use chimera_core::dsp::voice_fx::fold;

pub const CLIP_RATE: f32 = 16_000.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    B12,
    B8,
    Adpcm4,
    /// The first prototype's ADPCM: every clip starts at the smallest
    /// step, so a sharp attack takes ~10 samples to reach (kept for the
    /// before/after).
    Adpcm4First,
}

impl Depth {
    pub fn name(self) -> &'static str {
        match self {
            Depth::B12 => "12-bit",
            Depth::B8 => "8-bit",
            Depth::Adpcm4 => "4-bit ADPCM",
            Depth::Adpcm4First => "4-bit ADPCM first",
        }
    }

    pub fn bytes(self, n: usize) -> usize {
        match self {
            Depth::B12 => (n * 3).div_ceil(2),
            Depth::B8 => n,
            // A 4-byte header: the first prediction and step index.
            Depth::Adpcm4 | Depth::Adpcm4First => n.div_ceil(2) + 4,
        }
    }
}

/// A clip as the head reads it: 12-bit samples (−2048..2047) at 16 kHz.
#[derive(Clone)]
pub struct Clip {
    pub q: Vec<i32>,
}

/// `x` (±1 at 16 kHz) stored at `depth` and decoded back to 12-bit.
pub fn store(x: &[f32], depth: Depth) -> Clip {
    let q12 = |v: f32| (v * 2048.0).round().clamp(-2048.0, 2047.0) as i32;
    let q = match depth {
        Depth::B12 => x.iter().map(|&v| q12(v)).collect(),
        Depth::B8 => x
            .iter()
            .map(|&v| q12(v).clamp(-2048, 2047) >> 4 << 4)
            .collect(),
        Depth::Adpcm4 => {
            let q: Vec<i32> = x.iter().map(|&v| q12(v)).collect();
            let (idx, codes) = adpcm_encode_fitted(&q);
            adpcm_decode(idx, &codes)
        }
        Depth::Adpcm4First => adpcm_decode(
            0,
            &adpcm_encode(0, false, &x.iter().map(|&v| q12(v)).collect::<Vec<_>>()),
        ),
    };
    Clip { q }
}

/// The IMA-style step ladder: 89 steps, ×1.1 each, on the 12-bit scale.
fn steps() -> [i32; 89] {
    core::array::from_fn(|i| ((7.0 * 1.1f64.powi(i as i32)).round() as i32 >> 4).max(1))
}

const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

fn adpcm_step(pred: &mut i32, idx: &mut i32, code: u8, st: &[i32; 89]) {
    let s = st[*idx as usize];
    let mut d = s >> 3;
    if code & 4 != 0 {
        d += s;
    }
    if code & 2 != 0 {
        d += s >> 1;
    }
    if code & 1 != 0 {
        d += s >> 2;
    }
    *pred = (*pred + if code & 8 != 0 { -d } else { d }).clamp(-2048, 2047);
    *idx = (*idx + INDEX[(code & 7) as usize]).clamp(0, 88);
}

/// IMA's code for `diff` at step `s`.
fn adpcm_code(diff: i32, s: i32) -> u8 {
    let (mut code, mut d) = if diff < 0 { (8u8, -diff) } else { (0, diff) };
    let mut t = s;
    for bit in [4u8, 2, 1] {
        if d >= t {
            code |= bit;
            d -= t;
        }
        t >>= 1;
    }
    code
}

/// Encodes from step index `idx0`. With `look` each code looks one sample
/// ahead: of IMA's code and its two neighbours, the one whose next sample
/// also lands nearest (the decoder is IMA's, unchanged).
fn adpcm_encode(idx0: i32, look: bool, x: &[i32]) -> Vec<u8> {
    let st = steps();
    let (mut pred, mut idx) = (0i32, idx0);
    let mut out = Vec::with_capacity(x.len());
    for (n, &v) in x.iter().enumerate() {
        let base = adpcm_code(v - pred, st[idx as usize]);
        let code = if !look {
            base
        } else {
            let next = x.get(n + 1).copied().unwrap_or(v);
            let cost = |c: u8| {
                let (mut p, mut i) = (pred, idx);
                adpcm_step(&mut p, &mut i, c, &st);
                let e0 = (v - p) as i64;
                let c1 = adpcm_code(next - p, st[i as usize]);
                adpcm_step(&mut p, &mut i, c1, &st);
                let e1 = (next - p) as i64;
                e0 * e0 + e1 * e1
            };
            let mag = base & 7;
            let sign = base & 8;
            [mag, mag.saturating_sub(1), (mag + 1).min(7)]
                .into_iter()
                .map(|m| m | sign)
                .min_by_key(|&c| cost(c))
                .unwrap()
        };
        adpcm_step(&mut pred, &mut idx, code, &st);
        out.push(code);
    }
    out
}

/// The clip encoded from the step index that codes it best (its first
/// 32 ms weigh in), stored in the header: the attack starts at the right
/// step instead of climbing to it.
fn adpcm_encode_fitted(x: &[i32]) -> (i32, Vec<u8>) {
    let head = &x[..x.len().min(512)];
    let err = |idx: i32| {
        let d = adpcm_decode(idx, &adpcm_encode(idx, true, head));
        d.iter()
            .zip(head)
            .map(|(a, b)| ((a - b) as i64).pow(2))
            .sum::<i64>()
    };
    let idx = (0..89).min_by_key(|&i| err(i)).unwrap();
    (idx, adpcm_encode(idx, true, x))
}

fn adpcm_decode(idx0: i32, codes: &[u8]) -> Vec<i32> {
    let st = steps();
    let (mut pred, mut idx) = (0i32, idx0);
    codes
        .iter()
        .map(|&c| {
            adpcm_step(&mut pred, &mut idx, c, &st);
            pred
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Forward,
    Reverse,
    PingPong,
}

/// TAPE's WALK head, once through a clip: Hermite on the 12-bit taps,
/// `inc` clip samples an output sample.
pub struct Head<'a> {
    clip: &'a Clip,
    pos: f64,
    dir: f64,
    turned: bool,
    mode: Dir,
    pub done: bool,
}

impl<'a> Head<'a> {
    pub fn new(clip: &'a Clip, mode: Dir) -> Self {
        let last = clip.q.len() as f64 - 1.0;
        let (pos, dir) = match mode {
            Dir::Reverse => (last, -1.0),
            _ => (0.0, 1.0),
        };
        Self {
            clip,
            pos,
            dir,
            turned: false,
            mode,
            done: false,
        }
    }

    /// The next sample at `rate` (1 the clip's own); a negative rate reads
    /// backward (FM SMP ← MCH through zero).
    pub fn tick(&mut self, rate: f32) -> f32 {
        if self.done {
            return 0.0;
        }
        let q = &self.clip.q;
        let last = q.len() as f64 - 1.0;
        let tap = |i: i64| q[i.clamp(0, last as i64) as usize];
        let i = self.pos.floor() as i64;
        let y = hermite(
            [tap(i - 1), tap(i), tap(i + 1), tap(i + 2)],
            (self.pos - i as f64) as f32,
        );
        self.pos += self.dir * rate as f64 * (CLIP_RATE / FS) as f64;
        if self.pos > last || self.pos < 0.0 {
            if self.mode == Dir::PingPong && !self.turned {
                self.turned = true;
                self.dir = -self.dir;
                self.pos = self.pos.clamp(0.0, last);
            } else {
                self.done = true;
            }
        }
        y
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Mix,
    Ring,
    Am,
    Xor,
    Bits,
    Max,
    FoldIntoMch,
    FoldIntoSmp,
    FmMch,
    FmSmp,
}

impl Mode {
    pub const ALL: [Mode; 10] = [
        Mode::Mix,
        Mode::Ring,
        Mode::Am,
        Mode::Xor,
        Mode::Bits,
        Mode::Max,
        Mode::FoldIntoMch,
        Mode::FoldIntoSmp,
        Mode::FmMch,
        Mode::FmSmp,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Mix => "MIX",
            Mode::Ring => "RING c(1+m)",
            Mode::Am => "AM",
            Mode::Xor => "XOR",
            Mode::Bits => "BITS",
            Mode::Max => "MAX",
            Mode::FoldIntoMch => "FOLD MCH <- SMP",
            Mode::FoldIntoSmp => "FOLD SMP <- MCH",
            Mode::FmMch => "FM MCH <- SMP",
            Mode::FmSmp => "FM SMP <- MCH",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Mode::Mix => "mix",
            Mode::Ring => "ring",
            Mode::Am => "am",
            Mode::Xor => "xor",
            Mode::Bits => "bits",
            Mode::Max => "max",
            Mode::FoldIntoMch => "fold-mch",
            Mode::FoldIntoSmp => "fold-smp",
            Mode::FmMch => "fm-mch",
            Mode::FmSmp => "fm-smp",
        }
    }

    pub fn listen(self) -> &'static str {
        match self {
            Mode::Mix => "the two layers side by side, no interaction: the reference",
            Mode::Ring => {
                "the clip's texture imprinted on the machine's body; the body never drops out when the clip ends"
            }
            Mode::Am => "VA's AM: gentler imprint, the body dimmed by half at full AMOUNT",
            Mode::Xor => "bitwise grit over the overlap, clean body once the clip is gone",
            Mode::Bits => "both layers crushed together; the body stays crushed after the clip",
            Mode::Max => "the louder layer wins sample by sample: a hard, buzzy edge on the attack",
            Mode::FoldIntoMch => {
                "the clip pushes the machine through the folder: bright harmonics only during the clip"
            }
            Mode::FoldIntoSmp => "the machine folds the clip: clip-length, tonal grit",
            Mode::FmMch => {
                "the clip shakes the machine's pitch: a noisy, metallic attack, clean tail"
            }
            Mode::FmSmp => {
                "the machine moves the clip's read rate (through zero): a warbled, pitched clip; hit ends with the clip"
            }
        }
    }
}

/// One pad: a machine and a clip, each at its LEVEL, through MODE at
/// AMOUNT `x`. `secs` long.
pub fn pad(
    mut m: Option<Box<dyn Machine>>,
    clip: Option<(&Clip, Dir)>,
    levels: (f32, f32),
    mode: Mode,
    x: f32,
    secs: f32,
) -> Vec<f32> {
    let n = (secs * FS) as usize;
    let mut out = Vec::with_capacity(n + B);
    let mut head = clip.map(|(c, d)| Head::new(c, d));
    let mut crush = Crusher::NEW;
    crush.set(x);
    let pk = Peaks { a: 1.0, b: 1.0 };
    let fold_at = |v: f32| 0.7 * fold::curve(v, [0.3 + 0.7 * x, 0.5, fold::UNITY_LEVEL]);
    while out.len() < n {
        // The sample ahead of the machine (FM MCH ← SMP reads it), except
        // FM SMP ← MCH, where the machine's block moves the head's read.
        let mut s = [0.0; B];
        let read = |h: &mut Option<Head>, s: &mut Blk, c: Option<&Blk>| {
            if let Some(h) = h {
                for (i, v) in s.iter_mut().enumerate() {
                    let rate = c.map_or(1.0, |c| 1.0 + 4.0 * x * c[i]);
                    *v = levels.1 * h.tick(rate);
                }
            }
        };
        if mode != Mode::FmSmp {
            read(&mut head, &mut s, None);
        }
        let mut c = [0.0; B];
        if let Some(m) = &mut m {
            let fm = (mode == Mode::FmMch).then(|| s.map(|v| v * x));
            m.block(&mut c, fm.as_ref());
            c.iter_mut().for_each(|v| *v *= levels.0);
        }
        if mode == Mode::FmSmp {
            read(&mut head, &mut s, Some(&c));
        }
        for i in 0..B {
            let (a, b) = (c[i], s[i]);
            let y = match mode {
                Mode::Mix => a + b,
                Mode::Ring => a * (1.0 + 2.0 * x * b),
                Mode::Am => combine(VaMode::Am, a, b, x, 1.0, pk, &crush),
                Mode::Xor => combine(VaMode::Xor, a, b, x, 1.0, pk, &crush),
                Mode::Bits => 2.0 * combine(VaMode::Bits, a, b, x, 1.0, pk, &crush),
                Mode::Max => combine(VaMode::Max, a, b, x, 1.0, pk, &crush),
                Mode::FoldIntoMch => fold_at(a * (1.0 + 3.0 * x * b.abs())),
                Mode::FoldIntoSmp => fold_at(b * (1.0 + 3.0 * x * a.abs())),
                // The carrier alone, as VA's FM: the modulator is not heard.
                Mode::FmMch => a,
                Mode::FmSmp => b,
            };
            out.push(y);
        }
    }
    out.truncate(n);
    out
}

/// The clips: a noise burst, a click, and a machine hit resampled to
/// 16 kHz. All ours, nothing third-party.
pub fn clips(snare: &[f32]) -> Vec<(&'static str, &'static str, Vec<f32>)> {
    let n16 = |ms: f32| (ms * CLIP_RATE / 1000.0) as usize;
    let mut rng = Lcg(0xC1A9);
    // NOISE BURST: 40 ms of white, a 1 ms rise and a 10 ms fall, one-pole
    // softened.
    let mut lp = 0.0f32;
    let burst = (0..n16(40.0))
        .map(|i| {
            let t = i as f32 / CLIP_RATE;
            let e = (t / 0.001).min(1.0) * (-t / 0.010).exp();
            lp += 0.6 * (rng.next() - lp);
            0.9 * e * lp
        })
        .collect();
    // CLICK: 6 ms, a decaying 3.1 kHz sine with a sharp first sample.
    let click = (0..n16(6.0))
        .map(|i| {
            let t = i as f32 / CLIP_RATE;
            let s = (core::f32::consts::TAU * 3100.0 * t).sin() * (-t / 0.0012).exp();
            0.85 * if i == 0 { 1.0 } else { s }
        })
        .collect();
    // SNARE HIT: TONE SNARE rendered at 48 kHz, low-passed at 7 kHz and
    // kept every third sample: 120 ms.
    let mut f = Flt::clean(FilterMode::Lp24, 7000.0, 0.0);
    let mut y = snare.to_vec();
    y.resize(y.len().div_ceil(B) * B, 0.0);
    for b in y.as_chunks_mut::<B>().0 {
        f.run(b, 7000.0);
    }
    let hit = y.iter().step_by(3).take(n16(120.0)).copied().collect();
    vec![
        (
            "noise-burst",
            "40 ms of softened white noise, 10 ms fall",
            burst,
        ),
        (
            "click",
            "6 ms decaying 3.1 kHz ping with a sharp first sample",
            click,
        ),
        (
            "snare-hit",
            "120 ms of our TONE SNARE resampled to 16 kHz",
            hit,
        ),
    ]
}
