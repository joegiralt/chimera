// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's pad cells (ADR 0265): `BlockRef::Pad` edits the Part's selected
//! pad, and a knob cell takes its label, short and readout from that pad's
//! machine, empty where the machine has no such knob. The pad's sound's
//! cells (ADR 0269): `PadFilter`, `PadAmp`, `PadEnv`, `PadLfo` and
//! `PadRoute` edit the selected pad's, on DRUM's FLT, AMP and MOD pages.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::drum::laws::spec;
use chimera_core::dsp::drum::params::PadParams;
use chimera_core::dsp::drum::types::{ChokeGroup, Kit, Knob, KnobSet, Machine, PadId, Tonal};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::VizType;
use chimera_core::ui::block_def::{BlockDef, ParamSlot};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::page::{PageLayout, ValFmt};
use chimera_core::ui::view::{self, View};
use chimera_midi::value::U7;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The one `KitWriter`, shared by the tests that publish.
fn writer() -> MutexGuard<'static, KitWriter> {
    static W: OnceLock<Mutex<KitWriter>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(KitWriter::take().expect("the kit writer, once")))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).unwrap()
}

/// A page of MCH's six knobs (Task 8 builds the real one).
static KNOBS: BlockDef = BlockDef {
    id: 9_999,
    name: "Knobs",
    short: "KNB",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::knob(Knob::A),
        ParamSlot::knob(Knob::B),
        ParamSlot::knob(Knob::C),
        ParamSlot::knob(Knob::D),
        ParamSlot::knob(Knob::E),
        ParamSlot::knob(Knob::F),
    ],
};

fn knob_addr(k: Knob) -> ParamAddr {
    ParamAddr::new(BlockRef::Pad, PadParams::knob(k))
}

fn machine_addr() -> ParamAddr {
    ParamAddr::new(BlockRef::Pad, PadParams::MACHINE)
}

/// A UI on Part 1 with pad `n` selected.
fn ui_on(n: u8) -> UiState {
    let mut ui = UiState::new();
    ui.select_pad(P1, pad(n));
    ui
}

fn cell(ui: &UiState, k: Knob) -> View {
    view::view(&KNOBS, k as usize, &ui.slot_ctx_for_test())
}

/// Turns MACHINE until pad `n` of Part 1 plays `m`.
fn set_machine(ui: &mut UiState, m: Machine) {
    let i = Machine::ALL.iter().position(|&x| x == m).unwrap() as i8;
    let now = ui.project().kit(P1).pad(ui.selected_pad(P1)).machine();
    let at = Machine::ALL.iter().position(|&x| now == x.into()).unwrap() as i8;
    ui.edit_addr(P1, machine_addr(), i - at, false);
}

#[test]
fn the_snare_s_third_knob_reads_bump() {
    let ui = ui_on(2);
    assert_eq!(ui.selected_pad(P1), pad(2));
    assert_eq!(
        ui.project().kit(P1).pad(pad(2)).machine(),
        Machine::ToneSnare.into()
    );
    let v = cell(&ui, Knob::C);
    assert_eq!(v.label(), "BUMP");
    assert_eq!(v.short(), Some("BUMP"));
    assert_eq!(v.addr(), Some(knob_addr(Knob::C)));
    assert_eq!(v.fmt(), ValFmt::Knob(Machine::ToneSnare, Knob::C));
    assert_eq!(cell(&ui, Knob::E).label(), "SNAP DECAY");
    assert_eq!(cell(&ui, Knob::E).short(), Some("SDEC"));
}

#[test]
fn a_plain_noise_has_no_third_knob_to_turn() {
    let mut ui = ui_on(2);
    set_machine(&mut ui, Machine::PlainNoise);
    let p = *ui.project().kit(P1).pad(pad(2));
    assert_eq!(p.machine(), Machine::PlainNoise.into());
    assert_eq!(cell(&ui, Knob::A).label(), "DECAY");
    for k in [Knob::C, Knob::D, Knob::E, Knob::F] {
        assert_eq!(cell(&ui, k), View::Empty, "{k:?}");
    }
    // Nor through its address: the machine reads no knob C.
    assert!(!ui.edit_addr(P1, knob_addr(Knob::C), 5, false));
    assert_eq!(*ui.project().kit(P1).pad(pad(2)), p);
}

