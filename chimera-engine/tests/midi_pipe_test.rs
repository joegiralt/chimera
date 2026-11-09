//! `DinPipe`: both parser layers into the note queue and the control ring.
//! What the retired `MidiParser` + `route` queued is frozen here as
//! `old_route_queued`.

use chimera_engine::midi_in::{
    self, CTRL_RING_LEN, CTRL_USB_LEN, CtrlEvent, CtrlPorts, CtrlRing, DinPipe, DinSinks, Left,
    PipeSlot, Port, Sinks, UsbSlot,
};
use chimera_engine::note_queue::{
    BendEvent, ChannelMode, ChannelModeEvent, Expr, ExprEvent, InEvent, NOTE_QUEUE_LEN,
    NoteConsumer, NoteEvent, NoteKind, NoteProducer, NoteQueue, RealTime, SustainEvent,
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
    0xB1, 71, 33, // free CC: not the queue's
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
        // PRESSURE's source (#514); the map hears it too.
        expr(0, Expr::Pressure(u7(50))),
        on(3, 70, 80),
        off(3, 70),
    ]
}

fn expr(c: u8, expr: Expr) -> InEvent {
    InEvent::Expr(ExprEvent {
        channel: ch(c),
        expr,
    })
}

#[test]
fn pipe_matches_old_route_for_notes_bend_sustain_clock() {
    let want = old_route_queued();
    let r = pipe(OLD_ROUTE_CORPUS);
    assert_eq!(r.notes, want);
    // The old route's one CC (71 on channel 2) rides the ring; pressure is
    // new there, and so is the program change (#515).
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
                cc: FreeCc::new(71).unwrap(),
                value: u7(33)
            }
        )]
    );
    assert_eq!(
        r.ctrl.len(),
        3,
        "and the program and pressure: {:?}",
        r.ctrl
    );
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
    for n in [71, 7] {
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
    // It forgets the NRPN selected: the data entry after it is inert.
    let r = pipe(&[0xB0, 99, 1, 98, 2, 0xFF, 0xB0, 6, 9]);
    assert!(r.ctrl.is_empty(), "{:?}", r.ctrl);
}

