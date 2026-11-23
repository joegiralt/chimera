// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SysEx controllers (#846, ADR 0241): learn diffs two messages of an old
//! box's control to find its value, the cell maps to it, and later
//! messages move the param; another address leaves it be. Through the
//! pipe, as the DIN ISR feeds it.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::midi_in::{CtrlRing, DinSinks, PipeSlot};
use chimera_core::midi_map::{Mappable, MidiMap};
use chimera_core::note_queue::NoteQueue;
use chimera_core::params::FilterParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::learn::Learn;
use chimera_core::ui::nav::PartChain;
use chimera_hal::ButtonId::{Mix, Plus, Seq};
use chimera_hal::EncoderId::F;
use chimera_hal::Ms;
use screen::{Input, feed};

/// `SYSEX_IN` is global: one test at a time feeds it.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

/// Part 1 on INIT Algo with the 2040, on FLT, CUTOFF focused, no map.
fn on_cutoff() -> Box<UiState> {
    let mut ui = Box::new(crate::screen::stack_ui());
    let was = PartChain::of(&ui.project().part(P1).sound);
    screen::init_part(&mut ui, P1);
    let sound = ui.project_mut().edit_part(P1).sound;
    sound.params.filter.set_kind(FilterKind::K2040);
    sound.map = MidiMap::EMPTY;
    ui.replaced_for_test(was);
    for _ in 0..12 {
        if ui.page_def().id == FILTER.id {
            break;
        }
        feed(&mut ui, Input::press(Plus));
    }
    feed(&mut ui, Input::turn(F, 1));
    ui
}

fn mix_seq(ui: &mut UiState) {
    feed(ui, Input::chord(Mix, Seq).at(10_000));
    feed(ui, Input::release(Seq).and_held(Mix).at(10_100));
    feed(ui, Input::release(Mix).at(10_133));
}

fn stored(ui: &UiState, a: ParamAddr) -> f32 {
    ui.project()
        .read_part(P1)
        .block(a.block)
        .unwrap()
        .get(a.param)
}

/// A Roland DT1 body: 41 dev 42 12, address, value, checksum.
fn dt1(addr: u8, v: u8) -> Vec<u8> {
    let mut b = vec![0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x01, addr, v];
    let sum: u32 = b[5..].iter().map(|&x| u32::from(x)).sum();
    b.push(((128 - sum % 128) % 128) as u8);
    b.push(0xF7);
    b
}

/// `bytes` into DIN's pipe, then a UI pass at `at` ms.
fn din(ui: &mut UiState, bytes: &[u8], at: u32) {
    let q = Box::leak(Box::new(NoteQueue::new()));
    let (mut prod, _cons) = q.split().unwrap();
    let ring: CtrlRing = CtrlRing::new();
    // The shells' path, through the door's sinks.
    let mut slot = PipeSlot::new();
    chimera_core::midi_in::feed(
        &mut slot,
        bytes,
        &mut DinSinks {
            notes: &mut prod,
            ctrl: &ring,
            sysex: Some(&chimera_core::midi_in::SYSEX_IN[0]),
        },
    );
    ui.hear_ccs(ring.drain(), Ms(at), true);
}

#[test]
fn learn_finds_the_value_and_messages_move_the_cell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    din(&mut ui, &dt1(0x1A, 10), 11_000);
    assert!(
        matches!(ui.learn(), Learn::Armed { .. }),
        "one message is not enough"
    );
    let before = stored(&ui, CUTOFF);
    din(&mut ui, &dt1(0x1A, 90), 11_010);
    assert!(
        matches!(ui.learn(), Learn::Sensing { .. }),
        "it listens on for a carry"
    );
    assert_eq!(stored(&ui, CUTOFF), before, "learning moves nothing");
    mix_seq(&mut ui);
    assert_eq!(ui.learn(), Learn::Off);
    let map = ui.project().part(P1).sound.map;
    assert_eq!(map.len(), 1);
    assert_eq!(map.entries()[0].target, Mappable::new(CUTOFF).unwrap());
    // Its top: the cell glides there.
    for t in 0..40 {
        din(&mut ui, &dt1(0x1A, 127), 12_000 + t * 20);
    }
    let top = stored(&ui, CUTOFF);
    for t in 0..40 {
        din(&mut ui, &dt1(0x1A, 0), 14_000 + t * 20);
    }
    let bottom = stored(&ui, CUTOFF);
    assert!(top > bottom, "{top} {bottom}");
    // Another address is another control.
    for t in 0..40 {
        din(&mut ui, &dt1(0x1B, 127), 16_000 + t * 20);
    }
    assert_eq!(stored(&ui, CUTOFF), bottom);
}

