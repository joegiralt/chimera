//! The plain kernel: one source under one decay. PLAIN SINE, NOISE and
//! IMPULSE (`Plain`, machines.rs:2197).

use crate::dsp::Unit;
use crate::dsp::filter::FilterMode;
use crate::hw::BLOCK_SIZE;

use super::super::laws::{NoiseTone, PlainParams};
use super::super::types::{Hz, PulsePace};
use super::{Blk, Decay, Hiss, Hit, Octaves, Pulse, Svf, Swept, octaves, quiet};

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
        oct: Octaves,
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
                    oct: Octaves::of_semis(body.depth),
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

    /// `KernelState::pace`: the impulse's pulse (`Pulse::pace`); SINE and
    /// NOISE have none.
    pub(super) fn pace(&self) -> PulsePace {
        match &self.src {
            Source::Impulse { p, f0, oct, sw, .. } => p.pace(*f0, *oct, sw),
            Source::Sine(_) | Source::Noise { .. } => PulsePace::Runs,
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
        HitSeed, Knob, KnobSet, KnobValue, Machine, PulsePace, SampleRate, Transpose,
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

    /// When a case's pace returns to square runs.
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

    /// ADR 0287: `pace` is swept before every block whose sweep moves the
    /// pulse, floored exactly when the block's width floors at one of its
    /// samples, and once it runs it stays running. SWEEP 0 never ticks;
    /// SWEEP 64 till its sweep is spent; full WIDTH at 2 kHz an octave up
    /// floors for as long as it sounds, swept too at SWEEP 127; at its
    /// tuned 2 kHz SWEEP 127's head floors it, its tail doesn't.
    #[test]
    fn pace_covers_every_ticked_block() {
        const LONG: (Knob, u8) = (Knob::B, 127);
        const HIGH: [(Knob, u8); 3] = [LONG, (Knob::A, 127), (Knob::G, 127)];
        const HIGH_SWEPT: [(Knob, u8); 5] = [
            LONG,
            (Knob::A, 127),
            (Knob::G, 127),
            (Knob::C, 127),
            (Knob::D, 30),
        ];
        let cases: [Case; 7] = [
            (&[LONG], 0, Stops::AtOnce),
            (&[LONG, (Knob::C, 1)], 0, Stops::Later),
            (&[LONG, (Knob::C, 64)], 0, Stops::Later),
            (&[LONG, (Knob::C, 127), (Knob::D, 30)], 0, Stops::Later),
            (&HIGH, 12, Stops::Never),
            (&HIGH_SWEPT, 12, Stops::Never),
            (&HIGH_SWEPT, 0, Stops::Later),
        ];
        for (n, &(knobs, by, want)) in cases.iter().enumerate() {
            let mut k = impulse(knobs, by);
            let (mut stopped, mut floors) = (None, 0);
            for b in 0..400 {
                let pace = k.pace();
                let Source::Impulse { p, f0, oct, sw, .. } = &k.src else {
                    panic!("an impulse");
                };
                let (mut d, mut m) = (*sw, [0.0; BLOCK_SIZE]);
                octaves(&mut d, *oct, &mut m);
                let swept = m.iter().any(|&x| x != 1.0);
                let floored = m.iter().any(|&x| p.floored(*f0 * x));
                let said_swept = matches!(pace, PulsePace::Swept | PulsePace::FlooredSwept);
                let said_floored = matches!(pace, PulsePace::Floored | PulsePace::FlooredSwept);
                assert!(said_swept || !swept, "case {n}, block {b}");
                assert_eq!(said_floored, floored, "case {n}, block {b}");
                assert!(pace != PulsePace::Runs || p.steady(*f0, &m).is_some());
                floors += usize::from(floored);
                match stopped {
                    None if pace == PulsePace::Runs => stopped = Some(b),
                    Some(_) => assert_eq!(pace, PulsePace::Runs, "case {n}, block {b}"),
                    None => {}
                }
                k.render::<false>(&mut [0.0; BLOCK_SIZE]);
            }
            match want {
                Stops::AtOnce => assert_eq!(stopped, Some(0), "case {n}"),
                Stops::Later => assert!(stopped.is_some_and(|s| s > 0), "case {n}"),
                Stops::Never => assert_eq!(stopped, None, "case {n}"),
            }
            // The two high cases and the tuned sweep's head floor.
            assert_eq!(floors > 0, n >= 4, "case {n}");
        }
    }
}
