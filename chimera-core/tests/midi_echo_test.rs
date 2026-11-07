//! Value echo out of DIN (spec §§ 8.4, 9): each feedback binding's param,
//! quantised to its control, sent on its Part's channel when it changes:
//! never back to the hand, latest value only, a burst on a load, the
//! identity reply first, nothing with MIDI CONFIG › ECHO off.
#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::Blocks;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Block;
use chimera_core::midi_in::{CtrlEvent, Port};
use chimera_core::midi_map::{
    Binding, CcKind, Feedback, Input, LearnCc, Mappable, MidiMap, NrpnKind, Span,
};
use chimera_core::nrpn::{self, ChartLine};
use chimera_core::out_ring::{OUT_RING_LEN, OutPorts, OutRing};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::{PartFrom, PartId, PartSource};
use chimera_core::storage::{AbFile, Card, Side, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::learn::Learn;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_core::ui::nav::PartChain;
use chimera_hal::ButtonId::{Mix, Plus, Seq};
use chimera_hal::EncoderId;
use chimera_hal::Ms;
use chimera_hal::testkit::MemStore;
use chimera_midi::ctl::{Ctl, FreeCc};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;
use chimera_midi::value::U7;
use screen::{Input as Key, feed};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
const PASS_MS: u32 = 4;
const JUMP: CcKind = CcKind::Absolute(Takeover::Jump);

/// A UI on INIT Algo (Parts 1 and 2), its maps empty, on FLT, and the out
/// ring a pass's DIN OUT fills.
struct Rig {
    ui: Box<UiState>,
    ring: OutRing,
    now: u32,
}

impl Rig {
    fn new() -> Self {
        Self::with_maps(false)
    }

    /// INIT's maps kept, or emptied.
    fn with_maps(keep: bool) -> Self {
        let mut ui = Box::new(screen::stack_ui());
        let was = PartChain::of(&ui.project().part(P1).sound);
        for p in [P1, P2] {
            screen::init_part(&mut ui, p);
            if !keep {
                ui.project_mut().edit_part(p).sound.map = MidiMap::EMPTY;
            }
        }
        ui.replaced_for_test(was);
        let mut r = Rig {
            ui,
            ring: OutRing::new(),
            now: 1_000,
        };
        r.go_to_filter();
        r.settle();
        r
    }

    fn go_to_filter(&mut self) {
        for _ in 0..12 {
            if self.ui.page_def().id == FILTER.id {
                return;
            }
            feed(&mut self.ui, Key::press(Plus));
        }
        panic!("FLT not reached");
    }

    fn ch(&self, p: PartId) -> MidiChannel {
        self.ui.project().part(p).mix.channel
    }

    /// `input` on Part `p` moves `a`, echo `fb`.
    fn bind(&mut self, p: PartId, input: Input, a: ParamAddr, fb: Feedback) {
        let map = &mut self.ui.project_mut().edit_part(p).sound.map;
        map.learn(input, Mappable::new(a).unwrap()).unwrap();
        let i = map.entries().iter().position(|e| e.target.addr() == a);
        let e = Binding {
            input,
            target: Mappable::new(a).unwrap(),
            span: Span::FULL,
            feedback: fb,
        };
        map.set(i.unwrap(), e);
    }

    fn set(&mut self, p: PartId, a: ParamAddr, place: f32) {
        let s = self.ui.project_mut().edit_part(p).sound;
        let spec = a.spec().unwrap();
        s.params
            .block_mut(a.block)
            .unwrap()
            .set(a.param, spec.denormalize(place));
    }

    /// One pass: a key, what was heard, then DIN OUT's pass.
    fn pass_with(&mut self, ev: &[CtrlEvent], key: Option<Key>) {
        self.now += PASS_MS;
        if let Some(k) = key {
            feed(&mut self.ui, k.at(self.now));
        }
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
        self.ui.send_out(&self.ring);
    }

    fn pass(&mut self, ev: &[CtrlEvent]) {
        self.pass_with(ev, None);
    }

    /// Passes enough for any ramp and quiet window to end, then the ring
    /// emptied: what they sent.
    fn settle(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..100 {
            self.pass(&[]);
            out.extend(self.drain());
        }
        out
    }

    /// `a`'s value on Part `p` as a CC sends it.
    fn want(&self, p: PartId, a: ParamAddr) -> u8 {
        let part = self.ui.project().read_part(p);
        let v = part.block(a.block).unwrap().get(a.param);
        (a.spec().unwrap().normalize(v) * 127.0).round() as u8
    }

    fn drain(&self) -> Vec<u8> {
        let mut b = [0u8; OUT_RING_LEN];
        let n = self.ring.pop_into(&mut b);
        b[..n].to_vec()
    }

    fn cc(&self, p: PartId, n: u8, v: u8) -> CtrlEvent {
        CtrlEvent::Ctl(
            self.ch(p),
            Ctl::Cc {
                cc: FreeCc::new(n).unwrap(),
                value: U7::new(v).unwrap(),
            },
        )
    }
}

fn cc_in(n: u8, k: CcKind) -> Input {
    Input::Cc(LearnCc::new(n).unwrap(), k)
}

/// CC `n`'s message on channel `ch`, with its status.
fn cc_msg(ch: MidiChannel, n: u8, v: u8) -> Vec<u8> {
    vec![0xB0 | ch.get(), n, v]
}

/// The CC 20 values in `bytes`, on channel `ch`, running status read.
fn values(bytes: &[u8], ch: MidiChannel, n: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut status = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] >= 0x80 {
            status = bytes[i];
            i += 1;
            continue;
        }
        if status == 0xB0 | ch.get() && bytes[i] == n {
            out.push(bytes[i + 1]);
        }
        i += 2;
    }
    out
}