#[test]
fn the_binding_survives_the_codec_and_old_maps_load() {
    use chimera_core::midi_map::codec::{MapRecord, decode, encode};
    use chimera_core::storage::RecordBuf;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    din(&mut ui, &dt1(0x1A, 10), 11_000);
    din(&mut ui, &dt1(0x1A, 90), 11_010);
    let map = ui.project().part(P1).sound.map;
    let mut r = RecordBuf::new();
    encode(&map, &mut r);
    assert_eq!(r.as_slice()[0], 2, "version 2 with a SysEx entry");
    let mut back = MidiMap::EMPTY;
    assert_eq!(
        decode(MapRecord::Bindings, r.as_slice(), Some(&mut back)),
        Ok(0)
    );
    assert_eq!(back, map);
}

/// A DX7 parameter change: 43 10 01 pp vv.
fn dx7(pp: u8, vv: u8) -> Vec<u8> {
    vec![0xF0, 0x43, 0x10, 0x01, pp, vv, 0xF7]
}

/// `body` to its top and bottom, a glide's worth each; CUTOFF at both.
fn sweep(ui: &mut UiState, top: &[u8], bottom: &[u8], at: u32) -> (f32, f32) {
    for t in 0..40 {
        din(ui, top, at + t * 20);
    }
    let hi = stored(ui, CUTOFF);
    for t in 0..40 {
        din(ui, bottom, at + 1_000 + t * 20);
    }
    (hi, stored(ui, CUTOFF))
}

#[test]
fn two_knobs_apart_by_their_parameter_byte_never_move_each_others_cell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    // Knob 5 first, then knob 6: two bytes apart, no binding yet.
    din(&mut ui, &dx7(5, 100), 11_000);
    din(&mut ui, &dx7(6, 10), 11_010);
    assert!(ui.project().part(P1).sound.map.is_empty());
    din(&mut ui, &dx7(6, 20), 11_020);
    mix_seq(&mut ui);
    assert_eq!(ui.project().part(P1).sound.map.len(), 1);
    let (hi, lo) = sweep(&mut ui, &dx7(5, 127), &dx7(5, 0), 12_000);
    assert_eq!(hi, lo, "knob 5 moves nothing");
    let (hi, lo) = sweep(&mut ui, &dx7(6, 127), &dx7(6, 0), 15_000);
    assert!(hi > lo, "knob 6 moves CUTOFF: {hi} {lo}");
}

#[test]
fn a_slow_14_bit_sweep_learns_the_pair_at_its_carry() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let m = |msb: u8, lsb: u8| vec![0xF0, 0x42, 0x30, 7, msb, lsb, 0xF7];
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    din(&mut ui, &m(3, 10), 11_000);
    din(&mut ui, &m(3, 11), 11_010);
    // The sweep crosses two MSBs while learn listens: one carry could be
    // two knobs side by side, two in a row are a sweep.
    din(&mut ui, &m(3, 127), 11_020);
    din(&mut ui, &m(4, 0), 11_030);
    din(&mut ui, &m(4, 127), 11_040);
    din(&mut ui, &m(5, 0), 11_050);
    mix_seq(&mut ui);
    let (hi, lo) = sweep(&mut ui, &m(127, 127), &m(0, 0), 12_000);
    assert!(hi > lo + 0.5, "the whole range: {hi} {lo}");
}

