//! SEM: the Oberheim SEM-1A VCF as built (ADR 0192), ported from the
//! circuit lab's firmware model (`chimera-utils/src/sem_circuit/fw.rs`,
//! branch `sem-circuit-lab`, #720). Every constant below is the lab's,
//! derived there from the schematic's parts and the datasheets.
//!
//! Zavalishin's TPT SVF in the circuit's own sign and volts, at 1×:
//! - A12, the summer: `hp = −u − D(bp) − (k + g)·s₁ − s₂`, held at ±14 V.
//! - A13/A15, the CA3080 integrators: each input through `V0·tanh(v/V0)`
//!   as its series to v⁵, V0 = 2V_T/(β·A_div) (the Wasp fit's β).
//! - Q39/Q40, the 2N4302 followers: each integrator's step scaled by the
//!   follower's slope against its own output, a cubic `r(b)`. The circuit's
//!   only even-order term.
//! - D11/D12: the antisaturator `D(bp)`, a C1 dead-zone fit to the diodes.
//! - The OTA swing at the followers: the states held at −12.6 / +13.8 V;
//!   A14/A16 hold BP and LP at ±13.5 V. Each hold is `clampf`: on the M7
//!   (`target-cpu=cortex-m7`) a `vmaxnm` and a `vminnm`, two instructions,
//!   where a compare and select is three (vcmp, vmrs, vsel).
//!
//! Per block, from the parts: RES's pot law (PS16's Thevenin into R157),
//! the integrators' leak, C18's lead, A12's lag, C22's coupling and the
//! trimmed cutoff's droop. C15 and C23, the couplings, are the voice's DC
//! blocker (ADR 0060); only sub-20 Hz phase differs.
//!
//! MODE (Filter 16) mixes LP into HP through the notch, `lp + m·(hp − lp)`,
//! as PS19 does; BANDPASS (MODE's BP2) takes A14's BP instead, S504.
//!
//! Every loop is bank 2's (`kern`, ADR 0192): with the door closed, the
//! block plays dry, as FUZZ does.

use super::coef::g_of;
use super::level::{FilterMods, clip_amount, level_gain};
use super::{Fed, FilterMode, tap_g_range};
use crate::dsp::clampf;
use crate::hw::{BLOCK_SIZE, Cost};
use crate::meter::Clip;
use crate::params::FilterParams;

/// SEM's MODE choices: the MODE pot's mix (LP2's code) and BANDPASS (BP2's).
pub const SEM_MODES: [FilterMode; 2] = [FilterMode::Lp12, FilterMode::Bp12];
pub const SEM_LABELS: [&str; 2] = ["MODE", "BP"];

