// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's voice groups (ADR 0280, #993): a kit has at most 8, each choke
//! group in use once and each pad with choke OFF once; a choke edit that
//! would make a 9th is refused on the cell with its reason; a STACK pad
//! that needs room ends its own oldest voice, never another pad's; an old
//! kit over 8 loads and plays as before, the page showing its count.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test that publishes holds the one writer.

mod common;
mod screen;

use std::sync::{Mutex, MutexGuard, OnceLock};

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::drum::groups::VoiceGroups;
use chimera_core::dsp::drum::params::PadParams;
use chimera_core::dsp::drum::types::{ChokeGroup, Kit, PadId};
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::params::EngineType;
use chimera_core::project::{PartId, Project};
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::held::Reason;
use chimera_core::ui::{UiState, draw, theme, view};
use chimera_hal::{ButtonId, EncoderId};
use common::drum::{PLAY_BUDGET, drum_hits, drum_rig};
use screen::{Input, feed, load_init, tap};

fn writer() -> MutexGuard<'static, KitWriter> {
    static W: OnceLock<Mutex<KitWriter>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(KitWriter::take().expect("the kit writer, once")))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

const P1: PartId = PartId::ALL[0];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).unwrap()
}

/// INIT DRUM with every choke OFF: twelve groups, a kit from before
/// the bound.
fn all_off() -> Kit {
    let mut k = Kit::DEFAULT;
    for p in PadId::ALL {
        k.pad_mut(p).set_choke(ChokeGroup::Off);
    }
    k
}

// ---------------------------------------------------------------------------
// The count
// ---------------------------------------------------------------------------

#[test]
fn init_drum_has_exactly_eight_groups() {
    let g = VoiceGroups::of(&Kit::DEFAULT);
    assert_eq!(g.get(), 8);
    assert_eq!(g, VoiceGroups::MAX);
    assert!(!g.over());
}

#[test]
fn a_group_counts_once_and_an_off_pad_once() {
    assert_eq!(VoiceGroups::of(&all_off()).get(), 12);
    let mut k = Kit::DEFAULT;
    for p in PadId::ALL {
        k.pad_mut(p).set_choke(ChokeGroup::A);
    }
    assert_eq!(VoiceGroups::of(&k).get(), 1);
    k.pad_mut(pad(1)).set_choke(ChokeGroup::Off);
    k.pad_mut(pad(2)).set_choke(ChokeGroup::B);
    assert_eq!(VoiceGroups::of(&k).get(), 3);
    assert!(VoiceGroups::of(&all_off()).over());
}

/// Pad 3 leaving A (pad 4 keeps it) for OFF makes a 9th: refused, the
/// count it would make. Pad 1 (OFF) joining A makes 7; pad 4 moving A to
/// B keeps 8 (pad 3 keeps A).
#[test]
fn a_choke_edit_past_eight_is_refused() {
    let k = Kit::DEFAULT;
    let ninth = VoiceGroups::choke(&k, pad(3), ChokeGroup::Off).unwrap_err();
    assert_eq!(ninth.get(), 9);
    assert_eq!(
        VoiceGroups::choke(&k, pad(1), ChokeGroup::A).map(VoiceGroups::get),
        Ok(7)
    );
    assert_eq!(
        VoiceGroups::choke(&k, pad(4), ChokeGroup::B).map(VoiceGroups::get),
        Ok(8)
    );
}

/// An old kit over 8: an edit that lowers or keeps the count lands, one
/// that raises it is refused.
#[test]
fn an_old_kit_over_eight_may_only_come_down() {
    let mut k = all_off();
    let to_a = VoiceGroups::choke(&k, pad(1), ChokeGroup::A).unwrap();
    assert_eq!(to_a.get(), 12, "A alone: one group for another");
    k.pad_mut(pad(1)).set_choke(ChokeGroup::A);
    assert_eq!(
        VoiceGroups::choke(&k, pad(2), ChokeGroup::A).map(VoiceGroups::get),
        Ok(11)
    );
    k.pad_mut(pad(2)).set_choke(ChokeGroup::A);
    assert_eq!(
        VoiceGroups::choke(&k, pad(2), ChokeGroup::Off).map_err(VoiceGroups::get),
        Err(12)
    );
}

