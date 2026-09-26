mod screen;

use chimera_core::clock_plan::SiliconRev;
use chimera_core::ui::UiState;
use chimera_core::ui::audio_page::cell_texts;
use chimera_core::ui::block_registry::SYS_AUDIO;
use chimera_core::ui::draw::text_width;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme::{CELL_COL_W, FONT_VALUE};
use chimera_hal::ButtonId;
use screen::*;

fn on_audio_page() -> UiState {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::Menu));
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    settle(&mut ui);
    ui
}

fn texts(s: Option<&chimera_core::perf::load::AudioStats>) -> Vec<String> {
    cell_texts(s)
        .iter()
        .map(|b| b.as_str().to_string())
        .collect()
}

#[test]
fn the_page_is_system_about_audio() {
    assert_eq!(on_audio_page().nav.active_block_def().id, SYS_AUDIO.id);
}

#[test]
fn cells_show_the_stats() {
    assert_eq!(
        texts(Some(&audio_fixture())),
        ["23%", "41%", "2", "0/3", "1", "12K"]
    );
}

#[test]
fn without_stats_every_cell_shows_dashes() {
    assert_eq!(texts(None), ["--"; 6]);
}

#[test]
fn a_changed_counter_is_redrawn_and_matches_a_full_render() {
    let mut ui = on_audio_page();
    let (perf, scope) = (PerfStats::zero(), scope_fixture());
    let mut s = audio_fixture();
    let mut fb = Fb::new();
    ui.render_dirty_with_audio(&mut fb, &perf, Some(&s), &scope);
    let idle = ui.render_dirty_with_audio(&mut fb, &perf, Some(&s), &scope);
    assert!(
        idle.iter().all(|r| r.0 == r.1),
        "nothing changed, nothing flushed"
    );
    s.overruns += 1;
    let flushed = ui.render_dirty_with_audio(&mut fb, &perf, Some(&s), &scope);
    assert!(
        flushed.iter().any(|r| r.0 != r.1),
        "the new overrun is drawn"
    );
    let mut full = Fb::new();
    ui.render_with_audio(&mut full, &perf, Some(&s), &scope);
    assert_eq!(fb.hash(), full.hash());
}

#[test]
fn extreme_stats_stay_on_screen() {
    let ui = on_audio_page();
    let mut s = audio_fixture();
    s.load_avg = 250;
    s.load_peak = u16::MAX;
    s.overruns = u32::MAX;
    s.desyncs = u32::MAX;
    s.drops = [u32::MAX; 2];
    s.stack_used = 131_072;
    s.rev = SiliconRev::Unknown(0x2001);
    s.cpu_hz = 400_000_000;
    let mut fb = Fb::new();
    ui.render_with_audio(&mut fb, &PerfStats::zero(), Some(&s), &scope_fixture());
    assert_eq!(fb.oob, 0);
    // Both sources maxed: the "a/b" join would be wider than the column, so
    // DROPS falls back to the saturating total alone.
    assert_eq!(
        texts(Some(&s)),
        ["250%", "65535%", "4294M", "4294M", "4294M", "128K"]
    );
    for text in cell_texts(Some(&s)) {
        let w = text_width(&FONT_VALUE, text.as_str(), 0);
        assert!(
            w <= CELL_COL_W,
            "{:?} is {w}px, wider than the {CELL_COL_W}px column",
            text.as_str()
        );
    }
}

#[test]
fn drops_shows_dashes_with_no_sources() {
    let mut s = audio_fixture();
    s.sources = 0;
    assert_eq!(texts(Some(&s))[3], "--");
}

#[test]
fn other_pages_ignore_the_stats() {
    let ui = UiState::new();
    let (mut a, mut b) = (Fb::new(), Fb::new());
    ui.render_with_audio(
        &mut a,
        &PerfStats::zero(),
        Some(&audio_fixture()),
        &scope_fixture(),
    );
    ui.render_with_audio(&mut b, &PerfStats::zero(), None, &scope_fixture());
    assert_eq!(a.hash(), b.hash());
}
