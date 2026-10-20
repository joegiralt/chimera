//! FUZZ: an EHX Big Muff Pi (NYC reissue) from its circuit (ADR 0156). P1
//! is SUSTAIN, P2 TONE (the LP/HP blend, the mid scoop at noon), P3 VOLUME.
//!
//! A linear Q1, two diode clipping stages (Q2, Q3: a shelf, then the 1N4148
//! pair's law, first-order ADAA at 1× on both), the tone stack, then Q4
//! folded into VOLUME. Yeh, Abel & Smith (DAFx-07): a conditioning filter, then the
//! static law. Parker, Zavalishin & Le Bivic (DAFx-16): the ADAA. Values:
//! the NYC column of PedalParts' Multi Muff BOM, gains from a nodal
//! analysis of it (the research ledger, `fuzz-sim/muff.py`). Written from
//! those; no GPL code read.
//!
//! Self-contained: its rodata is `TABLE`, its text this file's, so with
//! `bank2` the module links into `.bank2` whole (`bank2.x`,
//! `*8voice_fx4fuzz*`, ADR 0157) and bank 1 reaches `run` through
//! `FuzzDoor` (`Fuzz::play`). `fast_exp2` and `level_gain` inline into it.

use super::Ends;
use crate::dsp::ease::{at, step_of};
use crate::dsp::fast_exp2;
use crate::dsp::filter::{SUB, step_to};
use crate::hw::BLOCK_SIZE;

/// Normalised units per full scale: 1 FS is 0.5 V at the pedal (a hot
/// pickup's peak), over `b = n·Vt = 1.752 × 25.85 mV` (the 1N4148 model
/// card's n; Vt at 300 K).
const IN: f32 = 11.040_16;
/// Q1's low-frequency gain, 7.8× (17.9 dB): the nodal sim of R1 39 k,
/// R3 510 k ∥ C2 470 pF, R4 100 Ω, R5 10 k, loaded by SUSTAIN.
const Q1: f32 = 7.8;
/// Q1's one-pole, 1.0 kHz: `1 − e^(−2π·1000/48000)`.
const K_Q1: f32 = 0.122_694_23;
/// Q2's shelf from SUSTAIN's wiper: 10.3× low (20.3 dB), 1.45× high, pole
/// 2,284 Hz (R8 10 k, R10 470 k ∥ C5 470 pF, R12 390 Ω; nodal sim fit).
const Q2_LF: f32 = 10.3;
const Q2_HF: f32 = 1.45;
const K_Q2: f32 = 0.258_422;
/// The shelf with its high gain folded into the drive: `x·HF + LP·(LF − HF)`
/// is `HF·(x + LP·R2)`.
const R2: f32 = (Q2_LF - Q2_HF) / Q2_HF;
/// Q3's shelf: 12.0× low (21.6 dB), 1.45× high, pole 1,268 Hz (R13 10 k,
/// R15 470 k ∥ C8 470 pF, R14 390 Ω; nodal sim fit).
const Q3_LF: f32 = 12.0;
const Q3_HF: f32 = 1.45;
const K_Q3: f32 = 0.152_937_51;
/// The tone stack's LP, 723 Hz (R19 22 k, C11 10 nF), and the LP its HP is
/// the rest of, 1,855 Hz (C10 3.9 nF, R18 22 k).
const K_LP: f32 = 0.090_347_7;
const K_HP: f32 = 0.215_586_47;
/// SUSTAIN's span, 35.6 dB in `log2`: `d(s) = 10^(−1.78·(1 − s))`, dB-linear
/// for the 100 kA pot over R7 1.8 k (its placement assumed, lug 1 to ground).
const SPAN: f32 = 5.913_032;
/// The wet's gain at VOLUME ½: Q4's 4.4× and the law's `b` folded into one.
/// At SUSTAIN, TONE, VOLUME ½ the wet is 1.6 dB under VA INIT C3's dry, and
/// every pot at 1 (VOLUME's ×4 top) peaks at 3.99 into the 2040, under its
/// bound of 4 (RF 4). Task 14 makes the final pass (ADR 0156).
const MAKE_UP: f32 = 0.092;

