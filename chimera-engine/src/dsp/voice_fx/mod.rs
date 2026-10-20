//! The voice's one FX slot, PRE or POST the filter (ADR 0155): its type,
//! place and pots (`params`), and `VoiceFx`, which runs them.

pub mod fold;
pub mod fuzz;
pub mod legacy;
pub mod params;
pub mod soft;

use crate::dsp::ease::{Gate, Ramp, at, step_of};
use crate::hw::BLOCK_SIZE;
use crate::taps::TapLane;
use params::{FxType, Place, VoiceFxParams, pot_level};

/// The TYPE and PLACE switch's crossfade, in samples: 20 ms at 48 kHz, the
/// stage gate's. Old and new run together, unbilled, as the filter's fade.
pub const FX_FADE: usize = 960;

const _: () = assert!(FX_FADE.is_multiple_of(BLOCK_SIZE));

/// A block's ramp ends, `(from, to)`: P1-P3, MIX and the stage gate. The
/// slot steps its ramps and hands the kernels these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ends {
    pub pots: [(f32, f32); 3],
    pub mix: (f32, f32),
    pub gate: (f32, f32),
}

impl Ends {
    /// P3's LEVEL gain at the block's ends: the wet's, exactly 1 at 1/2.
    #[inline]
    pub(crate) fn level(&self) -> (f32, f32) {
        let (a, b) = self.pots[2];
        (pot_level(a), pot_level(b))
    }

    /// Nothing moves and the gate is open: the steady loop's case.
    #[inline]
    pub(crate) fn steady(&self) -> bool {
        self.pots.iter().all(|p| p.0 == p.1) && self.mix.0 == self.mix.1 && self.gate == (1.0, 1.0)
    }
}

/// `t`'s memoryless transfer at MIX 1, for the FX page's curve: OFF is the
/// dry line.
pub fn curve(t: FxType, x: f32, pots: [f32; 3]) -> f32 {
    match t {
        FxType::Off => x,
        FxType::Soft => soft::curve(x, pots),
        FxType::Fuzz => fuzz::curve(x, pots),
        FxType::Fold => fold::curve(x, pots),
    }
}

/// What a block runs, `(type, place)`. OFF runs nowhere, so its place is
/// PRE's: an OFF slot's PLACE turn starts no fade.
type Setting = (FxType, Place);

fn setting(p: &VoiceFxParams) -> Setting {
    match p.ty() {
        FxType::Off => (FxType::Off, Place::Pre),
        t => (t, p.place),
    }
}

/// One side of a block: its setting and its weight at the block's ends.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Side {
    set: Setting,
    w: (f32, f32),
}

/// A block of the slot (`VoiceFx::step`): the ramp ends and the fade's two
/// sides, the incoming one first. Both sides read the same pots: a switch
/// changes what they mean, not where they are. Outside a fade the second
/// side is OFF, which runs nowhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxBlock {
    ends: Ends,
    sides: [Side; 2],
}

/// A switch under way: the setting it leaves and the samples it has run.
#[derive(Clone, Copy, Debug)]
struct Fade {
    from: Setting,
    done: usize,
}

/// A voice's FX slot: P1–P3's and MIX's ramps, the run gate, the setting
/// the last block ran, the switch in progress and FUZZ's memory, one a
/// place. A TYPE or PLACE change crossfades from the old setting over
/// `FX_FADE`, never snapped; one arriving mid-fade waits for its end.
#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceFx {
    pots: [Ramp; 3],
    mix: Ramp,
    gate: Gate,
    /// The last block's setting (a fade's destination); `None` before a
    /// note's first.
    last: Option<Setting>,
    /// No `pending`: the params hold the latest setting, so a change made
    /// mid-fade is read again when the fade ends, and the last one wins.
    fade: Option<Fade>,
    /// FUZZ PRE's and POST's: a PLACE fade runs both sides at once, each on
    /// its own signal, so each keeps its own memory.
    fuzz: [fuzz::Fuzz; 2],
}

impl VoiceFx {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh note: no ramp or fade from the last one; its first block
    /// lands on its setting.
    pub fn hold(&mut self) {
        *self = Self::new();
    }

    /// Once a block: the ramps and the gate (`on`: the slot runs) stepped,
    /// and the fade stepped, or started by a change of TYPE or PLACE.
    pub fn step(&mut self, p: &VoiceFxParams, on: bool) -> FxBlock {
        let ends = Ends {
            pots: core::array::from_fn(|i| self.pots[i].step(p.pots[i])),
            mix: self.mix.step(p.mix),
            gate: self.gate.step(on),
        };
        let want = setting(p);
        let to = match (self.fade, self.last) {
            (Some(_), Some(to)) => to,
            (_, Some(from)) if from != want => {
                self.fade = Some(Fade { from, done: 0 });
                // The side fading in starts fresh, not on a past run's memory.
                if want.0 == FxType::Fuzz {
                    self.fuzz[want.1 as usize] = fuzz::Fuzz::new();
                }
                want
            }
            _ => want,
        };
        self.last = Some(to);
        let sides = match self.fade {
            None => [
                Side {
                    set: to,
                    w: (1.0, 1.0),
                },
                Side {
                    set: (FxType::Off, Place::Pre),
                    w: (0.0, 0.0),
                },
            ],
            Some(f) => {
                let next = f.done + BLOCK_SIZE;
                let w = |n: usize| n as f32 * (1.0 / FX_FADE as f32);
                let (a, b) = (w(f.done), w(next));
                self.fade = (next < FX_FADE).then_some(Fade { done: next, ..f });
                [
                    Side { set: to, w: (a, b) },
                    Side {
                        set: f.from,
                        w: (1.0 - a, 1.0 - b),
                    },
                ]
            }
        };
        FxBlock { ends, sides }
    }