/// A Reset into a busy queue: each channel whose SoundOff it refused is
/// owed a NotesOff, and each whose pedal was down a pedal-up, so no note
/// sticks (#820).
#[test]
fn a_reset_refused_owes_notes_off() {
    let mut usb = Usb::new();
    usb.run(&[0xB7, 64, 127]); // channel 7's pedal down
    usb.fill();
    usb.pop(10); // room for ch 0..=4's two events
    usb.feed(&[0xFF]);
    usb.take();
    let mut want: Vec<_> = (5..16).map(|c| mode(c, ChannelMode::NotesOff)).collect();
    want.push(InEvent::Sustain(SustainEvent {
        channel: ch(7),
        down: false,
    }));
    assert_eq!(usb.settle(), want);
    assert_eq!(usb.settle(), [], "once");
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
    let r = pipe(&[0xB5, 71, 9]);
    assert_eq!(
        r.ctrl,
        [CtrlEvent::Ctl(
            ch(5),
            Ctl::Cc {
                cc: FreeCc::new(71).unwrap(),
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
    let ring: CtrlRing = CtrlRing::new();
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

/// USB's ring takes a host's burst whole: 3,780 B on the unit, 420 note
/// pairs each with a CC, landed between two UI laps. DIN's stays 64.
#[test]
fn usb_ring_takes_a_burst_whole() {
    assert_eq!(CTRL_RING_LEN, 64, "DIN's ring unchanged");
    assert_eq!(core::mem::size_of::<CtrlRing>(), 64 * 8 + 12);
    let ring: CtrlRing<CTRL_USB_LEN> = CtrlRing::new();
    for n in 0..420 {
        assert!(ring.push(data(n)), "CC {n} refused");
    }
    assert_eq!(ring.dropped(), 0);
    assert!(ring.drain().eq((0..420).map(data)), "in order, whole");
}

/// A build with no MIDI map drops the ring unread each pass, so a flood
/// never counts as a drop.
#[test]
fn clear_drops_what_waits_and_frees_the_ring() {
    let ring: CtrlRing = CtrlRing::new();
    for n in 0..64 {
        assert!(ring.push(data(n)));
    }
    ring.clear();
    assert_eq!(ring.pop(), None);
    for n in 0..64 {
        assert!(ring.push(data(n)), "room for a whole ring again");
    }
    assert_eq!(ring.dropped(), 0);
    assert_eq!(ring.pop(), Some(data(0)));
}

/// One drain over both ports: DIN's first, each tagged with its port.
#[test]
fn ports_drain_din_then_usb_tagged() {
    let ports: CtrlPorts = CtrlPorts::new();
    assert!(ports.usb.push(data(10)));
    assert!(ports.din.push(data(1)));
    assert!(ports.usb.push(data(11)));
    assert!(ports.din.push(data(2)));
    let all: Vec<_> = ports.drain().collect();
    assert_eq!(
        all,
        [
            (Port::Din, data(1)),
            (Port::Din, data(2)),
            (Port::Usb, data(10)),
            (Port::Usb, data(11)),
        ]
    );
    assert_eq!(ports.drain().next(), None);
}

#[test]
fn ports_clear_both() {
    let ports: CtrlPorts<64> = CtrlPorts::new();
    assert!(ports.din.push(data(1)) && ports.usb.push(data(2)));
    ports.clear();
    assert_eq!(ports.drain().next(), None);
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
            p.feed(&[0xB0, 71, (i % 128) as u8], &mut door);
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
    assert_eq!(p.stats().refused, 0, "the queue refused nothing");
    assert_eq!(
        u32::from(p.stats().ctrl_refused),
        ring.dropped(),
        "the ring's drops, counted apart"
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

/// A port's slot as its shell drives it: DIN's through `midi_in::feed`
/// and `idle` (the USART1 ISR's path), USB's through its own.
trait Slot {
    fn new() -> Self;
    /// Empty: DIN's receiver timed out; USB's poll retries what is owed.
    fn feed(&mut self, bytes: &[u8], out: &mut DinSinks<'_, '_>);
}

impl Slot for PipeSlot {
    fn new() -> Self {
        PipeSlot::new()
    }

    fn feed(&mut self, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
        if bytes.is_empty() {
            midi_in::idle(self, out);
        } else {
            midi_in::feed(self, bytes, out);
        }
    }
}

impl Slot for UsbSlot {
    fn new() -> Self {
        UsbSlot::new()
    }

    fn feed(&mut self, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
        UsbSlot::feed(self, bytes, out);
    }
}

/// A slot into a note queue and a control ring, armed read off DIN's slot
/// as the ISR reads it to set the receiver timeout's interrupt.
struct Rig<P> {
    notes: NoteProducer<'static>,
    audio: NoteConsumer<'static>,
    ring: &'static CtrlRing,
    slot: P,
}

type Din = Rig<PipeSlot>;
/// USB's: held keys and pedals, `lost`.
type Usb = Rig<UsbSlot>;

impl<P: Slot> Rig<P> {
    fn new() -> Self {
        let (notes, audio) = Box::leak(Box::new(NoteQueue::new())).split().unwrap();
        Self {
            notes,
            audio,
            ring: Box::leak(Box::new(CtrlRing::new())),
            slot: P::new(),
        }
    }

    /// `bytes` in (`Slot::feed`), then what reached the queue and the ring.
    fn run(&mut self, bytes: &[u8]) -> (Vec<InEvent>, Vec<CtrlEvent>) {
        self.feed(bytes);
        self.take()
    }

    /// An empty feed: the poll's retry of what `lost` owes.
    fn settle(&mut self) -> Vec<InEvent> {
        self.run(&[]).0
    }

    /// Bytes fed, nothing popped.
    fn feed(&mut self, bytes: &[u8]) {
        let mut out = DinSinks {
            notes: &mut self.notes,
            ctrl: self.ring,
            sysex: None,
        };
        self.slot.feed(bytes, &mut out);
    }

    fn take(&mut self) -> (Vec<InEvent>, Vec<CtrlEvent>) {
        let heard = core::iter::from_fn(|| self.audio.pop()).collect();
        (heard, self.ring.drain().collect())
    }

    /// The queue filled to its last slot, as a host's burst leaves it.
    fn fill(&mut self) {
        while self.notes.push(off(15, 0)) {}
    }

    fn pop(&mut self, n: usize) {
        for _ in 0..n {
            self.audio.pop().unwrap();
        }
    }
}

impl Usb {
    /// `UsbSlot::lost`, then what reached the queue and the ring.
    fn lost(&mut self) -> (Vec<InEvent>, Vec<CtrlEvent>) {
        let mut out = DinSinks {
            notes: &mut self.notes,
            ctrl: self.ring,
            sysex: None,
        };
        self.slot.lost(&mut out);
        self.take()
    }
}

/// DIN's slot carries no held keys: only USB's reads them.
#[test]
fn only_usb_tracks_what_it_holds() {
    // Past the SysEx room, a pipe is under a key set (256 B) three times
    // (AUTO's latch, ADR 0254, and MPE's owed slide, ADR 0252, took it
    // past twice: 520 B with `controller-maps`).
    assert!(
        size_of::<PipeSlot>() - midi_in::SYSEX_LEN < 768,
        "a key set is 256 B"
    );
    assert!(size_of::<UsbSlot>() >= size_of::<PipeSlot>() + 256);
}

fn all_notes_off() -> Vec<InEvent> {
    (0..16).map(|c| mode(c, ChannelMode::NotesOff)).collect()
}

#[test]
fn idle_after_fe_releases_all_channels() {
    let mut din = Din::new();
    din.run(&[0x90, 60, 100, 0xFE]);
    assert!(din.slot.sensing_armed(), "the ISR arms the timeout");
    let (notes, ctrl) = din.run(&[]);
    assert_eq!(notes, all_notes_off(), "notes off, not sound off");
    assert_eq!(ctrl, [CtrlEvent::SensingLost]);
    assert!(!din.slot.sensing_armed(), "the ISR disarms");
}

#[test]
fn idle_before_fe_does_nothing() {
    let mut din = Din::new();
    assert!(!din.slot.sensing_armed(), "nothing built, nothing armed");
    assert_eq!(din.run(&[]), (vec![], vec![]), "an unbuilt pipe");
    din.run(&[0x90, 60, 100, 0xF8]);
    assert!(!din.slot.sensing_armed());
    assert_eq!(din.run(&[]), (vec![], vec![]), "no FE yet");
}

#[test]
fn fe_rearms_after_timeout() {
    let mut din = Din::new();
    din.run(&[0xFE]);
    assert_eq!(din.run(&[]).0, all_notes_off());
    assert_eq!(din.run(&[]), (vec![], vec![]), "once per silence");
    din.run(&[0x90, 60, 100]);
    assert!(!din.slot.sensing_armed(), "notes alone don't re-arm");
    din.run(&[0xFE]);
    assert!(din.slot.sensing_armed(), "the next FE does");
    assert_eq!(din.run(&[]).0, all_notes_off());
}

/// USB's detach: every channel let go, sensing armed or not, every time;
/// the UI told only when it let go of something (a host's re-enumeration
/// with nothing down says nothing, #818).
#[test]
fn lost_releases_every_channel() {
    let mut usb = Usb::new();
    usb.run(&[0x90, 60, 100, 0x99, 36, 100]);
    assert_eq!(usb.lost(), (all_notes_off(), vec![CtrlEvent::SensingLost]));
    assert_eq!(
        usb.lost(),
        (all_notes_off(), vec![]),
        "each transition, told once"
    );
    usb.run(&[0xB0, 64, 127]);
    assert_eq!(usb.lost().1, [CtrlEvent::SensingLost], "a pedal down");
    usb.run(&[0x91, 62, 100, 0xFE]);
    let (notes, ctrl) = usb.lost();
    assert_eq!(notes, all_notes_off(), "armed too");
    assert_eq!(ctrl, [CtrlEvent::SensingLost]);
}

/// USB leaving Configured as the poll sees it: a reset always lets go; a
/// suspend only while USB holds a key or a pedal.
fn detach(usb: &mut Usb, left: Left) -> bool {
    let go = midi_in::lets_go(left, usb.slot.holds());
    if go {
        usb.lost();
    }
    go
}

#[test]
fn suspend_lets_go_only_while_usb_holds_a_key() {
    let mut usb = Usb::new();
    assert!(!detach(&mut usb, Left::Suspend), "nothing played yet");
    usb.run(&[0x90, 60, 100, 0x80, 60, 0]);
    assert!(!detach(&mut usb, Left::Suspend), "played and let go");
    usb.run(&[0x90, 60, 100, 0x90, 60, 0]);
    assert!(!usb.slot.holds(), "velocity 0 lets go");
    assert_eq!(usb.take(), (vec![], vec![]), "a DIN-held note survives");
    usb.run(&[0x90, 60, 100, 62, 100, 0x80, 60, 0]);
    assert!(usb.slot.holds(), "62 still down");
    assert!(detach(&mut usb, Left::Suspend));
    assert!(!usb.slot.holds(), "`lost` lets go of all");
    assert!(!detach(&mut usb, Left::Suspend), "nothing since");
}

#[test]
fn channel_mode_clears_what_usb_holds() {
    let mut usb = Usb::new();
    for bytes in [
        &[0x90, 60, 100, 0xB0, 123, 0][..], // All Notes Off
        &[0x91, 60, 100, 0xB1, 120, 0],     // All Sound Off
        &[0x92, 60, 100, 0xB2, 124, 0],     // Omni off: notes off too
        &[0x93, 60, 100, 0xFF],             // Reset: sound off on all
    ] {
        usb.run(bytes);
        assert!(!usb.slot.holds(), "{bytes:?}");
    }
    usb.run(&[0x90, 60, 100, 0xB0, 121, 0]);
    assert!(usb.slot.holds(), "Reset All Controllers lets nothing go");
    usb.run(&[0xB1, 123, 0]);
    assert!(usb.slot.holds(), "another channel's");
}

/// Held keys, not a count a channel: a key let go on another channel, or
/// one never down, lets nothing go.
#[test]
fn a_stray_note_off_keeps_a_held_key() {
    let mut usb = Usb::new();
    usb.run(&[0x90, 60, 100, 0x81, 60, 0]);
    assert!(usb.slot.holds(), "released on another channel");
    usb.run(&[0x80, 61, 0]);
    assert!(usb.slot.holds(), "a key never down");
    usb.run(&[0x90, 60, 100, 0x80, 60, 0]);
    assert!(!usb.slot.holds(), "twice down, once up: let go");
}

#[test]
fn reset_always_lets_go() {
    let mut usb = Usb::new();
    assert!(detach(&mut usb, Left::Reset), "with nothing held too");
    usb.run(&[0x90, 60, 100]);
    assert!(detach(&mut usb, Left::Reset));
}

/// A key the queue refused is not held.
#[test]
fn a_refused_note_on_is_not_held() {
    let mut usb = Usb::new();
    usb.fill();
    usb.feed(&[0x90, 60, 100]);
    assert!(!usb.slot.holds());
}

/// `lost` into a full queue: its NotesOff wait for room, each channel's
/// once, in order, ahead of anything fed since.
#[test]
fn lost_waits_for_room() {
    let mut usb = Usb::new();
    usb.run(&[0x90, 60, 100]);
    usb.fill();
    let (notes, ctrl) = usb.lost();
    assert_eq!(notes.len(), 64, "only the fill");
    assert_eq!(ctrl, [CtrlEvent::SensingLost], "the UI hears it now");
    assert!(usb.slot.owes());
    usb.fill();
    usb.pop(5);
    usb.feed(&[0x91, 62, 100, 0x81, 62, 0]);
    let got = usb.take().0;
    assert_eq!(got[59..], all_notes_off()[..5], "room for 5, no note");
    assert_eq!(
        usb.slot.stats().refused,
        3,
        "the lost, the note fed meanwhile"
    );
    assert!(usb.slot.owes());
    assert_eq!(usb.settle(), all_notes_off()[5..], "the rest");
    assert!(!usb.slot.owes());
    assert_eq!(usb.settle(), [], "once");
}

#[test]
fn a_second_lost_while_owed_does_not_double() {
    let mut usb = Usb::new();
    usb.fill();
    usb.lost();
    usb.fill();
    let (notes, ctrl) = usb.lost();
    assert_eq!(notes.len(), 64);
    assert_eq!(ctrl, [], "nothing held, nothing told");
    assert_eq!(usb.settle(), all_notes_off());
    assert_eq!(usb.settle(), []);
}

/// Owed NotesOff go ahead of what is fed after room frees.
#[test]
fn owed_notes_off_go_first() {
    let mut usb = Usb::new();
    usb.fill();
    usb.lost();
    usb.feed(&[0x90, 60, 100]);
    let mut want = all_notes_off();
    want.push(on(0, 60, 100));
    assert_eq!(usb.take().0, want);
    assert!(usb.slot.holds());
}

fn pedal_up(c: u8) -> InEvent {
    InEvent::Sustain(SustainEvent {
        channel: ch(c),
        down: false,
    })
}

/// A pedal down holds: a suspend lets go with the keys up, NotesOff then
/// the pedal lifted.
#[test]
fn suspend_lifts_a_held_pedal() {
    let mut usb = Usb::new();
    usb.run(&[0x90, 60, 100, 0xB0, 64, 127, 0x80, 60, 0]);
    assert!(usb.slot.holds(), "the pedal holds 60");
    assert!(midi_in::lets_go(Left::Suspend, usb.slot.holds()));
    let mut want = all_notes_off();
    want.push(pedal_up(0));
    assert_eq!(usb.lost().0, want);
    assert!(!usb.slot.holds());
    assert!(!detach(&mut usb, Left::Suspend), "lifted once");
}

#[test]
fn reset_lifts_a_held_pedal() {
    let mut usb = Usb::new();
    usb.run(&[0xB2, 64, 127, 0xB5, 64, 64]);
    let mut want = all_notes_off();
    want.extend([pedal_up(2), pedal_up(5)]);
    assert_eq!(usb.lost().0, want, "a reset always calls `lost`");
}

#[test]
fn a_pedal_let_up_or_reset_holds_nothing() {
    let mut usb = Usb::new();
    usb.run(&[0xB0, 64, 127, 0xB0, 64, 63]);
    assert!(!usb.slot.holds(), "below 64 is up");
    usb.run(&[0xB1, 64, 127, 0xB1, 121, 0]);
    assert!(!usb.slot.holds(), "Reset All Controllers lifts it");
    usb.run(&[0xB1, 64, 127, 0xB1, 123, 0]);
    assert!(usb.slot.holds(), "All Notes Off leaves it down");
    usb.lost();
    assert!(!usb.slot.holds());
    usb.run(&[0xB0, 64, 127]);
    usb.fill();
    usb.feed(&[0xB0, 64, 0]);
    assert!(usb.slot.holds(), "a refused pedal-up still holds");
}

/// The pedal-up a full queue refuses is owed after the NotesOff, delivered
/// once, ahead of anything fed since.
#[test]
fn a_pedal_up_waits_for_room() {
    let mut usb = Usb::new();
    usb.run(&[0xB3, 64, 127]);
    usb.fill();
    usb.lost();
    usb.fill();
    usb.pop(16);
    usb.feed(&[0x90, 60, 100]);
    assert_eq!(
        usb.take().0[48..],
        all_notes_off()[..],
        "no room for the pedal"
    );
    assert!(usb.slot.owes());
    assert_eq!(usb.slot.stats().refused, 2, "the lost, the note");
    assert_eq!(usb.settle(), [pedal_up(3)]);
    assert!(!usb.slot.owes());
    assert_eq!(usb.settle(), [], "once");
}

#[test]
fn stats_count_faults_and_refusals() {
    let mut p = DinPipe::new();
    let mut r = Rec::default();
    p.feed(&[0xC2, 7, 0x40, 0xF9], &mut r);
    let s = p.stats();
    assert_eq!(s.faults.undefined, 1);
    assert_eq!(s.faults.orphan, 0);
    r.refuse = true;
    p.feed(&[0x90, 60, 1, 0xB0, 71, 1, 0xB0, 71, 2], &mut r);
    assert_eq!(p.stats().refused, 1, "the note");
    assert_eq!(p.stats().ctrl_refused, 2, "the CCs");
}

/// #515: a program change reaches the control ring with the bank its CC 0
/// and CC 32 selected, running status and all.
#[test]
fn program_change_reaches_the_ring_with_its_bank() {
    let r = pipe(&[0xC2, 7, 0xB0, 0, 1, 32, 2, 0xC0, 9, 5]);
    let prog = |c: u8, bank: Option<U14>, p: u8| {
        CtrlEvent::Ctl(
            ch(c),
            Ctl::Program {
                bank,
                program: u7(p),
            },
        )
    };
    assert_eq!(
        r.ctrl,
        [
            prog(2, None, 7),
            prog(0, Some(U14::from_parts(u7(1), u7(2))), 9),
            prog(0, None, 5),
        ]
    );
    assert!(r.notes.is_empty());
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

    /// Garbage never panics, and what it reads packs back to itself.
    #[test]
    fn unpack_garbage(w in any::<[u32; 2]>()) {
        if let Some(e) = CtrlEvent::unpack(w) {
            prop_assert_eq!(CtrlEvent::unpack(e.pack()), Some(e));
        }
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

/// DIN's sensing lost lets go of its pedals too, after the NotesOff (#849).
#[test]
fn din_sensing_lost_lifts_a_held_pedal() {
    let mut din = Din::new();
    din.run(&[0xB2, 64, 127, 0xB5, 64, 127, 0xB5, 64, 0, 0xFE]);
    let mut want = all_notes_off();
    want.push(pedal_up(2));
    assert_eq!(din.run(&[]).0, want, "only the pedal still down");
    din.run(&[0xFE]);
    assert_eq!(din.run(&[]).0, all_notes_off(), "lifted once");
}

/// A silence into a full queue: the NotesOff it can't place count as
/// refused, so status shows them though no byte follows (#865).
#[test]
fn idle_into_a_full_queue_counts_the_refusal() {
    let mut din = Din::new();
    din.run(&[0xFE]);
    din.fill();
    din.feed(&[]);
    assert_eq!(din.slot.stats().refused, 1);
    let mut usb = Usb::new();
    usb.run(&[0x90, 60, 100]);
    usb.fill();
    usb.lost();
    assert_eq!(usb.slot.stats().refused, 1);
}

/// #514: the wheel (CC 1) and channel pressure reach their matrix sources
/// on the note queue and the map on the ring; poly pressure the queue only.
#[test]
fn wheel_and_pressure_reach_the_queue_and_the_map() {
    let key = MidiNote::new(64).unwrap();
    let r = pipe(&[0xB3, 1, 99]);
    assert_eq!(r.notes, [expr(3, Expr::Wheel(u7(99)))]);
    assert_eq!(
        r.ctrl,
        [CtrlEvent::Ctl(
            ch(3),
            Ctl::Cc {
                cc: FreeCc::new(1).unwrap(),
                value: u7(99)
            }
        )]
    );
    let r = pipe(&[0xD5, 7, 9]);
    assert_eq!(
        r.notes,
        [
            expr(5, Expr::Pressure(u7(7))),
            expr(5, Expr::Pressure(u7(9)))
        ]
    );
    assert_eq!(r.ctrl.len(), 2);
    assert!(matches!(r.ctrl[0], CtrlEvent::Ctl(_, Ctl::Pressure(_))));
    let r = pipe(&[0xAF, 64, 127]);
    assert_eq!(r.notes, [expr(15, Expr::Poly(key, u7(127)))]);
    assert!(r.ctrl.is_empty());
}

/// CC 74 (MPE's slide, ADR 0252) reaches its matrix source on the note
/// queue and the map on the ring, as the wheel does.
#[test]
fn cc74_reaches_the_queue_and_the_map() {
    let r = pipe(&[0xB4, 74, 33]);
    assert_eq!(r.notes, [expr(4, Expr::Timbre(u7(33)))]);
    assert_eq!(
        r.ctrl,
        [CtrlEvent::Ctl(
            ch(4),
            Ctl::Cc {
                cc: FreeCc::new(74).unwrap(),
                value: u7(33)
            }
        )]
    );
}

/// Every expression event packs and unpacks whole, at its extremes.
#[test]
fn expr_events_round_trip_the_queue() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    let evs = [0u8, 127].into_iter().flat_map(|v| {
        [0u8, 15].into_iter().flat_map(move |c| {
            [
                expr(c, Expr::Wheel(u7(v))),
                expr(c, Expr::Pressure(u7(v))),
                expr(c, Expr::Poly(MidiNote::new(0).unwrap(), u7(v))),
                expr(c, Expr::Poly(MidiNote::new(127).unwrap(), u7(v))),
                expr(c, Expr::Timbre(u7(v))),
            ]
        })
    });
    for e in evs {
        assert!(tx.push(e));
        assert_eq!(rx.pop(), Some(e));
    }
}

/// A pressure flood takes half the queue at most: the note-ons that follow
/// it, undrained, still land.
#[test]
fn pressure_flood_leaves_room_for_notes() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    let poly = expr(0, Expr::Poly(MidiNote::new(60).unwrap(), u7(64)));
    let took = (0..NOTE_QUEUE_LEN).filter(|_| tx.push(poly)).count();
    assert_eq!(took, NOTE_QUEUE_LEN / 2);
    for n in 0..16 {
        assert!(tx.push(on(0, 40 + n, 100)), "note-on {n}");
    }
    assert_eq!(core::iter::from_fn(|| rx.pop()).count(), took + 16);
}

/// A refused wheel or pressure is owed, latest wins: once the queue has
/// room again the last value lands, never counted as a refusal.
#[test]
fn a_refused_final_wheel_still_lands() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    let ring = CtrlRing::new();
    struct Door<'a, 'q> {
        notes: &'a mut NoteProducer<'q>,
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
    let mut feed = |p: &mut DinPipe, b: &[u8]| {
        p.feed(
            b,
            &mut Door {
                notes: &mut tx,
                ring: &ring,
            },
        )
    };
    // A wheel sweep up then home, pressure and CC 74 (MPE's slide, on the
    // last channel: the owed bits' top), past half the queue.
    for v in (0..=127).chain((0..127).rev()) {
        feed(&mut p, &[0xB2, 1, v, 0xD2, v, 0xBF, 74, v]);
    }
    let heard: Vec<_> = core::iter::from_fn(|| rx.pop()).collect();
    assert_eq!(heard.len(), NOTE_QUEUE_LEN / 2);
    assert_eq!(p.stats().refused, 0, "owed, not lost");
    feed(&mut p, &[]);
    let last: Vec<_> = core::iter::from_fn(|| rx.pop()).collect();
    assert_eq!(
        last,
        [
            expr(2, Expr::Wheel(u7(0))),
            expr(2, Expr::Pressure(u7(0))),
            expr(15, Expr::Timbre(u7(0)))
        ]
    );
    // Nothing more owed.
    feed(&mut p, &[]);
    assert_eq!(rx.pop(), None);
}

/// An owed value goes ahead of what comes after it: a reset still has the
/// last word.
#[test]
fn an_owed_value_lands_before_a_later_reset() {
    let mut p = DinPipe::new();
    let mut r = Rec {
        refuse: true,
        ..Rec::default()
    };
    p.feed(&[0xB0, 1, 99], &mut r);
    r.refuse = false;
    p.feed(&[0xB0, 121, 0], &mut r);
    assert_eq!(
        r.notes,
        [
            expr(0, Expr::Wheel(u7(99))),
            mode(0, ChannelMode::ResetControllers)
        ]
    );
}

/// USB cable n is Part n's, on the channel the UI last gave it (#840);
/// cable 0 and any past the Parts are no Part's.
#[test]
fn a_part_cable_plays_on_its_parts_channel() {
    use chimera_engine::midi_in::PartChannels;
    use chimera_midi::usb::unpack;
    let cable = |n: u8| unpack([n << 4 | 0x09, 0x90, 60, 100]).unwrap().0;
    let parts = PartChannels::new();
    assert_eq!(parts.of_cable(cable(0)), None);
    assert_eq!(parts.of_cable(cable(1)), Some(ch(0)), "the default");
    parts.set(2, ch(9));
    assert_eq!(parts.of_cable(cable(3)), Some(ch(9)));
    assert_eq!(parts.of_cable(cable(6)), Some(ch(5)));
    assert_eq!(parts.of_cable(cable(7)), None);
}

/// A Part cable's message between cable 0's SysEx packets goes past the
/// shared parser: the SysEx arrives whole, the note plays (#840 review).
#[test]
fn a_part_cable_message_never_cuts_cable_zeros_sysex() {
    let mut usb = Usb::new();
    usb.feed(&[0xF0, 0x7E, 0x7F]);
    let mut out = DinSinks {
        notes: &mut usb.notes,
        ctrl: usb.ring,
        sysex: None,
    };
    usb.slot.feed_msg(&[0x92, 60, 100], &mut out);
    let (notes, ctrl) = usb.run(&[0x06, 0x01, 0xF7]);
    assert_eq!(notes, [on(2, 60, 100)]);
    assert!(
        ctrl.iter().any(|e| matches!(e, CtrlEvent::Identity { .. })),
        "{ctrl:?}"
    );
}

#[cfg(feature = "tuning")]
#[test]
fn mts_retunes_the_mts_table() {
    use chimera_engine::tuning::TUNINGS;
    // A Part that follows MTS plays it: key 61 to 61.5, then a bulk dump
    // sized past the old 32 B buffer sets every key.
    TUNINGS.follow_mts(5, true);
    let r = pipe(&[0xF0, 0x7F, 0x7F, 8, 2, 0, 1, 61, 61, 0x40, 0, 0xF7]);
    assert!(r.ctrl.is_empty());
    assert_eq!(TUNINGS.offset(5, 61), 0.5);
    let mut b = vec![0x7E, 0x7F, 8, 1, 0];
    b.extend_from_slice(&[b' '; 16]);
    for k in 0..128u8 {
        b.extend_from_slice(&[k, 0x10, 0]);
    }
    b.push(b.iter().fold(0, |a, x| a ^ x) & 0x7F);
    let mut m = vec![0xF0];
    m.extend(b);
    m.push(0xF7);
    pipe(&m);
    assert_eq!(TUNINGS.offset(5, 61), 0.125);
    assert_eq!(TUNINGS.offset(5, 0), 0.125);
    TUNINGS.follow_mts(5, false);
    assert_eq!(TUNINGS.offset(5, 61), 0.0);
}

/// #884: DIN's slot says when it owes (a refused wheel, a silence's
/// NotesOff), so its shell retries with no byte to come: an empty feed,
/// as the ISR's retry calls it, lands it and clears the debt.
#[test]
fn din_owes_until_an_empty_feed_lands_it() {
    let mut din = Din::new();
    assert!(!din.slot.owes());
    din.run(&[0xB0, 1, 10]);
    din.fill();
    din.feed(&[0xB0, 1, 0]);
    assert!(din.slot.owes(), "the refused wheel is owed");
    din.take();
    let mut out = DinSinks {
        notes: &mut din.notes,
        ctrl: din.ring,
        sysex: None,
    };
    midi_in::feed(&mut din.slot, &[], &mut out);
    assert_eq!(din.take().0, [expr(0, Expr::Wheel(u7(0)))]);
    assert!(!din.slot.owes());
    // A silence's NotesOff into a full queue: owed too.
    din.run(&[0xFE]);
    din.fill();
    din.feed(&[]);
    assert!(din.slot.owes());
    din.take();
    let mut out = DinSinks {
        notes: &mut din.notes,
        ctrl: din.ring,
        sysex: None,
    };
    midi_in::feed(&mut din.slot, &[], &mut out);
    assert_eq!(din.take().0.len(), 16, "every channel's NotesOff");
    assert!(!din.slot.owes());
}

mod sysex_in {
    use chimera_engine::midi_in::{CtrlEvent, CtrlRing, DinPipe, DinSinks, SX_LEN, SxRing};
    use chimera_engine::note_queue::NoteQueue;

    /// `bytes` through a pipe into a 64-slot ring and `sx`, if any: what
    /// the ring heard, and the bodies `sx` holds.
    fn feed(bytes: &[u8], sx: Option<&SxRing>) -> (Vec<CtrlEvent>, Vec<Vec<u8>>, u16) {
        let q = NoteQueue::new();
        let (mut notes, _audio) = q.split().unwrap();
        let ring: CtrlRing = CtrlRing::new();
        let mut pipe = DinPipe::new();
        pipe.feed(
            bytes,
            &mut DinSinks {
                notes: &mut notes,
                ctrl: &ring,
                sysex: sx,
            },
        );
        let mut bodies = Vec::new();
        let mut b = [0; SX_LEN];
        while let Some(n) = sx.and_then(|r| r.pop(&mut b)) {
            bodies.push(b[..n].to_vec());
        }
        (ring.drain().collect(), bodies, pipe.stats().ctrl_refused)
    }

    const DX7: [u8; 7] = [0xF0, 0x43, 0x10, 0x01, 5, 9, 0xF7];

    #[test]
    fn a_body_lands_in_the_ring_it_is_given() {
        // USB's sinks on a 64-slot ring (a build without the map) are
        // still USB's: the ring is passed, never guessed from a size.
        let usb = SxRing::new();
        let (ev, bodies, _) = feed(&DX7, Some(&usb));
        assert_eq!(ev, [CtrlEvent::SysEx]);
        assert_eq!(bodies, [DX7[1..6].to_vec()]);
    }

    #[test]
    fn a_body_reaches_its_ring_through_the_door() {
        // The shells' path: `midi_in::feed` on a slot, as the ISR runs it.
        let q = NoteQueue::new();
        let (mut notes, _audio) = q.split().unwrap();
        let ring: CtrlRing = CtrlRing::new();
        let sx = SxRing::new();
        let mut slot = chimera_engine::midi_in::PipeSlot::new();
        chimera_engine::midi_in::feed(
            &mut slot,
            &DX7,
            &mut DinSinks {
                notes: &mut notes,
                ctrl: &ring,
                sysex: Some(&sx),
            },
        );
        let mut b = [0; SX_LEN];
        assert_eq!(sx.pop(&mut b), Some(5));
        assert_eq!(&b[..5], &DX7[1..6]);
        assert_eq!(ring.drain().collect::<Vec<_>>(), [CtrlEvent::SysEx]);
    }

    #[test]
    fn no_ring_no_body_and_no_marker() {
        // The map left out: nothing waits that nobody drains.
        let (ev, bodies, refused) = feed(&DX7, None);
        assert!(ev.is_empty() && bodies.is_empty());
        assert_eq!(refused, 0);
    }

    #[test]
    fn a_body_past_sx_len_is_dropped_uncounted() {
        let mut m = vec![0xF0, 0x41];
        m.extend(std::iter::repeat_n(1, SX_LEN));
        m.push(0xF7);
        let sx = SxRing::new();
        let (ev, bodies, refused) = feed(&m, Some(&sx));
        assert!(ev.is_empty() && bodies.is_empty());
        assert_eq!(refused, 0);
    }
}
