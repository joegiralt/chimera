//! QUAD's SMEAR (reverb TYPEs spec § SMEAR, § Pitch bounds) and GRIT's
//! retirement (§ GRIT is retired).

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{REVERB_SPECS, ReverbParams};
use chimera_core::dsp::ring::{
    BASE, SIZE_TABLE, STAGES, size_step, smear_depth, smear_probe, stage_len,
};
use chimera_core::storage::RETIRED;
use chimera_hal::BLOCK_SIZE;
use common::rev::{Noise, blocks, check_smear_bound, max_d2, run, tail_pitch_hz, tone};
use common::{SR, fnv1a};

const FS_RING: f32 = 24_000.0;

fn tick(
    rv: &mut Reverb,
    p: &ReverbParams,
    send: &[f32; BLOCK_SIZE],
) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    let mut out = Stereo::SILENT;
    rv.process(send, p, p.mix, Tempo::DEFAULT, SR, &mut out);
    (out.l, out.r)
}

/// QUAD's round trip at `size`, in seconds.
fn trip_s(size: f32) -> f32 {
    (0..STAGES)
        .map(|k| stage_len(size_step(size), k) as f32)
        .sum::<f32>()
        / FS_RING
}

// ── SMEAR 0 is QUAD as it was ──

/// `fx_golden_test`'s reverb render (a 110 Hz saw burst, then silence).
fn golden_render(size: f32) -> Vec<f32> {
    let p = ReverbParams {
        time: 0.7,
        damping: 0.3,
        size,
        mix: 0.5,
        ..ReverbParams::default()
    };
    let mut rv = Box::new(Reverb::new());
    let mut out = Vec::new();
    let mut phase = 0.0f32;
    for b in 0..450 {
        let mut block = [0.0f32; BLOCK_SIZE];
        if b < 50 {
            for s in block.iter_mut() {
                *s = phase - 0.5;
                phase = (phase + 110.0 / SR as f32).fract();
            }
        }
        let (l, r) = tick(&mut rv, &p, &block);
        out.extend_from_slice(&l);
        out.extend_from_slice(&r);
    }
    out
}

/// Recorded at the T4 golden reference (a2abe82f), at GRIT 0.
/// `reverb_dispatch_test` holds its sweeps at SMEAR 0 to theirs.
const T4_GRIT_0: &[(&str, u64)] = &[
    ("reverb_ring", 0xd17b7769e13816bf),
    ("reverb_ring_max_size", 0x83591bf24cab49f6),
];

#[test]
fn quad_at_smear_0_is_the_t4_reference() {
    let got = [
        ("reverb_ring", fnv1a(&golden_render(0.5))),
        ("reverb_ring_max_size", fnv1a(&golden_render(1.0))),
    ];
    common::golden::check(T4_GRIT_0, &got);
}

// ── GRIT is retired ──

/// A lone reverb block as `Blocks`.
struct One<'a>(&'a mut ReverbParams);

impl BlockRead for One<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        (b == BlockRef::Reverb).then_some(&*self.0 as &dyn Block)
    }
}

impl Blocks for One<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        (b == BlockRef::Reverb).then_some(&mut *self.0 as &mut dyn Block)
    }
}

#[test]
fn grit_is_retired() {
    assert!(REVERB_SPECS.iter().all(|s| s.id != ParamId(5)));
    assert!(RETIRED.contains(&(22, 5)));
    // A v1 reverb block: GRIT 0.8 beside the live ids at their defaults.
    let d = ReverbParams::default();
    let mut v1 = vec![22u8];
    for (id, v) in [
        (5u8, 0.8f32),
        (1, d.time),
        (2, d.damping),
        (3, d.size),
        (4, d.mix),
    ] {
        v1.push(id);
        v1.extend_from_slice(&v.to_le_bytes());
    }
    let mut got = ReverbParams::default();
    chimera_core::storage::decode_block(&v1, &[], &[], Some(&mut One(&mut got))).unwrap();
    assert_eq!(got, d);
}

// ── the reads and the cap ──

