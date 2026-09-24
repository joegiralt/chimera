//! Sound browser in Direction A (UI refresh spec § Page types).

mod screen;

use chimera_core::preset::{ChainType, Sound, SoundPool};
use chimera_core::ui::browser::{self, row_y, SCROLL_TOP, SCROLL_X, TOTAL_ENTRIES, VISIBLE_ROWS};
use chimera_core::ui::theme;
use chimera_core::ui::UiMode;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn drawn(pool: &SoundPool, cursor: usize, scroll: usize) -> Fb {
    let mut fb = Fb::new();
    browser::draw(&mut fb, pool, 0, cursor, scroll);
    assert_eq!(fb.oob, 0);
    fb
}

fn row_has(fb: &Fb, i: usize, c: embedded_graphics::pixelcolor::Rgb565) -> bool {
    let y = row_y(i);
    (y - 12..y + 2).any(|yy| (40..230).any(|x| fb.at(x, yy) == c))
}

#[test]
fn the_selected_row_is_an_accent_pill() {
    let fb = drawn(&SoundPool::new(), 2, 0);
    assert_eq!(fb.at(120, row_y(2) - 5), theme::ACCENT);
    assert_ne!(fb.at(120, row_y(1) - 5), theme::ACCENT);
}

#[test]
fn empty_slots_are_dimmed_and_saved_ones_bright() {
    let mut pool = SoundPool::new();
    pool.store(0, Sound::init(ChainType::Fm));
    let fb = drawn(&pool, 5, 0);
    assert!(row_has(&fb, 0, theme::INK), "saved slot name in ink");
    assert!(!row_has(&fb, 1, theme::INK) && row_has(&fb, 1, theme::FAINT), "empty slot: a dim dash");
}

#[test]
fn the_scroll_thumb_follows_the_list() {
    let thumb_top = |fb: &Fb| (SCROLL_TOP..SCROLL_TOP + 208).find(|&y| fb.at(SCROLL_X, y) == theme::MID).unwrap();
    let top = thumb_top(&drawn(&SoundPool::new(), 0, 0));
    let bottom = thumb_top(&drawn(&SoundPool::new(), TOTAL_ENTRIES - 1, TOTAL_ENTRIES - VISIBLE_ROWS));
    assert_eq!(top, SCROLL_TOP);
    assert!(bottom > top + 150, "{bottom}");
}

#[test]
fn init_rows_end_the_list_and_load() {
    let mut ui = chimera_core::ui::UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    feed(&mut ui, Input::turn(EncoderId::Main, 100)); // clamps to the last row
    assert_eq!(ui.ui_mode, UiMode::SoundBrowser { part: 1, cursor: TOTAL_ENTRIES - 1, scroll: TOTAL_ENTRIES - VISIBLE_ROWS });
    let fb = render_ui(&ui);
    assert_eq!(fb.at(120, row_y(VISIBLE_ROWS - 1) - 5), theme::ACCENT, "last visible row selected");
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.performance.parts[1].sound.chain_type, ChainType::Fm);
}

fn render_ui(ui: &chimera_core::ui::UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &chimera_core::ui::perf::PerfStats::zero(), &scope_fixture());
    fb
}

#[test]
fn browser_dirty_render_equals_full_render() {
    assert!(render("sound_browser").px == render_dirty("sound_browser").px);
}

/// A saved Sound's name fills all of `NAME_LEN` (16 bytes, no null
/// terminator): `name_str` and the row's `FmtBuf` (32 bytes) must still
/// render it in full, not truncated, and stay inside the screen.
#[test]
fn the_longest_sound_name_is_not_truncated() {
    use chimera_core::preset::NAME_LEN;

    let mut pool = SoundPool::new();
    let mut s = Sound::init(ChainType::Fm);
    s.name = *b"ABCDEFGHIJKLMNOP"; // exactly NAME_LEN bytes, no trailing 0
    assert_eq!(s.name.len(), NAME_LEN);
    pool.store(0, s);
    let fb = drawn(&pool, 5, 0); // drawn() asserts fb.oob == 0 (nothing clipped off-screen)
    assert!(row_has(&fb, 0, theme::INK), "16-char name drawn in full");
}
