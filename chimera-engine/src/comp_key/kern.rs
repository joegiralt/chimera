// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! KEY's block (ADR 0303), bank 2's: an S4 group (`bank2.x`,
//! `*8comp_key4kern*`), reached through `KeyDoor`. `run` is the
//! compressor's own loop (`MasterComp::process_keyed`, inlined here) with
//! the detector's level and the pass brought by `Keyed`. Everything here
//! inlines into `run`, so the module links into its group whole.
//!
//! LOW and HIGH: second-order Butterworth sections at their corners. HIT:
//! ITU-R BS.1770-4's K-weighting, its shelf then its high-pass, from the
//! analogue prototypes its 48 kHz coefficients come from, so any rate
//! plays the same curve. PART n: the Part's bus as the mix has it; after
//! the gain, `(1 − e) · weight ·` the Part's own share of its pair is put
//! back, so it leaves as it came.

use chimera_hal::BLOCK_SIZE;

use super::{
    Ask, Biquad, HIGH_CORNER, KeyState, LOW_CORNER, Made, Memory, PASS_FADE_S, Pairs, PartShare,
    Passing, Ranged, SECTIONS, Source, Tap, Weight,
};
use crate::dsp::Hz;
use crate::dsp::comp::{Listen, STEP, WholeMix, summed};
use crate::dsp::ease::step_of;
use crate::hw::DAC_PAIRS;
use crate::part::DacPair;
use crate::project::PartId;

/// A Butterworth section's Q.
const BUTTERWORTH_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// K-weighting's shelf (BS.1770-4's first stage): its corner, Q, gain
/// `10^(4 dB / 20)` and that gain to the 0.4997 its mid-point takes.
const SHELF: Hz = Hz::new(1_681.974_5);
const SHELF_Q: f32 = 0.707_175_24;
const SHELF_GAIN: f32 = 1.584_864_7;
const SHELF_MID: f32 = 1.258_720_9;
/// K-weighting's high-pass (its second stage, the RLB curve).
const RLB: Hz = Hz::new(38.135_47);
const RLB_Q: f32 = 0.500_327_05;

/// `tan x`'s series to x⁷, for corners well under Nyquist (the shelf at
/// 48 kHz is 0.11 rad, where the next term is 5e-10 of it): inlined
/// here, not libm's `tanf` in bank 1.
#[inline(always)]
fn tan_small(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 + x2 * (1.0 / 3.0 + x2 * (2.0 / 15.0 + x2 * (17.0 / 315.0))))
}

/// The bilinear transform's `tan(π f / fs)` and what a section of `q`
/// divides by.
#[inline(always)]
fn warp(f: Hz, q: f32, sample_rate: u32) -> (f32, f32, f32) {
    let k = tan_small(core::f32::consts::PI * f.get() / sample_rate.max(1) as f32);
    let kq = k / q;
    (k, kq, 1.0 / (1.0 + kq + k * k))
}

/// A second-order low-pass at `f`.
#[inline(always)]
fn low_pass(f: Hz, q: f32, sample_rate: u32) -> Biquad {
    let (k, kq, n) = warp(f, q, sample_rate);
    let b = k * k * n;
    Biquad {
        b: [b, 2.0 * b, b],
        a: [2.0 * (k * k - 1.0) * n, (1.0 - kq + k * k) * n],
    }
}

/// A second-order high-pass at `f`.
#[inline(always)]
fn high_pass(f: Hz, q: f32, sample_rate: u32) -> Biquad {
    let (k, kq, n) = warp(f, q, sample_rate);
    Biquad {
        b: [n, -2.0 * n, n],
        a: [2.0 * (k * k - 1.0) * n, (1.0 - kq + k * k) * n],
    }
}

/// K-weighting's shelf.
#[inline(always)]
fn shelf(sample_rate: u32) -> Biquad {
    let (k, kq, n) = warp(SHELF, SHELF_Q, sample_rate);
    Biquad {
        b: [
            (SHELF_GAIN + SHELF_MID * kq + k * k) * n,
            2.0 * (k * k - SHELF_GAIN) * n,
            (SHELF_GAIN - SHELF_MID * kq + k * k) * n,
        ],
        a: [2.0 * (k * k - 1.0) * n, (1.0 - kq + k * k) * n],
    }
}

/// K-weighting's high-pass: the standard leaves its numerator 1, −2, 1.
#[inline(always)]
fn rlb(sample_rate: u32) -> Biquad {
    let (k, kq, n) = warp(RLB, RLB_Q, sample_rate);
    Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) * n, (1.0 - kq + k * k) * n],
    }
}

