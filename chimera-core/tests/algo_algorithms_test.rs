//! Spec § Algorithms and § Testing: every algorithm has operator 1 as a
//! carrier, no self-links, modulators numbered above their targets; T1–T8
//! are the TX81Z's routings; every pair plans forward.

use chimera_core::dsp::algo::algorithms::{ALGO_COUNT, ALGO_NAMES, ALGORITHMS, AlgoId, plan};
use chimera_core::dsp::algo::plan::OPS;

/// (modulator, target) links and carriers.
type Routing = (&'static [(u8, u8)], &'static [u8]);

/// The TX81Z's eight algorithms on operators 1–4 (owner's manual; ADR 0018
/// for 4).
const TX81Z: [Routing; 8] = [
    (&[(4, 3), (3, 2), (2, 1)], &[1]),
    (&[(3, 2), (4, 2), (2, 1)], &[1]),
    (&[(3, 2), (2, 1), (4, 1)], &[1]),
    (&[(4, 3), (3, 1), (2, 1)], &[1]),
    (&[(2, 1), (4, 3)], &[1, 3]),
    (&[(4, 1), (4, 2), (4, 3)], &[1, 2, 3]),
    (&[(4, 3)], &[1, 2, 3]),
    (&[], &[1, 2, 3, 4]),
];

/// Links and carriers per algorithm, counted from the spec's tables.
const LINKS: [u32; ALGO_COUNT] = [
    4, 4, 4, 4, 3, 4, 2, 1, 0, 1, 2, 2, 4, 2, 3, 3, 3, 4, 5, 4, 4, 6, 4, 8, 5, 5, 6, 5, 5, 6, 8, 7,
];
const CARRIERS: [u32; ALGO_COUNT] = [
    2, 2, 2, 2, 3, 4, 4, 5, 6, 5, 5, 4, 5, 4, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1,
];

#[test]
fn every_algorithm_follows_the_convention() {
    for (i, alg) in ALGORITHMS.iter().enumerate() {
        assert!(
            alg.carriers & 1 != 0,
            "{}: operator 1 is a carrier",
            alg.name
        );
        for (op, m) in alg.mods.iter().enumerate() {
            assert_eq!(
                m >> op,
                0,
                "{}: operator {} links to itself or up",
                alg.name,
                op + 1
            );
            let heard = alg.carriers & (1 << op) != 0 || *m != 0;
            assert!(heard, "{}: operator {} heard", alg.name, op + 1);
        }
        let links: u32 = alg.mods.iter().map(|m| m.count_ones()).sum();
        assert_eq!(links, LINKS[i], "{} links", alg.name);
        assert_eq!(
            alg.carriers.count_ones(),
            CARRIERS[i],
            "{} carriers",
            alg.name
        );
        assert_eq!(ALGO_NAMES[i], alg.name);
    }
}

#[test]
fn names_are_t1_to_t8_then_a1_to_a24() {
    for (i, name) in ALGO_NAMES.iter().enumerate() {
        let want = if i < 8 {
            format!("T{}", i + 1)
        } else {
            format!("A{}", i - 7)
        };
        assert_eq!(*name, want);
    }
}

#[test]
fn t1_to_t8_are_the_tx81z_routings_with_a_6_to_5_pair() {
    for (t, (links, carriers)) in TX81Z.iter().enumerate() {
        let alg = &ALGORITHMS[t];
        let mut mods = [0u8; OPS];
        for &(m, target) in *links {
            mods[m as usize - 1] |= 1 << (target - 1);
        }
        let carriers = carriers.iter().fold(0u8, |c, &op| c | 1 << (op - 1));
        let low: Vec<u8> = alg.mods[..4].iter().map(|m| m & 0b1111).collect();
        assert_eq!(low, mods[..4], "{} links", alg.name);
        assert_eq!(alg.carriers & 0b1111, carriers, "{} carriers", alg.name);
        assert_eq!(
            (alg.mods[4], alg.mods[5]),
            (0, 0b1_0000),
            "{}: 6 → 5",
            alg.name
        );
        assert_eq!(alg.carriers >> 4, 0b01, "{}: 5 heard, 6 not", alg.name);
    }
}

/// ADR 0018: in T4, operator 3 modulates operator 1, never operator 2.
#[test]
fn t4_operator_2_is_not_modulated_by_operator_3() {
    let t4 = AlgoId::T4.algorithm();
    assert_eq!(t4.mods[2] & 0b10, 0);
    assert_ne!(t4.mods[2] & 0b01, 0);
}

#[test]
fn every_pair_plans_forward_with_both_algorithms_links() {
    for a in 0..ALGO_COUNT as u8 {
        for b in 0..ALGO_COUNT as u8 {
            let (ia, ib) = (AlgoId::clamped(a), AlgoId::clamped(b));
            let p = plan(ia, ib);
            assert_eq!(p.order, [5, 4, 3, 2, 1, 0]);
            assert_eq!(p.delayed, 0);
            for e in &p.edges[..p.edge_count()] {
                let bit = 1 << e.dst;
                assert_eq!(
                    e.a,
                    (ia.algorithm().mods[e.src as usize] & bit != 0) as u8 as f32
                );
                assert_eq!(
                    e.b,
                    (ib.algorithm().mods[e.src as usize] & bit != 0) as u8 as f32
                );
            }
            let union: u32 = (0..OPS)
                .map(|i| (ia.algorithm().mods[i] | ib.algorithm().mods[i]).count_ones())
                .sum();
            assert_eq!(p.edge_count() as u32, union);
        }
    }
}

#[test]
fn ids_clamp_and_name_their_algorithm() {
    assert_eq!(AlgoId::clamped(99), AlgoId::A24);
    assert_eq!(AlgoId::A14.algorithm().name, "A14");
    assert_eq!(AlgoId::T1.get(), 0);
    assert_eq!(AlgoId::A1.get(), 8);
}
