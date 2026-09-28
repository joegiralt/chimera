//! The FLT pages are honest (filter-routing spec § Tests "Knobs are
//! honest", "Route knobs").

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModSource;
use chimera_core::params::EngineType;
use chimera_core::params::FilterParams;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::renderer::amount_of;
use chimera_core::ui::{PrimeStatus, UiState};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId};
use screen::*;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const ENC: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

fn flt_node(ct: EngineType) -> usize {
    chain_def_for(ct)
        .blocks
        .iter()
        .position(|b| b.def.id == FILTER.id)
        .expect("FLT is on every Part chain")
}

/// Part 1 on `ct`'s init Sound with a held saw worth filtering, on FLT.
fn on_flt(ct: EngineType) -> UiState {
    let mut ui = UiState::new();
    load_init(&mut ui, ct);
    let p = ui.params_mut();
    p.filter.cutoff = 2000.0;
    p.algo.ops[0].wave = WaveId::SAW.get();
    for _ in 0..flt_node(ct) {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    ui
}

/// 32 blocks of note 72 (so KEY moves the cutoff) from the UI's Sound.
fn render(ui: &UiState) -> Vec<f32> {
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(72).unwrap(), Velocity::DEFAULT, ui.params());
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..32 {
        v.render(&mut b, ui.params(), ui.mod_state());
        out.extend_from_slice(&b);
    }
    out
}

/// Every bound FLT and FLT › MODE knob changes a held note on Algo and
/// Modal once turned (a route knob once its route is nonzero).
#[test]
fn every_flt_knob_changes_a_held_note() {
    for ct in EngineType::ALL {
        for (sub, slots) in [(0, &[1usize, 2, 3, 4, 5][..]), (1, &[0usize, 1, 2][..])] {
            for &slot in slots {
                let mut ui = on_flt(ct);
                if sub == 1 {
                    feed(&mut ui, Input::press(ButtonId::Edit));
                }
                let before = render(&ui);
                feed(&mut ui, Input::turn(ENC[slot], 40));
                assert_ne!(render(&ui), before, "{ct:?} page {sub} slot {slot}");
            }
        }
    }
}

/// Removes CUTOFF from Part 1's matrix and reloads it (B1 snaps home).
fn without_cutoff(ui: &mut UiState, fill: bool) {
    let sound = &mut ui.performance.parts[0].sound;
    sound.dest_registry.remove(CUTOFF);
    if fill {
        for b in BlockRef::ALL {
            for s in b.specs() {
                let a = ParamAddr::new(b, s.id);
                if a != CUTOFF && a.modulatable() {
                    let _ = sound.dest_registry.add(a, *b"FILL\0\0\0\0");
                }
            }
        }
    }
    // The audio-side matrix follows the registry (no routes kept).
    sound.mod_state = chimera_core::modulation::ModState::from_registry(&sound.dest_registry, 8);
    feed(ui, Input::press(ButtonId::B1));
    for _ in 0..flt_node(EngineType::Algo) {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

#[test]
fn a_route_knob_creates_its_column() {
    let mut ui = on_flt(EngineType::Algo);
    without_cutoff(&mut ui, false);
    let col =
        |ui: &UiState| (0..ui.mod_state().num_dests()).find(|&d| ui.mod_state().dest(d) == CUTOFF);
    assert_eq!(col(&ui), None);
    feed(&mut ui, Input::turn(EncoderId::E, 5)); // ENV
    let d = col(&ui).expect("the turn created CUTOFF's column");
    assert_eq!(ui.mod_state().amount(ModSource::Env1.index(), d), 5);
    // The knob and the matrix cell show the same amount.
    settle(&mut ui);
    assert_eq!(amount_of(ui.renderer.anim[4].current()), 5);
}

#[test]
fn a_full_matrix_keeps_the_route_knob_off() {
    let mut ui = on_flt(EngineType::Algo);
    without_cutoff(&mut ui, true);
    feed(&mut ui, Input::turn(EncoderId::F, 5)); // KEY
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Full));
    assert!((0..ui.mod_state().num_dests()).all(|d| ui.mod_state().dest(d) != CUTOFF));
}

