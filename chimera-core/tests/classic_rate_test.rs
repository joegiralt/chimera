//! CLASSIC LFO RATE's turn (ADR 0279, #1005): a twelfth of an octave a
//! tick, so the slow end has room; what is stored is unchanged.

use chimera_core::block::Block;
use chimera_core::dsp::lfo::{LFO_SPECS, LfoParams};

/// CLASSIC RATE steps a twelfth of an octave: under 1 Hz a detent moves
/// under 6 %, the ends are still reached in about as many detents as the
/// linear 0.15 Hz steps took, and a stored rate (Hz, as before) is kept.
#[test]
fn classic_rate_is_finer_below_one_hertz() {
    let turned = |hz: f32, d: i8| {
        let mut p = LfoParams {
            rate: hz,
            ..LfoParams::default()
        };
        p.nudge(LfoParams::RATE, d);
        p.rate
    };
    for hz in [0.01f32, 0.1, 0.5, 0.95] {
        let up = turned(hz, 1);
        assert!(up > hz && up - hz < 0.06 * hz, "{hz} -> {up}");
        assert!((turned(up, -1) - hz).abs() < 1e-4 * hz.max(1.0));
    }
    let s = &LFO_SPECS[0];
    let (mut v, mut n) = (s.min, 0);
    while v < s.max {
        v = turned(v, 1);
        n += 1;
    }
    assert!((120..=140).contains(&n), "{n} detents end to end");
    assert_eq!(turned(7.3, 0), 7.3);
}
