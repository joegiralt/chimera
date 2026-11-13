//! Incoming pitch bend and the sustain pedal (#437): per Part, smoothed,
//! on every engine; every other CC is the Part's MIDI map's (#247).

mod common;
use common::{InstRig, Rig, SR, goertzel, init_params, tri};

use chimera_core::block::{Block, GlideMs};
use chimera_core::dsp::knobs::coeff;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{BendEvent, SustainEvent};
use chimera_core::params::EngineType;
use chimera_core::part::{INIT_BEND, MAX_BEND, PartParams};
use chimera_core::project::{PartId, Project};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

fn wheel(rig: &mut InstRig, c: u8, value: i16) {
    rig.inst.bend(BendEvent {
        channel: ch(c),
        value,
    });
}

fn pedal(rig: &mut InstRig, c: u8, down: bool) {
    rig.inst.sustain(SustainEvent {
        channel: ch(c),
        down,
    });
}

/// The strongest bin between 100 Hz and 4 kHz of 8192 samples.
fn peak_hz(x: &[f32]) -> f32 {
    let w = &x[256..256 + 8192];
    let bin = SR as f32 / w.len() as f32;
    (17..=700)
        .map(|k| k as f32 * bin)
        .max_by(|&a, &b| goertzel(w, a, SR).total_cmp(&goertzel(w, b, SR)))
        .unwrap()
}

fn held(rig: &InstRig) -> usize {
    rig.inst
        .allocator()
        .slots()
        .iter()
        .filter(|s| s.held())
        .count()
}

fn rev_v() -> InstRig {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *rig.shared.parts[0].params = tri();
    rig
}

// ---- BEND, the range ----

#[test]
fn bend_defaults_to_two_semitones_and_clamps_to_its_range() {
    assert_eq!(PartParams::default().bend, INIT_BEND);
    assert_eq!(INIT_BEND, 2);
    let mut m = PartParams::default();
    for (v, want) in [(0.0, 0), (7.0, 7), (24.0, 24), (99.0, 24), (-5.0, 0)] {
        m.write(PartParams::BEND, v);
        assert_eq!(m.bend, want);
        assert_eq!(m.get(PartParams::BEND), f32::from(want));
    }
    let spec = chimera_core::part::PART_SPECS
        .iter()
        .find(|s| s.id == PartParams::BEND)
        .unwrap();
    assert_eq!((spec.min, spec.max, spec.default), (0.0, 24.0, 2.0));
    assert_eq!(spec.label, "BEND");
    assert_eq!(MAX_BEND, 24);
}

#[test]
fn the_wheel_reaches_the_range_each_way_and_a_range_of_zero_does_nothing() {
    let mut rig = rev_v();
    rig.shared.parts[0].mix.bend = 7;
    rig.note_on(0, 60);
    wheel(&mut rig, 0, 8191);
    rig.render(400);
    assert!((rig.inst.bend_of(0) - 7.0).abs() < 1e-3);
    wheel(&mut rig, 0, -8192);
    rig.render(400);
    assert!((rig.inst.bend_of(0) + 7.0).abs() < 1e-3);
    rig.shared.parts[0].mix.bend = 0;
    rig.render(400);
    assert_eq!(rig.inst.bend_of(0), 0.0);
}

#[test]
fn a_wheel_bends_only_the_parts_on_its_channel() {
    let mut rig = rev_v();
    rig.note_on(0, 60);
    rig.note_on(1, 60);
    wheel(&mut rig, 1, 8191);
    rig.render(400);
    assert_eq!(rig.inst.bend_of(0), 0.0);
    assert!((rig.inst.bend_of(1) - 2.0).abs() < 1e-3);
}

// ---- smoothing ----

#[test]
fn the_bend_glides_no_faster_than_a_knob() {
    let mut rig = rev_v();
    rig.shared.parts[0].mix.bend = 12;
    rig.note_on(0, 60);
    rig.render(2);
    let k = coeff(GlideMs::KNOB, SR);
    let mut prev = rig.inst.bend_of(0);
    assert_eq!(prev, 0.0);
    for jump in [8191, -8192, 0] {
        wheel(&mut rig, 0, jump);
        let to = f32::from(jump) / if jump > 0 { 8191.0 } else { 8192.0 } * 12.0;
        for _ in 0..400 {
            let before = prev;
            rig.render(1);
            let now = rig.inst.bend_of(0);
            // A one-pole: the block's step is (1 - k) of what was left.
            let allowed = (1.0 - k) * (to - before).abs() + 1e-4;
            assert!((now - before).abs() <= allowed, "{before} -> {now} to {to}");
            prev = now;
        }
        assert!((prev - to).abs() < 1e-3, "{prev} lands on {to}");
    }
}

