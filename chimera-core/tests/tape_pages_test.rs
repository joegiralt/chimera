//! TAPE's pages (spec § 3, ADR 0093, TAPE Task 15, ADR 0178): the chain
//! SRC · DECK · DRV · FLT · AMP · MOD opening on DECK, DECK holding MODEL
//! and every knob that moves the machine's glyph, the model page the rest
//! and TAPE, AGE and WOW, and `tape_engine::reads` dimming what a machine
//! ignores. Precedent: the
//! Digitakt II's machines, each its own labelled page.

mod common;
mod screen;

use core::mem::MaybeUninit;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{Block, ParamId, ParamKind};
use chimera_core::dsp::tape_engine::machine::{MACHINES, machine};
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    MODEL_NAMES, PlayMode, TAPE_ENGINE_SPECS, TapeEngine, TapeEngineParams, TapeMachine, TapeModel,
    reads,
};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{SourceRef, TapeSource};
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry::{
    ALL_CHAINS, PITCH, TAPE_CHAIN, TAPE_DECK, TAPE_MODEL, TAPE_SRC,
};
use chimera_core::ui::dungeon_map::page_label;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::mod_grid::{block_name, block_tag};
use chimera_core::ui::view::{self, Models, SlotCtx, SourceState, View};
use chimera_core::ui::{draw, nav, theme};
use chimera_hal::{BLOCK_SIZE, EncoderId};
use common::tape::{held_plain, held_transwave, saw_with_burst, sine};
use screen::*;

type P = TapeEngineParams;
const SR: u32 = 48_000;

fn ctx_on(m: TapeMachine) -> SlotCtx {
    SlotCtx::read(&sound_on(m).params, chimera_core::addr::Op::A)
}

fn sound_on(m: TapeMachine) -> Sound {
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape.set_machine(m);
    s
}

fn labels(def: &BlockDef, ctx: &SlotCtx) -> Vec<&'static str> {
    (0..6).map(|i| view::view(def, i, ctx).label()).collect()
}

fn tape(id: ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::TapeEngine, id)
}

/// A UI on Part 1's TAPE init, at DECK.
fn ui_at_deck() -> chimera_core::ui::UiState {
    let mut ui = chimera_core::ui::UiState::new();
    load_init(&mut ui, EngineType::Tape);
    plus(&mut ui, 1); // SRC · DECK
    assert_eq!(page_def_id(&ui), TAPE_DECK.id);
    ui
}

#[test]
fn chain_opens_on_deck() {
    let shorts: Vec<_> = TAPE_CHAIN
        .blocks
        .iter()
        .map(|b| b.map.unwrap_or(b.def.short))
        .collect();
    assert_eq!(shorts, ["SRC", "DECK", "FLT", "AMP", "MOD"]);
    assert_eq!(TAPE_CHAIN.home().node(), 1);
    assert_eq!(TAPE_CHAIN.home().sub(), 0);
    assert!(core::ptr::eq(
        nav::chain_def_for(EngineType::Tape),
        &TAPE_CHAIN
    ));
    let subs: Vec<u16> = TAPE_CHAIN.blocks[1]
        .sub_pages
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(subs, [TAPE_MODEL.id, PITCH.id]);
    assert_eq!((TAPE_SRC.id, TAPE_DECK.id, TAPE_MODEL.id), (93, 94, 95));
    // A load lands there.
    let mut ui = chimera_core::ui::UiState::new();
    load_init(&mut ui, EngineType::Tape);
    plus(&mut ui, 1);
    assert_eq!(page_def_id(&ui), TAPE_DECK.id);
}