/// The law's table: `u' = |x| + 2⁻⁶` over `[2⁻⁶, 2¹⁰)`, four intervals an
/// octave, indexed by its float bits. The offset starts interval 0 at
/// `u = 0`: no branch, no dead zone.
const LO_BITS: u32 = 0x3c80_0000;
const OFFSET: f32 = 1.0 / 64.0;
/// The largest float under 2¹⁰: `j ≤ 63`. Never reached in use (full
/// SUSTAIN at full scale peaks at `u ≈ 887`).
const TOP: f32 = f32::from_bits(0x447f_ffff);
/// The ADAA quotient's guard, relative to `1 + F` (F's f32 rounding).
const EPS: f32 = 1.0 / 8192.0;

/// A test's switch for the anti-aliasing: `Off` is the plain law.
#[cfg(feature = "test-support")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aa {
    On,
    Off,
}

/// FUZZ's entry in the door's table (ADR 0157).
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct FuzzDoor {
    pub run: fn(&mut Fuzz, &mut [f32; BLOCK_SIZE], &Ends, bool),
}

#[cfg(feature = "bank2")]
impl Fuzz {
    pub const DOOR: FuzzDoor = FuzzDoor { run: Self::run };
}

/// One clipping stage's ADAA memory: the last input and its `F`.
#[derive(Clone, Copy, Debug, Default)]
struct Clip {
    x: f32,
    big: f32,
}

impl Clip {
    /// The stage's law on `x`: `(F(x₋₁) − F(x)) / (x₋₁ − x)`, or `f(x)`
    /// where the step is too small for the quotient (its error under
    /// `ε·(1 + F)·max f′`, −80 dB). `AA` false is the plain law; `CLAMP`
    /// as `law`'s.
    #[inline(always)]
    fn run<const AA: bool, const CLAMP: bool>(&mut self, x: f32) -> f32 {
        let (big, f) = law::<CLAMP>(x);
        if !AA {
            return f;
        }
        let dx = x - self.x;
        // `|Δx| ≥ ε·(1 + F)`, as one multiply-subtract.
        let ok = dx.abs() - big * EPS >= EPS;
        // Divide by 1, not 0, on the fallback: no NaN forms.
        let q = (big - self.big) / if ok { dx } else { 1.0 };
        *self = Clip { x, big };
        if ok { q } else { f }
    }
}

/// The law's largest `f`, at `TOP` (`tests::the_table_is_the_law`): Q2's
/// output, so Q3's input is within the table without the clamp.
const F_MAX: f32 = 11.5;
const _: () = assert!(Q3_LF * F_MAX + OFFSET < 1024.0);

/// The 1N4148 pair at `x` (normalised): `(F, f)`, `F` even and `f` odd.
/// One diode a side (D'Angelo et al., DAFx-19): `ŷ + â·(e^ŷ − 1) = |x|`,
/// `F̂` its antiderivative, `F̂(0) = 0`. The table's Hermite cubic gives
/// both, `f` its exact derivative, so the quotient is always the mean of
/// a continuous `f`. `t` is the interval's raw 21 bits: the table's
/// coefficients carry its `2⁻²¹` powers, exactly. `CLAMP` holds `x` (and a
/// NaN) to `TOP`; Q3 skips it, its input bounded by `F_MAX`.
#[inline(always)]
fn law<const CLAMP: bool>(x: f32) -> (f32, f32) {
    let u = x.abs() + OFFSET;
    // `min` maps a NaN to TOP.
    let u = if CLAMP { u.min(TOP) } else { u };
    let r = u.to_bits() - LO_BITS;
    let c = &TABLE[(r >> 21) as usize & 63];
    let t = (r & 0x1f_ffff) as f32;
    let big = c[0] + t * (c[1] + t * (c[2] + t * c[3]));
    let f = c[4] + t * (c[5] + t * c[6]);
    (big, f.copysign(x))
}

/// The law's `f` at `x`, as the plain stage plays it.
#[inline(always)]
fn plain(x: f32) -> f32 {
    law::<true>(x).1
}

/// SUSTAIN's gain into Q1: input scale, Q1 and Q2's high gain with it.
#[inline(always)]
fn drive(s: f32) -> f32 {
    IN * Q1 * Q2_HF * fast_exp2(SPAN * (s - 1.0))
}

/// The wet's gain at VOLUME `v`.
#[inline(always)]
fn volume(v: f32) -> f32 {
    MAKE_UP * super::params::pot_level(v)
}