// ---------------------------------------------------------------------------
// The allocator
// ---------------------------------------------------------------------------

/// The MIDI key INIT DRUM's pad `p` answers to.
fn key(p: PadId) -> u8 {
    p.index() as u8
}

/// Part 1's live (not dying) voices: each one's pad.
fn live(r: &common::InstRig) -> Vec<(usize, PadId)> {
    let slots = r.inst.allocator().slots();
    drum_hits(&r.inst)
        .filter(|&(v, _)| slots[v].part() == Some(PartId::ALL[0]) && !slots[v].dying())
        .map(|(v, h)| (v, h.pad()))
        .collect()
}

/// A SNARE, then a STACK KICK struck until it fills the pool, then once
/// more: the KICK ends its own oldest voice, never the SNARE's, older.
#[test]
fn a_stack_pad_ends_its_own_oldest_voice_not_another_pads() {
    let mut w = writer();
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    let mut r = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    r.note_on_vel(0, key(pad(2)), 127);
    r.render(1);
    for _ in 1..chimera_core::hw::MAX_VOICES {
        r.note_on_vel(0, key(pad(1)), 127);
        r.render(1);
    }
    let before = live(&r);
    assert_eq!(before.len(), chimera_core::hw::MAX_VOICES, "the pool full");
    let snare = before.iter().find(|&&(_, p)| p == pad(2)).unwrap().0;
    let oldest_kick = r
        .inst
        .allocator()
        .slots()
        .iter()
        .enumerate()
        .filter(|&(v, _)| before.contains(&(v, pad(1))))
        .min_by_key(|(_, s)| s.age())
        .unwrap()
        .0;
    r.note_on_vel(0, key(pad(1)), 127);
    assert_eq!(r.inst.allocator().refused(), 0);
    let slots = r.inst.allocator().slots();
    assert!(!slots[snare].dying(), "the SNARE keeps its voice");
    assert_eq!(slots[snare].note().map(|n| n.get()), Some(key(pad(2))));
    assert_eq!(
        slots[oldest_kick].note().map(|n| n.get()),
        Some(key(pad(1))),
        "the KICK took its own oldest voice"
    );
    r.render(4);
    assert!(
        live(&r).contains(&(snare, pad(2))),
        "the SNARE still sounds"
    );
}

/// A STACK pad whose own voice frees too little for its hit, beside
/// another pad's tail that would free enough: the hit is refused and
/// neither voice is taken (ADR 0280). Pad 1, STACK, rings a KICK; pad 11
/// rings a COWBELL; pad 1 is then made a COWBELL too, so its next hit
/// costs a COWBELL while its own KICK frees a KICK's worth. The budget
/// leaves room only for a steal of the COWBELL's size. Both COWBELLs at
/// full BUMP, billed their sweep head above a KICK (ADR 0287).
#[test]
fn a_stack_pad_refuses_where_only_another_pads_voice_would_fit() {
    use chimera_core::dsp::drum::types::{Knob, KnobValue, Machine, Ticking};
    use chimera_core::hw::SampleBudget;
    let bump = KnobValue::new(chimera_midi::value::U7::MAX);
    let mut w = writer();
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    k.pad_mut(pad(11)).set_machine(Machine::ToneCowbell.into());
    k.pad_mut(pad(11)).knobs_mut().set(Knob::H, bump);
    let mut k2 = k;
    k2.pad_mut(pad(1)).set_machine(Machine::ToneCowbell.into());
    k2.pad_mut(pad(1)).knobs_mut().set(Knob::H, bump);
    // Pad 11's tail, then pad 1's KICK, then pad 1 a COWBELL.
    let play = |r: &mut common::InstRig, w: &mut KitWriter| {
        r.note_on_vel(0, key(pad(11)), 127);
        r.render(1);
        r.note_off(0, key(pad(11)));
        r.note_on_vel(0, key(pad(1)), 127);
        r.render(1);
        w.publish(P1, &k2);
        r.render(1);
    };
    // A first pass on a wide budget measures the pool and the reserve.
    let mut probe = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    play(&mut probe, &mut w);
    let a = probe.inst.allocator();
    let pool: u32 = a
        .slots()
        .iter()
        .filter(|s| !s.is_free() && !s.dying())
        .map(|s| s.cost().0)
        .sum();
    let booked = |pct: u8| u32::from(pct) * a.budget().as_cost().0 / 100;
    let reserve = booked(probe.inst.cost_pct()) - a.sounding_cost().0;
    let (kick, cowbell) = (Machine::ToneKick.cost().0, Ticking::CowbellSwept.cost().0);
    // Free room under the COWBELL hit by midway between a KICK and a
    // COWBELL: the KICK's steal doesn't fit, the COWBELL's would.
    let budget = reserve + pool + cowbell - (kick + cowbell) / 2;
    let mut r = drum_rig(&mut w, SampleBudget::for_cpu(budget * 64_000), &k, &k);
    assert_eq!(r.inst.allocator().budget().as_cost().0, budget);
    play(&mut r, &mut w);
    let before = live(&r);
    assert_eq!(before.len(), 2, "pad 11's tail and pad 1's KICK");
    let a = r.inst.allocator();
    let any = a.pick(
        P1,
        chimera_core::part::PartMode::Poly,
        chimera_core::MidiNote::new(key(pad(1))).unwrap(),
        chimera_core::hw::Cost(cowbell),
        chimera_core::hw::Cost(reserve),
    );
    let tail = before.iter().find(|&&(_, p)| p == pad(11)).unwrap().0;
    assert_eq!(
        any.map(|v| v.index()),
        Some(tail),
        "rule 3 across pads would take pad 11's"
    );
    let was = a.refused();
    r.note_on_vel(0, key(pad(1)), 127);
    assert_eq!(r.inst.allocator().refused(), was + 1, "refused");
    assert_eq!(live(&r), before, "neither voice taken");
}

