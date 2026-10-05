//! A synced reverb cell redraws when the clock moves (reverb TYPEs spec §
//! Tempo sync): its text reads the tempo and fold the audio thread
//! publishes, outside the cell's own value. Its own binary: it publishes
//! the global clock (`meter::rev_clock`).

#![cfg(feature = "orbit")]

mod screen;

use chimera_core::dsp::rev::sync::{FoldState, Tempo};
use chimera_core::dsp::reverb::{RevSync, ReverbType};
use chimera_core::meter::publish_rev_clock;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::{UiState, draw, theme};
use chimera_hal::EncoderId;
use embedded_graphics::prelude::*;

/// GATE's 1/4 synced at 120 BPM reads `1/4`; published at 90 BPM, where
/// the engine halves it (fold 1), the dirty render redraws the LENGTH cell
/// and the focus band to `▾1/8` though no stored value moved.
#[test]
fn a_synced_cell_redraws_when_the_clock_moves() {
    let mut ui = UiState::new();
    screen::to_fx(&mut ui, screen::REV);
    screen::feed(&mut ui, screen::Input::turn(EncoderId::A, 4)); // TYPE → GATE
    screen::feed(&mut ui, screen::Input::turn(EncoderId::B, 1)); // focus LENGTH
    {
        let r = &mut ui.project_mut().edit_fx().reverb;
        assert_eq!(r.kind, ReverbType::Gate);
        (r.sync, r.size) = (RevSync::Time, 1.0); // 1/4
    }
    screen::settle(&mut ui);
    let (perf, scope) = (PerfStats::zero(), screen::scope_fixture());
    publish_rev_clock(Tempo(120.0), FoldState(0));
    let mut fb = screen::Fb::new();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let at_120 = fb.px.clone();

    publish_rev_clock(Tempo(90.0), FoldState(1));
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let mut full = screen::Fb::new();
    ui.render_with_audio(&mut full, &perf, None, &scope);
    publish_rev_clock(Tempo::DEFAULT, FoldState(0));

    assert!(fb.px == full.px, "the dirty render kept a stale note");
    // The band's value and the cells both moved.
    let rows = |px: &[u16], r: core::ops::Range<i32>| {
        px[r.start as usize * screen::W..r.end as usize * screen::W].to_vec()
    };
    let band = theme::HEADER_BOTTOM..theme::FOCUS_BOTTOM;
    let cells = theme::FOCUS_BOTTOM..screen::H as i32;
    assert!(
        rows(&fb.px, band.clone()) != rows(&at_120, band),
        "the band"
    );
    assert!(
        rows(&fb.px, cells.clone()) != rows(&at_120, cells),
        "the cell"
    );
    // The band reads `▾1/8`: drawn alone at its place, it matches.
    let (x0, x1) = (theme::FOCUS_VALUE_X, theme::CUBE_X - 4);
    let (y0, y1) = (theme::FOCUS_VALUE_Y - 48, theme::FOCUS_VALUE_Y + 8);
    let mut want = screen::Fb::new();
    want.px = fb.px.clone();
    for y in y0..y1 {
        for x in x0..x1 {
            Pixel(Point::new(x, y), theme::BG).draw(&mut want).unwrap();
        }
    }
    draw::text(
        &mut want,
        &theme::FONT_FOCUS,
        "▾1/8",
        theme::FOCUS_VALUE_X,
        theme::FOCUS_VALUE_Y,
        theme::INK,
    );
    assert!(fb.px == want.px, "the band does not read ▾1/8");
}