#[test]
fn a_silent_part_lands_its_bend() {
    let mut rig = rev_v();
    wheel(&mut rig, 0, 8191);
    rig.render(1);
    assert!((rig.inst.bend_of(0) - 2.0).abs() < 1e-6);
}

// ---- every engine follows ----

#[test]
fn the_bend_moves_the_pitch_of_every_engine() {
    let f0 = chimera_core::dsp::note_to_freq(60);
    // DRUM plays pads, not notes: `drum_engine_test`.
    for engine in EngineType::ALL
        .into_iter()
        .filter(|&e| e != EngineType::Drum)
    {
        let p = init_params(engine);
        // The window's energy at `hz`, a note of C4 bent by `bend`.
        let energy = |bend: f32, hz: f32| {
            let mut v = Rig::new(SR);
            let mut blk = [0.0f32; BLOCK_SIZE];
            // A throwaway note puts the voice on the engine, so the note
            // under test is struck on it, not after a switch's fade.
            v.note_on(MidiNote::new(48).unwrap(), Velocity::DEFAULT, &p);
            let _ = v.kill();
            for _ in 0..40 {
                v.render_glided(&mut blk, &p, &ModState::new());
            }
            v.set_bend(bend);
            v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p);
            let mut out = Vec::new();
            for _ in 0..200 {
                v.render_glided(&mut blk, &p, &ModState::new());
                out.extend_from_slice(&blk);
            }
            goertzel(&out[256..256 + 8192], hz, SR)
        };
        // Up: the unbent fundamental empties. Down: the one an octave
        // below, which the unbent note has not, fills.
        let (bent, flat) = (energy(12.0, f0), energy(0.0, f0));
        assert!(
            bent < flat / 3.0,
            "{engine:?} +12: {bent} vs {flat} at {f0} Hz"
        );
        let low = f0 / 2.0;
        let (bent, flat) = (energy(-12.0, low), energy(0.0, low));
        assert!(
            bent > 3.0 * flat,
            "{engine:?} -12: {bent} vs {flat} at {low} Hz"
        );
    }
}

#[test]
fn the_wheel_bends_a_note_through_the_instrument() {
    let mut rig = rev_v();
    rig.shared.parts[0].mix.bend = 12;
    rig.note_on(0, 60);
    let mut out = Vec::new();
    for _ in 0..200 {
        rig.render(1);
        out.extend_from_slice(rig.inst.part_bus(0));
    }
    let flat = peak_hz(&out);
    wheel(&mut rig, 0, 8191);
    out.clear();
    for _ in 0..400 {
        rig.render(1);
        out.extend_from_slice(rig.inst.part_bus(0));
    }
    let ratio = peak_hz(&out[out.len() - 8448..]) / flat;
    assert!((ratio - 2.0).abs() < 0.03, "x{ratio}");
}

// ---- sustain ----

#[test]
fn the_pedal_holds_a_released_note_until_it_is_up() {
    let mut rig = rev_v();
    pedal(&mut rig, 0, true);
    rig.note_on(0, 60);
    rig.render(10);
    rig.note_off(0, 60);
    rig.render(600);
    assert_eq!(held(&rig), 1, "held through the pedal");
    assert!(rig.inst.sounding() > 0);
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 0);
    rig.render(2000);
    assert!(rig.inst.quiet(), "released with the pedal");
}

#[test]
fn a_pedal_down_after_the_key_holds_nothing_and_a_key_up_before_it_is_free() {
    let mut rig = rev_v();
    rig.note_on(0, 60);
    rig.note_off(0, 60);
    pedal(&mut rig, 0, true);
    assert_eq!(held(&rig), 0);
    // A key still down is held by its key, not the pedal; the pedal's
    // release leaves it.
    rig.note_on(0, 62);
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 1);
    rig.note_off(0, 62);
    assert_eq!(held(&rig), 0);
}

#[test]
fn the_pedal_is_per_channel() {
    let mut rig = rev_v();
    pedal(&mut rig, 1, true);
    rig.note_on(0, 60);
    rig.note_off(0, 60);
    assert_eq!(held(&rig), 0, "channel 1's pedal does not hold channel 0");
    rig.note_on(1, 60);
    rig.note_off(1, 60);
    assert_eq!(held(&rig), 1);
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 1, "channel 0's pedal up leaves channel 1's");
    pedal(&mut rig, 1, false);
    assert_eq!(held(&rig), 0);
}