/// The memoryless transfer of the clipping stages at the pots' gains, for
/// the viz: SUSTAIN's drive, both laws at their shelves' low-frequency
/// gains, then VOLUME. TONE is a filter, so it isn't in it.
pub fn curve(x: f32, pots: [f32; 3]) -> f32 {
    let [s, _, v] = pots;
    let y = plain(x * drive(s) * (Q2_LF / Q2_HF));
    plain(y * Q3_LF) * volume(v)
}

/// A block's settings: the drive, MIX's dry gain, and the wet's gains on
/// the tone stack's LP and HP, `(1 − t)` and `t` with VOLUME and MIX in them.
#[derive(Clone, Copy)]
struct Coef {
    g: f32,
    dry: f32,
    lp: f32,
    hp: f32,
}

impl Coef {
    #[inline(always)]
    fn of(s: f32, t: f32, v: f32, mix: f32) -> Self {
        Self::mixed(drive(s), t, volume(v), mix)
    }

    #[inline(always)]
    fn mixed(g: f32, t: f32, out: f32, mix: f32) -> Self {
        let wet = out * mix;
        Coef {
            // Q1's one-pole input weight with it.
            g: g * K_Q1,
            dry: 1.0 - mix,
            lp: wet - wet * t,
            hp: wet * t,
        }
    }
}

/// A voice's FUZZ: Q1's, the shelves' and the tone stack's one-poles and
/// both stages' ADAA memory. Fixed size, no heap.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fuzz {
    q1: f32,
    s2: f32,
    c2: Clip,
    s3: f32,
    c3: Clip,
    lp: f32,
    hp: f32,
}

impl Fuzz {
    pub fn new() -> Self {
        Self::default()
    }

    /// `run` as the slot plays it: with `bank2`, bank 2's, through the door
    /// (ADR 0157). Closed, `buf` stays dry and the pedal's state waits.
    #[inline]
    pub fn play(&mut self, buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool) {
        #[cfg(feature = "bank2")]
        let _ = crate::bank2::DOOR_CELL.with(|d| (d.fuzz.run)(self, buf, e, glide));
        #[cfg(not(feature = "bank2"))]
        self.run(buf, e, glide);
    }

    /// `buf` through the pedal. `glide` (only a knob moves a setting) steps a
    /// moving block `STEPS` times, each sub-block the steady loop (ADR 0117).
    #[inline(never)]
    pub fn run(&mut self, buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool) {
        self.run_aa::<true>(buf, e, glide);
    }

    /// `run` with the anti-aliasing `aa`, for the aliasing tests.
    #[cfg(feature = "test-support")]
    pub fn run_with(&mut self, buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool, aa: Aa) {
        match aa {
            Aa::On => self.run_aa::<true>(buf, e, glide),
            Aa::Off => self.run_aa::<false>(buf, e, glide),
        }
    }

    /// The tone stack alone at TONE `t`, VOLUME 1/2 divided out: the
    /// kernel's blend on `buf` with the clippers bypassed, for the scoop test.
    #[cfg(feature = "test-support")]
    pub fn tone_only(&mut self, buf: &mut [f32], t: f32) {
        let c = Coef::mixed(0.0, t, 1.0, 1.0);
        for x in buf {
            self.lp += K_LP * (*x - self.lp);
            self.hp += K_HP * (*x - self.hp);
            *x = self.lp * c.lp + (*x - self.hp) * c.hp;
        }
    }

