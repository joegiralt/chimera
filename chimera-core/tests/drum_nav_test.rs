//! DRUM's chain (ADR 0265, slice 1 Task 8): KIT · MCH · FLT · AMP · MOD,
//! KEYS under KIT and MORE under MCH. MIX + MINUS / PLUS steps the pad on
//! the pad's pages only; MTX keeps it for deleting a route. A live hit
//! selects its pad.

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::drum::params::PadParams;
use chimera_core::dsp::drum::types::{Knob, KnobSet, Machine, PadId, PadSet};
use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry::{
    AMP, DRUM_CHAIN, DRUM_KEYS, DRUM_KIT, DRUM_MCH, DRUM_MORE, FILTER, MOD_MATRIX,
};
use chimera_core::ui::theme_settings::{PadFollow, ThemeSettings};
use chimera_core::ui::view::{self, View};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, load_init, page_def_id, settle, stack_ui, tap};

const P1: PartId = PartId::ALL[0];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).unwrap()
}

/// Part 1 on DRUM INIT, on KIT.
fn drum() -> UiState {
    let mut ui = stack_ui();
    load_init(&mut ui, EngineType::Drum);
    ui
}

fn on(ui: &UiState, def: &BlockDef) -> bool {
    page_def_id(ui) == def.id
}

fn mix(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::chord(ButtonId::Mix, b));
    feed(ui, Input::release(b));
    feed(ui, Input::release(ButtonId::Mix));
}

#[test]
fn the_chain_is_kit_mch_flt_amp_mod() {
    let map: Vec<_> = DRUM_CHAIN
        .blocks
        .iter()
        .map(|b| b.map.unwrap_or(b.def.short))
        .collect();
    assert_eq!(map, ["KIT", "MCH", "FLT", "AMP", "MOD"]);
    assert_eq!(DRUM_CHAIN.blocks[0].sub_pages[0].id, DRUM_KEYS.id);
    assert_eq!(DRUM_CHAIN.blocks[1].sub_pages[0].id, DRUM_MORE.id);
    assert_eq!((DRUM_CHAIN.home().node(), DRUM_CHAIN.home().sub()), (0, 0));
}

#[test]
fn minus_and_plus_walk_the_chain() {
    let mut ui = drum();
    assert!(on(&ui, &DRUM_KIT), "a load lands on KIT");
    for def in [&DRUM_MCH, &FILTER, &AMP, &MOD_MATRIX] {
        feed(&mut ui, Input::press(ButtonId::Plus));
        assert!(on(&ui, def), "PLUS to {}", def.short);
    }
    for def in [&AMP, &FILTER, &DRUM_MCH, &DRUM_KIT] {
        feed(&mut ui, Input::press(ButtonId::Minus));
        assert!(on(&ui, def), "MINUS to {}", def.short);
    }
}

#[test]
fn edit_opens_keys_under_kit_and_more_under_mch() {
    let mut ui = drum();
    tap(&mut ui, ButtonId::Edit);
    assert!(on(&ui, &DRUM_KEYS));
    tap(&mut ui, ButtonId::Seq);
    assert!(on(&ui, &DRUM_KIT));
    feed(&mut ui, Input::press(ButtonId::Plus));
    tap(&mut ui, ButtonId::Edit);
    assert!(on(&ui, &DRUM_MORE));
}

#[test]
fn mix_plus_steps_the_pad_and_wraps() {
    let mut ui = drum();
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert!(on(&ui, &DRUM_MCH));
    assert_eq!(ui.selected_pad(P1), pad(1));
    mix(&mut ui, ButtonId::Plus);
    assert_eq!(ui.selected_pad(P1), pad(2));
    assert!(on(&ui, &DRUM_MCH), "the page stays");
    ui.select_pad(P1, pad(12));
    mix(&mut ui, ButtonId::Plus);
    assert_eq!(ui.selected_pad(P1), pad(1), "12 wraps to 1");
    mix(&mut ui, ButtonId::Minus);
    assert_eq!(ui.selected_pad(P1), pad(12), "1 wraps to 12");
}

#[test]
fn mix_minus_and_plus_step_the_pad_on_every_pad_page() {
    let mut ui = drum();
    let pages: [(&BlockDef, &[ButtonId]); 4] = [
        (&DRUM_KIT, &[]),
        (&DRUM_KEYS, &[ButtonId::Edit]),
        (&DRUM_MCH, &[ButtonId::Seq, ButtonId::Plus]),
        (&DRUM_MORE, &[ButtonId::Edit]),
    ];
    for (def, keys) in pages {
        for &k in keys {
            match k {
                ButtonId::Plus => feed(&mut ui, Input::press(k)),
                _ => tap(&mut ui, k),
            }
        }
        assert!(on(&ui, def), "{}", def.short);
        let was = ui.selected_pad(P1);
        mix(&mut ui, ButtonId::Plus);
        assert_eq!(ui.selected_pad(P1), was.next(), "{}", def.short);
        mix(&mut ui, ButtonId::Minus);
        assert_eq!(ui.selected_pad(P1), was, "{}", def.short);
    }
}

