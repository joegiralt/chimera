// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SMART EQ's host cost (ADR 0296 § Measurements): `cargo test --release
//! -p chimera-engine --test smart_eq_cost -- --ignored --nocapture`.
//! Six Parts written, broadband: one FRONT listened to and five behind
//! it cut in every band (the worst case), against the five's band loops
//! alone (`probe`), which gives the share of a block that is block-rate
//! work (the rules, the levels, the dips): the unit's count of that share
//! is this ratio of its per-sample count. And six Parts only listened to.

use std::hint::black_box;
use std::time::Instant;

use chimera_engine::hw::MAX_PARTS;
use chimera_engine::instrument::Sides;
use chimera_engine::part_eq::PartEqs;
use chimera_engine::part_eq::dsp::smart::{SmartBlock, probe, run};
use chimera_engine::part_eq::smart::{Amount, Band, PartKind, Role, SmartEq, Switch};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

const FS: u32 = 48_000;
const BLOCKS: usize = 200_000;

/// Every Part broadband (a noise each), so every band of every lower Part
/// is cut.
fn buses(k: usize) -> [[f32; BLOCK_SIZE]; MAX_PARTS] {
    core::array::from_fn(|p| {
        core::array::from_fn(|i| {
            let n = ((k * BLOCK_SIZE + i) as u32).wrapping_mul(2_654_435_761 + 2 * p as u32);
            0.2 * ((n >> 8) as f32 / 8_388_608.0 - 1.0)
        })
    })
}

/// ns a block of `run` on six written Parts in `roles`, settled; and how
/// many of the Parts' bands are cut.
fn time(roles: [Role; MAX_PARTS]) -> (f64, usize) {
    let mut st = PartEqs::REST.smart;
    let smart = SmartEq::new(Switch::On, Amount::from_code(64).unwrap());
    let input: Vec<_> = (0..64).map(buses).collect();
    let mut b = input[0];
    let block = |st: &mut _, b: &mut [[f32; BLOCK_SIZE]; MAX_PARTS]| {
        let mut sides: Sides = [None; MAX_PARTS];
        run(
            st,
            SmartBlock {
                buses: b,
                sides: &mut sides,
                written: &[true; MAX_PARTS],
                roles: &roles,
                kinds: &[PartKind::Tonal; MAX_PARTS],
                wash: Role::Back,
                smart,
                sample_rate: FS,
            },
        );
    };
    // Settle the cuts first: the steady state is what plays.
    for k in 0..20_000 {
        b = input[k % 64];
        block(&mut st, &mut b);
    }
    let cut = PartId::ALL
        .iter()
        .flat_map(|&p| Band::ALL.map(|band| st.cut(p, band).get() > 0.5))
        .filter(|c| *c)
        .count();
    let t = Instant::now();
    for k in 0..BLOCKS {
        b = input[k % 64];
        block(&mut st, black_box(&mut b));
    }
    black_box(&b);
    (t.elapsed().as_nanos() as f64 / BLOCKS as f64, cut)
}

/// ns a block of one cut Part's four bands alone.
fn time_bands() -> f64 {
    let input = buses(0)[0];
    let dips = [-0.3f32; 4];
    let mut x = vec![0.0f32; BLOCK_SIZE * 64];
    let rounds = BLOCKS / 64;
    let t = Instant::now();
    for _ in 0..rounds {
        for c in x.chunks_mut(BLOCK_SIZE) {
            c.copy_from_slice(&input);
        }
        probe(black_box(&dips), black_box(&mut x), FS);
    }
    t.elapsed().as_nanos() as f64 / (rounds * 64) as f64
}

#[test]
#[ignore]
fn cost() {
    let block_ns = BLOCK_SIZE as f64 * 1e9 / FS as f64;
    let worst = [
        Role::Front,
        Role::Back,
        Role::Back,
        Role::Back,
        Role::Back,
        Role::Back,
    ];
    let (cutting, cut) = time(worst);
    let (listening, _) = time([Role::Mid; MAX_PARTS]);
    let bands = time_bands();
    println!("one cut Part's bands alone: {bands:.0} ns a block");
    println!(
        "SMART EQ, one FRONT and five BACK, {cut} of 20 bands cut: {cutting:.0} ns a block, {:.2} % of the block; less the five's bands {:.0} ns (the block-rate work and the FRONT's listening), {:.2} of the five's bands",
        100.0 * cutting / block_ns,
        cutting - 5.0 * bands,
        (cutting - 5.0 * bands) / (5.0 * bands),
    );
    println!(
        "SMART EQ, six Parts of one role, listening only: {listening:.0} ns a block, {:.2} % of the block",
        100.0 * listening / block_ns
    );
}
