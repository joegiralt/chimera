//! The FLT pages are honest (filter-routing spec § Tests "Knobs are
//! honest", "Route knobs").
mod common;
use chimera_core::project::PartId;
use chimera_core::ui::animation::UiTick;
use common::Rig;

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::modulation::{FixedCol, MAX_MOD_DESTS, ModSource};
use chimera_core::params::EngineType;
use chimera_core::params::FilterParams;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::nav::chain_def_for;
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
        for (sub, slots) in [(0, &[1usize, 2, 3, 4, 5][..]), (1, &[0usize, 1, 3][..])] {
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

/// Fills Part 1's registry (every column but the fixed ones) and reloads it
/// (re-entering Part 1); `clear` also deletes its FREQ routes.
fn full_registry(ui: &mut UiState, clear: bool) {
    let sound = ui.project_mut().edit_part(PartId::ALL[0]).sound;
    for b in BlockRef::ALL {
        for s in b.specs() {
            let a = ParamAddr::new(b, s.id);
            if a.modulatable() {
                let _ = sound.dest_registry.add(a, *b"FILL\0\0\0\0");
            }
        }
    }
    let mut ms = chimera_core::modulation::ModState::from_registry(&sound.dest_registry, 8);
    if !clear {
        for s in ModSource::ALL {
            let c = FixedCol::Freq;
            if sound.mod_state.fixed_present(c) & 1 << s.index() != 0 {
                ms.set_fixed(s.index(), c, sound.mod_state.fixed_amount(s.index(), c));
            }
        }
    }
    sound.mod_state = ms;
    feed(ui, Input::press(ButtonId::B2));
    feed(ui, Input::press(ButtonId::B1)); // via Part 2: Part 1's home
    for _ in 0..flt_node(EngineType::Algo) {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// A route knob writes the fixed FREQ column, creating its route.
#[test]
fn a_route_knob_creates_its_route() {
    let mut ui = on_flt(EngineType::Algo);
    full_registry(&mut ui, true);
    assert_eq!(ui.mod_state().fixed_present(FixedCol::Freq), 0);
    feed(&mut ui, Input::turn(EncoderId::E, 5)); // ENV
    assert_eq!(
        ui.mod_state()
            .fixed_amount(ModSource::Env1.index(), FixedCol::Freq),
        5
    );
    // The knob and the matrix cell show the same amount.
    settle(&mut ui);
    assert_eq!(amount_of(ui.renderer.anim[4].current()), 5);
}

/// FREQ is a permanent column: a full registry never turns a route knob off.
#[test]
fn a_full_registry_keeps_the_route_knob_on() {
    let mut ui = on_flt(EngineType::Algo);
    full_registry(&mut ui, false);
    assert_eq!(ui.mod_state().num_dests(), MAX_MOD_DESTS);
    feed(&mut ui, Input::turn(EncoderId::F, 5)); // KEY
    assert_eq!(ui.prime_status(), None);
    let note = ui
        .mod_state()
        .fixed_amount(ModSource::Note.index(), FixedCol::Freq);
    assert_eq!(
        ui.matrix_state.route(ModSource::Note.index(), CUTOFF),
        Some(note)
    );
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
    // Deleting the route, not the column: LF1 and NTE stay.
    assert_eq!(
        ui.mod_state().fixed_present(FixedCol::Freq),
        1 << 1 | 1 << 7
    );
    assert_eq!(ui.matrix_state.col_of(CUTOFF), Some(0), "the column stays");
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

    let (label, fmt) = ("FM1", chimera_core::ui::page::ValFmt::Route);
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

/// On a CellGrid page (FLT › MOD's FM3 knob) the focus band reads `--`
/// as the cell does: in INK2, with no arc, and no delete hint.
#[test]
fn an_absent_route_knob_reads_dashes_in_the_focus_band() {
    use chimera_core::ui::components;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme;
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::turn(EncoderId::B, 1)); // focus FM3
    feed(&mut ui, Input::turn(EncoderId::B, -1));
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
        "FM3",
        "--",
        chimera_core::ui::glyph::Gauge::Arc {
            value: ui.renderer.anim[1].current(),
            bipolar: chimera_core::ui::page::ValFmt::Route.is_bipolar(),
        },
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

/// The Cells key carries the real looks: FLT's TYPE (slot 0) live, ENV
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
    assert_eq!(looks >> 8 & 3, Look::Absent as u32, "ENV");
    assert_eq!(looks >> 10 & 3, Look::Live as u32, "KEY");
    assert_eq!(looks & 3, Look::Live as u32, "TYPE");
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
    ui.update(UiTick::for_test());
    let base = ui.renderer.anim[1].target();
    let spec = CUTOFF.spec().unwrap();
    let bar = |ui: &mut UiState, src: ModSource, amount: i8| {
        ui.mod_state_mut()
            .set_fixed(src.index(), FixedCol::Freq, amount);
        ui.update(UiTick::for_test());
        let t = ui.renderer.anim[1].target();
        ui.mod_state_mut().set_fixed(src.index(), FixedCol::Freq, 0);
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

/// Every KIND change, from every mode of every built kind: MODE stays if
/// the new kind has it, else lands on the new kind's `default_mode()`, never
/// its `modes()[0]` (Task 4 review F1). With CLEAN alone the fallback can't
/// run; it runs once a kind lacks one of another's modes: DIRTY HP2 →
/// CLEAN must land on LP4, not LP1.
#[test]
fn kind_change_falls_back_to_the_default_mode() {
    let mut fell_back = 0;
    for from in FilterKind::BUILT {
        for &m in from.modes() {
            let mut p = FilterParams::default();
            p.set_kind(from);
            assert!(p.set_mode(m));
            for to in FilterKind::BUILT {
                let q = kind_change(p, to);
                assert_eq!(q.kind(), to);
                if to.modes().contains(&m) {
                    assert_eq!(q.mode(), m, "{from:?} {m:?} → {to:?}");
                } else {
                    fell_back += 1;
                    assert_eq!(q.mode(), to.default_mode(), "{from:?} {m:?} → {to:?}");
                }
                assert_eq!(
                    (q.cutoff, q.resonance, q.level),
                    (p.cutoff, p.resonance, p.level)
                );
            }
        }
    }
    // Every kind holds every mode only while CLEAN is the one built.
    let all = |k: FilterKind| FilterMode::ALL.iter().all(|m| k.modes().contains(m));
    assert_eq!(fell_back == 0, FilterKind::BUILT.iter().all(|&k| all(k)));
}

/// `applies` names exactly the filter parameters on the kind's panel.
#[test]
fn applies_matches_the_panel() {
    for k in FilterKind::BUILT {
        let p = filter_panel::panel(k);
        let on = |id| {
            p.main
                .iter()
                .chain(p.mods.iter().flatten())
                .any(|n| n.target == PanelTarget::Filter(id))
        };
        for s in FILTER_SPECS.iter() {
            let want = on(s.id) || s.id == FilterParams::KIND || s.id == FilterParams::MODE;
            assert_eq!(filter_panel::applies(k, s.id), want, "{k:?} {:?}", s.id);
        }
    }
}

/// TYPE is live once a second kind is built: a detent a kind, CLEAN to
/// 2040 (whose one MODE shows fixed) to DIRTY, each at its default MODE and
/// held at the last; a TYPE change leaves the matrix byte-identical. (TYPE
/// is an enum the registry refuses to prime.)
#[test]
fn type_steps_the_built_kinds_and_never_edits_the_matrix() {
    use chimera_core::ui::view::{self, SlotCtx};
    let s = chimera_core::preset::Sound::init(EngineType::Algo);
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    assert!(!view::is_dimmed(
        &view::view(&chimera_core::ui::block_registry::FILTER, 0, &ctx),
        &s,
        &ctx
    ));
    let mut ui = on_flt(EngineType::Algo);
    let before = format!("{:?}", ui.mod_state());
    for (kind, mode) in [
        (FilterKind::K2040, FilterMode::Lp24),
        (FilterKind::Dirty, FilterMode::Lp12),
        (FilterKind::Dirty, FilterMode::Lp12),
    ] {
        feed(&mut ui, Input::turn(EncoderId::A, 1));
        assert_eq!(ui.params().filter.kind(), kind);
        assert_eq!(ui.params().filter.mode(), mode);
        assert_eq!(format!("{:?}", ui.mod_state()), before);
    }
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::NotModulatable));
    feed(&mut ui, Input::turn(EncoderId::A, -3));
    assert_eq!(ui.params().filter.kind(), FilterKind::Svf);
    assert_eq!(format!("{:?}", ui.mod_state()), before);
}

// ── Task 12: the jacks, the per-type panels, FM3 and QM as cell views ──

use chimera_core::preset::Sound;
use chimera_core::taps::Jack;
use chimera_core::ui::block_registry::FILTER_MOD;
use chimera_core::ui::filter_panel::{
    JackSlot, RouteView, jack_amount, keybd_step, knob, repatch, route_amount, set_jack_amount,
};
use chimera_core::ui::mod_grid::{self, MatrixState};

fn fixed(s: &Sound, src: ModSource, c: FixedCol) -> Option<i8> {
    (s.mod_state.fixed_present(c) & 1 << src.index() != 0)
        .then(|| s.mod_state.fixed_amount(src.index(), c))
}

/// The matrix's own edit of cell (`src`, `c`), synced as the UI syncs.
fn matrix_set(s: &mut Sound, src: ModSource, c: FixedCol, a: i8) {
    let mut m = MatrixState::new();
    m.rebuild_sources(chain_def_for(s.engine()).mod_sources);
    m.rebuild_dests_from_registry(&s.dest_registry);
    m.load_amounts(&s.mod_state);
    m.set(src.index(), c.index(), a);
    s.mod_state.sync_from_matrix(&m);
}

/// Spec § 2: one stored value, two places to edit it.
#[test]
fn fm3_and_qm_are_views_of_one_cell() {
    let mut s = Sound::init(EngineType::Algo);
    for (slot, src, c) in [
        (JackSlot::Fm3, ModSource::Lfo1, FixedCol::Freq),
        (JackSlot::Qm, ModSource::Env3, FixedCol::Q),
    ] {
        set_jack_amount(&mut s, slot, 50);
        assert_eq!(fixed(&s, src, c), Some(50), "{slot:?}");
        matrix_set(&mut s, src, c, -20);
        assert_eq!(jack_amount(&s, slot), -20, "{slot:?}");
    }
    for (v, src) in [
        (RouteView::Freq(ModSource::Env1), ModSource::Env1),
        (RouteView::Freq(ModSource::Note), ModSource::Note),
        (RouteView::Keybd, ModSource::Note),
    ] {
        matrix_set(&mut s, src, FixedCol::Freq, 33);
        assert_eq!(route_amount(&s, v), 33, "{v:?}");
    }
    // The jack picks the row: FM3 now reads LFO 2's cell.
    matrix_set(&mut s, ModSource::Lfo2, FixedCol::Freq, 9);
    assert!(s.params.filter.set_fm3_src(Jack::Mod(ModSource::Lfo2)));
    assert_eq!(jack_amount(&s, JackSlot::Fm3), 9);
}

/// The knob on FLT › MOD writes the cell the matrix shows.
#[test]
fn the_fm3_and_qm_knobs_write_their_cells() {
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::press(ButtonId::Edit)); // FLT › MOD
    let lfo1 = ui
        .mod_state()
        .fixed_amount(ModSource::Lfo1.index(), FixedCol::Freq);
    feed(&mut ui, Input::turn(EncoderId::B, 7)); // FM3
    feed(&mut ui, Input::turn(EncoderId::D, -5)); // QM
    let m = ui.mod_state();
    assert_eq!(
        m.fixed_amount(ModSource::Lfo1.index(), FixedCol::Freq),
        lfo1 + 7
    );
    assert_eq!(m.fixed_amount(ModSource::Env3.index(), FixedCol::Q), -5);
    assert_eq!(
        ui.matrix_state
            .route(ModSource::Env3.index(), FixedCol::Q.addr()),
        Some(-5)
    );
}

/// Every cell other than (`a`, `c`) and (`b`, `c`) is as it was.
fn others_unchanged(x: &Sound, y: &Sound, moved: [(ModSource, FixedCol); 2]) {
    for src in ModSource::ALL {
        for c in FixedCol::ALL {
            if !moved.contains(&(src, c)) {
                assert_eq!(fixed(x, src, c), fixed(y, src, c), "{src:?} → {c:?}");
            }
        }
    }
    let (m, n) = (&x.mod_state, &y.mod_state);
    assert_eq!(m.num_dests(), n.num_dests());
    for d in 0..m.num_dests() {
        assert_eq!((m.dest(d), m.present(d)), (n.dest(d), n.present(d)));
        for src in 0..m.num_sources() {
            assert_eq!(m.amount(src, d), n.amount(src, d));
        }
    }
}

#[test]
fn repatch_moves_exactly_one_route() {
    let mut s = Sound::init(EngineType::Algo);
    s.mod_state
        .set_fixed(ModSource::Lfo1.index(), FixedCol::Freq, 40);
    s.mod_state
        .set_fixed(ModSource::Env2.index(), FixedCol::Q, -12);
    let before = s.clone();
    assert!(repatch(&mut s, JackSlot::Fm3, Jack::Mod(ModSource::Lfo2)));
    assert_eq!(s.params.filter.fm3_src(), Jack::Mod(ModSource::Lfo2));
    assert_eq!(fixed(&s, ModSource::Lfo2, FixedCol::Freq), Some(40));
    assert_eq!(fixed(&s, ModSource::Lfo1, FixedCol::Freq), None);
    let freq = |src| (src, FixedCol::Freq);
    others_unchanged(&before, &s, [freq(ModSource::Lfo1), freq(ModSource::Lfo2)]);
    // QM IN: ENV 3 holds no route, so none moves, and none appears.
    let before = s.clone();
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Mod(ModSource::Lfo3)));
    assert_eq!(s.params.filter.qm_src(), Jack::Mod(ModSource::Lfo3));
    assert_eq!(fixed(&s, ModSource::Lfo3, FixedCol::Q), None);
    others_unchanged(&before, &s, [(ModSource::Env3, FixedCol::Q); 2]);
    // To itself: nothing to move.
    let before = s.clone();
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Mod(ModSource::Lfo3)));
    assert!(s.bits_eq(&before));
}