/// Volts at A12's summing node for digital 1.0: one VCO saw at the input
/// pot's top, ±3.6 V (the VCO sheet's comparator thresholds, the lab's
/// derivation), times R139/R155.
pub const IN: f32 = 3.6 * (100.0 / 220.0);
/// The outputs' volts back to digital: the SEM inverts, so 1.0 is LP's
/// pass band for a unit input.
pub const OUT: f32 = -1.0 / IN;
/// The CA3080's tanh scale at the integrator's input, 2V_T/(β·A_div).
const V0: f32 = 25.035_631;
const A3: f32 = 1.0 / (3.0 * V0 * V0);
const A5: f32 = 2.0 / (15.0 * V0 * V0 * V0 * V0);
/// A12's swing (LM301A, ±14 V).
const RAIL_HP: f32 = 14.0;
/// The followers' sources where the OTA's output reaches its swing.
const LO: f32 = -12.600_327;
const HI: f32 = 13.775_356;
/// A14/A16 (741).
const RAIL_OUT: f32 = 13.5;
/// The followers' rate, `1 + b·(R₀ + b·(R₁ + b·R₂))`: the 2N4302's slope,
/// least squares in the lab.
const R: [f32; 3] = [1.163_084_8e-3, -2.017_724_9e-4, 1.374_420_8e-5];
/// D11/D12 through R156: `KD·φ(|b| − B0)`, φ quadratic over the knee `W`.
const B0: f32 = 0.10;
const W: f32 = 3.32;
const KD: f32 = 0.154;
/// The trim at 1 kHz and the cutoff's droop above it (C18, A12's lag).
const DROOP: f32 = 1.9e-6;
/// The integrators' leak, 2/(gm·r_o·A_div·g_f).
#[allow(clippy::excessive_precision)] // the lab's digits
const LEAK: f32 = 8.081_521_5e-3;
/// C18's lead and A12's lag, each a damping per Hz: ω·R138·C18 and the
/// noise gain over the 301A's GBW.
const C18_HZ: f32 = 6.283_185_3e-6;
const LAG_HZ: f32 = 1.080_808_1e-6;
/// C22 in series with R157: ω·R157·C22 per Hz.
#[allow(clippy::excessive_precision)] // the lab's digits
const WT_HZ: f32 = 6.842_388_8e-2;
/// PS16, 50 K with 100 Ω of track at each end (the lab's assumption), into
/// R157 33 K; R139 100 K.
const POT: f32 = 50e3;
const R_END: f32 = 100.0;
const R157: f32 = 33e3;
const R139: f32 = 100e3;

/// The output's bound for any input, MODE in 0..1: A12's ±14 V at the
/// output's scale (BP's and LP's ±13.5 V are inside it). Every state is
/// held, so a huge, infinite or NaN input stays inside it too.
pub const SEM_BOUND: f32 = RAIL_HP * -OUT;

/// The LED: A14's BP past a third of its swing lights it, full at the
/// swing, where the diodes have long been squashing it.
const LED_KNEE: f32 = RAIL_OUT / 3.0;

/// The pot law alone: PS16's wiper into R157, R139 over it. 3.02 at RES
/// 0 (Q 0.33), 1.10 at ½, 0.27 at 0.9, 0.006 at 1: the circuit's, not
/// spread (all the high Q is in the knob's last tenth).
#[inline(always)]
pub fn k_pot(res: f32) -> f32 {
    let r = super::hold(res, (0.0, 1.0));
    let rt = R_END + POT * r;
    let rb = R_END + POT * (1.0 - r);
    R139 * rb / (R157 * (2.0 * R_END + POT) + rt * rb)
}

/// SEM's damping `k` at cutoff `fc` Hz and RES: the pot law, C22's
/// coupling at `fc`, the leak, C18's lead less A12's lag. Never below
/// 0.0145 (RES 1 at 100 Hz, Q 69): it rings, it never holds a tone.
#[inline(always)]
pub fn sem_k(fc: f32, res: f32) -> f32 {
    let kp = k_pot(res);
    let wt = WT_HZ * fc;
    let w2 = wt * wt;
    kp * w2 / (1.0 + w2) + LEAK + (C18_HZ - LAG_HZ) * fc
}

/// RES 1's `k` at `fc` less C22's sliver (under 1 % above 30 Hz): the
/// floor STABILITY's spread holds SEM's `k` at. No divide.
#[inline(always)]
fn k_floor(fc: f32) -> f32 {
    const K_POT_1: f32 = R139 * R_END / (R157 * (2.0 * R_END + POT) + (R_END + POT) * R_END);
    K_POT_1 + LEAK + (C18_HZ - LAG_HZ) * fc
}

/// A Q tap's RES → `k`, at 1 kHz: the tap moves `k` linearly between RES
/// 0's and RES 1's, as every type's does.
pub fn sem_k_tap(res: f32) -> f32 {
    sem_k(1000.0, res)
}

/// `g` at FREQ `fc`: the trim at 1 kHz and the droop above it.
#[inline(always)]
pub fn sem_g(fc: f32, fs: f32) -> f32 {
    g_of(fc * (1.0 - DROOP * (fc - 1000.0)), fs)
}

