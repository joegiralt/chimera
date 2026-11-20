//! ALGO's 0.4 UI tickets (ADR 0298): the envelope pages' order, names and
//! wiring (#987, #956), RATE SCALE's hint (#988), the ratio readout
//! (#986), FDBK in tenths of the TX81Z's steps (#990, ADR 0299) and the
//! chart's feedback loops (#989). Each ticket's tests follow its imports.

mod common;
mod screen;

use chimera_core::addr::{BlockRead, BlockRef, Op};
use chimera_core::block::{ParamId, ValFmt};
use chimera_core::dsp::algo::env::EnvRates;
use chimera_core::dsp::algo::params::{ALGO_OP_SPECS, AlgoOpParams};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_def::{BlockDef, VizType};
use chimera_core::ui::block_registry::{
    ALGO_AR, ALGO_CHAIN, ALGO_D1L, ALGO_D1R, ALGO_D2R, ALGO_RR,
};
use chimera_core::ui::glyph::{FocusGlyph, Gauge, gauge};
use chimera_core::ui::{UiState, components, draw, dungeon_map, part_page, theme};
use chimera_hal::EncoderId;
use screen::*;

/// An envelope page: its def, the TX81Z's name on its tab, the stored
/// param, the words the header, the row and the focus band spell out, and
/// the engine's rate it must move.
struct Stage {
    def: &'static BlockDef,
    tx: &'static str,
    id: ParamId,
    words: &'static str,
    rate: fn(&EnvRates) -> u8,
}

/// The TX81Z's order: AR, D1R, D1L, D2R, RR.
const STAGES: [Stage; 5] = [
    Stage {
        def: &ALGO_AR,
        tx: "AR",
        id: AlgoOpParams::AR,
        words: "ATTACK RATE",
        rate: |r| r.ar,
    },
    Stage {
        def: &ALGO_D1R,
        tx: "D1R",
        id: AlgoOpParams::D1R,
        words: "DECAY 1 RATE",
        rate: |r| r.d1r,
    },
    Stage {
        def: &ALGO_D1L,
        tx: "D1L",
        id: AlgoOpParams::D1L,
        words: "DECAY 1 LEVEL",
        rate: |r| r.d1l,
    },
    Stage {
        def: &ALGO_D2R,
        tx: "D2R",
        id: AlgoOpParams::D2R,
        words: "DECAY 2 RATE",
        rate: |r| r.d2r,
    },
    Stage {
        def: &ALGO_RR,
        tx: "RR",
        id: AlgoOpParams::RR,
        words: "RELEASE RATE",
        rate: |r| r.rr,
    },
];

/// Part 1 on ALGO's INIT, at OSC, under STACK's keys.
fn ui() -> UiState {
    let mut ui = stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    to_osc(&mut ui);
    ui
}

#[test]
fn the_envelope_pages_follow_the_tx81z_order() {
    let subs = ALGO_CHAIN.blocks[osc_node()].sub_pages;
    let at = |d: &BlockDef| subs.iter().position(|s| s.id == d.id).unwrap();
    let first = at(STAGES[0].def);
    for (i, s) in STAGES.iter().enumerate() {
        assert_eq!(at(s.def), first + i, "{}", s.tx);
        assert_eq!(s.def.short, s.tx);
        assert_eq!(s.def.viz, VizType::OpEnv, "{}", s.tx);
    }
}

