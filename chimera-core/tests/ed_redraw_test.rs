// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! An editor's `ed set` takes the cell's focus, as a turn of its knob
//! does, so the focus band's glyph and the viz show the set: MORPH from
//! the editor moves the crossfader and blends ALG A into B on the box
//! (ADR 0331). Never another Part's set, another pad's, nor one under a
//! gesture that reads the focus: a held choice (in `console_ed_engines`),
//! learn, MIX down.

mod common;
mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::glyph::Gauge;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::{UiState, theme};
use chimera_hal::{ButtonId, EncoderId};
use common::console_fake::{Fake, ask};
use screen::*;

fn frame(ui: &mut UiState, fb: &mut Fb) {
    ui.update(UiTick::for_test());
    ui.render_dirty_with_scope(fb, &PerfStats::zero(), &scope_fixture());
}

fn ed_set(f: &mut Fake, addr: &str, v: &str) {
    ed(f, &format!("ed set {addr} {v}"));
}

/// `line`, queued, then the UI pass that applies it.
fn ed(f: &mut Fake, line: &str) {
    assert_eq!(ask(f, &format!("{line}\n")), "OK queued\n", "{line}");
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

fn band(fb: &Fb) -> Vec<u16> {
    (theme::VIZ_BAND_TOP..theme::VIZ_BAND_BOTTOM)
        .flat_map(|y| (theme::VIZ_LEFT..theme::VIZ_RIGHT).map(move |x| (x, y)))
        .map(|(x, y)| fb.px[y as usize * W + x as usize])
        .collect()
}

/// ALGO's ALG page, ALG B one past A, focus on ALG A (as the box last
/// left it), MORPH 0, drawn.
fn alg_page() -> (Fake, Fb) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut f = Fake::new();
    stack_map(&mut f.ui);
    init_part(&mut f.ui, PartId::ALL[0]);
    feed(&mut f.ui, Input::turn(EncoderId::B, 1));
    feed(&mut f.ui, Input::turn(EncoderId::A, 1));
    feed(&mut f.ui, Input::turn(EncoderId::A, -1));
    settle(&mut f.ui);
    let mut fb = Fb::new();
    for _ in 0..3 {
        frame(&mut f.ui, &mut fb);
    }
    assert_eq!(f.ui.focused_slot(), 0);
    (f, fb)
}

#[test]
fn an_ed_set_on_morph_moves_its_glyph_and_the_diagram() {
    let (mut f, mut fb) = alg_page();
    let before = band(&fb);
    ed_set(&mut f, "ALGO.MORPH", "100");
    for _ in 0..60 {
        frame(&mut f.ui, &mut fb);
    }
    assert_eq!(
        f.ui.project().part(PartId::ALL[0]).sound.params.algo.morph,
        100
    );
    assert_eq!(f.ui.focused_slot(), 2, "MORPH's cell takes the focus");
    match f.ui.shown_gauge() {
        Some(Gauge::Crossfader { value }) => assert!((value - 100.0 / 127.0).abs() < 1e-3),
        g => panic!("the band draws MORPH's crossfader, not {g:?}"),
    }
    assert!(band(&fb) != before, "the diagram blends A into B");
    assert!(fb.px == render_ui(&f.ui).px, "the screen is stale");
}

#[test]
fn an_ed_set_off_the_page_leaves_the_focus() {
    let (mut f, mut fb) = alg_page();
    ed_set(&mut f, "FILTER.CUTOFF", "0.25");
    frame(&mut f.ui, &mut fb);
    assert_eq!(f.ui.focused_slot(), 0);
}

/// Taken for Part 1, applied after the box went to Part 2's ALG page: it
/// lands on Part 1 and leaves the focus Part 2's page shows.
#[test]
fn another_parts_set_leaves_the_focus() {
    let (mut f, _) = alg_page();
    assert_eq!(ask(&mut f, "ed set ALGO.MORPH 100\n"), "OK queued\n");
    feed(&mut f.ui, Input::press(ButtonId::B2));
    assert_eq!(f.ui.active_part, PartId::ALL[1]);
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
    assert_eq!(f.ui.active_part, PartId::ALL[1]);
    assert_eq!(
        f.ui.page_def().id,
        chimera_core::ui::block_registry::ALGO_ALG.id
    );
    assert_eq!(
        f.ui.project().part(PartId::ALL[0]).sound.params.algo.morph,
        100
    );
    assert_eq!(f.ui.focused_slot(), 0);
}

/// DRUM's KIT shows the selected pad (pad 1): pad 3's LEVEL leaves the
/// focus, pad 1's takes it.
#[test]
fn a_pad_the_box_hasnt_selected_leaves_the_focus() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut f = Fake::new();
    stack_map(&mut f.ui);
    load_init(&mut f.ui, chimera_core::params::EngineType::Drum);
    settle(&mut f.ui);
    assert_eq!(
        f.ui.page_def().id,
        chimera_core::ui::block_registry::DRUM_KIT.id
    );
    let was = f.ui.focused_slot();
    assert_ne!(was, 2, "LEVEL is KIT's slot c");
    ed_set(&mut f, "PAD3.LEVEL", "0.25");
    assert_eq!(f.ui.focused_slot(), was);
    ed_set(&mut f, "PAD1.LEVEL", "0.25");
    assert_eq!(f.ui.focused_slot(), 2);
}

/// Learn armed on CUTOFF binds the focused cell: a set on RESO, on the
/// same page, leaves it on CUTOFF. Learn off, the same set takes it.
#[cfg(feature = "midi-map")]
#[test]
fn a_set_under_learn_leaves_the_focus_learn_reads() {
    use chimera_core::ui::learn::Learn;
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut f = Fake::new();
    ed(&mut f, "ed learn FILTER.CUTOFF");
    assert!(matches!(f.ui.learn(), Learn::Armed { .. }));
    let cutoff = f.ui.focused_slot();
    ed_set(&mut f, "FILTER.RESO", "0.3");
    assert_eq!(f.ui.focused_slot(), cutoff, "learn binds CUTOFF still");
    assert!(matches!(f.ui.learn(), Learn::Armed { .. }));
    ed(&mut f, "ed learn off");
    ed_set(&mut f, "FILTER.RESO", "0.4");
    assert_ne!(f.ui.focused_slot(), cutoff);
}

/// MIX down: MIX + PLUS primes the focused cell, so the set leaves it.
#[test]
fn a_set_under_mix_leaves_the_focus_priming_reads() {
    let (mut f, _) = alg_page();
    feed(&mut f.ui, Input::held(ButtonId::Mix));
    ed_set(&mut f, "ALGO.MORPH", "100");
    assert_eq!(f.ui.focused_slot(), 0);
    feed(&mut f.ui, Input::release(ButtonId::Mix));
    ed_set(&mut f, "ALGO.MORPH", "90");
    assert_eq!(f.ui.focused_slot(), 2);
}
