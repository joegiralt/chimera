//! MIDI CONFIG › CONTROL in the pipe (ADR 0254): AUTO's messages move to
//! the focused Part's channel, held notes and the pedal staying with the
//! Part that has them; a note-on on SEQUENCER also reaches the UI as a key.
//! One test: `ROUTES` is a process-wide static. AUTO holds keys only with
//! the MIDI UI (`controller-maps`).

#![cfg(feature = "controller-maps")]

use chimera_engine::midi_in::{CtrlEvent, DinPipe, ROUTES, Sinks};
use chimera_engine::note_queue::{InEvent, NoteKind, SustainEvent};
use chimera_engine::{MidiChannel, MidiNote};

#[derive(Default)]
struct Rec {
    notes: Vec<InEvent>,
    ctrl: Vec<CtrlEvent>,
}

impl Sinks for Rec {
    fn note(&mut self, e: InEvent) -> bool {
        self.notes.push(e);
        true
    }
    fn ctrl(&mut self, e: CtrlEvent) -> bool {
        self.ctrl.push(e);
        true
    }
}

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

/// The channels `bytes`' notes and pedal landed on.
fn landed(p: &mut DinPipe, bytes: &[u8]) -> Vec<(u8, bool)> {
    let mut r = Rec::default();
    p.feed(bytes, &mut r);
    r.notes
        .iter()
        .filter_map(|e| match *e {
            InEvent::Note(n) => Some((n.channel.get(), matches!(n.kind, NoteKind::On(_)))),
            InEvent::Sustain(SustainEvent { channel, down }) => Some((channel.get(), down)),
            _ => None,
        })
        .collect()
}

/// The notes and modes `bytes` queued: (channel, on), a mode as `false`.
fn modes(p: &mut DinPipe, bytes: &[u8]) -> Vec<u8> {
    let mut r = Rec::default();
    p.feed(bytes, &mut r);
    r.notes
        .iter()
        .filter_map(|e| match *e {
            InEvent::Mode(m) => Some(m.channel.get()),
            _ => None,
        })
        .collect()
}

#[test]
fn auto_follows_the_focus_as_elektrons_and_sequencer_notes_are_keys() {
    let mut p = DinPipe::new();
    // Off: channel 16 plays as it came.
    ROUTES.set(None, None);
    assert_eq!(landed(&mut p, &[0x9F, 60, 100]), [(15, true)]);
    assert_eq!(landed(&mut p, &[0x8F, 60, 0]), [(15, false)]);

    // AUTO on 16, the focused Part on 3: key and pedal down there.
    ROUTES.set(Some((ch(15), ch(2))), None);
    assert_eq!(landed(&mut p, &[0x9F, 60, 100]), [(2, true)]);
    assert_eq!(landed(&mut p, &[0xBF, 64, 127]), [(2, true)]);
    // The focus moves to Part 5's channel, both still down: a new key and
    // a new pedal-down play Part 5 at once (Elektron's).
    ROUTES.set(Some((ch(15), ch(4))), None);
    assert_eq!(landed(&mut p, &[0x9F, 62, 100]), [(4, true)]);
    assert_eq!(landed(&mut p, &[0xBF, 64, 127]), [(4, true)]);
    // Each note-off reaches the Part its note started on; the pedal-up
    // reaches every Part a pedal-down went to.
    assert_eq!(landed(&mut p, &[0x8F, 60, 0]), [(2, false)]);
    assert_eq!(landed(&mut p, &[0x8F, 62, 0]), [(4, false)]);
    assert_eq!(landed(&mut p, &[0xBF, 64, 0]), [(2, false), (4, false)]);
    // A CC on AUTO reaches the UI on the focused Part's channel.
    let mut r = Rec::default();
    p.feed(&[0xBF, 21, 64], &mut r);
    assert!(matches!(r.ctrl[..], [CtrlEvent::Ctl(c, _)] if c == ch(4)));
    // Other channels pass as they came.
    assert_eq!(
        landed(&mut p, &[0x90, 60, 100, 0x80, 60, 0]),
        [(0, true), (0, false)]
    );

    // AUTO turned OFF mid-note: the note-off still reaches its Part.
    assert_eq!(landed(&mut p, &[0x9F, 64, 100]), [(4, true)]);
    ROUTES.set(None, None);
    assert_eq!(landed(&mut p, &[0x8F, 64, 0]), [(4, false)]);
    assert_eq!(landed(&mut p, &[0x9F, 64, 100]), [(15, true)], "then raw");
    assert_eq!(landed(&mut p, &[0x8F, 64, 0]), [(15, false)]);

    // AUTO moved to channel 15 with a key held from 16: a new note on 15
    // plays the focus, and 16's note-off still reaches its Part.
    ROUTES.set(Some((ch(15), ch(2))), None);
    assert_eq!(landed(&mut p, &[0x9F, 60, 100]), [(2, true)]);
    ROUTES.set(Some((ch(14), ch(4))), None);
    assert_eq!(landed(&mut p, &[0x9E, 62, 100]), [(4, true)]);
    assert_eq!(landed(&mut p, &[0x8F, 60, 0]), [(2, false)]);
    assert_eq!(landed(&mut p, &[0x8E, 62, 0]), [(4, false)]);
    // A raw note-off for the same key elsewhere is that channel's own.
    assert_eq!(landed(&mut p, &[0x9E, 60, 100]), [(4, true)]);
    assert_eq!(landed(&mut p, &[0x80, 60, 0]), [(0, false)]);
    assert_eq!(landed(&mut p, &[0x8E, 60, 0]), [(4, false)]);

    // A mode on AUTO reaches the focus and every Part AUTO holds keys on,
    // and lets go of them: Omni Off clears the latch as All Notes Off does.
    ROUTES.set(Some((ch(15), ch(2))), None);
    assert_eq!(landed(&mut p, &[0x9F, 60, 100]), [(2, true)]);
    ROUTES.set(Some((ch(15), ch(4))), None);
    assert_eq!(modes(&mut p, &[0xBF, 124, 0]), [2, 4]);
    ROUTES.set(None, None);
    assert_eq!(
        landed(&mut p, &[0x8F, 60, 0]),
        [(15, false)],
        "nothing latched"
    );

    // System Reset clears it too.
    ROUTES.set(Some((ch(15), ch(2))), None);
    assert_eq!(landed(&mut p, &[0x9F, 60, 100]), [(2, true)]);
    let _ = landed(&mut p, &[0xFF]);
    ROUTES.set(None, None);
    assert_eq!(landed(&mut p, &[0x8F, 60, 0]), [(15, false)]);

    // SEQUENCER on 15: a note-on is a key for the UI, and no note is queued.
    ROUTES.set(None, Some(ch(14)));
    let mut r = Rec::default();
    p.feed(&[0x9E, 67, 100, 0x8E, 67, 0], &mut r);
    assert_eq!(r.ctrl, [CtrlEvent::Key(MidiNote::new(67).unwrap())]);
    assert!(r.notes.is_empty(), "SEQUENCER plays nothing: {:?}", r.notes);
    let packed = CtrlEvent::Key(MidiNote::new(67).unwrap()).pack();
    assert_eq!(CtrlEvent::unpack(packed), Some(r.ctrl[0]));
}
