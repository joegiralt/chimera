//! #515, ADR 0224: a program change loads a Sound into every Part on its
//! channel. Bank 0 is the project's pool, bank 1 the INITs by engine disk
//! code; CC 0 / CC 32 select the bank and it stands till changed. An
//! edited Part keeps its edits as its recall copy (Elektron style), a
//! later change back to that slot brings them back. While a load runs or
//! a prompt is open the latest change per channel waits.

#![cfg(feature = "midi-map")]

mod screen;

use std::sync::atomic::Ordering;

use chimera_core::MidiChannel;
use chimera_core::midi_in::{CtrlEvent, PROGRAM_PENDING, Port};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{Origin, PartFrom, PartId, SlotId, SoundStatus, sound_status};
use chimera_core::ui::UiState;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_hal::{ButtonId, Ms};
use chimera_midi::ctl::Ctl;
use chimera_midi::value::{Channel, U7, U14};
use screen::{Input, feed, tap, to_row};

const P: [PartId; 6] = PartId::ALL;

fn ch(c: u8) -> Channel {
    Channel::new(c).unwrap()
}

fn prog(c: u8, bank: Option<(u8, u8)>, p: u8) -> (Port, CtrlEvent) {
    let bank = bank.map(|(m, l)| U14::from_parts(U7::new(m).unwrap(), U7::new(l).unwrap()));
    (
        Port::Din,
        CtrlEvent::Ctl(
            ch(c),
            Ctl::Program {
                bank,
                program: U7::new(p).unwrap(),
            },
        ),
    )
}

/// A slot's Sound: its engine's INIT, named so each slot differs.
fn sound(e: EngineType, name: &str) -> Sound {
    let mut s = Sound::init(e);
    s.name = chimera_core::name::SoundName::new(name).unwrap();
    s
}

/// Slots 1–3 filled (VA, MODAL, ALGO), PROGRAM IN on; Parts 1 and 2 on
/// channel 3, Part 3 on channel 4.
fn rig() -> Box<UiState> {
    let mut ui = Box::new(screen::stack_ui());
    for (i, e) in [EngineType::Va, EngineType::Modal, EngineType::Algo]
        .into_iter()
        .enumerate()
    {
        let s = sound(e, &format!("SLOT {i}"));
        ui.project_mut().pool_store(SlotId::ALL[i], &s).unwrap();
    }
    for (p, c) in [(P[0], 2), (P[1], 2), (P[2], 3)] {
        ui.project_mut().edit_part(p).mix.channel = MidiChannel::new(c).unwrap();
    }
    ui.set_midi(MidiSettings::DEFAULT.with_program_in(true));
    ui
}

fn pass(ui: &mut UiState, ev: &[(Port, CtrlEvent)]) {
    ui.hear_ccs(ev.iter().copied(), Ms(1_000), true);
}

fn from(ui: &UiState, p: PartId) -> PartFrom {
    match ui.project().part(p).origin() {
        Origin::Slot { slot, .. } => PartFrom::Slot(slot),
        Origin::Init(e) => PartFrom::Init(e),
    }
}

fn status(ui: &UiState, p: PartId) -> SoundStatus {
    sound_status(ui.project().part(p), ui.project().pool())
}

fn edit(ui: &mut UiState, p: PartId) {
    ui.project_mut().edit_part(p).sound.params.filter.cutoff *= 0.5;
}

fn cutoff(ui: &UiState, p: PartId) -> f32 {
    ui.project().part(p).sound.params.filter.cutoff
}

#[test]
fn bank_0_loads_the_pool_slot_into_every_part_on_the_channel() {
    let mut ui = rig();
    let third = from(&ui, P[2]);
    pass(&mut ui, &[prog(2, None, 1)]);
    for p in [P[0], P[1]] {
        assert_eq!(from(&ui, p), PartFrom::Slot(SlotId::ALL[1]), "{p:?}");
        assert_eq!(status(&ui, p), SoundStatus::Clean);
    }
    assert_eq!(from(&ui, P[2]), third, "another channel's Part stays");
    assert_eq!(
        ui.project().part(P[0]).sound.engine(),
        EngineType::Modal,
        "program 1 is slot 2"
    );
}

