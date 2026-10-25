//! `DinPipe`: both parser layers into the note queue and the control ring.
//! What the retired `MidiParser` + `route` queued is frozen here as
//! `old_route_queued`.

use chimera_engine::midi_in::{CtrlEvent, CtrlRing, DinPipe, Sinks};
use chimera_engine::note_queue::{
    BendEvent, ChannelMode, ChannelModeEvent, InEvent, NoteEvent, NoteKind, NoteQueue, RealTime,
    SustainEvent,
};
use chimera_engine::{MidiNote, Velocity};
use chimera_midi::ctl::{Ctl, DataValue, Dir, FreeCc, Sel};
use chimera_midi::value::{Channel, U7, U14};
use proptest::prelude::*;

fn ch(c: u8) -> Channel {
    Channel::new(c).unwrap()
}

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

#[derive(Default)]
struct Rec {
    notes: Vec<InEvent>,
    ctrl: Vec<CtrlEvent>,
    refuse: bool,
}

impl Sinks for Rec {
    fn note(&mut self, e: InEvent) -> bool {
        if self.refuse {
            return false;
        }
        self.notes.push(e);
        true
    }
    fn ctrl(&mut self, e: CtrlEvent) -> bool {
        if self.refuse {
            return false;
        }
        self.ctrl.push(e);
        true
    }
}

fn pipe(bytes: &[u8]) -> Rec {
    let mut p = DinPipe::new();
    let mut r = Rec::default();
    p.feed(bytes, &mut r);
    r
}

/// Notes, bend, the pedal, channel mode (120, 121, 123) and clock, with
/// running status, note-on velocity 0, real-time inside messages, other CCs
/// (not queued) and bytes the old parser also drops.
const OLD_ROUTE_CORPUS: &[u8] = &[
    0x90, 60, 100, 64, 90, 60, 0, // running status, vel 0 = off
    0x80, 64, 40, //
    0xE0, 0x00, 0x40, 0x7F, 0x7F, // bend centre, then max
    0xB0, 64, 127, 64, 0, // pedal down, up
    0xB1, 74, 33, // free CC: not the queue's
    0x91, 0xF8, 62, 0xF8, 99, // clock inside a message
    0xFA, 0xFB, 0xFC, 0xF8, // transport
    0xB2, 120, 0, 121, 0, 123, 0, // mode, running status
    0xC0, 5, // program: not the queue's
    0xD0, 50,   // pressure: not the queue's
    0xFE, // active sensing: not the queue's
    0x93, 70, 80, 0x83, 70, 0,
];

fn on(c: u8, n: u8, v: u8) -> InEvent {
    InEvent::Note(NoteEvent {
        channel: ch(c),
        note: MidiNote::new(n).unwrap(),
        kind: NoteKind::On(Velocity::new(v).unwrap()),
    })
}

fn off(c: u8, n: u8) -> InEvent {
    InEvent::Note(NoteEvent {
        channel: ch(c),
        note: MidiNote::new(n).unwrap(),
        kind: NoteKind::Off,
    })
}

fn mode(c: u8, mode: ChannelMode) -> InEvent {
    InEvent::Mode(ChannelModeEvent {
        channel: ch(c),
        mode,
    })
}

/// What the old `MidiParser` + `route` queued for `OLD_ROUTE_CORPUS`,
/// recorded from it before it was deleted (Task 8, ruling R5).
fn old_route_queued() -> Vec<InEvent> {
    let bend = |value| {
        InEvent::Bend(BendEvent {
            channel: ch(0),
            value,
        })
    };
    let pedal = |down| {
        InEvent::Sustain(SustainEvent {
            channel: ch(0),
            down,
        })
    };
    vec![
        on(0, 60, 100),
        on(0, 64, 90),
        off(0, 60),
        off(0, 64),
        bend(0),
        bend(8191),
        pedal(true),
        pedal(false),
        InEvent::Rt(RealTime::Clock),
        InEvent::Rt(RealTime::Clock),
        on(1, 62, 99),
        InEvent::Rt(RealTime::Start),
        InEvent::Rt(RealTime::Continue),
        InEvent::Rt(RealTime::Stop),
        InEvent::Rt(RealTime::Clock),
        mode(2, ChannelMode::SoundOff),
        mode(2, ChannelMode::ResetControllers),
        mode(2, ChannelMode::NotesOff),
        on(3, 70, 80),
        off(3, 70),
    ]
}

