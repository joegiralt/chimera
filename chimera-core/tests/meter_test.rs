//! The pool meter (ADR 0087, sound-pool spec § The meter, and the UI):
//! the bar's percentage, its preview, the browser's line, its byte-bearing
//! hints, and the refusal a save past free space gets.

mod screen;

use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::dense;
use chimera_core::project::{
    Origin, PartActionKind, PartId, PoolFull, SlotId, part_actions, project_crc,
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

/// Rows `y0..=y1` of `a` and `b` match, left of the scroll bar.
fn rows_eq(a: &Fb, b: &Fb, y0: i32, y1: i32) -> bool {
    (y0..=y1).all(|y| (0..browser::SCROLL_X).all(|x| a.at(x, y) == b.at(x, y)))
}

/// NEW's line is its percentage alone, right-aligned, over the bar.
#[test]
fn new_reads_its_percent() {
    let ui = browsing();
    let pool = ui.project().pool();
    let fb = render_ui(&ui);
    let mut want = Fb::new();
    let _ = want.clear(theme::BG);
    draw::text_right(
        &mut want,
        &theme::FONT_LABEL,
        "2%",
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
    let fb = render_ui(&ui);
    let mut want = Fb::new();
    let _ = want.clear(theme::BG);
    hint::draw(
        &mut want,
        &[Gesture("MIX + MINUS", "CLEARS · FREES 0.3K")],
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
    let growth = Offer::of(ui.project(), part).save_needs();
    assert_eq!(
        growth,
        ui.project()
            .pool()
            .needs(slot, &ui.project().part(part).sound)
    );
    fill(&mut ui);
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
    assert_eq!(Offer::of(ui.project(), part).save_cost(), None, "WON'T FIT");
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
    // On two lines, tracked as every label: POOL FULL, then the bytes.
    let mut lines = Fb::new();
    let _ = lines.clear(theme::BG);
    let (head, why) = browser::refusal_lines(want.as_str());
    assert_eq!(head, "POOL FULL");
    assert!(why.starts_with("NEEDS ") && why.ends_with(" FREE"), "{why}");
    for (t, y) in [(head, browser::HINT_Y), (why, browser::SAVE_Y)] {
        draw::text_tracked(
            &mut lines,
            &theme::FONT_LABEL,
            t,
            theme::MARGIN_X,
            y,
            theme::ALERT,
            theme::LABEL_TRACKING,
        );
    }
    assert!(rows_eq(
        &fb,
        &lines,
        browser::HINT_Y - 9,
        browser::CLEAR_Y + 2
    ));

    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.refusal(), None, "until the next input");
    assert!(!band(&render_ui(&ui)));
}

/// Every empty slot filled: dense Sounds, then INIT ones, until the
/// gap takes no more.
fn fill(ui: &mut UiState) {
    for s in SlotId::ALL {
        if ui.project().pool().get(s).is_some() {
            continue;
        }
        let mut x = Sound::init(EngineType::Algo);
        dense(&mut x, s.index() as u64);
        let p = ui.project_mut();
        if p.pool_store(s, &x).is_err() && p.pool_store(s, &Sound::init(EngineType::Algo)).is_err()
        {
            break;
        }
    }
}

/// An empty bar is all rest; a full one has none.
#[test]
fn empty_and_full_bars() {
    let zero = Cost::of(0);
    let empty = bar(&MeterView {
        sounds: zero,
        sources: zero,
        pending: zero,
        preview: None,
    });
    let x = theme::MARGIN_X;
    assert!((x..x + meter::W).all(|i| empty.at(i, 10) == theme::BAR_REST));
    let full = bar(&MeterView {
        sounds: Cost::of(ARENA),
        sources: zero,
        pending: zero,
        preview: Some(Cost::of(16)),
    });
    assert!((x..x + meter::W).all(|i| full.at(i, 10) != theme::BAR_REST));
    assert_eq!(
        full.at(x + meter::W - 1, 10),
        theme::ALERT,
        "a pixel of ALERT"
    );
}

/// One granule shows: at least a pixel.
#[test]
fn one_granule_shows() {
    let zero = Cost::of(0);
    let fb = bar(&MeterView {
        sounds: Cost::of(16),
        sources: zero,
        pending: zero,
        preview: None,
    });
    assert_eq!(fb.at(theme::MARGIN_X, 10), theme::INK);
}

/// Free reads floored: never more than ARENA − used.
#[test]
fn free_never_reads_more_than_there_is() {
    let shown = |l: &str| -> u64 {
        let v = l.strip_suffix('K').unwrap();
        match v.split_once('.') {
            Some((a, b)) => {
                (a.parse::<u64>().unwrap() * 10 + b.parse::<u64>().unwrap()) * 1024 / 10
            }
            None => v.parse::<u64>().unwrap() * 1024,
        }
    };
    for b in (0..=ARENA).step_by(16) {
        let l = Cost::of(b).kib_floor_label();
        assert!(shown(l.as_str()) <= b as u64, "{b}: {}", l.as_str());
    }
    let ui = browsing();
    let pool = ui.project().pool();
    let free = ARENA as u64 - pool.used().bytes() as u64;
    let refusal = prompt::pool_full(PoolFull {
        needs: Cost::of(4096),
        free: pool.free(),
    });
    let label = refusal
        .as_str()
        .strip_suffix(" FREE")
        .and_then(|r| r.rsplit_once(", "))
        .unwrap()
        .1;
    assert!(shown(label) <= free, "{label}");
}

/// Every slot taken, the bytes there: MIX + EDIT reads plain POOL FULL.
#[test]
fn all_slots_taken_reads_plain_pool_full() {
    let mut ui = browsing();
    for s in SlotId::ALL {
        if ui.project().pool().get(s).is_none() {
            ui.project_mut()
                .pool_store(s, &Sound::init(EngineType::Algo))
                .unwrap();
        }
    }
    let offer = Offer::of(ui.project(), PartId::ALL[0]);
    assert!(ui.project().pool().fits(offer.save_needs()));
    assert_eq!(offer.save_cost(), None);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Edit));
    assert_eq!(ui.refusal(), Some("POOL FULL"));
}