/// Review Focus 5: no free cell, no move; the jack stays where it was.
#[test]
fn repatch_with_every_target_occupied_changes_nothing() {
    let mut s = Sound::init(EngineType::Algo);
    for j in Jack::FM3_CHOICES {
        if let Jack::Mod(m) = j {
            s.mod_state.set_fixed(m.index(), FixedCol::Freq, 7);
        }
    }
    for j in Jack::QM_CHOICES {
        if let Jack::Mod(m) = j {
            s.mod_state.set_fixed(m.index(), FixedCol::Q, 7);
        }
    }
    let before = s.clone();
    for (slot, choices) in [
        (JackSlot::Fm3, &Jack::FM3_CHOICES[..]),
        (JackSlot::Qm, &Jack::QM_CHOICES[..]),
    ] {
        for &j in choices {
            if j != slot.default() {
                assert!(!repatch(&mut s, slot, j), "{slot:?} → {j:?}");
                assert!(s.bits_eq(&before), "{slot:?} → {j:?}");
            }
        }
    }
    assert_eq!(s.params.filter.fm3_src(), Jack::Mod(ModSource::Lfo1));
    assert_eq!(s.params.filter.qm_src(), Jack::Mod(ModSource::Env3));
}

/// FM3 IN through the UI: a detent moves the cable to the next free
/// source, skipping an occupied one.
#[test]
fn turning_fm3_in_moves_the_cable_past_an_occupied_cell() {
    let mut ui = on_flt(EngineType::Algo);
    let s = ui.project_mut().edit_part(PartId::ALL[0]).sound;
    s.mod_state
        .set_fixed(ModSource::Lfo1.index(), FixedCol::Freq, 40);
    s.mod_state
        .set_fixed(ModSource::Lfo2.index(), FixedCol::Freq, 11);
    back_to_flt(&mut ui, true);
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // FM3 IN: LFO 2 is taken
    assert_eq!(ui.params().filter.fm3_src(), Jack::Mod(ModSource::Lfo3));
    let m = ui.mod_state();
    assert_eq!(m.fixed_amount(ModSource::Lfo3.index(), FixedCol::Freq), 40);
    assert_eq!(m.fixed_amount(ModSource::Lfo2.index(), FixedCol::Freq), 11);
    assert_eq!(
        m.fixed_present(FixedCol::Freq) & 1 << ModSource::Lfo1.index(),
        0
    );
    assert_eq!(
        ui.matrix_state.route(ModSource::Lfo3.index(), CUTOFF),
        Some(40),
        "the matrix shows the move"
    );
}