/// DECK: MODEL, then every knob that moves the machine's glyph, none
/// dimmed (ADR 0178).
#[test]
fn deck_holds_the_glyphs_knobs() {
    use TapeMachine as M;
    let want: [(M, [&str; 5]); 7] = [
        (M::One, ["START", "END", "--", "--", "--"]),
        (M::Loop, ["POSITION", "WALK", "LENGTH", "XFADE", "--"]),
        (M::Ping, ["POSITION", "WALK", "LENGTH", "--", "--"]),
        (M::Tron, ["LENGTH", "FADE", "--", "--", "--"]),
        (M::Scrub, ["FINGER", "SPEED", "LENGTH", "XFADE", "DRAG"]),
        (M::Stretch, ["SPEED", "D-TIME", "--", "--", "--"]),
        (M::Wave, ["FRAME", "SCAN", "RANGE", "--", "--"]),
    ];
    for (m, cells) in want {
        let ctx = ctx_on(m);
        let got = labels(&TAPE_DECK, &ctx);
        assert_eq!(got[0], "MODEL");
        assert_eq!(got[1..], cells, "{m:?}");
        for (k, c) in machine(m).deck.iter().enumerate() {
            assert_eq!(view::view(&TAPE_DECK, k + 1, &ctx).addr(), Some(tape(c.id)));
            assert!(!view::dimmed(tape(c.id), &sound_on(m)), "{m:?} {}", c.label);
        }
    }
}

/// The model page: TAPE, AGE and WOW in the same cells on every machine,
/// then what else the machine reads.
#[test]
fn model_page_cells() {
    use TapeMachine as M;
    let want: [(M, [&str; 3]); 7] = [
        (M::One, ["FADE", "--", "--"]),
        (M::Loop, ["FADE", "--", "--"]),
        (M::Ping, ["FADE", "--", "--"]),
        (M::Tron, ["--", "--", "--"]),
        (M::Scrub, ["--", "--", "--"]),
        (M::Stretch, ["POSITION", "TYPE", "--"]),
        (M::Wave, ["--", "--", "--"]),
    ];
    let pages = ["ONE", "LOOP", "PING", "TRON", "SCRUB", "STRCH", "WAVE"];
    for ((m, cells), page) in want.into_iter().zip(pages) {
        let mut ctx = ctx_on(m);
        ctx.unpitched = false;
        let got = labels(&TAPE_MODEL, &ctx);
        assert_eq!(got[..3], ["TAPE", "AGE", "WOW"], "{m:?}");
        assert_eq!(got[3..], cells, "{m:?}");
        for (k, c) in machine(m).cells.iter().enumerate() {
            assert_eq!(
                view::view(&TAPE_MODEL, k + 3, &ctx).addr(),
                Some(tape(c.id))
            );
            assert!(!view::dimmed(tape(c.id), &sound_on(m)), "{m:?} {}", c.label);
        }
        for (k, id) in [(0, P::TAPE), (1, P::AGE), (2, P::WOW)] {
            assert_eq!(view::view(&TAPE_MODEL, k, &ctx).addr(), Some(tape(id)));
            assert!(!view::dimmed(tape(id), &sound_on(m)), "{m:?} {id:?}");
        }
        let models = Models {
            tape: m,
            ..Models::default()
        };
        assert_eq!(page_label(&TAPE_MODEL, models), page, "{m:?}");
        assert_eq!(page_label(&TAPE_DECK, models), "DECK");
    }
}

/// Every param that moves a machine's glyph has its cell on DECK, and
/// every DECK cell moves it: the glyph reads what DECK holds.
#[test]
fn deck_is_what_moves_the_glyph() {
    use chimera_core::ui::glyph::TAPE_GLYPH_PARAMS;
    use chimera_core::ui::tape_glyph::TapeGauge;
    for m in TapeMachine::ALL {
        let mut set = [0.0, 0.1, 0.9, 0.3, 0.6, 0.5, 0.3, 0.0, 0.0, 0.2];
        set[0] = m.index() as f32 / 6.0;
        let base = TapeGauge::from_set(set, false);
        assert_eq!(base.kind(), m);
        for (i, a) in TAPE_GLYPH_PARAMS.iter().enumerate().skip(1) {
            if matches!(a.param, P::TAPE | P::AGE) {
                continue;
            }
            let moves = [0.0, 1.0].into_iter().any(|v| {
                let mut moved = set;
                moved[i] = v;
                TapeGauge::from_set(moved, false).key() != base.key()
            });
            let on_deck = machine(m).deck.iter().any(|c| c.id == a.param);
            assert_eq!(moves, on_deck, "{m:?} {:?}", a.param);
        }
    }
}

