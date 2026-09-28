//! The filter: typed modes (#111) and retired ids (#112).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::fast_exp2;
use chimera_core::dsp::filter::routed_cutoff;
use chimera_core::dsp::filter::{FilterMode, SVF_MODES, SvfFilter};
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::ui::block_def::slot_addr;
use chimera_core::ui::block_registry::{FILTER, FILTER_MODE};
use chimera_core::ui::view::SlotCtx;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

#[test]
fn mode_is_typed_and_stays_in_the_svf_list() {
    let mut f = FilterParams::default();
    assert_eq!(f.mode(), FilterMode::Lp24, "every Sound is on LP24 today");
    for m in FilterMode::ALL {
        assert!(f.set_mode(m));
        assert_eq!(f.mode(), m);
    }
    // The block value is the index in the SVF's list, so an encoder steps it.
    f.set(FilterParams::MODE, 0.0);
    assert_eq!(f.mode(), SVF_MODES[0]);
    assert_eq!(SVF_MODES[0], FilterMode::Lp24, "the default comes first");
    f.set(FilterParams::MODE, 99.0);
    assert_eq!(f.mode(), FilterMode::Phaser);
    assert_eq!(f.get(FilterParams::MODE), 7.0);
}

#[test]
fn the_discriminants_are_the_old_mode_byte() {
    for (i, m) in FilterMode::ALL.iter().enumerate() {
        assert_eq!(*m as u8, i as u8);
    }
}

#[test]
fn retired_filter_ids_have_no_spec() {
    let p = ParamSnapshot::default();
    for id in [3, 4, 5] {
        assert!(p.filter.spec(ParamId(id)).is_none(), "id {id} is retired");
    }
}

#[test]
fn mode_is_on_the_flt_pages() {
    let mode = ParamAddr::new(BlockRef::Filter, FilterParams::MODE);
    let ctx = SlotCtx::read(&ParamSnapshot::default(), chimera_core::addr::Op::A);
    let on = |def| (0..6).any(|i| slot_addr(def, i, &ctx) == Some(mode));
    assert!(on(&FILTER) && on(&FILTER_MODE));
}