/// MIX + MINUS on any route view deletes that cell's route, and the view's
/// hint says so (ADR 0086).
#[test]
fn mix_minus_on_a_route_view_deletes_its_route() {
    use chimera_core::ui::view::{self, SlotCtx};
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::press(ButtonId::Edit)); // FLT › MOD
    feed(&mut ui, Input::turn(EncoderId::D, 9)); // QM: ENV 3 → Q
    assert!(ui.mod_state().fixed_present(FixedCol::Q) != 0);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.mod_state().fixed_present(FixedCol::Q), 0);
    let ctx = SlotCtx::read(ui.params(), chimera_core::addr::Op::A);
    for i in [1, 3] {
        assert_eq!(
            view::view(&FILTER_MOD, i, &ctx).hint(),
            mod_grid::DELETE,
            "{i}"
        );
    }
    for i in [4, 5] {
        assert_eq!(view::view(&FILTER, i, &ctx).hint(), mod_grid::DELETE, "{i}");
    }
    back_to_flt(&mut ui, false);
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // FM2
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(
        ui.mod_state().fixed_present(FixedCol::Freq) & 1 << ModSource::Note.index(),
        0
    );
}

/// Priming a permanent column's param reports ALREADY ROUTED (Task 11
/// review).
#[test]
fn mix_plus_on_a_fixed_param_reports_already_routed() {
    let mut ui = on_flt(EngineType::Algo);
    for enc in [EncoderId::B, EncoderId::C] {
        feed(&mut ui, Input::turn(enc, 1)); // FREQ, Q
        feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
        assert_eq!(
            ui.prime_status(),
            Some(PrimeStatus::AlreadyRouted),
            "{enc:?}"
        );
    }
}

