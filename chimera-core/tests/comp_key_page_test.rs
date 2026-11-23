// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! KEY's page (ADR 0303): MIXER › MST's cell E, MAKEUP on MST › LVL's
//! cell C with everything of it but its place unchanged; KEY's hint on
//! MST's status line and its name under the curve.

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{Block, ParamId, ValFmt};
use chimera_core::comp_key::{CompKey, KEY_NAMES, KEYS};
use chimera_core::dsp::comp::{COMP_SPECS, CompParams, Ratio};
use chimera_core::nrpn::NrpnAddr;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, SlotBinding};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::hint;
use chimera_core::ui::notes::key_hint;
use chimera_core::ui::theme;
use chimera_core::ui::viz::{PLOT_BASE, PLOT_TOP};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Fb, Input, feed};

/// MIXER › MST.
fn mst() -> UiState {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = screen::stack_ui();
    let master = reg::MIXER_CHANNEL_CHAIN
        .blocks
        .iter()
        .position(|b| b.def.id == reg::MASTER.id)
        .unwrap();
    screen::to_fx(&mut ui, master);
    assert_eq!(ui.page_def().id, reg::MASTER.id);
    ui
}

/// The status line's lit pixels: between the header and the plot.
fn status_px(shot: &Fb) -> usize {
    let mut n = 0;
    for y in theme::HEADER_BOTTOM..PLOT_TOP {
        for x in 0..240 {
            if shot.at(x, y) != theme::BG {
                n += 1;
            }
        }
    }
    n
}

