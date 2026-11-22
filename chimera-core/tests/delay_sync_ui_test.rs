//! The delay's tempo sync on the box (ADR 0302): SYNC in cell E of DLY ›
//! CHAR and DLY › GRAIN, the TIME cell turning its note a detent a note
//! and reading the note that plays, MIX + turn flipping SYNC with its
//! hint, and the band's words. Its own binary: it publishes the global
//! clock (`meter::delay_fold`).

#![cfg(feature = "orbit")]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::delay::{DelayNote, DelayParams, DelaySync};
use chimera_core::dsp::rev::sync::{FoldState, Tempo};
use chimera_core::meter::publish_rev_clock;
use chimera_core::ui::block_def::slot_addr;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::hint::{self, Gesture};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme::{self, TextSize};
use chimera_core::ui::{UiState, draw, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed};

/// The published clock is one global: a test at a time.
static CLOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn alone() -> std::sync::MutexGuard<'static, ()> {
    let g = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
    publish_rev_clock(Tempo::DEFAULT, FoldState(0), FoldState(0));
    g
}

const TIME: EncoderId = EncoderId::B;
const TIME_SLOT: usize = 1;
const SYNC_SLOT: usize = 4;

fn at(id: chimera_core::block::ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::Delay, id)
}

fn delay(ui: &UiState) -> DelayParams {
    ui.project().perf().fx.delay
}

/// On DELAY, the TIME cell focused.
fn on_delay() -> UiState {
    let mut ui = screen::stack_ui();
    screen::to_fx(&mut ui, screen::DLY);
    feed(&mut ui, Input::turn(TIME, 1));
    feed(&mut ui, Input::turn(TIME, -1));
    ui
}

fn mix_turn(ui: &mut UiState, e: EncoderId, d: i8) {
    feed(ui, Input::held(ButtonId::Mix).and_turn(e, d));
    feed(ui, Input::release(ButtonId::Mix));
}

/// SYNC is cell E on DLY › CHAR and on DLY › GRAIN, live under either
/// TYPE; a detent turns it on and off, and the band says what it does.
#[test]
fn sync_is_cell_e_on_char_and_grain() {
    let _alone = alone();
    for def in [&reg::DELAY_CHAR, &reg::DELAY_GRAIN] {
        let mut ui = screen::stack_ui();
        screen::to_fx(&mut ui, screen::DLY);
        for _ in 0..2 {
            if ui.page_def().id != def.id {
                feed(&mut ui, Input::press(ButtonId::Edit));
            }
        }
        assert_eq!(ui.page_def().id, def.id);
        let ctx = ui.slot_ctx_for_test();
        assert_eq!(slot_addr(def, SYNC_SLOT, &ctx), Some(at(DelayParams::SYNC)));
        let v = view::view(def, SYNC_SLOT, &ctx);
        assert_eq!(v.label(), "SYNC");
        assert_eq!(ui.cell_text_for_test(SYNC_SLOT).as_str(), "FREE");
        feed(&mut ui, Input::turn(EncoderId::E, 1));
        assert_eq!(delay(&ui).sync(), DelaySync::Sync, "{}", def.name);
        screen::settle(&mut ui);
        assert_eq!(ui.cell_text_for_test(SYNC_SLOT).as_str(), "SYNC");
        let sound = &ui.project().part(ui.active_part).sound;
        assert_eq!(
            view::band_note(Some(at(DelayParams::SYNC)), sound, ctx.src),
            Some("TIME TURNS NOTES OF THE TEMPO")
        );
        feed(&mut ui, Input::turn(EncoderId::E, -1));
        assert_eq!(delay(&ui).sync(), DelaySync::Free);
    }
}