#[test]
fn panels_follow_the_type() {
    use chimera_core::ui::draw::text_width;
    use chimera_core::ui::filter_panel::PanelTarget as T;
    use chimera_core::ui::theme::{CELL_COL_W, FONT_LABEL};
    let labels = |k| {
        (0..5)
            .map(|i| knob(k, i).unwrap().label)
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(FilterKind::Svf), ["FREQ", "Q", "MODE", "FM1", "FM2"]);
    assert_eq!(labels(FilterKind::Dirty), labels(FilterKind::Svf));
    assert_eq!(
        labels(FilterKind::K2040),
        ["CUTOFF", "RES", "ENV AMT", "KEYBD", "DRIVE"]
    );
    let t = |k, i| knob(k, i).map(|n| n.target);
    assert_eq!(
        t(FilterKind::K2040, 4),
        Some(T::Filter(FilterParams::LEVEL))
    );
    assert_eq!(t(FilterKind::K2040, 3), Some(T::Keybd));
    assert_eq!(t(FilterKind::K2040, 2), Some(T::Route(ModSource::Env1)));
    assert_eq!(
        t(FilterKind::K2040, 5),
        Some(T::Filter(FilterParams::VOICE))
    );
    assert_eq!(t(FilterKind::K2040, 10), Some(T::Fixed(FilterParams::MODE)));
    assert_eq!(t(FilterKind::Svf, 3), Some(T::Route(ModSource::Env1)));
    assert_eq!(t(FilterKind::Svf, 4), Some(T::Route(ModSource::Note)));
    assert_eq!(t(FilterKind::Svf, 10), None);
    for k in [FilterKind::Svf, FilterKind::Dirty, FilterKind::K2040] {
        assert!(t(k, 5).is_some(), "{k:?}");
        assert_eq!(t(k, 6), Some(T::JackAmount(JackSlot::Fm3)));
        assert_eq!(t(k, 7), Some(T::Filter(FilterParams::FM3_IN)));
        assert_eq!(t(k, 8), Some(T::JackAmount(JackSlot::Qm)));
        assert_eq!(t(k, 9), Some(T::Filter(FilterParams::QM_IN)));
        for i in 0..11 {
            if let Some(n) = knob(k, i) {
                let w = text_width(&FONT_LABEL, n.label, 0);
                assert!(w <= CELL_COL_W - 4, "{k:?} {i} {:?}: {w}px", n.label);
            }
        }
    }
    assert_eq!(t(FilterKind::Svf, 5), Some(T::Filter(FilterParams::LEVEL)));
    // The 2040's MODE shows LP4, dimmed.
    use chimera_core::ui::view::{self, SlotCtx};
    let mut s = Sound::init(EngineType::Algo);
    s.params.filter.set_kind(FilterKind::K2040);
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    let v = view::view(&FILTER_MOD, 5, &ctx);
    assert_eq!(v.label(), "MODE");
    assert!(view::is_dimmed(&v, &s, &ctx));
    assert_eq!(view::view(&FILTER_MOD, 0, &ctx).label(), "VOICE");
}