#[test]
fn a_knob_turn_moves_only_the_selected_pad() {
    let mut ui = ui_on(2);
    let before = *ui.project().kit(P1);
    let other = *ui.project().kit(P2);
    assert!(ui.edit_addr(P1, knob_addr(Knob::C), 3, false));
    let after = ui.project().kit(P1);
    for p in PadId::ALL {
        if p == pad(2) {
            let (b, a) = (before.pad(p).knobs(), after.pad(p).knobs());
            assert_eq!(a.get(Knob::C).get().get(), b.get(Knob::C).get().get() + 3);
            for k in Knob::ALL.into_iter().filter(|&k| k != Knob::C) {
                assert_eq!(a.get(k), b.get(k), "{k:?}");
            }
        } else {
            assert_eq!(after.pad(p), before.pad(p), "{p:?}");
        }
    }
    assert_eq!(*ui.project().kit(P2), other);
}

#[test]
fn a_machine_change_resets_that_pad_s_knobs() {
    let mut ui = ui_on(5);
    ui.edit_addr(P1, knob_addr(Knob::A), 9, false);
    set_machine(&mut ui, Machine::MetalHat);
    let p = ui.project().kit(P1).pad(pad(5));
    assert_eq!(p.machine(), Machine::MetalHat.into());
    assert_eq!(*p.knobs(), KnobSet::defaults(Machine::MetalHat));
    // Its other settings stay.
    assert_eq!(p.choke(), Kit::DEFAULT.pad(pad(5)).choke());
}

#[test]
fn level_pan_choke_and_retrig_edit_the_selected_pad() {
    let mut ui = ui_on(4);
    let at = |id| ParamAddr::new(BlockRef::Pad, id);
    ui.edit_addr(P1, at(PadParams::LEVEL), -27, false);
    ui.edit_addr(P1, at(PadParams::PAN), -10, false);
    ui.edit_addr(P1, at(PadParams::CHOKE), 2, false);
    ui.edit_addr(P1, at(PadParams::RETRIG), -1, false);
    let p = ui.project().kit(P1).pad(pad(4));
    assert_eq!(p.level(), U7::new(100).unwrap());
    assert_eq!(p.pan().get(), -10);
    assert_eq!(p.choke(), ChokeGroup::C);
    assert_eq!(
        p.retrig(),
        chimera_core::dsp::tape_engine::params::Retrig::Stack
    );
}

#[test]
fn pad_params_are_the_ui_s_not_a_sound_s() {
    assert!(!BlockRef::Pad.in_sound());
    assert!(!BlockRef::Pad.voice_reads());
    assert!(!BlockRef::ALL.contains(&BlockRef::Pad));
    assert!(!knob_addr(Knob::A).modulatable());
}

#[test]
fn a_knob_reads_in_its_machine_s_unit() {
    let show = |m: Machine, k: Knob, v: u8| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, f32::from(v) / 127.0, ValFmt::Knob(m, k));
        b.as_str().to_owned()
    };
    // The law's value, in its fmt: expo 100..400 Hz at 0 and 127.
    assert_eq!(show(Machine::ToneSnare, Knob::A, 0), "100 Hz");
    assert_eq!(show(Machine::ToneSnare, Knob::A, 127), "400 Hz");
    assert_eq!(show(Machine::ToneSnare, Knob::B, 0), "40 ms");
    assert_eq!(show(Machine::ToneKick, Knob::B, 127), "3.00 s");
    assert_eq!(show(Machine::ToneSnare, Knob::C, 127), "+12.0");
    assert_eq!(show(Machine::ToneClap, Knob::C, 127), "8");
    assert_eq!(show(Machine::ToneKick, Knob::F, 127), "100");
    // A knob the machine lacks reads nothing.
    assert_eq!(show(Machine::PlainNoise, Knob::C, 64), "");
    assert!(spec(Machine::PlainNoise).knob(Knob::C).is_none());
}

