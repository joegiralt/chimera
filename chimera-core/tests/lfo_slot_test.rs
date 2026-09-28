//! LFO slots (filter-routing spec § LFO slots).

use chimera_core::dsp::lfo::{Lfo, LfoParams};
use chimera_core::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use chimera_core::dsp::modulator::{FuncParams, LfoType};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

/// Today's LFO, verbatim (with OFFSET at its only value, 0).
struct Reference {
    phase: f32,
    random_value: f32,
    rng_state: u32,
}

impl Reference {
    fn process(&mut self, p: &LfoParams) -> f32 {
        let raw = match p.shape {
            0 => chimera_core::dsp::fast_sin(self.phase * core::f32::consts::TAU),
            1 => {
                let t = self.phase;
                if t < 0.25 {
                    t * 4.0
                } else if t < 0.75 {
                    2.0 - t * 4.0
                } else {
                    t * 4.0 - 4.0
                }
            }
            2 => self.phase * 2.0 - 1.0,
            3 => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => self.random_value,
        };
        let out = (raw * p.depth + 0.0).clamp(-1.0, 1.0);
        self.phase += p.rate / SR as f32 * BLOCK_SIZE as f32;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            if p.shape == 4 {
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                self.random_value = (self.rng_state >> 16) as f32 / 32768.0 - 1.0;
            }
        }
        out
    }
}

/// Spec § Tests: LFO 1 CLASSIC is bit-identical to today's LFO over 1,000 blocks.
#[test]
fn classic_is_todays_lfo() {
    for shape in 0..5u8 {
        for (rate, depth) in [(1.0, 1.0), (5.0, 0.7), (19.0, 0.3)] {
            let p = LfoParams {
                shape,
                rate,
                depth,
                ..LfoParams::default()
            };
            let mut new = Lfo::new();
            let mut old = Reference {
                phase: 0.0,
                random_value: 0.0,
                rng_state: 12345,
            };
            for b in 0..1000 {
                assert_eq!(
                    new.run_block(&p, SR),
                    old.process(&p),
                    "shape {shape} rate {rate} block {b}"
                );
            }
        }
    }
}

#[test]
fn offset_is_no_longer_applied() {
    let with = LfoParams {
        offset: 0.5,
        ..LfoParams::default()
    };
    let (mut a, mut b) = (Lfo::new(), Lfo::new());
    for _ in 0..100 {
        assert_eq!(
            a.run_block(&with, SR),
            b.run_block(&LfoParams::default(), SR)
        );
    }
}

/// FUNC is Envelope B locked to LFO mode (DEPTH not applied).
#[test]
fn func_matches_b_in_lfo_mode() {
    let p = LfoParams {
        lfo_type: LfoType::Func,
        depth: 0.2,
        func: FuncParams {
            rise: 0.4,
            shape: 0.2,
            ..FuncParams::LFO
        },
        ..LfoParams::default()
    };
    let c = BCoefs::new(&p.func, &Slides::default(), SR, false);
    let mut lfo = Lfo::new();
    let mut g = FuncGen::new();
    g.set(&c);
    for b in 0..200 {
        let want = g.output();
        g.advance(&c, false, BLOCK_SIZE as u32);
        assert_eq!(lfo.run_block(&p, SR), want, "block {b}");
    }
}

/// A TYPE change glides: no step, and the glide is gone after 256 samples.
#[test]
fn a_type_change_glides_out() {
    let classic = LfoParams {
        shape: 3,
        ..LfoParams::default()
    };
    let func = LfoParams {
        lfo_type: LfoType::Func,
        ..LfoParams::default()
    };
    let mut l = Lfo::new();
    let mut last = 0.0;
    for _ in 0..30 {
        last = l.run_block(&classic, SR);
    }
    // FUNC from its first block: the same generator state as `l`'s.
    let mut fresh = Lfo::new();
    assert_eq!(l.run_block(&func, SR), last, "no step");
    fresh.run_block(&func, SR);
    for b in 1..8 {
        let (x, y) = (l.run_block(&func, SR), fresh.run_block(&func, SR));
        if b >= 4 {
            assert_eq!(x, y, "block {b}: the glide is over");
        }
    }
}
