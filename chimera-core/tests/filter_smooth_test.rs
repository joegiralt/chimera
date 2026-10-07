//! The filter is smooth, never steppy (owner, 2026-10-07; ADR 0107).
//!
//! Turns go through the real path: the panel's acceleration
//! (`chimera_hal::encoder_accel`), then `Block::nudge` or the matrix
//! amount's `+ delta`, a publish, and the Instrument. What is measured is
//! the cutoff each block hands the filter, routes applied, on every TYPE.
//! The detent timings are the root-cause report's
//! (`.superpowers/ledgers/filter-steppy-rootcause.md`).

mod common;

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::modulator::EnvSpeed;
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::ui::filter_panel::keybd_step;
use chimera_hal::{BLOCK_SIZE, ENCODER_BURST_GAP_MS, SAMPLE_RATE, encoder_accel};
use common::sweep::{Bench, Patch, Tape};

const MS_PER_BLOCK: f32 = BLOCK_SIZE as f32 * 1000.0 / SAMPLE_RATE as f32;

const TYPES: [FilterKind; 3] = [FilterKind::Svf, FilterKind::K2040, FilterKind::Dirty];

/// A slow turn's bound, cents a millisecond. One detent alone (1/24 oct,
/// 50 c) must spread so that no block (1.33 ms) moves more than about 4 c,
/// under the ~5 c a pitch must move to be heard to move: a slow turn has no
/// step anywhere.
const SLOW_C_PER_MS: f32 = 3.0;

/// A fast turn's bound, cents a millisecond. At a detent every 30 ms the
/// hand asks for 10 ticks (500 c) a detent, 17 c/ms; the glide may run at
/// most 2× the hand's own rate. A step is a whole detent in one block, ~23×
/// it: e940fd8c moved 998 c in one block (750 c/ms) here.
const FAST_C_PER_MS: f32 = 2.0 * 500.0 / FAST_DETENT_MS;

/// The fast turn's detent period, ms.
const FAST_DETENT_MS: f32 = 30.0;

/// Ticks landing in each of `n` blocks from detents at `(ms, dir)`,
/// accelerated as the panel does.
fn ticks(detents: &[(f32, i8)], n: usize) -> Vec<i8> {
    let mut out = vec![0i8; n];
    let (mut burst, mut last) = (0u8, f32::NEG_INFINITY);
    for &(t, dir) in detents {
        if t - last > ENCODER_BURST_GAP_MS as f32 {
            burst = 0;
        }
        last = t;
        let b = (t / MS_PER_BLOCK) as usize;
        out[b] = out[b].saturating_add(dir * encoder_accel(burst));
        burst = burst.saturating_add(1);
    }
    out
}

/// The report's fast turn: 12 detents up, one every 30 ms; then 12 down,
/// one every 200 ms (no acceleration). 3.4 s.
fn fast_turn() -> Vec<i8> {
    let mut d: Vec<(f32, i8)> = (0..12)
        .map(|i| (100.0 + FAST_DETENT_MS * i as f32, 1))
        .collect();
    d.extend((0..12).map(|i| (800.0 + 200.0 * i as f32, -1)));
    ticks(&d, (3400.0 / MS_PER_BLOCK) as usize)
}

/// The report's slow turn: 8 detents up, one every 200 ms. 2 s.
fn slow_turn() -> Vec<i8> {
    let d: Vec<(f32, i8)> = (0..8).map(|i| (100.0 + 200.0 * i as f32, 1)).collect();
    ticks(&d, (2000.0 / MS_PER_BLOCK) as usize)
}

/// A held saw through `kind` at `cutoff` Hz and RES `res`, ENV 1 at full
/// sustain.
fn saw(kind: FilterKind, cutoff: f32, res: f32) -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = AlgoParams::single(WaveId::SAW);
    s.params.filter.set_kind(kind);
    (s.params.filter.cutoff, s.params.filter.resonance) = (cutoff, res);
    s.params.envelopes[0].sustain = 1.0;
    s
}

/// `note` held over `ticks.len()` blocks, `edit(x, ticks[b])` applied to
/// the Sound (and published) before block `b` where `ticks[b]` isn't 0:
/// the filter settings each block played.
fn play(s: Sound, note: u8, ticks: &[i8], edit: impl Fn(&mut Sound, i8)) -> Vec<FilterParams> {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
    b.edit_sound(|x| *x = s);
    b.note(note, Some(100));
    let mut t = Tape::default();
    ticks
        .iter()
        .map(|&d| {
            if d != 0 {
                b.edit_sound(|x| edit(x, d));
            }
            b.render(&mut t);
            b.played_filter()
        })
        .collect()
}

