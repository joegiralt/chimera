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
use chimera_hal::encoder_accel;
use common::sweep::{Bench, Patch, Tape};
use common::turn::{FAST_DETENT_MS, MS_PER_BLOCK, fast_turn, slow_turn, ticks};

/// Every TYPE TYPE steps: a new one (LADDER) is covered as it is built.
const TYPES: [FilterKind; FilterKind::BUILT.len()] = FilterKind::BUILT;

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

/// A route amount unit into FREQ at a full source: 10/127 octave, cents.
const ENV_UNIT_C: f32 = 1200.0 * 10.0 / 127.0;

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
    // 10 units of 94.5 c a detent: 2× the hand is 63 c/ms, the CUTOFF rule
    // (measured 49, the same 1.3× headroom as CUTOFF's 25.9 under 33.3).
    let bound = 2.0 * 10.0 * ENV_UNIT_C / FAST_DETENT_MS;
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

// ── Loads, notes and Parts (ADR 0107 § 3–4) ────────────────────────────

use common::part_rig::Rig;

/// Parts 1 and 2 a held-saw Sound on `kind` at `hz`, RES 51.
fn rig(kind: FilterKind, hz: [f32; 2]) -> Rig {
    Rig::new(hz.map(|hz| saw(kind, hz, 51.0 / 127.0)))
}

/// A Sound loaded over a sounding voice lands on its knobs, the next
/// block: no 90 ms sweep from the old Sound's (the voice still rings).
#[test]
fn a_load_under_a_sounding_voice_lands() {
    for kind in TYPES {
        let mut r = rig(kind, [8000.0, 1000.0]);
        r.note(0, true);
        for _ in 0..20 {
            r.block();
        }
        r.note(0, false);
        r.load(0);
        let want =
            r.p.part(chimera_core::project::PartId::ALL[0])
                .sound
                .params
                .filter
                .cutoff;
        assert_ne!(want, 8000.0);
        let got = r.block();
        assert_eq!(got.len(), 1, "{kind:?}: the released voice still rings");
        assert_eq!(got[0], want, "{kind:?}");
    }
}

/// A turn made while a Part is silent is where its next note starts.
#[test]
fn a_turn_on_a_silent_part_lands_on_the_next_note() {
    let mut r = rig(FilterKind::K2040, [8000.0, 1000.0]);
    r.block();
    r.sound(0).params.filter.cutoff = 300.0;
    r.block();
    r.note(0, true);
    assert_eq!(r.block(), [300.0]);
}

/// A note mid-glide joins the Part's glide where it is, and goes on with
/// it: the two voices play one cutoff, still moving.
#[test]
fn a_note_mid_glide_joins_the_glide() {
    let mut r = rig(FilterKind::K2040, [300.0, 1000.0]);
    r.note(0, true);
    // The Sound read in (nine blocks at the read budget, ADR 0115): a knob
    // not yet read has no old value to glide from, and lands.
    for _ in 0..9 {
        r.block();
    }
    r.sound(0).params.filter.cutoff = 3000.0;
    let first = r.block()[0];
    let was = r.block()[0];
    r.note(0, false);
    // A second key on the same Part: a new voice.
    r.key(0, 64, true);
    let both = r.block();
    assert_eq!(both.len(), 2);
    assert_eq!(both[0], both[1], "one Part, one cutoff");
    assert!(
        first > 300.0 && was > first && both[0] > was && both[0] < 3000.0,
        "{first} {was} {both:?}"
    );
    let step = 1200.0 * (both[0] / was).log2();
    assert!(
        step < 2.0 * 1200.0 * 10.0f32.log2() * MS_PER_BLOCK / 30.0,
        "{step} c"
    );
}

/// Two Parts glide apart: turning Part 1 never moves Part 2.
#[test]
fn two_parts_glide_independently() {
    let mut r = rig(FilterKind::Svf, [300.0, 1000.0]);
    r.note(0, true);
    r.note(1, true);
    r.block();
    r.sound(0).params.filter.cutoff = 3000.0;
    for _ in 0..10 {
        let c = r.block();
        assert_eq!(c.len(), 2);
        assert!(c.contains(&1000.0), "{c:?}");
        assert!(c.iter().any(|&x| x > 300.0 && x < 3000.0), "{c:?}");
    }
}

/// A NaN RES (a raw write; the encoder clamps) is played one block, then
/// the next good value lands: it never sticks in the Part's glide.
#[test]
fn a_nan_res_recovers() {
    let s = saw(FilterKind::K2040, 1000.0, 0.4);
    let res = [f32::NAN, 0.5, 0.5];
    let b = std::cell::Cell::new(0);
    let p = play(s, 57, &[1; 3], |x, _| {
        x.params.filter.write(FilterParams::RESONANCE, res[b.get()]);
        b.set(b.get() + 1);
    });
    let got: Vec<f32> = p.iter().map(|q| q.resonance).collect();
    assert!(got[0].is_nan(), "{got:?}");
    assert_eq!(&got[1..], [0.5, 0.5], "{got:?}");
}