#[test]
fn a_re_strike_while_held_is_one_note_the_pedal_still_ends() {
    let mut rig = rev_v();
    pedal(&mut rig, 0, true);
    rig.note_on(0, 60);
    rig.note_off(0, 60);
    rig.note_on(0, 60); // struck again over the held one
    assert_eq!(held(&rig), 1, "it takes the sustained voice back");
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 1, "the key is down: the pedal leaves it");
    rig.note_off(0, 60);
    assert_eq!(held(&rig), 0);
    // And lifted with the pedal down again: held again, ended by the pedal.
    pedal(&mut rig, 0, true);
    rig.note_on(0, 60);
    rig.note_off(0, 60);
    rig.note_on(0, 60);
    rig.note_off(0, 60);
    assert_eq!(held(&rig), 1);
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 0);
}

#[test]
fn a_sustained_note_can_be_stolen_and_the_pedal_then_leaves_its_thief() {
    let mut rig = rev_v();
    pedal(&mut rig, 0, true);
    rig.note_on(0, 30);
    rig.note_off(0, 30); // the oldest, sustained
    let keys = 2 * MAX_VOICES as u8;
    for n in 0..keys {
        rig.note_on(0, 40 + n); // held keys fill the pool and steal
    }
    rig.render(20);
    let before = held(&rig);
    assert!(before <= MAX_VOICES && before > 0);
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), before, "no key was let go by the pedal");
    rig.render(20);
    for n in 0..keys {
        rig.note_off(0, 40 + n);
    }
    assert_eq!(held(&rig), 0);
}

#[test]
fn modal_keeps_ringing_under_the_pedal_as_it_does_without() {
    use chimera_core::params::EngineType;
    let mut rig = rev_v();
    *rig.shared.parts[0].params = init_params(EngineType::Modal);
    pedal(&mut rig, 0, true);
    rig.note_on(0, 60);
    rig.note_off(0, 60);
    rig.render(50);
    assert_eq!(held(&rig), 1);
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 0);
    rig.render(20);
    assert!(rig.inst.sounding() > 0, "released Modal rings on");
}

#[test]
fn orbit_played_notes_ignore_the_pedal() {
    use chimera_core::instrument::NoteOrigin;
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    let mut rig = rev_v();
    let ring = chimera_core::orbit::RingIdx::ALL[0];
    let ev = |kind| NoteEvent {
        channel: ch(0),
        note: MidiNote::new(60).unwrap(),
        kind,
    };
    pedal(&mut rig, 0, true);
    rig.inst.handle_from(
        ev(NoteKind::On(Velocity::DEFAULT)),
        &rig.shared,
        NoteOrigin::Orbit(ring),
    );
    rig.inst
        .handle_from(ev(NoteKind::Off), &rig.shared, NoteOrigin::Orbit(ring));
    assert_eq!(held(&rig), 0, "a ring's note ends on its own");
}

// ---- storage ----

#[test]
fn bend_round_trips_through_a_project_and_an_old_file_reads_the_default() {
    use common::codec_util::{decode_project, encode_project};
    let (mut p, _) = Project::boxed();
    p.edit_part(PartId::ALL[2]).mix.bend = 12;
    p.edit_part(PartId::ALL[4]).mix.bend = 0;
    let bytes = encode_project(&p);
    let (mut q, _) = Project::boxed();
    decode_project(&bytes, &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[2]).mix.bend, 12);
    assert_eq!(q.part(PartId::ALL[4]).mix.bend, 0);
    assert_eq!(q.part(PartId::ALL[0]).mix.bend, INIT_BEND);
    // A file written before BEND has no entry for it.
    let f = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/v1/project.prj"
    ))
    .unwrap();
    let (mut old, _) = Project::boxed();
    decode_project(&f, &mut old).unwrap();
    for id in PartId::ALL {
        assert_eq!(old.part(id).mix.bend, INIT_BEND);
    }
}

// ---- channel mode: CC 120, 121, 123 (#501) ----

use chimera_core::note_queue::{ChannelMode, ChannelModeEvent, InEvent};

fn mode(rig: &mut InstRig, c: u8, mode: ChannelMode) {
    rig.inst.channel_mode(ChannelModeEvent {
        channel: ch(c),
        mode,
    });
}