/// INIT DRUM's 8 groups, every pad CUT, struck round the kit 16ths apart
/// three times over with the pool to itself: no hit is refused and none
/// ends a pad outside its own choke group.
#[test]
fn eight_groups_play_without_taking_each_others_voices() {
    let mut w = writer();
    let k = Kit::DEFAULT;
    let mut r = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    let gap = (0.054 * common::SR as f32) as usize / chimera_hal::BLOCK_SIZE;
    for i in 0..36 {
        let p = PadId::ALL[i % 12];
        let before = live(&r);
        r.note_on_vel(0, key(p), 110);
        let choke = k.pad(p).choke();
        let after = live(&r);
        for &(v, q) in &before {
            let mine = q == p || (choke != ChokeGroup::Off && k.pad(q).choke() == choke);
            if !mine {
                assert!(
                    after.contains(&(v, q)),
                    "hit {i} (pad {}) ended pad {}",
                    p.number(),
                    q.number()
                );
            }
        }
        r.render(1);
        r.note_off(0, key(p));
        r.render(gap - 1);
    }
    assert_eq!(r.inst.allocator().refused(), 0);
}

// ---------------------------------------------------------------------------
// An old kit over 8
// ---------------------------------------------------------------------------

/// A project whose Part 1 kit has every choke OFF saves and loads as it
/// is, Saved, its count 12.
#[test]
fn an_old_kit_over_eight_loads_as_it_is() {
    let (mut p, _) = Project::boxed();
    *p.kit_mut(P1) = all_off();
    let f = common::project::encode(&p);
    let (mut q, _) = Project::boxed();
    common::project::decode(&f, &mut q).unwrap();
    assert_eq!(*q.kit(P1), all_off());
    assert_eq!(VoiceGroups::of(q.kit(P1)).get(), 12);
}

/// The old kit's twelve pads struck a block apart: every hit books a
/// voice, as before the bound (a pad with no voice of its own steals as
/// rule 3, its hit starting once the stolen voice has faded).
#[test]
fn an_old_kit_over_eight_plays_every_hit() {
    let mut w = writer();
    let k = all_off();
    let mut r = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    for p in PadId::ALL {
        r.note_on_vel(0, key(p), 120);
        let booked = r.inst.allocator().slots().iter().any(|s| {
            s.part() == Some(PartId::ALL[0])
                && !s.dying()
                && s.note().map(|n| n.get()) == Some(key(p))
        });
        assert!(booked, "pad {} books a voice", p.number());
        r.render(1);
    }
    assert_eq!(r.inst.allocator().refused(), 0);
}

