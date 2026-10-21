//! VA NOISE (ADR 0197): one source a voice, mixed after MODE beside SUB,
//! before the filter. KIND picks the colour, LEVEL the amount.
//!
//! - WHITE: a uniform draw a sample.
//! - PINK: white through Paul Kellet's three-pole economy filter, −3 dB an
//!   octave from about 16 Hz up.
//! - RED: white through a one-pole low-pass at 150 Hz, less its own
//!   one-pole at 25 Hz: −6 dB an octave above 150 Hz, and no sub-20 Hz
//!   wander for the DC blocker to miss.
//! - DIGITAL: a sample-and-hold, a fresh draw `STEPS` times each of OSC
//!   1's cycles: it brightens up the keyboard. The steps are naive (the
//!   grit is the point), as XOR's and BITS' are.
//!
//! Every KIND sits at the waves' RMS `R` before LEVEL. The stream is an
//! LCG (Numerical Recipes' constants): a step is one multiply-add, and
//! every voice's stream is its own, seeded by its slot.

use super::osc::R;
use crate::block::DiskCode;
use chimera_hal::BLOCK_SIZE;

/// The noise's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum NoiseKind {
    #[default]
    White = 0,
    Pink = 1,
    Red = 2,
    Digital = 3,
}

impl NoiseKind {
    pub const ALL: [NoiseKind; 4] = [
        NoiseKind::White,
        NoiseKind::Pink,
        NoiseKind::Red,
        NoiseKind::Digital,
    ];
}

/// KIND's names, in `NoiseKind`'s order.
pub const KIND_NAMES: [&str; 4] = ["WHITE", "PINK", "RED", "DIGITAL"];

impl DiskCode for NoiseKind {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        KIND_NAMES[self as usize]
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        NoiseKind::ALL.get(usize::from(c)).copied()
    }
}

/// The LCG.
const A: u32 = 1_664_525;
const C: u32 = 1_013_904_223;

/// A draw's word to the waves' RMS: a uniform ±√3·R has RMS R.
const WORD: f32 = R * 1.732_050_8 / 2_147_483_648.0;

/// PINK: Kellet's economy filter, poles and gains, and its power make-up
/// (`1/√(d² + 2dΣc + ΣΣ cⱼcₖ/(1 − aⱼaₖ))`).
const PK_A: [f32; 3] = [0.997_65, 0.963, 0.57];
const PK_C: [f32; 3] = [0.099_046, 0.296_516_4, 1.052_691_3];
const PK_D: f32 = 0.1848;
const PK_N: f32 = 0.335_681_5;

/// RED: the low-pass at 150 Hz and the high-pass's at 25 Hz (`1 −
/// e^(−2πf/48k)`), and the band's power make-up.
const RD_G: [f32; 2] = [0.019_443_44, 0.003_267_144];
const RD_N: f32 = 10.919_244;

/// DIGITAL's draws a cycle of OSC 1.
pub const STEPS: f32 = 16.0;

/// Each KIND's gain from its filter's output to RMS R.
const fn make_up(k: NoiseKind) -> f32 {
    match k {
        NoiseKind::White | NoiseKind::Digital => WORD,
        NoiseKind::Pink => WORD * PK_N,
        NoiseKind::Red => WORD * RD_N,
    }
}

/// LEVEL's law: the noise's RMS, a share of the waves' `R`, at LEVEL `l`
/// and KIND `k`. Squared, so the knob's lower half is the breath (LEVEL ½
/// is −12 dB on full), and each KIND's full is `FULL`.
pub fn level(l: f32, k: NoiseKind) -> f32 {
    l * l * FULL[k as usize]
}

/// Each KIND's RMS at full LEVEL, a share of `R`, set by loudness (BS.1770
/// LUFS, a held note, filter open): at full the noise is as loud as INIT
/// VA's two saws (within 0.3 LU) and 4 to 5 LU under the owner's brighter
/// single oscillator, never over them (ADR 0197). Mixed like SUB, the
/// oscillators give way by `1/√(1 + SUB² + z²)`, so the filter's input
/// stays within 1.7 LU of LEVEL 0.
pub const FULL: [f32; 4] = [0.64, 0.8, 0.83, 0.69];