    #[inline(always)]
    fn run_aa<const AA: bool>(&mut self, buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool) {
        let ([s, t, v], m, g) = (e.pots, e.mix, e.gate);
        if g == (0.0, 0.0) {
            // Shut: the next open starts from silence, not stale memory.
            *self = Self::new();
            return;
        }
        if e.steady() {
            self.steady::<AA>(buf, Coef::of(s.1, t.1, v.1, m.1));
            return;
        }
        if g == (1.0, 1.0) && glide {
            for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
                let c = Coef::of(step_to(s, j), step_to(t, j), step_to(v, j), step_to(m, j));
                self.steady::<AA>(sub, c);
            }
            return;
        }
        // Moving: each setting's ends, stepped a sample at a time.
        let n = buf.len();
        let (d, o) = ((drive(s.0), drive(s.1)), (volume(v.0), volume(v.1)));
        let (sd, st, so, sm, sw) = (
            step_of(d, n),
            step_of(t, n),
            step_of(o, n),
            step_of(m, n),
            step_of(g, n),
        );
        for (i, x) in buf.iter_mut().enumerate() {
            let mix = at(m.0, sm, i) * at(g.0, sw, i);
            let c = Coef::mixed(at(d.0, sd, i), at(t.0, st, i), at(o.0, so, i), mix);
            *x = self.tick::<AA>(*x, &c);
        }
    }

    /// The steady loop: one setting over `buf`, two samples a time round
    /// so the stages' memory alternates registers instead of moving. Its
    /// own symbol, for `tools/loop_count.py`.
    #[inline(never)]
    fn steady<const AA: bool>(&mut self, buf: &mut [f32], c: Coef) {
        for [a, b] in buf.as_chunks_mut::<2>().0 {
            *a = self.tick::<AA>(*a, &c);
            *b = self.tick::<AA>(*b, &c);
        }
    }

    /// One sample: Q1, Q2's shelf and law, Q3's shelf and law, the tone
    /// blend `(1 − t)·LP + t·HP`, VOLUME, MIX.
    #[inline(always)]
    fn tick<const AA: bool>(&mut self, dry: f32, c: &Coef) -> f32 {
        self.q1 = self.q1 * (1.0 - K_Q1) + dry * c.g;
        self.s2 += K_Q2 * (self.q1 - self.s2);
        let y2 = self.c2.run::<AA, true>(self.q1 + R2 * self.s2);
        self.s3 += K_Q3 * (y2 - self.s3);
        let y3 = self
            .c3
            .run::<AA, false>(Q3_HF * y2 + (Q3_LF - Q3_HF) * self.s3);
        self.lp += K_LP * (y3 - self.lp);
        self.hp += K_HP * (y3 - self.hp);
        dry * c.dry + self.lp * c.lp + (y3 - self.hp) * c.hp
    }
}