#[test]
fn a_cell_no_control_may_drive_takes_no_sysex() {
    use chimera_core::ui::block_registry::FILTER_JACKS;
    use chimera_hal::ButtonId::Edit;
    use chimera_hal::EncoderId::C;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = on_cutoff();
    // CUTOFF already on knob 5.
    mix_seq(&mut ui);
    din(&mut ui, &dx7(5, 3), 10_500);
    din(&mut ui, &dx7(5, 4), 10_510);
    mix_seq(&mut ui);
    for _ in 0..3 {
        feed(&mut ui, Input::press(Edit));
    }
    assert_eq!(ui.page_def().id, FILTER_JACKS.id);
    mix_seq(&mut ui);
    feed(&mut ui, Input::turn(C, 1));
    let band = |ui: &UiState| {
        ui.status_text()
            .map_or_else(String::new, |b| b.as_str().into())
    };
    assert_eq!(band(&ui), "FREQ SRC NOT MAPPABLE");
    // Learn takes nothing: knob 5's binding hears it, as a CC's would.
    let (hi, lo) = sweep(&mut ui, &dx7(5, 127), &dx7(5, 0), 20_000);
    assert!(hi > lo, "{hi} {lo}");
    assert_eq!(ui.project().part(P1).sound.map.len(), 1);
    assert!(matches!(ui.learn(), Learn::Armed { .. }));
    assert_eq!(band(&ui), "FREQ SRC NOT MAPPABLE");
}

#[test]
fn a_cancelled_learn_forgets_its_first_message() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    din(&mut ui, &dx7(5, 3), 11_000);
    mix_seq(&mut ui);
    din(&mut ui, &dx7(9, 9), 11_500); // a pass with learn off
    mix_seq(&mut ui);
    // Armed again: one message is a first, not a second.
    din(&mut ui, &dx7(5, 90), 12_000);
    assert!(ui.project().part(P1).sound.map.is_empty());
}

#[test]
fn a_neighbouring_knob_at_the_wrap_stays_its_own_control() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let at = |a: u8, v: u8| {
        let mut m = dt1(a, v);
        m.retain(|_| true);
        m
    };
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    din(&mut ui, &at(0x1A, 10), 11_000);
    din(&mut ui, &at(0x1A, 11), 11_010);
    // Cutoff's knob at 120, then resonance's (1B) touched at 2: still sensing.
    din(&mut ui, &at(0x1A, 120), 11_020);
    din(&mut ui, &at(0x1B, 2), 11_030);
    din(&mut ui, &at(0x1B, 9), 11_040);
    assert!(matches!(ui.learn(), Learn::Sensing { .. }));
    mix_seq(&mut ui);
    // 1B (and 1C) move nothing; 1A still moves CUTOFF.
    let (hi, lo) = sweep(&mut ui, &at(0x1B, 127), &at(0x1B, 0), 12_000);
    assert_eq!(hi, lo, "the neighbour moves nothing");
    let (hi, lo) = sweep(&mut ui, &at(0x1C, 127), &at(0x1C, 0), 15_000);
    assert_eq!(hi, lo);
    let (hi, lo) = sweep(&mut ui, &at(0x1A, 127), &at(0x1A, 0), 18_000);
    assert!(hi > lo, "{hi} {lo}");
}

#[test]
fn sysex_sensing_shows_its_control_and_the_done_hint() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    din(&mut ui, &dx7(5, 3), 11_000);
    din(&mut ui, &dx7(5, 90), 11_010);
    let (top, words, sure) = ui.learn_text().expect("sensing's words");
    assert!(top.as_str().starts_with("SYSEX"), "{}", top.as_str());
    assert_eq!(words.as_str(), "ABSOLUTE");
    assert!(sure, "MIX + SEQ  DONE");
}