/// One voice's noise: the stream, the hold clock, the held draw and the
/// filters' states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    s: u32,
    h: f32,
    v: f32,
    b: [f32; 3],
    kind: NoiseKind,
}

impl Noise {
    pub const fn new(seed: u32) -> Self {
        Self {
            s: seed,
            h: 0.0,
            v: 0.0,
            b: [0.0; 3],
            kind: NoiseKind::White,
        }
    }

    /// `out[i] += w(i)·noise(i)` over a block: `w` is the level's lane
    /// `(from, step)` times the chain's norm, a quadratic `(n, dn, ddn)`
    /// (`Lanes::norm`); `inc`, OSC 1's cycles a sample (DIGITAL's clock).
    /// A new KIND starts its filter from rest. Through bank 2's door with
    /// `bank2`: closed, it adds nothing.
    #[inline(always)]
    pub fn add(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        kind: NoiseKind,
        z: (f32, f32),
        norm: (f32, f32, f32),
        inc: f32,
    ) {
        #[cfg(feature = "bank2")]
        crate::bank2::DOOR_CELL.with(|d| (d.noise.add)(self, out, kind, z, norm, inc));
        #[cfg(not(feature = "bank2"))]
        kern::add(self, out, kind, z, norm, inc);
    }
}

/// NOISE's pass as the door carries it (`Noise::add`'s arguments).
pub type NoiseAdd =
    fn(&mut Noise, &mut [f32; BLOCK_SIZE], NoiseKind, (f32, f32), (f32, f32, f32), f32);

/// NOISE's entry in bank 2's door (ADR 0197).
#[repr(C)]
pub struct NoiseDoor {
    pub add: NoiseAdd,
}

impl NoiseDoor {
    pub const DOOR: NoiseDoor = NoiseDoor { add: kern::add };
}

/// The pass, linked into bank 2 with `bank2` (`bank2.x`,
/// `*2va5noise4kern*`): every KIND's loop. Free functions, so each symbol
/// names this module.
mod kern {
    use super::{A, C, Noise, NoiseKind, PK_A, PK_C, PK_D, RD_G, STEPS, make_up};
    use chimera_hal::BLOCK_SIZE;

    type Buf = [f32; BLOCK_SIZE];

    /// The door's entry (`Noise::add`).
    #[inline(never)]
    pub(super) fn add(
        nz: &mut Noise,
        out: &mut Buf,
        kind: NoiseKind,
        z: (f32, f32),
        norm: (f32, f32, f32),
        inc: f32,
    ) {
        if kind != nz.kind {
            (nz.kind, nz.b, nz.h) = (kind, [0.0; 3], 1.0);
        }
        let g = make_up(kind);
        let (z, dz) = (z.0 * g, z.1 * g);
        match kind {
            NoiseKind::White => white(nz, out, (z, dz), norm),
            NoiseKind::Pink => pink(nz, out, (z, dz), norm),
            NoiseKind::Red => red(nz, out, (z, dz), norm),
            NoiseKind::Digital => digital(nz, out, (z, dz), norm, inc * STEPS),
        }
    }

    #[inline(always)]
    fn white(nz: &mut Noise, out: &mut Buf, z: (f32, f32), n: (f32, f32, f32)) {
        let mut s = nz.s;
        let (dz, ddn) = (z.1, n.2);
        let (mut z, mut n, mut dn) = (z.0, n.0, n.1);
        for o in out.iter_mut() {
            z += dz;
            s = s.wrapping_mul(A).wrapping_add(C);
            *o += z * n * s as i32 as f32;
            n += dn;
            dn += ddn;
        }
        nz.s = s;
    }

    #[inline(always)]
    fn pink(nz: &mut Noise, out: &mut Buf, z: (f32, f32), n: (f32, f32, f32)) {
        let (mut s, [mut b0, mut b1, mut b2]) = (nz.s, nz.b);
        let (dz, ddn) = (z.1, n.2);
        let (mut z, mut n, mut dn) = (z.0, n.0, n.1);
        for o in out.iter_mut() {
            z += dz;
            s = s.wrapping_mul(A).wrapping_add(C);
            let x = s as i32 as f32;
            b0 = PK_A[0] * b0 + PK_C[0] * x;
            b1 = PK_A[1] * b1 + PK_C[1] * x;
            b2 = PK_A[2] * b2 + PK_C[2] * x;
            *o += z * n * (b0 + b1 + b2 + PK_D * x);
            n += dn;
            dn += ddn;
        }
        (nz.s, nz.b) = (s, [b0, b1, b2]);
    }