#[test]
fn channel_mode_ccs_reach_the_queue_as_events() {
    use chimera_core::midi_in::{CtrlEvent, DinPipe, Sinks};
    struct Notes(Vec<InEvent>);
    impl Sinks for Notes {
        fn note(&mut self, e: InEvent) -> bool {
            self.0.push(e);
            true
        }
        fn ctrl(&mut self, _: CtrlEvent) -> bool {
            true
        }
    }
    for (cc, want) in [
        (120, ChannelMode::SoundOff),
        (121, ChannelMode::ResetControllers),
        (123, ChannelMode::NotesOff),
    ] {
        let mut got = Notes(Vec::new());
        DinPipe::new().feed(&[0xB3, cc, 0], &mut got);
        assert_eq!(
            got.0,
            [InEvent::Mode(ChannelModeEvent {
                channel: ch(3),
                mode: want
            })]
        );
    }
}

#[test]
fn all_notes_off_releases_its_channel_only() {
    let mut rig = rev_v();
    rig.note_on(0, 60);
    rig.note_on(1, 62);
    mode(&mut rig, 0, ChannelMode::NotesOff);
    assert_eq!(held(&rig), 1, "channel 1 still holds");
    mode(&mut rig, 1, ChannelMode::NotesOff);
    assert_eq!(held(&rig), 0);
}

#[test]
fn all_notes_off_under_the_pedal_holds_until_it_is_up() {
    let mut rig = rev_v();
    pedal(&mut rig, 0, true);
    rig.note_on(0, 60);
    rig.note_on(0, 64);
    mode(&mut rig, 0, ChannelMode::NotesOff);
    rig.render(50);
    assert_eq!(held(&rig), 2, "the pedal holds both voices");
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 0);
}

/// `midi_in::lost` under a pedal: NotesOff on every channel, then the
/// pedal up, frees a held key and a sustained one.
#[test]
fn a_lost_source_under_the_pedal_frees_every_voice() {
    let mut rig = rev_v();
    pedal(&mut rig, 0, true);
    rig.note_on(0, 60);
    rig.note_on(0, 64);
    rig.note_off(0, 60);
    for c in 0..16 {
        mode(&mut rig, c, ChannelMode::NotesOff);
    }
    assert_eq!(held(&rig), 2, "both sustained");
    pedal(&mut rig, 0, false);
    assert_eq!(held(&rig), 0);
}

#[test]
fn modal_rings_on_after_all_notes_off() {
    let mut rig = rev_v();
    *rig.shared.parts[0].params = init_params(EngineType::Modal);
    rig.note_on(0, 60);
    rig.render(20);
    mode(&mut rig, 0, ChannelMode::NotesOff);
    assert_eq!(held(&rig), 0);
    rig.render(20);
    assert!(rig.inst.sounding() > 0);
}

#[test]
fn all_sound_off_fades_its_channel_without_a_click() {
    let mut rig = rev_v();
    *rig.shared.parts[1].params = tri();
    rig.note_on(0, 60);
    rig.note_on(1, 60);
    rig.render(100);
    let mut out = Vec::new();
    for _ in 0..4 {
        rig.render(1);
        out.extend_from_slice(rig.inst.part_bus(0));
    }
    mode(&mut rig, 0, ChannelMode::SoundOff);
    for _ in 0..20 {
        rig.render(1);
        out.extend_from_slice(rig.inst.part_bus(0));
    }
    assert!(
        common::clicks(&out).is_empty(),
        "{:?}",
        common::clicks(&out)
    );
    assert!(
        out[out.len() - 256..].iter().all(|x| x.abs() < 1e-6),
        "silent"
    );
    assert_eq!(held(&rig), 1, "channel 1 plays on");
    assert!(rig.inst.part_bus(1).iter().any(|x| x.abs() > 1e-3));
    // Silent within the fade: the first block already ramps down.
    let mut late = rev_v();
    late.note_on(0, 60);
    late.render(100);
    mode(&mut late, 0, ChannelMode::SoundOff);
    late.render(3);
    assert!(late.inst.part_bus(0).iter().all(|x| x.abs() < 1e-6));
}

#[test]
fn reset_controllers_centres_the_bend_and_lifts_the_pedal_on_its_channel() {
    let mut rig = rev_v();
    rig.note_on(0, 60);
    rig.note_on(1, 60);
    wheel(&mut rig, 0, 8191);
    wheel(&mut rig, 1, 8191);
    pedal(&mut rig, 0, true);
    pedal(&mut rig, 1, true);
    rig.note_off(0, 60);
    rig.note_off(1, 60);
    rig.render(400);
    mode(&mut rig, 0, ChannelMode::ResetControllers);
    rig.render(400);
    assert_eq!(held(&rig), 1, "channel 0's pedal is up, 1's is down");
    assert!(rig.inst.bend_of(0).abs() < 1e-3);
    assert!((rig.inst.bend_of(1) - 2.0).abs() < 1e-3);
}