/// The save line, drawn: `WON'T FIT` in ALERT when no save fits.
#[test]
fn wont_fit_reads_in_alert() {
    let mut ui = browsing();
    dense(ui.project_mut().edit_part(PartId::ALL[0]).sound, 7);
    fill(&mut ui);
    let fb = render_ui(&ui);
    let mut want = Fb::new();
    let _ = want.clear(theme::BG);
    hint::gesture(
        &mut want,
        &Gesture("MIX + EDIT", "WON'T FIT"),
        theme::MARGIN_X,
        browser::SAVE_Y,
        theme::ALERT,
    );
    assert!(rows_eq(
        &fb,
        &want,
        browser::SAVE_Y - 9,
        browser::SAVE_Y + 2
    ));
    assert!((0..240).any(|x| fb.at(x, browser::SAVE_Y - 3) == theme::ALERT));
}

/// An edited Part's save names its growth over its slot: `pool.needs`.
#[test]
fn save_hint_names_the_growth() {
    let mut ui = browsing();
    let part = PartId::ALL[0];
    let Origin::Slot { slot, .. } = ui.project().part(part).origin() else {
        panic!("NEW's Part 1 plays a slot");
    };
    dense(ui.project_mut().edit_part(part).sound, 7);
    let needs = ui
        .project()
        .pool()
        .needs(slot, &ui.project().part(part).sound);
    assert_eq!(Offer::of(ui.project(), part).save_cost(), Some(needs));
    let fb = render_ui(&ui);
    let mut want = Fb::new();
    let _ = want.clear(theme::BG);
    let words = format!("SAVES · +{}", needs.kib_label().as_str());
    hint::gesture(
        &mut want,
        &Gesture("MIX + EDIT", &words),
        theme::MARGIN_X,
        browser::SAVE_Y,
        theme::MID,
    );
    assert!(rows_eq(
        &fb,
        &want,
        browser::SAVE_Y - 9,
        browser::SAVE_Y + 2
    ));
}
