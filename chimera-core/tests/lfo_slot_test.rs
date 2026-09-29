//! LFO slots (filter-routing spec § LFO slots).
mod common;
use common::Rig;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::lfo::{Lfo, LfoParams};
use chimera_core::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use chimera_core::dsp::modulator::{FuncParams, LfoForm, LfoType};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

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

/// Spec § LFO slots: a note-on resets FUNC's φ on SYNC; FREE runs on.
#[test]
fn func_note_on_resets_phase_on_sync_only() {
    for (form, resets) in [(LfoForm::Sync, true), (LfoForm::Free, false)] {
        let p = LfoParams {
            lfo_type: LfoType::Func,
            func: FuncParams {
                lfo_form: form,
                ..FuncParams::LFO
            },
            ..LfoParams::default()
        };
        let (mut l, mut run_on, mut fresh) = (Lfo::new(), Lfo::new(), Lfo::new());
        for _ in 0..100 {
            l.run_block(&p, SR);
            run_on.run_block(&p, SR);
        }
        l.note_on(&p);
        let (x, y, z) = (
            l.run_block(&p, SR),
            run_on.run_block(&p, SR),
            fresh.run_block(&p, SR),
        );
        assert_ne!(y, z, "{form:?}: 100 blocks in, φ has moved");
        assert_eq!(x, if resets { z } else { y }, "{form:?}");
    }
}

/// Two voices whose LFO 1 ran at different rates (route at 0, unheard),
/// then a note-on with LFO 1 → CUTOFF at 16 (not pinned to the range's edge): with SYNC 1 both restart at
/// PHASE and render alike; with SYNC 0 their phases still differ.
fn after_retrigger(sync: u8, phase: f32, pre_rate: f32) -> Vec<f32> {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.cutoff = 1000.0;
    p.lfos[0] = LfoParams {
        shape: 2,
        rate: pre_rate,
        sync,
        phase_offset: phase,
        ..LfoParams::default()
    };
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF, *b"FLTCUTOF").unwrap();
    let mut ms = ModState::from_registry(&reg, 8);
    let mut v = Rig::new(SR);
    let (n, vel) = (MidiNote::new(60).unwrap(), Velocity::new(100).unwrap());
    let mut b = [0.0f32; BLOCK_SIZE];
    v.note_on(n, vel, &p);
    for _ in 0..7 {
        v.render(&mut b, &p, &ms);
    }
    p.lfos[0].rate = 3.0;
    ms.set_amount(ModSource::Lfo1.index(), 0, 16);
    v.note_on(n, vel, &p);
    let mut out = Vec::new();
    for _ in 0..24 {
        v.render(&mut b, &p, &ms);
        out.extend_from_slice(&b);
    }
    out
}

#[test]
fn classic_sync_retriggers_at_phase() {
    assert_eq!(
        after_retrigger(1, 0.25, 1.0),
        after_retrigger(1, 0.25, 9.0),
        "SYNC 1 restarts"
    );
    assert_ne!(
        after_retrigger(0, 0.0, 1.0),
        after_retrigger(0, 0.0, 9.0),
        "SYNC 0 runs on"
    );
    assert_ne!(
        after_retrigger(1, 0.25, 1.0),
        after_retrigger(1, 0.5, 1.0),
        "PHASE is the restart point"
    );
}
