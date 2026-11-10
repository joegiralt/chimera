//! MIDI CONFIG › ECHO › PARAM OUT (#913): every edit made on the unit goes
//! out as its NRPN (ADR 0216's address and law) on the Part's channel, the
//! latest value a pass while DIN has room; never a value MIDI brought in,
//! nothing with PARAM OUT or ECHO off.
#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use chimera_core::block::Block;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::midi_map::MidiMap;
use chimera_core::nrpn::{self, ChartLine};
use chimera_core::out_ring::{OUT_RING_LEN, OutRing};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::{PartFrom, PartId, PartSource, ReplaceGuard};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{FILTER, SYS_ECHO};
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_core::ui::nav::PartChain;
use chimera_core::ui::param_out;
use chimera_hal::ButtonId::Plus;
use chimera_hal::EncoderId;
use chimera_hal::Ms;
use chimera_midi::ctl::{Ctl, DataValue, Sel};
use chimera_midi::law::{ValueLaw, place_of};
use chimera_midi::value::U14;
use screen::{Input as Key, feed};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const PASS_MS: u32 = 4;

struct Rig {
    ui: Box<UiState>,
    ring: OutRing,
    now: u32,
}

impl Rig {
    /// INIT Algo on Part 1, its map empty (no echo of its own), on FLT,
    /// PARAM OUT `on`.
    fn new(on: bool) -> Self {
        let mut ui = Box::new(screen::stack_ui());
        let was = PartChain::of(&ui.project().part(P1).sound);
        screen::init_part(&mut ui, P1);
        ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
        ui.replaced_for_test(was);
        let m = ui.midi().with_param_out(on);
        ui.set_midi(m);
        let mut r = Rig {
            ui,
            ring: OutRing::new(),
            now: 1_000,
        };
        for _ in 0..12 {
            if r.ui.page_def().id == FILTER.id {
                break;
            }
            feed(&mut r.ui, Key::press(Plus));
        }
        assert_eq!(r.ui.page_def().id, FILTER.id);
        r.settle();
        r
    }

    fn ch(&self) -> MidiChannel {
        self.ui.project().part(P1).mix.channel
    }

    fn pass(&mut self, ev: &[CtrlEvent], key: Option<Key>) {
        self.now += PASS_MS;
        if let Some(k) = key {
            feed(&mut self.ui, k.at(self.now));
        }
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
        self.ui.send_out(&self.ring);
    }

    fn drain(&self) -> Vec<u8> {
        let mut b = [0u8; OUT_RING_LEN];
        let n = self.ring.pop_into(&mut b);
        b[..n].to_vec()
    }

    fn settle(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..100 {
            self.pass(&[], None);
            out.extend(self.drain());
        }
        out
    }

    /// `a`'s NRPN data now, by its law, as the chart reads it.
    fn want(&self, a: ParamAddr) -> DataValue {
        param_out::data(self.ui.project(), P1, a).unwrap()
    }
}

/// An NRPN heard in `bytes`: channel, address, data MSB, data LSB if sent.
type Heard = (u8, u16, u8, Option<u8>);