/// The NRPN addresses an Algo Part reads, each a writable param: 16.
fn nrpn_targets() -> Vec<(chimera_midi::value::U14, ParamAddr)> {
    nrpn::chart()
        .filter_map(|l| match l {
            ChartLine::Live { at, block, spec } => {
                let a = ParamAddr::new(block, spec.id);
                let m = nrpn::writable(a)?;
                m.live(EngineType::Algo).then_some((at.get(), a))
            }
            ChartLine::Retired(_) => None,
        })
        .filter(|(_, a)| a.block != BlockRef::Part)
        .take(16)
        .collect()
}

#[test]
fn panel_turn_echoes_bound_cc() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.set(P1, CUTOFF, 0.5);
    let ch = r.ch(P1);
    let first = r.settle();
    let at = r.want(P1, CUTOFF);
    assert!((63..=64).contains(&at));
    assert_eq!(values(&first, ch, 20), [at], "the burst: where it is");
    // CUTOFF is FLT's F: a panel turn up echoes, on Part 1's channel.
    for _ in 0..10 {
        r.pass_with(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    let got = r.settle();
    let v = values(&got, ch, 20);
    assert!(!v.is_empty(), "{got:02X?}");
    assert!(v.windows(2).all(|w| w[0] < w[1]), "climbs: {v:?}");
    assert_eq!(
        *v.last().unwrap(),
        r.want(P1, CUTOFF),
        "the last echo is the value"
    );
    assert_eq!(&got[..3], &cc_msg(ch, 20, v[0])[..], "status first");
}

#[test]
fn echo_never_returns_to_the_hand() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.settle();
    // A BCR-style stream: a value a pass, never echoed while it moves.
    let mut sent = Vec::new();
    for v in (10..=90).step_by(2) {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
        sent.extend(r.drain());
    }
    assert!(sent.is_empty(), "echoed to the hand: {sent:02X?}");
    // Its ramp lands on what it sent: nothing after the quiet either.
    assert!(r.settle().is_empty());
    // Inside the quiet the panel's turn waits; after it, it echoes.
    let e = r.cc(P1, 20, 92);
    r.pass(&[e]);
    for _ in 0..10 {
        r.pass_with(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    assert!(r.drain().is_empty(), "held 250 ms");
    let v = values(&r.settle(), r.ch(P1), 20);
    assert_eq!(v, [r.want(P1, CUTOFF)]);
    assert!(v[0] > 92);
}

/// A 14-bit controller's sweep lands where it sent: nothing comes back,
/// the float trip through the param's law included.
#[test]
fn fourteen_bit_sweep_never_returns() {
    let mut r = Rig::new();
    let n = nrpn_targets()[0].0;
    let m = chimera_core::midi_map::Cc14Msb::new(2).unwrap();
    r.bind(P1, Input::Cc14(m, Takeover::Jump), CUTOFF, Feedback::Value);
    r.bind(
        P1,
        Input::Nrpn(n, NrpnKind::Absolute(Takeover::Jump)),
        RESO,
        Feedback::Value,
    );
    r.settle();
    let ch = r.ch(P1);
    for v in (100u16..16_000).step_by(997) {
        let v = chimera_midi::value::U14::new(v).unwrap();
        let msb = CtrlEvent::Ctl(
            ch,
            Ctl::Cc {
                cc: FreeCc::new(2).unwrap(),
                value: v.msb(),
            },
        );
        let lsb = CtrlEvent::Ctl(
            ch,
            Ctl::Cc {
                cc: FreeCc::new(34).unwrap(),
                value: v.lsb(),
            },
        );
        let fine = CtrlEvent::Ctl(
            ch,
            Ctl::Data {
                sel: chimera_midi::ctl::Sel::Nrpn(n),
                value: chimera_midi::ctl::DataValue::Fine(v),
            },
        );
        r.pass(&[msb, lsb, fine]);
        assert!(r.drain().is_empty());
        let rest = r.settle();
        assert!(rest.is_empty(), "at {}: {rest:02X?}", v.get());
    }
}

#[test]
fn echo_coalesces_latest() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.settle();
    // The ring full: five moves wait as one dirty binding.
    let filler = [0xB5u8, 1, 2];
    while r.ring.push_msg(&filler) {}
    for place in [0.1, 0.2, 0.3, 0.4, 0.8] {
        r.set(P1, CUTOFF, place);
        r.pass(&[]);
    }
    let full = r.drain();
    assert!(full.chunks(3).all(|c| c == filler), "nothing squeezed in");
    r.pass(&[]);
    assert_eq!(r.drain(), cc_msg(r.ch(P1), 20, 102), "the latest alone");
    assert!(r.settle().is_empty());
}

#[test]
fn burst_on_load_marks_all_feedback_bindings() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.bind(
        P1,
        cc_in(21, CcKind::Relative(ByteEncoding::TwosComplement)),
        RESO,
        Feedback::Value,
    );
    let env = ParamAddr::new(
        BlockRef::Env(chimera_core::dsp::modulator::EnvSlot::Env1),
        chimera_core::params::EnvParams::ATTACK,
    );
    r.bind(P1, cc_in(22, JUMP), env, Feedback::Off);
    r.settle();
    // A load whose Sound maps the same: unchanged values, all sent again.
    let src = PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Algo),
    };
    let t = r.ui.template();
    let map = r.ui.project().part(P1).sound.map;
    common::project::load_anyway(r.ui.project_mut(), t, src).unwrap();
    r.ui.project_mut().edit_part(P1).sound.map = map;
    let got = r.settle();
    let ch = r.ch(P1);
    assert_eq!(values(&got, ch, 20).len(), 1, "{got:02X?}");
    assert_eq!(values(&got, ch, 21).len(), 1, "{got:02X?}");
    assert!(values(&got, ch, 22).is_empty(), "echo off stays silent");
    // A page change is no burst.
    feed(&mut r.ui, Key::press(Plus));
    assert!(r.settle().is_empty());
}