#[test]
fn keybd_steps_off_half_full() {
    assert_eq!(keybd_step(0, 1), 64);
    assert_eq!(keybd_step(64, 1), 127);
    assert_eq!(keybd_step(127, 1), 127);
    assert_eq!(keybd_step(100, -1), 64);
    assert_eq!(keybd_step(30, 1), 64);
    assert_eq!(keybd_step(30, -1), 0);
    assert_eq!(keybd_step(0, -1), 0);
    assert_eq!(keybd_step(-40, 1), 0);
    use chimera_core::block::ValFmt;
    use chimera_core::ui::fmt::{FmtBuf, fmt_val};
    use chimera_core::ui::renderer::amount_value;
    let shown = |a| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, amount_value(a), ValFmt::Keybd);
        b.as_str().to_string()
    };
    assert_eq!(
        [shown(0), shown(64), shown(127), shown(100)],
        ["OFF", "HALF", "FULL", "+79%"]
    );
}

/// The 2040's KEYBD knob steps NOTE → FREQ.
#[test]
fn keybd_knob_steps_the_note_route() {
    let mut ui = on_flt(EngineType::Algo);
    feed(&mut ui, Input::turn(EncoderId::A, 1)); // TYPE: 2040
    assert_eq!(ui.params().filter.kind(), FilterKind::K2040);
    let note = |ui: &UiState| {
        ui.mod_state()
            .fixed_amount(ModSource::Note.index(), FixedCol::Freq)
    };
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(note(&ui), keybd_step(0, 1).max(64));
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(note(&ui), 127);
    feed(&mut ui, Input::turn(EncoderId::E, -2));
    assert_eq!(note(&ui), 0);
}