/// The steady bill over `OLD_LP24` (95), the `filter-probes` ELF's count
/// (`tools/loop_count.py`, `probe::sem_probe`, ADR 0192): MODE's loop 84
/// instructions with the diodes conducting (77 quiet), 0 `vdiv`, its block
/// 1.83 and 0.062 `vdiv`: ceil5(85.83 × 1.606 + 12 × 0.062) = 140, less 95.
/// BANDPASS's runs 79: 135. The unit reads SEM 9 over CLEAN LP4 (667 and
/// 658 a voice, 2026-10-22, ROUTING bench, rev V at 480 MHz; #726): 10.
pub const SEM_COST: Cost = Cost(10);
/// The moving path over the steady one: MODE's loop 91 instructions and 1
/// `vdiv`, its block 2.30 and 0.047, `ramps`' fill 6.25: ceil5(99.55 ×
/// 1.606 + 12 × 1.047) = 175, less the steady 140. FB's loop (135, 1
/// `vdiv`: 245) is inside `FB_COST` over it. The unit reads the moving path
/// 47 over SEM's steady one (SEM MOVE 611 against SVF MOVE 568, CLEAN's
/// own moving path 13 over its steady; 2026-10-22, ROUTING bench, #726): 50.
/// SEM MOVE reads 38 over 1 CUTOFF (2026-10-26, ROUTING bench, rev V at
/// 480 MHz, 4eae5d81), 28 over the steady bill, and CLEAN's stepped
/// CUTOFF 42 more (ADR 0249): 70.
pub const SEM_MOVE_COST: Cost = Cost(70);

/// The circuit's state in volts: BP and LP (the followers' sources), the
/// last block's ends, and FB's signal.
#[derive(Clone, Debug, Default)]
pub struct Sem {
    s1: f32,
    s2: f32,
    last: Option<Ends>,
    /// FB's signal (#393).
    fbp: f32,
}

/// A block's end values: `g`, `k`, the input scale `gain·IN` and MODE's
/// mix.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ends {
    g: f32,
    k: f32,
    a: f32,
    m: f32,
}

/// One block's `(from, to)` of each, and what the block runs.
#[derive(Clone, Copy, Debug)]
struct Run {
    g: (f32, f32),
    k: (f32, f32),
    a: (f32, f32),
    m: (f32, f32),
    bp: bool,
    glide: bool,
    fb: Fed,
    /// `g`'s range under a FREQ tap or FB.
    gr: (f32, f32),
}

/// The kernel's entry in bank 2's door: the block, its params, rate and
/// mods; the block's peak |BP| for the LED. The ends, the taps and every
/// loop are bank 2's: bank 1 only calls through.
pub type SemRun = fn(&mut Sem, &mut [f32; BLOCK_SIZE], &FilterParams, u32, &FilterMods) -> f32;

#[repr(C)]
pub struct SemDoor {
    pub run: SemRun,
}

impl SemDoor {
    pub const DOOR: SemDoor = SemDoor { run: kern::run };
}

/// `f` on the kernel: through the door with `bank2` (`None` while it is
/// closed), else bank 1's own.
#[inline(always)]
fn through<R>(f: impl FnOnce(&SemDoor) -> R) -> Option<R> {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL.with(|d| f(&d.sem))
    }
    #[cfg(not(feature = "bank2"))]
    {
        Some(f(&SemDoor::DOOR))
    }
}

/// The diodes' extra damping at `b` volts of BP.
#[inline(always)]
fn diode(b: f32) -> f32 {
    let e = b.abs() - B0;
    // Both pieces, then selects (`vsel`), not two compare-and-branches a
    // sample: the same formulas, the same bits.
    let (bend, line) = (e * e * (0.5 / W), e - 0.5 * W);
    let y = if e < W { bend } else { line };
    let y = if e <= 0.0 { 0.0 } else { y };
    (KD * y).copysign(b)
}