/// The engine's own reads (`ring::smear_probe`), at SMEAR 1 and RATE 1 for
/// 3 s at SIZE 0 and 1: every stage delay reads within [d − 2D, d], never
/// past its line or under 2 back, and the four stages' observed travel
/// sums to at most 1.45 % of the round trip.
#[test]
fn quad_smear_reads_only_shorten_and_respect_the_cap() {
    for size in [0.0, 1.0] {
        let step = size_step(size);
        let trip: f32 = (0..STAGES).map(|k| stage_len(step, k) as f32).sum();
        let big_d = smear_depth(1.0, step);
        let p = ReverbParams {
            size,
            smear: 1.0,
            rate: 1.0,
            mix: 1.0,
            ..ReverbParams::default()
        };
        let mut rv = Box::new(Reverb::new());
        let mut n = Noise(3);
        smear_probe::reset();
        let mut smeared = 0;
        for _ in 0..blocks(3.0) {
            let (l, r) = tick(&mut rv, &p, &core::array::from_fn(|_| n.next()));
            assert!(l.iter().chain(&r).all(|s| s.is_finite()));
            smeared += rv.debug_paths().1 as usize;
        }
        assert!(smeared > blocks(2.9), "SIZE {size}: {smeared} SMEAR blocks");
        let reads = smear_probe::reads();
        let mut travel = 0.0;
        for (k, &(shortest, longest)) in reads.iter().enumerate() {
            let j = 3 * k + 2;
            let d = SIZE_TABLE[step as usize][j] as f32;
            let what = format!("SIZE {size}, stage {k}: {shortest}..{longest} of {d}, D {big_d}");
            assert!(d <= BASE[j] as f32, "{what}");
            assert!(longest <= d && shortest >= d - 2.0 * big_d - 1e-3, "{what}");
            assert!(shortest >= 2.0, "{what}");
            assert!(
                longest - shortest > 0.5 * big_d,
                "{what}: the read hardly moved"
            );
            travel += longest - shortest;
        }
        assert!(
            travel <= 0.0145 * trip,
            "SIZE {size}: travel {travel} of {trip}"
        );
    }
}

// ── pitch ──

/// QUAD at SMEAR 1, RATE 1 (1 Hz), SIZE 0, TIME 1 (a long tail), MIX 1.
fn smeared_quad(frozen: bool) -> (Box<Reverb>, ReverbParams) {
    let p = ReverbParams {
        time: 1.0,
        damping: 0.0,
        size: 0.0,
        mix: 1.0,
        smear: 1.0,
        rate: 1.0,
        ..Default::default()
    };
    let mut rv = Box::new(Reverb::new());
    if frozen {
        rv.freeze_smear();
    }
    (rv, p)
}

#[test]
fn quad_smear_has_no_vibrato_and_no_drift() {
    let (mut still, p) = smeared_quad(true);
    let f_ref = tail_pitch_hz(&mut |s| tick(&mut still, &p, s));
    let (mut rv, p) = smeared_quad(false);
    check_smear_bound(&mut |s| tick(&mut rv, &p, s), trip_s(0.0), f_ref, 1.0);
}

/// QUAD's defaults but RATE 1 (1 Hz). At the default RATE (0.22 Hz) the
/// wander is slow enough that it spreads the −20 dB width by 1.5× only
/// (1.5 → 2.2 Hz; 1.7× at −30 dB): a slow wash, by design.
#[test]
fn smear_spreads_a_held_tone() {
    // A held 1 kHz tone, steady from 1.5 s, over 8 s (Welch, 2.7 s
    // frames): the band around 1 kHz within 20 dB of its peak. At SMEAR 0
    // the ring is linear and still, so it is a frame's own main lobe; the
    // wander spreads it.
    let width_hz = |smear: f32| {
        let p = ReverbParams {
            mix: 1.0,
            smear,
            rate: 1.0,
            ..ReverbParams::default()
        };
        let mut rv = Box::new(Reverb::new());
        let sine = tone(1_000.0, 0.25);
        let (l, _) = run(&mut |s| tick(&mut rv, &p, s), &mut |i| sine(i), 9.5);
        let n = 1 << 17;
        let pw = common::sweep::power(&l[3 * SR as usize / 2..], n);
        let hz = SR as f32 / n as f32;
        let k = (1_000.0 / hz) as usize;
        let band = (k - 200..k + 200)
            .max_by(|&a, &b| pw[a].total_cmp(&pw[b]))
            .unwrap();
        let floor = pw[band] * 0.01;
        let (mut lo, mut hi) = (band, band);
        while pw[lo - 1] >= floor {
            lo -= 1;
        }
        while pw[hi + 1] >= floor {
            hi += 1;
        }
        (hi - lo + 1) as f32 * hz
    };
    let (still, smeared) = (width_hz(0.0), width_hz(1.0));
    println!("−20 dB width: {still:.1} Hz at SMEAR 0, {smeared:.1} Hz at SMEAR 1");
    assert!(smeared >= 2.0 * still, "{smeared} Hz against {still} Hz");
}