#[test]
fn bank_1_is_the_inits_by_disk_code_and_the_bank_stands() {
    let mut ui = rig();
    pass(&mut ui, &[prog(3, Some((1, 0)), 3)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Init(EngineType::Va));
    // No bank select since: bank 1 still.
    pass(&mut ui, &[prog(3, None, 1)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Init(EngineType::Modal));
    // MSB 0, LSB 1 is bank 1 too (Yamaha and Korg sequencers).
    pass(&mut ui, &[prog(3, Some((0, 0)), 0)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[0]));
    pass(&mut ui, &[prog(3, Some((0, 1)), 3)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Init(EngineType::Va));
    // Past the last engine, and banks 2 up: nothing.
    pass(&mut ui, &[prog(3, None, 9)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Init(EngineType::Va));
    pass(&mut ui, &[prog(3, Some((2, 0)), 0)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Init(EngineType::Va));
    // The bank is the port's: USB's channel 4 is still bank 0.
    let (_, e) = prog(3, None, 2);
    pass(&mut ui, &[(Port::Usb, e)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[2]));
}

#[test]
fn noops_cost_nothing() {
    let mut ui = rig();
    pass(&mut ui, &[prog(2, None, 0)]);
    let loads = ui.project().part(P[0]).loads();
    pass(&mut ui, &[prog(2, None, 0)]);
    assert_eq!(ui.project().part(P[0]).loads(), loads, "the slot it holds");
    pass(&mut ui, &[prog(2, None, 40)]);
    assert_eq!(ui.project().part(P[0]).loads(), loads, "an empty slot");
    ui.set_midi(MidiSettings::DEFAULT);
    pass(&mut ui, &[prog(2, None, 1)]);
    assert_eq!(
        from(&ui, P[0]),
        PartFrom::Slot(SlotId::ALL[0]),
        "PROGRAM IN off"
    );
    // A Stale Part reloads its slot.
    ui.set_midi(MidiSettings::DEFAULT.with_program_in(true));
    let s = sound(EngineType::Va, "NEWER");
    ui.project_mut().pool_store(SlotId::ALL[0], &s).unwrap();
    assert!(matches!(status(&ui, P[0]), SoundStatus::Stale(_)));
    pass(&mut ui, &[prog(2, None, 0)]);
    assert_eq!(status(&ui, P[0]), SoundStatus::Clean);
    assert_eq!(ui.project().part(P[0]).sound.name.as_str(), "NEWER");
}

#[test]
fn an_edited_part_keeps_its_edits_for_its_return() {
    let mut ui = rig();
    pass(&mut ui, &[prog(3, None, 0)]);
    let clean = cutoff(&ui, P[2]);
    edit(&mut ui, P[2]);
    let edited = cutoff(&ui, P[2]);
    assert_ne!(edited, clean);
    // Edited: it loads anyway, never asks.
    pass(&mut ui, &[prog(3, None, 2)]);
    assert!(!ui.prompt_open());
    assert_eq!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[2]));
    assert_eq!(status(&ui, P[2]), SoundStatus::Clean);
    // Back: the edits, still marked edited.
    pass(&mut ui, &[prog(3, None, 0)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[0]));
    assert_eq!(cutoff(&ui, P[2]), edited);
    assert_eq!(status(&ui, P[2]), SoundStatus::Edited);
    // Out, through another clean slot, and back: still there.
    pass(&mut ui, &[prog(3, None, 2)]);
    pass(&mut ui, &[prog(3, None, 1)]);
    pass(&mut ui, &[prog(3, None, 0)]);
    assert_eq!(cutoff(&ui, P[2]), edited);
}

#[test]
fn edits_swap_between_two_slots() {
    let mut ui = rig();
    pass(&mut ui, &[prog(3, None, 0)]);
    edit(&mut ui, P[2]);
    let a = cutoff(&ui, P[2]);
    pass(&mut ui, &[prog(3, None, 2)]);
    edit(&mut ui, P[2]);
    edit(&mut ui, P[2]);
    let b = cutoff(&ui, P[2]);
    pass(&mut ui, &[prog(3, None, 0)]);
    assert_eq!(cutoff(&ui, P[2]), a);
    pass(&mut ui, &[prog(3, None, 2)]);
    assert_eq!(cutoff(&ui, P[2]), b);
    assert_eq!(status(&ui, P[2]), SoundStatus::Edited);
}

/// Any other load of the Part (RELOAD, the browser) drops its recall copy.
#[test]
fn another_load_drops_the_recall_copy() {
    let mut ui = rig();
    pass(&mut ui, &[prog(3, None, 0)]);
    let clean = cutoff(&ui, P[2]);
    edit(&mut ui, P[2]);
    pass(&mut ui, &[prog(3, None, 2)]);
    screen::init_part(&mut ui, P[2]);
    pass(&mut ui, &[prog(3, None, 0)]);
    assert_eq!(cutoff(&ui, P[2]), clean);
    assert_eq!(status(&ui, P[2]), SoundStatus::Clean);
}

#[test]
fn waits_while_a_load_runs_latest_wins() {
    let mut ui = rig();
    ui.start_pending_library_load_for_test();
    pass(&mut ui, &[prog(3, None, 1), prog(3, None, 2)]);
    assert_ne!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[2]), "waits");
    tap(&mut ui, ButtonId::Menu);
    pass(&mut ui, &[]);
    assert_eq!(
        from(&ui, P[2]),
        PartFrom::Slot(SlotId::ALL[2]),
        "the latest"
    );
}

#[test]
fn waits_while_its_part_is_browsed() {
    let mut ui = rig();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B3));
    to_row(&mut ui, chimera_core::ui::browser::HEAD);
    let at = ui.location();
    pass(&mut ui, &[prog(3, None, 1), prog(2, None, 1)]);
    assert_eq!(
        from(&ui, P[0]),
        PartFrom::Slot(SlotId::ALL[1]),
        "another Part's"
    );
    assert_ne!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[1]), "waits");
    assert_eq!(ui.location(), at, "the browser stays put");
    tap(&mut ui, ButtonId::Menu);
    pass(&mut ui, &[]);
    assert_eq!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[1]));
}