/// MODEL reads the machine's page name, which fits a cell's bar (STRETCH
/// doesn't); the header spells it out.
#[test]
fn model_names_fit_the_cell() {
    assert_eq!(
        MODEL_NAMES,
        ["ONE", "LOOP", "PING", "TRON", "SCRUB", "STRCH", "WAVE"]
    );
    for (k, name) in MACHINES.iter().zip(MODEL_NAMES) {
        assert_eq!(k.page, name);
        let w = draw::text_width(&theme::FONT_VALUE, name, 0);
        assert!(w <= theme::CELL_BAR_W, "{name}: {w} px");
    }
    let full = [
        "ONE",
        "LOOP",
        "PING",
        "TRON",
        "SCRUB",
        "STRETCH",
        "TRANSWAVE",
    ];
    for (m, want) in TapeMachine::ALL.into_iter().zip(full) {
        let models = Models {
            tape: m,
            ..Models::default()
        };
        let h = chimera_core::ui::components::header_text(
            chimera_core::ui::components::Head::Sound(chimera_core::project::PartId::ALL[0]),
            &TAPE_MODEL,
            models,
            chimera_core::dsp::filter::FilterKind::Svf,
            "",
            chimera_core::part::DacPair::P1,
            None,
        );
        assert_eq!(h.name.as_str(), want);
    }
}

#[test]
fn model_change_applies_machine_defaults() {
    let mut ui = ui_at_deck();
    {
        let t = &mut ui.params_mut().tape;
        (t.start, t.end, t.tape, t.age) = (0.2, 0.7, 0.4, 0.3);
        t.wow = chimera_core::dsp::tape_engine::Wow::Warble;
        (t.motion, t.size) = (-0.4, 0.9);
    }
    let r = SourceRef {
        crc: 7,
        frames: 100,
        rate: 48_000,
    };
    ui.project_mut()
        .edit_part(chimera_core::project::PartId::ALL[0])
        .sound
        .source = Some(TapeSource { r, tune: None });
    settle(&mut ui);
    let settled = render_ui(&ui).hash();
    feed(&mut ui, Input::turn(EncoderId::A, 4)); // LOOP → STRETCH
    let t = ui.params().tape;
    assert_eq!(t.model, TapeModel::Stretch);
    assert_eq!((t.motion, t.size, t.kind as u8), (0.5, 0.5, 0));
    assert_eq!((t.start, t.end, t.tape, t.age), (0.2, 0.7, 0.4, 0.3));
    assert_eq!(t.wow, chimera_core::dsp::tape_engine::Wow::Warble);
    assert_eq!(
        ui.project()
            .part(chimera_core::project::PartId::ALL[0])
            .sound
            .source
            .map(|s| s.r),
        Some(r)
    );
    // Eased: the screen right after the turn is on its way, not there.
    let now = render_ui(&ui).hash();
    settle(&mut ui);
    assert_ne!(now, render_ui(&ui).hash(), "the change snapped");
    assert_ne!(settled, render_ui(&ui).hash());

    // The matrix reads the new machine's labels.
    assert_eq!(ui.matrix_state.ctx().tape, TapeMachine::Stretch);

    // Onto ONE: PLAY in MODE ONE, its FADE holding; LOOP's cells keep
    // their values, which ONE doesn't read.
    feed(&mut ui, Input::turn(EncoderId::A, -5)); // ONE
    let t = ui.params().tape;
    assert_eq!((t.model, t.mode), (TapeModel::Play, PlayMode::One));
    assert_eq!(t.drag, chimera_core::dsp::tape_engine::FADE_HOLD);
    assert_eq!((t.start, t.end), (0.2, 0.7), "the window is the source's");

    // MODEL turned past its end changes nothing, so writes nothing.
    ui.params_mut().tape.slice = chimera_core::dsp::tape_engine::SliceCode::new(5);
    ui.params_mut().tape.drag = 0.5;
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    assert_eq!(ui.params().tape.mode, PlayMode::One);
    assert_eq!(ui.params().tape.slice.code(), 5);
    assert_eq!(ui.params().tape.drag, 0.5);

    // A Sound loaded on a machine keeps what it stored.
    {
        let t = &mut ui.params_mut().tape;
        t.model = TapeModel::Scrub;
        t.motion = 0.2;
    }
    feed(&mut ui, Input::default());
    settle(&mut ui);
    assert_eq!(ui.params().tape.motion, 0.2);
}