/// ADR 0270: the kit's TONAL reaches the audio; the selected pad never
/// decides what a note plays.
#[test]
fn tonal_reaches_the_audio() {
    let mut w = writer();
    let mut ui = ui_on(7);
    ui.select_pad(P2, pad(11));
    ui.project_mut().kit_mut(P2).set_tonal(Tonal::Pad(pad(4)));
    w.publish_all(ui.project().kits());
    let c4 = chimera_core::MidiNote::new(60).unwrap();
    assert_eq!(
        KIT_VIEW.hit(P1, c4).unwrap().pad(),
        pad(1),
        "OFF: note 60 is pad 1"
    );
    assert_eq!(KIT_VIEW.hit(P2, c4).unwrap().pad(), pad(4));
}

/// CHOKE and RETRIG: every choice reads back as its own cell value, writes
/// back to itself, and shows its own name.
#[test]
fn choke_and_retrig_cells_round_trip() {
    use chimera_core::block::{Block, DiskCode};
    use chimera_core::dsp::tape_engine::params::Retrig;
    fn shown(p: &chimera_core::dsp::drum::types::Pad, id: chimera_core::block::ParamId) -> String {
        let s = p.specs().iter().find(|s| s.id == id).unwrap();
        let ValFmt::Names(n) = s.fmt else {
            panic!("{} not names", s.label)
        };
        n[p.get(id) as usize].to_owned()
    }
    let base = *Kit::DEFAULT.pad(pad(1));
    for c in ChokeGroup::ALL {
        let mut p = base;
        p.set_choke(c);
        let mut q = base;
        q.write(PadParams::CHOKE, p.get(PadParams::CHOKE));
        assert_eq!(q.choke(), c);
        assert_eq!(shown(&p, PadParams::CHOKE), c.disk_ident());
    }
    for r in Retrig::ALL {
        let mut p = base;
        p.set_retrig(r);
        let mut q = base;
        q.write(PadParams::RETRIG, p.get(PadParams::RETRIG));
        assert_eq!(q.retrig(), r);
        assert_eq!(shown(&p, PadParams::RETRIG), r.disk_ident());
    }
}

/// A pad's MACHINE cell (design rule: full words when they fit): the full
/// name, else its last word (the instrument), else the machine's short
/// name; never wider than the column, so COWBELL can't run into LEVEL.
#[test]
fn machine_cells_fit_their_column() {
    use chimera_core::ui::renderer::{CELL_VALUE_W, pad_machine_cell};
    use chimera_core::ui::{draw, theme};
    let w = |s: &str| draw::text_width(&theme::FONT_VALUE, s, 0);
    for m in Machine::ALL {
        let s = spec(m);
        let last = s.name.rsplit(' ').next().unwrap();
        let cell = pad_machine_cell(m.into());
        assert!(w(cell) <= CELL_VALUE_W, "{m:?}: {cell} is {} wide", w(cell));
        let want = if w(s.name) <= CELL_VALUE_W {
            s.name
        } else if w(last) <= CELL_VALUE_W {
            last
        } else {
            s.short
        };
        assert_eq!(cell, want, "{m:?}");
    }
    assert_eq!(pad_machine_cell(Machine::ToneCowbell.into()), "BELL");
}

/// Machines with no PITCH play the same hit on every TONAL note: exactly
/// those `transposed` leaves alone.
#[test]
fn pitched_is_what_transposes() {
    use chimera_core::dsp::drum::laws::resolve;
    use chimera_core::dsp::drum::types::Transpose;
    let up = Transpose::new(12).unwrap();
    for m in Machine::ALL {
        let p = resolve(m, &KnobSet::defaults(m));
        assert_eq!(m.pitched(), p.transposed(up) != p, "{m:?}");
    }
    assert!(!Machine::ToneClap.pitched());
    assert!(!Machine::BodyShaker.pitched());
    assert!(!Machine::PlainNoise.pitched());
}

/// TONAL's band says so when its pad's machine has no PITCH (ADR 0270).
#[test]
fn tonal_on_an_unpitched_pad_notes_it() {
    use chimera_core::dsp::drum::params::KitParams;
    let a = ParamAddr::new(BlockRef::Kit, KitParams::TONAL);
    let mut ui = ui_on(1);
    let note = |ui: &UiState| view::tonal_note(a, &ui.slot_ctx_for_test());
    assert!(note(&ui).is_none(), "OFF");
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(1)));
    assert!(note(&ui).is_none(), "KICK has PITCH");
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(4)));
    assert_eq!(note(&ui).unwrap().as_str(), "NO PITCH ON CLAP");
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(12)));
    assert_eq!(note(&ui).unwrap().as_str(), "NO PITCH ON SHAKER");
    let other = ParamAddr::new(BlockRef::Pad, PadParams::MACHINE);
    assert!(view::tonal_note(other, &ui.slot_ctx_for_test()).is_none());
}

