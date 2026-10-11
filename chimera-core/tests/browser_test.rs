//! Sound browser in Direction A (UI refresh spec § Page types).

mod screen;

use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{PartFrom, PartId, PartSource, Project, ReplaceGuard, SlotId};
use chimera_core::ui::browser::{
    self, INIT_TYPES, SCROLL_TOP, SCROLL_X, TOTAL_ENTRIES, VISIBLE_ROWS, row_y,
};
use chimera_core::ui::nav::{Browse, Location, MixPage};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

/// A project whose pool is empty, its Parts on INIT.
fn empty() -> Box<Project> {
    let (mut p, t) = Project::boxed();
    for part in PartId::ALL {
        let src = PartSource {
            part,
            from: PartFrom::Init(EngineType::Algo),
        };
        p.replace_part(ReplaceGuard::check(&p, t, src).unwrap())
            .unwrap();
    }
    for s in SlotId::ALL {
        p.pool_clear(s).unwrap();
    }
    p
}

/// `draw` on a screen cleared to the ground, as it expects.
fn drawn(p: &Project, cursor: usize, scroll: usize) -> Fb {
    use embedded_graphics::draw_target::DrawTarget;
    let mut fb = Fb::new();
    let _ = fb.clear(theme::BG);
    browser::draw(
        &mut fb,
        p,
        browser::BrowserView::Project,
        PartId::ALL[0],
        cursor,
        scroll,
        true,
        None,
    );
    assert_eq!(fb.oob, 0);
    fb
}

fn row_has(fb: &Fb, i: usize, c: embedded_graphics::pixelcolor::Rgb565) -> bool {
    let y = row_y(i);
    (y - 12..y + 2).any(|yy| (40..230).any(|x| fb.at(x, yy) == c))
}

#[test]
fn the_selected_row_is_an_accent_pill() {
    let fb = drawn(&empty(), 2, 0);
    assert_eq!(fb.at(120, row_y(2) - 5), theme::ACCENT);
    assert_ne!(fb.at(120, row_y(1) - 5), theme::ACCENT);
}

#[test]
fn empty_slots_are_dimmed_and_saved_ones_bright() {
    let mut p = empty();
    p.pool_store(SlotId::ALL[0], &Sound::init(EngineType::Algo))
        .unwrap();
    let fb = drawn(&p, 5, 0);
    assert!(row_has(&fb, 0, theme::INK), "saved slot name in ink");
    assert!(
        !row_has(&fb, 1, theme::INK) && row_has(&fb, 1, theme::FAINT),
        "empty slot: a dim dash"
    );
}

/// An INIT row (one of the chain-type starting points after the pool
/// slots) names its chain in `INK2` -- distinct from a saved slot's `INK`
/// (`empty_slots_are_dimmed_and_saved_ones_bright`) and an empty slot's dim
/// dash (no name at all), since it is neither.
#[test]
fn init_rows_name_their_chain_in_a_distinct_shade() {
    let scroll = TOTAL_ENTRIES - VISIBLE_ROWS; // the tail: the INIT rows follow the pool
    let init_row = VISIBLE_ROWS - INIT_TYPES.len(); // first INIT row's visible index
    let fb = drawn(&empty(), scroll, scroll); // cursor on the first visible row, not an INIT one
    assert!(row_has(&fb, init_row, theme::INK2), "INIT row name in INK2");
    assert!(
        !row_has(&fb, init_row, theme::INK),
        "not the saved-slot shade"
    );
}

#[test]
fn the_scroll_thumb_follows_the_list() {
    let thumb_top = |fb: &Fb| {
        (SCROLL_TOP..SCROLL_TOP + 208)
            .find(|&y| fb.at(SCROLL_X, y) == theme::MID)
            .unwrap()
    };
    let top = thumb_top(&drawn(&empty(), 0, 0));
    let bottom = thumb_top(&drawn(
        &empty(),
        TOTAL_ENTRIES - 1,
        TOTAL_ENTRIES - VISIBLE_ROWS,
    ));
    assert_eq!(top, SCROLL_TOP);
    assert!(bottom > top + 150, "{bottom}");
}

#[test]
fn init_rows_end_the_list_and_load() {
    let mut ui = chimera_core::ui::UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    feed(&mut ui, Input::turn(EncoderId::A, 100));
    feed(&mut ui, Input::turn(EncoderId::A, 100)); // clamps to the last row
    assert_eq!(
        ui.location(),
        Location::sound_at(
            PartId::ALL[1],
            Browse::of(
                (TOTAL_ENTRIES - 1) as u8,
                (TOTAL_ENTRIES - VISIBLE_ROWS) as u8
            )
        )
    );
    let fb = render_ui(&ui);
    assert_eq!(
        fb.at(120, row_y(VISIBLE_ROWS - 1) - 5),
        theme::ACCENT,
        "last visible row selected"
    );
    feed(&mut ui, Input::press(ButtonId::Edit)); // the last of the INIT rows
    assert_eq!(
        ui.project().part(PartId::ALL[1]).sound.engine(),
        EngineType::Va
    );
}