/// Plus five times from Part 1's home: the MOD node (the matrix, until
/// Task 16 puts E1 there).
fn to_matrix(ui: &mut UiState) {
    feed(ui, Input::press(ButtonId::B1));
    for _ in 0..5 {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// Deleting a route at 0 changes no amount, but the dirty render redraws
/// the grid (the desktop and the firmware draw through `render_dirty`).
#[test]
fn deleting_a_route_at_zero_redraws_the_grid() {
    use chimera_core::ui::page::PageLayout;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::{RegionKind, layout_regions};
    let mut ui = UiState::new();
    to_matrix(&mut ui); // E1 → CUTOFF, present at 0
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    let flushed = ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    let &(_, y0, y1) = layout_regions(PageLayout::Matrix)
        .iter()
        .find(|r| r.0 == RegionKind::Grid)
        .unwrap();
    assert!(flushed.contains(&(y0, y1)), "{flushed:?}");
}

/// MIX+MINUS on a matrix cell deletes its route; the ENV knob shows a dash
/// until it is turned, and turning it creates the route.
#[test]
fn mix_minus_deletes_and_the_knob_recreates() {
    let mut ui = on_flt(EngineType::Algo);
    to_matrix(&mut ui); // cursor on E1 → FLT CUTOFF
    assert_eq!(ui.matrix_state.route(0, CUTOFF), Some(0));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.matrix_state.route(0, CUTOFF), None, "deleted");
    // Deleting the route, not un-priming the column: LF1 and NTE stay.
    assert_eq!(ui.mod_state().present(0), 1 << 1 | 1 << 7);
    assert_eq!(ui.mod_state().find(CUTOFF), Some(0), "the column stays");
    assert_eq!(ui.matrix_state.col_of(CUTOFF), Some(0));
    feed(&mut ui, Input::press(ButtonId::B1));
    for _ in 0..flt_node(EngineType::Algo) {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::turn(EncoderId::E, 3)); // ENV
    assert_eq!(
        ui.matrix_state.route(0, CUTOFF),
        Some(3),
        "created at 0, then turned"
    );
}

/// Back to FLT (B1 then PLUS), or on to FLT › MODE with `sub`.
fn back_to_flt(ui: &mut UiState, sub: bool) {
    feed(ui, Input::press(ButtonId::B1));
    for _ in 0..flt_node(EngineType::Algo) {
        feed(ui, Input::press(ButtonId::Plus));
    }
    if sub {
        feed(ui, Input::press(ButtonId::Edit));
    }
}

/// Rows `y0..y1`, columns `x0..x1` of two frames agree.
fn same_rect(a: &Fb, b: &Fb, (x0, x1): (i32, i32), (y0, y1): (i32, i32)) -> bool {
    (y0..y1).all(|y| (x0..x1).all(|x| a.at(x, y) == b.at(x, y)))
}

/// The "—" knob: with E1 → CUTOFF deleted, FLT's focused ENV cell draws a
/// dash where the value goes (no value, no bar) and the viz readout reads
/// `--`, through the dirty render the desktop and firmware use.
#[test]
fn an_absent_route_knob_shows_a_dash() {
    use chimera_core::ui::components::{self, Cell, Look};
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::{theme, viz};
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::turn(EncoderId::E, 1)); // focus ENV
    feed(&mut ui, Input::turn(EncoderId::E, -1)); // back to 0, still present
    to_matrix(&mut ui);
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    back_to_flt(&mut ui, false);
    settle(&mut ui);
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);

    let slot = &FILTER.params[4];
    let (x, y) = (
        theme::MARGIN_X + theme::CELL_COL_W,
        theme::CELL_LABEL_Y + theme::CELL_ROW_H,
    );
    for dx in 0..12 {
        for dy in 0..2 {
            assert_eq!(
                fb.at(x + dx, y + theme::CELL_VALUE_DY - 5 + dy),
                theme::INK2
            );
        }
    }
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    let c = Cell {
        label: slot.label(),
        text: "",
        value: 0.0,
        fmt: slot.format(),
        active: true,
        mod_amount: None,
        look: Look::Absent,
    };
    components::cell(&mut want, 4, theme::CELL_LABEL_Y, Some(&c));
    assert!(
        same_rect(
            &fb,
            &want,
            (x, x + theme::CELL_COL_W),
            (y - 8, theme::CELLS_BOTTOM)
        ),
        "the ENV cell is a dash: no value text, no bar"
    );

    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    let a = |i: usize| ui.renderer.anim[i].current();
    viz::filter(&mut want, a(1), a(2), Some((slot.label(), "--")));
    assert!(
        same_rect(
            &fb,
            &want,
            (0, 240),
            (theme::HEADER_BOTTOM, theme::BIGVIZ_BOTTOM)
        ),
        "the readout reads --"
    );
}

/// On a CellGrid page (FLT › MODE's LFO knob) the focus band reads `--`.
#[test]
fn an_absent_route_knob_reads_dashes_in_the_focus_band() {
    use chimera_core::ui::block_registry::FILTER_MODE;
    use chimera_core::ui::components;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme;
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // focus LFO
    feed(&mut ui, Input::turn(EncoderId::C, -1));
    to_matrix(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::A, 1)); // row LF1
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.matrix_state.route(1, CUTOFF), None);
    back_to_flt(&mut ui, true);
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());

    let slot = &FILTER_MODE.params[2];
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    components::focus_band(
        &mut want,
        slot.label(),
        "--",
        ui.renderer.anim[2].current(),
        slot.format().is_bipolar(),
        None,
    );
    assert!(same_rect(
        &fb,
        &want,
        (0, 240),
        (theme::HEADER_BOTTOM, theme::FOCUS_BOTTOM)
    ));
}