/// `id` of the Filter block nudged by each block's ticks.
fn turn(s: Sound, id: ParamId, ticks: &[i8]) -> Vec<FilterParams> {
    play(s, 57, ticks, |x, d| x.params.filter.nudge(id, d))
}

/// The largest block-to-block cutoff move, cents a millisecond.
fn max_c_per_ms(p: &[FilterParams]) -> f32 {
    p.windows(2)
        .map(|w| (1200.0 * (w[1].cutoff / w[0].cutoff).log2()).abs() / MS_PER_BLOCK)
        .fold(0.0, f32::max)
}

#[test]
fn a_fast_resonant_cutoff_turn_glides_on_every_type() {
    let worst = TYPES.map(|kind| {
        let p = turn(
            saw(kind, 332.0, 51.0 / 127.0),
            FilterParams::CUTOFF,
            &fast_turn(),
        );
        let (lo, hi) = p.iter().fold((f32::MAX, 0.0f32), |(l, h), q| {
            (l.min(q.cutoff), h.max(q.cutoff))
        });
        let worst = max_c_per_ms(&p);
        eprintln!(
            "{kind:?} fast: worst {worst:.1} c/ms ({:.1} c a block), {lo:.0}–{hi:.0} Hz",
            worst * MS_PER_BLOCK
        );
        assert!(hi > 4.0 * lo, "{kind:?}: the turn moved {lo}–{hi} Hz");
        worst
    });
    assert!(
        worst.iter().all(|&w| w <= FAST_C_PER_MS),
        "{worst:?} c/ms > {FAST_C_PER_MS}"
    );
}

#[test]
fn a_slow_resonant_cutoff_turn_never_steps_on_every_type() {
    let worst = TYPES.map(|kind| {
        let p = turn(
            saw(kind, 176.0, 51.0 / 127.0),
            FilterParams::CUTOFF,
            &slow_turn(),
        );
        let worst = max_c_per_ms(&p);
        eprintln!(
            "{kind:?} slow: worst {worst:.2} c/ms ({:.2} c a block)",
            worst * MS_PER_BLOCK
        );
        assert!(
            p.last().unwrap().cutoff > 1.2 * 176.0,
            "{kind:?}: the turn moved nothing"
        );
        worst
    });
    assert!(
        worst.iter().all(|&w| w <= SLOW_C_PER_MS),
        "{worst:?} c/ms > {SLOW_C_PER_MS}"
    );
}

/// RES's k is `res·4/0.95` on the 2040: a fast detent (10 ticks, 10/128)
/// is 0.33 of k, all in one block. The bound is the cutoff's: 2× the hand's
/// rate.
#[test]
fn a_fast_res_turn_glides_on_every_type() {
    let worst = TYPES.map(|kind| {
        let p = turn(
            saw(kind, 1000.0, 0.2),
            FilterParams::RESONANCE,
            &fast_turn(),
        );
        let worst = p
            .windows(2)
            .map(|w| (w[1].resonance - w[0].resonance).abs() / MS_PER_BLOCK)
            .fold(0.0, f32::max);
        eprintln!("{kind:?} RES fast: worst {worst:.4}/ms");
        worst
    });
    let bound = 2.0 * 10.0 / 128.0 / FAST_DETENT_MS;
    assert!(
        worst.iter().all(|&w| w <= bound),
        "RES {worst:?}/ms > {bound}"
    );
}

/// The matrix amount's `+ delta` (the UI's route edit), clamped.
fn turn_route(
    s: Sound,
    src: ModSource,
    note: u8,
    ticks: &[i8],
    step: fn(i8, i8) -> i8,
) -> Vec<FilterParams> {
    play(s, note, ticks, |x, d| {
        let a = x.mod_state.fixed_amount(src.index(), FixedCol::Freq);
        x.mod_state
            .set_fixed(src.index(), FixedCol::Freq, step(a, d));
    })
}