/// FLT's cells on DRUM are the selected pad's (ADR 0269): MIX + PLUS
/// steps the pad there, as on every pad page, and FREQ turns that pad.
#[test]
fn mix_plus_on_flt_steps_the_pad() {
    let mut ui = drum();
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert!(on(&ui, &FILTER));
    feed(&mut ui, Input::turn(EncoderId::F, 1));
    mix(&mut ui, ButtonId::Plus);
    assert_eq!(ui.selected_pad(P1), pad(2));
    assert!(ui.prime_status().is_none(), "nothing primed");
    let cutoff = |ui: &UiState, n| ui.project().kit(P1).pad(pad(n)).sound().filter().cutoff();
    let before = cutoff(&ui, 1);
    feed(&mut ui, Input::turn(EncoderId::F, -3));
    assert!(cutoff(&ui, 2) < before, "FREQ turns pad 2");
    assert_eq!(cutoff(&ui, 1), before, "pad 1 is left");
}

/// AMP's cells on DRUM are the selected pad's (ADR 0269): a turn of LPG
/// moves pad 2's gate, never the Sound's.
#[test]
fn an_amp_turn_edits_the_pad() {
    let mut ui = drum();
    ui.select_pad(P1, pad(2));
    for _ in 0..3 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    assert!(on(&ui, &AMP));
    let sound = ui.project().part(P1).sound.params.out.gate;
    feed(&mut ui, Input::turn(EncoderId::B, 5));
    let lpg = |ui: &UiState, n| {
        ui.project()
            .kit(P1)
            .pad(pad(n))
            .sound()
            .amp()
            .gate()
            .lpg
            .get()
    };
    assert!(lpg(&ui, 2) > 0.0, "LPG turns pad 2");
    assert_eq!(lpg(&ui, 1), 0.0, "pad 1 is left");
    assert_eq!(
        ui.project().part(P1).sound.params.out.gate,
        sound,
        "the Sound is left"
    );
}

/// Review Focus 5: on MTX, MIX + MINUS deletes the route under the cursor
/// and leaves the pad.
#[test]
fn mix_minus_on_mtx_deletes_the_route_and_leaves_the_pad() {
    let mut ui = drum();
    ui.select_pad(P1, pad(3));
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    assert!(on(&ui, &MOD_MATRIX));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    feed(&mut ui, Input::turn(EncoderId::E, 20));
    let (row, col) = (ui.matrix_state.sel_row, ui.matrix_state.sel_col);
    assert!(ui.matrix_state.is_present(row, col), "a route to delete");
    mix(&mut ui, ButtonId::Minus);
    assert!(
        !ui.matrix_state.is_present(row, col),
        "MIX + MINUS deletes it"
    );
    assert_eq!(ui.selected_pad(P1), pad(3), "and leaves the pad");
}

#[test]
fn a_hit_on_the_shown_part_selects_its_pad() {
    let mut ui = drum();
    ui.pad_struck(P1, pad(9));
    assert_eq!(ui.selected_pad(P1), pad(9));
    // Another Part's hit leaves the shown Part's pad.
    ui.pad_struck(PartId::ALL[1], pad(4));
    assert_eq!(ui.selected_pad(P1), pad(9));
    assert_eq!(ui.selected_pad(PartId::ALL[1]), pad(1));
}

#[test]
fn encoder_a_on_the_pad_cell_picks_the_pad() {
    let mut ui = drum();
    feed(&mut ui, Input::turn(EncoderId::A, 4));
    assert_eq!(ui.selected_pad(P1), pad(5));
    feed(&mut ui, Input::turn(EncoderId::A, 20));
    assert_eq!(ui.selected_pad(P1), pad(12), "the turn stops at 12");
    feed(&mut ui, Input::turn(EncoderId::A, -30));
    assert_eq!(ui.selected_pad(P1), pad(1));
}

