//! Spec § UI and § Testing: the computed diagram stays inside the viz band
//! for all 32 algorithms, with no overlapping nodes and every link pointing
//! down; a deep chain switches to the compact spacing.

use chimera_core::dsp::algo::algorithms::{ALGORITHMS, AlgoId};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::ui::alg_layout::{blend, layout};
use chimera_core::ui::theme;

#[test]
fn every_algorithm_fits_the_band_without_overlap_and_links_point_down() {
    for alg in ALGORITHMS.iter() {
        let l = layout(alg);
        for (op, &(x, y)) in l.pos.iter().enumerate() {
            assert!(
                y - l.r > theme::VIZ_BAND_TOP && y + l.r < theme::VIZ_BAND_BOTTOM,
                "{} op {} y {y}",
                alg.name,
                op + 1
            );
            assert!(
                x - l.r >= theme::VIZ_LEFT && x + l.r <= theme::VIZ_RIGHT,
                "{} op {} x {x}",
                alg.name,
                op + 1
            );
        }
        for i in 0..OPS {
            for j in i + 1..OPS {
                let (dx, dy) = (l.pos[i].0 - l.pos[j].0, l.pos[i].1 - l.pos[j].1);
                assert!(
                    dx * dx + dy * dy >= (2 * l.r + 1).pow(2),
                    "{}: operators {} and {} overlap",
                    alg.name,
                    i + 1,
                    j + 1
                );
            }
        }
        for (src, m) in alg.mods.iter().enumerate() {
            for dst in 0..OPS {
                if m & (1 << dst) != 0 {
                    assert!(
                        l.pos[src].1 < l.pos[dst].1,
                        "{}: {}→{}",
                        alg.name,
                        src + 1,
                        dst + 1
                    );
                }
            }
        }
    }
}

#[test]
fn a_six_deep_chain_is_compact_and_a1_is_one_row() {
    let a17 = layout(AlgoId::A17.algorithm());
    assert!(a17.r < layout(AlgoId::T1.algorithm()).r);
    let a1 = layout(AlgoId::A1.algorithm());
    assert!(a1.pos.iter().all(|p| p.1 == theme::VIZ_BAND_MID));
}

#[test]
fn the_blend_ends_are_the_two_layouts() {
    let (a, b) = (
        layout(AlgoId::T1.algorithm()),
        layout(AlgoId::A17.algorithm()),
    );
    assert_eq!(blend(&a, &b, 0.0).pos, a.pos);
    assert_eq!(blend(&a, &b, 1.0).pos, b.pos);
    assert_eq!(blend(&a, &b, 0.5).r, b.r);
}