/// #987: each envelope cell edits the parameter its label names, for
/// every operator, and that parameter is the engine's rate of that name.
#[test]
fn each_envelope_cell_edits_the_parameter_its_label_names() {
    for s in &STAGES {
        let spec = ALGO_OP_SPECS.iter().find(|p| p.id == s.id).unwrap();
        assert_eq!((spec.label, spec.ident), (s.tx, s.tx));
        for (k, op) in Op::ALL.into_iter().enumerate() {
            // A cell at its top only moves down.
            for d in [1i8, -1] {
                let mut sound = Sound::init(EngineType::Algo);
                let before = sound.params.clone();
                part_page::apply_encoder(s.def, k, d, &mut sound.params, &mut Op::A);
                let moved: Vec<(Op, ParamId)> = Op::ALL
                    .into_iter()
                    .flat_map(|m| ALGO_OP_SPECS.iter().map(move |p| (m, p.id)))
                    .filter(|&(m, id)| {
                        let b = BlockRef::AlgoOp(m);
                        sound.params.block(b).unwrap().get(id) != before.block(b).unwrap().get(id)
                    })
                    .collect();
                let (was, now) = (
                    before.algo.ops[op.index()].rates(),
                    sound.params.algo.ops[op.index()].rates(),
                );
                if moved.is_empty() {
                    assert_eq!(was, now);
                    continue;
                }
                assert_eq!(moved, [(op, s.id)], "{} OP{}", s.tx, k + 1);
                // The engine's rate of that name moved, and no other.
                assert_ne!((s.rate)(&was), (s.rate)(&now), "{}", s.tx);
                let others = |r: &EnvRates| {
                    STAGES
                        .iter()
                        .filter(|o| o.id != s.id)
                        .map(|o| (o.rate)(r))
                        .collect::<Vec<_>>()
                };
                assert_eq!(others(&was), others(&now), "{}", s.tx);
            }
        }
    }
}

/// #987, #956: the header, the row and the focus band spell the stage out.
#[test]
fn the_envelope_pages_spell_their_stage_out() {
    for s in &STAGES {
        // The header has 90 px beside `PART 6 · SOUND`: LEVEL is LVL there.
        let header = s.words.replace("1 LEVEL", "1 LVL");
        assert_eq!(s.def.name.to_uppercase(), header);
        let row = dungeon_map::ROW_WORDS.iter().find(|w| w.0.id == s.def.id);
        assert_eq!(row.unwrap().1, s.words);
        let mut ui = ui();
        to_osc_sub(&mut ui, s.def);
        for (k, e) in [EncoderId::A, EncoderId::F].into_iter().enumerate() {
            feed(&mut ui, Input::turn(e, -1));
            let op = [1usize, 6][k];
            // The lit cell names the operator; the band, the stage.
            assert_eq!(ui.focused_slot(), op - 1);
            assert_eq!(ui.focus_label_for_test().as_str(), s.words);
        }
    }
}

/// The op page's FDBK cell is short for its 62 px; the band has room.
#[test]
fn the_op_pages_feedback_reads_in_full_in_the_band() {
    use chimera_core::ui::block_registry::ALGO_OP_PAGES;
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_OP_PAGES[0]);
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(ui.focus_label_for_test().as_str(), "FEEDBACK");
    feed(&mut ui, Input::turn(EncoderId::D, -1));
    assert_eq!(ui.focus_label_for_test().as_str(), "LEVEL");
}

/// Each label clears the arc gauge at NORMAL; with the operator before
/// it (`OP6 ATTACK RATE`) none would, so the lit cell names the operator.
#[test]
fn the_spelled_out_labels_fit_the_band() {
    let arc = gauge(FocusGlyph::Arc, 0.0, ValFmt::Int(31), |_| Gauge::None);
    let slot = components::focus_label_slot(&arc);
    let w = |t: &str| draw::text_width(&theme::FONT_VALUE, t, theme::LABEL_TRACKING);
    for s in &STAGES {
        assert!(
            slot.left + w(s.words) <= slot.right,
            "{}: {}",
            s.words,
            w(s.words)
        );
        let with_op = format!("OP6 {}", s.words);
        assert!(
            slot.left + w(&with_op) > slot.right,
            "{with_op} fits after all"
        );
    }
    let bar = gauge(FocusGlyph::LevelBar, 0.0, ValFmt::Uni, |_| Gauge::None);
    assert!(slot.left + w("FEEDBACK") <= components::focus_label_slot(&bar).right);
}

// #988: RATE SCALE's words, and DECAY 1 LEVEL's under a held decay.

use chimera_core::addr::ParamAddr;
use chimera_core::ui::block_registry::ALGO_RATE_SCALE;
use chimera_core::ui::notes;
use chimera_core::ui::view::{SlotCtx, view};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;

fn op_addr(op: Op, id: ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::AlgoOp(op), id)
}

