//! BRIGHT TAIL: the spectrum continued above the last exact partial to
//! ~20 kHz by VA's band-limited saw (`VarOsc`, called), high-passed by
//! CLEAN's HP4 and tilted by CLEAN's LP1 (the engine's filter, called).
//!
//! - Source: `α·saw(f0) + β·saw(2f0)`. Odd harmonics get α, even ones
//!   α + 2β, so ODD/EVEN's law carries into the tail exactly (a saw an
//!   octave up is the evens alone).
//! - HP4 at RES 0 (two TPT stages, k = 2 and 2.7: a soft knee, −14.6 dB
//!   at its cutoff) with its cutoff at `HP_AT`·(N + ½)·f0.
//! - TILT: `x + c·(LP1(x) − x)`, LP1 at the band's geometric middle, `c`
//!   solved a block so the band's ends keep the spectrum's own slope.
//! - The skirt under N never doubles a partial: both saws are phase-locked
//!   to the note (`set_phase` a block from the sample count), so the
//!   tail's complex amplitude at every harmonic ≤ N is known, and each
//!   lane is solved so lane + tail lands on the lane's own target
//!   (`yield_to`). Exact while the target is reachable: always, but for a
//!   hole (a silent partial) the skirt reaches.

use super::voice::{LANES, SUB};
use crate::drums::parts::{B, Blk, FS};
use chimera_engine::dsp::filter::clean::{clean_k, clean_k2};
use chimera_engine::dsp::filter::coef::g_of;
use chimera_engine::dsp::filter::{Clean, FilterMode, FilterMods};
use chimera_engine::dsp::va::osc::{NO_WRAP, Ramps, VarOsc};
use chimera_engine::params::FilterParams;
use rustfft::num_complex::Complex32 as C;

/// The tail starts only while its first harmonic is under this.
pub const TAIL_BELOW: f32 = 16_000.0;
/// Where the tail's spectrum ends (its slope is fitted up to here).
pub const TAIL_TOP: f32 = 20_000.0;
/// HP4's cutoff as a share of (N + ½)·f0: −5.7 dB at N + 1, −1.8 dB by
/// 2N. Lower than (N + ½)·f0 so the octave above N is not left thin; the
/// bank yields where the skirt reaches under N.
pub const HP_AT: f32 = 0.5;
/// VA's saw: harmonic k is `SAW_A / k` (× `resp(f)` near Nyquist), one
/// sample late, sine phase (measured: `tests::saw_is_calibrated`).
pub const SAW_A: f32 = 0.22053;

/// The saw's magnitude against an ideal one, in 46.875 Hz steps (VA's
/// correction rolls the top off: 0.96 at 5 kHz, 0.85 at 10, 0.66 at 15).
fn resp_table() -> &'static [f32; 512] {
    static T: std::sync::OnceLock<[f32; 512]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        // 46.875 Hz is 2^-10 a sample: exact, 1024 samples a period
        let mut o = Saw::new();
        let mut x = Vec::with_capacity(1 << 16);
        while x.len() < 1 << 16 {
            let mut b = [0.0; B];
            o.render(46.875, &mut b);
            x.extend(b);
        }
        let n = 1 << 15;
        let mut buf: Vec<C> = x[n..2 * n].iter().map(|&v| C::new(v, 0.0)).collect();
        rustfft::FftPlanner::new()
            .plan_fft_forward(n)
            .process(&mut buf);
        let mut t = [1.0; 512];
        for (k, t) in t.iter_mut().enumerate().skip(1) {
            *t = buf[32 * k].norm() * 2.0 / n as f32 * k as f32 / SAW_A;
        }
        t
    })
}

/// `resp_table` at `hz`.
fn resp(hz: f32) -> f32 {
    let t = resp_table();
    let p = (hz / 46.875).clamp(0.0, 510.0);
    let i = p as usize;
    t[i] + (t[i + 1] - t[i]) * (p - i as f32)
}

/// The TPT SVF's `s` at `hz` for a cutoff `fc` (exact on the unit circle).
fn s_at(hz: f32, fc: f32) -> C {
    C::new(0.0, (core::f32::consts::PI * hz / FS).tan() / g_of(fc, FS))
}

/// CLEAN's HP4 at RES 0, at `hz`.
pub fn hp4(hz: f32, fc: f32) -> C {
    let s = s_at(hz, fc);
    let st = |k: f32| s * s / (s * s + s * k + 1.0);
    let k = clean_k(0.0);
    st(k) * st(clean_k2(k))
}

/// CLEAN's LP1 at RES 0, at `hz`: `1/(1 + s)`.
fn lp1(hz: f32, fc: f32) -> C {
    C::new(1.0, 0.0) / (s_at(hz, fc) + 1.0)
}

