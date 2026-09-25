//! Focus band tracking (UI refresh spec § Focus tracking): the last-touched
//! slot per page, slot a by default, no timer.

mod screen;

use chimera_core::ui::UiState;
use chimera_core::ui::block_def::ChainDef2;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::focus::{FocusMemory, MAX_PAGES};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed};

#[test]
fn every_page_starts_on_slot_a() {
    let ui = UiState::new();
    assert_eq!(ui.focused_slot(), 0);
    let m = FocusMemory::new();
    assert!((0..MAX_PAGES as u16).all(|id| m.get(id) == 0));
}

#[test]
fn the_first_tick_of_another_slot_moves_focus_and_it_stays() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    assert_eq!(ui.focused_slot(), 2);
    for _ in 0..50 {
        ui.update(); // no timer: focus does not fall back
    }
    assert_eq!(ui.focused_slot(), 2);
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(ui.focused_slot(), 1);
}

#[test]
fn focus_is_remembered_per_page() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // Pizza: C
    feed(&mut ui, Input::press(ButtonId::Plus)); // → Drive
    assert_eq!(ui.focused_slot(), 0, "a page not yet touched shows slot a");
    feed(&mut ui, Input::turn(EncoderId::B, 1)); // Drive: B
    feed(&mut ui, Input::press(ButtonId::Minus)); // ← Pizza
    assert_eq!(ui.focused_slot(), 2);
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert_eq!(ui.focused_slot(), 1);
}

#[test]
fn mixer_part_and_matrix_pages_use_the_same_mechanism() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::turn(EncoderId::D, -1)); // LEVEL
    assert_eq!(ui.focused_slot(), 3);
    feed(&mut ui, Input::press(ButtonId::B1)); // Part 1 chain, Pizza
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // → MOD
    }
    feed(&mut ui, Input::turn(EncoderId::E, 5)); // amount
    assert_eq!(ui.focused_slot(), 4);
}

#[test]
fn out_of_range_page_ids_are_ignored() {
    let mut m = FocusMemory::new();
    m.touch(MAX_PAGES as u16 + 3, 4);
    assert_eq!(m.get(MAX_PAGES as u16 + 3), 0);
    m.touch(1, 9);
    assert_eq!(m.get(1), 5, "slots clamp to f");
}

#[test]
fn every_page_id_fits_the_focus_table() {
    let chains: [&ChainDef2; 9] = [
        &reg::PIZZA_POLY_CHAIN,
        &reg::KICK_CHAIN,
        &reg::MODAL_PLUCK_CHAIN,
        &reg::FM_CHAIN,
        &reg::MIX_CHAIN,
        &reg::ENVELOPE_CHAIN,
        &reg::MIXER_CHANNEL_CHAIN,
        &reg::SYSTEM_CHAIN,
        &reg::DEMO_CHAIN,
    ];
    for chain in chains {
        for block in chain.blocks {
            for def in core::iter::once(block.def).chain(block.sub_pages.iter().copied()) {
                assert!((def.id as usize) < MAX_PAGES, "{} id {}", def.name, def.id);
            }
        }
    }
}

/// Turning an encoder over an empty slot (4opFM slot b) edits nothing, so it
/// does not take the focus: the focus band and the active cell stay put.
#[test]
fn an_empty_slot_does_not_take_focus() {
    use chimera_core::preset::ChainType;
    use screen::{Fb, W, load_init, settle};
    let render = |ui: &UiState| {
        let mut fb = Fb::new();
        ui.render_with_scope(
            &mut fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            &screen::scope_fixture(),
        );
        fb
    };
    let mut ui = UiState::new();
    load_init(&mut ui, ChainType::Fm);
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // LEVEL
    settle(&mut ui);
    let before = render(&ui);
    feed(&mut ui, Input::turn(EncoderId::B, 3)); // slot b: empty
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 2);
    let after = render(&ui);
    assert!(
        before.px[28 * W..118 * W] == after.px[28 * W..118 * W],
        "focus band unchanged"
    );
    assert!(
        before.px[186 * W..266 * W] == after.px[186 * W..266 * W],
        "cells unchanged"
    );
}