#[test]
fn pipe_matches_old_route_for_notes_bend_sustain_clock() {
    let want = old_route_queued();
    let r = pipe(OLD_ROUTE_CORPUS);
    assert_eq!(r.notes, want);
    // The old route's one CC (74 on channel 2) rides the ring; pressure is
    // new there, and program is dropped (Phase 1).
    let ccs: Vec<_> = r
        .ctrl
        .iter()
        .filter(|e| matches!(e, CtrlEvent::Ctl(_, Ctl::Cc { .. })))
        .collect();
    assert_eq!(
        ccs,
        [&CtrlEvent::Ctl(
            ch(1),
            Ctl::Cc {
                cc: FreeCc::new(74).unwrap(),
                value: u7(33)
            }
        )]
    );
    assert_eq!(r.ctrl.len(), 2, "and channel pressure: {:?}", r.ctrl);
    // The same bytes in awkward slices: the pipe keeps state across `feed`s.
    let mut p = DinPipe::new();
    let mut r = Rec::default();
    for b in OLD_ROUTE_CORPUS.chunks(3) {
        p.feed(b, &mut r);
    }
    assert_eq!(r.notes, want);
}

/// Ported from `chimera-core/tests/midi_route_test.rs`' `route_splits_ccs`:
/// notes, bend, the pedal and clock queue; a free CC rides the ring. CC 0
/// (which the old route rang) is bank select now, held for a program.
#[test]
fn pipe_splits_ccs_from_the_queue() {
    let r = pipe(&[0x90, 60, 100, 0xE0, 100, 0x40, 0xB0, 64, 127, 0xF8]);
    assert_eq!(r.notes.len(), 4);
    assert!(r.ctrl.is_empty());
    for n in [74, 7] {
        let r = pipe(&[0xB0, n, 33]);
        assert!(r.notes.is_empty());
        assert_eq!(
            r.ctrl,
            [CtrlEvent::Ctl(
                ch(0),
                Ctl::Cc {
                    cc: FreeCc::new(n).unwrap(),
                    value: u7(33)
                }
            )]
        );
    }
    let r = pipe(&[0xB0, 0, 33]);
    assert!(r.notes.is_empty() && r.ctrl.is_empty(), "bank select");
}

#[test]
fn cc_124_127_are_notes_off() {
    for n in 124..=127u8 {
        let r = pipe(&[0xB3, n, 0]);
        assert_eq!(
            r.notes,
            [InEvent::Mode(ChannelModeEvent {
                channel: ch(3),
                mode: ChannelMode::NotesOff
            })],
            "cc {n}"
        );
        assert!(r.ctrl.is_empty());
    }
    let r = pipe(&[0xB3, 122, 127]);
    assert!(
        r.notes.is_empty() && r.ctrl.is_empty(),
        "local control: nothing"
    );
}

#[test]
fn reset_is_sound_off_and_reset_on_all_channels() {
    let r = pipe(&[0xFF]);
    assert_eq!(r.notes.len(), 32);
    for c in 0..16u8 {
        for mode in [ChannelMode::SoundOff, ChannelMode::ResetControllers] {
            let want = InEvent::Mode(ChannelModeEvent {
                channel: ch(c),
                mode,
            });
            assert!(r.notes.contains(&want), "ch {c} {mode:?}");
        }
    }
    assert!(r.ctrl.is_empty());
}

#[test]
fn nrpn_arrives_as_one_event() {
    let r = pipe(&[0xB0, 99, 1, 98, 2, 6, 0x40]);
    assert!(r.notes.is_empty());
    assert_eq!(
        r.ctrl,
        [CtrlEvent::Ctl(
            ch(0),
            Ctl::Data {
                sel: Sel::Nrpn(U14::from_parts(u7(1), u7(2))),
                value: DataValue::Coarse(u7(0x40)),
            }
        )]
    );
    let r = pipe(&[0xB5, 74, 9]);
    assert_eq!(
        r.ctrl,
        [CtrlEvent::Ctl(
            ch(5),
            Ctl::Cc {
                cc: FreeCc::new(74).unwrap(),
                value: u7(9)
            }
        )]
    );
}