// ── SIZE under SMEAR (Review Focus 4) ──

/// SMEAR 1, RATE 1: a 1 kHz tone at −12 dBFS for 0.5 s, then SIZE `from`
/// → `to` at 0.7 s (`from` = `to` holds it), the depth gated or, with
/// `hard`, snapped. The left return over the 0.5 s from the move, and each
/// block's paths.
fn size_move(from: f32, to: f32, hard: bool) -> (Vec<f32>, Vec<(bool, bool, f32)>) {
    let mut p = ReverbParams {
        size: from,
        smear: 1.0,
        rate: 1.0,
        mix: 1.0,
        ..ReverbParams::default()
    };
    let mut rv = Box::new(Reverb::new());
    if hard {
        rv.hard_smear_gate();
    }
    let sine = tone(1_000.0, 0.25);
    let (at, end) = (blocks(0.7), blocks(1.2));
    let (mut out, mut paths) = (Vec::new(), Vec::new());
    for b in 0..end {
        if b == at {
            p.size = to;
        }
        let send = core::array::from_fn(|i| {
            if b < blocks(0.5) {
                sine(b * BLOCK_SIZE + i)
            } else {
                0.0
            }
        });
        let (l, _) = tick(&mut rv, &p, &send);
        if b >= at - 1 {
            out.extend_from_slice(&l);
        }
        let (xf, sm) = rv.debug_paths();
        paths.push((xf, sm, rv.debug_depth()));
    }
    (out, paths)
}

/// Review Focus 4. The depth eases to 0 before the crossfade and back
/// after it, never together; the tone's tail through the move is no
/// rougher than held at either SIZE.
///
/// The judge is the largest second difference against those genuinely
/// held tails (SIZE 0.2 and 0.8 throughout), within 1.25×. The absolute
/// `click_free_test` detector cannot judge this: a hard-switched depth
/// moves its largest step only 0.022 → 0.027 at −12 dBFS (0.087 → 0.104 at
/// 0 dBFS), under its 0.15, while it doubles the second difference. The
/// hard switch is the negative control.
#[test]
fn quad_size_move_under_smear_does_not_click() {
    let (moved, paths) = size_move(0.2, 0.8, false);
    assert!(paths.iter().all(|p| !(p.0 && p.1)), "XF and SMEAR together");
    let first_xf = paths.iter().position(|p| p.0).expect("a crossfade");
    assert!(first_xf > blocks(0.7), "the crossfade waits for the depth");
    assert!(
        paths[blocks(0.7)..first_xf].iter().any(|p| p.1),
        "SMEAR ran on into the move"
    );
    assert_eq!(
        paths[first_xf - 1].2,
        0.0,
        "the depth is 0 before the crossfade"
    );
    assert!(paths.last().unwrap().1, "SMEAR comes back after it");
    let held = max_d2(&size_move(0.2, 0.2, false).0).max(max_d2(&size_move(0.8, 0.8, false).0));
    let (got, hard) = (max_d2(&moved), max_d2(&size_move(0.2, 0.8, true).0));
    println!("d2: moved {got}, hard {hard}, held {held}");
    assert!(got <= 1.25 * held, "d2 {got} against held {held}");
    assert!(
        hard > 1.25 * held,
        "the snapped depth passes: d2 {hard}, held {held}"
    );
}

// ── silence ──

#[test]
fn quad_falls_silent_with_smear() {
    let p = ReverbParams {
        time: 1.0,
        damping: 0.0,
        size: 1.0,
        mix: 1.0,
        smear: 1.0,
        rate: 1.0,
        ..Default::default()
    };
    let mut rv = Box::new(Reverb::new());
    let mut n = Noise(0xdead_beef);
    for _ in 0..blocks(1.0) {
        tick(&mut rv, &p, &core::array::from_fn(|_| n.next()));
    }
    let silence = [0.0; BLOCK_SIZE];
    let mut b = 0;
    while !rv.quad().unwrap().ring().is_silent(rv.arena()) {
        tick(&mut rv, &p, &silence);
        b += 1;
        assert!(b <= blocks(25.0), "still ringing after 25 s");
    }
    for _ in 0..blocks(0.1) {
        let (l, r) = tick(&mut rv, &p, &silence);
        assert!(l.iter().chain(&r).all(|&s| s == 0.0));
    }
}