#[test]
fn scrub_finger_jogs() {
    let mut ui = ui_at_deck();
    feed(&mut ui, Input::turn(EncoderId::A, 3)); // LOOP → SCRUB
    assert_eq!(ui.params().tape.model, TapeModel::Scrub);
    let drag = ui.params().tape.drag;
    assert_eq!(ui.params().tape.position, 0.0);
    // 8 detents in 125 ms.
    for k in 0..8u32 {
        feed(&mut ui, Input::turn(EncoderId::B, 1).at(1_000 + k * 16));
    }
    let turned = ui.params().tape.position;
    assert!(turned > 0.0, "the finger moved nothing");
    // The last turn's pass ran at 0.5 of the window a second.
    let mut last = turned;
    let mut prev_step = f32::MAX;
    for k in 1..=200u32 {
        feed(&mut ui, Input::default().at(1_112 + k * 16));
        let p = ui.params().tape.position;
        let step = p - last;
        assert!(
            (0.0..=0.5 * 0.016 + 1e-6).contains(&step),
            "pass {k}: {step}"
        );
        assert!(step <= prev_step + 1e-7, "pass {k}: the glide sped up");
        (last, prev_step) = (p, step);
    }
    // A stop: the glide's whole travel is the speed × τ.
    let tau = chimera_core::dsp::tape_engine::drag_s(drag);
    assert!(prev_step < 1e-6, "still moving: {prev_step}");
    assert!(
        (last - turned - 0.5 * tau).abs() < 0.01,
        "{last} {turned} {tau}"
    );

    // The reel stops at the tape's end.
    ui.params_mut().tape.position = 0.99;
    for k in 0..8u32 {
        feed(&mut ui, Input::turn(EncoderId::B, 8).at(5_000 + k * 16));
    }
    for k in 1..=30u32 {
        feed(&mut ui, Input::default().at(5_112 + k * 16));
    }
    assert_eq!(ui.params().tape.position, 1.0);

    // LOOP's POSITION steps 1/128 a detent.
    let mut ui = ui_at_deck();
    feed(&mut ui, Input::turn(EncoderId::B, 1).at(1_000));
    feed(&mut ui, Input::default().at(1_016));
    assert_eq!(ui.params().tape.position, 1.0 / 128.0);
}

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    unsafe { slot.assume_init() }
}