// ---------------------------------------------------------------------------
// The pad's sound (ADR 0269)
// ---------------------------------------------------------------------------

use chimera_core::dsp::drum::params::{PadAmpParams, PadRouteParams};
use chimera_core::dsp::drum::sound::{
    AmpMode, EnvDecay, PadSound, RouteDest, RouteId, RouteSource,
};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::{PartFrom, PartSource};
use chimera_core::ui::block_registry::{DRUM_AMP, DRUM_FLT, DRUM_ROUTE, FILTER};
use chimera_core::ui::filter_panel;

/// A page of the pad's FLT panel cells, routes and jacks among them.
static PAD_FLT: BlockDef = BlockDef {
    id: 9_998,
    name: "Pad filter",
    short: "PFL",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::pad_filter_panel(0),
        ParamSlot::pad_filter_panel(1),
        ParamSlot::pad_filter_panel(2),
        ParamSlot::pad_filter_panel(3),
        ParamSlot::pad_filter_panel(10),
        ParamSlot::param(BlockRef::PadFilter, FilterParams::KIND),
    ],
};

/// Part 1 on DRUM INIT with pad `n` selected.
fn drum_on(n: u8) -> UiState {
    let mut ui = UiState::new();
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Drum),
    });
    ui.select_pad(P1, pad(n));
    ui
}

fn sound(ui: &UiState, n: u8) -> PadSound {
    *ui.project().kit(P1).pad(pad(n)).sound()
}

fn at(b: BlockRef, id: chimera_core::block::ParamId) -> ParamAddr {
    ParamAddr::new(b, id)
}

/// Sets pad `n`'s filter KIND to `k` through its cell.
fn set_kind(ui: &mut UiState, n: u8, k: FilterKind) {
    ui.select_pad(P1, pad(n));
    let to = FilterKind::BUILT.iter().position(|&x| x == k).unwrap() as i8;
    ui.edit_addr(P1, at(BlockRef::PadFilter, FilterParams::KIND), -10, false);
    ui.edit_addr(P1, at(BlockRef::PadFilter, FilterParams::KIND), to, false);
    assert_eq!(sound(ui, n).filter().kind(), k);
}

#[test]
fn flt_cutoff_edits_the_selected_pad_only() {
    let mut ui = drum_on(2);
    let flt = ui.project().part(P1).sound.params.filter;
    let (p1, p2) = (sound(&ui, 1), sound(&ui, 2));
    let a = at(BlockRef::PadFilter, FilterParams::CUTOFF);
    assert!(ui.edit_addr(P1, a, -24, false));
    assert_ne!(sound(&ui, 2).filter().cutoff(), p2.filter().cutoff());
    assert_eq!(sound(&ui, 1), p1);
    let now = ui.project().part(P1).sound.params.filter;
    assert_eq!(now.cutoff, flt.cutoff, "the Sound's filter is left");
    // A cutoff a full octave down: 24 ticks of 1/24 octave.
    let ratio = p2.filter().cutoff().get() / sound(&ui, 2).filter().cutoff().get();
    assert!((ratio - 2.0).abs() < 1e-3, "{ratio}");
}