/// `x + c·(LP1 − x)` from LP1's response `l`.
fn tilted(l: C, c: f32) -> C {
    C::new(1.0, 0.0) + (l - 1.0) * c
}

/// One VA saw.
struct Saw(VarOsc);

impl Saw {
    fn new() -> Self {
        let mut o = VarOsc::NEW;
        o.prepare(FS);
        o.reset();
        Self(o)
    }

    fn render(&mut self, hz: f32, out: &mut Blk) {
        let r = Ramps {
            inc: (self.0.inc_for(hz), 0.0),
            wave: (2.0 / 3.0, 0.0),
            shape: (0.0, 0.0),
        };
        let mut mk = [[NO_WRAP; 2]; B];
        self.0.render::<B, false, false, false, false>(
            out,
            &mut mk,
            &r,
            &mut 1.0,
            &mut || 1.0,
            &[0.0; B],
            (0.0, 0.0),
        );
    }
}

/// What the control frame asks of the tail this block (before PLUCK).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TailSpec {
    /// The envelope's amplitude at harmonic N + 1, in lane units.
    pub e_lo: f32,
    /// Its slope above, dB/oct.
    pub slope: f32,
    /// Odd and even harmonics' gains (ODD/EVEN's law).
    pub odd: f32,
    pub even: f32,
}

/// The tail's settings at a block's end; each ramps across the block.
#[derive(Clone, Copy, Debug, Default)]
struct Plan {
    g: f32,
    alpha: f32,
    beta: f32,
    c: f32,
}

pub struct Tail {
    f0: f32,
    saw: [Saw; 2],
    hp: Clean,
    lp: Clean,
    hp_p: FilterParams,
    lp_p: FilterParams,
    k_lo: f32,
    f_lo: f32,
    f_hi: f32,
    f_lp: f32,
    /// Per lane: the tail's harmonic there at g 1, α 1, β 0, c 0, turned
    /// into the lane's phase; and LP1 there.
    base: [C; LANES],
    lpk: [C; LANES],
    /// The first lane the skirt reaches (above −80 dB of the saw).
    first: usize,
    /// The last TILT solve: (dB/oct asked, c).
    tilt_at: (f32, f32),
    last: Plan,
    next: Option<Plan>,
}

impl Tail {
    /// For `n` partials at `f0`, the lanes started at phases `psi`.
    pub fn new(n: usize, f0: f32, psi: &[f32; LANES]) -> Self {
        let k_lo = (n + 1) as f32;
        let (f_lo, f_hi) = (k_lo * f0, TAIL_TOP.min(0.44 * FS));
        let f_lp = (f_lo * f_hi).sqrt();
        let fc = HP_AT * (n as f32 + 0.5) * f0;
        let mut hp_p = FilterParams::default();
        assert!(hp_p.set_mode(FilterMode::Hp24));
        hp_p.cutoff = fc;
        let mut lp_p = FilterParams::default();
        assert!(lp_p.set_mode(FilterMode::Lp6));
        lp_p.cutoff = f_lp;
        let mut base = [C::new(0.0, 0.0); LANES];
        let mut lpk = [C::new(0.0, 0.0); LANES];
        let mut first = SUB + n;
        for i in 0..n {
            let k = (i + 1) as f32;
            let hz = k * f0;
            let w = core::f32::consts::TAU * hz / FS;
            base[SUB + i] = C::from_polar(SAW_A / k * resp(hz), -w - psi[SUB + i]) * hp4(hz, fc);
            lpk[SUB + i] = lp1(hz, f_lp);
            if base[SUB + i].norm() * k > 1e-4 * SAW_A {
                first = first.min(SUB + i);
            }
        }
        Self {
            f0,
            saw: [Saw::new(), Saw::new()],
            hp: Clean::new(),
            lp: Clean::new(),
            hp_p,
            lp_p,
            k_lo,
            f_lo,
            f_hi,
            f_lp,
            base,
            lpk,
            first,
            tilt_at: (f32::NAN, 0.0),
            last: Plan::default(),
            next: None,
        }
    }

