//! The parameter dump (#984, ADR 0301): a controller asks with Chimera's
//! own SysEx and reads the Sound's values back as the NRPNs PARAM OUT
//! sends, on the Part's channel, out of the port that asked, paced by
//! DIN's ring, then an end message. Nothing with ECHO off.
#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Blocks, ParamAddr};
use chimera_core::midi_in::{CtrlEvent, CtrlRing, DinSinks, PipeSlot, Port};
use chimera_core::midi_map::{Mappable, MidiMap};
use chimera_core::note_queue::NoteQueue;
use chimera_core::nrpn::{self, ChartLine};
use chimera_core::out_ring::{OUT_RING_LEN, OutPorts, OutRing};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::{PartFrom, PartId, PartSource, ReplaceGuard};
use chimera_core::ui::UiState;
use chimera_core::ui::nav::PartChain;
use chimera_core::ui::param_out;
use chimera_hal::ButtonId;
use chimera_hal::EncoderId;
use chimera_hal::Ms;
use chimera_midi::ctl::DataValue;
use chimera_midi::sysex::{DumpAsk, DumpWho, PartNo};
use chimera_midi::value::Channel;
use screen::{Input as Key, feed};

const P: [PartId; 6] = PartId::ALL;
const PASS_MS: u32 = 4;
/// DIN's 31,250 baud, ten bits a byte.
const DIN_BYTES_PER_S: u32 = 3_125;
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

struct Rig {
    ui: Box<UiState>,
    din: OutRing,
    usb: OutRing,
    now: u32,
    /// DIN's wire: what its UART has sent, a byte every 320 µs.
    wire: Vec<u8>,
    /// Micro-bytes the UART may still send this pass.
    credit: u32,
    /// The most DIN's ring held after a pass.
    peak: usize,
}

impl Rig {
    /// Part 1 on `engine`'s INIT, its map empty (no echo of its own).
    fn new(engine: EngineType) -> Self {
        let mut ui = Box::new(screen::stack_ui());
        let was = PartChain::of(&ui.project().part(P[0]).sound);
        screen::init_part_engine(&mut ui, P[0], engine);
        ui.project_mut().edit_part(P[0]).sound.map = MidiMap::EMPTY;
        ui.replaced_for_test(was);
        let mut r = Rig {
            ui,
            din: OutRing::new(),
            usb: OutRing::new(),
            now: 1_000,
            wire: Vec::new(),
            credit: 0,
            peak: 0,
        };
        for _ in 0..50 {
            r.pass(&[]);
        }
        r.wire.clear();
        r.drain_usb();
        r
    }

    /// One UI pass, then the UART's 4 ms of DIN.
    fn pass(&mut self, ev: &[(Port, CtrlEvent)]) {
        self.now += PASS_MS;
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
        self.ui.send(&OutPorts::new(&self.din, &self.usb));
        self.peak = self.peak.max(self.din.held());
        self.credit += DIN_BYTES_PER_S * PASS_MS;
        let mut b = [0u8; OUT_RING_LEN];
        let n = self.din.pop_into(&mut b[..(self.credit / 1000) as usize]);
        // An idle UART banks no time.
        self.credit = if n == 0 { 0 } else { self.credit - n as u32 * 1000 };
        self.wire.extend(&b[..n]);
    }

    fn drain_usb(&self) -> Vec<u8> {
        let mut b = [0u8; OUT_RING_LEN];
        let n = self.usb.pop_into(&mut b);
        b[..n].to_vec()
    }

    /// Passes till DIN's wire ends in an end message, or `ms` pass.
    fn until_end(&mut self, ms: u32) -> u32 {
        let from = self.now;
        while self.now - from < ms {
            self.pass(&[]);
            if self.wire.ends_with(&[0xF7]) {
                break;
            }
        }
        self.now - from
    }
}

fn ask(who: DumpWho) -> CtrlEvent {
    CtrlEvent::Dump(DumpAsk::of(who))
}

fn part(n: u8) -> DumpWho {
    DumpWho::Part(PartNo::new(n).unwrap())
}

/// An NRPN heard: channel, address, data MSB, data LSB if sent.
type Heard = (u8, u16, u8, Option<u8>);

