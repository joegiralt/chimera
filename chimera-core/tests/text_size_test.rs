//! TEXT SIZE (ADR 0154): LARGE draws the reading text a size up where it
//! fits, NORMAL's faces where it doesn't; the setting turns from its leaf
//! and redraws every band.

mod screen;

use chimera_core::ui::animation::UiTick;
use chimera_core::ui::components::{self, Cell, Look};
use chimera_core::ui::draw::{self, Slot, overflow};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme::{self, Face, TextSize};
use chimera_hal::EncoderId;
use screen::*;

fn cell(label: &'static str, text: &'static str) -> Cell<'static> {
    Cell {
        label,
        text,
        value: 0.5,
        fmt: ValFmt::Int(0),
        active: false,
        mod_amount: Some(0.5),
        look: Look::Live,
        motion: None,
    }
}

fn drawn(c: &Cell, size: TextSize) -> Fb {
    let mut fb = Fb::new();
    components::cell(&mut fb, 4, theme::CELL_LABEL_Y, Some(c), size);
    fb
}

/// A word that fits takes LARGE's face; one that doesn't keeps NORMAL's.
#[test]
fn the_fit_rule_falls_back_per_text() {
    let wide = |z| if z == TextSize::Large { 80 } else { 60 };
    assert_eq!(theme::fit(TextSize::Large, 70, wide), TextSize::Normal);
    assert_eq!(theme::fit(TextSize::Large, 80, wide), TextSize::Large);
    assert_eq!(theme::fit(TextSize::Normal, 999, wide), TextSize::Normal);
    for face in [Face::Label, Face::LabelBold, Face::Value] {
        let (n, l) = (
            theme::font(face, TextSize::Normal),
            theme::font(face, TextSize::Large),
        );
        assert!(
            draw::text_width(l, "VELOCITY", 1) > draw::text_width(n, "VELOCITY", 1),
            "{face:?} is larger at LARGE"
        );
    }
}

/// LARGE changes a cell; a value too wide for its column at LARGE draws
/// in NORMAL's face, the same pixels as NORMAL's value line.
#[test]
fn a_cell_scales_and_a_wide_value_keeps_the_normal_face() {
    let short = cell("Q", "79");
    assert!(drawn(&short, TextSize::Large).px != drawn(&short, TextSize::Normal).px);

    let wide = cell("FREQ", "3.54 kHz");
    let (l, n) = (
        drawn(&wide, TextSize::Large),
        drawn(&wide, TextSize::Normal),
    );
    let x = theme::MARGIN_X + theme::CELL_COL_W;
    let row = |fb: &Fb, y: i32| {
        (x..x + theme::CELL_COL_W)
            .map(|x| fb.at(x, y))
            .collect::<Vec<_>>()
    };
    let ln = theme::cell_lines(TextSize::Large).value - theme::cell_lines(TextSize::Normal).value;
    let base = theme::CELL_LABEL_Y + theme::CELL_ROW_H;
    for dy in 0..12 {
        let y = base + theme::CELL_VALUE_DY - dy;
        assert_eq!(row(&l, y + ln), row(&n, y), "the value line, {dy} px up");
    }
}

/// The probe sees a LARGE text out of its slot, and nothing at NORMAL.
#[test]
fn the_overflow_probe_reports_a_large_spill() {
    overflow::take();
    let mut fb = Fb::new();
    let tight = Slot {
        left: 10,
        top: 95,
        right: 200,
        bottom: 101,
    };
    for size in [TextSize::Normal, TextSize::Large] {
        draw::text_fit(
            &mut fb,
            Face::Value,
            size,
            "ABC",
            10,
            100,
            theme::INK,
            0,
            tight,
        );
    }
    let spills = overflow::take();
    assert_eq!(spills.len(), 1, "{spills:?}");
    let roomy = Slot { top: 80, ..tight };
    draw::text_fit(
        &mut fb,
        Face::Value,
        TextSize::Large,
        "ABC",
        10,
        100,
        theme::INK,
        0,
        roomy,
    );
    assert!(overflow::take().is_empty());
}

/// Each size's cell lines stay inside the row: LARGE's label cap clears
/// the cells band's top, its value's cap clears the label, the mod line
/// clears the next row's label.
#[test]
fn large_cell_lines_fit_the_row() {
    // u8g2's header: helvR10's cap 11, helvB14's 14.
    let (label_cap, value_cap) = (11, 14);
    let l = theme::cell_lines(TextSize::Large);
    assert!(theme::CELL_LABEL_Y + l.label - label_cap >= theme::VIZ_BAND_BOTTOM);
    assert!(l.value - value_cap > l.label, "value cap clears the label");
    assert!(l.bar > l.value, "bar under the value");
    assert!(
        l.modl + 1 < theme::CELL_ROW_H + l.label - label_cap,
        "mod tick clears the next label"
    );
    let last = theme::CELL_LABEL_Y + theme::CELL_ROW_H + l.modl + 1;
    assert!(last < theme::CELLS_BOTTOM);
}

/// SETTINGS › PERSONALIZE › TEXT SIZE turns NORMAL to LARGE and back; the
/// dirty render redraws every band to the full render's pixels.
#[test]
fn the_leaf_sets_the_size_and_every_band_redraws() {
    let (perf, scope) = (PerfStats::zero(), scope_fixture());
    let mut ui = stack_ui();
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    to_leaf(&mut ui, &["PERSONALIZE", "TEXT SIZE"]);
    assert_eq!(ui.theme().text, TextSize::Normal);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.theme().text, TextSize::Large);
    feed(&mut ui, Input::press(chimera_hal::ButtonId::B1));
    for _ in 0..4 {
        ui.update(UiTick::for_test());
        ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    }
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &perf, &scope);
    assert!(fb.px == full.px, "LARGE: dirty == full");

    // Back to NORMAL from outside SETTINGS: the screen drawn LARGE redraws.
    let t = ui.theme();
    ui.set_theme(chimera_core::ui::theme_settings::ThemeSettings {
        text: TextSize::Normal,
        ..t
    });
    ui.update(UiTick::for_test());
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &perf, &scope);
    assert!(fb.px == full.px, "NORMAL again: dirty == full");
}

/// The LARGE screens the goldens lock, the browser's among them: no LARGE
/// text out of its slot (the walks cover the chains and SETTINGS).
#[test]
fn the_large_cases_never_overflow() {
    for name in case_names().filter(|n| n.starts_with("large_")) {
        overflow::take();
        let fb = render(name);
        let spills = overflow::take();
        assert!(spills.is_empty(), "{name}:\n{}", spills.join("\n"));
        assert_eq!(fb.oob, 0, "{name}");
    }
}
