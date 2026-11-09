//! MPE (#845, ADR 0252): a zone's member channels play the Parts on its
//! master channel, each note's bend, pressure and timbre (CC 74) its own,
//! glided; the master's bend, pressure and timbre reach the whole zone.

mod common;
use common::{InstRig, tri};

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::modulation::{CUTOFF, FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::mpe::{DEFAULT_ZONE_BEND, LOWER_MASTER, UPPER_MASTER, Zones};
use chimera_core::note_queue::{
    BendEvent, ChannelMode, ChannelModeEvent, Expr, ExprEvent, SustainEvent,
};
use chimera_core::params::PitchParams;
use chimera_midi::value::U7;

const PITCH: ParamAddr = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

/// Part 1 (channel 1) a triangle with `src` into CUTOFF, the lower zone
/// on every other channel.
fn rig(src: ModSource) -> InstRig {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *rig.shared.parts[0].params = tri();
    rig.shared.parts[0].params.filter.cutoff = 200.0;
    let mut m = ModState::with_sources(MAX_MOD_SOURCES);
    m.set_fixed(src.index(), FixedCol::Freq, 60);
    *rig.shared.parts[0].mod_state = m;
    rig.shared.mpe = Zones::OFF.with_lower(15);
    rig
}

fn bend(rig: &mut InstRig, c: u8, value: i16) {
    rig.inst.bend(BendEvent {
        channel: ch(c),
        value,
    });
}

fn send(rig: &mut InstRig, c: u8, expr: Expr) {
    rig.inst.expr(ExprEvent {
        channel: ch(c),
        expr,
    });
}

/// The held voices, in voice order, with their Parts.
fn held(rig: &InstRig) -> Vec<(usize, u8)> {
    let slots = rig.inst.allocator().slots();
    (0..slots.len())
        .filter(|&v| slots[v].held())
        .map(|v| (v, slots[v].part().unwrap()))
        .collect()
}

/// The held voice playing `note`.
fn voice(rig: &InstRig, note: u8) -> usize {
    let slots = rig.inst.allocator().slots();
    (0..slots.len())
        .find(|&v| slots[v].held() && slots[v].note().map(|n| n.get()) == Some(note))
        .unwrap()
}

#[test]
fn zones_never_overlap_and_member_channels_name_their_master() {
    let z = Zones::OFF.with_lower(5).with_upper(5);
    assert_eq!((z.lower(), z.upper()), (5, 5));
    assert_eq!(z.member(ch(0)), None, "the lower master");
    assert_eq!(z.member(ch(1)), Some((LOWER_MASTER, DEFAULT_ZONE_BEND)));
    assert_eq!(z.member(ch(5)), Some((LOWER_MASTER, DEFAULT_ZONE_BEND)));
    assert_eq!(z.member(ch(6)), None);
    assert_eq!(z.member(ch(9)), None);
    assert_eq!(z.member(ch(10)), Some((UPPER_MASTER, DEFAULT_ZONE_BEND)));
    assert_eq!(z.member(ch(14)), Some((UPPER_MASTER, DEFAULT_ZONE_BEND)));
    assert_eq!(z.member(ch(15)), None, "the upper master");
    assert_eq!(z.master_of(ch(7)), ch(7));
    assert_eq!(z.master_of(ch(12)), UPPER_MASTER);
    // A zone grown into the other shrinks it; 15 leaves no upper zone.
    assert_eq!(z.with_lower(12).upper(), 2);
    assert_eq!(z.with_lower(15).upper(), 0);
    assert_eq!(z.with_upper(14).lower(), 0);
    assert_eq!(
        Zones::OFF.with_lower(15).member(ch(15)).unwrap().0,
        LOWER_MASTER
    );
    // Off: every channel its own.
    assert!((0..16).all(|c| Zones::OFF.member(ch(c)).is_none()));
}

#[test]
fn a_configuration_message_sets_its_zone_and_the_spec_s_bend() {
    let z = Zones::OFF.with_lower_bend(12);
    let l = z.configured(LOWER_MASTER, 7);
    assert_eq!((l.lower(), l.lower_bend()), (7, DEFAULT_ZONE_BEND));
    let u = l.configured(UPPER_MASTER, 9);
    assert_eq!((u.lower(), u.upper()), (5, 9), "the newer zone wins");
    assert_eq!(u.configured(ch(3), 4), u, "only a master configures");
    assert_eq!(u.configured(LOWER_MASTER, 0).lower(), 0, "0 turns it off");
    assert_eq!(u.configured(LOWER_MASTER, 99).lower(), 15);
    assert_eq!(u.with_bend_on(ch(1), 24).lower_bend(), 24);
    assert_eq!(u.with_bend_on(ch(14), 99).upper_bend(), 96);
    assert_eq!(l.with_bend_on(ch(10), 24), l, "no zone's channel");
}

#[test]
fn member_channels_play_the_master_s_parts_alone() {
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(2, 60);
    rig.note_on(5, 64);
    rig.render(2);
    let parts: Vec<u8> = held(&rig).iter().map(|&(_, p)| p).collect();
    assert_eq!(parts, [0, 0], "Part 1 plays both, Parts 3 and 6 neither");
    rig.note_off(2, 60);
    rig.note_off(5, 64);
    assert!(held(&rig).is_empty(), "each released on its own channel");
}

#[test]
fn with_mpe_off_a_channel_plays_its_own_parts() {
    let mut rig = rig(ModSource::Wheel);
    rig.shared.mpe = Zones::OFF;
    rig.note_on(2, 60);
    rig.render(2);
    let parts: Vec<u8> = held(&rig).iter().map(|&(_, p)| p).collect();
    assert_eq!(parts, [2]);
}

#[test]
fn each_note_bends_on_its_own_channel_over_the_zone_s_range() {
    let mut rig = rig(ModSource::Wheel);
    rig.shared.mpe = rig.shared.mpe.with_lower_bend(12);
    rig.note_on(1, 60);
    rig.note_on(2, 64);
    rig.render(2);
    let (a, b) = (voice(&rig, 60), voice(&rig, 64));
    let base = rig.inst.played(PITCH);
    bend(&mut rig, 2, 8191);
    rig.render(1);
    let first = rig.inst.played(PITCH)[b] - base[b];
    assert!(first > 0.0 && first < 2.0, "glides, never steps: {first}");
    rig.render(400);
    let now = rig.inst.played(PITCH);
    assert_eq!(now[a], base[a], "the other note holds");
    assert!((now[b] - base[b] - 12.0).abs() < 1e-3);
    bend(&mut rig, 2, -8192);
    rig.render(400);
    assert!((rig.inst.played(PITCH)[b] - base[b] + 12.0).abs() < 1e-3);
}

#[test]
fn the_master_s_bend_moves_the_whole_zone_on_top() {
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(1, 60);
    rig.note_on(2, 64);
    rig.render(2);
    let (a, b) = (voice(&rig, 60), voice(&rig, 64));
    let base = rig.inst.played(PITCH);
    bend(&mut rig, 0, 8191);
    bend(&mut rig, 1, -8192);
    rig.render(400);
    let now = rig.inst.played(PITCH);
    // Part BEND 2 for the master; ±48 a member.
    assert!((now[b] - base[b] - 2.0).abs() < 1e-3);
    assert!((now[a] - base[a] - 2.0 + 48.0).abs() < 1e-3);
}

#[test]
fn pressure_and_timbre_on_a_member_reach_its_note_alone() {
    for (src, e) in [
        (ModSource::Press, Expr::Pressure(u7(127))),
        (ModSource::Timbre, Expr::Timbre(u7(127))),
    ] {
        let mut rig = rig(src);
        rig.note_on(1, 60);
        rig.note_on(2, 64);
        rig.render(4);
        let (a, b) = (voice(&rig, 60), voice(&rig, 64));
        let base = rig.inst.played(CUTOFF);
        send(&mut rig, 2, e);
        rig.render(1);
        let first = rig.inst.played(CUTOFF);
        assert!(first[b] > base[b], "{src:?}");
        rig.render(400);
        let full = rig.inst.played(CUTOFF);
        assert_eq!(full[a], base[a], "{src:?}: the other note");
        assert!(
            full[b] - base[b] > 10.0 * (first[b] - base[b]),
            "{src:?} glides"
        );
        // The master's reaches both.
        send(&mut rig, 0, e);
        rig.render(400);
        let both = rig.inst.played(CUTOFF);
        assert_eq!(both[a], full[b], "{src:?}");
    }
}

#[test]
fn timbre_on_a_part_s_channel_moves_its_voices_without_mpe() {
    let mut rig = rig(ModSource::Timbre);
    rig.shared.mpe = Zones::OFF;
    rig.note_on(0, 60);
    rig.render(4);
    let v = held(&rig)[0].0;
    let base = rig.inst.played(CUTOFF)[v];
    send(&mut rig, 0, Expr::Timbre(u7(127)));
    rig.render(400);
    assert!(rig.inst.played(CUTOFF)[v] > base);
    rig.inst.channel_mode(ChannelModeEvent {
        channel: ch(0),
        mode: ChannelMode::ResetControllers,
    });
    rig.render(400);
    assert_eq!(rig.inst.played(CUTOFF)[v], base, "CC 121 zeroes it");
}

#[test]
fn the_master_s_pedal_and_notes_off_hold_and_free_the_zone() {
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(1, 60);
    rig.note_on(2, 64);
    rig.inst.sustain(SustainEvent {
        channel: ch(0),
        down: true,
    });
    rig.note_off(1, 60);
    rig.note_off(2, 64);
    assert_eq!(held(&rig).len(), 2, "the master's pedal holds them");
    rig.inst.sustain(SustainEvent {
        channel: ch(0),
        down: false,
    });
    assert!(held(&rig).is_empty());
    rig.note_on(3, 67);
    rig.inst.channel_mode(ChannelModeEvent {
        channel: ch(0),
        mode: ChannelMode::NotesOff,
    });
    assert!(held(&rig).is_empty(), "CC 123 on the master");
}

#[test]
fn a_member_s_reset_zeroes_its_note_s_bend() {
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(1, 60);
    rig.render(2);
    let v = held(&rig)[0].0;
    let base = rig.inst.played(PITCH)[v];
    bend(&mut rig, 1, 8191);
    rig.render(400);
    assert!(rig.inst.played(PITCH)[v] > base + 40.0);
    rig.inst.channel_mode(ChannelModeEvent {
        channel: ch(1),
        mode: ChannelMode::ResetControllers,
    });
    rig.render(400);
    assert_eq!(rig.inst.played(PITCH)[v], base);
}

/// Unrouted, timbre and a zone change nothing: a note on the master, no
/// member, renders bit for bit as with MPE off.
#[test]
fn a_zone_s_master_note_plays_as_without_one() {
    let play = |mpe: bool| {
        let mut rig = rig(ModSource::Wheel);
        if !mpe {
            rig.shared.mpe = Zones::OFF;
        }
        rig.note_on(0, 60);
        let mut out = Vec::new();
        for i in 0..64 {
            if i == 8 {
                send(&mut rig, 0, Expr::Timbre(u7(127)));
            }
            rig.render(1);
            out.extend_from_slice(&rig.out()[0]);
        }
        out
    };
    let (off, on) = (play(false), play(true));
    assert!(off.iter().any(|&x| x != 0.0));
    assert!(off.iter().zip(&on).all(|(a, b)| a.to_bits() == b.to_bits()));
}

fn orbit_note(rig: &mut InstRig, c: u8, note: u8, on: bool) {
    use chimera_core::instrument::NoteOrigin;
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    use chimera_core::orbit::RingIdx;
    let ev = NoteEvent {
        channel: ch(c),
        note: chimera_core::MidiNote::new(note).unwrap(),
        kind: if on {
            NoteKind::On(chimera_core::Velocity::DEFAULT)
        } else {
            NoteKind::Off
        },
    };
    let shared = &*rig.shared;
    rig.inst
        .handle_from(ev, shared, NoteOrigin::Orbit(RingIdx::new(0).unwrap()));
}

/// ORBIT sends on its Part's channel: a zone over that channel neither
/// takes the note to the master's Parts nor gives it the channel's bend.
#[test]
fn orbit_notes_on_a_member_channel_stay_their_part_s() {
    let mut rig = rig(ModSource::Wheel);
    orbit_note(&mut rig, 2, 60, true);
    rig.render(2);
    let parts: Vec<u8> = held(&rig).iter().map(|&(_, p)| p).collect();
    assert_eq!(parts, [2], "Part 3's ring plays Part 3");
    let v = voice(&rig, 60);
    let base = rig.inst.played(PITCH)[v];
    bend(&mut rig, 2, 8191);
    send(&mut rig, 2, Expr::Pressure(u7(127)));
    rig.render(400);
    // Its Part's BEND (2) from its channel's wheel, not the zone's 48.
    let moved = rig.inst.played(PITCH)[v] - base;
    assert!((moved - 2.0).abs() < 1e-3, "{moved}");
    orbit_note(&mut rig, 2, 60, false);
    assert!(held(&rig).is_empty());
}

/// A zone changed while the pedal holds a member note: the pedal up and
/// All Notes Off still find it by the master it played on.
#[test]
fn a_zone_change_never_strands_a_held_note() {
    let pedal = |rig: &mut InstRig, down| {
        rig.inst.sustain(SustainEvent {
            channel: ch(0),
            down,
        })
    };
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(3, 60);
    pedal(&mut rig, true);
    rig.note_off(3, 60);
    rig.shared.mpe = Zones::OFF;
    rig.render(2);
    assert_eq!(held(&rig).len(), 1, "held by the pedal");
    pedal(&mut rig, false);
    assert!(held(&rig).is_empty(), "the master's pedal up lets it go");

    rig.shared.mpe = Zones::OFF.with_lower(15);
    rig.note_on(4, 62);
    rig.shared.mpe = Zones::OFF;
    rig.inst.channel_mode(ChannelModeEvent {
        channel: ch(0),
        mode: ChannelMode::NotesOff,
    });
    assert!(held(&rig).is_empty(), "CC 123 on its master");
}