#[test]
fn identity_request_raises_identity() {
    for dev in [0x7F, 0x00] {
        let r = pipe(&[0xF0, 0x7E, dev, 0x06, 0x01, 0xF7]);
        assert_eq!(r.ctrl, [CtrlEvent::Identity { dev: u7(dev) }]);
    }
    let r = pipe(&[0xF0, 0x7E, 0x05, 0x06, 0x01, 0xF7]);
    assert!(r.ctrl.is_empty(), "another device's request");
}

fn data(i: u16) -> CtrlEvent {
    CtrlEvent::Ctl(
        ch((i % 16) as u8),
        Ctl::Data {
            sel: Sel::Nrpn(U14::new(i).unwrap()),
            value: DataValue::Fine(U14::new(16383 - i).unwrap()),
        },
    )
}

#[test]
fn ring_drop_loses_whole_events() {
    let ring = CtrlRing::new();
    for n in 0..64 {
        assert!(ring.push(data(n)));
    }
    assert!(!ring.push(data(900)));
    assert_eq!(ring.dropped(), 1);
    assert_eq!(ring.pop(), Some(data(0)));
    assert!(ring.push(data(901)), "room freed");
    let all: Vec<_> = ring.drain().collect();
    assert_eq!(all.len(), 64);
    for (i, e) in all[..63].iter().enumerate() {
        assert_eq!(*e, data(i as u16 + 1), "survivor {i} whole");
    }
    assert_eq!(all[63], data(901), "the next one is intact");
    assert_eq!(ring.pop(), None);
}

#[test]
fn cc_flood_never_drops_note_ons() {
    let queue = NoteQueue::new();
    let (mut notes, mut audio) = queue.split().unwrap();
    let ring = CtrlRing::new();
    struct Door<'a, 'q> {
        notes: &'a mut chimera_engine::note_queue::NoteProducer<'q>,
        ring: &'a CtrlRing,
    }
    impl Sinks for Door<'_, '_> {
        fn note(&mut self, e: InEvent) -> bool {
            self.notes.push(e)
        }
        fn ctrl(&mut self, e: CtrlEvent) -> bool {
            self.ring.push(e)
        }
    }
    let mut p = DinPipe::new();
    let (mut sent, mut heard) = (0, 0);
    for i in 0..10_000u32 {
        let mut door = Door {
            notes: &mut notes,
            ring: &ring,
        };
        if i % 300 == 0 {
            sent += 1;
            p.feed(&[0x90, 60, 100], &mut door);
        } else {
            p.feed(&[0xB0, 74, (i % 128) as u8], &mut door);
        }
        if i % 64 == 63 {
            while let Some(e) = audio.pop() {
                assert!(matches!(e, InEvent::Note(_)));
                heard += 1;
            }
        }
    }
    while audio.pop().is_some() {
        heard += 1;
    }
    assert!(sent > 0);
    assert_eq!(heard, sent);
    assert_eq!(queue.dropped(), 0);
    assert!(
        ring.dropped() > 0,
        "the undrained ring fills, not the queue"
    );
    assert_eq!(
        u32::from(p.stats().refused),
        ring.dropped(),
        "refusals are the ring's drops"
    );
}

#[test]
fn active_sensing_arms_and_idle_releases_once() {
    let mut p = DinPipe::new();
    let mut r = Rec::default();
    p.idle(&mut r);
    assert!(r.notes.is_empty() && r.ctrl.is_empty(), "never armed");
    p.feed(&[0xFE], &mut r);
    assert!(p.sensing_armed());
    assert!(
        r.notes.is_empty() && r.ctrl.is_empty(),
        "FE is not an event"
    );
    p.idle(&mut r);
    assert_eq!(r.notes.len(), 16);
    for (c, e) in r.notes.iter().enumerate() {
        assert_eq!(
            *e,
            InEvent::Mode(ChannelModeEvent {
                channel: ch(c as u8),
                mode: ChannelMode::NotesOff
            })
        );
    }
    assert_eq!(r.ctrl, [CtrlEvent::SensingLost]);
    assert!(!p.sensing_armed());
    p.idle(&mut r);
    assert_eq!(r.notes.len(), 16, "once per silence");
}