/// A new pad's cells start at its own values: nothing glides from the
/// last pad's.
#[test]
fn a_new_pad_reseeds_the_cells() {
    let mut ui = drum();
    feed(&mut ui, Input::press(ButtonId::Plus));
    settle(&mut ui);
    mix(&mut ui, ButtonId::Plus);
    let ctx = ui.slot_ctx_for_test();
    let kit = ui.project().kit(P1);
    let knobs = kit.pad(pad(2)).knobs();
    for (i, k) in Knob::MAIN.into_iter().enumerate() {
        if view::view(&DRUM_MCH, i, &ctx) == View::Empty {
            continue;
        }
        let want = f32::from(knobs.get(k).get().get()) / 127.0;
        let a = &ui.renderer.anim[i];
        assert!((a.current() - want).abs() < 1e-6, "{k:?}");
    }
}

#[test]
fn the_header_names_the_pad_and_its_machine() {
    let mut ui = drum();
    ui.select_pad(P1, pad(2));
    let h = ui.page_header();
    assert_eq!(h.context.as_str(), "PART 1 · 02 TONE SNARE");
    assert_eq!(h.name.as_str(), "KIT");
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert_eq!(ui.page_header().context.as_str(), "PART 1 · SOUND");
}

#[test]
fn pad_pages_cells() {
    let ui = drum();
    let ctx = ui.slot_ctx_for_test();
    let labels = |def: &BlockDef| -> Vec<&str> {
        (0..6).map(|i| view::view(def, i, &ctx).label()).collect()
    };
    assert_eq!(
        labels(&DRUM_KIT),
        ["PAD", "MACHINE", "LEVEL", "PAN", "TONAL", "--"]
    );
    assert_eq!(
        labels(&DRUM_KEYS),
        ["PAD", "CHOKE", "RETRIG", "--", "--", "--"]
    );
    // Pad 1 is TONE KICK.
    assert_eq!(
        labels(&DRUM_MCH),
        ["PITCH", "DECAY", "SWEEP", "SWEEP TIME", "CLICK", "DRIVE"]
    );
    assert_eq!(
        labels(&DRUM_MORE),
        ["MACHINE", "START", "HARMONICS", "--", "--", "--"]
    );
}

#[test]
fn the_pad_cells_hint_the_step() {
    let ui = drum();
    let ctx = ui.slot_ctx_for_test();
    for def in [&DRUM_KIT, &DRUM_KEYS, &DRUM_MCH, &DRUM_MORE] {
        let v = view::view(def, 0, &ctx);
        assert_eq!(
            v.hint(&ctx),
            view::PAD_HINT,
            "{}'s first cell hints MIX + -/+",
            def.short
        );
    }
}

/// A pad knob's default is its machine's, not the spec's 0.
#[test]
fn a_knob_resets_to_its_machine_default() {
    for m in Machine::ALL {
        for k in Knob::ALL {
            let want = f32::from(KnobSet::defaults(m).get(k).get().get());
            assert_eq!(
                PadParams::default_on(m, PadParams::knob(k)),
                Some(want),
                "{m:?} {k:?}"
            );
        }
    }
    assert_eq!(
        PadParams::default_on(Machine::ToneKick, PadParams::LEVEL),
        None
    );
    let _ = ParamAddr::new(BlockRef::Pad, PadParams::LEVEL);
}

#[test]
fn machine_index_is_its_place() {
    for (i, m) in Machine::ALL.into_iter().enumerate() {
        assert_eq!(m.index(), i);
    }
}

/// A hit with nothing sounding lights its pad until the next frame.
#[test]
fn a_hit_lights_its_pad_for_a_moment() {
    use chimera_core::ui::animation::UiTick;
    let mut ui = drum();
    ui.pad_struck(PartId::ALL[1], pad(4));
    assert!(ui.project().pad_lights(PartId::ALL[1]).is_lit(pad(4)));
    assert_eq!(
        ui.project().pad_lights(P1).lit(),
        PadSet::EMPTY,
        "only its Part's"
    );
    ui.update(UiTick::for_test());
    assert_eq!(ui.project().pad_lights(PartId::ALL[1]).lit(), PadSet::EMPTY);
}

fn follow(ui: &mut UiState, f: PadFollow) {
    ui.set_theme(ThemeSettings {
        pad_follow: f,
        ..ui.theme()
    });
}

/// PAD FOLLOW OFF: a hit lights its pad, never selects it.
#[test]
fn pad_follow_off_keeps_the_pad() {
    let mut ui = drum();
    follow(&mut ui, PadFollow::Off);
    ui.pad_struck(P1, pad(9));
    assert_eq!(ui.selected_pad(P1), pad(1));
    assert!(ui.project().pad_lights(P1).is_lit(pad(9)));
    mix(&mut ui, ButtonId::Plus);
    assert_eq!(ui.selected_pad(P1), pad(2), "MIX + PLUS still steps it");
}

