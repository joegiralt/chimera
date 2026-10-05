//! The pool meter (ADR 0087, sound-pool spec § The meter, and the UI):
//! the bar's percentage, its preview, the browser's line, its byte-bearing
//! hints, and the refusal a save past free space gets.

mod screen;

use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::dense;
use chimera_core::project::{
    Origin, PartActionKind, PartId, PoolFull, SlotId, part_actions, project_crc, save_needs,
};
use chimera_core::ui::hint::{self, Gesture};
use chimera_core::ui::meter::{self, MeterView, percent};
use chimera_core::ui::settings::part::{Offer, PartCmd};
use chimera_core::ui::settings::prompt;
use chimera_core::ui::{UiState, browser, draw, theme};
use chimera_hal::{ButtonId, EncoderId};
use embedded_graphics::draw_target::DrawTarget;
use screen::{Fb, Input, feed, render_ui};

const ARENA: usize = ARENA_BYTES;

#[test]
fn meter_percent() {
    assert_eq!(percent(Cost::of(0)), 0);
    assert_eq!(percent(Cost::of(16)), 1);
    assert_eq!(percent(Cost::of(ARENA - 16)), 100);
    assert_eq!(percent(Cost::of(ARENA / 2)), 50);
}

fn bar(v: &MeterView) -> Fb {
    let mut fb = Fb::new();
    let _ = fb.clear(theme::BG);
    meter::draw(&mut fb, v, theme::MARGIN_X, 10);
    fb
}

fn half(preview: Option<Cost>) -> MeterView {
    MeterView {
        sounds: Cost::of(ARENA / 4),
        sources: Cost::of(ARENA / 8),
        pending: Cost::of(ARENA / 8),
        preview,
    }
}

/// Each part in its token, in order, the bar `meter::W` wide.
#[test]
fn the_bar_reads_each_part() {
    let fb = bar(&half(None));
    let x = theme::MARGIN_X;
    assert_eq!(fb.at(x, 10), theme::INK);
    assert_eq!(fb.at(x + 60, 10), theme::MID);
    assert_eq!(fb.at(x + 90, 13), theme::FAINT);
    assert_eq!(fb.at(x + 120, 10), theme::BAR_REST);
    assert_eq!(fb.at(x + meter::W - 1, 10), theme::BAR_REST);
    assert_eq!(fb.at(x + meter::W, 10), theme::BG);
    assert_eq!(fb.at(x, 10 + meter::H), theme::BG);
}

/// A preview that fits is ACCENT for its width; one that won't is ALERT
/// from the used edge to the bar's end.
#[test]
fn preview_alert_when_it_wont_fit() {
    let x = theme::MARGIN_X;
    let used = x + meter::W / 2;
    let fits = bar(&half(Some(Cost::of(ARENA / 4))));
    assert!((used..used + meter::W / 4).all(|i| fits.at(i, 11) == theme::ACCENT));
    assert_eq!(fits.at(x + meter::W - 1, 11), theme::BAR_REST);
    let over = bar(&half(Some(Cost::of(ARENA / 2 + 16))));
    assert_eq!(over.at(used - 1, 11), theme::FAINT);
    assert!((used..x + meter::W).all(|i| over.at(i, 11) == theme::ALERT));
    assert_eq!(over.at(x + meter::W, 11), theme::BG);
}

fn browsing() -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    ui
}

/// Rows `y0..=y1` of `a` and `b` match across the screen.
fn rows_eq(a: &Fb, b: &Fb, y0: i32, y1: i32) -> bool {
    (y0..=y1).all(|y| (0..240).all(|x| a.at(x, y) == b.at(x, y)))
}

/// NEW's line is its percentage alone, right-aligned, over the bar.
#[test]
fn new_reads_its_percent() {
    let ui = browsing();
    let pool = ui.project().pool();
    let p = percent(pool.used());
    assert!(p > 0);
    let fb = render_ui(&ui);
    let mut want = Fb::new();
    let _ = want.clear(theme::BG);
    draw::text_right(
        &mut want,
        &theme::FONT_LABEL,
        &format!("{p}%"),
        theme::VIZ_RIGHT,
        browser::INFO_Y,
        theme::MID,
        0,
    );
    meter::draw(
        &mut want,
        &MeterView::of(pool, None),
        theme::MARGIN_X,
        browser::METER_Y,
    );
    assert!(rows_eq(&fb, &want, browser::INFO_Y - 10, 319));
    assert_eq!(fb.at(theme::MARGIN_X, browser::METER_Y), theme::INK);
}