/// A param `reads` says a machine ignores, moved: the render is
/// bit-identical. Every path in play: TAPE on, a moving POSITION, a
/// note-off before the end.
#[test]
fn reads_matches_the_audio() {
    let held = held_plain(&saw_with_burst(48_000, 1.0), 48_000);
    let wave = held_transwave(512, 4, |f, h| if h <= 1 + f as u32 { 0.5 } else { 0.0 });
    let mut checked = 0;
    for m in TapeMachine::ALL {
        let src = if m == TapeMachine::Wave { &wave } else { &held };
        let v = SourceView::strict(src).unwrap();
        let base = P {
            model: m.model(),
            mode: m.mode().unwrap_or(PlayMode::Loop),
            start: 0.1,
            end: 0.9,
            position: 0.2,
            motion: 0.3,
            tape: 0.6,
            age: 0.3,
            size: 0.6,
            xfade: 0.2,
            drag: 0.4,
            period: 109.1,
            slice: chimera_core::dsp::tape_engine::SliceCode::OFF,
            ..P::default()
        };
        let render = |p: P| {
            let mut e = fresh();
            e.seed(1);
            e.note_on(60, &p, &v, SR);
            let mut out = Vec::new();
            let mut b = [0.0; BLOCK_SIZE];
            for n in 0..300 {
                let q = P {
                    position: if n < 150 { p.position } else { 0.7 },
                    ..p
                };
                e.render(&mut b, &q, &v, SR);
                out.extend_from_slice(&b);
                if n == 200 {
                    e.note_off();
                }
            }
            out
        };
        let plain = render(base);
        assert!(plain.iter().any(|&y| y != 0.0), "{m:?} is silent");
        for spec in &TAPE_ENGINE_SPECS {
            // SLICE is retired: a loaded Sound's only, ONE still plays it.
            if reads(m, spec.id) || spec.id == P::SLICE {
                continue;
            }
            let mut p = base;
            let moved = match spec.kind {
                ParamKind::Enum => (base.get(spec.id) + 2.0).min(spec.max),
                _ => spec.min + 0.83 * (spec.max - spec.min),
            };
            p.set(spec.id, moved);
            assert_ne!(p.get(spec.id), base.get(spec.id), "{m:?} {}", spec.label);
            assert!(
                render(p) == plain,
                "{m:?}: {} isn't read, yet moving it changes the render",
                spec.label
            );
            checked += 1;
        }
    }
    assert!(checked >= 18, "{checked}");
}

#[test]
fn reads_is_the_machine_table() {
    for k in &MACHINES {
        let cells: Vec<ParamId> = k.deck.iter().chain(k.cells).map(|c| c.id).collect();
        for id in [
            P::POSITION,
            P::MOTION,
            P::SIZE,
            P::XFADE,
            P::DRAG,
            P::TYPE,
            P::SLICE,
        ] {
            assert_eq!(
                reads(k.machine, id),
                cells.contains(&id),
                "{:?} {id:?}",
                k.machine
            );
        }
        for id in [P::MODEL, P::MODE, P::START, P::END, P::TAPE, P::AGE, P::WOW] {
            assert!(reads(k.machine, id), "{:?} {id:?}", k.machine);
        }
    }
}

#[test]
fn type_reads_poly_unpitched() {
    let mut s = sound_on(TapeMachine::Stretch);
    s.params.tape.period = 0.0;
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    assert_eq!(
        view::view(&TAPE_MODEL, 4, &ctx),
        View::Text {
            label: "TYPE",
            text: "POLY (UNPITCHED)"
        }
    );
    s.params.tape.period = 109.1;
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    assert_eq!(view::view(&TAPE_MODEL, 4, &ctx).addr(), Some(tape(P::TYPE)));
}