/// MID pixels in the strip under the curve where KEY's name is drawn.
fn name_px(shot: &Fb) -> usize {
    let mut n = 0;
    for y in PLOT_BASE - 14..PLOT_BASE - 2 {
        for x in 100..theme::VIZ_RIGHT - 30 {
            if shot.at(x, y) == theme::MID {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn key_is_msts_fifth_cell_and_reads_its_choice() {
    let mut ui = mst();
    assert_eq!(ui.project().perf().fx.comp.key, CompKey::Full);
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    let full = name_px(&shot);
    assert!(full > 0, "FULL is named under the curve");
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(ui.project().perf().fx.comp.key, CompKey::Low);
    feed(&mut ui, Input::turn(EncoderId::E, 2));
    assert_eq!(ui.project().perf().fx.comp.key, CompKey::Hit);
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(
        ui.project().perf().fx.comp.key,
        CompKey::Part(PartId::ALL[0])
    );
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("comp_key_part_1");
    assert!(name_px(&shot) > full, "PART 1 is named under the curve");
    // Past the last Part the knob stops.
    feed(&mut ui, Input::turn(EncoderId::E, 20));
    assert_eq!(
        ui.project().perf().fx.comp.key,
        CompKey::Part(PartId::ALL[5])
    );
    // The cell reads the approved names.
    let ValFmt::Names(names) = COMP_SPECS[10].fmt else {
        panic!("KEY is a choice of names");
    };
    assert_eq!(names, &KEY_NAMES);
    assert_eq!(COMP_SPECS[10].label, "KEY");
}

/// KEY's cell touched, the status line says what its choice listens to;
/// another cell touched, it says nothing.
#[test]
fn keys_hint_shows_while_its_cell_is_touched() {
    let mut ui = mst();
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    screen::settle(&mut ui);
    let quiet = status_px(&screen::render_ui(&ui));
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("comp_key_low_hint");
    #[cfg(feature = "tape")]
    assert!(status_px(&shot) > quiet + 200, "LOW's hint is drawn");
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    screen::settle(&mut ui);
    assert_eq!(status_px(&screen::render_ui(&ui)), quiet);
}

/// Each choice's words, and each line fits the status line.
#[test]
fn every_key_says_what_it_listens_to() {
    let says = |k: CompKey| {
        let h = key_hint(k);
        assert_eq!(h.len(), 1);
        assert!(
            hint::width(h) <= theme::CONTENT_W,
            "{k:?}: {} px",
            hint::width(h)
        );
        (h[0].0, h[0].1)
    };
    assert_eq!(says(CompKey::Full), ("FULL", "HEARS THE WHOLE MIX"));
    assert_eq!(says(CompKey::Low), ("LOW", "THE KICK PUMPS THE MIX"));
    assert_eq!(says(CompKey::High), ("HIGH", "HEARS ALL BUT THE BASS"));
    assert_eq!(says(CompKey::Hit), ("HIT", "EVERY HIT PUSHES IT ALIKE"));
    for (i, p) in PartId::ALL.into_iter().enumerate() {
        let (name, does) = says(CompKey::Part(p));
        assert_eq!(name, format!("PART {}", i + 1));
        assert_eq!(does, "DUCKS ALL BUT ITSELF");
    }
}

/// MAKEUP moved cells and nothing else: its id, ident, range, default,
/// glyph and NRPN address are the ones it had on MST.
#[test]
fn makeup_keeps_everything_but_its_cell() {
    assert_eq!(CompParams::MAKEUP, ParamId(4));
    let spec = &COMP_SPECS[4];
    assert_eq!(
        (spec.id, spec.label, spec.ident),
        (ParamId(4), "MAKEUP", "MAKEUP")
    );
    assert_eq!((spec.min, spec.max, spec.default), (0.0, 1.0, 0.0));
    let addr = ParamAddr::new(BlockRef::Comp, CompParams::MAKEUP);
    let n = NrpnAddr::of(addr).unwrap().get();
    assert_eq!((n.msb().get(), n.lsb().get()), (24, 4));
    let slot = |def: &BlockDef, i: usize| match def.params[i].binding {
        SlotBinding::Param(a) => Some(a),
        _ => None,
    };
    assert_eq!(slot(&reg::MASTER_LEVEL, 2), Some(addr));
    assert!((0..6).all(|i| slot(&reg::MASTER, i) != Some(addr)));
    assert_eq!(
        slot(&reg::MASTER, 4),
        Some(ParamAddr::new(BlockRef::Comp, CompParams::KEY))
    );
    // Stored as it was: 0..1, 24 dB at the top.
    let mut p = CompParams::DEFAULT;
    p.write(CompParams::MAKEUP, 0.5);
    assert_eq!((p.makeup, p.makeup_db()), (0.5, 12.0));
    assert_eq!(p.get(CompParams::MAKEUP), 0.5);
    // On LVL's cell C it turns as it did.
    let mut ui = mst();
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, reg::MASTER_LEVEL.id);
    assert_eq!(ui.project().perf().fx.comp.makeup, 0.0);
    feed(&mut ui, Input::turn(EncoderId::C, 8));
    assert!(ui.project().perf().fx.comp.makeup > 0.0);
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("comp_key_level");
    assert_eq!(reg::MASTER_LEVEL.params[2].label(), "MAKEUP");
}

/// RATIO's last step reads MAX, and KEY and RATIO are what they store.
#[test]
fn ratio_ends_at_max() {
    let mut ui = mst();
    feed(&mut ui, Input::turn(EncoderId::B, 20));
    assert_eq!(ui.project().perf().fx.comp.ratio, Ratio::MAX);
    let ValFmt::Names(names) = COMP_SPECS[1].fmt else {
        panic!("RATIO is a choice of names");
    };
    let (last, before) = names.split_last().unwrap();
    assert_eq!((*last, Ratio::MAX.ident()), ("MAX", "MAX"));
    assert_eq!(
        before,
        ["1:1", "1.5:1", "2:1", "3:1", "4:1", "6:1", "10:1", "20:1"]
    );
    // The names are the table's idents, index for index.
    assert_eq!(names, &Ratio::ALL.map(Ratio::ident));
    let mut p = CompParams::DEFAULT;
    assert!(p.set_enum_code(CompParams::RATIO, Ratio::MAX.code()));
    assert_eq!(p.enum_ident(CompParams::RATIO), Some("MAX"));
    assert!(!p.set_enum_code(CompParams::RATIO, Ratio::MAX.code() + 1));
    assert_eq!(p.ratio, Ratio::MAX);
    for k in CompKey::ALL {
        assert!(p.set_enum_code(CompParams::KEY, k.code()));
        assert_eq!(p.key, k);
        assert_eq!(p.enum_code(CompParams::KEY), Some(k.code()));
        assert_eq!(p.enum_ident(CompParams::KEY), Some(k.ident()));
        assert_eq!(p.get(CompParams::KEY), f32::from(k.code()));
    }
    assert!(!p.set_enum_code(CompParams::KEY, KEYS as u8));
}

/// #1066: KEY's hint on the status line clears the curve's OUT label: three
/// blank rows between the two over OUT's columns, for every choice.
#[cfg(feature = "tape")]
#[test]
fn keys_hint_clears_the_out_label() {
    let x0 = theme::VIZ_LEFT + 30 - 20;
    let inked = |shot: &Fb, y: i32| (x0..x0 + 20).any(|x| shot.at(x, y) != theme::BG);
    let mut ui = mst();
    for turn in [1, 1, 1, 1, 1] {
        feed(&mut ui, Input::turn(EncoderId::E, turn));
        screen::settle(&mut ui);
        let shot = screen::render_ui(&ui);
        let key = ui.project().perf().fx.comp.key;
        // The hint's last inked row, then OUT's first under it.
        let rows: Vec<i32> = (theme::HEADER_BOTTOM..PLOT_TOP + 12)
            .filter(|&y| inked(&shot, y))
            .collect();
        let gap = rows.windows(2).any(|w| w[1] > w[0] + 3);
        if !gap {
            shot.dump("comp_key_hint_out");
        }
        assert!(gap, "{key:?}: the hint runs into OUT, rows {rows:?}");
    }
}