#[test]
fn stats_count_faults_dropped_programs_and_refusals() {
    let mut p = DinPipe::new();
    let mut r = Rec::default();
    p.feed(&[0xC2, 7, 0xB0, 0, 1, 32, 2, 0xC0, 9, 0x40, 0xF9], &mut r);
    let s = p.stats();
    assert_eq!(s.programs_dropped, 3, "running status counts");
    assert_eq!(s.faults.undefined, 1);
    assert_eq!(s.faults.orphan, 0);
    assert!(r.ctrl.is_empty(), "Phase 1 drops Program Change");
    r.refuse = true;
    p.feed(&[0x90, 60, 1, 0xB0, 74, 1, 0xB0, 74, 2], &mut r);
    assert_eq!(p.stats().refused, 3);
}

fn arb_sel() -> impl Strategy<Value = Sel> {
    (any::<bool>(), 0..=16383u16).prop_map(|(n, v)| {
        let v = U14::new(v).unwrap();
        if n { Sel::Nrpn(v) } else { Sel::Rpn(v) }
    })
}

fn arb_ctrl() -> impl Strategy<Value = CtrlEvent> {
    let c = (0..16u8).prop_map(|c| Channel::new(c).unwrap());
    let v7 = (0..=127u8).prop_map(|v| U7::new(v).unwrap());
    let free = (0..=119u8).prop_filter_map("free", FreeCc::new);
    prop_oneof![
        (c.clone(), free, v7.clone())
            .prop_map(|(c, cc, value)| CtrlEvent::Ctl(c, Ctl::Cc { cc, value })),
        (c.clone(), arb_sel(), v7.clone()).prop_map(|(c, sel, v)| CtrlEvent::Ctl(
            c,
            Ctl::Data {
                sel,
                value: DataValue::Coarse(v)
            }
        )),
        (c.clone(), arb_sel(), 0..=16383u16).prop_map(|(c, sel, v)| CtrlEvent::Ctl(
            c,
            Ctl::Data {
                sel,
                value: DataValue::Fine(U14::new(v).unwrap())
            }
        )),
        (c.clone(), arb_sel(), any::<bool>()).prop_map(|(c, sel, up)| CtrlEvent::Ctl(
            c,
            Ctl::Step {
                sel,
                dir: if up { Dir::Up } else { Dir::Down }
            }
        )),
        (c.clone(), proptest::option::of(0..=16383u16), v7.clone()).prop_map(|(c, b, program)| {
            CtrlEvent::Ctl(
                c,
                Ctl::Program {
                    bank: b.map(|b| U14::new(b).unwrap()),
                    program,
                },
            )
        }),
        (c, v7.clone()).prop_map(|(c, p)| CtrlEvent::Ctl(c, Ctl::Pressure(p))),
        v7.prop_map(|dev| CtrlEvent::Identity { dev }),
        Just(CtrlEvent::SensingLost),
    ]
}

proptest! {
    #[test]
    fn pack_round_trips(e in arb_ctrl()) {
        prop_assert_eq!(CtrlEvent::unpack(e.pack()), Some(e));
    }
}

/// Ported from `chimera-core/tests/midi_route_test.rs`: the map's CC
/// number and the ones it never maps.
#[test]
fn cc_newtype_and_reserved() {
    use chimera_engine::midi_in::Cc;
    assert_eq!(Cc::new(127).map(Cc::get), Some(127));
    assert_eq!(Cc::new(128), None);
    let reserved: Vec<u8> = (0..=127)
        .filter(|&n| Cc::new(n).unwrap().is_reserved())
        .collect();
    let want: Vec<u8> = [0, 6, 32, 38, 64]
        .into_iter()
        .chain(96..=101)
        .chain(120..=127)
        .collect();
    assert_eq!(reserved, want);
}
