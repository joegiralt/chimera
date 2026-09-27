//! The reverb ring (FX diet spec § Reverb, § Testing). GRIT 0 wherever RT60
//! or stereo is measured.

use chimera_core::dsp::ring::*;

const FS_RING: f32 = 24_000.0;

#[test]
fn every_size_step_is_distinct_primes_that_fit_their_lines() {
    for (i, row) in SIZE_TABLE.iter().enumerate() {
        for (j, &n) in row.iter().enumerate() {
            assert!(
                n >= 2 && (2..n).all(|d| n % d != 0),
                "step {i} line {j}: {n}"
            );
            assert!(n <= BASE[j], "step {i} line {j}: {n} > {}", BASE[j]);
            assert!(!row[..j].contains(&n), "step {i}: {n} twice");
        }
    }
    assert_eq!(SIZE_TABLE[31], BASE);
    assert_eq!(
        SIZE_TABLE[0],
        [53, 109, 1721, 61, 127, 1801, 73, 139, 1889, 83, 97, 1973]
    );
    let round_trip = |s| (0..STAGES).map(|k| stage_len(s, k) as u32).sum::<u32>();
    assert_eq!(round_trip(31), 23_208);
    assert_eq!(round_trip(0), 8_126);
}

#[test]
fn size_rounds_to_32_steps() {
    assert_eq!(size_step(0.0), 0);
    assert_eq!(size_step(0.5), 16);
    assert_eq!(size_step(1.0), 31);
    assert_eq!(size_step(-1.0), 0);
    assert_eq!(size_step(2.0), 31);
    for i in 0..32u8 {
        assert_eq!(size_step(i as f32 / 31.0), i);
    }
}

#[test]
fn time_floor_rises_only_at_the_largest_sizes() {
    for s in 0..=26 {
        assert_eq!(t_min(s, FS_RING), 0.3, "step {s}");
    }
    assert!((t_min(31, FS_RING) - 0.335).abs() < 1e-3);
    assert!((rt60(0.0, 15, FS_RING) - 0.3).abs() < 1e-6);
    assert!((rt60(1.0, 15, FS_RING) - 12.0).abs() < 1e-3);
    assert_eq!(rt60(0.0, 31, FS_RING), t_min(31, FS_RING));
}

#[test]
fn stage_gains_spread_the_decay_over_the_round_trip() {
    for s in [0u8, 15, 31] {
        let trip: f32 = (0..STAGES).map(|k| stage_len(s, k) as f32).sum();
        let g = stage_gains(trip / FS_RING, s, FS_RING);
        let product: f32 = g.iter().product();
        assert!((product - 1e-3).abs() < 1e-6, "step {s}: {product}");
    }
    let most = (0..32u8)
        .flat_map(|s| stage_gains(rt60(1.0, s, FS_RING), s, FS_RING))
        .fold(0.0f32, f32::max);
    assert!(most < MAX_GAIN && (most - 0.956).abs() < 1e-3, "{most}");
}

#[test]
fn damp_is_a_stable_one_pole_from_11_khz_to_1_5_khz() {
    assert!((damp_coef(0.0, FS_RING) - 0.943_85).abs() < 1e-4);
    assert!((damp_coef(1.0, FS_RING) - 0.324_77).abs() < 1e-4);
    for i in 0..=128 {
        let a = damp_coef(i as f32 / 128.0, FS_RING);
        assert!(a > 0.0 && a <= 1.0, "{i}: {a}");
    }
}

#[test]
fn grit_rounds_onto_its_grid() {
    let g0 = Grid::new(0.0);
    assert_eq!(g0.delta(), 1.0);
    for v in i16::MIN..=i16::MAX {
        assert_eq!(g0.q(v as f32), v);
    }
    assert_eq!(g0.q(-100.7), -101);
    assert_eq!(g0.q(100.7), 101);
    assert_eq!(g0.q(-0.5), -1);
    assert_eq!(g0.q(0.5), 1);
    let g1 = Grid::new(1.0);
    assert_eq!(g1.delta(), 64.0);
    assert_eq!(g1.q(32.0), 64);
    assert_eq!(g1.q(-32.0), -64);
    assert_eq!(g1.q(31.9), 0);
    assert_eq!(g1.q(-31.9), 0);
    assert_eq!(Grid::new(0.5).delta(), 8.0);
    for i in 0..=100 {
        let g = Grid::new(i as f32 / 100.0);
        for x in [-30_000.3f32, -777.7, -1.5, 0.0, 2.5, 999.9, 30_000.1] {
            // The grid point, rounded (never truncated) to an LSB.
            let p = (x / g.delta()).round() * g.delta();
            assert!((g.q(x) as f32 - p).abs() <= 0.501, "GRIT {i}: {x}");
            assert!(
                (g.q(x) as f32 - x).abs() <= g.delta() / 2.0 + 0.501,
                "GRIT {i}: {x}"
            );
        }
    }
}

#[test]
fn the_first_reflection_is_the_shortest_tap_at_twice_the_rate() {
    assert_eq!(first_reflection(0), 240);
    assert_eq!(first_reflection(16), 470);
    assert_eq!(first_reflection(31), 686);
}

#[test]
fn grit_saturates_past_the_i16_range() {
    for grit in [0.0, 0.5, 1.0] {
        let g = Grid::new(grit);
        assert_eq!(g.q(33_000.0), i16::MAX, "grit {grit}");
        assert_eq!(g.q(2_100_000.0), i16::MAX, "grit {grit}");
        assert_eq!(g.q(-40_000.0), i16::MIN, "grit {grit}");
        assert_eq!(g.q(-2_100_000.0), i16::MIN, "grit {grit}");
    }
}
