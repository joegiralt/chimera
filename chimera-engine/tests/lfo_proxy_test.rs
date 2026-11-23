// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The LFO page's raster through the door (ADR 0177): open (or with no
//! door, a build without `bank2`) the glyph and the scope's curve are
//! `draw`'s own; closed, nothing is drawn and the page keeps its ground.

use chimera_engine::dsp::lfo::LfoShape;
use chimera_engine::lfo_glyph::{COLS, Canvas, Glyph, LfoGauge, Q, Wave, curve, draw, paint};

fn gauge() -> LfoGauge {
    LfoGauge {
        glyph: Glyph::Phase,
        wave: Wave::Classic {
            rate: Q::of(0.05),
            shape: LfoShape::Sine,
            retrig: true,
            phase: Q::of(0.25),
            depth: Q::of(1.0),
        },
    }
}

#[test]
fn the_door_draws_what_draw_draws() {
    let g = gauge();
    let (mut want, mut want_ys) = (Canvas::new(), [0i8; COLS]);
    draw::render(&g, &mut want);
    draw::curve(&g.wave, &mut want_ys);
    #[cfg(feature = "bank2")]
    {
        chimera_engine::bank2::DOOR_CELL.close();
        let (mut c, mut ys) = (Canvas::new(), [0i8; COLS]);
        assert!(!paint(&g, &mut c), "closed: nothing");
        assert!(!curve(&g.wave, &mut ys));
        assert!(c.0.iter().all(|&i| i == chimera_engine::lfo_glyph::Ink::Bg) && ys == [0; COLS]);
        chimera_engine::bank2::DOOR_CELL.open_host();
    }
    let (mut c, mut ys) = (Canvas::new(), [0i8; COLS]);
    assert!(paint(&g, &mut c));
    assert!(curve(&g.wave, &mut ys));
    assert!(c.0 == want.0 && ys == want_ys);
}