/// DRUM's FLT and AMP pages edit the pad: KIND and the panel cells on
/// FLT, every AMP cell; FX TYPE and FX PLACE stay the Part's.
#[test]
fn drum_flt_and_amp_cells_edit_the_pad() {
    let mut ui = drum_on(2);
    let ctx = ui.slot_ctx_for_test();
    assert!(ctx.drum);
    let sound_before = ui.project().part(P1).sound.params.clone();
    for (def, cells) in [
        (&DRUM_FLT, &[0usize, 2, 3, 5][..]),
        (&DRUM_AMP, &[0, 1, 2, 3, 4, 5][..]),
    ] {
        for &i in cells {
            let v = view::view(def, i, &ctx);
            let a = v.addr().unwrap_or_else(|| panic!("{} {i}", def.short));
            assert!(a.block.is_pad(), "{} {i}: {a:?}", def.short);
            assert_eq!(v.hint(&ctx), view::PAD_HINT, "{} {i}", def.short);
        }
        assert!(def.edits_pad(), "{}", def.short);
    }
    for i in [1, 4] {
        let a = view::view(&DRUM_FLT, i, &ctx).addr().unwrap();
        assert_eq!(a.block, BlockRef::VoiceFx, "FLT {i} is the Part's");
    }
    // FREQ, FLT's F, turns pad 2's CUTOFF.
    let freq = view::view(&DRUM_FLT, 5, &ctx).addr().unwrap();
    assert_eq!(freq, at(BlockRef::PadFilter, FilterParams::CUTOFF));
    assert!(ui.edit_addr(P1, freq, -12, false));
    let vel = view::view(&DRUM_AMP, 4, &ctx).addr().unwrap();
    assert_eq!(vel, at(BlockRef::PadAmp, PadAmpParams::VEL));
    assert!(ui.edit_addr(P1, vel, -10, false));
    let mode = view::view(&DRUM_AMP, 5, &ctx).addr().unwrap();
    assert_eq!(mode, at(BlockRef::PadAmp, PadAmpParams::MODE));
    let after = ui.project().part(P1).sound.params.clone();
    assert_eq!(after.filter.cutoff, sound_before.filter.cutoff);
    assert_eq!(after.out.vca_vel, sound_before.out.vca_vel);
    assert!(sound(&ui, 2).filter().cutoff().get() < sound(&ui, 1).filter().cutoff().get());
    assert!(sound(&ui, 2).amp().vel().get() < 1.0);
}

/// A pad's VEL drives its own amp (ADR 0269): lit on DRUM INIT, which
/// has no Part VCA route.
#[test]
fn pad_vel_is_not_dimmed() {
    let ui = drum_on(1);
    let ctx = ui.slot_ctx_for_test();
    let s = &ui.project().part(P1).sound;
    assert_eq!(s.mod_state.routes_into(chimera_core::modulation::VCA), 0);
    let v = view::view(&DRUM_AMP, 4, &ctx);
    assert_eq!(v.addr(), Some(at(BlockRef::PadAmp, PadAmpParams::VEL)));
    assert!(!view::is_dimmed(&v, s, &ctx));
}

/// FLT › MOD's OUTPUT on DRUM is a SEM pad's MODE, never the Sound's
/// (masked there); off SEM the cell is empty. Touched, it names the pad.
#[test]
fn fmd_output_is_a_sem_pad_s() {
    use chimera_core::ui::block_registry::FILTER_MOD;
    let mut ui = drum_on(2);
    let ctx = ui.slot_ctx_for_test();
    assert_eq!(view::view(&FILTER_MOD, 5, &ctx), View::Empty);
    assert!(!view::edits_pad(&FILTER_MOD, 5, &ctx));
    set_kind(&mut ui, 2, FilterKind::Sem);
    let ctx = ui.slot_ctx_for_test();
    let v = view::view(&FILTER_MOD, 5, &ctx);
    assert_eq!(v.label(), "OUTPUT");
    let a = v.addr().unwrap();
    assert_eq!(a, at(BlockRef::PadFilter, FilterParams::MODE));
    assert_eq!(v.hint(&ctx), view::PAD_HINT);
    assert!(view::edits_pad(&FILTER_MOD, 5, &ctx));
    assert!(
        !view::edits_pad(&FILTER_MOD, 0, &ctx),
        "ENV AMT is the Part's"
    );
    let mode = ui.project().part(P1).sound.params.filter.mode();
    let pad_mode = sound(&ui, 2).filter().mode();
    assert!(ui.edit_addr(P1, a, 1, false));
    assert_ne!(sound(&ui, 2).filter().mode(), pad_mode);
    assert_eq!(ui.project().part(P1).sound.params.filter.mode(), mode);
    // Not a DRUM Part: the Sound's, as ever.
    let mut va = UiState::new();
    va.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Va),
    });
    let ctx = va.slot_ctx_for_test();
    assert_eq!(
        view::view(&FILTER, 5, &ctx).addr(),
        Some(at(BlockRef::Filter, FilterParams::CUTOFF))
    );
    assert!(!view::edits_pad(&FILTER, 5, &ctx));
}