/// #988: RATE SCALE says what it does, on every operator, in the hint's
/// place; its cells carry the operators' names, not the tab's RS.
#[test]
fn rate_scale_says_what_it_does() {
    let s = Sound::init(EngineType::Algo);
    for op in Op::ALL {
        assert_eq!(
            notes::cell(op_addr(op, AlgoOpParams::RATE_SCALE), &s),
            Some("HIGH NOTES, FASTER ENVELOPES")
        );
    }
    let w = draw::text_width(&theme::FONT_LABEL, notes::RATE_SCALE, theme::LABEL_TRACKING);
    assert!(w <= theme::CONTENT_W, "{w}");
    let ctx = SlotCtx::read(&s.params, Op::A);
    let labels: Vec<_> = (0..6)
        .map(|k| view(&ALGO_RATE_SCALE, k, &ctx).label())
        .collect();
    assert_eq!(labels, ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"]);
    assert_eq!(
        (ALGO_RATE_SCALE.name, ALGO_RATE_SCALE.short),
        ("Rate Scale", "RS")
    );
}

/// The words reach the screen: the band draws them where the hint was.
#[test]
fn rate_scales_words_are_drawn_in_the_band() {
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_RATE_SCALE);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    settle(&mut ui);
    let shot = render_ui(&ui);
    let mut want = Fb::new();
    draw::text_tracked(
        &mut want,
        &theme::FONT_LABEL,
        notes::RATE_SCALE,
        theme::MARGIN_X,
        chimera_core::ui::hint::FOCUS_Y,
        theme::MID,
        chimera_core::ui::source_cell::tracking(notes::RATE_SCALE),
    );
    let lit: Vec<_> = (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
        .filter(|&(x, y)| want.at(x, y) != Rgb565::BLACK)
        .collect();
    assert!(!lit.is_empty());
    assert!(lit.iter().all(|&(x, y)| shot.at(x, y) == want.at(x, y)));
}

/// #987's likely cause: with DECAY 1 RATE 0 the envelope holds at full
/// and DECAY 1 LEVEL does nothing, so its band says why.
#[test]
fn decay_1_level_says_when_it_is_never_reached() {
    let d1l = op_addr(Op::B, AlgoOpParams::D1L);
    let mut s = Sound::init(EngineType::Algo);
    // INIT: D1R 0, D1L 15, where D2 runs at once.
    assert_eq!(notes::cell(d1l, &s), None);
    s.params.algo.ops[1].d1l = 9;
    assert_eq!(notes::cell(d1l, &s), Some(notes::D1_HOLDS));
    assert_eq!(notes::cell(op_addr(Op::A, AlgoOpParams::D1L), &s), None);
    s.params.algo.ops[1].d1r = 1;
    assert_eq!(notes::cell(d1l, &s), None);
    let w = draw::text_width(&theme::FONT_LABEL, notes::D1_HOLDS, theme::LABEL_TRACKING);
    assert!(w <= theme::CONTENT_W, "{w}");
}

// #986: the ratio CRSE, FINE and DETUN come to.

use chimera_core::dsp::algo::params::AlgoParams;

/// The ratio and the cents a readout line shows.
fn read_ratio(line: &str) -> (f32, f32) {
    let rest = line.strip_prefix("RATIO ").expect(line);
    match rest.split_once(", DETUNE ") {
        None => (rest.parse().expect(line), 0.0),
        Some((r, c)) => {
            let c = c.strip_suffix(" CENTS").expect(line);
            assert!(c.starts_with(['+', '-']), "{line}");
            (r.parse().expect(line), c.parse().expect(line))
        }
    }
}

/// #986: the readout is the engine's own ratio and detune, at every
/// CRSE x FINE x DETUN, to the two decimals COARSE itself shows.
#[test]
fn the_ratio_readout_matches_the_engine_everywhere() {
    use chimera_core::dsp::algo::tx::{detune_at, ratio_at};
    let mut s = Sound::init(EngineType::Algo);
    for coarse in 0..=63u8 {
        for fine in 0..=15u8 {
            for detune in -3..=3i8 {
                let o = &mut s.params.algo.ops[2];
                (o.coarse, o.fine, o.detune) = (coarse, fine, detune);
                let line = notes::ratio(&s.params.algo, Op::C, AlgoOpParams::FINE).unwrap();
                let (r, cents) = read_ratio(line.as_str());
                // What `AlgoEngine` multiplies the note's frequency by.
                let engine = ratio_at(coarse, f32::from(fine));
                assert!(
                    (r - engine).abs() <= 0.005 + 1e-4,
                    "{}: {engine}",
                    line.as_str()
                );
                let heard = 1200.0 * detune_at(f32::from(detune)).log2();
                assert!((cents - heard).abs() < 0.01, "{}: {heard}", line.as_str());
                assert_eq!(line.as_str().contains("DETUNE"), detune != 0);
            }
        }
    }
}

/// Each FINE step moves the readout, and a step past a low CRSE's top
/// (FINE 8-15 below 1.00) doesn't.
#[test]
fn the_ratio_readout_moves_with_each_step_the_engine_takes() {
    let mut s = Sound::init(EngineType::Algo);
    let mut at = |coarse: u8, fine: u8| {
        let o = &mut s.params.algo.ops[0];
        (o.coarse, o.fine) = (coarse, fine);
        notes::ratio(&s.params.algo, Op::A, AlgoOpParams::COARSE).unwrap()
    };
    for coarse in 0..=63u8 {
        let top = if coarse < 4 { 7 } else { 15 };
        for fine in 1..=15u8 {
            let (was, now) = (at(coarse, fine - 1), at(coarse, fine));
            assert_eq!(was != now, fine <= top, "CRSE {coarse} FINE {fine}");
        }
    }
    assert_eq!(at(4, 0).as_str(), "RATIO 1.00");
    assert_eq!(at(5, 0).as_str(), "RATIO 1.41");
}

/// CRSE, FINE and DETUN show it, on the group pages and the operator's
/// own; no other cell does, and a noise band has no pitch to show.
#[test]
fn the_ratio_shows_on_the_three_tuning_cells_only() {
    use chimera_core::dsp::algo::kinds::OpKind;
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.ops[0].detune = -3;
    for p in ALGO_OP_SPECS.iter() {
        let tuned = [
            AlgoOpParams::COARSE,
            AlgoOpParams::FINE,
            AlgoOpParams::DETUNE,
        ]
        .contains(&p.id);
        let line = chimera_core::ui::view::ratio_note(op_addr(Op::A, p.id), &s);
        assert_eq!(line.is_some(), tuned, "{}", p.label);
        if let Some(l) = line {
            assert_eq!(l.as_str(), "RATIO 1.00, DETUNE -4.5 CENTS");
            let w = draw::text_width(&theme::FONT_LABEL, l.as_str(), theme::LABEL_TRACKING);
            assert!(w <= theme::CONTENT_W, "{w}");
        }
    }
    // The widest line there is.
    let o = &mut s.params.algo.ops[0];
    (o.coarse, o.fine, o.detune) = (63, 15, 3);
    let l = notes::ratio(&s.params.algo, Op::A, AlgoOpParams::FINE).unwrap();
    assert_eq!(l.as_str(), "RATIO 27.57, DETUNE +4.5 CENTS");
    let w = draw::text_width(&theme::FONT_LABEL, l.as_str(), theme::LABEL_TRACKING);
    assert!(w <= theme::CONTENT_W, "{w}");
    s.params.algo.try_set_kind(1, OpKind::Noise).unwrap();
    assert_eq!(
        notes::ratio(&s.params.algo, Op::B, AlgoOpParams::FINE),
        None
    );
    let part = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    assert_eq!(chimera_core::ui::view::ratio_note(part, &s), None);
}

/// The ratio reaches the screen: FINE's band draws it where the hint was.
#[test]
fn the_ratio_is_drawn_in_fines_band() {
    use chimera_core::ui::block_registry::ALGO_FINE;
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_FINE);
    feed(&mut ui, Input::turn(EncoderId::A, 5));
    settle(&mut ui);
    let sound = &ui.project().part(PartId::ALL[0]).sound;
    let line = notes::ratio(&sound.params.algo, Op::A, AlgoOpParams::FINE).unwrap();
    assert_eq!(line.as_str(), "RATIO 1.31");
    let shot = render_ui(&ui);
    let mut want = Fb::new();
    draw::text_tracked(
        &mut want,
        &theme::FONT_LABEL,
        line.as_str(),
        theme::MARGIN_X,
        chimera_core::ui::hint::FOCUS_Y,
        theme::MID,
        chimera_core::ui::source_cell::tracking(line.as_str()),
    );
    let lit: Vec<_> = (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
        .filter(|&(x, y)| want.at(x, y) != Rgb565::BLACK)
        .collect();
    assert!(!lit.is_empty());
    assert!(lit.iter().all(|&(x, y)| shot.at(x, y) == want.at(x, y)));
}

// #990: FDBK in tenths.

use chimera_core::dsp::algo::params::Fdbk;
use chimera_core::ui::block_registry::ALGO_FEEDBACK;

const FDBK: ParamId = AlgoOpParams::FEEDBACK;

fn fdbk_spec() -> &'static chimera_core::block::ParamSpec {
    ALGO_OP_SPECS.iter().find(|p| p.id == FDBK).unwrap()
}