// ---------------------------------------------------------------------------
// The KEYS page
// ---------------------------------------------------------------------------

fn choke_addr() -> ParamAddr {
    ParamAddr::new(BlockRef::Pad, PadParams::CHOKE)
}

/// Part 1 on INIT DRUM's KEYS page, pad `n` selected, CHOKE touched.
fn keys_on(n: u8, kit: &Kit) -> UiState {
    let mut ui = UiState::new();
    // EDIT steps down to KEYS under STACK, as the goldens are recorded.
    screen::stack_map(&mut ui);
    load_init(&mut ui, EngineType::Drum);
    *ui.project_mut().kit_mut(P1) = *kit;
    // PAD, KIT's first cell, turned to pad `n` (as `drum_keys`' golden).
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    feed(&mut ui, Input::turn(EncoderId::A, n as i8 - 2));
    assert_eq!(ui.selected_pad(P1), pad(n));
    tap(&mut ui, ButtonId::Edit);
    assert_eq!(
        ui.page_def().id,
        chimera_core::ui::block_registry::DRUM_KEYS.id
    );
    // CHOKE is the second cell: touch it without moving it.
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    ui.step_toast(60_000);
    ui
}

fn choke(ui: &UiState, n: u8) -> ChokeGroup {
    ui.project().kit(P1).pad(pad(n)).choke()
}

fn fits_band(s: &str) {
    let w = draw::text_width(&theme::FONT_LABEL, s, 0);
    assert!(w <= theme::CONTENT_W, "{s}: {w} px");
}

#[test]
fn the_choke_cell_shows_the_count() {
    let ui = keys_on(3, &Kit::DEFAULT);
    let note = view::groups_note(choke_addr(), ui.project().kit(P1)).unwrap();
    assert_eq!(note.as_str(), "GROUPS 8/8");
    let other = ParamAddr::new(BlockRef::Pad, PadParams::RETRIG);
    assert!(view::groups_note(other, ui.project().kit(P1)).is_none());
    let old = keys_on(1, &all_off());
    let note = view::groups_note(choke_addr(), old.project().kit(P1)).unwrap();
    assert_eq!(
        note.as_str(),
        "GROUPS 12/8",
        "over 8 until the player fixes it"
    );
}

/// Pad 3 (A, with pad 4) turned to OFF would make a 9th group: the turn
/// lands on OFF dimmed with the reason, nothing written; on to A again,
/// nothing held.
#[test]
fn a_ninth_group_is_held_with_its_reason() {
    let mut ui = keys_on(3, &Kit::DEFAULT);
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(choke(&ui, 3), ChokeGroup::A, "nothing written");
    let shown = ui.held_choice().expect("OFF held");
    assert_eq!(shown.held.code(), ChokeGroup::Off.index() as u8);
    assert!(matches!(shown.why, Reason::Groups(g) if g.get() == 9));
    let why = chimera_core::ui::held::reason(shown.why);
    assert_eq!(why.as_str(), "9 GROUPS · KIT HOLDS 8");
    fits_band(why.as_str());
    let shot = screen::render_ui(&ui);
    assert!(screen::cell_reads(&shot, 1, "OFF"), "the held OFF shows");
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert!(ui.held_choice().is_none());
    assert_eq!(choke(&ui, 3), ChokeGroup::A);
    // On to B: pad 3 leaves A to pad 4 and joins B, still 8.
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 3), ChokeGroup::B);
    assert!(ui.held_choice().is_none());
}

#[test]
fn a_choke_edit_within_eight_lands() {
    let mut ui = keys_on(1, &Kit::DEFAULT);
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 1), ChokeGroup::A);
    assert!(ui.held_choice().is_none());
    assert_eq!(VoiceGroups::of(ui.project().kit(P1)).get(), 7);
}