    #[inline(always)]
    fn red(nz: &mut Noise, out: &mut Buf, z: (f32, f32), n: (f32, f32, f32)) {
        let (mut s, [mut lp, mut hp, b2]) = (nz.s, nz.b);
        let (dz, ddn) = (z.1, n.2);
        let (mut z, mut n, mut dn) = (z.0, n.0, n.1);
        for o in out.iter_mut() {
            z += dz;
            s = s.wrapping_mul(A).wrapping_add(C);
            lp += RD_G[0] * (s as i32 as f32 - lp);
            hp += RD_G[1] * (lp - hp);
            *o += z * n * (lp - hp);
            n += dn;
            dn += ddn;
        }
        (nz.s, nz.b) = (s, [lp, hp, b2]);
    }

    /// The hold clock `h` gains `r` a sample; past 1 a fresh draw. `r ≥ 1`
    /// draws every sample, WHITE.
    #[inline(always)]
    fn digital(nz: &mut Noise, out: &mut Buf, z: (f32, f32), n: (f32, f32, f32), r: f32) {
        let (mut s, mut h, mut v) = (nz.s, nz.h, nz.v);
        let (dz, ddn) = (z.1, n.2);
        let (mut z, mut n, mut dn) = (z.0, n.0, n.1);
        for o in out.iter_mut() {
            z += dz;
            h += r;
            if h >= 1.0 {
                h -= h as u32 as f32;
                s = s.wrapping_mul(A).wrapping_add(C);
                v = s as i32 as f32;
            }
            *o += z * n * v;
            n += dn;
            dn += ddn;
        }
        (nz.s, nz.h, nz.v) = (s, h, v);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const N: usize = BLOCK_SIZE;

    /// `kind` alone at weight 1 (`R`), `blocks` blocks at OSC 1's `inc`.
    fn noise(kind: NoiseKind, inc: f32, blocks: usize) -> Vec<f32> {
        let mut nz = Noise::new(1);
        let mut v = Vec::new();
        for _ in 0..blocks {
            let mut b = [0.0f32; N];
            nz.add(&mut b, kind, (1.0, 0.0), (1.0, 0.0, 0.0), inc);
            v.extend(b);
        }
        v
    }

    fn rms(v: &[f32]) -> f32 {
        let tail = &v[v.len() / 4..];
        (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
    }

    /// Every KIND at the waves' RMS within ½ dB, on a low and a high note.
    #[test]
    fn every_kind_at_r() {
        for k in NoiseKind::ALL {
            for inc in [55.0 / 48_000.0, 880.0 / 48_000.0] {
                let db = 20.0 * (rms(&noise(k, inc, 3000)) / R).log10();
                std::eprintln!("{k:?} at {inc:.4}: {db:+.2} dB");
                assert!(db.abs() < 0.5, "{k:?}: {db:.2} dB");
            }
        }
    }

    /// The draws are zero-mean: no KIND carries DC (PINK's 16 Hz pole
    /// wanders a little over 4 s).
    #[test]
    fn no_dc() {
        for k in NoiseKind::ALL {
            let v = noise(k, 110.0 / 48_000.0, 3000);
            let m = v.iter().sum::<f32>() / v.len() as f32;
            assert!(m.abs() < 0.05 * R, "{k:?}: mean {m}");
        }
    }

    /// The weight's ramp and the norm reach every sample: a zero weight
    /// adds nothing, not a bit.
    #[test]
    fn zero_weight_adds_nothing() {
        for k in NoiseKind::ALL {
            let mut nz = Noise::new(7);
            let mut b = [0.25f32; N];
            nz.add(&mut b, k, (0.0, 0.0), (1.0, 0.0, 0.0), 0.01);
            assert!(b.iter().all(|&x| x.to_bits() == 0.25f32.to_bits()), "{k:?}");
        }
    }

    #[test]
    fn codes_round_trip() {
        for k in NoiseKind::ALL {
            assert_eq!(NoiseKind::from_disk_code(k.disk_code()), Some(k));
        }
        assert_eq!(NoiseKind::from_disk_code(4), None);
    }
}