/// A filled slot's CLEAR hint names the bytes the clear frees.
#[test]
fn clear_hint_names_bytes() {
    assert_eq!(
        browser::clear_words(Cost::of(300)).as_str(),
        "CLEARS · FREES 0.3K"
    );
    let ui = browsing();
    let cost = ui.project().pool().get(SlotId::ALL[0]).unwrap().cost();
    let fb = render_ui(&ui);
    let mut want = Fb::new();
    let _ = want.clear(theme::BG);
    let words = browser::clear_words(cost);
    hint::draw(
        &mut want,
        &[Gesture("MIX + MINUS", words.as_str())],
        theme::MARGIN_X,
        browser::CLEAR_Y,
    );
    assert!(rows_eq(
        &fb,
        &want,
        browser::CLEAR_Y - 9,
        browser::CLEAR_Y + 2
    ));
}

/// The Part's save names its cost against the slot it replaces.
#[test]
fn save_hint_names_its_cost() {
    assert_eq!(
        browser::save_words(Some(Cost::of(96))).as_str(),
        "SAVES · +0.1K"
    );
    assert_eq!(browser::save_words(None).as_str(), "WON'T FIT");
}

/// Review Focus 2: SAVE OVER a slot whose edit grew past free space is
/// refused with POOL FULL, in the band until the next input. The Part
/// keeps its edits and the project its CRC.
#[test]
fn save_over_growth_refused_keeps_part() {
    let mut ui = browsing();
    let part = PartId::ALL[0];
    let Origin::Slot { slot, .. } = ui.project().part(part).origin() else {
        panic!("NEW's Part 1 plays a slot");
    };
    dense(ui.project_mut().edit_part(part).sound, 7);
    let growth = save_needs(ui.project(), part);
    assert_eq!(
        growth,
        ui.project()
            .pool()
            .needs(slot, &ui.project().part(part).sound)
    );
    for s in SlotId::ALL {
        if ui.project().pool().get(s).is_some() {
            continue;
        }
        let mut x = Sound::init(EngineType::Algo);
        dense(&mut x, s.index() as u64);
        if ui.project_mut().pool_store(s, &x).is_err()
            && ui
                .project_mut()
                .pool_store(s, &Sound::init(EngineType::Algo))
                .is_err()
        {
            break;
        }
    }
    let free = ui.project().pool().free();
    assert!(free < growth, "{free:?} < {growth:?}");
    // An offer that can't apply isn't made (ADR 0043).
    let offered = part_actions(ui.project(), part);
    let saves = offered.iter().filter(|a| {
        matches!(
            a.kind(),
            PartActionKind::OverSlot(_) | PartActionKind::NewSlot(_)
        )
    });
    assert_eq!(saves.count(), 0);
    assert_eq!(browser::save_cost(ui.project(), part), None, "WON'T FIT");
    // SAVE TO PROJ's rows say why in their place.
    let offer = Offer::of(ui.project(), part);
    for c in [PartCmd::OverSlot, PartCmd::NewSlot] {
        assert_eq!(offer.label(c).as_str(), "POOL FULL");
    }

    let sound = ui.project().part(part).sound.clone();
    let crc = project_crc(ui.project());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Edit));

    let want = prompt::pool_full(PoolFull {
        needs: growth,
        free,
    });
    assert!(
        want.as_str().starts_with("POOL FULL · NEEDS "),
        "{}",
        want.as_str()
    );
    assert_eq!(ui.refusal(), Some(want.as_str()));
    assert!(ui.project().part(part).sound.bits_eq(&sound));
    assert_eq!(project_crc(ui.project()), crc);

    let fb = render_ui(&ui);
    let band = |fb: &Fb| (0..240).any(|x| fb.at(x, browser::HINT_Y - 4) == theme::ALERT);
    assert!(band(&fb), "the refusal reads in ALERT");

    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.refusal(), None, "until the next input");
    assert!(!band(&render_ui(&ui)));
}