/// A dump off the wire: its NRPNs, running status read, and the Part its
/// end names; panics on anything else, or on bytes after the end.
fn dump(bytes: &[u8]) -> (Vec<Heard>, Option<u8>) {
    let mut ccs = Vec::new();
    let (mut status, mut i, mut end) = (0u8, 0, None);
    while i < bytes.len() {
        assert_eq!(end, None, "bytes after the end: {bytes:02X?}");
        if bytes[i] == 0xF0 {
            assert_eq!(bytes[i..i + 5], [0xF0, 0x7D, 0x43, 0x48, 0x02]);
            assert_eq!(bytes[i + 6], 0xF7);
            end = Some(bytes[i + 5]);
            i += 7;
            continue;
        }
        if bytes[i] >= 0x80 {
            status = bytes[i];
            i += 1;
            continue;
        }
        assert_eq!(status & 0xF0, 0xB0, "a CC: {bytes:02X?}");
        ccs.push((status & 15, bytes[i], bytes[i + 1]));
        i += 2;
    }
    let mut out: Vec<Heard> = Vec::new();
    let (mut m, mut l) = (0u16, 0u16);
    for (ch, cc, v) in ccs {
        match cc {
            99 => m = u16::from(v),
            98 => l = u16::from(v),
            6 => out.push((ch, m << 7 | l, v, None)),
            38 => out.last_mut().expect("38 after 6").3 = Some(v),
            _ => panic!("CC {cc}: {bytes:02X?}"),
        }
    }
    (out, end)
}

/// What Part `p` of `ui` answers: every writable cell of its Sound its
/// engine reads, in the chart's order, as PARAM OUT sends it.
fn want(ui: &UiState, p: PartId) -> Vec<Heard> {
    let part = ui.project().part(p);
    let (ch, engine) = (part.mix.channel.get(), part.sound.engine());
    nrpn::chart()
        .filter_map(|l| match l {
            ChartLine::Live { at, block, spec } => Some((at, ParamAddr::new(block, spec.id))),
            ChartLine::Retired(_) => None,
        })
        .filter(|(_, a)| a.block.in_sound() && Mappable::new(*a).is_some_and(|m| m.live(engine)))
        .map(|(at, a)| {
            let n = at.get().get();
            match param_out::data(ui.project(), p, a).unwrap() {
                DataValue::Coarse(m) => (ch, n, m.get(), None),
                DataValue::Fine(f) => (ch, n, f.msb().get(), Some(f.lsb().get())),
            }
        })
        .collect()
}

/// The request as a controller sends it, through DIN's pipe.
fn through_the_pipe(bytes: &[u8]) -> Vec<CtrlEvent> {
    let q = Box::leak(Box::new(NoteQueue::new()));
    let (mut prod, _cons) = q.split().unwrap();
    let ring: CtrlRing = CtrlRing::new();
    let mut slot = PipeSlot::new();
    chimera_core::midi_in::feed(
        &mut slot,
        bytes,
        &mut DinSinks {
            notes: &mut prod,
            ctrl: &ring,
            sysex: None,
        },
    );
    ring.drain().collect()
}

#[test]
fn the_request_is_heard_through_the_pipe() {
    assert_eq!(
        through_the_pipe(&[0xF0, 0x7D, 0x43, 0x48, 0x01, 0x02, 0xF7]),
        [ask(part(2))]
    );
    assert_eq!(
        through_the_pipe(&[0xF0, 0x7D, 0x43, 0x48, 0x01, 0x1F, 0xF7]),
        [ask(DumpWho::Channel(Channel::new(15).unwrap()))]
    );
    assert_eq!(
        through_the_pipe(&[0xF0, 0x7D, 0x43, 0x48, 0x01, 0x7F, 0xF7]),
        [ask(DumpWho::Editing)]
    );
    // The ring carries it whole.
    for who in [part(0), part(15), DumpWho::Editing] {
        assert_eq!(CtrlEvent::unpack(ask(who).pack()), Some(ask(who)));
    }
}