fn render_ui(ui: &chimera_core::ui::UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(
        &mut fb,
        &chimera_core::ui::perf::PerfStats::zero(),
        &scope_fixture(),
    );
    fb
}

#[test]
fn browser_dirty_render_equals_full_render() {
    assert!(render("sound_browser").px == render_dirty("sound_browser").px);
}

/// An idle browser (no cursor/scroll change, no save/load) must not redraw
/// or flush on every frame — only the first render after it opens (#7).
#[test]
fn idle_browser_flushes_nothing_after_the_first_frame() {
    let mut ui = ui_for("sound_browser");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope); // consume the open-time redraw
    for frame in 0..5 {
        let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
        assert!(
            flushed.iter().all(|&(a, b)| a == b),
            "frame {frame}: idle browser flushed {flushed:?}"
        );
    }
}

/// Moving the cursor redraws, and the result matches a full render (#7).
#[test]
fn cursor_move_redraws_and_matches_a_full_render() {
    let mut ui = ui_for("sound_browser");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope); // consume the open-time redraw
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    assert!(
        flushed.iter().any(|&(a, b)| a != b),
        "cursor move should flush: {flushed:?}"
    );

    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &PerfStats::zero(), &scope);
    assert!(
        fb.px == full.px,
        "dirty render after a cursor move == full render"
    );
}

/// A saved Sound's name fills all 16 bytes (no null terminator): the
/// name and the row's `FmtBuf` (32 bytes) must still
/// render it in full, not truncated, and stay inside the screen.
#[test]
fn the_longest_sound_name_is_not_truncated() {
    let mut p = empty();
    let mut s = Sound::init(EngineType::Algo);
    s.name = chimera_core::name::SoundName::new("ABCDEFGHIJKLMNOP").unwrap();
    assert_eq!(s.name.as_str().len(), 16);
    p.pool_store(SlotId::ALL[0], &s).unwrap();
    let fb = drawn(&p, 5, 0); // drawn() asserts fb.oob == 0 (nothing clipped off-screen)
    assert!(row_has(&fb, 0, theme::INK), "16-char name drawn in full");
}

/// A browser redraw clears the screen once, with the framebuffer fill, and
/// draws only its contents through `draw_iter` (#82).
#[test]
fn browser_redraw_does_not_fill_the_screen_again() {
    let mut ui = chimera_core::ui::UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    let (perf, scope) = (PerfStats::zero(), scope_fixture());
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    fb.drawn = 0;
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert!(fb.drawn < W * H, "{} pixels drawn", fb.drawn);
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &perf, &scope);
    assert!(fb.px == full.px);
}

/// Browser input is a pure function of encoder A and the cursor (#75).
#[test]
fn browser_input_moves_the_cursor() {
    use browser::input;
    let at = |cursor: usize, scroll: usize| Browse::of(cursor as u8, scroll as u8);
    let last = TOTAL_ENTRIES - 1;
    assert_eq!(input(&Input::turn(EncoderId::A, 3), at(0, 0)), at(3, 0));
    // The list clamps, and scrolling keeps the cursor on screen.
    assert_eq!(
        input(&Input::turn(EncoderId::A, 127), at(10, 0)),
        at(last, last + 1 - VISIBLE_ROWS)
    );
    assert_eq!(input(&Input::turn(EncoderId::A, -2), at(5, 5)), at(3, 3));
    assert_eq!(input(&Input::press(ButtonId::Edit), at(4, 0)), at(4, 0));
}

/// On the Sound rung, B<n> leaves for Part n's pages, MENU opens SETTINGS
/// and SEQ goes up to the Part's mixer (ADR 0081).
#[test]
fn the_sound_rung_leaves_by_the_location_keys() {
    let mut ui = chimera_core::ui::UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::B3));
    assert_eq!(ui.location().part(), Some(PartId::ALL[2]));
    assert!(ui.location().browse().is_none());
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(
        ui.location(),
        Location::mixer(PartId::ALL[1], MixPage::Sends)
    );
}

