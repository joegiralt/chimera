// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLASSIC LFO RATE's turn (ADR 0279, #1005): a twelfth of an octave a
//! tick, so the slow end has room; what is stored is unchanged.

use chimera_core::dsp::lfo::LFO_SPECS;
use chimera_core::ui::feel::lfo_rate;

/// CLASSIC RATE steps a twelfth of an octave: under 1 Hz a detent moves
/// under 6 %, the ends are still reached in about as many detents as the
/// linear 0.15 Hz steps took, and a stored rate (Hz, as before) is kept.
#[test]
fn classic_rate_is_finer_below_one_hertz() {
    let turned = lfo_rate;
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

/// Each detent reads new, at the slow end too (review A7), and the
/// readout fits its cell.
#[test]
fn each_detent_reads_new() {
    use chimera_core::ui::fmt::{FmtBuf, fmt_val};
    use chimera_core::ui::{draw, theme};
    let s = &LFO_SPECS[0];
    let text = |hz: f32| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, (hz - s.min) / (s.max - s.min), s.fmt);
        b.as_str().to_owned()
    };
    let mut hz = s.min;
    let mut last = text(hz);
    while hz < s.max {
        hz = lfo_rate(hz, 1);
        let t = text(hz);
        assert_ne!(t, last, "{hz} Hz");
        let w = draw::text_width(&theme::FONT_VALUE, &t, 0);
        assert!(w <= theme::CELL_COL_W - 4, "{t}: {w} px");
        last = t;
    }
}