/// The law's Hermite cubics, one row an interval: `F = c0 + t(c1 + t(c2 +
/// t·c3))`, `f = d0 + t(d1 + t·d2)` its derivative in `u`, then a pad (a
/// 32-byte stride). Filled on the host from the closed forms
/// (`tests::table`): `â = R_eff·Is/b = 200 k × 2.52 nA / b`, R_eff the NYC
/// stage's fit (`fuzz-sim/clipfit.py`), Is the 1N4148 model card's.
/// 64 × 32 B = 2,048 B.
#[rustfmt::skip]
static TABLE: [[f32; 8]; 64] = [
    [0e0, 0e0, 1.7156311e-18, -1.1616286e-29, 0e0, 1.8421449e-9, -1.8709339e-20, 0e0],
    [7.5453186e-6, 7.1957254e-12, 1.715558e-18, -1.1659754e-29, 3.8631756e-3, 1.8420664e-9, -1.8779347e-20, 0e0],
    [3.0180845e-5, 1.4391144e-11, 1.7154847e-18, -1.1703378e-29, 7.7261864e-3, 1.8419877e-9, -1.8849611e-20, 0e0],
    [6.790593e-5, 2.1586254e-11, 1.7154111e-18, -1.1747158e-29, 1.1589032e-2, 1.8419086e-9, -1.8920122e-20, 0e0],
    [1.2071994e-4, 5.7562108e-11, 6.861349e-18, -9.4505045e-29, 1.5451711e-2, 3.6836587e-9, -7.6105515e-20, 0e0],
    [2.7161208e-4, 8.633944e-11, 6.8607543e-18, -9.521303e-29, 2.3176568e-2, 3.6833394e-9, -7.6675656e-20, 0e0],
    [4.8285205e-4, 1.1511428e-10, 6.8601554e-18, -9.5926057e-29, 3.0900754e-2, 3.683018e-9, -7.724987e-20, 0e0],
    [7.544346e-4, 1.4388658e-10, 6.859552e-18, -9.664417e-29, 3.862426e-2, 3.682694e-9, -7.7828164e-20, 0e0],
    [1.0863544e-3, 3.4531272e-10, 2.743578e-17, -7.818506e-28, 4.634709e-2, 7.364736e-9, -3.1481465e-19, 0e0],
    [1.9311842e-3, 4.603764e-10, 2.7430861e-17, -7.9358757e-28, 6.1790675e-2, 7.3634157e-9, -3.1954057e-19, 0e0],
    [3.0172984e-3, 5.754193e-10, 2.7425868e-17, -8.0549184e-28, 7.7231474e-2, 7.3620754e-9, -3.2433385e-19, 0e0],
    [4.344653e-3, 6.904411e-10, 2.74208e-17, -8.175654e-28, 9.2669435e-2, 7.360715e-9, -3.2919534e-19, 0e0],
    [5.913203e-3, 1.610883e-9, 1.096627e-16, -6.688091e-27, 1.0810453e-1, 1.4718679e-8, -1.3464905e-18, 0e0],
    [9.77371e-3, 2.0707533e-9, 1.0962063e-16, -6.889614e-27, 1.389659e-1, 1.4713032e-8, -1.3870624e-18, 0e0],
    [1.4598448e-2, 2.5304447e-9, 1.09577284e-16, -7.0968685e-27, 1.6981527e-1, 1.4707214e-8, -1.4287883e-18, 0e0],
    [2.0387035e-2, 2.9899516e-9, 1.09532636e-16, -7.309998e-27, 2.0065226e-1, 1.4701222e-8, -1.471697e-18, 0e0],
    [2.713908e-2, 6.898537e-9, 4.37948e-16, -6.1131925e-26, 2.3147649e-1, 2.9390193e-8, -6.153741e-18, 0e0],
    [4.3531913e-2, 8.7346175e-9, 4.3756348e-16, -6.483843e-26, 2.9308513e-1, 2.9364388e-8, -6.5268504e-18, 0e0],
    [6.377356e-2, 1.0569036e-8, 4.3715566e-16, -6.875423e-26, 3.54638e-1, 2.933702e-8, -6.9210276e-18, 0e0],
    [8.786043e-2, 1.2401693e-8, 4.367232e-16, -7.288919e-26, 4.1613176e-1, 2.9307998e-8, -7.337266e-18, 0e0],
    [1.1578872e-1, 2.8464962e-8, 1.7450877e-15, -6.3634397e-25, 4.7756281e-1, 5.8555425e-8, -3.202824e-17, 0e0],
    [1.8315318e-1, 3.5775994e-8, 1.7410876e-15, -7.1403557e-25, 6.002216e-1, 5.8421204e-8, -3.593859e-17, 0e0],
    [2.6583168e-1, 4.3069225e-8, 1.7365989e-15, -8.002308e-25, 7.225817e-1, 5.827059e-8, -4.0276933e-17, 0e0],
    [3.6378464e-1, 5.034249e-8, 1.7315684e-15, -8.956047e-25, 8.446068e-1, 5.8101794e-8, -4.507726e-17, 0e0],
    [4.7696775e-1, 1.1518679e-7, 6.904475e-15, -8.464972e-24, 9.6625686e-1, 1.1583787e-7, -2.1302799e-16, 0e0],
    [7.4882007e-1, 1.4403457e-7, 6.8513696e-15, -1.049393e-23, 1.2082496e0, 1.1494691e-7, -2.640884e-16, 0e0],
    [1.0809183e0, 1.7263284e-7, 6.7855077e-15, -1.2895733e-23, 1.4481492e0, 1.1384193e-7, -3.2453175e-16, 0e0],
    [1.4726796e0, 2.0092317e-7, 6.704534e-15, -1.5680497e-23, 1.6854657e0, 1.1248341e-7, -3.9461264e-16, 0e0],
    [1.9233884e0, 4.5767428e-7, 2.644569e-14, -1.6432023e-22, 1.919625e0, 2.2184253e-7, -2.0676271e-15, 0e0],
    [2.9979947e0, 5.6642745e-7, 2.5413402e-14, -2.2206288e-22, 2.375769e0, 2.1318307e-7, -2.7941976e-15, 0e0],
    [4.2956004e0, 6.700891e-7, 2.4012635e-14, -2.7748163e-22, 2.8105574e0, 2.0143258e-7, -3.4915272e-15, 0e0],
    [5.8039284e0, 7.6714423e-7, 2.2258045e-14, -3.1774676e-22, 3.217636e0, 1.8671402e-7, -3.9981797e-15, 0e0],
    [7.5077076e0, 1.7126177e-6, 8.094799e-14, -2.645484e-21, 3.5916197e0, 3.3952048e-7, -1.6643947e-14, 0e0],
    [1.143094e1, 2.0172333e-6, 6.4127665e-14, -2.2566517e-21, 4.230445e0, 2.6897092e-7, -1.4197625e-14, 0e0],
    [1.5922607e1, 2.2564295e-6, 4.9937515e-14, -1.6872338e-21, 4.7320757e0, 2.0945312e-7, -1.06151575e-14, 0e0],
    [2.085875e1, 2.4436213e-6, 3.9381953e-14, -1.2132414e-21, 5.124645e0, 1.6517988e-7, -7.633055e-15, 0e0],
    [2.6145407e1, 5.185587e-6, 1.2572807e-13, -6.0643317e-21, 5.437482e0, 2.6367087e-7, -1.9076739e-14, 0e0],
    [3.7517395e1, 5.632915e-6, 8.837071e-14, -3.4722195e-21, 5.9065394e0, 1.8532681e-7, -1.0922658e-14, 0e0],
    [4.9687107e1, 5.9577555e-6, 6.687237e-14, -2.1824734e-21, 6.2471595e0, 1.4024153e-7, -6.8654676e-15, 0e0],
    [6.2455402e1, 6.2094427e-6, 5.330492e-14, -1.4769085e-21, 6.5110726e0, 1.11788516e-7, -4.645952e-15, 0e0],
    [7.5698364e1, 1.2827067e-5, 1.746632e-13, -7.34577e-21, 6.725077e0, 1.8314763e-7, -1.1553898e-14, 0e0],
    [1.03299095e2, 1.3462736e-5, 1.2938504e-13, -4.371689e-21, 7.058351e0, 1.3567005e-7, -6.8760724e-15, 0e0],
    [1.3206122e2, 1.3947735e-5, 1.022631e-13, -2.8788993e-21, 7.31263e0, 1.0723063e-7, -4.528117e-15, 0e0],
    [1.6173494e2, 1.4338673e-5, 8.4332836e-14, -2.0314031e-21, 7.5175943e0, 8.842939e-8, -3.1951207e-15, 0e0],
    [1.9215749e2, 2.9331177e-5, 2.8436232e-13, -1.0614585e-20, 7.688992e0, 1.4908775e-7, -8.3476495e-15, 0e0],
    [2.5482216e2, 3.0383828e-5, 2.1873098e-13, -6.647707e-21, 7.964938e0, 1.1467803e-7, -5.2279695e-15, 0e0],
    [3.1944235e2, 3.121354e-5, 1.7740539e-13, -4.5430944e-21, 8.182443e0, 9.3011515e-8, -3.572835e-15, 0e0],
    [3.856402e2, 3.1897693e-5, 1.4907182e-13, -3.2967209e-21, 8.361789e0, 7.815657e-8, -2.5926467e-15, 0e0],
    [4.5315973e2, 6.495889e-5, 5.1039153e-13, -1.7753448e-20, 8.514292e0, 1.3379608e-7, -6.9809393e-15, 0e0],
    [5.9146936e2, 6.686539e-5, 4.0042053e-13, -1.14502694e-20, 8.76418e0, 1.0496784e-7, -4.502429e-15, 0e0],
    [7.3335175e2, 6.83938e-5, 3.2916042e-13, -7.988906e-21, 8.964512e0, 8.628743e-8, -3.1413657e-15, 0e0],
    [8.781579e2, 6.966899e-5, 2.7930013e-13, -5.8872322e-21, 9.131654e0, 7.321685e-8, -2.314954e-15, 0e0],
    [1.0254385e3, 1.4152557e-4, 9.638836e-13, -3.223044e-20, 9.27502e0, 1.2633815e-7, -6.3367623e-15, 0e0],
    [1.326181e3, 1.4514313e-4, 7.640436e-13, -2.1121582e-20, 9.5121e0, 1.00144725e-7, -4.1526718e-15, 0e0],
    [1.6337336e3, 1.4806908e-4, 6.325167e-13, -1.4902782e-20, 9.7038555e0, 8.290523e-8, -2.9300062e-15, 0e0],
    [1.9469015e3, 1.5052542e-4, 5.3946935e-13, -1.1074235e-20, 9.864834e0, 7.0709326e-8, -2.1772832e-15, 0e0],
    [2.2648467e3, 3.0528402e-4, 1.8695611e-12, -6.117069e-20, 1.0003547e1, 1.2252356e-7, -6.0133236e-15, 0e0],
    [2.9127317e3, 3.123184e-4, 1.4900847e-12, -4.0434553e-20, 1.023405e1, 9.7654194e-8, -3.9748783e-15, 0e0],
    [3.5738916e3, 3.1803478e-4, 1.2382134e-12, -2.870322e-20, 1.0421364e1, 8.1147554e-8, -2.8216412e-15, 0e0],
    [4.24604e3, 3.2284952e-4, 1.0589638e-12, -2.1425933e-20, 1.0579133e1, 6.940025e-8, -2.106255e-15, 0e0],
    [4.927564e3, 6.5401685e-4, 3.6781238e-12, -1.1892421e-19, 1.0715412e1, 1.2052476e-7, -5.845363e-15, 0e0],
    [6.314217e3, 6.6787493e-4, 2.9401635e-12, -7.8979043e-20, 1.0942463e1, 9.634328e-8, -3.881978e-15, 0e0],
    [7.727054e3, 6.7916483e-4, 2.44811e-12, -5.624962e-20, 1.1127437e1, 8.021967e-8, -2.7647812e-15, 0e0],
    [9.161614e3, 6.886908e-4, 2.0967956e-12, -4.2091357e-20, 1.128351e1, 6.87078e-8, -2.0688744e-15, 0e0],
];