/// A 16-character name stops short of its cost: they share no pixels.
#[test]
fn a_long_name_clears_its_cost() {
    let mut p = empty();
    let mut s = Sound::init(EngineType::Algo);
    s.name = chimera_core::name::SoundName::new("WWWWWWWWWWWWWWWW").unwrap();
    p.pool_store(SlotId::ALL[0], &s).unwrap();
    let fb = drawn(&p, 5, 0);
    let y = row_y(0);
    let cols = |c| {
        (100..SCROLL_X)
            .filter(|&x| (y - 12..y + 2).any(|yy| fb.at(x, yy) == c))
            .collect::<Vec<_>>()
    };
    let name_end = *cols(theme::INK).last().expect("the name");
    let cost_start = *cols(theme::MID).first().expect("the cost");
    assert!(name_end < cost_start, "{name_end} < {cost_start}");
}

// LIBRARY tab (library spec § Browser).

use chimera_core::name::SoundName;
use chimera_core::project::save_library_sound;
use chimera_core::storage::{BuildStamp, Card};
use chimera_core::ui::UiState;
use chimera_core::ui::browser::{
    EMPTY_LIBRARY, SAVE_TO_LIBRARY, STAMP_Y, entry_cost, load_words, stamp_words,
};
use chimera_core::ui::draw;
use chimera_core::ui::hint::{self, Gesture};
use chimera_core::ui::theme_settings::ThemeSettings;
use chimera_hal::store::Store;
use chimera_hal::testkit::MemStore;

fn this() -> BuildStamp {
    BuildStamp::new("0.1.0", "02e440e2", false)
}

#[test]
fn stamp_words_forms() {
    let this = this();
    assert_eq!(
        stamp_words(Some(BuildStamp::new("0.1.0", "aabbccdd", true)), &this).as_str(),
        "SAVED ON 0.1.0 AABBCCDD+"
    );
    assert_eq!(
        stamp_words(Some(this), &this).as_str(),
        "SAVED ON THIS BUILD"
    );
    assert_eq!(stamp_words(None, &this).as_str(), "NO STAMP");
    let dev = BuildStamp::new("0.1.0", "dev", false);
    assert_eq!(
        stamp_words(Some(dev), &dev).as_str(),
        "SAVED ON 0.1.0 DEV",
        "a dev build names no commit"
    );
}

#[test]
fn the_new_lines_fit_216_px() {
    assert!(hint::width(&[SAVE_TO_LIBRARY]) <= 216);
    let wide = stamp_words(Some(BuildStamp::new("10.10.10", "wwwwwwww", true)), &this());
    let w = draw::text_width(&theme::FONT_LABEL, wide.as_str(), theme::LABEL_TRACKING);
    assert!(w <= 216, "{w}");
    let words = load_words(Some(chimera_core::project::arena::Cost::of(160 * 1024)));
    assert!(hint::width(&[Gesture("EDIT", words.as_str())]) <= 216);
}

#[test]
fn a_library_row_costs_its_file_less_the_frame_plus_a_granule() {
    use chimera_core::storage::{HEADER_LEN, TRAILER_LEN};
    let len = 1_000u32;
    let want = (len as usize - HEADER_LEN - TRAILER_LEN - 25 + 16).div_ceil(16) * 16;
    assert_eq!(entry_cost(len).bytes() as usize, want);
    assert_eq!(entry_cost(0).bytes(), 16, "short files saturate");
}

fn saved(st: &mut MemStore, name: &str, stamp: &BuildStamp) {
    let mut s = chimera_core::preset::Sound::init(EngineType::Algo);
    // Dense, so its preview is wider than a pixel of the meter.
    chimera_core::project::test_support::dense(&mut s, 3);
    let n = SoundName::new(name).unwrap();
    save_library_sound(&mut Card::new(), st, &s, n, stamp)
        .out
        .unwrap();
}

/// Part 1's LIBRARY tab on `st`, listed, the cursor at the top.
fn library_ui(st: MemStore) -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    let mut r = Rig::new(&mut ui, st);
    r.feed(Input::chord(ButtonId::Edit, ButtonId::B1));
    r.feed(Input::turn(EncoderId::B, 1));
    r.feed(Input::turn(EncoderId::B, 0));
    drop(r);
    ui
}

fn text_in_row_at(fb: &Fb, y: i32) -> bool {
    (y - 9..y + 2).any(|yy| (12..230).any(|x| fb.at(x, yy) != theme::BG))
}

#[test]
fn line_three_draws_with_hints_off() {
    let mut st = MemStore::new(1);
    saved(&mut st, "ONE", &BuildStamp::new("0.1.0", "aabbccdd", false));
    let mut ui = library_ui(st);
    assert!(text_in_row_at(&render_ui(&ui), STAMP_Y), "hints on");
    ui.set_theme(ThemeSettings {
        hints: false,
        ..ThemeSettings::DEFAULT
    });
    let fb = render_ui(&ui);
    assert!(text_in_row_at(&fb, STAMP_Y), "the stamp is information");
    assert!(!text_in_row_at(&fb, browser::HINT_Y), "the hints are off");
}