/// ENV AMT at full sustain: a unit is 10/127 oct (94 c), so the fast turn
/// asks for 945 c a detent, 31.5 c/ms. The glide runs at most 2× that.
#[test]
fn an_env_amt_turn_glides() {
    let add = |a: i8, d: i8| (a as i16 + d as i16).clamp(-127, 127) as i8;
    let worst = TYPES.map(|kind| {
        let p = turn_route(
            saw(kind, 200.0, 51.0 / 127.0),
            ModSource::Env1,
            57,
            &fast_turn(),
            add,
        );
        let worst = max_c_per_ms(&p);
        eprintln!("{kind:?} ENV AMT fast: worst {worst:.1} c/ms");
        worst
    });
    let bound = 2.0 * 10.0 * 1200.0 * 10.0 / 127.0 / FAST_DETENT_MS;
    assert!(
        worst.iter().all(|&w| w <= bound),
        "{worst:?} c/ms > {bound}"
    );
}

/// KEYBD OFF → HALF → FULL on note 96 (+3 oct at FULL): one detent was
/// 1.5 oct (1,800 c) in one block. Eased, a detent's first block moves
/// 1,800 c × 1.33 ms / 30 ms: the bound, with 10 % for the second.
#[test]
fn a_keybd_detent_glides() {
    let d: Vec<(f32, i8)> = vec![(100.0, 1), (400.0, 1)];
    let t = ticks(&d, (800.0 / MS_PER_BLOCK) as usize);
    let step = |a: i8, d: i8| keybd_step(a, d.signum());
    let p = turn_route(
        saw(FilterKind::K2040, 200.0, 51.0 / 127.0),
        ModSource::Note,
        96,
        &t,
        step,
    );
    let moved = 1200.0 * (p.last().unwrap().cutoff / p[0].cutoff).log2();
    let worst = max_c_per_ms(&p);
    eprintln!("KEYBD: moved {moved:.0} c, worst {worst:.1} c/ms");
    assert!(moved > 3000.0, "{moved}");
    assert!(worst <= 1800.0 / 30.0 * 1.1, "{worst:.1} c/ms");
}

/// ENV 1 → FREQ with the knobs still: what the filter gets is the
/// envelope's own trajectory, block for block, as before the knobs were
/// eased (values from e940fd8c). The snap of a pluck is the envelope's.
#[test]
fn env_to_freq_keeps_its_snap() {
    let mut s = saw(FilterKind::K2040, 150.0, 51.0 / 127.0);
    s.mod_state
        .set_fixed(ModSource::Env1.index(), FixedCol::Freq, 57);
    let e = &mut s.params.envelopes[0];
    (e.sustain, e.decay, e.speed, e.attack) = (0.0, 0.3, EnvSpeed::Fast, 0.0);
    let p = play(s, 57, &[0; 24], |_, _| {});
    let got: Vec<f32> = p.iter().map(|q| q.cutoff).collect();
    eprintln!("ENV→FREQ: {got:?}");
    for (b, (g, w)) in got.iter().zip(ENV_SNAP).enumerate() {
        assert!(
            (1200.0 * (g / w).log2()).abs() < 0.1,
            "block {b}: {g} Hz, was {w} Hz"
        );
    }
}

/// `env_to_freq_keeps_its_snap`'s cutoffs on e940fd8c, before the easing.
const ENV_SNAP: [f32; 24] = [
    150.0, 681.53015, 283.00513, 193.75587, 164.56033, 153.37296, 150.0, 150.0, 150.0, 150.0,
    150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0, 150.0,
    150.0,
];

/// CUTOFF's law (ADR 0107): 1/24 octave a tick, the same in cents at any
/// cutoff; a fast turn (the acceleration's 10 ticks a detent) still
/// crosses 20 Hz – 20 kHz in 27 detents.
#[test]
fn cutoff_ticks_a_24th_of_an_octave_and_a_fast_turn_crosses_the_range() {
    let mut f = FilterParams::default();
    for hz in [40.0, 1000.0, 15_000.0] {
        f.cutoff = hz;
        f.nudge(FilterParams::CUTOFF, 1);
        assert!(
            (1200.0 * (f.cutoff / hz).log2() - 50.0).abs() < 0.1,
            "{hz}: {}",
            f.cutoff
        );
    }
    f.cutoff = 20.0;
    let detents = (0u8..)
        .take_while(|&n| {
            f.nudge(FilterParams::CUTOFF, encoder_accel(n));
            f.cutoff < 20_000.0
        })
        .count()
        + 1;
    assert_eq!(detents, 27);
    assert_eq!(f.cutoff, 20_000.0);
}