/// PAD FOLLOW ALWAYS: a hit selects its pad, even mid-edit.
#[test]
fn pad_follow_always_follows_every_hit() {
    let mut ui = drum();
    follow(&mut ui, PadFollow::Always);
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    ui.pad_struck(P1, pad(9));
    assert_eq!(ui.selected_pad(P1), pad(9));
}

/// PAD FOLLOW IDLE, the default: a hit within 2 s of a turn on a DRUM page
/// leaves the pad; after, it follows.
#[test]
fn pad_follow_idle_waits_out_an_edit() {
    use chimera_core::ui::animation::{FRAME_MS, UiTick};
    let mut ui = drum();
    assert_eq!(ui.theme().pad_follow, PadFollow::Idle);
    ui.pad_struck(P1, pad(5));
    assert_eq!(ui.selected_pad(P1), pad(5), "untouched, it follows");
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    let frames = 2_000 / FRAME_MS;
    for _ in 1..frames {
        ui.update(UiTick::for_test());
    }
    ui.pad_struck(P1, pad(9));
    assert_eq!(ui.selected_pad(P1), pad(5), "inside 2 s of the turn");
    ui.update(UiTick::for_test());
    ui.pad_struck(P1, pad(9));
    assert_eq!(ui.selected_pad(P1), pad(9), "2 s on, it follows");
}

/// Each slice-1 machine has an icon: an instrument mark with ink and a
/// family letter.
#[test]
fn every_machine_has_an_icon() {
    use chimera_core::ui::kit_band::{Tone, family, instrument, letter, mark};
    for m in Machine::ALL {
        let mk = mark(instrument(m));
        let full = (0..12)
            .flat_map(|y| (0..14).map(move |x| (x, y)))
            .filter(|&(x, y)| mk.at(x, y) == Tone::Full)
            .count();
        assert!(full > 8, "{m:?}");
        let l = letter(family(m));
        assert!((0..5).any(|y| (0..3).any(|x| l.at(x, y))), "{m:?}");
    }
}

/// Frames to just short of PAD FOLLOW IDLE's 2 s.
fn almost_two_seconds(ui: &mut UiState) {
    use chimera_core::ui::animation::{FRAME_MS, UiTick};
    for _ in 1..2_000 / FRAME_MS {
        ui.update(UiTick::for_test());
    }
}

/// PAD FOLLOW IDLE: MIX + PLUS is an edit too, so a hit within 2 s of the
/// step leaves the pad.
#[test]
fn pad_follow_idle_waits_out_a_pad_step() {
    use chimera_core::ui::animation::UiTick;
    let mut ui = drum();
    ui.pad_struck(P1, pad(4));
    mix(&mut ui, ButtonId::Plus);
    assert_eq!(ui.selected_pad(P1), pad(5));
    almost_two_seconds(&mut ui);
    ui.pad_struck(P1, pad(1));
    assert_eq!(ui.selected_pad(P1), pad(5), "inside 2 s of the step");
    ui.update(UiTick::for_test());
    ui.pad_struck(P1, pad(1));
    assert_eq!(ui.selected_pad(P1), pad(1), "2 s on, it follows");
}

/// PAD FOLLOW IDLE: a turn off the DRUM Part's own pages (its MIXER
/// channel) holds nothing.
#[test]
fn pad_follow_idle_ignores_a_turn_on_the_mixer() {
    use chimera_core::ui::nav::{Location, MixPage};
    let mut ui = drum();
    ui.go_for_test(Location::mixer(P1, MixPage::Sends));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    ui.pad_struck(P1, pad(9));
    assert_eq!(ui.selected_pad(P1), pad(9));
}

/// PAD FOLLOW IDLE: the hold is per Part; an edit on Part 1 leaves Part
/// 2's hits following.
#[test]
fn pad_follow_idle_holds_per_part() {
    use chimera_core::project::{PartFrom, PartSource};
    use chimera_core::ui::nav::Location;
    let p2 = PartId::ALL[1];
    let mut ui = drum();
    ui.load_for_test(PartSource {
        part: p2,
        from: PartFrom::Init(EngineType::Drum),
    });
    let home = DRUM_CHAIN.home();
    ui.go_for_test(Location::pages(P1, home));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    ui.go_for_test(Location::pages(p2, home));
    ui.pad_struck(p2, pad(7));
    assert_eq!(ui.selected_pad(p2), pad(7), "Part 2 not held");
    ui.go_for_test(Location::pages(P1, home));
    ui.pad_struck(P1, pad(7));
    assert_eq!(ui.selected_pad(P1), pad(1), "Part 1 still held");
}
