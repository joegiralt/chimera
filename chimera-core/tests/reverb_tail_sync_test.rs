//! #366: the reverb's TAIL glyph draws the lengths the reverb plays, so
//! under SYNC its synced ones (GATE's LENGTH in note values).

#![cfg(feature = "tape")]

mod screen;

use chimera_core::dsp::reverb::{RevSync, ReverbType};
use chimera_core::ui::UiState;
use chimera_core::ui::theme::{CUBE_H, CUBE_W, CUBE_X, CUBE_Y};
use chimera_hal::EncoderId;
use embedded_graphics::pixelcolor::IntoStorage;
use screen::{Fb, Input, REV, feed, render_ui, settle, stack_ui, to_fx};

/// The focus band's glyph box on the reverb page, GATE at LENGTH `size`
/// under `sync`.
fn tail(size: f32, sync: RevSync) -> Vec<u16> {
    let ui = gate_ui(size, sync);
    let fb: Fb = render_ui(&ui);
    let mut px = vec![];
    for y in CUBE_Y..CUBE_Y + CUBE_H {
        for x in CUBE_X..CUBE_X + CUBE_W {
            px.push(fb.at(x, y).into_storage());
        }
    }
    px
}

/// The reverb page, GATE at LENGTH `size` under `sync`, LENGTH focused.
fn gate_ui(size: f32, sync: RevSync) -> UiState {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui: UiState = stack_ui();
    {
        let r = &mut ui.project_mut().edit_fx().reverb;
        r.kind = ReverbType::Gate;
        r.size = size;
        r.sync = sync;
    }
    to_fx(&mut ui, REV);
    // LENGTH focused: the band draws the TAIL.
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    ui.project_mut().edit_fx().reverb.size = size;
    settle(&mut ui);
    ui
}

#[test]
fn a_synced_gate_draws_its_synced_length() {
    // Somewhere LENGTH's ms and its note value differ.
    let moved = [0.3, 0.5, 0.7]
        .into_iter()
        .any(|v| tail(v, RevSync::Off) != tail(v, RevSync::Time));
    assert!(moved, "SYNC leaves the TAIL as unsynced");
}

/// The TAIL's burst ends where the reverb's synced LENGTH does: its
/// accent's edge (LENGTH focused) at `length_ms / span` of the columns,
/// the length `reverb::knobs` plays at the reverb's tempo.
#[test]
fn a_synced_gate_tails_burst_ends_at_the_synced_length() {
    use chimera_core::dsp::rev::sync::{FoldState, Tempo};
    use chimera_core::dsp::reverb::knobs;
    use chimera_core::ui::glyph::{Gauge, TAIL_COLS, tail_span_ms};
    for size in [0.3, 0.5, 0.7] {
        let ui = gate_ui(size, RevSync::Time);
        let p = ui.project().perf().fx.reverb;
        let (tempo, mut fold): (Tempo, FoldState) = chimera_core::meter::rev_clock();
        let k = knobs(&p, tempo, &mut fold);
        let n = TAIL_COLS as f32;
        let want =
            chimera_core::dsp::clampf(k.length_ms / tail_span_ms(ReverbType::Gate) * n, 0.0, n)
                as u8;
        let Some(Gauge::Tail(t)) = ui.shown_gauge() else {
            panic!("LENGTH's band draws the TAIL");
        };
        assert_eq!(t.accent, (0, want), "size {size}");
    }
}