#[test]
fn burst_fits_ring_or_defers() {
    let mut r = Rig::new();
    let targets = nrpn_targets();
    assert_eq!(targets.len(), 16);
    for p in [P1, P2] {
        for &(n, a) in &targets {
            let input = Input::Nrpn(n, NrpnKind::Absolute(Takeover::Jump));
            r.bind(p, input, a, Feedback::Value);
        }
    }
    // Both Parts burst: Part 1's 16 NRPN whole, one status and 8 bytes
    // each (129 B); Part 2's as far as the ring has room, whole messages.
    r.pass(&[]);
    let first = r.drain();
    assert!(first.len() <= OUT_RING_LEN);
    let (c1, c2) = (0xB0 | r.ch(P1).get(), 0xB0 | r.ch(P2).get());
    assert_eq!(first[0], c1);
    let part2 = first.iter().position(|&b| b == c2).unwrap();
    assert_eq!(part2, 1 + 16 * 8, "Part 1's burst: 129 B");
    let rest = &first[part2 + 1..];
    assert_eq!(rest.len() % 8, 0, "whole messages only");
    let sent2 = rest.len() / 8;
    assert!(sent2 < 16, "the ring was too small for both");
    // The deferred rest goes next pass, each once.
    r.pass(&[]);
    let next = r.drain();
    assert_eq!(next[0], c2);
    assert_eq!((next.len() - 1) / 8, 16 - sent2, "{next:02X?}");
    assert!(r.settle().is_empty(), "each once");
}

