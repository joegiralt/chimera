//! PARTICLE: random impulses (dust) shared round-robin over six of the
//! engine's CLEAN band-passes (`filter::Filter`, BP12, its own state each),
//! then an all-pass diffuser. TIMBRE is the dust's density (2 to 3000
//! impulses a second); HARMONICS scatters each particle's centre on its
//! impulse, up to ±3 octaves of the pitch (`h²` law, so low settings stay
//! pitched); MORPH runs from the diffuser on raw dust (0: clicks smeared
//! into reverberant splashes) through wide band-passes (½) to sharp,
//! ringing ones (1, Q ≈ 100).
//!
//! The diffuser is four Schroeder all-passes on the reverbs' kernel
//! (`rev::parts::ap_f32`) over our own delay lines (7 to 45 ms).
//!
//! Per sample: one random draw for the dust; six CLEAN band-passes (the
//! engine's per-sample SVF, ~6 × CLEAN's cost); the diffuser's four
//! all-passes and the mixes (~25 cycles), run only while MORPH < 1.
//!
//! Debt to Plaits (structure only): dust exciting band-passes whose
//! centres are randomised per impulse, with an all-pass network at one end
//! of MORPH. The filter is the engine's; densities, laws, delay lengths and
//! level compensation are ours.

use super::{B, Blk, Ctl, FS, Model};
use crate::drums::parts::{Flt, Lcg, expo};
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::rev::parts::ap_f32;

pub const PARTICLES: usize = 6;
/// The diffuser's delays (samples at 48 kHz, primes) and its gain.
const LINES: [usize; 4] = [337, 709, 1259, 2153];
const AP_G: f32 = 0.7;

pub struct Particle {
    flt: [Flt; PARTICLES],
    hz: [f32; PARTICLES],
    next: usize,
    lines: [Vec<f32>; 4],
    pos: [usize; 4],
    rng: Lcg,
}

impl Particle {
    pub fn new(seed: u32) -> Self {
        Self {
            flt: core::array::from_fn(|_| Flt::clean(FilterMode::Bp12, 440.0, 0.5)),
            hz: [0.0; PARTICLES],
            next: 0,
            lines: LINES.map(|n| vec![0.0; n]),
            pos: [0; 4],
            rng: Lcg(seed.wrapping_mul(0x85EB_CA6B) | 1),
        }
    }
}

/// MORPH's resonance: CLEAN's RES for SVF damping `k` from 2 (Q ½) down
/// to 0.01 (Q 100); 0.95 is CLEAN's oscillation onset (`clean_k`).
fn res(m: f32) -> f32 {
    let k = expo(m, 2.0, 0.01);
    0.95 * (1.0 - 0.5 * k)
}

/// A unit impulse's ring energy through CLEAN's BP12 at `E_HZ`, dB, at
/// MORPH 0, ¼, ½, ¾, 1 (measured on the engine's filter; ∝ f elsewhere).
const E_DB: [f32; 5] = [-14.8, -8.95, -10.05, -8.0, -3.6];
const E_HZ: f32 = 261.0;

fn energy_db(m: f32) -> f32 {
    let x = m * 4.0;
    let i = (x as usize).min(3);
    E_DB[i] + (E_DB[i + 1] - E_DB[i]) * (x - i as f32)
}

impl Model for Particle {
    fn block(&mut self, out: &mut Blk, hz: f32, c: Ctl) {
        let (h, t, m) = (
            c.harmonics.clamp(0.0, 1.0),
            c.timbre.clamp(0.0, 1.0),
            c.morph.clamp(0.0, 1.0),
        );
        let p = expo(t, 2.0, 3000.0) / FS;
        let oct = 3.0 * h * h;
        let r = res(m);
        // the dust, dealt round-robin; a particle retunes on its first
        // impulse in the block (the filter's cutoff is a block's)
        let mut dry: Blk = [0.0; B];
        let mut ins = [[0.0f32; B]; PARTICLES];
        let mut tuned = [false; PARTICLES];
        for (s, d) in dry.iter_mut().enumerate() {
            if self.rng.unit() < p {
                let a = 0.4 + 0.6 * self.rng.unit();
                let j = self.next;
                self.next = (j + 1) % PARTICLES;
                if !tuned[j] {
                    tuned[j] = true;
                    self.hz[j] = hz * (oct * self.rng.next()).exp2();
                }
                ins[j][s] = a;
                *d = a;
            }
        }
        out.fill(0.0);
        let g_m = 0.5 * 10f32.powf(-0.05 * energy_db(m));
        for j in 0..PARTICLES {
            if self.hz[j] == 0.0 {
                self.hz[j] = hz;
            }
            let f = &mut self.flt[j];
            f.set_res(r);
            f.run(&mut ins[j], self.hz[j]);
            // an impulse's ring carries energy ∝ f (CLEAN's BP12, measured):
            // back to the raw dust's ¼ at every pitch and Q
            let comp = g_m * (E_HZ / self.hz[j]).sqrt();
            for (o, x) in out.iter_mut().zip(ins[j].iter()) {
                *o += x * comp;
            }
        }
        // MORPH below ½ brings the raw dust in; the diffuser fades out by 1
        let raw = (1.0 - 2.0 * m).max(0.0);
        let wet = (1.0 - m) * (1.0 - m);
        for (o, d) in out.iter_mut().zip(dry.iter()) {
            let x = *o * (1.0 - raw) + 0.5 * d * raw;
            let mut y = x;
            if wet > 0.0 {
                for (l, line) in self.lines.iter_mut().enumerate() {
                    let i = self.pos[l];
                    let (ap, v) = ap_f32(y, line[i], AP_G);
                    line[i] = v;
                    self.pos[l] = if i + 1 == line.len() { 0 } else { i + 1 };
                    y = ap;
                }
            }
            *o = x + (y - x) * wet;
        }
    }
}