#[cfg(test)]
mod tests {
    extern crate std;
    use super::super::rig::{Rig, sine};
    use super::*;
    use std::vec::Vec;

    const A_HAT: f64 = 200e3 * 2.52e-9 / (1.752 * 0.02585);

    /// ŷ at `u`: Newton on `ŷ + â·(e^ŷ − 1) = u`.
    fn y_hat(u: f64) -> f64 {
        let mut y = if u > 0.0 {
            u.min(libm::log(u / A_HAT + 1.0))
        } else {
            0.0
        };
        for _ in 0..60 {
            let e = libm::exp(y);
            y -= (y + A_HAT * (e - 1.0) - u) / (1.0 + A_HAT * e);
        }
        y
    }

    /// The closed antiderivative, `F̂(0) = 0`: `(u + â + 1 − ŷ/2)·ŷ − u`.
    fn f_hat(u: f64) -> f64 {
        let y = y_hat(u);
        (u + A_HAT + 1.0 - y / 2.0) * y - u
    }

    fn table() -> [[f32; 8]; 64] {
        let node = |j: u32| f64::from(f32::from_bits(LO_BITS + (j << 21))) - 1.0 / 64.0;
        core::array::from_fn(|j| {
            let (u0, u1) = (node(j as u32), node(j as u32 + 1));
            let h = u1 - u0;
            let (f0, f1, y0, y1) = (f_hat(u0), f_hat(u1), y_hat(u0), y_hat(u1));
            let c2 = 3.0 * (f1 - f0) - h * (2.0 * y0 + y1);
            let c3 = 2.0 * (f0 - f1) + h * (y0 + y1);
            // In the raw 21 bits of `t`: exact powers of 2.
            let k = 1.0 / f64::from(1u32 << 21);
            [
                f0,
                h * y0 * k,
                c2 * k * k,
                c3 * k * k * k,
                y0,
                2.0 * c2 / h * k,
                3.0 * c3 / h * k * k,
                0.0,
            ]
            .map(|v| v as f32)
        })
    }