#[test]
fn src_reads_states() {
    let text = |s: SourceState| {
        let (mut b, mut c) = (FmtBuf::new(), FmtBuf::new());
        s.text(&mut b);
        s.cell(&mut c);
        // The cell's short form fits its column.
        assert!(draw::text_width(&theme::FONT_VALUE, c.as_str(), 0) <= theme::CELL_COL_W - 4);
        format!("{} / {}", b.as_str(), c.as_str())
    };
    let mut ui = chimera_core::ui::UiState::new();
    let held = held_plain(&sine(24_000, 100.7, 36_000), 24_000);
    let r = SourceRef {
        crc: 0x5EED,
        frames: 36_000,
        rate: 24_000,
    };
    ui.project_mut().source_store(r, &held).unwrap();
    let pool = ui.project().pool();
    let state = |model, r| {
        let p = TapeEngineParams {
            model,
            ..TapeEngineParams::default()
        };
        SourceState::of(&p, r, pool)
    };
    assert_eq!(text(state(TapeModel::Play, None)), "NONE / NONE");
    assert_eq!(
        text(state(TapeModel::Play, Some(r))),
        "24K · 1.5 S · 53K / 1.5 S"
    );
    assert_eq!(
        text(state(TapeModel::Transwave, Some(r))),
        "NOT A TRANSWAVE / NO WAVE"
    );
    let gone = SourceRef { crc: 1, ..r };
    assert_eq!(
        text(state(TapeModel::Play, Some(gone))),
        "SOURCE MISSING / MISSING"
    );

    // On SRC, cell A.
    let mut ui = chimera_core::ui::UiState::new();
    load_init(&mut ui, EngineType::Tape);
    assert_eq!(page_def_id(&ui), TAPE_SRC.id);
    let labels: Vec<_> = (0..6)
        .map(|i| {
            view::view(
                &TAPE_SRC,
                i,
                &SlotCtx::read(ui.params(), chimera_core::addr::Op::A),
            )
            .label()
        })
        .collect();
    assert_eq!(labels, ["SOURCE", "START", "END", "VOL", "--", "--"]);
}

