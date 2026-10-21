//! TAPE, bit for bit (ADR 0199): a saved project loads with TYPE TAPE and
//! its delay plays exactly what it played before TYPE existed. The hashes
//! were taken on origin/main f745c0a1, before GRAIN.

use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_hal::BLOCK_SIZE;

/// Plucks every 0.3 s and a quiet noise bed, block `b`.
fn stimulus(b: usize, rng: &mut u32) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| {
        let n = b * BLOCK_SIZE + i;
        let t = (n % 14_400) as f32;
        let pluck = 0.8 * (t * 0.031).sin() * (-t / 2400.0).exp();
        *rng ^= *rng << 13;
        *rng ^= *rng >> 17;
        *rng ^= *rng << 5;
        let noise = (*rng as f32 / u32::MAX as f32 - 0.5) * 0.05;
        pluck + noise
    })
}

/// The settings at block `b`: TIME, FDBK, MECHANICS, SAT, TONE and MIX each
/// move mid-take, TIME twice (its crossfade).
fn params(b: usize) -> DelayParams {
    let mut p = DelayParams {
        time_ms: 375.0,
        feedback: 0.7,
        wow_flutter: 0.3,
        saturation: 0.4,
        tone: 0.5,
        mix: 0.8,
        ..DelayParams::DEFAULT
    };
    if b >= 400 {
        p.time_ms = 120.0;
    }
    if b >= 700 {
        p.feedback = 0.95;
        p.wow_flutter = 0.9;
    }
    if b >= 1000 {
        p.time_ms = 499.0;
        p.saturation = 0.0;
        p.tone = 1.0;
    }
    if b >= 1300 {
        p.mix = 0.3;
    }
    p
}

fn fnv(h: &mut u64, x: f32) {
    for byte in x.to_bits().to_le_bytes() {
        *h ^= u64::from(byte);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

fn take(wet: bool) -> u64 {
    let mut d = Box::new(TapeDelay::new());
    let (mut rng, mut h) = (0x1234_5678u32, 0xcbf2_9ce4_8422_2325u64);
    for b in 0..1600 {
        let mut buf = stimulus(b, &mut rng);
        if wet {
            d.process_wet(&mut buf, &params(b), 48_000);
        } else {
            d.process(&mut buf, &params(b), 48_000);
        }
        buf.iter().for_each(|&s| fnv(&mut h, s));
    }
    h
}

#[test]
fn tape_plays_bit_for_bit_as_before_type() {
    assert_eq!(take(true), 0x9af3_b7ef_7be8_fcc5, "send");
    assert_eq!(take(false), 0x832e_040d_2a77_8985, "insert");
}