/// #990: FDBK keeps the TX81Z's 0 to 7 and its place on the card and the
/// NRPN chart, in 71 steps of a tenth.
#[test]
fn fdbk_runs_the_tx81zs_range_in_tenths() {
    use chimera_core::nrpn;
    use chimera_midi::law::ValueLaw;
    let s = fdbk_spec();
    assert_eq!(
        (s.id, s.ident, s.min, s.max),
        (ParamId(11), "FDBK", 0.0, 7.0)
    );
    assert_eq!((s.step, s.default), (Fdbk::STEP, 0.0));
    assert_eq!(s.fmt, ValFmt::Tenths(70));
    assert_eq!(nrpn::steps(s), 70);
    assert!(matches!(
        nrpn::law(BlockRef::AlgoOp(Op::A), s),
        ValueLaw::Index7
    ));
    // Every other stepped param still counts whole numbers.
    for b in BlockRef::ALL {
        for p in b
            .specs()
            .iter()
            .filter(|p| p.id != FDBK || !matches!(b, BlockRef::AlgoOp(_)))
        {
            if p.kind == chimera_core::block::ParamKind::Stepped {
                assert_eq!(p.step, 1.0, "{b:?} {}", p.label);
                assert_eq!(nrpn::steps(p), (p.max - p.min).round() as u16);
            }
        }
    }
}