/// A CC or the console's `ed set` onto a 9th group: refused with the
/// reason as a toast, never silently.
#[test]
fn a_set_onto_a_ninth_group_is_refused_aloud() {
    let mut ui = keys_on(3, &Kit::DEFAULT);
    assert!(!ui.edit_to(P1, choke_addr(), 0.0), "nothing moved");
    assert_eq!(choke(&ui, 3), ChokeGroup::A);
    match ui.step_toast(0) {
        ToastStep::Show(l) => assert_eq!(l.as_str(), "9 GROUPS · KIT HOLDS 8"),
        s => panic!("no toast: {s:?}"),
    }
    // Within the bound it lands.
    ui.select_pad(P1, pad(1));
    assert!(ui.edit_to(P1, choke_addr(), 1.0));
    assert_eq!(choke(&ui, 1), ChokeGroup::A);
}

/// A controller sweep onto a 9th group: each message is refused, the
/// kit and its revision untouched (the Part doesn't read Edited), and the
/// reason is said once while its toast is up, not again per message.
#[test]
fn a_controller_sweep_onto_a_ninth_group_is_refused_once() {
    let mut ui = keys_on(3, &Kit::DEFAULT);
    let rev = ui.project().rev();
    assert!(!ui.edit_to(P1, choke_addr(), 0.0));
    assert!(!ui.edit_addr(P1, choke_addr(), -1, false));
    assert_eq!(ui.project().rev(), rev, "a refused edit is no edit");
    assert_eq!(choke(&ui, 3), ChokeGroup::A);
    assert!(matches!(ui.step_toast(0), ToastStep::Show(_)));
    assert!(matches!(ui.step_toast(700), ToastStep::Show(_)));
    for _ in 0..8 {
        assert!(!ui.edit_to(P1, choke_addr(), 0.0));
    }
    assert_eq!(ui.project().rev(), rev);
    // Not shown again: its 1.2 s ran on from the first.
    assert_eq!(ui.step_toast(700), ToastStep::Ended);
    // Once it is down, a new refusal is said again.
    assert!(!ui.edit_to(P1, choke_addr(), 0.0));
    assert!(matches!(ui.step_toast(0), ToastStep::Show(_)));
}

/// A={1,2,3}, C={4,5}, D={6,7}, pads 8–12 OFF: 8 groups.
fn spread() -> Kit {
    use ChokeGroup::{A, C, D, Off};
    let mut k = Kit::DEFAULT;
    for (n, c) in [A, A, A, C, C, D, D, Off, Off, Off, Off, Off]
        .into_iter()
        .enumerate()
    {
        k.pad_mut(pad(n as u8 + 1)).set_choke(c);
    }
    assert_eq!(VoiceGroups::of(&k).get(), 8);
    k
}

/// A relative CC's step onto a 9th group (pad 1, A → B) skips on to the
/// next group that fits its way (C), as the encoder's held turn can: it
/// lands, with no toast.
#[test]
fn a_cc_step_skips_a_ninth_group_to_the_next_that_fits() {
    let mut ui = keys_on(1, &spread());
    assert!(ui.edit_addr(P1, choke_addr(), 1, false));
    assert_eq!(choke(&ui, 1), ChokeGroup::C, "B skipped");
    assert_eq!(ui.step_toast(0), ToastStep::Idle, "no refusal said");
    // Back down: C → B refused, on to A, which fits.
    assert!(ui.edit_addr(P1, choke_addr(), -1, false));
    assert_eq!(choke(&ui, 1), ChokeGroup::A);
}

/// A step with nothing that fits its way (pad 1, A → OFF, the end) is
/// refused and said.
#[test]
fn a_cc_step_with_nothing_that_fits_is_refused_aloud() {
    let mut ui = keys_on(1, &spread());
    let rev = ui.project().rev();
    assert!(!ui.edit_addr(P1, choke_addr(), -1, false));
    assert_eq!(choke(&ui, 1), ChokeGroup::A);
    assert_eq!(ui.project().rev(), rev);
    match ui.step_toast(0) {
        ToastStep::Show(l) => assert_eq!(l.as_str(), "9 GROUPS · KIT HOLDS 8"),
        s => panic!("no toast: {s:?}"),
    }
}

/// A stepped CC's ramp from A up through B (refused) to D: each bin a
/// step; it never says a refusal for a group it passes, and ends on D.
#[test]
fn a_cc_ramp_through_a_ninth_group_says_nothing() {
    let mut ui = keys_on(1, &spread());
    for _ in 0..3 {
        ui.edit_addr(P1, choke_addr(), 1, false);
        assert_eq!(ui.step_toast(0), ToastStep::Idle);
    }
    assert_eq!(choke(&ui, 1), ChokeGroup::D);
}

