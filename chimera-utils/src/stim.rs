//! Test signals: the click, interrupted noise (ISO 3382-2) and the Latin
//! hypercube, drawn as numpy's `default_rng(seed)` draws them (SeedSequence,
//! PCG64, the ziggurat normal), so a seed gives the Python harness's noise.

use std::f64::consts::PI;
use std::sync::OnceLock;

pub const SR: f64 = 48_000.0;
pub const CLICK_AT: usize = 480;
pub const NOISE_SEEDS: usize = 16;
const NOISE_RMS_DB: f64 = -18.0;

/// numpy's `SeedSequence(seed).generate_state(4, uint64)`: the pool of four
/// 32-bit words mixed from the seed, then drawn out as eight.
fn seed_state(seed: u32) -> [u64; 4] {
    const INIT_A: u32 = 0x43b0_d7e5;
    const MULT_A: u32 = 0x931e_8875;
    const INIT_B: u32 = 0x8b51_f9dd;
    const MULT_B: u32 = 0x58f3_8ded;
    const MIX_L: u32 = 0xca01_f9dd;
    const MIX_R: u32 = 0x4973_f715;
    let mut ha = INIT_A;
    let mut hashmix = |v: u32| {
        let mut v = v ^ ha;
        ha = ha.wrapping_mul(MULT_A);
        v = v.wrapping_mul(ha);
        v ^ (v >> 16)
    };
    let mix = |x: u32, y: u32| {
        let r = MIX_L.wrapping_mul(x).wrapping_sub(MIX_R.wrapping_mul(y));
        r ^ (r >> 16)
    };
    let mut pool = [0u32; 4];
    for (i, w) in pool.iter_mut().enumerate() {
        *w = hashmix(if i == 0 { seed } else { 0 });
    }
    for src in 0..4 {
        for dst in 0..4 {
            if src != dst {
                let h = hashmix(pool[src]);
                pool[dst] = mix(pool[dst], h);
            }
        }
    }
    let mut hb = INIT_B;
    let words: Vec<u32> = (0..8)
        .map(|i| {
            let mut v = pool[i % 4] ^ hb;
            hb = hb.wrapping_mul(MULT_B);
            v = v.wrapping_mul(hb);
            v ^ (v >> 16)
        })
        .collect();
    core::array::from_fn(|i| words[2 * i] as u64 | (words[2 * i + 1] as u64) << 32)
}

/// The ziggurat's tables (256 levels, 52-bit), built by Marsaglia and
/// Tsang's recurrence. numpy's were computed at higher precision; these
/// agree to about 1e-14, below anything an f32 render can see.
struct Zig {
    ki: [u64; 256],
    wi: [f64; 256],
    fi: [f64; 256],
}

const ZIG_R: f64 = 3.654_152_885_361_009;
/// Each layer's area: `r·e^(−r²/2) + √(π/2)·erfc(r/√2)`.
const ZIG_V: f64 = 0.004_928_673_233_974_658;

fn zig() -> &'static Zig {
    static Z: OnceLock<Zig> = OnceLock::new();
    Z.get_or_init(|| {
        let m1 = (1u64 << 52) as f64;
        let mut z = Zig {
            ki: [0; 256],
            wi: [0.0; 256],
            fi: [0.0; 256],
        };
        let (mut dn, mut tn) = (ZIG_R, ZIG_R);
        let q = ZIG_V / (-0.5 * dn * dn).exp();
        z.ki[0] = ((dn / q) * m1) as u64;
        z.wi[0] = q / m1;
        z.wi[255] = dn / m1;
        z.fi[0] = 1.0;
        z.fi[255] = (-0.5 * dn * dn).exp();
        for i in (1..255).rev() {
            dn = (-2.0 * (ZIG_V / dn + (-0.5 * dn * dn).exp()).ln()).sqrt();
            z.ki[i + 1] = ((dn / tn) * m1) as u64;
            tn = dn;
            z.fi[i] = (-0.5 * dn * dn).exp();
            z.wi[i] = dn / m1;
        }
        z
    })
}

/// numpy's `Generator(PCG64(SeedSequence(seed)))`.
pub struct Rng {
    state: u128,
    inc: u128,
    /// The upper half of the last 64-bit draw, unused (`next_uint32`).
    half: Option<u32>,
}

const PCG_MULT: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

impl Rng {
    pub fn new(seed: u32) -> Self {
        let v = seed_state(seed);
        let init = (v[0] as u128) << 64 | v[1] as u128;
        let seq = (v[2] as u128) << 64 | v[3] as u128;
        let mut r = Self {
            state: 0,
            inc: (seq << 1) | 1,
            half: None,
        };
        r.step();
        r.state = r.state.wrapping_add(init);
        r.step();
        r
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    /// XSL-RR of the advanced state.
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let s = self.state;
        ((s >> 64) as u64 ^ s as u64).rotate_right((s >> 122) as u32)
    }