/// The followers' rate at `b` volts.
#[inline(always)]
fn rate(b: f32) -> f32 {
    1.0 + b * (R[0] + b * (R[1] + b * R[2]))
}

/// The CA3080's bend at `v` volts into its divider.
#[inline(always)]
fn ota(v: f32) -> f32 {
    let v2 = v * v;
    v - v * v2 * (A3 - A5 * v2)
}

impl Sem {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block: the LED's clip, A14's BP past `LED_KNEE`. With bank 2's
    /// door closed the block stays dry and the LED dark.
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Clip {
        match through(|d| (d.run)(self, buf, p, fs, mods)) {
            Some(peak) => clip_amount(peak * (1.0 / LED_KNEE)),
            None => {
                self.fbp = 0.0;
                clip_amount(0.0)
            }
        }
    }

    /// This block's run, its ends stored as the next block's start.
    /// STABILITY's spread on `g` and `k` (ADR 0168, opened for SEM by ADR
    /// 0192): `k` is held at the circuit's own floor.
    #[inline(always)]
    fn ramps(&mut self, p: &FilterParams, fs: u32, mods: &FilterMods) -> Run {
        let to = Ends {
            g: sem_g(p.cutoff, fs as f32),
            k: sem_k(p.cutoff, p.resonance),
            a: level_gain(p.level) * IN,
            m: super::hold(p.morph, (0.0, 1.0)),
        };
        let from = self.last.replace(to).unwrap_or(to);
        Run {
            g: mods.spread.g((from.g, to.g)),
            k: mods.spread.k_damping((from.k, to.k), k_floor(p.cutoff)),
            a: (from.a, to.a),
            m: (from.m, to.m),
            bp: p.mode() == FilterMode::Bp12,
            glide: mods.glide,
            fb: mods.fb,
            gr: tap_g_range(fs),
        }
    }

    /// One sample of `u` volts into the loop at `g`, `k + g` and `d = 1/(1
    /// + kg + g²)`: `(hp, bp, lp)`, volts, BP and LP before A14/A16.
    #[inline(always)]
    fn tick(&mut self, u: f32, g: f32, kg: f32, d: f32) -> (f32, f32, f32) {
        let (s1, s2) = (self.s1, self.s2);
        let dd = diode(s1);
        let (g1, g2) = (g * rate(s1), g * rate(s2));
        let hp = clampf((-u - dd - kg * s1 - s2) * d, -RAIL_HP, RAIL_HP);
        let v1 = g1 * ota(hp);
        let bp = s1 + v1;
        self.s1 = clampf(bp + v1, LO, HI);
        let v2 = g2 * ota(bp);
        let lp = s2 + v2;
        self.s2 = clampf(lp + v2, LO, HI);
        (hp, bp, lp)
    }

    /// Forget the last ends: the next block starts unramped.
    pub fn hold(&mut self) {
        self.last = None;
    }

    /// A fresh voice's filter: at rest and unramped.
    pub fn reset(&mut self) {
        (self.s1, self.s2, self.fbp) = (0.0, 0.0, 0.0);
        self.hold();
    }

    /// Still ringing: a state above −80 dB at the output.
    pub fn rings(&self) -> bool {
        self.s1.abs().max(self.s2.abs()) * -OUT > 1e-4
    }

    /// `(lp, bp)` at the output's scale, for another type's warm start.
    pub fn outputs(&self) -> (f32, f32) {
        (self.s2 * OUT, self.s1 * OUT)
    }

    /// A warm start from another type's `lp` and `bp` (spec § 1): LP and BP
    /// at the same output, in volts.
    pub fn seed(&mut self, lp: f32, bp: f32) {
        let v = |y: f32| super::hold(y * (1.0 / OUT), (LO, HI));
        (self.s2, self.s1) = (v(lp), v(bp));
    }

    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        self.last.map(|e| e.g)
    }
}

