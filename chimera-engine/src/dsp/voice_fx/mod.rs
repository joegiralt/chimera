//! The voice's one FX slot, PRE or POST the filter (ADR 0155): its type,
//! place and pots (`params`), and `VoiceFx`, which runs them.

pub mod fold;
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
/// the last block ran and the switch in progress. A TYPE or PLACE change
/// crossfades from the old setting over `FX_FADE`, never snapped; one
/// arriving mid-fade waits for its end.
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
    /// `y = x − Σ w·(x·scale − kernel(x·scale)) / scale`. A kernel at MIX 0
    /// returns `x·scale` exactly, so its change is ±0, the sum (from +0)
    /// +0 and `y` is `x` bit for bit, −0 too. `lane` reaches only FOLD.
    pub fn apply(
        &mut self,
        b: &FxBlock,
        hook: Place,
        buf: &mut [f32; BLOCK_SIZE],
        scale: f32,
        glide: bool,
        lane: Option<&TapLane>,
    ) {
        let mut d = [0.0f32; BLOCK_SIZE];
        let mut ran = false;
        for s in &b.sides {
            if s.set.1 != hook || s.w == (0.0, 0.0) {
                continue;
            }
            let mut k: [f32; BLOCK_SIZE] = core::array::from_fn(|i| buf[i] * scale);
            match s.set.0 {
                FxType::Off => continue,
                FxType::Soft => soft::run(&mut k, &b.ends, glide),
                FxType::Fold => fold::run(&mut k, &b.ends, glide, lane),
            }
            let sw = step_of(s.w, BLOCK_SIZE);
            for (i, (d, k)) in d.iter_mut().zip(&k).enumerate() {
                *d += at(s.w.0, sw, i) * (buf[i] * scale - k);
            }
            ran = true;
        }
        if ran {
            let inv = 1.0 / scale;
            for (y, d) in buf.iter_mut().zip(&d) {
                *y -= d * inv;
            }
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

    pub fn sine(b: usize, amp: f32, hz: f32) -> [f32; BLOCK_SIZE] {
        core::array::from_fn(|i| {
            let t = (b * BLOCK_SIZE + i) as f32 / 48_000.0;
            amp * libm::sinf(core::f32::consts::TAU * hz * t)
        })
    }
}