/// Each tenth is stored, read back as the number a text file writes for
/// it, shown to one decimal, and one encoder tick from the next.
#[test]
fn each_tenth_of_fdbk_is_stored_shown_and_one_tick_apart() {
    use chimera_core::addr::Blocks;
    use chimera_core::ui::fmt::{FmtBuf, fmt_val};
    let mut s = Sound::init(EngineType::Algo);
    let spec = fdbk_spec();
    for t in 0..=70u8 {
        let text = format!("{}.{}", t / 10, t % 10);
        let x: f32 = text.parse().unwrap();
        let mut b = s.params.block_mut(BlockRef::AlgoOp(Op::C)).unwrap();
        b.set(FDBK, x);
        assert_eq!(b.get(FDBK).to_bits(), x.to_bits(), "{text}");
        if t < 70 {
            b.nudge(FDBK, 1);
            assert_eq!(b.get(FDBK), Fdbk::tenths(t + 1).steps(), "{text} + a tick");
            b.nudge(FDBK, -1);
        }
        let mut shown = FmtBuf::new();
        fmt_val(&mut shown, spec.normalize(b.get(FDBK)), spec.fmt);
        assert_eq!(shown.as_str(), text);
        assert_eq!(s.params.algo.ops[2].feedback, Fdbk::tenths(t));
    }
    let mut b = s.params.block_mut(BlockRef::AlgoOp(Op::C)).unwrap();
    for (v, want) in [
        (-1.0, 0.0),
        (9.0, 7.0),
        (0.04, 0.0),
        (0.06, 0.1),
        (3.96, 4.0),
    ] {
        b.set(FDBK, v);
        assert_eq!(b.get(FDBK), want, "{v}");
    }
}

