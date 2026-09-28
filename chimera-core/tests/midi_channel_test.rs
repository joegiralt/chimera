//! MIDI channel selection end to end: an encoder on a channel slot, the
//! Part's `mix.channel`, the `AudioShared` publish and the Instrument's
//! note-on filter.

mod common;
mod screen;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{CPU_HZ_REV_V, DAC_PAIRS, MAX_PARTS, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::SYS_MIDI;
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId, SAMPLE_RATE};
use screen::*;

const ENC: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

/// The text slot `i` of the current page shows once the lerps settle.
fn shown(ui: &mut UiState, i: usize) -> String {
    settle(ui);
    let mut buf = FmtBuf::new();
    let fmt = ui.nav.active_block_def().params[i].format();
    fmt_val(&mut buf, ui.renderer.anim[i].current(), fmt);
    buf.as_str().to_owned()
}

/// The Parts that sound for a note-on on status-byte channel `ch` (0..15).
fn parts_hearing(ui: &UiState, ch: u8) -> Vec<usize> {
    let shared = AudioShared::from_performance(&ui.performance);
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let mut fx = Box::new(FxBus::new());
    let mut out = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    let mut scope = common::scope_writer();
    inst.handle(
        NoteEvent {
            channel: MidiChannel::new(ch).unwrap(),
            note: MidiNote::new(60).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        },
        &shared,
    );
    inst.render(&mut fx, &mut out, &shared, &mut scope);
    (0..MAX_PARTS)
        .filter(|&p| inst.part_bus(p).iter().any(|s| *s != 0.0))
        .collect()
}

/// Defaults: Part n listens on channel n (status nibble n − 1).
#[test]
fn part_n_listens_on_channel_n_by_default() {
    let ui = UiState::new();
    for p in 0..MAX_PARTS {
        assert_eq!(ui.performance.parts[p].mix.channel.get(), p as u8);
        assert_eq!(parts_hearing(&ui, p as u8), [p], "status nibble {p}");
    }
}

/// MIX + B1, CH +2: Part 1 shows 3 and plays from status nibble 2.
#[test]
fn mixer_part_page_moves_part_1_to_channel_3() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::turn(EncoderId::A, 2));
    assert_eq!(shown(&mut ui, 0), "3");
    assert_eq!(parts_hearing(&ui, 2), [0, 2], "Part 1 layered on Part 3");
    assert_eq!(parts_hearing(&ui, 0), [] as [usize; 0]);
}

/// MENU opens System › MIDI Setup: P1 CH .. P6 CH show each Part's channel.
#[test]
fn midi_setup_shows_every_parts_channel() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::Menu));
    assert_eq!(ui.nav.active_block_def().id, SYS_MIDI.id);
    for (i, want) in ["1", "2", "3", "4", "5", "6"].into_iter().enumerate() {
        assert_eq!(shown(&mut ui, i), want, "P{} CH", i + 1);
    }
}

/// MIDI Setup's encoders move each Part's channel, and the Instrument
/// follows: P2 CH +3 puts Part 2 on channel 5.
#[test]
fn midi_setup_edits_each_parts_channel() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::Menu));
    feed(&mut ui, Input::turn(ENC[1], 3));
    assert_eq!(ui.performance.parts[1].mix.channel.get(), 4);
    assert_eq!(shown(&mut ui, 1), "5");
    assert_eq!(parts_hearing(&ui, 4), [1, 4]);
    assert_eq!(parts_hearing(&ui, 1), [] as [usize; 0]);
    // Every other Part is where it was, whichever Part the UI last edited.
    for p in [0, 2, 3, 4, 5] {
        assert_eq!(ui.performance.parts[p].mix.channel.get(), p as u8);
    }
    // Clamped to 1..16.
    feed(&mut ui, Input::turn(ENC[5], 20));
    assert_eq!(shown(&mut ui, 5), "16");
    feed(&mut ui, Input::turn(ENC[0], -5));
    assert_eq!(shown(&mut ui, 0), "1");
}