/// The NRPNs in `bytes`, running status read; panics on anything else.
fn nrpns(bytes: &[u8]) -> Vec<Heard> {
    let mut ccs = Vec::new();
    let (mut status, mut i) = (0u8, 0);
    while i < bytes.len() {
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
    out
}

fn as_heard(ch: MidiChannel, n: u16, d: DataValue) -> Heard {
    match d {
        DataValue::Coarse(m) => (ch.get(), n, m.get(), None),
        DataValue::Fine(f) => (ch.get(), n, f.msb().get(), Some(f.lsb().get())),
    }
}

/// CUTOFF's address: 10:0, FILTER's code and the param's id.
const CUTOFF_N: u16 = 10 << 7;

#[test]
fn param_out_is_off_by_default_and_stored_by_ident() {
    let s = MidiSettings::DEFAULT;
    assert!(!s.param_out());
    let spec = s
        .specs()
        .iter()
        .find(|p| p.id == MidiSettings::PARAM_OUT)
        .unwrap();
    assert_eq!(
        (spec.label, spec.ident, spec.default),
        ("PARAM OUT", "PARAM_OUT", 0.0)
    );
    assert_eq!(MidiSettings::PARAM_OUT.0, 12);
    let on = s.with_param_out(true);
    assert!(on.param_out() && on.echo(), "its own bit");
    assert_eq!(on.get(MidiSettings::PARAM_OUT), 1.0);
    let mut w = s;
    w.write(MidiSettings::PARAM_OUT, 1.0);
    assert_eq!(w, on);
}

#[test]
fn panel_turn_sends_nothing_with_param_out_off() {
    let mut r = Rig::new(false);
    for _ in 0..10 {
        r.pass(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    assert!(r.settle().is_empty());
}

/// CUTOFF is FLT's F, Place14: each pass's latest value, 14-bit, on Part
/// 1's channel, climbing, the last where it rests; nothing once it rests.
#[test]
fn panel_turn_sends_its_nrpn() {
    let mut r = Rig::new(true);
    let mut got = Vec::new();
    for _ in 0..10 {
        r.pass(&[], Some(Key::turn(EncoderId::F, 3)));
        got.extend(r.drain());
    }
    got.extend(r.settle());
    let heard = nrpns(&got);
    assert!(!heard.is_empty() && heard.len() <= 10, "{heard:?}");
    let ch = r.ch().get();
    assert!(
        heard
            .iter()
            .all(|h| h.0 == ch && h.1 == CUTOFF_N && h.3.is_some())
    );
    let v: Vec<u16> = heard
        .iter()
        .map(|h| u16::from(h.2) << 7 | u16::from(h.3.unwrap()))
        .collect();
    assert!(v.windows(2).all(|w| w[0] < w[1]), "climbs: {v:?}");
    assert_eq!(
        *heard.last().unwrap(),
        as_heard(r.ch(), CUTOFF_N, r.want(CUTOFF))
    );
    assert_eq!(got[0], 0xB0 | ch, "status first");
    assert!(r.settle().is_empty());
}

/// A turn against the top moves nothing: nothing more goes.
#[test]
fn a_turn_that_moves_nothing_sends_nothing() {
    let mut r = Rig::new(true);
    for _ in 0..40 {
        r.pass(&[], Some(Key::turn(EncoderId::F, 100)));
    }
    r.settle();
    r.pass(&[], Some(Key::turn(EncoderId::F, 10)));
    assert!(r.settle().is_empty());
}

/// What MIDI moved is never sent back: no loop through a thru box.
#[test]
fn midi_in_is_never_sent_back() {
    let mut r = Rig::new(true);
    let before =
        r.ui.project()
            .read_part(P1)
            .block(BlockRef::Filter)
            .unwrap()
            .get(CUTOFF.param);
    let ev = CtrlEvent::Ctl(
        r.ch(),
        Ctl::Data {
            sel: Sel::Nrpn(U14::new(CUTOFF_N).unwrap()),
            value: DataValue::Fine(U14::new(3000).unwrap()),
        },
    );
    r.pass(&[ev], None);
    let mut got = r.drain();
    got.extend(r.settle());
    let after =
        r.ui.project()
            .read_part(P1)
            .block(BlockRef::Filter)
            .unwrap()
            .get(CUTOFF.param);
    assert_ne!(before, after, "the NRPN moved CUTOFF");
    assert!(got.is_empty(), "{got:02X?}");
}

/// DIN's ring full: the turns wait as one, and the latest goes alone.
#[test]
fn a_full_ring_coalesces_to_the_latest() {
    let mut r = Rig::new(true);
    let filler = [0xB5u8, 1, 2];
    while r.ring.push_msg(&filler) {}
    for _ in 0..5 {
        r.pass(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    assert!(r.drain().chunks(3).all(|c| c == filler));
    r.pass(&[], None);
    let got = r.drain();
    assert_eq!(nrpns(&got), [as_heard(r.ch(), CUTOFF_N, r.want(CUTOFF))]);
    assert!(r.settle().is_empty());
}

/// A turn still waiting when a Sound loads into its Part is dropped: the
/// load's values never go out as edits.
#[test]
fn a_load_drops_the_turns_waiting() {
    let mut r = Rig::new(true);
    let filler = [0xB5u8, 1, 2];
    while r.ring.push_msg(&filler) {}
    r.pass(&[], Some(Key::turn(EncoderId::F, 9)));
    let was = PartChain::of(&r.ui.project().part(P1).sound);
    let src = PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Algo),
    };
    let ask = ReplaceGuard::check(r.ui.project(), r.ui.template(), src).expect_err("edited");
    let c = ask.into_pending().anyway(r.ui.project());
    r.ui.project_mut().replace_part(c).expect("INIT loads");
    r.ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    r.ui.replaced_for_test(was);
    assert!(r.drain().chunks(3).all(|c| c == filler));
    assert!(r.settle().is_empty());
}

/// ECHO is DIN OUT's master switch (ADR 0217): off, PARAM OUT is quiet.
#[test]
fn echo_off_silences_param_out() {
    let mut r = Rig::new(true);
    let m = r.ui.midi().with_echo(false);
    r.ui.set_midi(m);
    for _ in 0..10 {
        r.pass(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    assert!(r.settle().is_empty());
}

/// SETTINGS › MIDI CONFIG › ECHO: B is PARAM OUT, OFF then NRPN. Shot:
/// `$SCREEN_DUMP/midi_echo_param_out.ppm`.
#[test]
fn echo_page_b_turns_param_out() {
    let mut ui = Box::new(screen::stack_ui());
    screen::to_leaf(&mut ui, &["MIDI CONFIG", "ECHO"]);
    assert_eq!(ui.page_def().id, SYS_ECHO.id);
    assert!(!ui.midi().param_out());
    feed(&mut ui, Key::turn(EncoderId::B, 1));
    assert!(ui.midi().param_out());
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("midi_echo_param_out");
    assert!(screen::cell_reads(&shot, 0, "ON"));
    assert!(screen::cell_reads(&shot, 1, "NRPN"));
    feed(&mut ui, Key::turn(EncoderId::B, -1));
    assert!(!ui.midi().param_out());
}

/// Every writable param's data, read back through the receiver's law
/// (`place_of`), names the value it was sent from: the out law is the
/// in law's inverse, for each law, at a few places of each range.
#[test]
fn data_reads_back_through_the_in_law() {
    let mut r = Rig::new(true);
    let mut laws = std::collections::HashSet::new();
    for l in nrpn::chart() {
        let ChartLine::Live { block, spec, .. } = l else {
            continue;
        };
        let a = ParamAddr::new(block, spec.id);
        // TAPE SLICE's code loads as OFF (its load policy), so the in law
        // reads every code as OFF: the out law sends the code regardless.
        if nrpn::writable(a).is_none() || (block, spec.ident) == (BlockRef::TapeEngine, "SLICE") {
            continue;
        }
        // Sound blocks: the probe for Code7 needs a block to write.
        let Some(_) = r.ui.project().part(P1).sound.params.block(block) else {
            continue;
        };
        let law = nrpn::law(block, spec);
        for place in [0.0, 0.3, 0.71, 1.0] {
            let s = r.ui.project_mut().edit_part(P1).sound;
            let mut b = s.params.block_mut(block).unwrap();
            b.set(a.param, spec.denormalize(place));
            let was = b.get(a.param);
            let d = param_out::data(r.ui.project(), P1, a).unwrap();
            if matches!(law, ValueLaw::Index7 | ValueLaw::Code7) {
                assert!(matches!(d, DataValue::Coarse(_)), "{l}");
            } else {
                assert!(matches!(d, DataValue::Fine(_)), "{l}");
            }
            let mut probe = r.ui.project().part(P1).sound.params.clone();
            let p = place_of(law, d, nrpn::steps(spec), |c| {
                nrpn::index_of_code(&mut *probe.block_mut(block).unwrap(), a.param, c)
            })
            .unwrap_or_else(|| panic!("{l} at {place}"));
            let back = match law {
                ValueLaw::Place14 => spec.denormalize(f32::from(p.get()) / 16383.0),
                _ => {
                    let i = (u32::from(p.get()) * u32::from(nrpn::steps(spec)) + 8191) / 16383;
                    spec.min + i as f32
                }
            };
            let tol = (spec.max - spec.min).abs() * 2e-4;
            assert!((back - was).abs() <= tol.max(1e-4), "{l}: {was} -> {back}");
            laws.insert(law_name(law));
        }
    }
    assert_eq!(laws.len(), 4, "every law: {laws:?}");
}

fn law_name(l: ValueLaw) -> &'static str {
    match l {
        ValueLaw::Place14 => "Place14",
        ValueLaw::Index7 => "Index7",
        ValueLaw::Index14 => "Index14",
        ValueLaw::Code7 => "Code7",
    }
}

/// The master FX's REV page turns by its TYPE's panel: MIX's turn goes
/// out too, on the active Part's channel.
#[test]
fn a_reverb_turn_sends_its_nrpn() {
    use chimera_core::dsp::reverb::ReverbParams;
    use chimera_core::ui::block_registry::EFX;
    use chimera_hal::ButtonId::{B1, Mix};
    let mix = ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX);
    let mut r = Rig::new(true);
    feed(&mut r.ui, Key::press(B1).and_held(Mix));
    feed(&mut r.ui, Key::release(B1));
    for _ in 0..3 {
        feed(&mut r.ui, Key::press(Plus));
    }
    assert_eq!(r.ui.page_def().id, EFX.id);
    r.settle();
    r.pass(&[], Some(Key::turn(EncoderId::F, -5)));
    let n = nrpn::NrpnAddr::of(mix).unwrap().get().get();
    let heard = nrpns(&r.settle());
    assert_eq!(heard, [as_heard(r.ch(), n, r.want(mix))]);
}