/// MIX + a turn steps the TX81Z's whole steps.
#[test]
fn a_snap_steps_fdbks_whole_numbers() {
    use chimera_core::addr::Blocks;
    let mut s = Sound::init(EngineType::Algo);
    let mut b = s.params.block_mut(BlockRef::AlgoOp(Op::A)).unwrap();
    for (from, d, to) in [
        (0.0, 1, 1.0),
        (0.3, 1, 1.0),
        (1.0, 1, 2.0),
        (6.5, 1, 7.0),
        (7.0, 1, 7.0),
        (1.0, -1, 0.0),
        (2.4, -1, 2.0),
        (0.0, -1, 0.0),
    ] {
        b.set(FDBK, from);
        b.snap(FDBK, d);
        assert_eq!(b.get(FDBK), to, "{from} {d:+}");
    }
}

/// A Sound saved before holds a whole step: it loads as that step, plays
/// the TX81Z's swing bit for bit, and saves as the bytes it was.
#[test]
fn an_old_sounds_fdbk_loads_and_plays_as_it_did() {
    use chimera_core::dsp::algo::engine::AlgoLive;
    use chimera_core::dsp::algo::tx::{FEEDBACK_CYCLES, feedback_cycles};
    use chimera_core::storage::{MIGRATIONS, RecordBuf, TRANSLATIONS, decode_block, encode_block};
    for n in 0..=7u8 {
        // OP_B's block (code 4), FDBK (id 11) as 0.3 wrote it: a whole f32.
        let mut payload = vec![4u8, 11];
        payload.extend_from_slice(&f32::from(n).to_le_bytes());
        let mut s = Sound::init(EngineType::Algo);
        let lost = decode_block(&payload, MIGRATIONS, TRANSLATIONS, Some(&mut s.params)).unwrap();
        assert_eq!(lost.count, 0);
        assert_eq!(s.params.algo.ops[1].feedback, Fdbk::tx(n));
        let live = AlgoLive::from_params(&s.params.algo);
        assert_eq!(live.feedback[1].to_bits(), f32::from(n).to_bits());
        assert_eq!(
            feedback_cycles(live.feedback[1]).to_bits(),
            FEEDBACK_CYCLES[usize::from(n)].to_bits()
        );
        let base = Sound::init(EngineType::Algo);
        let mut out = RecordBuf::new();
        let b = BlockRef::AlgoOp(Op::B);
        let wrote = encode_block(
            b,
            &*s.params.block(b).unwrap(),
            Some(&*base.params.block(b).unwrap()),
            &mut out,
        );
        assert_eq!(wrote, usize::from(n != 0));
        if n != 0 {
            assert_eq!(out.as_slice(), &payload[..]);
        }
    }
}

/// A tick on FBK's page moves a tenth and the cell shows it.
#[test]
fn a_tick_on_the_page_is_a_tenth() {
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_FEEDBACK);
    feed(&mut ui, Input::turn(EncoderId::A, 3));
    settle(&mut ui);
    let algo = &ui.project().part(PartId::ALL[0]).sound.params.algo;
    assert_eq!(algo.ops[0].feedback, Fdbk::tenths(3));
    assert!(first_cell_reads(&render_ui(&ui), "0.3"));
}

/// The knobs glide FDBK on its place, read from its byte of tenths.
#[test]
fn the_knobs_place_fdbk_by_its_tenths() {
    use chimera_core::dsp::knobs::KNOBS;
    let mut s = Sound::init(EngineType::Algo);
    let k = KNOBS
        .iter()
        .find(|k| k.addr == op_addr(Op::D, FDBK))
        .unwrap();
    for t in 0..=70u8 {
        s.params.algo.ops[3].feedback = Fdbk::tenths(t);
        let want = k.spec.normalize(Fdbk::tenths(t).steps());
        assert!((k.read(&s.params) - want).abs() <= 1e-6, "{t}");
        let back = k.spec.denormalize(k.read(&s.params));
        assert!((back - Fdbk::tenths(t).steps()).abs() <= 1e-5, "{t}");
    }
}

// #989: the chart's feedback loops.

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::ui::notes::Loop;
use chimera_core::ui::{alg_layout, viz};

/// The chart of `algo`, MORPH at rest on ALG A.
fn chart(algo: &AlgoParams) -> Fb {
    let mut fb = Fb::new();
    let (a, _) = algo.algs();
    viz::algo_diagram(&mut fb, a.algorithm(), a.algorithm(), 0.0, algo);
    assert_eq!(fb.oob, 0);
    fb
}