    /// The slot at `hook`, PRE or POST the filter: each side placed here
    /// runs on the same input `x`, at the engine's reference `scale`
    /// (`EngineType::fx_scale`), and adds its weighted wet change:
    /// `y = x − Σ w·(x·scale − kernel(x·scale)) / scale`; one side alone at
    /// full weight and scale 1 is the kernel's output. A kernel at MIX 0
    /// returns `x·scale` exactly, so its change is ±0, the sum (from +0)
    /// +0 and `y` is `x` bit for bit, up to the sign of zero (the lone fast
    /// path returns the kernel's, and FOLD's is +0 for −0). `lane` reaches
    /// only FOLD.
    pub fn apply(
        &mut self,
        b: &FxBlock,
        hook: Place,
        buf: &mut [f32; BLOCK_SIZE],
        scale: f32,
        glide: bool,
        lane: Option<&TapLane>,
    ) {
        let runs = |s: &&Side| s.set.1 == hook && s.set.0 != FxType::Off && s.w != (0.0, 0.0);
        let n = b.sides.iter().filter(runs).count();
        if n == 0 {
            return;
        }
        // One side, whole, at unit scale: the kernel's output itself, bit
        // for bit today's DRV and fold, not `x − (x − k)` a rounding off.
        let lone = n == 1 && scale == 1.0;
        let mut d = [0.0f32; BLOCK_SIZE];
        for s in b.sides.iter().filter(runs) {
            let mut k: [f32; BLOCK_SIZE] = core::array::from_fn(|i| buf[i] * scale);
            match s.set.0 {
                FxType::Off => {}
                FxType::Soft => soft::run(&mut k, &b.ends, glide),
                FxType::Fuzz => self.fuzz[s.set.1 as usize].run(&mut k, &b.ends, glide),
                FxType::Fold => fold::run(&mut k, &b.ends, glide, lane),
            }
            if lone && s.w == (1.0, 1.0) {
                *buf = k;
                return;
            }
            let sw = step_of(s.w, BLOCK_SIZE);
            for (i, (d, k)) in d.iter_mut().zip(&k).enumerate() {
                *d += at(s.w.0, sw, i) * (buf[i] * scale - k);
            }
        }
        let inv = 1.0 / scale;
        for (y, d) in buf.iter_mut().zip(&d) {
            *y -= d * inv;
        }
    }
}

/// What the slot does around a kernel, for the kernels' tests: steps the
/// ramps and gate into `Ends`.
#[cfg(test)]
pub(crate) mod rig {
    use super::Ends;
    use crate::dsp::ease::{Gate, Ramp};
    use crate::hw::BLOCK_SIZE;

    #[derive(Default)]
    pub struct Rig(pub [Ramp; 3], pub Ramp, pub Gate);

    impl Rig {
        pub fn ends(&mut self, pots: [f32; 3], mix: f32, on: bool) -> Ends {
            Ends {
                pots: [0, 1, 2].map(|i| self.0[i].step(pots[i])),
                mix: self.1.step(mix),
                gate: self.2.step(on),
            }
        }
    }

    /// FNV-1a 64 over `b`'s sample bits, from `h`: the old stages' outputs,
    /// recorded before they were deleted, are kept as these.
    pub fn fnv(mut h: u64, b: &[f32]) -> u64 {
        for x in b {
            for byte in x.to_bits().to_le_bytes() {
                h = (h ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    pub const FNV0: u64 = 0xcbf2_9ce4_8422_2325;

    pub fn sine(b: usize, amp: f32, hz: f32) -> [f32; BLOCK_SIZE] {
        core::array::from_fn(|i| {
            let t = (b * BLOCK_SIZE + i) as f32 / 48_000.0;
            amp * libm::sinf(core::f32::consts::TAU * hz * t)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::params::{FxType, Place, VoiceFxParams};
    use super::rig::{Rig, sine};
    use super::*;

    /// A lone SOFT at scale 1 and full weight is the kernel's output, bit
    /// for bit (today's DRV), not `x − (x − k)`: high DRIVE, where the
    /// difference rounds.
    #[test]
    fn a_lone_side_at_unit_scale_is_the_kernel_bit_for_bit() {
        let mut q = VoiceFxParams::default();
        q.set_type(FxType::Soft);
        (q.pots, q.mix) = ([0.9, 0.3, 0.5], 0.8);
        let (mut fx, mut rig) = (VoiceFx::new(), Rig::default());
        for b in 0..8 {
            let mut got = sine(b, 0.7, 110.0);
            let mut want = got;
            let blk = fx.step(&q, true);
            fx.apply(&blk, Place::Pre, &mut got, 1.0, false, None);
            soft::run(&mut want, &rig.ends(q.pots, q.mix, true), false);
            let bits = |x: &[f32; BLOCK_SIZE]| x.map(f32::to_bits);
            assert_eq!(bits(&got), bits(&want), "block {b}");
        }
    }
}
