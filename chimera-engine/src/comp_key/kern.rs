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
    Ask, Biquad, CompKey, HIGH_CORNER, Held, KeyLoad, KeyState, LOW_CORNER, Memory, PASS_FADE_S,
    Pairs, Passing, SECTIONS, Tap, Weight,
};
use crate::dsp::comp::{Listen, STEP, WholeMix, summed};
use crate::dsp::ease::step_of;
use crate::dsp::{Hz, clampf};
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

/// How many sections `key` runs the detector through; none for a key
/// that filters nothing.
#[inline(always)]
fn ranged(key: CompKey) -> Option<Sections> {
    match key {
        CompKey::Low | CompKey::High => Some(Sections::One),
        CompKey::Hit => Some(Sections::Two),
        CompKey::Full | CompKey::Part(_) => None,
    }
}

/// `key`'s sections at `sample_rate`, made when the key or the rate
/// changes; a key that filters nothing has none.
#[inline(always)]
fn sections(key: CompKey, sample_rate: u32) -> [Biquad; SECTIONS] {
    match key {
        CompKey::Low => [
            low_pass(LOW_CORNER, BUTTERWORTH_Q, sample_rate),
            Biquad::ZERO,
        ],
        CompKey::High => [
            high_pass(HIGH_CORNER, BUTTERWORTH_Q, sample_rate),
            Biquad::ZERO,
        ],
        CompKey::Hit => [shelf(sample_rate), rlb(sample_rate)],
        CompKey::Full | CompKey::Part(_) => [Biquad::ZERO; SECTIONS],
    }
}

/// `key`'s sections at `sample_rate`: for the tests, which hold them to
/// the stated filters.
#[cfg(any(test, feature = "test-support"))]
pub fn sections_of(key: CompKey, sample_rate: u32) -> Option<[Biquad; SECTIONS]> {
    ranged(key).map(|_| sections(key, sample_rate))
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

/// A value ramped across the block as the mix ramps a gain: `from`, then
/// `step` more each sample, the last landing on its end.
#[derive(Clone, Copy)]
struct Ramp {
    from: f32,
    step: f32,
}

impl Ramp {
    #[inline(always)]
    fn between(from: f32, to: f32) -> Ramp {
        Ramp {
            from,
            step: step_of((from, to), BLOCK_SIZE),
        }
    }

    /// At sample `i`, counted as `glide` counts: `step · (i + 1)`.
    #[inline(always)]
    fn at(self, i: usize) -> f32 {
        self.from + self.step * (i + 1) as f32
    }
}

/// One block of the passing Part: its bus (and side), its pair, its left
/// and right gains and the pass's weight, each ramped across the block.
#[derive(Clone, Copy)]
struct Pass<'a> {
    bus: &'a [f32; BLOCK_SIZE],
    side: Option<&'a [f32; BLOCK_SIZE]>,
    pair: DacPair,
    left: Ramp,
    right: Ramp,
    weight: Ramp,
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
        for (j, &e) in gains.iter().enumerate() {
            let i = i0 + j;
            let back = (1.0 - e) * clampf(p.weight.at(i), 0.0, 1.0);
            let (mid, side) = (p.bus[i], p.side.map_or(0.0, |s| s[i]));
            pair[2 * i] += back * (mid - side) * p.left.at(i);
            pair[2 * i + 1] += back * (mid + side) * p.right.at(i);
        }
    }
}

/// The pass this block: who passes, by how much, and what of them is in
/// the mix. A Part starts passing from nothing, and gives way only once
/// faded out, so neither end steps.
#[inline(always)]
fn pass<'a>(
    st: &mut KeyState,
    wanted: Option<PartId>,
    tap: Option<&'a Tap<'_>>,
    sample_rate: u32,
) -> Option<Pass<'a>> {
    let now = match st.passing {
        Passing::Part(p) if st.weight > Weight::NONE || wanted == Some(p) => Some(p),
        Passing::Part(_) | Passing::Nobody => {
            (st.weight, st.held) = (Weight::NONE, Held::Fresh);
            wanted
        }
    };
    st.passing = now.map_or(Passing::Nobody, Passing::Part);
    let p = now?;
    let dw = 1.0 / (PASS_FADE_S * sample_rate.max(1) as f32);
    let dw = if wanted == Some(p) { dw } else { -dw };
    let w0 = st.weight.0;
    st.weight = Weight(clampf(
        w0 + dw * BLOCK_SIZE as f32,
        Weight::NONE.0,
        Weight::ALL.0,
    ));
    // No Parts given: nothing to put back, and nothing held.
    let Some(tap) = tap else {
        st.held = Held::Fresh;
        return None;
    };
    let i = p.index();
    let to = [tap.gains[i][0].at(), tap.gains[i][1].at()];
    let from = match st.held {
        Held::Held => st.gains,
        Held::Fresh => to,
    };
    (st.gains, st.held) = (to, Held::Held);
    tap.written[i].then(|| Pass {
        bus: &tap.buses[i],
        side: tap.sides[i].as_ref(),
        pair: tap.parts[i].mix.output,
        left: Ramp::between(from[0], to[0]),
        right: Ramp::between(from[1], to[1]),
        weight: Ramp { from: w0, step: dw },
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
        comp.key_state
    } else {
        KeyState::REST
    };
    let key = params.key;
    let detector = match (key, ranged(key)) {
        (_, Some(n)) => {
            // Its sections, made on a new key or rate, start from silence.
            if st.made != key || st.rate != sample_rate {
                st.sections = sections(key, sample_rate);
                (st.made, st.rate) = (key, sample_rate);
                st.memory = [[Memory::ZERO; 2]; SECTIONS];
            }
            Detector::Ranged(n)
        }
        (CompKey::Part(p), None) => {
            st.made = CompKey::Full;
            Detector::Bus(tap.and_then(|t| t.written[p.index()].then(|| &t.buses[p.index()])))
        }
        (CompKey::Full | CompKey::Low | CompKey::High | CompKey::Hit, None) => {
            st.made = CompKey::Full;
            Detector::Mix
        }
    };
    let pass = pass(&mut st, key.part(), tap, sample_rate);
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
    // Back at FULL with the pass faded out, or bypassed: at rest, and the
    // next block is bank 1's alone.
    st.load = KeyLoad::Runs;
    comp.key_state = match (ran, key, st.passing) {
        (false, _, _) | (true, CompKey::Full, Passing::Nobody) => KeyState::REST,
        (true, CompKey::Full, Passing::Part(_))
        | (true, CompKey::Low | CompKey::High | CompKey::Hit | CompKey::Part(_), _) => st,
    };
    Pairs::of(ran)
}