/// Where two charts differ.
fn moved(a: &Fb, b: &Fb) -> Vec<(i32, i32)> {
    (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
        .filter(|&(x, y)| a.at(x, y) != b.at(x, y))
        .collect()
}

fn algo_on(alg: AlgoId) -> AlgoParams {
    let mut p = Sound::init(EngineType::Algo).params.algo;
    p.try_set_algs(alg, alg).unwrap();
    p
}

/// #989: FDBK 0 draws the chart as before; above 0 the operator, and only
/// it, gains a loop beside its node, on a stack, a fan and a row.
#[test]
fn an_operator_that_feeds_back_wears_a_loop() {
    for alg in [AlgoId::T1, AlgoId::A1, AlgoId::A16, AlgoId::A17, AlgoId::T8] {
        let plain = chart(&algo_on(alg));
        let at = alg_layout::layout(alg.algorithm());
        for op in Op::ALL {
            let mut p = algo_on(alg);
            assert_eq!(Loop::of(&p, op), Loop::None);
            p.ops[op.index()].feedback = Fdbk::tenths(1);
            assert_eq!(Loop::of(&p, op), Loop::Soft);
            let looped = chart(&p);
            let px = moved(&plain, &looped);
            assert!(px.len() >= 6, "{alg:?} OP{}: {px:?}", op.index() + 1);
            let (cx, cy) = notes::loop_centre(&at, op, &links_of(alg));
            for &(x, y) in &px {
                assert!(
                    (x - cx).abs() <= notes::LOOP_R && (y - cy).abs() <= notes::LOOP_R,
                    "{alg:?} OP{}: ({x}, {y}) off the loop at ({cx}, {cy})",
                    op.index() + 1
                );
                assert_eq!(looped.at(x, y), theme::MID);
                // Outside the node: the node covers the loop's inner half.
                let (nx, ny) = at.pos[op.index()];
                assert!((x - nx).pow(2) + (y - ny).pow(2) > (at.r - 1).pow(2));
                assert!((theme::VIZ_LEFT..=theme::VIZ_RIGHT).contains(&x), "{x}");
            }
        }
    }
}

/// The loop brightens where the sine starts to break up, and a noise
/// band, which ignores FDBK, wears none.
#[test]
fn the_loop_follows_the_amount_and_the_kind() {
    use chimera_core::dsp::algo::kinds::OpKind;
    let mut p = algo_on(AlgoId::T1);
    let soft: Vec<_> = (0..=Fdbk::MAX.get())
        .map(|t| {
            p.ops[0].feedback = Fdbk::tenths(t);
            Loop::of(&p, Op::A)
        })
        .collect();
    assert_eq!(soft[0], Loop::None);
    assert!(soft[1..40].iter().all(|&l| l == Loop::Soft));
    assert!(soft[40..].iter().all(|&l| l == Loop::Hard));
    p.ops[0].feedback = Fdbk::tx(1);
    let low = chart(&p);
    p.ops[0].feedback = Fdbk::tx(7);
    let high = chart(&p);
    let px = moved(&low, &high);
    assert!(!px.is_empty());
    assert!(px.iter().all(|&(x, y)| low.at(x, y) == theme::MID));
    assert!(px.iter().all(|&(x, y)| high.at(x, y) == theme::ACCENT));
    p.try_set_kind(0, OpKind::Noise).unwrap();
    assert_eq!(Loop::of(&p, Op::A), Loop::None);
    assert!(moved(&chart(&p), &chart(&algo_on(AlgoId::T1))).is_empty());
}

/// The links `alg`'s chart draws, as `algo_diagram` hands them on.
fn links_of(alg: AlgoId) -> [u8; 6] {
    alg.algorithm().mods
}

/// A node at the band's right edge wears its loop on the left; and on
/// every algorithm every operator's loop stays inside the band, both
/// ways, and lands on no link's lit pixel.
#[test]
fn a_loop_stays_inside_the_band_and_off_the_links() {
    use chimera_core::dsp::algo::algorithms::ALGO_COUNT;
    let mut at = alg_layout::layout(AlgoId::A1.algorithm());
    at.pos[0].0 = theme::VIZ_RIGHT - at.r;
    let (cx, _) = notes::loop_centre(&at, Op::A, &[0; 6]);
    assert!(cx + notes::LOOP_R <= theme::VIZ_RIGHT);
    assert!(cx < at.pos[0].0);
    for i in 0..ALGO_COUNT as u8 {
        let alg = AlgoId::from_index(i).unwrap();
        let name = alg.algorithm().name;
        let at = alg_layout::layout(alg.algorithm());
        let plain = chart(&algo_on(alg));
        for op in Op::ALL {
            let (cx, cy) = notes::loop_centre(&at, op, &links_of(alg));
            assert!(cx - notes::LOOP_R >= theme::VIZ_LEFT, "{name}");
            assert!(cx + notes::LOOP_R <= theme::VIZ_RIGHT, "{name}");
            assert!(cy - notes::LOOP_R > theme::VIZ_BAND_TOP, "{name}");
            assert!(cy + notes::LOOP_R < theme::VIZ_BAND_BOTTOM, "{name}");
            let mut p = algo_on(alg);
            p.ops[op.index()].feedback = Fdbk::MAX;
            let looped = chart(&p);
            let ear: Vec<_> = moved(&plain, &looped);
            assert!(ear.len() >= 6, "{name} OP{}: {ear:?}", op.index() + 1);
            for (x, y) in ear {
                assert_eq!(looped.at(x, y), theme::ACCENT);
                // Nothing was lit there: no link's MID or FAINT.
                assert_eq!(
                    plain.at(x, y),
                    Rgb565::BLACK,
                    "{name} OP{}: the loop is over a link at ({x}, {y})",
                    op.index() + 1
                );
            }
        }
    }
}

/// FBK's page keeps the live scope (the owner's call); back on ALG's
/// page the chart wears the loop of the operator just turned.
#[test]
fn the_loop_is_on_the_chart_after_fdbk_is_turned() {
    assert_eq!(ALGO_FEEDBACK.viz, VizType::None);
    let alg_after = |turn: i8| {
        let mut ui = ui();
        to_osc_sub(&mut ui, &ALGO_FEEDBACK);
        feed(&mut ui, Input::turn(EncoderId::B, turn));
        feed(&mut ui, Input::press(chimera_hal::ButtonId::Minus));
        assert_eq!(
            ui.page_def().id,
            chimera_core::ui::block_registry::ALGO_ALG.id
        );
        settle(&mut ui);
        let algo = ui.project().part(PartId::ALL[0]).sound.params.algo;
        (render_ui(&ui), algo)
    };
    let (before, _) = alg_after(0);
    let (after, algo) = alg_after(1);
    assert_eq!(Loop::of(&algo, Op::B), Loop::Soft);
    let at = alg_layout::layout(algo.algs().0.algorithm());
    let (cx, cy) = notes::loop_centre(&at, Op::B, &algo.algs().0.algorithm().mods);
    let band: Vec<_> = moved(&before, &after)
        .into_iter()
        .filter(|&(_, y)| (theme::VIZ_BAND_TOP..theme::VIZ_BAND_BOTTOM).contains(&y))
        .collect();
    assert!(!band.is_empty());
    assert!(
        band.iter()
            .all(|&(x, y)| (x - cx).abs() <= notes::LOOP_R && (y - cy).abs() <= notes::LOOP_R),
        "{band:?}"
    );
}

/// FDBK's band says its two turns: MIX for the TX81Z's whole numbers,
/// SEQ for a tenth a detent (ADR 0299).
#[test]
fn fdbks_band_hints_the_whole_step_turn() {
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_FEEDBACK);
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    let h = ui.focus_hint_for_test();
    assert_eq!(h[0], chimera_core::ui::hint::Gesture("MIX+TURN", "WHOLE"));
    assert_eq!(h[1].0, "SEQ+TURN");
    assert!(chimera_core::ui::hint::width(h) <= theme::CONTENT_W);
    // LEVEL's keeps the plain pair.
    to_osc_sub(&mut ui, &chimera_core::ui::block_registry::ALGO_OP_PAGES[0]);
    feed(&mut ui, Input::turn(EncoderId::D, -1));
    assert!(ui.focus_hint_for_test().iter().all(|g| g.0 != "MIX+TURN"));
}