    /// Prints `TABLE`: `cargo test -p chimera-engine --lib print_the_table
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_the_table() {
        for r in table() {
            let row: Vec<_> = r.iter().map(|v| std::format!("{v:e}")).collect();
            std::println!("    [{}],", row.join(", "));
        }
    }

    #[test]
    fn the_table_is_the_law() {
        assert_eq!(
            table().map(|r| r.map(f32::to_bits)),
            TABLE.map(|r| r.map(f32::to_bits))
        );
        for i in 1..4000 {
            let u = libm::pow(10.0, -3.0 + 6.0 * f64::from(i) / 4000.0);
            if u >= 1000.0 {
                break;
            }
            let (big, f) = law::<true>(u as f32);
            assert!((f64::from(f) - y_hat(u)).abs() < 1e-3, "f at {u}");
            assert!(
                (f64::from(big) - f_hat(u)).abs() < 1e-2 * (1.0 + f_hat(u) * 1e-3),
                "F at {u}"
            );
        }
        assert_eq!(law::<true>(0.0), (0.0, 0.0));
        assert_eq!(law::<true>(f32::NAN).0, law::<true>(TOP).0);
        assert!(law::<true>(TOP).1 <= F_MAX);
        // The Hermite f between nodes, at the top interval.
        for i in 0..=64 {
            let u = 512.0 + 8.0 * i as f32;
            assert!(law::<true>(u.min(TOP)).1 <= F_MAX, "f at {u}");
        }
    }