fn saw_bench() -> common::sweep::Bench {
    use chimera_core::dsp::fx_bus::FxParams;
    use chimera_core::part::PartParams;
    use common::sweep::{Bench, Patch};
    let mut b = Bench::new(Patch::Probe, PartParams::default(), FxParams::default());
    b.edit_sound(|s| {
        s.params.algo = chimera_core::dsp::algo::params::AlgoParams::single(WaveId::SAW);
        s.params.filter.cutoff = 900.0;
        s.params.filter.resonance = 0.2;
    });
    b
}

/// Events at `events`: none clicks (`Tape::click`).
fn no_clicks(tape: &common::sweep::Tape, events: &[usize]) {
    for (i, &e) in events.iter().enumerate() {
        let prev = if i == 0 { 0 } else { events[i - 1] };
        let next = events.get(i + 1).copied().unwrap_or(usize::MAX);
        let c = tape.click(0, e, (prev, next), true);
        assert!(c.is_none(), "event {i} at block {e}: click ×{c:?}");
    }
}

#[test]
fn qm_from_a_stepped_lfo_square_does_not_click() {
    use chimera_core::dsp::lfo::LfoShape;
    const START: usize = 100;
    let mut b = saw_bench();
    b.edit_sound(|s| {
        let l = &mut s.params.lfos[0];
        (l.shape, l.rate, l.sync, l.phase_offset) = (LfoShape::Square as u8, 4.0, 1, 0.0);
        assert!(repatch(s, JackSlot::Qm, Jack::Mod(ModSource::Lfo1)));
        set_jack_amount(s, JackSlot::Qm, 127);
    });
    let mut tape = common::sweep::Tape::default();
    for _ in 0..START {
        b.render(&mut tape);
    }
    b.note(45, Some(100));
    // The square flips every 125 ms: 93.75 blocks.
    let events: Vec<usize> = (1..12)
        .map(|k| START + (k as f32 * 93.75) as usize)
        .collect();
    for _ in 0..12 * 94 + 40 {
        b.render(&mut tape);
    }
    no_clicks(&tape, &events);
}