/// MIX + turn on TIME flips SYNC from the main page: right for SYNC, left
/// for FREE, and again either way changes nothing. The ms and the note
/// stay what they were, and the band hints the gesture in both states.
#[test]
fn mix_turn_on_time_flips_sync_and_is_hinted() {
    let _alone = alone();
    let mut ui = on_delay();
    // The flip, and beside it learn's words where a CC may drive the
    // cell, else FREE's fine turn: neither gesture lost its hint.
    let learn = Gesture("MIX + SEQ", "LEARN");
    let mapped = cfg!(feature = "midi-map");
    let to_sync = Gesture("MIX+TURN", "SYNC");
    let to_free = Gesture("MIX+TURN", "FREE");
    let free: Vec<Gesture> = match mapped {
        true => vec![to_sync, learn],
        false => vec![to_sync, Gesture("SEQ+TURN", "FINE")],
    };
    let synced: Vec<Gesture> = match mapped {
        true => vec![to_free, learn],
        false => vec![to_free],
    };
    assert_eq!(ui.focus_hint_for_test(), &free[..]);
    let before = delay(&ui);
    assert_eq!(before.sync(), DelaySync::Free);

    mix_turn(&mut ui, TIME, 1);
    assert_eq!(delay(&ui).sync(), DelaySync::Sync);
    assert_eq!(delay(&ui).time_ms, before.time_ms, "the ms is kept");
    assert_eq!(delay(&ui).note(), DelayNote::DEFAULT);
    let ctx = ui.slot_ctx_for_test();
    assert_eq!(
        slot_addr(&reg::DELAY, TIME_SLOT, &ctx),
        Some(at(DelayParams::NOTE))
    );
    assert_eq!(view::view(&reg::DELAY, TIME_SLOT, &ctx).label(), "TIME");
    assert_eq!(ui.focus_hint_for_test(), &synced[..]);
    mix_turn(&mut ui, TIME, 3);
    assert_eq!(delay(&ui).sync(), DelaySync::Sync, "SYNC is the end");
    assert_eq!(
        delay(&ui).note(),
        DelayNote::DEFAULT,
        "MIX + turn turns no note"
    );

    mix_turn(&mut ui, TIME, -1);
    assert_eq!(delay(&ui), before, "back in FREE, as it was");
    assert_eq!(ui.focus_hint_for_test(), &free[..]);
    mix_turn(&mut ui, TIME, -1);
    assert_eq!(delay(&ui), before);
    // No other cell of the page takes it: FDBK's MIX + turn snaps.
    mix_turn(&mut ui, EncoderId::C, 1);
    assert_eq!(delay(&ui).sync(), DelaySync::Free);
    assert_ne!(delay(&ui).feedback, before.feedback);
}

/// Under SYNC the TIME cell walks the notes one a detent, at any speed
/// of turn, end to end, and reads the note; FREE again, it reads and
/// turns the ms it left.
#[test]
fn time_turns_a_note_a_detent() {
    let _alone = alone();
    let names = [
        "1/32T", "1/32", "1/16T", "1/32D", "1/16", "1/8T", "1/16D", "1/8", "1/4T", "1/8D", "1/4",
    ];
    let mut ui = on_delay();
    publish_rev_clock(Tempo::DEFAULT, FoldState(0), FoldState(0));
    let free_text = ui.cell_text_for_test(TIME_SLOT);
    mix_turn(&mut ui, TIME, 1);
    screen::settle(&mut ui);
    assert_eq!(ui.cell_text_for_test(TIME_SLOT).as_str(), "1/8D");
    // Down to the first, a detent at a time.
    for i in (0..9).rev() {
        feed(&mut ui, Input::turn(TIME, -1));
        screen::settle(&mut ui);
        assert_eq!(delay(&ui).note().index(), i);
        assert_eq!(ui.cell_text_for_test(TIME_SLOT).as_str(), names[i]);
    }
    feed(&mut ui, Input::turn(TIME, -1));
    assert_eq!(delay(&ui).note().index(), 0, "the list ends");
    // A fast turn: three detents in a frame are three notes, no more.
    feed(&mut ui, Input::turn(TIME, 3));
    assert_eq!(delay(&ui).note().index(), 3);
    feed(&mut ui, Input::turn(TIME, 7));
    feed(&mut ui, Input::turn(TIME, 7));
    assert_eq!(delay(&ui).note().index(), DelayNote::COUNT - 1);
    screen::settle(&mut ui);
    assert_eq!(ui.cell_text_for_test(TIME_SLOT).as_str(), "1/4");

    mix_turn(&mut ui, TIME, -1);
    screen::settle(&mut ui);
    assert_eq!(
        ui.cell_text_for_test(TIME_SLOT).as_str(),
        free_text.as_str()
    );
    let ms = delay(&ui).time_ms;
    feed(&mut ui, Input::turn(TIME, -1));
    assert!(delay(&ui).time_ms < ms, "FREE turns the ms");
    assert_eq!(
        delay(&ui).note().index(),
        DelayNote::COUNT - 1,
        "the note waits"
    );
}