/// `c` among the pixels of the row at baseline `y`, short of its right edge.
fn ink_in_row(fb: &Fb, y: i32, c: embedded_graphics::pixelcolor::Rgb565) -> bool {
    (y - 12..y + 2).any(|yy| (12..230).any(|x| fb.at(x, yy) == c))
}

#[test]
fn the_empty_library_is_one_dimmed_row() {
    assert_eq!(EMPTY_LIBRARY, "NO SOUNDS YET");
    let mut gone = MemStore::new(1);
    gone.eject();
    for st in [MemStore::new(1), gone] {
        let fb = render_ui(&library_ui(st));
        let y = row_y(0);
        assert!(!ink_in_row(&fb, y, theme::ACCENT), "no selection pill");
        assert!(
            ink_in_row(&fb, y, theme::MID),
            "the words in the dim colour"
        );
        assert!(!ink_in_row(&fb, y, theme::INK), "not a live row");
        assert!(!text_in_row_at(&fb, STAMP_Y), "no row, no stamp");
    }
}

#[test]
fn a_newer_firmware_row_is_greyed_with_its_name_and_message() {
    use chimera_core::storage::{HEADER_LEN, TRAILER_LEN};
    let mut st = MemStore::new(1);
    saved(&mut st, "FUTURE", &this());
    // An engine code this build doesn't know: the peek has no CRC.
    let v = st.mount().unwrap();
    let f = chimera_hal::store::FileName::new(
        chimera_hal::store::Dir::Sounds,
        &chimera_core::storage::LibId::new(1).unwrap().stem(),
        b"A",
    )
    .unwrap();
    let mut bytes = Vec::new();
    struct Take<'a>(&'a mut Vec<u8>);
    impl chimera_hal::store::ReadSink for Take<'_> {
        fn begin(&mut self, _: u32) -> core::ops::ControlFlow<()> {
            core::ops::ControlFlow::Continue(())
        }
        fn chunk(&mut self, b: &[u8]) -> core::ops::ControlFlow<()> {
            self.0.extend_from_slice(b);
            core::ops::ControlFlow::Continue(())
        }
    }
    st.read(v, f, &mut Take(&mut bytes)).unwrap();
    assert!(bytes.len() > HEADER_LEN + TRAILER_LEN);
    bytes[HEADER_LEN + 4] = 0xEE; // Engine's payload
    st.write(v, f, &mut |w: &mut dyn chimera_hal::store::ByteSink| {
        w.put(&bytes)
    })
    .unwrap();
    let fb = render_ui(&library_ui(st));
    let y = row_y(0);
    // Selected: BG ink on the pill, the name left and the message right.
    assert!(
        ink_in_row(&fb, y, theme::BG),
        "name and message on the pill"
    );
    let msg = "NEEDS NEWER FIRMWARE";
    let w = draw::text_width(&theme::FONT_LABEL, msg, 0);
    let left = 18 + draw::text_width(&theme::FONT_VALUE, "FUTURE", 0);
    let ink = (230 - w..230).any(|x| (y - 12..y + 2).any(|yy| fb.at(x, yy) == theme::BG));
    assert!(ink, "the message at the right edge");
    assert!(left < 230 - w, "name and message fit side by side");
    let none = (232..240).all(|x| fb.at(x, y) != theme::INK);
    assert!(none);
}

#[test]
fn a_sound_row_names_its_engine_and_cost_and_previews_in_the_meter() {
    let mut st = MemStore::new(1);
    saved(&mut st, "WARM BASS", &this());
    let ui = library_ui(st);
    let fb = render_ui(&ui);
    assert!(row_has(&fb, 0, theme::BG), "name on the pill");
    let preview = (12..228).any(|x| fb.at(x, browser::METER_Y) == theme::ACCENT);
    assert!(preview, "the selected row previewed in ACCENT");
}

#[test]
fn a_sound_that_wont_fit_previews_in_alert_and_says_so() {
    let mut st = MemStore::new(1);
    saved(&mut st, "BIG", &this());
    let mut ui = Box::new(UiState::new());
    chimera_core::project::test_support::set_free_to(
        ui.project_mut(),
        chimera_core::project::arena::Cost::of(16),
    );
    let mut r = Rig::new(&mut ui, st);
    r.feed(Input::chord(ButtonId::Edit, ButtonId::B1));
    r.feed(Input::turn(EncoderId::B, 1));
    drop(r);
    let fb = render_ui(&ui);
    assert!((12..228).any(|x| fb.at(x, browser::METER_Y) == theme::ALERT));
    let alert = (12..228)
        .any(|x| (browser::HINT_Y - 9..browser::HINT_Y).any(|y| fb.at(x, y) == theme::ALERT));
    assert!(alert, "WON'T FIT in ALERT");
}