/// Every engine: each cell's NRPN once, its value the Sound's, on the
/// Part's channel, then the end naming the Part; DIN's alone.
#[test]
fn a_request_answers_every_cell_of_the_sound() {
    for engine in EngineType::ALL {
        let mut r = Rig::new(engine);
        // Off its default: a value to read.
        r.ui.params_mut()
            .block_mut(BlockRef::Filter)
            .unwrap()
            .set(FilterParams::CUTOFF, 1234.0);
        r.pass(&[(Port::Din, ask(part(0)))]);
        let took = r.until_end(3_000);
        let (heard, end) = dump(&r.wire);
        let want = want(&r.ui, P[0]);
        assert!(want.len() > 40, "{engine:?}: {} cells", want.len());
        assert_eq!(heard, want, "{engine:?}");
        assert_eq!(end, Some(0), "{engine:?}");
        assert!(r.drain_usb().is_empty(), "{engine:?}: asked on DIN");
        // DIN's pace: the bytes at 3,125 a second, and a pass's slack.
        let floor = r.wire.len() as u32 * 1000 / DIN_BYTES_PER_S;
        assert!(
            took + 2 * PASS_MS >= floor && took <= floor + 40,
            "{engine:?}: {took} ms for {floor}"
        );
        assert!(took < 1_000, "{engine:?}: {took} ms");
        // Nothing more.
        let n = r.wire.len();
        for _ in 0..100 {
            r.pass(&[]);
        }
        assert_eq!(r.wire.len(), n, "{engine:?}");
    }
}

/// DIN is never flooded: the dump keeps half the ring free for the echo
/// and PARAM OUT, whose turn goes out within the half ring's 41 ms.
#[test]
fn din_is_paced_and_a_turn_still_goes_at_once() {
    let mut r = Rig::new(EngineType::Algo);
    let m = r.ui.midi().with_param_out(true);
    r.ui.set_midi(m);
    for _ in 0..12 {
        if r.ui.page_def().id == chimera_core::ui::block_registry::FILTER.id {
            break;
        }
        feed(&mut r.ui, Key::press(ButtonId::Plus));
    }
    r.pass(&[(Port::Din, ask(part(0)))]);
    for _ in 0..25 {
        r.pass(&[]);
    }
    assert!(!r.wire.ends_with(&[0xF7]), "mid dump");
    assert!(r.peak <= OUT_RING_LEN / 2, "the dump filled {} B", r.peak);
    // The hand turns CUTOFF: its NRPN, the new value, is on the wire soon.
    let at = r.wire.len();
    feed(&mut r.ui, Key::turn(EncoderId::F, -20).at(r.now));
    let turned = match param_out::data(r.ui.project(), P[0], CUTOFF).unwrap() {
        DataValue::Fine(f) => [6, f.msb().get(), 38, f.lsb().get()],
        DataValue::Coarse(_) => panic!("CUTOFF is 14-bit"),
    };
    let mut ms = 0;
    while !r.wire[at..].windows(4).any(|w| w == turned) {
        r.pass(&[]);
        ms += PASS_MS;
        assert!(ms <= 60, "the turn waited behind the dump");
    }
    r.until_end(3_000);
    assert!(r.peak <= OUT_RING_LEN / 2 + 9, "{} B", r.peak);
    // Whole messages only: the wire still reads as NRPNs and one end.
    let (heard, end) = dump(&r.wire);
    assert_eq!(end, Some(0));
    assert!(heard.len() > want(&r.ui, P[0]).len(), "the dump and the turn");
}

/// Asked on USB, answered on USB alone, a ring at a time.
#[test]
fn asked_on_usb_it_answers_on_usb() {
    let mut r = Rig::new(EngineType::Va);
    r.pass(&[(Port::Usb, ask(part(0)))]);
    let mut got = r.drain_usb();
    for _ in 0..50 {
        r.pass(&[]);
        got.extend(r.drain_usb());
    }
    assert!(r.wire.is_empty(), "DIN stays quiet");
    let (heard, end) = dump(&got);
    assert_eq!(heard, want(&r.ui, P[0]));
    assert_eq!(end, Some(0));
}