/// A note the engine halves reads as it plays, marked: 1/4 at 90 BPM is
/// `▾1/8`, and the band says why in the hint's place. The dirty render
/// redraws the cell and the band when only the published fold moves.
#[test]
fn a_folded_note_reads_as_it_plays_and_redraws() {
    let _alone = alone();
    let mut ui = on_delay();
    mix_turn(&mut ui, TIME, 1);
    feed(&mut ui, Input::turn(TIME, 1)); // 1/4
    screen::settle(&mut ui);
    let (perf, scope) = (PerfStats::zero(), screen::scope_fixture());
    publish_rev_clock(Tempo(120.0), FoldState(0), FoldState(0));
    assert_eq!(ui.cell_text_for_test(TIME_SLOT).as_str(), "1/4");
    let sound = ui.project().part(ui.active_part).sound.clone();
    let src = ui.slot_ctx_for_test().src;
    let note = |a| view::band_note(Some(at(a)), &sound, src);
    assert_eq!(note(DelayParams::NOTE), None);
    let mut fb = screen::Fb::new();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let whole = fb.px.clone();

    // The reverb's fold is not the delay's.
    publish_rev_clock(Tempo(90.0), FoldState(1), FoldState(0));
    assert_eq!(ui.cell_text_for_test(TIME_SLOT).as_str(), "1/4");
    publish_rev_clock(Tempo(90.0), FoldState(0), FoldState(1));
    assert_eq!(ui.cell_text_for_test(TIME_SLOT).as_str(), "▾1/8");
    assert_eq!(note(DelayParams::NOTE), Some("HALVED TO FIT 500 MS"));
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let mut full = screen::Fb::new();
    ui.render_with_audio(&mut full, &perf, None, &scope);
    assert!(fb.px == full.px, "the dirty render kept a stale note");
    let rows = |px: &[u16], r: core::ops::Range<i32>| {
        px[r.start as usize * screen::W..r.end as usize * screen::W].to_vec()
    };
    let band = theme::HEADER_BOTTOM..theme::FOCUS_BOTTOM;
    let cells = theme::FOCUS_BOTTOM..screen::H as i32;
    assert!(rows(&fb.px, band.clone()) != rows(&whole, band), "the band");
    assert!(
        rows(&fb.px, cells.clone()) != rows(&whole, cells),
        "the cell"
    );

    publish_rev_clock(Tempo(1_200.0), FoldState(0), FoldState(-2));
    assert_eq!(note(DelayParams::NOTE), Some("DOUBLED TO FIT 10 MS"));
    publish_rev_clock(Tempo::DEFAULT, FoldState(0), FoldState(0));
    // FREE's TIME says nothing in the hint's place.
    assert_eq!(note(DelayParams::TIME_MS), None);
}

/// The band's hint and words fit its line in NORMAL and LARGE, and
/// SYNC's words and every note fit a cell.
#[test]
fn the_words_fit() {
    let _alone = alone();
    use chimera_core::ui::delay_sync::{
        DOUBLED, FREE_FINE, FREE_LEARN, HALVED, SYNC_LEARN, SYNC_PLAIN, SYNC_WORDS,
    };
    let line = screen::W as i32 - 2 * theme::MARGIN_X;
    // Each hint fits the band's line at NORMAL; at LARGE the band draws a
    // line that doesn't fit in NORMAL's faces (`hint::focus`), checked
    // drawn below.
    let hints: [hint::Hint; 4] = [&FREE_LEARN, &FREE_FINE, &SYNC_LEARN, &SYNC_PLAIN];
    for h in hints {
        let w = hint::width_at(h, TextSize::Normal);
        assert!(w <= line, "{h:?} is {w} px of {line}");
    }
    for words in [HALVED, DOUBLED, SYNC_WORDS] {
        let w = draw::text_width(&theme::FONT_LABEL, words, theme::LABEL_TRACKING);
        assert!(w <= line, "{words}: {w} px of {line}");
    }
    // The synced page drawn at LARGE, whole and halved: no text leaves
    // its slot, nothing off screen.
    let mut ui = on_delay();
    screen::set_text(&mut ui, TextSize::Large);
    mix_turn(&mut ui, TIME, 1);
    let (perf, scope) = (PerfStats::zero(), screen::scope_fixture());
    for (note, fold) in [(-1, 0), (1, 0), (1, 1), (-9, -2)] {
        feed(&mut ui, Input::turn(TIME, note));
        publish_rev_clock(Tempo::DEFAULT, FoldState(0), FoldState(fold));
        screen::settle(&mut ui);
        draw::overflow::take();
        let mut fb = screen::Fb::new();
        ui.render_with_audio(&mut fb, &perf, None, &scope);
        let spills = draw::overflow::take();
        assert!(spills.is_empty(), "{note} {fold}:\n{}", spills.join("\n"));
        assert_eq!(fb.oob, 0);
    }
    publish_rev_clock(Tempo::DEFAULT, FoldState(0), FoldState(0));
}