#[test]
fn pad_filter_panel_follows_the_pad() {
    let mut ui = drum_on(1);
    set_kind(&mut ui, 1, FilterKind::Ladder);
    set_kind(&mut ui, 2, FilterKind::Sem);
    for (n, kind) in [(1, FilterKind::Ladder), (2, FilterKind::Sem)] {
        ui.select_pad(P1, pad(n));
        let ctx = ui.slot_ctx_for_test();
        assert_eq!(ctx.pad_kind, kind);
        for k in 0..=2u8 {
            let v = view::view(&PAD_FLT, k as usize, &ctx);
            let want = filter_panel::knob(kind, k).unwrap();
            assert_eq!(v.label(), want.label, "pad {n} cell {k}");
            assert_eq!(v.addr().unwrap().block, BlockRef::PadFilter);
        }
        // A route and a jack: a pad has neither.
        assert_eq!(view::view(&PAD_FLT, 3, &ctx), View::Empty);
        assert_eq!(view::view(&PAD_FLT, 4, &ctx), View::Empty);
    }
    // SEM's MODE cell is its MORPH knob, the pad's.
    let ctx = ui.slot_ctx_for_test();
    assert_eq!(
        view::view(&PAD_FLT, 0, &ctx).addr(),
        Some(at(BlockRef::PadFilter, FilterParams::MORPH))
    );
}

#[test]
fn kind_change_keeps_mode_valid() {
    let mut ui = drum_on(3);
    let a = at(BlockRef::PadFilter, FilterParams::KIND);
    let m = at(BlockRef::PadFilter, FilterParams::MODE);
    let mut seen = vec![sound(&ui, 3).filter().kind()];
    for step in [1i8, 1, 1, 1, -1, -1, -1, -1] {
        // Every MODE the kind has, then the next KIND.
        for _ in 0..8 {
            ui.edit_addr(P1, m, 1, false);
            let f = *sound(&ui, 3).filter();
            assert!(f.kind().modes().contains(&f.mode()), "{f:?}");
        }
        assert!(ui.edit_addr(P1, a, step, false), "KIND moves");
        let f = *sound(&ui, 3).filter();
        assert!(f.kind().modes().contains(&f.mode()), "{f:?}");
        if !seen.contains(&f.kind()) {
            seen.push(f.kind());
        }
        // MODE reads in the kind's own names.
        let ctx = ui.slot_ctx_for_test();
        let v = view::view(&PAD_FLT, 5, &ctx);
        assert_eq!(v.addr(), Some(a));
        let mode = ParamAddr::new(BlockRef::PadFilter, FilterParams::MODE);
        assert_eq!(
            view::shown_fmt(mode, &ctx),
            Some(ValFmt::Names(f.kind().mode_labels()))
        );
    }
    // Every built KIND visited: the KIND turns were never no-ops.
    for k in FilterKind::BUILT {
        assert!(seen.contains(&k), "{k:?} never reached");
    }
}