    fn next_u32(&mut self) -> u32 {
        if let Some(h) = self.half.take() {
            return h;
        }
        let v = self.next_u64();
        self.half = Some((v >> 32) as u32);
        v as u32
    }

    /// `random()`: uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `standard_normal()`: the ziggurat.
    pub fn normal(&mut self) -> f64 {
        let z = zig();
        loop {
            let r = self.next_u64();
            let idx = (r & 0xff) as usize;
            let r = r >> 8;
            let rabs = (r >> 1) & 0x000f_ffff_ffff_ffff;
            let mut x = rabs as f64 * z.wi[idx];
            if r & 1 == 1 {
                x = -x;
            }
            if rabs < z.ki[idx] {
                return x;
            }
            if idx == 0 {
                loop {
                    let xx = -(1.0 / ZIG_R) * (-self.uniform()).ln_1p();
                    let yy = -(-self.uniform()).ln_1p();
                    if yy + yy > xx * xx {
                        return if (rabs >> 8) & 1 == 1 {
                            -(ZIG_R + xx)
                        } else {
                            ZIG_R + xx
                        };
                    }
                }
            } else if (z.fi[idx - 1] - z.fi[idx]) * self.uniform() + z.fi[idx]
                < (-0.5 * x * x).exp()
            {
                return x;
            }
        }
    }

    /// An integer in `0..=max` by masked rejection (`random_interval`).
    fn interval(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        let mask = u64::MAX >> max.leading_zeros();
        loop {
            let v = if max <= u32::MAX as u64 {
                self.next_u32() as u64
            } else {
                self.next_u64()
            } & mask;
            if v <= max {
                return v;
            }
        }
    }

    /// `shuffle()`: Fisher–Yates from the end.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.interval(i as u64) as usize;
            v.swap(i, j);
        }
    }
}

/// `seconds` of silence with a unit impulse at 10 ms.
pub fn click(seconds: f64) -> Vec<f64> {
    let mut x = vec![0.0; (seconds * SR) as usize];
    x[CLICK_AT] = 1.0;
    x
}

/// Gaussian noise at −18 dBFS RMS (clipped at ±0.95, 2 ms raised-cosine
/// ends) for `charge_s`, then `tail_s` of silence; and where it stops.
pub fn noise_stim(seed: u32, charge_s: f64, tail_s: f64) -> (Vec<f64>, usize) {
    let mut rng = Rng::new(seed);
    let n = (charge_s * SR) as usize;
    let g = 10f64.powf(NOISE_RMS_DB / 20.0);
    let mut x: Vec<f64> = (0..n)
        .map(|_| (rng.normal() * g).clamp(-0.95, 0.95))
        .collect();
    let r = (0.002 * SR) as usize;
    for i in 0..r {
        let a = (PI / 2.0) * i as f64 / (r - 1) as f64;
        x[i] *= a.sin().powi(2);
        x[n - r + i] *= a.cos().powi(2);
    }
    x.resize(n + (tail_s * SR) as usize, 0.0);
    (x, n)
}

/// `n` points of a Latin hypercube in `[0, 1)^d`.
pub fn lhs(n: usize, d: usize, seed: u32) -> Vec<Vec<f64>> {
    let mut rng = Rng::new(seed);
    let mut u: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..d)
                .map(|_| (i as f64 + rng.uniform()) / n as f64)
                .collect()
        })
        .collect();
    for j in 0..d {
        let mut col: Vec<f64> = u.iter().map(|r| r[j]).collect();
        rng.shuffle(&mut col);
        for (r, v) in u.iter_mut().zip(col) {
            r[j] = v;
        }
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_draws_what_numpy_default_rng_draws() {
        // np.random.default_rng(s).bit_generator.random_raw(3)
        let mut r = Rng::new(0);
        let raw = [r.next_u64(), r.next_u64(), r.next_u64()];
        assert_eq!(
            raw,
            [
                11749869230777074271,
                4976686463289251617,
                755828109848996024
            ]
        );
        let mut r = Rng::new(15);
        assert_eq!(r.next_u64(), 12778859617612771073);
        // default_rng(1).standard_normal(3)
        let mut r = Rng::new(1);
        let n = [r.normal(), r.normal(), r.normal()];
        // Equal to the tables' precision (about 1e-14).
        let want = [0.345584192064786, 0.8216181435011584, 0.33043707618338714];
        assert!(
            n.iter().zip(want).all(|(a, b)| (a / b - 1.0).abs() < 1e-13),
            "{n:?}"
        );
        // default_rng(1): random(5), then permutation(10) and permutation(7)
        let mut r = Rng::new(1);
        (0..5).for_each(|_| _ = r.uniform());
        let mut p: Vec<u32> = (0..10).collect();
        r.shuffle(&mut p);
        assert_eq!(p, [1, 0, 6, 9, 7, 2, 3, 4, 5, 8]);
        let mut p: Vec<u32> = (0..7).collect();
        r.shuffle(&mut p);
        assert_eq!(p, [1, 3, 0, 6, 4, 5, 2]);
    }
}