#[test]
fn no_four_tapes() {
    for chain in ALL_CHAINS {
        for b in chain.blocks {
            for d in core::iter::once(b.def).chain(b.sub_pages.iter().copied()) {
                assert_ne!(d.short, "TAPE", "{}", d.name);
                assert_ne!(upper(d.name), "TAPE", "{}", d.name);
            }
        }
    }
    for b in BlockRef::ALL {
        if b == BlockRef::TapeEngine {
            assert_eq!(block_name(b), "TAPE");
            continue;
        }
        assert_ne!(block_tag(b), "TAPE", "{b:?}");
        assert_ne!(block_name(b), "TAPE", "{b:?}");
        for s in b.specs() {
            assert!(s.label != "TAPE" && s.short != Some("TAPE"), "{b:?}");
        }
    }
    assert_eq!(block_tag(BlockRef::Tape), "REL");
    assert_eq!(block_name(BlockRef::Tape), "REEL");
    for k in &MACHINES {
        for c in k.deck.iter().chain(k.cells) {
            assert!(c.label != "TAPE" && c.short != "TAPE");
        }
    }
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

#[test]
fn tape_pages_have_no_other_gestures() {
    for m in TapeMachine::ALL {
        let ctx = ctx_on(m);
        for def in [&TAPE_SRC, &TAPE_DECK, &TAPE_MODEL] {
            for i in 0..6 {
                let v = view::view(def, i, &ctx);
                // MIX + SEQ's learn, every mappable cell's, and no other.
                let h = v.hint(&ctx);
                assert!(
                    h.iter().all(|g| *g == view::LEARN),
                    "{m:?} {} {i}",
                    def.short
                );
            }
        }
    }
}

/// A TAPE column reads the machine's label and short; a machine change
/// bumps the matrix's revision, so the headers redraw.
#[test]
fn matrix_columns_read_the_machine() {
    use chimera_core::ui::mod_grid::{DestCtx, MatrixState, ModDest, draw_grid, fmt_route_dest};
    let dest = ModDest {
        addr: tape(P::MOTION),
        label: [0; 8],
    };
    let name = |tape| {
        let mut b = FmtBuf::new();
        let c = DestCtx {
            tape,
            ..DestCtx::DEFAULT
        };
        fmt_route_dest(&mut b, &dest, c);
        b.as_str().to_owned()
    };
    assert_eq!(name(TapeMachine::Loop), "TPE WALK");
    assert_eq!(name(TapeMachine::Stretch), "TPE SPEED");
    assert_eq!(name(TapeMachine::Tron), "TPE MOTION");
    let mut m = MatrixState::new();
    m.dests[0] = Some(dest);
    m.num_dests = 1;
    let grid = |m: &MatrixState| {
        let mut fb = Fb::new();
        draw_grid(&mut fb, m, 0, 0);
        fb.hash()
    };
    let walk = grid(&m);
    let rev = m.rev;
    m.set_tape(TapeMachine::Stretch);
    assert_ne!(m.rev, rev);
    assert_ne!(grid(&m), walk, "the header still reads WLK");
    m.set_tape(TapeMachine::Loop);
    assert_eq!(grid(&m), walk);
}

/// What a machine ignores is dimmed where it still shows (the matrix's
/// columns): ONE plays one head to END, PING has no crossfade. No TAPE
/// page shows such a cell (`deck_holds_the_glyphs_knobs`).
#[test]
fn cells_a_machine_ignores_are_dimmed() {
    let dim = |m, id| view::dimmed(tape(id), &sound_on(m));
    for id in [P::POSITION, P::MOTION, P::SIZE, P::XFADE] {
        assert!(dim(TapeMachine::One, id), "ONE {id:?}");
    }
    for id in [P::MODEL, P::DRAG, P::WOW, P::TAPE, P::START, P::END] {
        assert!(!dim(TapeMachine::One, id), "ONE {id:?}");
    }
    for id in [P::POSITION, P::MOTION, P::SIZE, P::DRAG] {
        assert!(
            !dim(TapeMachine::Loop, id) && !dim(TapeMachine::Ping, id),
            "{id:?}"
        );
    }
    assert!(!dim(TapeMachine::Loop, P::XFADE));
    assert!(dim(TapeMachine::Ping, P::XFADE));
}

/// ONE's FADE ends in HOLD: elsewhere the top step is a number, as
/// TRON's and SCRUB's are.
#[test]
fn fade_reads_hold_only_in_one() {
    use chimera_core::ui::fmt::fmt_val;
    let text = |m| {
        let ctx = ctx_on(m);
        let fmt = view::shown_fmt(tape(P::DRAG), &ctx).unwrap();
        let mut b = FmtBuf::new();
        fmt_val(&mut b, 1.0, fmt);
        b.as_str().to_owned()
    };
    assert_eq!(text(TapeMachine::One), "HOLD");
    assert_eq!(text(TapeMachine::Loop), "127");
    assert_eq!(text(TapeMachine::Ping), "127");
    let mut s = sound_on(TapeMachine::Tron);
    s.params.tape.mode = PlayMode::One;
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    let mut b = FmtBuf::new();
    fmt_val(&mut b, 1.0, view::shown_fmt(tape(P::DRAG), &ctx).unwrap());
    assert_eq!(b.as_str(), "127", "TRON over a stale MODE ONE");
}

/// MODEL onto ONE, LOOP or PING writes PLAY and its MODE, and the
/// machine's defaults: ONE's FADE holds, the others 32 ms. The disk keeps
/// PLAY's code and MODE's (ADR 0178).
#[test]
fn model_edit_writes_play_and_its_mode() {
    use chimera_core::dsp::tape_engine::FADE_HOLD;
    use chimera_core::ui::part_page::{Edit, edit};
    let mut s = Sound::init(EngineType::Tape);
    assert_eq!(s.params.tape.machine(), TapeMachine::Loop);
    s.params.tape.drag = 0.7;
    let model = tape(P::MODEL);
    assert!(edit(&mut s.params, model, Edit::To(0.0)));
    let t = s.params.tape;
    assert_eq!(
        (t.model, t.mode, t.drag),
        (TapeModel::Play, PlayMode::One, FADE_HOLD)
    );
    assert!(edit(&mut s.params, model, Edit::To(2.0)));
    let t = s.params.tape;
    assert_eq!(
        (t.model, t.mode, t.drag),
        (TapeModel::Play, PlayMode::Ping, 0.3)
    );
    // Off PLAY, MODE stays as it was: no other machine reads it.
    assert!(edit(&mut s.params, model, Edit::To(3.0)));
    let t = s.params.tape;
    assert_eq!((t.model, t.mode), (TapeModel::Tron, PlayMode::Ping));
    assert_eq!(t.machine(), TapeMachine::Tron);
    use chimera_core::block::DiskCode;
    assert_eq!(t.enum_code(P::MODEL), Some(TapeModel::Tron.disk_code()));
    let mut one = Sound::init(EngineType::Tape);
    one.params.tape.set_machine(TapeMachine::One);
    assert_eq!(
        (
            one.params.tape.enum_code(P::MODEL),
            one.params.tape.enum_code(P::MODE)
        ),
        (Some(0), Some(0)),
        "PLAY's code and MODE ONE's"
    );
}

/// A streamed source says why a note on it won't play (slice 1 § 2.3), in
/// the focus band as every other refusal does: streams read forward only,
/// and the machines that seek need RAM.
#[test]
fn streamed_refusals_reach_the_band() {
    use chimera_core::stream::ring::{Rings, ThirdSlots};
    use chimera_core::stream::test_card::TestCard;
    use chimera_core::ui::source_cell::{PING_NEEDS_RAM, REVERSE_NEEDS_RAM};
    let text = |s: SourceState| {
        let (mut b, mut c) = (FmtBuf::new(), FmtBuf::new());
        s.text(&mut b);
        s.cell(&mut c);
        assert!(draw::text_width(&theme::FONT_VALUE, c.as_str(), 0) <= theme::CELL_COL_W - 4);
        format!("{} / {}", b.as_str(), c.as_str())
    };
    let whole = held_plain(&sine(24_000, 100.7, 720_000), 24_000);
    let rings: &'static Rings = Box::leak(Box::new(Rings::new()));
    let third: &'static ThirdSlots = Box::leak(Box::new(ThirdSlots::new()));
    let (_, card) = rings.sides(third).unwrap();
    let held = TestCard::new(card, &whole, 0).held(&[0, 0], 4_800);
    let r = SourceRef {
        crc: 0x5EED,
        frames: 720_000,
        rate: 24_000,
    };
    let mut ui = chimera_core::ui::UiState::new();
    ui.project_mut().source_store(r, &held).unwrap();
    let pool = ui.project().pool();
    let state = |model, mode, start, end| {
        let p = TapeEngineParams {
            model,
            mode,
            start,
            end,
            ..TapeEngineParams::default()
        };
        text(SourceState::of(&p, Some(r), pool))
    };
    use PlayMode::{Loop, One, Ping};
    assert!(state(TapeModel::Play, Loop, 0.0, 1.0).starts_with("24K · 30.0 S"));
    assert!(state(TapeModel::Play, One, 0.0, 1.0).starts_with("24K · 30.0 S"));
    let ping = state(TapeModel::Play, Ping, 0.0, 1.0);
    assert!(ping.starts_with(&format!("{PING_NEEDS_RAM} / ")), "{ping}");
    for mode in [One, Loop] {
        let rev = state(TapeModel::Play, mode, 0.9, 0.1);
        assert!(rev.starts_with(&format!("{REVERSE_NEEDS_RAM} / ")), "{rev}");
    }
    for (m, why) in [
        (TapeModel::Scrub, "SCRUB NEEDS RAM"),
        (TapeModel::Stretch, "STRETCH NEEDS RAM"),
        (TapeModel::Transwave, "WAVE NEEDS RAM"),
    ] {
        let s = state(m, Loop, 0.0, 1.0);
        assert!(s.starts_with(&format!("{why} / ")), "{s}");
    }
    // TRON streams, its note ONE's (#698); reversed it needs RAM.
    assert!(state(TapeModel::Tron, Loop, 0.0, 1.0).starts_with("24K · 30.0 S"));
    let rev = state(TapeModel::Tron, Loop, 0.9, 0.1);
    assert!(rev.starts_with(&format!("{REVERSE_NEEDS_RAM} / ")), "{rev}");
}
