//! The plain kernel: one source under one decay. PLAIN SINE, NOISE and
//! IMPULSE (`Plain`, machines.rs:2197).

use crate::dsp::Unit;
use crate::dsp::filter::FilterMode;
use crate::hw::BLOCK_SIZE;

use super::super::laws::{NoiseTone, PlainParams};
use super::super::types::Hz;
use super::{Blk, Decay, Hiss, Hit, Pulse, Svf, Swept, octaves, quiet};

pub struct Plain {
    src: Source,
    amp: Decay,
    vel: f32,
    quiet: bool,
}

#[allow(clippy::large_enum_variant)]
enum Source {
    Sine(Swept),
    /// The pulse, its pitch, sweep (octaves) and sweep's fall, and a
    /// 20 Hz high-pass for its DC.
    Impulse {
        p: Pulse,
        f0: f32,
        oct: f32,
        sw: Decay,
        hp: Svf,
    },
    Noise {
        hiss: Hiss,
        f: Svf,
    },
}

impl Plain {
    pub(super) fn new(p: PlainParams, h: Hit) -> Plain {
        let (src, decay) = match p {
            PlainParams::Sine { body, decay } => (Source::Sine(Swept::of(body, h.sr)), decay),
            PlainParams::Impulse { body, decay, width } => (
                Source::Impulse {
                    p: Pulse::new(width, h.sr),
                    f0: body.pitch.get(),
                    oct: body.depth.get() / 12.0,
                    sw: Decay::tau(body.time, h.sr),
                    hp: Svf::new(FilterMode::Hp24, Hz::new(20.0), Unit::ZERO, h.sr),
                },
                decay,
            ),
            PlainParams::Noise(n) => {
                // TONE: below ½ a low-pass closing, above a high-pass opening.
                let (mode, hz) = match n.tone {
                    NoiseTone::LowPass(hz) => (FilterMode::Lp12, hz),
                    NoiseTone::HighPass(hz) => (FilterMode::Hp24, hz),
                };
                (
                    Source::Noise {
                        hiss: Hiss::new(h.seed),
                        f: Svf::new(mode, hz, Unit::ZERO, h.sr),
                    },
                    n.decay,
                )
            }
        };
        Plain {
            src,
            amp: Decay::t60(decay, h.sr),
            vel: h.level.get(),
            quiet: false,
        }
    }

    /// The next block; `TICKED`, the impulse a sample at a time
    /// (`Pulse::sweep`).
    #[inline(never)]
    pub(super) fn render<const TICKED: bool>(&mut self, out: &mut Blk) {
        match &mut self.src {
            Source::Sine(b) => {
                b.block(out, None);
                for o in out.iter_mut() {
                    *o *= 0.9;
                }
            }
            Source::Impulse { p, f0, oct, sw, hp } => {
                let mut m = [0.0; BLOCK_SIZE];
                octaves(sw, *oct, &mut m);
                p.sweep::<TICKED>(*f0, &m, out);
                for o in out.iter_mut() {
                    *o *= -1.2;
                }
                hp.run(out);
            }
            Source::Noise { hiss, f } => {
                hiss.block(out);
                for o in out.iter_mut() {
                    *o *= 0.4;
                }
                f.run(out);
            }
        }
        for o in out.iter_mut() {
            *o *= self.vel * self.amp.next();
        }
        self.quiet = quiet(out);
    }

    pub(super) fn finished(&self) -> bool {
        self.quiet && self.amp.spent()
    }
}
