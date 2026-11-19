//! SMART EQ's host cost (ADR 0296 § Measurements): `cargo test --release
//! -p chimera-engine --test smart_eq_cost -- --ignored --nocapture`.
//! Six Parts written: SMART EQ cutting (every lower band moving), only
//! listening (one role), and, for the design's comparison, a separate
//! 8-band TPT band-pass bank listening to the same six buses.

use std::hint::black_box;
use std::time::Instant;

use chimera_engine::hw::MAX_PARTS;
use chimera_engine::instrument::Sides;
use chimera_engine::part_eq::PartEqs;
use chimera_engine::part_eq::dsp::smart::{SmartBlock, run};
use chimera_engine::part_eq::smart::{Amount, Role, SmartEq, Switch};
use chimera_hal::BLOCK_SIZE;

const FS: u32 = 48_000;
const BLOCKS: usize = 200_000;

fn buses(k: usize) -> [[f32; BLOCK_SIZE]; MAX_PARTS] {
    core::array::from_fn(|p| {
        core::array::from_fn(|i| {
            let n = (k * BLOCK_SIZE + i) as f32;
            0.3 * (n * 0.013 * (p + 1) as f32).sin() + 0.1 * (n * 0.41).sin()
        })
    })
}

fn time(roles: [Role; MAX_PARTS]) -> f64 {
    let mut st = PartEqs::REST.smart;
    let smart = SmartEq::new(Switch::On, Amount::from_code(127).unwrap());
    let input: Vec<_> = (0..64).map(buses).collect();
    // Settle the cuts first: the steady state is what plays.
    let mut b = input[0];
    for k in 0..20_000 {
        b = input[k % 64];
        let mut sides: Sides = [None; MAX_PARTS];
        run(
            &mut st,
            SmartBlock {
                buses: &mut b,
                sides: &mut sides,
                written: &[true; MAX_PARTS],
                roles: &roles,
                smart,
                sample_rate: FS,
            },
        );
    }
    let t = Instant::now();
    for k in 0..BLOCKS {
        b = input[k % 64];
        let mut sides: Sides = [None; MAX_PARTS];
        run(
            &mut st,
            SmartBlock {
                buses: black_box(&mut b),
                sides: &mut sides,
                written: &[true; MAX_PARTS],
                roles: &roles,
                smart,
                sample_rate: FS,
            },
        );
    }
    black_box(&b);
    t.elapsed().as_nanos() as f64 / BLOCKS as f64
}

/// A plain 8-band TPT band-pass bank's listening alone (no cut), for
/// comparison: what a separate analysis filterbank would add.
fn time_bank8() -> f64 {
    #[derive(Clone, Copy)]
    struct S {
        s1: f32,
        s2: f32,
    }
    let fc = [60.0f32, 120.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
    let k = 1.0 / 1.41f32;
    let co: Vec<(f32, f32, f32)> = fc
        .iter()
        .map(|f| {
            let g = (core::f32::consts::PI * f / FS as f32).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            (a1, g * a1, g * g * a1)
        })
        .collect();
    let mut st = [[S { s1: 0.0, s2: 0.0 }; 8]; MAX_PARTS];
    let input: Vec<_> = (0..64).map(buses).collect();
    let mut e = [[0.0f32; 8]; MAX_PARTS];
    let t = Instant::now();
    for blk in 0..BLOCKS {
        let b = black_box(&input[blk % 64]);
        for p in 0..MAX_PARTS {
            for &x in &b[p] {
                for (i, &(a1, a2, a3)) in co.iter().enumerate() {
                    let s = &mut st[p][i];
                    let v3 = x - s.s2;
                    let v1 = a1 * s.s1 + a2 * v3;
                    let v2 = s.s2 + a2 * s.s1 + a3 * v3;
                    s.s1 = 2.0 * v1 - s.s1;
                    s.s2 = 2.0 * v2 - s.s2;
                    e[p][i] += v1 * v1;
                }
            }
        }
    }
    black_box(&e);
    t.elapsed().as_nanos() as f64 / BLOCKS as f64
}

#[test]
#[ignore]
fn cost() {
    let block_ns = BLOCK_SIZE as f64 * 1e9 / FS as f64;
    let mixed = [
        Role::Front,
        Role::Front,
        Role::Mid,
        Role::Mid,
        Role::Back,
        Role::Back,
    ];
    let cutting = time(mixed);
    let listening = time([Role::Mid; MAX_PARTS]);
    let bank8 = time_bank8();
    for (what, ns) in [
        ("SMART EQ, six Parts, four cutting", cutting),
        ("SMART EQ, six Parts, listening only", listening),
        ("an 8-band analysis bank alone, six Parts", bank8),
    ] {
        println!(
            "{what}: {ns:.0} ns a block, {:.2} % of the block",
            100.0 * ns / block_ns
        );
    }
}