    /// TILT's `c` for `delta` dB/oct over the saw's −6 across the band:
    /// −3 (a +12 dB shelf) to 1 (LP1 itself). Re-solved only when the
    /// slope moves.
    fn tilt(&mut self, delta: f32) -> f32 {
        if (delta - self.tilt_at.0).abs() < 0.05 {
            return self.tilt_at.1;
        }
        // VA's own roll-off at the top is made up too
        let want = delta * (self.f_hi / self.f_lo).log2()
            - 20.0 * (resp(self.f_hi) / resp(self.f_lo)).log10();
        let (l0, l1) = (lp1(self.f_lo, self.f_lp), lp1(self.f_hi, self.f_lp));
        let db = |c: f32| 20.0 * (tilted(l1, c).norm() / tilted(l0, c).norm()).log10();
        let (mut lo, mut hi) = (-3.0f32, 1.0f32);
        for _ in 0..16 {
            let m = 0.5 * (lo + hi);
            if db(m) > want { lo = m } else { hi = m }
        }
        self.tilt_at = (delta, 0.5 * (lo + hi));
        self.tilt_at.1
    }

    /// Plans this block from `spec` (× `scale`: PLUCK, STRETCH's fade) and
    /// solves the lanes' targets `to` so lane + tail meets each.
    pub fn yield_to(&mut self, spec: &TailSpec, scale: f32, n: usize, to: &mut [f32; LANES]) {
        let c = self.tilt(spec.slope + 6.0206);
        let t_lo = tilted(lp1(self.f_lo, self.f_lp), c).norm();
        let g = spec.e_lo * scale * self.k_lo / (SAW_A * resp(self.f_lo) * t_lo);
        let (alpha, beta) = (spec.odd, 0.5 * (spec.even - spec.odd));
        for l in self.first..SUB + n {
            let i = l - SUB;
            let oe = if (i + 1).is_multiple_of(2) {
                alpha + 2.0 * beta
            } else {
                alpha
            };
            let r = self.base[l] * tilted(self.lpk[l], c) * (g * oe);
            let t = to[l];
            let d = t * t - r.im * r.im;
            to[l] = -r.re + d.max(0.0).sqrt();
        }
        self.next = Some(Plan { g, alpha, beta, c });
    }

    /// The tail into `acc`, for the block starting `on` samples into the
    /// note.
    pub fn block(&mut self, on: usize, acc: &mut Blk) {
        let p = self.next.take().unwrap_or_default();
        let l = self.last;
        self.last = p;
        if p.g == 0.0 && l.g == 0.0 {
            return;
        }
        let mut x = [[0.0f32; B]; 2];
        let octave = p.beta != 0.0 || l.beta != 0.0;
        for (j, (s, x)) in self.saw.iter_mut().zip(&mut x).enumerate() {
            if j == 1 && !octave {
                break;
            }
            let hz = self.f0 * (j + 1) as f32;
            s.0.set_phase((hz as f64 * on as f64 / FS as f64).fract() as f32);
            s.render(hz, x);
        }
        let mut y = [0.0f32; B];
        for (i, y) in y.iter_mut().enumerate() {
            let u = (i + 1) as f32 / B as f32;
            let a = l.alpha + (p.alpha - l.alpha) * u;
            let b = l.beta + (p.beta - l.beta) * u;
            *y = a * x[0][i] + b * x[1][i];
        }
        let fs = FS as u32;
        self.hp.process(&mut y, &self.hp_p, fs, &FilterMods::NONE);
        let mut lo = y;
        self.lp.process(&mut lo, &self.lp_p, fs, &FilterMods::NONE);
        for (i, a) in acc.iter_mut().enumerate() {
            let u = (i + 1) as f32 / B as f32;
            let c = l.c + (p.c - l.c) * u;
            let g = l.g + (p.g - l.g) * u;
            *a += g * (y[i] + c * (lo[i] - y[i]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VA's saw, phase-locked: harmonic k is SAW_A/k · resp at sine phase,
    /// one sample late; the model `Tail` solves the lanes on.
    #[test]
    fn saw_is_calibrated() {
        let f0 = 32.70f32;
        let mut s = Saw::new();
        let mut x = Vec::new();
        for i in 0..750 {
            let mut b = [0.0; B];
            s.0.set_phase((f0 as f64 * (i * B) as f64 / FS as f64).fract() as f32);
            s.render(f0, &mut b);
            x.extend(b);
        }
        let l = x.len() as f64;
        for k in [1usize, 20, 64, 153, 306] {
            let w = core::f64::consts::TAU * f0 as f64 * k as f64 / FS as f64;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (j, v) in x.iter().enumerate() {
                let n = (j + 1) as f64;
                re += *v as f64 * (w * n).cos();
                im -= *v as f64 * (w * n).sin();
            }
            let got = C::new((-2.0 * im / l) as f32, (2.0 * re / l) as f32);
            let want = C::from_polar(SAW_A / k as f32 * resp(f0 * k as f32), -w as f32);
            let err = (got - want).norm() / want.norm();
            assert!(err < 0.03, "harmonic {k}: {got} vs {want}");
        }
    }
}