#[test]
fn route_cells_edit_the_selected_route() {
    let mut ui = drum_on(4);
    let r2 = RouteId::new(1).unwrap();
    // ROUTE turns 1-4.
    assert_eq!(ui.selected_route(), RouteId::ALL[0]);
    ui.select_route(r2);
    let ctx = ui.slot_ctx_for_test();
    assert_eq!(view::view(&DRUM_ROUTE, 0, &ctx), View::SelectRoute);
    assert_eq!(view::view(&DRUM_ROUTE, 0, &ctx).hint(&ctx), view::PAD_HINT);
    assert!(DRUM_ROUTE.edits_pad());
    let shown = chimera_core::ui::part_page::values_under(
        &DRUM_ROUTE,
        &ui.project().part(P1).sound.params,
        &ctx,
    );
    let mut b = FmtBuf::new();
    fmt_val(&mut b, shown[0], view::ROUTE_FMT);
    assert_eq!(b.as_str(), "2", "ROUTE reads 2");
    let src = view::view(&DRUM_ROUTE, 1, &ctx).addr().unwrap();
    assert_eq!(src, at(BlockRef::PadRoute(r2), PadRouteParams::SOURCE));
    let before = *sound(&ui, 4).routes();
    assert!(ui.edit_addr(P1, src, 2, false));
    let after = *sound(&ui, 4).routes();
    assert_eq!(after.get(r2).source(), RouteSource::Vel);
    for r in RouteId::ALL.into_iter().filter(|&r| r != r2) {
        assert_eq!(after.get(r), before.get(r), "{r:?}");
    }
    let dest = view::view(&DRUM_ROUTE, 2, &ctx).addr().unwrap();
    ui.edit_addr(P1, dest, 2, false);
    assert_eq!(sound(&ui, 4).routes().get(r2).dest(), RouteDest::Volume);
    // AMOUNT runs -127..=127.
    let amt = view::view(&DRUM_ROUTE, 3, &ctx).addr().unwrap();
    let spec = amt.spec().unwrap();
    assert_eq!((spec.min, spec.max), (-127.0, 127.0));
    ui.edit_addr(P1, amt, 127, false);
    ui.edit_addr(P1, amt, 127, false);
    assert_eq!(sound(&ui, 4).routes().get(r2).amount().get(), 127);
    for _ in 0..3 {
        ui.edit_addr(P1, amt, -127, false);
    }
    assert_eq!(sound(&ui, 4).routes().get(r2).amount().get(), -127);
    // Route 1 is left; the selection is the UI's.
    assert_eq!(
        sound(&ui, 4).routes().get(RouteId::ALL[0]),
        before.get(RouteId::ALL[0])
    );
}

#[test]
fn amp_decay_reads_inf_at_127() {
    let mut ui = drum_on(5);
    let d = at(BlockRef::PadAmp, PadAmpParams::DECAY);
    let spec = d.spec().unwrap();
    assert_eq!(spec.fmt, ValFmt::Inf);
    let show = |v: f32| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, spec.normalize(v), spec.fmt);
        b.as_str().to_owned()
    };
    assert_eq!(show(127.0), "INF");
    assert_eq!(show(126.0), "126");
    // An open AMP: decay INF, so the pad sounds as in slice 1.
    assert_eq!(sound(&ui, 5).amp().env().decay(), EnvDecay::Infinite);
    ui.edit_addr(P1, d, -1, false);
    assert_eq!(sound(&ui, 5).amp().env().decay().code().get(), 126);
    ui.edit_addr(P1, d, 1, false);
    assert_eq!(sound(&ui, 5).amp().env().decay(), EnvDecay::Infinite);
    // MODE: ONE-SHOT, then GATED.
    let m = at(BlockRef::PadAmp, PadAmpParams::MODE);
    assert_eq!(sound(&ui, 5).amp().mode(), AmpMode::OneShot);
    ui.edit_addr(P1, m, 1, false);
    assert_eq!(sound(&ui, 5).amp().mode(), AmpMode::Gated);
    let ValFmt::Names(n) = m.spec().unwrap().fmt else {
        panic!("MODE names")
    };
    assert_eq!(n, ["1-SHOT", "GATED"]);
}

/// The pad's blocks are the UI's: no Sound holds them, none is
/// modulatable, and no CC learns them (the pad path, as `BlockRef::Pad`).
#[test]
fn pad_sound_blocks_are_the_ui_s() {
    use chimera_core::midi_map::Mappable;
    let blocks = [
        BlockRef::PadFilter,
        BlockRef::PadAmp,
        BlockRef::PadEnv,
        BlockRef::PadLfo,
        BlockRef::PadRoute(RouteId::ALL[3]),
    ];
    for b in blocks {
        assert!(b.is_pad() && !b.in_sound() && !b.voice_reads(), "{b:?}");
        assert!(!BlockRef::ALL.contains(&b));
        for s in b.specs() {
            let a = ParamAddr::new(b, s.id);
            assert!(!a.modulatable(), "{b:?} {}", s.label);
            assert!(Mappable::new(a).is_none(), "{b:?} {}", s.label);
        }
    }
    // A pad's filter speaks the Sound filter's table, but only its
    // `PadFilter::IDS` cells resolve: on the block, and in every view.
    use chimera_core::block::Block;
    use chimera_core::dsp::drum::sound::PadFilter;
    let f = *PadSound::DEFAULT.filter();
    for s in BlockRef::PadFilter.specs() {
        assert_eq!(
            f.spec(s.id).is_some(),
            PadFilter::IDS.contains(&s.id),
            "{}",
            s.label
        );
    }
    let mut ui = drum_on(1);
    for kind in FilterKind::BUILT {
        set_kind(&mut ui, 1, kind);
        let ctx = ui.slot_ctx_for_test();
        for def in [
            &PAD_FLT,
            &FILTER,
            &chimera_core::ui::block_registry::FILTER_MOD,
        ] {
            for i in 0..6 {
                if let Some(a) = view::view(def, i, &ctx).addr()
                    && a.block == BlockRef::PadFilter
                {
                    assert!(PadFilter::IDS.contains(&a.param), "{kind:?} {}", def.short);
                }
            }
        }
    }
}