/// Each mode filters a saw differently, and all stay finite.
#[test]
fn every_mode_renders_finite_and_distinct() {
    let mut seen = Vec::new();
    for m in FilterMode::ALL {
        let mut p = FilterParams::default();
        p.set_mode(m);
        p.resonance = 0.4;
        let mut f = SvfFilter::new();
        let mut out = Vec::new();
        for b in 0..20 {
            let mut buf: Vec<f32> = (0..BLOCK_SIZE)
                .map(|i| ((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0)
                .collect();
            f.process(&mut buf, &p, SR);
            out.extend(buf);
        }
        assert!(out.iter().all(|x| x.is_finite()), "{m:?}");
        let bits: Vec<u32> = out.iter().map(|x| x.to_bits()).collect();
        assert!(!seen.contains(&bits), "{m:?} renders like another mode");
        seen.push(bits);
    }
}

#[test]
fn fast_exp2_is_exact_at_integers_and_within_a_tenth_of_a_cent() {
    for i in -12..=12 {
        assert_eq!(fast_exp2(i as f32), 2f32.powi(i), "2^{i}");
    }
    let mut k = -12_000i32;
    while k <= 12_000 {
        let x = k as f32 / 1000.0;
        let cents = 1200.0 * (fast_exp2(x) as f64 / 2f64.powf(x as f64)).log2();
        assert!(cents.abs() < 0.1, "2^{x}: {cents} cents");
        k += 1;
    }
}

#[test]
fn routed_cutoff_is_in_octaves() {
    let base = 1000.0f32;
    assert_eq!(
        routed_cutoff(base, 0.0).to_bits(),
        base.to_bits(),
        "Σ 0 is bit for bit"
    );
    // NOTE at 100 %: (note − 60) / 120 × 127/127 → one octave per octave.
    for (note, want) in [(48.0f32, 0.5f32), (60.0, 1.0), (72.0, 2.0), (84.0, 4.0)] {
        let fc = routed_cutoff(base, (note - 60.0) / 120.0);
        assert!((fc / (base * want) - 1.0).abs() < 1e-6, "note {note}: {fc}");
    }
    // ENV at 100 % with the envelope at 1: +10 octaves before the clamp.
    assert!((routed_cutoff(15.0, 1.0) / (15.0 * 1024.0) - 1.0).abs() < 1e-6);
    assert_eq!(routed_cutoff(1000.0, 1.0), 20_000.0);
    assert_eq!(routed_cutoff(1000.0, -1.0), 20.0);
}

/// Review Focus 1: sums far past ±1 stay finite and inside the range.
#[test]
fn routed_cutoff_survives_huge_sums() {
    for sum in [3.0f32, -3.0, 8.0, -8.0, 1e6, -1e6] {
        let fc = routed_cutoff(1000.0, sum);
        assert!(
            fc.is_finite() && (20.0..=20_000.0).contains(&fc),
            "{sum}: {fc}"
        );
    }
}

/// Spec § Tests "Clicks": an ENV sweep on CUTOFF (attack 1 ms and 50 ms,
/// route ±100 %, RES 0.5) has a largest second difference at most 1.5 ×
/// that of the same render with each block's cutoff held at the block's
/// mean. And #53 itself: with the fast attack, the ramp removes at least
/// half the largest second difference of the same targets stepped per
/// block (a filter without the ramp fails this; the spec's own ratio would
/// not tell the two apart).
#[test]
fn a_cutoff_sweep_does_not_click() {
    #[derive(Clone, Copy, PartialEq)]
    enum How {
        Ramped,
        HeldAtMean,
        Stepped,
    }
    for attack in [0.001f32, 0.05] {
        for amount in [1.0f32, -1.0] {
            let env = |b: usize| ((b * BLOCK_SIZE) as f32 / (attack * SR as f32)).min(1.0);
            let fc = |b: usize| routed_cutoff(1000.0, amount * env(b));
            let render = |how: How| {
                let mut f = SvfFilter::new();
                let mut p = FilterParams::default();
                p.resonance = 0.5;
                let mut out = Vec::new();
                for b in 0..40usize {
                    p.cutoff = if how == How::HeldAtMean {
                        0.5 * (fc(b.saturating_sub(1)) + fc(b))
                    } else {
                        fc(b)
                    };
                    if how != How::Ramped {
                        f.hold();
                    }
                    let mut buf: Vec<f32> = (0..BLOCK_SIZE)
                        .map(|i| {
                            let n = (b * BLOCK_SIZE + i) as f32;
                            0.5 * (core::f32::consts::TAU * 220.0 * n / SR as f32).sin()
                        })
                        .collect();
                    f.process(&mut buf, &p, SR);
                    out.extend(buf);
                }
                out
            };
            let d2 = |s: &[f32]| {
                s.windows(3)
                    .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                    .fold(0.0f32, f32::max)
            };
            let ramped = d2(&render(How::Ramped));
            let held = d2(&render(How::HeldAtMean));
            assert!(
                ramped <= 1.5 * held,
                "attack {attack}, amount {amount}: {ramped} vs {held}"
            );
            // A 50 ms sweep moves a quarter octave a block: its steps hide
            // under the sine's own curvature, so only the fast one is checked.
            if attack < 0.01 {
                let stepped = d2(&render(How::Stepped));
                assert!(
                    ramped <= 0.5 * stepped,
                    "attack {attack}, amount {amount}: {ramped} vs stepped {stepped}"
                );
            }
        }
    }
}

/// Within a block, `g` moves by the same step every sample and lands on
/// the new value on the last one.
#[test]
fn g_ramps_evenly_to_the_new_value() {
    use chimera_core::dsp::filter::g_at;
    let (from, to) = (0.1f32, 0.9f32);
    let step = (to - from) / BLOCK_SIZE as f32;
    let g: Vec<f32> = (0..BLOCK_SIZE).map(|i| g_at(from, step, i)).collect();
    assert!((g[BLOCK_SIZE - 1] - to).abs() < 1e-6);
    let mut prev = from;
    for (i, &x) in g.iter().enumerate() {
        assert!((x - prev - step).abs() < 1e-6, "sample {i}");
        prev = x;
    }
}

/// After a cutoff change, the next steady blocks don't ramp again: the
/// ramp ended on the new `g` and kept it, so dropping any ramp (`hold`)
/// changes nothing. Fails if the ramp's end isn't stored.
#[test]
fn a_steady_cutoff_does_not_ramp() {
    // MODE is private: no struct-update syntax from a test.
    let (mut lo, mut hi) = (FilterParams::default(), FilterParams::default());
    (lo.cutoff, hi.cutoff) = (500.0, 2000.0);
    let saw = |b: usize| -> Vec<f32> {
        (0..BLOCK_SIZE)
            .map(|i| ((b * BLOCK_SIZE + i) % 97) as f32 / 48.5 - 1.0)
            .collect()
    };
    let mut a = SvfFilter::new();
    for (blk, p) in [&lo, &hi].into_iter().enumerate() {
        a.process(&mut saw(blk), p, SR); // block 1 ramps 500 → 2000 Hz
    }
    let mut b = a.clone();
    for blk in 2..10 {
        let (mut x, mut y) = (saw(blk), saw(blk));
        a.process(&mut x, &hi, SR);
        b.hold();
        b.process(&mut y, &hi, SR);
        assert_eq!(x, y, "block {blk}");
    }
}

/// Every mode's output, steady and ramped, hot enough to saturate, bit for
/// bit as recorded before `tick` was inlined into per-mode loops.
#[test]
fn every_mode_renders_as_recorded() {
    const WANT: [u64; 8] = [
        0x31ba_0de6_59aa_bd16,
        0x8881_22f4_1133_37c5,
        0xefd7_edba_5eab_f5ec,
        0x0ebe_833a_a4b4_21d0,
        0x9531_72d7_34b8_27cc,
        0xe9e1_2b33_8060_4879,
        0xeaac_ffc4_00fb_ef23,
        0x2a19_e4cc_f46f_f16a,
    ];
    let got = FilterMode::ALL.map(|m| {
        let mut p = FilterParams::default();
        p.set_mode(m);
        (p.resonance, p.drive) = (0.9, 0.5);
        let mut f = SvfFilter::new();
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in 0..24 {
            // Steady blocks, then a cutoff change every other block (ramps).
            p.cutoff = if b < 8 || b % 2 == 0 { 800.0 } else { 3000.0 };
            let mut buf: Vec<f32> = (0..BLOCK_SIZE)
                .map(|i| ((b * BLOCK_SIZE + i) % 150) as f32 / 75.0 - 1.0)
                .collect();
            f.process(&mut buf, &p, SR);
            for x in buf {
                h = (h ^ u64::from(x.to_bits())).wrapping_mul(0x100_0000_01b3);
            }
        }
        h
    });
    assert_eq!(got, WANT, "{got:#x?}");
}