    #[test]
    fn the_law_is_odd() {
        for x in [0.01f32, 0.3, 3.0, 40.0, 500.0] {
            assert_eq!(law::<true>(-x).1, -law::<true>(x).1);
            assert_eq!(law::<true>(-x).0, law::<true>(x).0);
        }
    }

    #[test]
    fn curve_is_the_clippers_odd_and_bounded() {
        let pots = [1.0, 0.5, 0.5];
        assert_eq!(curve(0.0, pots), 0.0);
        for x in [0.01f32, 0.2, 1.0, 4.0] {
            assert_eq!(curve(-x, pots), -curve(x, pots));
            assert!(curve(x, pots) <= curve(4.0 * x, pots) + 1e-6);
        }
        assert!(curve(1.0, pots) < 1.0);
    }

    fn steady(pots: [f32; 3], mix: f32) -> Ends {
        Rig::default().ends(pots, mix, true)
    }

    #[test]
    fn mix_zero_is_the_dry_bit_for_bit() {
        let mut fz = Fuzz::new();
        let e = steady([1.0, 0.7, 1.0], 0.0);
        for b in 0..8 {
            let mut x = sine(b, 0.9, 220.0);
            let dry = x;
            fz.run(&mut x, &e, false);
            assert_eq!(x.map(f32::to_bits), dry.map(f32::to_bits));
        }
    }

    /// Silence through the pedal is silence: the law is odd, `F(0) = 0`.
    #[test]
    fn silence_stays_silent() {
        let mut fz = Fuzz::new();
        let mut x = [0.0; BLOCK_SIZE];
        fz.run(&mut x, &steady([1.0, 1.0, 1.0], 1.0), false);
        assert!(x.iter().all(|v| *v == 0.0));
    }

    /// Huge inputs, either sign, come out bounded: the law's clamp.
    #[test]
    fn huge_input_stays_bounded() {
        let mut fz = Fuzz::new();
        let e = steady([1.0, 1.0, 1.0], 1.0);
        let mut x: [f32; BLOCK_SIZE] =
            core::array::from_fn(|i| if i % 7 < 3 { 1e30 } else { -1e30 });
        fz.run(&mut x, &e, false);
        assert!(x.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{x:?}");
    }

    #[test]
    fn the_shut_gate_passes_the_dry_and_forgets() {
        let mut fz = Fuzz::new();
        let mut x = sine(0, 0.9, 220.0);
        fz.run(&mut x, &steady([1.0, 0.5, 0.5], 1.0), false);
        let mut shut = sine(1, 0.9, 220.0);
        let dry = shut;
        let e = Ends {
            gate: (0.0, 0.0),
            ..steady([1.0, 0.5, 0.5], 1.0)
        };
        fz.run(&mut shut, &e, false);
        assert_eq!(shut, dry);
        assert_eq!(fz.q1, 0.0);
    }

    /// A glided block is the steady loop at each sub-block's setting.
    #[test]
    fn glide_is_sub_blocks_of_the_steady_loop() {
        let (mut a, mut b) = (Fuzz::new(), Fuzz::new());
        let e = Ends {
            pots: [(0.2, 0.9), (0.3, 0.6), (0.5, 0.4)],
            mix: (1.0, 1.0),
            gate: (1.0, 1.0),
        };
        let (mut x, mut y) = (sine(0, 0.8, 110.0), sine(0, 0.8, 110.0));
        a.run(&mut x, &e, true);
        for (j, sub) in y.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let p = e.pots.map(|r| step_to(r, j));
            b.steady::<true>(sub, Coef::of(p[0], p[1], p[2], 1.0));
        }
        assert_eq!(x, y);
    }
}