/// A Part by number, the Part on a channel (the lowest on it), the Part
/// being edited; a number or a channel with no Part answers nothing.
#[test]
fn the_request_names_its_part() {
    let mut r = Rig::new(EngineType::Algo);
    screen::init_part_engine(&mut r.ui, P[2], EngineType::Va);
    screen::init_part_engine(&mut r.ui, P[4], EngineType::Modal);
    let ch = Channel::new(9).unwrap();
    r.ui.project_mut().edit_part(P[2]).mix.channel = ch;
    r.ui.project_mut().edit_part(P[4]).mix.channel = ch;
    let free = Channel::new(13).unwrap();
    assert!(P.iter().all(|&p| r.ui.project().part(p).mix.channel != free));
    for _ in 0..50 {
        r.pass(&[]);
    }
    let answer = |r: &mut Rig, who| {
        r.wire.clear();
        r.pass(&[(Port::Din, ask(who))]);
        r.until_end(3_000);
        for _ in 0..20 {
            r.pass(&[]);
        }
        dump(&r.wire)
    };
    let (heard, end) = answer(&mut r, part(2));
    assert_eq!((heard, end), (want(&r.ui, P[2]), Some(2)));
    let (heard, end) = answer(&mut r, part(4));
    assert_eq!((heard, end), (want(&r.ui, P[4]), Some(4)));
    // Two Parts on channel 10: the lower answers, as it does the echo.
    let (heard, end) = answer(&mut r, DumpWho::Channel(ch));
    assert_eq!((heard, end), (want(&r.ui, P[2]), Some(2)));
    feed(&mut r.ui, Key::press(ButtonId::B5).at(r.now));
    assert_eq!(r.ui.active_part, P[4]);
    let (heard, end) = answer(&mut r, DumpWho::Editing);
    assert_eq!((heard, end), (want(&r.ui, P[4]), Some(4)));
    for who in [part(6), part(15), DumpWho::Channel(free)] {
        assert_eq!(answer(&mut r, who), (vec![], None), "{who:?}");
    }
}

/// ECHO off: no answer, then or later, as the identity request.
#[test]
fn echo_off_refuses() {
    let mut r = Rig::new(EngineType::Algo);
    let m = r.ui.midi().with_echo(false);
    r.ui.set_midi(m);
    r.pass(&[(Port::Din, ask(part(0))), (Port::Usb, ask(part(0)))]);
    for _ in 0..100 {
        r.pass(&[]);
    }
    let m = r.ui.midi().with_echo(true);
    r.ui.set_midi(m);
    for _ in 0..100 {
        r.pass(&[]);
    }
    assert!(r.wire.is_empty() && r.drain_usb().is_empty());
    // ECHO turned off mid dump stops it.
    r.pass(&[(Port::Din, ask(part(0)))]);
    for _ in 0..10 {
        r.pass(&[]);
    }
    let m = r.ui.midi().with_echo(false);
    r.ui.set_midi(m);
    for _ in 0..300 {
        r.pass(&[]);
    }
    assert!(!r.wire.is_empty() && !r.wire.ends_with(&[0xF7]));
}

/// A Sound loaded under a dump: it starts again, so what the controller
/// last read is the Sound that plays. A second ask on the port replaces
/// the first.
#[test]
fn a_load_or_a_new_ask_starts_again() {
    let mut r = Rig::new(EngineType::Algo);
    r.pass(&[(Port::Din, ask(part(0)))]);
    for _ in 0..30 {
        r.pass(&[]);
    }
    let was = PartChain::of(&r.ui.project().part(P[0]).sound);
    let src = PartSource {
        part: P[0],
        from: PartFrom::Init(EngineType::Va),
    };
    let edited = ReplaceGuard::check(r.ui.project(), r.ui.template(), src).expect_err("edited");
    let c = edited.into_pending().anyway(r.ui.project());
    r.ui.project_mut().replace_part(c).expect("INIT loads");
    r.ui.project_mut().edit_part(P[0]).sound.map = MidiMap::EMPTY;
    r.ui.replaced_for_test(was);
    r.until_end(3_000);
    let (heard, end) = dump(&r.wire);
    let want_va = want(&r.ui, P[0]);
    assert_eq!(end, Some(0));
    assert_eq!(heard[heard.len() - want_va.len()..], want_va[..], "VA, whole, last");

    screen::init_part_engine(&mut r.ui, P[1], EngineType::Modal);
    for _ in 0..50 {
        r.pass(&[]);
    }
    r.wire.clear();
    r.pass(&[(Port::Din, ask(part(0)))]);
    for _ in 0..10 {
        r.pass(&[]);
    }
    r.pass(&[(Port::Din, ask(part(1)))]);
    r.until_end(3_000);
    let (heard, end) = dump(&r.wire);
    let want_2 = want(&r.ui, P[1]);
    assert_eq!(end, Some(1), "one end: the second ask's");
    assert_eq!(heard[heard.len() - want_2.len()..], want_2[..]);
}