/// Each cell reads back what it writes, at every step of its range.
#[test]
fn pad_sound_cells_round_trip() {
    use chimera_core::block::Block;
    let ui = drum_on(1);
    let mut s = sound(&ui, 1);
    let r = RouteId::ALL[2];
    let check = |b: &mut dyn Block, name: &str| {
        for spec in b.specs() {
            let mut v = spec.min;
            while v <= spec.max {
                b.set(spec.id, v);
                assert!(
                    (b.get(spec.id) - v).abs() < 1e-6,
                    "{name} {} {v}",
                    spec.label
                );
                v += spec.step.max(1.0 / 127.0);
            }
        }
    };
    check(s.amp_mut(), "amp");
    check(s.env_mut(), "env");
    check(s.lfo_mut(), "lfo");
    check(s.routes_mut().route_mut(r), "route");
    // The filter, on every KIND: each cell at each of its steps (MODE's
    // range is the kind's), CUTOFF by its 1/24-octave ticks.
    use chimera_core::dsp::drum::sound::PadFilter;
    let f = s.filter_mut();
    for (i, kind) in FilterKind::BUILT.into_iter().enumerate() {
        f.set(FilterParams::KIND, i as f32);
        assert_eq!(f.kind(), kind);
        assert_eq!(f.get(FilterParams::KIND), i as f32);
        for id in PadFilter::IDS
            .into_iter()
            .filter(|&id| id != FilterParams::KIND)
        {
            let spec = *f.spec(id).unwrap();
            let mut v = spec.min;
            loop {
                f.set(id, v);
                let got = f.get(id);
                assert!(
                    (got - v).abs() <= 1e-4 * v.abs().max(1.0),
                    "{kind:?} {} {v}: {got}",
                    spec.label
                );
                let next = spec.nudged(v, 1);
                if next <= v {
                    break;
                }
                v = next;
            }
            assert_eq!(v, spec.max, "{kind:?} {} reaches its max", spec.label);
        }
    }
}

/// An edit reaches the audio's kit: the next hit plays it.
#[test]
fn a_pad_sound_edit_reaches_the_audio() {
    let mut w = writer();
    let mut ui = drum_on(1);
    let c4 = chimera_core::MidiNote::new(60).unwrap();
    ui.edit_addr(
        P1,
        at(BlockRef::PadFilter, FilterParams::CUTOFF),
        -48,
        false,
    );
    ui.edit_addr(P1, at(BlockRef::PadAmp, PadAmpParams::MODE), 1, false);
    w.publish_all(ui.project().kits());
    let hit = KIT_VIEW.hit(P1, c4).unwrap();
    assert_eq!(hit.pad(), pad(1));
    assert_eq!(*hit.params().sound(), sound(&ui, 1));
    assert_eq!(hit.params().sound().amp().mode(), AmpMode::Gated);
}

/// Every new name fits its cell (full words where they fit).
#[test]
fn pad_sound_names_fit_their_cells() {
    use chimera_core::ui::renderer::CELL_VALUE_W;
    use chimera_core::ui::{draw, theme};
    for b in [
        BlockRef::PadAmp,
        BlockRef::PadLfo,
        BlockRef::PadRoute(RouteId::ALL[0]),
    ] {
        for s in b.specs() {
            if let ValFmt::Names(n) = s.fmt {
                for name in n {
                    let w = draw::text_width(&theme::FONT_VALUE, name, 0);
                    assert!(w <= CELL_VALUE_W, "{b:?} {}: {name} is {w}", s.label);
                }
            }
        }
    }
}