#[test]
fn the_active_parts_pages_follow_its_engine() {
    let mut ui = rig();
    let algo = ui.page_def().id;
    pass(&mut ui, &[prog(2, None, 0)]);
    assert_eq!(ui.project().part(P[0]).sound.engine(), EngineType::Va);
    assert_ne!(ui.page_def().id, algo, "VA's page");
}

/// The channel's notes go free the pass after its change was applied.
#[test]
fn the_pending_bit_clears_the_pass_after() {
    let mut ui = rig();
    ui.project_mut().edit_part(P[5]).mix.channel = MidiChannel::new(15).unwrap();
    PROGRAM_PENDING.fetch_or(1 << 15, Ordering::Relaxed);
    pass(&mut ui, &[prog(15, None, 0)]);
    assert_eq!(from(&ui, P[5]), PartFrom::Slot(SlotId::ALL[0]));
    assert_ne!(
        PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 15,
        0,
        "published first"
    );
    pass(&mut ui, &[]);
    assert_eq!(PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 15, 0);
}

/// Review of #891: a second program change heard the pass after the
/// first was applied keeps its notes held till it is applied too.
#[test]
fn a_second_quick_change_keeps_its_hold() {
    let mut ui = rig();
    ui.project_mut().edit_part(P[4]).mix.channel = MidiChannel::new(14).unwrap();
    PROGRAM_PENDING.fetch_or(1 << 14, Ordering::Relaxed);
    pass(&mut ui, &[prog(14, None, 0)]);
    // The pipe queues the second and marks the channel again.
    PROGRAM_PENDING.fetch_or(1 << 14, Ordering::Relaxed);
    pass(&mut ui, &[prog(14, None, 1)]);
    assert_eq!(from(&ui, P[4]), PartFrom::Slot(SlotId::ALL[1]));
    assert_ne!(
        PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 14,
        0,
        "published first"
    );
    pass(&mut ui, &[]);
    assert_eq!(PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 14, 0);
}

/// Review of #891: a change heard while a load runs keeps its channel
/// held while it waits, and goes free the pass after it is applied.
#[test]
fn a_late_change_keeps_its_hold_while_it_waits() {
    let mut ui = rig();
    ui.project_mut().edit_part(P[3]).mix.channel = MidiChannel::new(13).unwrap();
    PROGRAM_PENDING.fetch_or(1 << 13, Ordering::Relaxed);
    pass(&mut ui, &[prog(13, None, 0)]);
    PROGRAM_PENDING.fetch_or(1 << 13, Ordering::Relaxed);
    ui.start_pending_library_load_for_test();
    pass(&mut ui, &[prog(13, None, 1)]);
    pass(&mut ui, &[]);
    assert_ne!(
        PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 13,
        0,
        "waiting"
    );
    tap(&mut ui, ButtonId::Menu);
    pass(&mut ui, &[]);
    assert_eq!(from(&ui, P[3]), PartFrom::Slot(SlotId::ALL[1]));
    assert_ne!(
        PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 13,
        0,
        "published first"
    );
    pass(&mut ui, &[]);
    assert_eq!(PROGRAM_PENDING.load(Ordering::Relaxed) & 1 << 13, 0);
}