#[test]
fn default_map_silent() {
    let mut r = Rig::with_maps(true);
    assert!(!r.ui.project().part(P1).sound.map.is_empty(), "INIT's map");
    assert!(r.settle().is_empty(), "no burst: echo off by default");
    for _ in 0..10 {
        r.pass_with(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    let e = r.cc(P1, 74, 20);
    r.pass(&[e]);
    assert!(r.settle().is_empty());
}

#[test]
fn learned_relative_binding_echoes() {
    let mut r = Rig::new();
    feed(&mut r.ui, Key::turn(EncoderId::F, 1));
    // MIX + SEQ arms learn on CUTOFF.
    let mix_seq = |ui: &mut UiState, at: u32| {
        feed(ui, Key::chord(Mix, Seq).at(at));
        feed(ui, Key::release(Seq).and_held(Mix).at(at + 100));
        feed(ui, Key::release(Mix).at(at + 133));
    };
    mix_seq(&mut r.ui, 10_000);
    assert!(matches!(r.ui.learn(), Learn::Armed { .. }));
    r.now = 20_000;
    for v in [1, 1, 0x7F, 0x7F] {
        let e = r.cc(P1, 21, v);
        r.pass(&[e]);
    }
    mix_seq(&mut r.ui, 30_000);
    let e = r.ui.project().part(P1).sound.map.entries()[0];
    assert_eq!(e.feedback, Feedback::Value, "learn turned echo on");
    r.now = 31_000;
    r.settle();
    let was = r.want(P1, CUTOFF);
    // Detents down: held while turning, then the value, absolute.
    for _ in 0..5 {
        let e = r.cc(P1, 21, 0x76);
        r.pass(&[e]);
        assert!(r.drain().is_empty());
    }
    let v = values(&r.settle(), r.ch(P1), 21);
    assert_eq!(v, [r.want(P1, CUTOFF)]);
    assert!(v[0] < was, "{was} -> {v:?}");
}

#[test]
fn shared_channel_lowest_part_echoes() {
    let mut r = Rig::new();
    let ch = r.ch(P1);
    r.ui.project_mut().edit_part(P2).mix.channel = ch;
    for p in [P1, P2] {
        r.bind(p, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    }
    r.set(P1, CUTOFF, 0.0);
    r.set(P2, CUTOFF, 1.0);
    let got = r.settle();
    assert_eq!(values(&got, ch, 20), [0], "Part 1's alone: {got:02X?}");
    r.set(P2, CUTOFF, 0.5);
    assert!(r.settle().is_empty(), "Part 2 never echoes CC 20");
    r.set(P1, CUTOFF, 0.5);
    assert_eq!(values(&r.settle(), ch, 20), [r.want(P1, CUTOFF)]);
    // Another control on Part 2 is its own.
    r.bind(P2, cc_in(21, JUMP), RESO, Feedback::Value);
    assert_eq!(values(&r.settle(), ch, 21).len(), 1);
}

#[test]
fn identity_first() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.settle();
    r.set(P1, CUTOFF, 0.25);
    let ask = CtrlEvent::Identity {
        dev: U7::new(0x7F).unwrap(),
    };
    r.pass(&[ask]);
    let got = r.drain();
    assert_eq!(got[0], 0xF0);
    let end = got.iter().position(|&b| b == 0xF7).unwrap();
    assert_eq!(end, 14, "the 15-byte reply first");
    assert_eq!(&got[15..], &cc_msg(r.ch(P1), 20, 32)[..], "then the echo");
}

#[test]
fn echo_off_sends_nothing() {
    let mut r = Rig::new();
    r.ui.set_midi(MidiSettings::DEFAULT.with_echo(false));
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    let ask = CtrlEvent::Identity {
        dev: U7::new(0x7F).unwrap(),
    };
    r.pass(&[ask]);
    for _ in 0..10 {
        r.pass_with(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    assert!(r.settle().is_empty(), "no echo, no reply");
    // On again: the values echo; the inquiry asked while off is not answered.
    r.ui.set_midi(MidiSettings::DEFAULT);
    let got = r.settle();
    assert_eq!(values(&got, r.ch(P1), 20).len(), 1, "{got:02X?}");
    assert!(!got.contains(&0xF0));
}

/// A SYSTEM file's settings, as boot reads them off the card.
fn boot_bytes(bytes: &[u8]) -> SystemSettings {
    use chimera_hal::store::{Dir, Store};
    let mut s = MemStore::new(1);
    let v = s.mount().unwrap();
    s.make_dir(v, Dir::Chimera).unwrap();
    s.write(v, AbFile::SYSTEM.side(Side::A), &mut |k| k.put(bytes))
        .unwrap();
    let mut card = Card::new();
    let (_, got, note) = SystemSync::boot(&mut card, &mut s);
    assert_eq!(note, None);
    got
}

#[test]
fn system_without_echo_reads_on() {
    assert!(MidiSettings::DEFAULT.echo(), "ON by default");
    for f in [
        include_bytes!("fixtures/v1/system.sys").as_slice(),
        include_bytes!("fixtures/v1/system_midi.sys").as_slice(),
    ] {
        assert!(
            boot_bytes(f).midi.echo(),
            "a SYSTEM from before ECHO reads ON"
        );
    }
    // OFF is stored and read back.
    let off = SystemSettings {
        midi: MidiSettings::DEFAULT.with_echo(false),
        ..SystemSettings::DEFAULT
    };
    assert!(
        !boot_bytes(&common::codec_util::system_file(&off))
            .midi
            .echo()
    );
    // The cell: ECHO, OFF / ON, ident ECHO, param 3, default ON.
    let s = MidiSettings::DEFAULT;
    let spec = s
        .specs()
        .iter()
        .find(|p| p.id == MidiSettings::ECHO)
        .unwrap();
    assert_eq!(
        (spec.label, spec.ident, spec.default),
        ("ECHO", "ECHO", 1.0)
    );
    assert_eq!(MidiSettings::ECHO.0, 3);
    assert_eq!(s.get(MidiSettings::ECHO), 1.0);
}

/// SETTINGS › MIDI CONFIG › ECHO: one cell, ON out of the box; A turns it
/// OFF, and DIN OUT goes quiet.
#[test]
fn echo_row_switches_din_out() {
    use chimera_core::ui::block_registry::SYS_ECHO;
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.settle();
    screen::to_leaf(&mut r.ui, &["MIDI CONFIG", "ECHO"]);
    assert_eq!(r.ui.page_def().id, SYS_ECHO.id);
    assert!(r.ui.midi().echo());
    feed(&mut r.ui, Key::turn(EncoderId::A, -1));
    assert!(!r.ui.midi().echo());
    r.set(P1, CUTOFF, 0.1);
    assert!(r.settle().is_empty());
    feed(&mut r.ui, Key::turn(EncoderId::A, 1));
    assert!(r.ui.midi().echo());
    assert_eq!(values(&r.settle(), r.ch(P1), 20), [r.want(P1, CUTOFF)]);
}

/// Everything waiting in `ring`.
fn pop(ring: &OutRing) -> Vec<u8> {
    let mut b = [0u8; OUT_RING_LEN];
    let n = ring.pop_into(&mut b);
    b[..n].to_vec()
}

impl Rig {
    /// A pass with `ev` heard on their ports, then the out pass to both
    /// rings, USB's `usb`.
    fn pass_ports(&mut self, ev: &[(Port, CtrlEvent)], usb: &OutRing) {
        self.now += PASS_MS;
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
        self.ui.send(&OutPorts::new(&self.ring, usb));
    }

    /// Passes enough to settle, both rings emptied each: what each sent.
    fn settle_ports(&mut self, usb: &OutRing) -> (Vec<u8>, Vec<u8>) {
        let (mut din, mut u) = (Vec::new(), Vec::new());
        for _ in 0..100 {
            self.pass_ports(&[], usb);
            din.extend(self.drain());
            u.extend(pop(usb));
        }
        (din, u)
    }

    /// Both Parts' 16 NRPN targets echoing: a burst of 32 messages.
    fn bind_nrpns(&mut self) {
        for p in [P1, P2] {
            for &(n, a) in &nrpn_targets() {
                let input = Input::Nrpn(n, NrpnKind::Absolute(Takeover::Jump));
                self.bind(p, input, a, Feedback::Value);
            }
        }
    }
}

#[test]
fn identity_replies_on_asking_port_only() {
    let mut r = Rig::new();
    let usb = OutRing::new();
    let ask = CtrlEvent::Identity {
        dev: U7::new(0x7F).unwrap(),
    };
    let reply = |b: &[u8]| b.len() == 15 && b[0] == 0xF0 && b[14] == 0xF7;
    r.pass_ports(&[(Port::Usb, ask)], &usb);
    assert!(reply(&pop(&usb)), "USB asked, USB answered");
    assert!(r.drain().is_empty(), "DIN didn't ask");
    r.pass_ports(&[(Port::Din, ask)], &usb);
    assert!(reply(&r.drain()), "DIN asked, DIN answered");
    assert!(pop(&usb).is_empty(), "USB didn't ask");
    r.pass_ports(
        &[(Port::Din, ask), (Port::Usb, ask), (Port::Usb, ask)],
        &usb,
    );
    assert!(reply(&r.drain()) && reply(&pop(&usb)), "one each");
}

#[test]
fn full_usb_ring_never_holds_din() {
    let (mut a, mut b) = (Rig::new(), Rig::new());
    a.bind_nrpns();
    b.bind_nrpns();
    // Room for two messages: the first with its status, one running.
    let usb = OutRing::new();
    assert!(usb.push_msg(&[0; OUT_RING_LEN - 17]));
    let want = a.settle();
    // USB's ring never read: full all along.
    let mut din = Vec::new();
    for _ in 0..100 {
        b.pass_ports(&[], &usb);
        din.extend(b.drain());
    }
    let u = pop(&usb);
    assert_eq!(din, want, "DIN as with no USB at all");
    // 32 NRPNs, Part 2's split over two passes: its status twice.
    assert_eq!(want.len(), 2 * (1 + 16 * 8) + 1, "the burst");
    assert_eq!(&u[OUT_RING_LEN - 17..], &want[..17], "USB's copy, whole");
    assert_eq!(usb.take_dropped(), 30, "the rest dropped, counted");
    assert_eq!(usb.take_dropped(), 0);
}

/// The UI always copies to USB's ring: whether the host is there to read
/// it is USB's poll's business (it drops what waits while not Configured).
#[test]
fn usb_gets_dins_echo() {
    let (mut a, mut b) = (Rig::new(), Rig::new());
    for r in [&mut a, &mut b] {
        r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
        r.set(P1, CUTOFF, 0.5);
    }
    let usb = OutRing::new();
    let want = a.settle();
    let (din, u) = b.settle_ports(&usb);
    assert_eq!(din, want, "DIN as today");
    assert_eq!(values(&din, a.ch(P1), 20).len(), 1);
    assert_eq!(u, want, "the same bytes into USB's ring");
    assert_eq!(usb.take_dropped(), 0);
}

/// A USB copy lost to a host that never reads (#842): once USB's poll has
/// flushed what went stale, the active Part's values go out again, so the
/// host ends on the latest.
#[test]
fn lost_usb_echo_resends_the_active_part() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.set(P1, CUTOFF, 0.2);
    let usb = OutRing::new();
    r.settle_ports(&usb);
    while usb.push_msg(&[0xB5, 1, 2]) {}
    r.set(P1, CUTOFF, 0.8);
    r.pass_ports(&[], &usb);
    assert_eq!(values(&r.drain(), r.ch(P1), 20), [r.want(P1, CUTOFF)]);
    assert_eq!(usb.take_dropped(), 1, "USB's copy lost");
    // Not read: nothing more goes.
    r.pass_ports(&[], &usb);
    assert!(r.drain().is_empty(), "DIN untouched while USB stays full");
    usb.discard();
    let (d, u) = r.settle_ports(&usb);
    assert_eq!(values(&u, r.ch(P1), 20), [r.want(P1, CUTOFF)]);
    assert!(d.is_empty(), "DIN heard it already: USB's alone");
}

/// The ring drained but its loss not yet taken (#842): USB's poll will
/// drop what waits, so nothing is sent again until it has.
#[test]
fn a_lost_echo_waits_for_its_loss_to_be_taken() {
    let mut r = Rig::new();
    r.bind(P1, cc_in(20, JUMP), CUTOFF, Feedback::Value);
    r.set(P1, CUTOFF, 0.2);
    let usb = OutRing::new();
    r.settle_ports(&usb);
    while usb.push_msg(&[0xB5, 1, 2]) {}
    r.set(P1, CUTOFF, 0.8);
    r.pass_ports(&[], &usb);
    usb.clear();
    r.pass_ports(&[], &usb);
    assert!(usb.is_empty(), "nothing before the poll takes the loss");
    assert_eq!(usb.take_dropped(), 1);
    let (_, u) = r.settle_ports(&usb);
    assert_eq!(values(&u, r.ch(P1), 20), [r.want(P1, CUTOFF)]);
}

/// Every Part that lost a copy goes again, not the active one alone.
#[test]
fn a_lost_echo_resends_each_part_that_lost() {
    let mut r = Rig::new();
    for p in [P1, P2] {
        r.bind(p, cc_in(20, JUMP), CUTOFF, Feedback::Value);
        r.set(p, CUTOFF, 0.2);
    }
    let usb = OutRing::new();
    r.settle_ports(&usb);
    while usb.push_msg(&[0xB5, 1, 2]) {}
    r.set(P2, CUTOFF, 0.8);
    r.pass_ports(&[], &usb);
    assert_eq!(values(&r.drain(), r.ch(P2), 20), [r.want(P2, CUTOFF)]);
    usb.discard();
    let (d, u) = r.settle_ports(&usb);
    assert!(d.is_empty());
    assert_eq!(values(&u, r.ch(P2), 20), [r.want(P2, CUTOFF)]);
}

/// A copy USB's poll moved out of the ring but the host hasn't taken
/// (held, its ring empty) is dropped by a later loss too: its Part goes
/// again.
#[test]
fn a_lost_echo_resends_what_the_poll_held() {
    let mut r = Rig::new();
    for p in [P1, P2] {
        r.bind(p, cc_in(20, JUMP), CUTOFF, Feedback::Value);
        r.set(p, CUTOFF, 0.2);
    }
    let usb = OutRing::new();
    r.settle_ports(&usb);
    r.set(P2, CUTOFF, 0.8);
    r.pass_ports(&[], &usb);
    assert_eq!(
        values(&pop(&usb), r.ch(P2), 20),
        [r.want(P2, CUTOFF)],
        "held"
    );
    r.pass_ports(&[], &usb);
    while usb.push_msg(&[0xB5, 1, 2]) {}
    r.set(P1, CUTOFF, 0.8);
    r.pass_ports(&[], &usb);
    usb.discard();
    r.drain();
    let (_, u) = r.settle_ports(&usb);
    assert_eq!(values(&u, r.ch(P2), 20), [r.want(P2, CUTOFF)]);
}
