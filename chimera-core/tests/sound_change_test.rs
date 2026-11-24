// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! #14 (decided 2026-11-22): a Part's new Sound (a program change, a
//! library load: its `loads` move) fades the voices still playing the old
//! one out through the voice's FADE, as CC 120 does. None restarts on the
//! new Sound, and none plays on under it; notes after the load play it.
mod common;
use chimera_core::MidiChannel;
use chimera_core::note_queue::{ChannelMode, ChannelModeEvent};
use common::{InstRig, SR};

use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::project::Loads;
use chimera_hal::BLOCK_SIZE;

const CHORD: [u8; 3] = [48, 55, 60];

fn sounding(r: &InstRig) -> usize {
    (0..MAX_VOICES)
        .filter(|&v| r.inst.voice(v).is_active())
        .count()
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

/// Part 1 on `from`, the chord held for 40 blocks; then `to` loads under
/// it. Part 1's bus over the 40 blocks before and the blocks after, the
/// same chord's bus after a CC 120 in the load's place, and the rig.
fn change(from: Sound, to: Sound) -> (Vec<f32>, Vec<f32>, Vec<f32>, InstRig) {
    let play = |cc120: bool| {
        let mut r = rig(from.clone());
        for n in CHORD {
            r.note_on(0, n);
        }
        let mut before = Vec::new();
        for _ in 0..40 {
            r.render(1);
            before.extend_from_slice(r.inst.part_bus(0));
        }
        assert_eq!(sounding(&r), CHORD.len());
        if cc120 {
            r.inst.channel_mode(ChannelModeEvent {
                channel: MidiChannel::new(0).unwrap(),
                mode: ChannelMode::SoundOff,
            });
        } else {
            load(&mut r, to.clone());
        }
        let mut after = Vec::new();
        for _ in 0..Voice::FADE as usize / BLOCK_SIZE + 20 {
            r.render(1);
            after.extend_from_slice(r.inst.part_bus(0));
        }
        (before, after, r)
    };
    let (before, after, r) = play(false);
    let (_, sound_off, _) = play(true);
    (before, after, sound_off, r)
}

fn check(name: &str, from: Sound, to: Sound) {
    let (before, after, sound_off, r) = change(from, to);
    assert!(
        before.iter().any(|x| x.abs() > 0.01),
        "{name}: the chord sounds"
    );
    assert_eq!(
        sounding(&r),
        0,
        "{name}: a held voice restarted or played on"
    );
    let fade = Voice::FADE as usize;
    assert!(
        after[fade..].iter().all(|&x| x == 0.0),
        "{name}: silent after the fade"
    );
    // Exactly CC 120's fade on the settings the chord played: no step of
    // its own, whatever the new Sound.
    assert!(
        after
            .iter()
            .zip(&sound_off)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "{name}: not CC 120's fade"
    );
    let mut joined = before[before.len() - BLOCK_SIZE..].to_vec();
    joined.extend_from_slice(&after[..fade]);
    assert!(
        max_step(&joined) <= max_step(&before),
        "{name}: a click: step {} over the chord's {}",
        max_step(&joined),
        max_step(&before)
    );
}

#[test]
fn a_new_engine_under_a_held_chord_fades_it_out() {
    check(
        "ALGO→MODAL",
        Sound::init(EngineType::Algo),
        Sound::init(EngineType::Modal),
    );
}

#[test]
fn a_new_sound_on_the_same_engine_fades_the_chord_out() {
    let s = chimera_core::factory::factory_sound(4).unwrap(); // SAW LEAD
    check("ALGO→SAW LEAD", Sound::init(EngineType::Algo), s);
}

/// A note in the block the new Sound lands in plays it: only the voices
/// from before fade.
#[test]
fn a_note_after_the_load_plays_the_new_sound() {
    let mut r = rig(Sound::init(EngineType::Algo));
    r.note_on(0, 48);
    r.render(20);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    r.shared.parts[0].loads = r.shared.parts[0].loads.next();
    r.note_on(0, 60);
    r.render(Voice::FADE as usize / BLOCK_SIZE + 2);
    assert_eq!(sounding(&r), 1, "the new note sounds, the old one is gone");
    let _ = SR;
}

fn rig(sound: Sound) -> InstRig {
    // The fade is bank 2's (`AdmitDoor`): open with `bank2`.
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *r.shared.parts[0].params = sound.params;
    *r.shared.parts[0].mod_state = sound.mod_state;
    r.shared.parts[0].loads = Loads::NONE.next();
    r
}

fn load(r: &mut InstRig, sound: Sound) {
    *r.shared.parts[0].params = sound.params;
    *r.shared.parts[0].mod_state = sound.mod_state;
    r.shared.parts[0].loads = r.shared.parts[0].loads.next();
}

const FADE_BLOCKS: usize = Voice::FADE as usize / BLOCK_SIZE + 2;

/// A chord the pedal holds after its keys are up is held: it fades too.
#[test]
fn a_chord_the_pedal_holds_fades_out() {
    use chimera_core::note_queue::SustainEvent;
    let mut r = rig(Sound::init(EngineType::Algo));
    for n in CHORD {
        r.note_on(0, n);
    }
    r.render(10);
    let ch = MidiChannel::new(0).unwrap();
    r.inst.sustain(SustainEvent {
        channel: ch,
        down: true,
    });
    for n in CHORD {
        r.note_off(0, n);
    }
    r.render(10);
    assert_eq!(sounding(&r), CHORD.len(), "the pedal holds the chord");
    load(&mut r, chimera_core::factory::factory_sound(4).unwrap());
    r.render(FADE_BLOCKS);
    assert_eq!(sounding(&r), 0);
}

/// A released tail rings on into the new Sound (its knobs landed): only
/// held voices fade.
#[test]
fn a_released_tail_rings_on() {
    let mut r = rig(Sound::init(EngineType::Algo));
    r.note_on(0, 60);
    r.render(10);
    r.note_off(0, 60);
    r.render(1);
    load(&mut r, chimera_core::factory::factory_sound(4).unwrap());
    r.render(FADE_BLOCKS);
    assert_eq!(sounding(&r), 1, "the tail still rings");
}

/// A held note fading toward its restart on an edited engine (ENGINE
/// turned, no load) stops restarting when a new Sound loads mid-fade.
#[test]
fn a_restart_mid_fade_stops_at_a_load() {
    let mut r = rig(Sound::init(EngineType::Algo));
    r.note_on(0, 60);
    r.render(10);
    // ENGINE turned in place: the voice fades toward a restart on MODAL.
    let modal = Sound::init(EngineType::Modal);
    *r.shared.parts[0].params = modal.params.clone();
    *r.shared.parts[0].mod_state = modal.mod_state.clone();
    r.render(1);
    assert!(
        r.inst.voice(voice_of_part(&r)).restarts(),
        "a restart is under way"
    );
    load(&mut r, modal);
    r.render(FADE_BLOCKS);
    assert_eq!(sounding(&r), 0, "nothing restarted");
}

fn voice_of_part(r: &InstRig) -> usize {
    (0..MAX_VOICES)
        .find(|&v| r.inst.voice(v).is_active())
        .expect("a voice")
}
