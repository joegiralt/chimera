//! The plain kernel: one source under one decay. PLAIN SINE, NOISE and
//! IMPULSE (`Plain`, machines.rs:2197).

use crate::dsp::Unit;
use crate::dsp::filter::FilterMode;
use crate::hw::BLOCK_SIZE;

use super::super::laws::{NoiseTone, PlainParams};
use super::super::types::{Hz, PulseMachine};
use super::{Blk, Decay, Hiss, Hit, Pulse, Svf, Swept, octaves, quiet, sweeping};

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

    /// `KernelState::ticking`: the impulse's while SWEEP moves its pulse
    /// or its width floors.
    pub(super) fn ticking(&self) -> Option<PulseMachine> {
        match &self.src {
            Source::Impulse { p, f0, oct, sw, .. } => {
                (sweeping(*oct, sw.v) || p.floored(*f0)).then_some(PulseMachine::Impulse)
            }
            Source::Sine(_) | Source::Noise { .. } => None,
        }
    }

    pub(super) fn finished(&self) -> bool {
        self.quiet && self.amp.spent()
    }
}

#[cfg(test)]
mod tests {
    use chimera_midi::value::U7;

    use super::super::super::laws::resolve;
    use super::super::super::types::{
        HitSeed, Knob, KnobSet, KnobValue, Machine, SampleRate, Transpose,
    };
    use super::super::{Hit, octaves};
    use super::{BLOCK_SIZE, Plain, Source, Unit};

    /// A PLAIN IMPULSE at `knobs` over its defaults, `by` semitones up.
    fn impulse(knobs: &[(Knob, u8)], by: i8) -> Plain {
        let mut k = KnobSet::defaults(Machine::PlainImpulse);
        for &(kn, v) in knobs {
            k.set(kn, KnobValue::new(U7::new(v).expect("0..=127")));
        }
        let p =
            resolve(Machine::PlainImpulse, &k).transposed(Transpose::new(by).expect("a transpose"));
        let super::super::super::laws::KernelParams::Plain(p) = p else {
            panic!("IMPULSE resolves to the plain kernel");
        };
        let h = Hit {
            level: Unit::ONE,
            seed: HitSeed::new(1),
            sr: SampleRate::HW,
        };
        Plain::new(p, h)
    }

    /// When a case's `ticking` stops.
    #[derive(Clone, Copy)]
    enum Stops {
        /// Before the first block: it never ticks.
        AtOnce,
        /// After some blocks: its sweep is spent.
        Later,
        /// Not while it sounds: its width floors.
        Never,
    }

    /// IMPULSE's knobs over its defaults, its transpose and when it stops.
    type Case = (&'static [(Knob, u8)], i8, Stops);

    /// ADR 0287: `ticking` says so before every block whose pulse renders
    /// a sample at a time, and once it stops it stays stopped. SWEEP 0
    /// never ticks; SWEEP 64 ticks till its sweep is spent; full WIDTH at
    /// 2 kHz an octave up floors and ticks for as long as it sounds.
    #[test]
    fn ticking_covers_every_ticked_block() {
        const LONG: (Knob, u8) = (Knob::B, 127);
        let cases: [Case; 5] = [
            (&[LONG], 0, Stops::AtOnce),
            (&[LONG, (Knob::C, 1)], 0, Stops::Later),
            (&[LONG, (Knob::C, 64)], 0, Stops::Later),
            (&[LONG, (Knob::C, 127), (Knob::D, 30)], 0, Stops::Later),
            (&[LONG, (Knob::A, 127), (Knob::G, 127)], 12, Stops::Never),
        ];
        for (n, &(knobs, by, want)) in cases.iter().enumerate() {
            let mut k = impulse(knobs, by);
            let mut stopped = None;
            for b in 0..400 {
                let said = k.ticking().is_some();
                let Source::Impulse { p, f0, oct, sw, .. } = &k.src else {
                    panic!("an impulse");
                };
                let (mut d, mut m) = (*sw, [0.0; BLOCK_SIZE]);
                octaves(&mut d, *oct, &mut m);
                let ticks = p.steady(*f0, &m).is_none();
                assert!(said || !ticks, "case {n}, block {b}");
                match stopped {
                    None if !said => stopped = Some(b),
                    Some(_) => assert!(!said, "case {n}, block {b}"),
                    None => {}
                }
                k.render::<false>(&mut [0.0; BLOCK_SIZE]);
            }
            match want {
                Stops::AtOnce => assert_eq!(stopped, Some(0), "case {n}"),
                Stops::Later => assert!(stopped.is_some_and(|s| s > 0), "case {n}"),
                Stops::Never => assert_eq!(stopped, None, "case {n}"),
            }
        }
    }
}