/// The kernel, linked into bank 2 with `bank2` (`bank2.x`,
/// `*6filter3sem4kern*`): every loop, steady, stepped, moving and FB, in
/// each output. Free functions, so each symbol names this module.
pub(super) mod kern {
    use super::super::{FilterMods, SUB, TapShape, ramps, step_to};
    use super::{OUT, RAIL_OUT, Run, Sem, sem_k_tap};
    use crate::dsp::clampf;
    use crate::hw::BLOCK_SIZE;
    use crate::params::FilterParams;

    type Buf = [f32; BLOCK_SIZE];

    /// The door's entry: this block's ends, its taps, then the path its
    /// run asks for. The block's peak |BP| in volts.
    #[inline(never)]
    pub(super) fn run(
        s: &mut Sem,
        buf: &mut Buf,
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> f32 {
        let r = s.ramps(p, fs, mods);
        let t = TapShape::new(mods, r.gr, sem_k_tap);
        let (r, t) = (&r, t.as_ref());
        let pk = if r.bp {
            block::<true>(s, buf, r, t)
        } else {
            block::<false>(s, buf, r, t)
        };
        if !r.fb.on() {
            s.fbp = 0.0;
        }
        pk
    }

    /// One path forced, for `filter::probe::sem_probe`'s count: 0 steady,
    /// 1 stepped, 2 moving, 3 FB; the ends and the taps as `run` builds
    /// them.
    #[cfg(any(test, feature = "probe", feature = "filter-probe"))]
    #[inline(always)]
    pub(in crate::dsp::filter) fn path<const BP: bool, const PATH: u8>(
        s: &mut Sem,
        buf: &mut Buf,
        p: &FilterParams,
        mods: &FilterMods,
    ) -> f32 {
        let r = s.ramps(p, crate::hw::SAMPLE_RATE, mods);
        let t = TapShape::new(mods, r.gr, sem_k_tap);
        match PATH {
            0 => steady::<BP>(s, buf, &r),
            1 => stepped::<BP>(s, buf, &r),
            _ => moving::<BP>(s, buf, &r, t.as_ref()),
        }
    }

    #[inline(always)]
    fn block<const BP: bool>(s: &mut Sem, buf: &mut Buf, r: &Run, t: Option<&TapShape>) -> f32 {
        let still = t.is_none() && !r.fb.on();
        if still && r.g.0 == r.g.1 && r.k.0 == r.k.1 {
            steady::<BP>(s, buf, r)
        } else if still && r.glide {
            stepped::<BP>(s, buf, r)
        } else {
            moving::<BP>(s, buf, r, t)
        }
    }

    /// A ramp's `(from, step)`.
    #[inline(always)]
    fn by(v: (f32, f32)) -> (f32, f32) {
        (v.0, (v.1 - v.0) * (1.0 / BLOCK_SIZE as f32))
    }

    /// One output sample: BANDPASS takes A14's BP, MODE mixes A16's LP
    /// into A12's HP by `m`, at the output's scale. And |BP|.
    #[inline(always)]
    fn out<const BP: bool>(
        s: &mut Sem,
        x: f32,
        a: f32,
        m: f32,
        g: f32,
        kg: f32,
        d: f32,
    ) -> (f32, f32) {
        let (hp, bp, lp) = s.tick(x * a, g, kg, d);
        let rail = (-RAIL_OUT, RAIL_OUT);
        let y = if BP {
            clampf(bp, rail.0, rail.1)
        } else {
            let l = clampf(lp, rail.0, rail.1);
            l + m * (hp - l)
        };
        (y * OUT, bp.abs())
    }

    /// `g` and `k` held: the divide once a block.
    #[inline(always)]
    fn steady<const BP: bool>(s: &mut Sem, buf: &mut Buf, r: &Run) -> f32 {
        let (g, k) = (r.g.1, r.k.1);
        let (kg, d) = (k + g, 1.0 / (1.0 + k * g + g * g));
        let ((mut a, da), (mut m, dm)) = (by(r.a), by(r.m));
        let mut pk = 0.0f32;
        for x in buf.iter_mut() {
            (a, m) = (a + da, m + dm);
            let (y, b) = out::<BP>(s, *x, a, m, g, kg, d);
            (*x, pk) = (y, crate::dsp::maxnm(pk, b));
        }
        pk
    }

    /// A knob glide: `g` and `k` step `STEPS` times to this block's, each
    /// sub-block the steady loop (ADR 0117).
    #[inline(always)]
    fn stepped<const BP: bool>(s: &mut Sem, buf: &mut Buf, r: &Run) -> f32 {
        let ((mut a, da), (mut m, dm)) = (by(r.a), by(r.m));
        let mut pk = 0.0f32;
        for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let (g, k) = (step_to(r.g, j), step_to(r.k, j));
            let (kg, d) = (k + g, 1.0 / (1.0 + k * g + g * g));
            for x in sub {
                (a, m) = (a + da, m + dm);
                let (y, b) = out::<BP>(s, *x, a, m, g, kg, d);
                (*x, pk) = (y, crate::dsp::maxnm(pk, b));
            }
        }
        pk
    }