/// A held CHOKE is dropped once the kit's chokes change under it: a CC
/// on another pad's choke, or a kit loaded in with other chokes. The
/// next turn starts from the pad's own group.
#[test]
fn a_held_choke_is_dropped_when_the_kit_changes() {
    let mut ui = keys_on(3, &Kit::DEFAULT);
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert!(ui.held_choice().is_some(), "OFF held");
    // Pad 12 (OFF) moved into D, as a CC on its cell does: the kit
    // changed.
    let p12 = pad(12);
    {
        let k = ui.project_mut().kit_mut(P1);
        k.pad_mut(p12).set_choke(ChokeGroup::D);
    }
    assert!(ui.held_choice().is_none(), "the kit changed: dropped");
    // A turn starts from pad 3's own A, not the held OFF: on to B.
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 3), ChokeGroup::B);

    // Held again, then a kit loaded in.
    let mut ui = keys_on(3, &Kit::DEFAULT);
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert!(ui.held_choice().is_some());
    *ui.project_mut().kit_mut(P1) = spread();
    assert!(ui.held_choice().is_none(), "a kit loaded: dropped");
}

/// An expression pedal streaming to Part 2 moves the project's revision
/// every message; it leaves Part 1's kit alone, so a held CHOKE stays
/// and the next turn goes on past it (pad 1: A, B held, on to C). CC 11
/// reaches a Part through MIDI's map.
#[cfg(feature = "midi-map")]
#[test]
fn a_pedal_on_another_part_keeps_the_held_choke() {
    let mut ui = keys_on(1, &spread());
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 1), ChokeGroup::A);
    assert!(ui.held_choice().is_some(), "B held");
    use chimera_core::midi_in::CtrlEvent;
    use chimera_midi::ctl::{Ctl, FreeCc};
    use chimera_midi::value::U7;
    let ch = ui.project().part(PartId::ALL[1]).mix.channel;
    assert_ne!(ch, ui.project().part(P1).mix.channel, "Part 2 its own");
    let rev = ui.project().rev();
    for (i, v) in [100u8, 90, 80, 70].into_iter().enumerate() {
        let pedal = CtrlEvent::Ctl(
            ch,
            Ctl::Cc {
                cc: FreeCc::new(11).unwrap(),
                value: U7::new(v).unwrap(),
            },
        );
        ui.hear_ccs(
            [pedal].into_iter(),
            chimera_hal::Ms(2_000 + 4 * i as u32),
            true,
        );
    }
    assert_eq!(ui.project().part(PartId::ALL[1]).expression(), 70);
    assert_ne!(ui.project().rev(), rev, "the pedal moved the revision");
    assert!(ui.held_choice().is_some(), "still held");
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 1), ChokeGroup::C, "on past B");
}

#[test]
fn a_choke_steps_saturating() {
    use ChokeGroup::{A, B, D, Off};
    assert_eq!(Off.step(1), A);
    assert_eq!(A.step(-1), Off);
    assert_eq!(Off.step(-1), Off);
    assert_eq!(D.step(1), D);
    assert_eq!(A.step(i8::MAX), D);
    assert_eq!(B.step(i8::MIN), Off);
    assert_eq!(ChokeGroup::end(1), D);
    assert_eq!(ChokeGroup::end(-1), Off);
}

/// On an old kit (12 groups) a turn that lowers the count lands; one that
/// raises it is held, the count it would make in the reason.
#[test]
fn an_old_kit_edits_only_downward() {
    let mut ui = keys_on(1, &all_off());
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 1), ChokeGroup::A, "12 stays 12: lands");
    ui.select_pad(P1, pad(2));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(choke(&ui, 2), ChokeGroup::A, "11: lands");
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(choke(&ui, 2), ChokeGroup::A, "back to 12: refused");
    let shown = ui.held_choice().expect("OFF held");
    let why = chimera_core::ui::held::reason(shown.why);
    assert_eq!(why.as_str(), "12 GROUPS · KIT HOLDS 8");
}
