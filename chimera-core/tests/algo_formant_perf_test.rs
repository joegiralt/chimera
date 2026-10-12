//! FRMT renders in spans between grain starts (Task 11b): bit for bit the
//! per-sample path, and its cost term against the formant budget.

mod common;

use chimera_core::dsp::algo::algorithms::{ALGO_COUNT, AlgoId};
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::waves::WaveId;
use common::formant::{render_algo_ref, render_algo_with};

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// What moves between blocks: a stepped knob or route, as a square LFO
/// steps it.
#[derive(Clone, Copy, Debug)]
enum Jump {
    Bw(usize, f32),
    Frmt(usize, f32),
    Feedback(usize, f32),
    Level(usize, f32),
    Morph(f32),
    Pitch(f32),
}

#[derive(Clone, Debug)]
struct Jumps(Vec<(usize, Jump)>);

impl Jumps {
    /// Plays every jump due by block `b`, from then on.
    fn apply(&self, b: usize, live: &mut AlgoLive, e: &mut AlgoEngine) {
        for &(at, j) in self.0.iter().filter(|(at, _)| *at <= b) {
            match j {
                Jump::Bw(i, v) => live.bw[i] = v,
                Jump::Frmt(i, v) => live.frmt[i] = v,
                Jump::Feedback(i, v) => live.feedback[i] = v,
                Jump::Level(i, v) => live.level[i] = v,
                Jump::Morph(v) => live.morph = v,
                Jump::Pitch(st) if at == b => e.set_pitch(st),
                Jump::Pitch(_) => {}
            }
        }
    }
}

/// A random patch of table, FRMT and NOIS operators at random SKIRT, BW,
/// FRMT, FDBK and LEVELs, on random algorithms (so PM links), with BW,
/// FRMT, FDBK, LEVEL, MORPH and pitch jumps.
fn random_formant_case(seed: u32) -> (AlgoParams, u8, usize, Jumps) {
    let mut r = Rng(seed.wrapping_mul(0x9e37_79b9) | 1);
    let mut p = AlgoParams::single(WaveId::W1);
    p.try_set_algs(
        AlgoId::clamped(r.below(ALGO_COUNT as u32) as u8),
        AlgoId::clamped(r.below(ALGO_COUNT as u32) as u8),
    )
    .unwrap();
    p.morph = r.below(100) as u8;
    p.vowel = r.below(101) as u8;
    for i in 0..OPS {
        let kind = match r.below(4) {
            0 | 1 => OpKind::Formant,
            2 => OpKind::Noise,
            _ => OpKind::Table(WaveId::clamped(r.below(16) as u8)),
        };
        let _ = p.try_set_kind(i, kind);
        let o = &mut p.ops[i];
        o.coarse = r.below(16) as u8;
        o.level = [0, 60, 80, 99][r.below(4) as usize];
        o.feedback = r.below(8) as u8;
        (o.frmt, o.bw, o.skirt) = (r.below(169) as u8, r.below(217) as u8, r.below(4) as u8);
        (o.key, o.res, o.vslot) = (r.below(101) as u8, r.below(101) as u8, r.below(5) as u8);
    }
    let note = 24 + r.below(72) as u8;
    let blocks = 40 + r.below(40) as usize;
    let mut jumps = Vec::new();
    for _ in 0..r.below(8) {
        let (at, i) = (
            r.below(blocks as u32) as usize,
            r.below(OPS as u32) as usize,
        );
        let j = match r.below(6) {
            0 => Jump::Bw(i, r.below(217) as f32),
            1 => Jump::Frmt(i, r.below(169) as f32),
            2 => Jump::Feedback(i, r.below(8) as f32),
            3 => Jump::Level(i, r.below(100) as f32),
            4 => Jump::Morph(r.below(100) as f32),
            _ => Jump::Pitch(r.below(25) as f32 - 12.0),
        };
        jumps.push((at, j));
    }
    (p, note, blocks, Jumps(jumps))
}

#[test]
fn spans_are_bit_exact_against_the_per_sample_path() {
    for seed in 0..200u32 {
        let (p, note, blocks, jumps) = random_formant_case(seed);
        let j = jumps.clone();
        let (fast, cuts_fast) =
            render_algo_with(&p, note, blocks, |b, _, live, e| jumps.apply(b, live, e));
        let (slow, cuts_slow) =
            render_algo_ref(&p, note, blocks, |b, _, live, e| j.apply(b, live, e));
        assert!(
            fast.iter()
                .zip(&slow)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "seed {seed}"
        );
        assert_eq!(cuts_fast, cuts_slow);
    }
}

/// The formant and noise laws, held while their inputs hold, are the laws
/// taken afresh every block, bit for bit: through knob, route and pitch
/// jumps and stored edits.
#[test]
fn held_laws_are_the_laws_taken_afresh() {
    for seed in 0..100u32 {
        let (p, note, blocks, jumps) = random_formant_case(seed);
        // One stored value at a time, each its own block.
        let edit = |b: usize, p: &mut AlgoParams| match b * 6 / blocks {
            _ if b * 6 % blocks >= 6 => {}
            1 => p.transpose += 1,
            2 => p.vowel = (p.vowel + 7) % 101,
            3 => p.ops.iter_mut().for_each(|o| o.frmt /= 2),
            4 => p.ops.iter_mut().for_each(|o| o.bw /= 2),
            5 => p.ops[3..].iter_mut().for_each(|o| o.coarse ^= 1),
            _ => {}
        };
        let (kept, _) = render_algo_with(&p, note, blocks, |b, p, live, e| {
            edit(b, p);
            jumps.apply(b, live, e);
        });
        let (fresh, _) = render_algo_with(&p, note, blocks, |b, p, live, e| {
            edit(b, p);
            jumps.apply(b, live, e);
            e.retake_laws();
        });
        assert!(
            kept.iter()
                .zip(&fresh)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "seed {seed}"
        );
    }
}
