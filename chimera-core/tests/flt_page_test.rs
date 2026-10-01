//! The FLT pages are honest (filter-routing spec § Tests "Knobs are
//! honest", "Route knobs").
mod common;
use chimera_core::project::PartId;
use common::Rig;

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::waves::WaveId;
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
    let mut v = Rig::new(chimera_hal::SAMPLE_RATE);
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

/// Removes CUTOFF from Part 1's matrix and reloads it (re-entering Part 1).
fn without_cutoff(ui: &mut UiState, fill: bool) {
    let sound = &mut ui.project_mut().edit_part(PartId::ALL[0]).part.sound;
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
    feed(ui, Input::press(ButtonId::B2));
    feed(ui, Input::press(ButtonId::B1)); // via Part 2: Part 1's home
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

/// Plus five times from Part 1's home: the MOD node, whose home is the
/// matrix.
fn to_matrix(ui: &mut UiState) {
    feed(ui, Input::press(ButtonId::B2));
    feed(ui, Input::press(ButtonId::B1)); // via Part 2: Part 1's home
    for _ in 0..5 {
        feed(ui, Input::press(ButtonId::Plus));
    }
    assert_eq!(
        ui.page(),
        chimera_core::ui::page::PageKey::Part {
            def: chimera_core::ui::block_registry::MOD_MATRIX.id,
            op: chimera_core::addr::Op::A
        }
    );
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
    feed(&mut ui, Input::press(ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::B1)); // via Part 2: Part 1's home
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

/// Back to FLT (Part 1's home, then PLUS), or on to FLT › MODE with `sub`.
fn back_to_flt(ui: &mut UiState, sub: bool) {
    feed(ui, Input::press(ButtonId::B2));
    feed(ui, Input::press(ButtonId::B1)); // via Part 2: Part 1's home
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

    let (label, fmt) = ("ENV", chimera_core::ui::page::ValFmt::Route);
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
        label,
        text: "",
        value: 0.0,
        fmt,
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
    viz::filter(
        &mut want,
        a(1),
        a(2),
        viz::Response::Low,
        Some((label, "--")),
    );
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

/// On a CellGrid page (FLT › MODE's LFO knob) the focus band reads `--`
/// as the cell does: in INK2, with no arc.
#[test]
fn an_absent_route_knob_reads_dashes_in_the_focus_band() {
    use chimera_core::ui::components;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme;
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // focus LFO
    feed(&mut ui, Input::turn(EncoderId::C, -1));
    to_matrix(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::A, 3)); // row LFO1
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.matrix_state.route(1, CUTOFF), None);
    back_to_flt(&mut ui, true);
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());

    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    components::focus_band(
        &mut want,
        "LFO",
        "--",
        ui.renderer.anim[2].current(),
        chimera_core::ui::page::ValFmt::Route.is_bipolar(),
        components::Look::Absent,
        None,
    );
    assert!(same_rect(
        &fb,
        &want,
        (0, 240),
        (theme::HEADER_BOTTOM, theme::FOCUS_BOTTOM)
    ));
    let band: Vec<_> = (theme::HEADER_BOTTOM..theme::FOCUS_BOTTOM)
        .flat_map(|y| (0..240).map(move |x| (x, y)))
        .map(|(x, y)| fb.at(x, y))
        .collect();
    assert!(band.contains(&theme::INK2), "the dash is INK2");
    let live = [theme::INK, theme::ACCENT];
    assert!(
        !band.iter().any(|c| live.contains(c)),
        "no INK value, no arc"
    );
}

/// The Cells key carries the real looks: FLT's KIND (slot 0) dimmed, ENV
/// (slot 4) absent, KEY (slot 5) live, and the matrix's revision.
#[test]
fn the_cells_key_carries_the_looks() {
    use chimera_core::ui::components::Look;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::{RegionData, RegionKind};
    let mut ui = on_flt(EngineType::Algo);
    to_matrix(&mut ui); // E1 → CUTOFF
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    back_to_flt(&mut ui, false);
    let mut fb = Fb::new();
    ui.render_dirty_with_audio(&mut fb, &PerfStats::zero(), None, &scope_fixture());
    let Some(RegionData::Cells {
        looks, matrix_rev, ..
    }) = ui.drawn_key(RegionKind::Cells)
    else {
        panic!("FLT has cells");
    };
    assert_eq!(looks >> 8 & 3, Look::Absent as u16, "ENV");
    assert_eq!(looks >> 10 & 3, Look::Live as u16, "KEY");
    assert_eq!(looks & 3, Look::Dimmed as u16, "KIND");
    assert_eq!(looks >> 2 & 0x3f, 0, "the other slots are live");
    assert_eq!(matrix_rev, ui.matrix_state.rev);
}