/// Review of #891: an edited Part whose new slot won't decode is left as
/// it was, edits and all, and its recall copy untouched.
#[test]
fn a_refused_load_leaves_the_part_as_it_was() {
    let mut ui = rig();
    pass(&mut ui, &[prog(3, None, 0)]);
    edit(&mut ui, P[2]);
    let edited = cutoff(&ui, P[2]);
    ui.project_mut().cut_slot_for_test(SlotId::ALL[2]);
    pass(&mut ui, &[prog(3, None, 2)]);
    assert_eq!(from(&ui, P[2]), PartFrom::Slot(SlotId::ALL[0]));
    assert_eq!(cutoff(&ui, P[2]), edited);
    assert_eq!(status(&ui, P[2]), SoundStatus::Edited);
    // No copy was kept: slot 2 and back, the slot as saved.
    pass(&mut ui, &[prog(3, None, 1)]);
    pass(&mut ui, &[prog(3, None, 0)]);
    assert_eq!(cutoff(&ui, P[2]), edited, "slot 1's change kept them");
}

/// #949: a DRUM Part's kit stays on the Part through a program change, so
/// a kit edit is no reason to drop one.
#[test]
fn a_kit_edit_never_drops_a_program_change() {
    use chimera_core::dsp::drum::types::{Machine, PadId};
    let mut ui = rig();
    screen::load_init(&mut ui, EngineType::Drum);
    assert_eq!(from(&ui, P[0]), PartFrom::Init(EngineType::Drum));
    ui.project_mut()
        .kit_mut(P[0])
        .pad_mut(PadId::ALL[0])
        .set_machine(Machine::PlainSine);
    let kit = *ui.project().kit(P[0]);
    pass(&mut ui, &[prog(2, None, 1)]);
    assert_eq!(from(&ui, P[0]), PartFrom::Slot(SlotId::ALL[1]));
    assert_eq!(*ui.project().kit(P[0]), kit, "the kit stays on the Part");
}

/// #1068: a program change that swaps the shown Part's engine redraws the
/// chain strip and the page tabs on the frame the body does, as the
/// box's own engine change does.
#[test]
fn a_program_change_to_another_engine_redraws_the_strip() {
    let mut ui = rig();
    let draw = |ui: &mut UiState, fb: &mut screen::Fb| {
        ui.render_dirty_with_audio(
            fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            Some(&screen::audio_fixture()),
            &screen::scope_fixture(),
        );
    };
    let mut fb = screen::Fb::new();
    draw(&mut ui, &mut fb);
    // Slot 1 (VA), slot 2 (MODAL), bank 1's DRUM, slot 3 (ALGO): each a
    // new engine on the Part shown.
    for ev in [
        prog(2, None, 0),
        prog(2, None, 1),
        prog(2, Some((1, 0)), 4),
        prog(2, Some((0, 0)), 2),
    ] {
        pass(&mut ui, &[ev]);
        screen::settle(&mut ui);
        draw(&mut ui, &mut fb);
        let full = screen::render_ui(&ui);
        assert!(
            fb.hash() == full.hash(),
            "{:?}: the dirty frame is stale",
            ui.project().part(P[0]).sound.engine()
        );
    }
}

/// #1068: an engine change from the box (an INIT load from the browser)
/// redraws the strip as a program change does.
#[test]
fn a_browser_load_of_another_engine_redraws_the_strip() {
    let draw = |ui: &mut UiState, fb: &mut screen::Fb| {
        ui.render_dirty_with_audio(
            fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            Some(&screen::audio_fixture()),
            &screen::scope_fixture(),
        );
    };
    // From ALGO's pages, a fresh UI's, each time.
    for e in [EngineType::Va, EngineType::Modal, EngineType::Drum] {
        let mut ui = Box::new(screen::stack_ui());
        let mut fb = screen::Fb::new();
        draw(&mut ui, &mut fb);
        screen::load_init(&mut ui, e);
        screen::settle(&mut ui);
        draw(&mut ui, &mut fb);
        assert!(fb.hash() == screen::render_ui(&ui).hash(), "{e:?}: stale");
    }
}