/// How many sections a ranged key runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sections {
    One,
    Two,
}

impl Ranged {
    /// How many sections the detector runs through.
    #[inline(always)]
    fn stages(self) -> Sections {
        match self {
            Ranged::Low | Ranged::High => Sections::One,
            Ranged::Hit => Sections::Two,
        }
    }

    /// Its sections at `sample_rate`, made when the key or the rate
    /// changes.
    #[inline(always)]
    fn sections(self, sample_rate: u32) -> [Biquad; SECTIONS] {
        match self {
            Ranged::Low => [
                low_pass(LOW_CORNER, BUTTERWORTH_Q, sample_rate),
                Biquad::ZERO,
            ],
            Ranged::High => [
                high_pass(HIGH_CORNER, BUTTERWORTH_Q, sample_rate),
                Biquad::ZERO,
            ],
            Ranged::Hit => [shelf(sample_rate), rlb(sample_rate)],
        }
    }
}

/// `key`'s sections at `sample_rate`: for the tests, which hold them to
/// the stated filters.
#[cfg(any(test, feature = "test-support"))]
pub fn sections_of(key: Ranged, sample_rate: u32) -> [Biquad; SECTIONS] {
    key.sections(sample_rate)
}

impl Biquad {
    /// `x` through the section, its memory `m` stepped.
    #[inline(always)]
    fn step(&self, m: &mut Memory, x: f32) -> f32 {
        let y = self.b[0] * x + m.0[0];
        m.0[0] = self.b[1] * x - self.a[0] * y + m.0[1];
        m.0[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The louder of a chunk's peak so far and `x`'s level: a compare and a
/// select, where `f32::max`'s NaN rules take seven instructions. A NaN
/// `x` leaves the peak, as `max` would.
#[inline(always)]
fn louder(peak: f32, x: f32) -> f32 {
    let x = x.abs();
    if x > peak { x } else { peak }
}

/// What the detector listens to this block.
#[derive(Clone, Copy)]
enum Detector<'a> {
    /// FULL, while a pass fades out.
    Mix,
    /// LOW, HIGH or HIT: the mix through the state's sections.
    Ranged(Sections),
    /// PART n's bus; `None` silent (no voices, or no Parts given).
    Bus(Option<&'a [f32; BLOCK_SIZE]>),
}

/// One block of the passing Part: its bus (and side), its pair, its
/// share of the pair as the mix played it, and the pass's weight from the
/// block's start to its end.
#[derive(Clone, Copy)]
struct Pass<'a> {
    bus: &'a [f32; BLOCK_SIZE],
    side: Option<&'a [f32; BLOCK_SIZE]>,
    pair: DacPair,
    share: PartShare,
    weight: (Weight, Weight),
}

/// `Listen` for every KEY but FULL.
struct Keyed<'a, 's> {
    detector: Detector<'a>,
    pass: Option<Pass<'a>>,
    state: &'s mut KeyState,
}

impl Listen<DAC_PAIRS> for Keyed<'_, '_> {
    /// One loop: the fade's few instructions a chunk for half the flash.
    const SETTLED_LOOP: bool = false;

    #[inline(always)]
    fn peak(&mut self, out: &[[f32; 2 * BLOCK_SIZE]; DAC_PAIRS], i0: usize) -> f32 {
        let mut peak = 0.0f32;
        match self.detector {
            Detector::Mix => return WholeMix.peak(out, i0),
            Detector::Ranged(n) => {
                let (s, m) = (&self.state.sections, &mut self.state.memory);
                for i in i0..i0 + STEP {
                    let (mut l, mut r) = summed(out, i);
                    l = s[0].step(&mut m[0][0], l);
                    r = s[0].step(&mut m[0][1], r);
                    if n == Sections::Two {
                        l = s[1].step(&mut m[1][0], l);
                        r = s[1].step(&mut m[1][1], r);
                    }
                    peak = louder(louder(peak, l), r);
                }
            }
            Detector::Bus(Some(bus)) => {
                for &x in &bus[i0..i0 + STEP] {
                    peak = louder(peak, x);
                }
            }
            Detector::Bus(None) => {}
        }
        peak
    }

    #[inline(always)]
    fn pass(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS],
        i0: usize,
        gains: &[f32; STEP],
    ) {
        let Some(p) = self.pass else {
            return;
        };
        let pair = &mut out[p.pair.index()];
        // Both ends are `Weight`s, so every sample between is in 0..=1.
        let (w0, w1) = (p.weight.0.get(), p.weight.1.get());
        let dw = step_of((w0, w1), BLOCK_SIZE);
        for (j, &e) in gains.iter().enumerate() {
            let i = i0 + j;
            let back = (1.0 - e) * (w0 + dw * (i + 1) as f32);
            let (l, r) = p.share.at(i, p.bus[i], p.side.map_or(0.0, |s| s[i]));
            pair[2 * i] += back * l;
            pair[2 * i + 1] += back * r;
        }
    }
}

/// The pass after this block: who passes and by how much. A Part starts
/// from nothing and fades toward all while it is the key, toward nothing
/// once it is not, and gives way only once faded out, so neither end
/// steps. With it, this block's weight at its start.
#[inline(always)]
fn fade(passing: Passing, wanted: Option<PartId>, sample_rate: u32) -> (Passing, Weight) {
    let (id, from) = match (passing, wanted) {
        (Passing::Part { id, weight }, _) if weight > Weight::NONE || wanted == Some(id) => {
            (id, weight)
        }
        (Passing::Part { .. } | Passing::Nobody, Some(id)) => (id, Weight::NONE),
        (Passing::Part { .. } | Passing::Nobody, None) => return (Passing::Nobody, Weight::NONE),
    };
    let block = BLOCK_SIZE as f32 / (PASS_FADE_S * sample_rate.max(1) as f32);
    let to = if wanted == Some(id) {
        from.get() + block
    } else {
        from.get() - block
    };
    let weight = Weight::new(to);
    (Passing::Part { id, weight }, from)
}

/// This block of the Part `passing` names, as the mix has it: none with
/// nobody passing, no Parts given, or the Part without voices.
#[inline(always)]
fn pass<'a>(passing: Passing, from: Weight, tap: Option<&'a Tap<'_>>) -> Option<Pass<'a>> {
    let Passing::Part { id, weight } = passing else {
        return None;
    };
    let (tap, i) = (tap?, id.index());
    Some(Pass {
        bus: &tap.buses[i],
        side: tap.sides[i].as_ref(),
        pair: tap.parts[i].mix.output,
        share: tap.shares[i]?,
        weight: (from, weight),
    })
}

/// The compressor's block under any KEY but FULL, and under FULL while a
/// pass fades out. `KeyDoor`'s entry.
pub(super) fn run(ask: Ask<'_, '_>) -> Pairs {
    let Ask {
        comp,
        out,
        params,
        sample_rate,
        tap,
    } = ask;
    // From bypass, KEY starts from rest.
    let mut st = if comp.is_running() {
        *comp.key_state()
    } else {
        KeyState::REST
    };
    let source = params.key.source();
    let detector = match source {
        Source::Ranged(key) => {
            // Its sections, made on a new key or rate, start from silence.
            let made = Made::For {
                key,
                rate: sample_rate,
            };
            if st.made != made {
                st.sections = key.sections(sample_rate);
                st.memory = [[Memory::ZERO; 2]; SECTIONS];
                st.made = made;
            }
            Detector::Ranged(key.stages())
        }
        Source::Part(p) => {
            st.made = Made::Nothing;
            let i = p.index();
            Detector::Bus(tap.and_then(|t| t.shares[i].map(|_| &t.buses[i])))
        }
        Source::Mix => {
            st.made = Made::Nothing;
            Detector::Mix
        }
    };
    let wanted = match source {
        Source::Part(p) => Some(p),
        Source::Mix | Source::Ranged(_) => None,
    };
    let (passing, from) = fade(st.passing, wanted, sample_rate);
    st.passing = passing;
    let pass = pass(passing, from, tap);
    let mut keyed = Keyed {
        detector,
        pass,
        state: &mut st,
    };
    let ran = comp.process_keyed(out, params, sample_rate, &mut keyed);
    // An inf or NaN in the mix would sit in a section's memory for good.
    if !st
        .memory
        .iter()
        .flatten()
        .all(|m| (m.0[0] + m.0[1]).is_finite())
    {
        st.memory = [[Memory::ZERO; 2]; SECTIONS];
    }
    // Bypassed, or back at FULL with the pass faded out: at rest, and the
    // next block is bank 1's alone.
    comp.set_key_state(match (ran, source) {
        (false, _) => KeyState::REST,
        (true, Source::Mix) if st.at_rest() => KeyState::REST,
        (true, Source::Mix | Source::Ranged(_) | Source::Part(_)) => st,
    });
    Pairs::of(ran)
}