/// The UI's stand-ins (spec § UI): VEL stands in at 1, a B slot at ½; the
/// CUTOFF bar moves by the octave law at that value. From 20 Hz, so no
/// offset here reaches the top.
#[test]
fn stand_ins_move_the_cutoff_bar() {
    use chimera_core::dsp::modulator::EnvType;
    use chimera_core::modulation::amount_scale;
    let mut ui = on_flt(EngineType::Algo);
    ui.params_mut().filter.cutoff = 20.0;
    ui.update();
    let base = ui.renderer.anim[1].target();
    let spec = CUTOFF.spec().unwrap();
    let d = ui.mod_state().find(CUTOFF).unwrap();
    let bar = |ui: &mut UiState, src: ModSource, amount: i8| {
        ui.mod_state_mut().set_route(src.index(), d, amount);
        ui.update();
        let t = ui.renderer.anim[1].target();
        ui.mod_state_mut().set_route(src.index(), d, 0);
        t
    };
    let vel = spec.offset_normalized(base, amount_scale(100));
    assert!(vel > base && vel < 1.0);
    assert_eq!(bar(&mut ui, ModSource::Vel, 100), vel, "VEL at 1");

    assert_eq!(ui.params().envelopes[2].env_type, EnvType::B);
    let half = spec.offset_normalized(base, 0.5 * amount_scale(127));
    assert!(half > base && half < 1.0);
    assert_eq!(bar(&mut ui, ModSource::Env3, 127), half, "ENV 3 (B) at 1/2");
}

use chimera_core::block::Block;
use chimera_core::dsp::filter::{FilterKind, FilterMode, kind_change};
use chimera_core::params::FILTER_SPECS;
use chimera_core::ui::filter_panel::{self, PanelTarget};

/// Spec § Tests "Kinds and modes", over every built kind.
#[test]
fn every_kind_keeps_its_mode_in_its_list() {
    for k in FilterKind::BUILT {
        assert!(!k.modes().is_empty(), "{k:?}");
        let mut p = FilterParams::default();
        p.set_kind(k);
        assert!(k.modes().contains(&p.mode()));
        for m in FilterMode::ALL {
            let took = p.set_mode(m);
            assert_eq!(took, k.modes().contains(&m), "{k:?} {m:?}");
            assert!(k.modes().contains(&p.mode()));
        }
        // A MODE write past the list lands on its last mode.
        p.set(FilterParams::MODE, 99.0);
        assert_eq!(p.mode(), *k.modes().last().unwrap(), "{k:?}");
        // A KIND change keeps MODE if the new kind has it, else its default.
        let q = kind_change(p, k);
        assert_eq!(q.mode(), p.mode());
    }
}

/// `applies` names exactly the filter parameters on the kind's panel.
#[test]
fn applies_matches_the_panel() {
    for k in FilterKind::BUILT {
        let p = filter_panel::panel(k);
        let on = |id| {
            p.main
                .iter()
                .chain(p.extras.iter().flatten())
                .any(|n| n.target == PanelTarget::Filter(id))
        };
        for s in FILTER_SPECS.iter() {
            let want = on(s.id) || s.id == FilterParams::KIND || s.id == FilterParams::MODE;
            assert_eq!(filter_panel::applies(k, s.id), want, "{k:?} {:?}", s.id);
        }
    }
}

/// KIND is dimmed while the SVF is the one built kind: its encoder does
/// nothing, and a KIND "change" leaves the matrix byte-identical. (KIND is
/// an enum the registry already refuses to prime, so the dimming is
/// asserted on its own.)
#[test]
fn kind_is_fixed_and_never_edits_the_matrix() {
    use chimera_core::ui::view::{self, SlotCtx};
    let s = chimera_core::preset::Sound::init(EngineType::Algo);
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    assert!(view::is_dimmed(
        &view::view(&chimera_core::ui::block_registry::FILTER, 0, &ctx),
        &s
    ));
    let mut ui = on_flt(EngineType::Algo);
    let before = format!("{:?}", ui.mod_state());
    feed(&mut ui, Input::turn(EncoderId::A, 3));
    assert_eq!(ui.params().filter.kind(), FilterKind::Svf);
    assert_eq!(format!("{:?}", ui.mod_state()), before);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::NotModulatable));
}