#[test]
fn repatching_mid_note_does_not_click() {
    const START: usize = 120;
    const EVERY: usize = 150; // 200 ms
    let mut b = saw_bench();
    b.edit_sound(|s| {
        set_jack_amount(s, JackSlot::Fm3, 50);
        set_jack_amount(s, JackSlot::Qm, 60);
    });
    b.note(45, Some(100));
    let mut tape = common::sweep::Tape::default();
    let (fm3, qm) = (Jack::FM3_CHOICES, Jack::QM_CHOICES);
    let mut events = Vec::new();
    for blk in 0..START + 12 * EVERY + 60 {
        if blk >= START && (blk - START).is_multiple_of(EVERY) && events.len() < 12 {
            let n = events.len() + 1;
            b.edit_sound(|s| {
                if n % 2 == 1 {
                    assert!(repatch(s, JackSlot::Fm3, fm3[(n / 2 + 1) % 6]));
                } else {
                    assert!(repatch(s, JackSlot::Qm, qm[(n / 2 + 3) % 8]));
                }
            });
            events.push(blk);
        }
        b.render(&mut tape);
    }
    no_clicks(&tape, &events);
}

/// A jack on OP 5 or OP 6 off ALGO acts as its default (L-new-2).
#[test]
fn a_tap_jack_off_algo_is_its_default() {
    use chimera_core::taps::TapOp;
    for (slot, d) in [
        (JackSlot::Fm3, Jack::Mod(ModSource::Lfo1)),
        (JackSlot::Qm, Jack::Mod(ModSource::Env3)),
    ] {
        assert_eq!(slot.default(), d);
        let t = Jack::Tap(TapOp::Op6);
        assert_eq!(t.effective(EngineType::Modal, d), d);
        assert_eq!(t.effective(EngineType::Algo, d), t);
        assert_eq!(
            Jack::Mod(ModSource::Vel).effective(EngineType::Modal, d),
            Jack::Mod(ModSource::Vel)
        );
    }
    let mut s = Sound::init(EngineType::Modal);
    assert!(s.params.filter.set_fm3_src(Jack::Tap(TapOp::Op5)));
    s.mod_state
        .set_fixed(ModSource::Lfo1.index(), FixedCol::Freq, 21);
    assert_eq!(jack_amount(&s, JackSlot::Fm3), 21);
    assert_eq!(TapOp::Op5.op_index(), 4);
    assert_eq!(TapOp::Op6.op_index(), 5);
    assert_eq!((TapOp::Op5.tag(), TapOp::Op6.tag()), ("OP5", "OP6"));
}

/// The jacks are patching, not sound: not in the sweep.
#[test]
fn the_jacks_are_not_swept() {
    let reg = common::sweep::registry(false);
    assert!(!reg.iter().any(|p| p.block == BlockRef::Filter
        && (p.spec.id == FilterParams::FM3_IN || p.spec.id == FilterParams::QM_IN)));
    assert!(
        reg.iter()
            .any(|p| p.block == BlockRef::Filter && p.spec.id == FilterParams::VOICE)
    );
}