    /// `g` and `k` ramp to this block's by adding, through the taps if any
    /// and FB if on, and `d` divides every sample.
    #[inline(always)]
    fn moving<const BP: bool>(s: &mut Sem, buf: &mut Buf, r: &Run, t: Option<&TapShape>) -> f32 {
        let (mut gs, mut ks) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut gs, &mut ks, by(r.g), by(r.k), true);
        if let Some(t) = t {
            t.apply(&mut gs, &mut ks);
        }
        let ((mut a, da), (mut m, dm)) = (by(r.a), by(r.m));
        let mut pk = 0.0f32;
        let fb = r.fb;
        if fb.on() {
            for (i, x) in buf.iter_mut().enumerate() {
                (a, m) = (a + da, m + dm);
                let (g, k) = (fb.g(gs[i], i, s.fbp, r.gr), ks[i]);
                let d = 1.0 / (1.0 + k * g + g * g);
                let (y, b) = out::<BP>(s, *x, a, m, g, k + g, d);
                (*x, pk) = (y, crate::dsp::maxnm(pk, b));
                fb.feed(&mut s.fbp, y);
            }
        } else {
            for (i, x) in buf.iter_mut().enumerate() {
                (a, m) = (a + da, m + dm);
                let (g, k) = (gs[i], ks[i]);
                let d = 1.0 / (1.0 + k * g + g * g);
                let (y, b) = out::<BP>(s, *x, a, m, g, k + g, d);
                (*x, pk) = (y, crate::dsp::maxnm(pk, b));
            }
        }
        pk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pot law and the parasitics match the lab's `fw::coefs` printout
    /// (`semc-consts`): `k` at 1 kHz for RES 0, ½, 0.9, 1, and at RES 1 for
    /// 100 Hz and 10 kHz.
    #[test]
    fn k_is_the_labs() {
        for (fc, res, want) in [
            (1000.0, 0.0, 3.027_788),
            (1000.0, 0.5, 1.110_744),
            (1000.0, 0.9, 0.283_552),
            (1000.0, 1.0, 0.019_301),
            (100.0, 1.0, 0.014_494),
            (10000.0, 1.0, 0.066_124),
        ] {
            let k = sem_k(fc, res);
            assert!(
                (k - want).abs() <= 2e-5 * want.max(1.0),
                "{fc} {res}: {k} vs {want}"
            );
        }
        let g = sem_g(1000.0, 48000.0);
        assert!((g - 0.065_543_46).abs() < 1e-6, "{g}");
    }
}
