// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bindings resolve (spec §§ 5, 6.2): takeover, relative, button, 14-bit,
//! pressure, canonical NRPN and RPN 0, each through ADR 0139's ramp, bin
//! and edit path, heard off the control ring as `CtrlEvent`s.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Blocks, ParamAddr};
use chimera_core::block::{ParamKind, ParamSpec};
use chimera_core::dsp::knobs::{KNOBS, KnobPublisher, MOVES};
use chimera_core::dsp::modal::ModalParams;
use chimera_core::dsp::va::VaParams;
use chimera_core::dsp::voice_fx::params::VoiceFxParams;
use chimera_core::instrument::AudioShared;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::midi_map::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, Mappable, NrpnKind, Span,
};
use chimera_core::nrpn::NrpnAddr;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::nav::PartChain;
use chimera_core::{MidiChannel, ui::midi::resolve};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId, Ms};
use chimera_midi::ctl::{Ctl, DataValue, Dir, FreeCc, Sel};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::{Place, Takeover};
use chimera_midi::value::{U7, U14};
use screen::{Input as Key, feed};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
const VA_MODE: ParamAddr = ParamAddr::new(BlockRef::Va, VaParams::MODE);
const STRUCTURE: ParamAddr = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);
const BEND: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::BEND);
const PASS_MS: u32 = 4;

fn spec(a: ParamAddr) -> &'static ParamSpec {
    a.spec().unwrap()
}

/// A UI published each pass, the audio taking every publish: the most
/// knob moves of Part 1 one publish carried.
struct Rig {
    ui: Box<UiState>,
    shared: Box<AudioShared>,
    knobs: Box<KnobPublisher>,
    now: u32,
    seen: u32,
    most: u32,
}

impl Rig {
    fn new(s: Sound) -> Self {
        let mut ui = Box::new(screen::stack_ui());
        screen::init_part(&mut ui, P1);
        let was = PartChain::of(&ui.project().part(P1).sound);
        **ui.project_mut().edit_part(P1).sound = s;
        ui.replaced_for_test(was);
        let mut r = Rig {
            ui,
            shared: Box::new(AudioShared::default()),
            knobs: Box::new(KnobPublisher::new()),
            now: 1_000,
            seen: 0,
            most: 0,
        };
        r.publish();
        r.seen = r.shared.parts[0].knobs.count();
        r
    }

    fn algo() -> Self {
        Self::new(Sound::plain_init(EngineType::Algo))
    }

    fn ch(&self, p: PartId) -> MidiChannel {
        self.ui.project().part(p).mix.channel
    }

    fn bind(&mut self, p: PartId, input: Input, a: ParamAddr) {
        let s = self.ui.project_mut().edit_part(p).sound;
        s.map.learn(input, Mappable::new(a).unwrap()).unwrap();
    }

    fn publish(&mut self) {
        self.shared
            .update_from_project(self.ui.project(), 0, &mut self.knobs);
        let c = self.shared.parts[0].knobs.count();
        self.most = self.most.max(c.wrapping_sub(self.seen));
        self.seen = c;
    }

    fn pass(&mut self, ev: &[CtrlEvent]) {
        self.pass_with(ev, None);
    }

    fn pass_with(&mut self, ev: &[CtrlEvent], key: Option<Key>) {
        self.now += PASS_MS;
        if let Some(k) = key {
            feed(&mut self.ui, k);
        }
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
        self.publish();
    }

    fn passes(&mut self, n: usize) {
        for _ in 0..n {
            self.pass(&[]);
        }
    }

    fn stored(&self, p: PartId, a: ParamAddr) -> f32 {
        let part = self.ui.project().read_part(p);
        part.block(a.block).unwrap().get(a.param)
    }

    fn place(&self, p: PartId, a: ParamAddr) -> f32 {
        spec(a).normalize(self.stored(p, a))
    }

    fn set(&mut self, p: PartId, a: ParamAddr, place: f32) {
        let s = self.ui.project_mut().edit_part(p).sound;
        let mut b = s.params.block_mut(a.block).unwrap();
        b.set(a.param, spec(a).denormalize(place));
    }

    fn cc(&self, p: PartId, n: u8, v: u8) -> CtrlEvent {
        ev(
            self.ch(p),
            Ctl::Cc {
                cc: FreeCc::new(n).unwrap(),
                value: U7::new(v).unwrap(),
            },
        )
    }

    fn nrpn(&self, p: PartId, n: U14, value: DataValue) -> CtrlEvent {
        ev(
            self.ch(p),
            Ctl::Data {
                sel: Sel::Nrpn(n),
                value,
            },
        )
    }

    fn go_to_page(&mut self, id: u16) {
        for _ in 0..12 {
            if self.ui.page_def().id == id {
                return;
            }
            feed(&mut self.ui, Key::press(ButtonId::Plus));
        }
        panic!("page {id} not reached");
    }
}

fn ev(ch: MidiChannel, c: Ctl) -> CtrlEvent {
    CtrlEvent::Ctl(ch, c)
}

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

fn cc_input(n: u8, k: CcKind) -> Input {
    Input::Cc(LearnCc::new(n).unwrap(), k)
}

fn addr_of(a: ParamAddr) -> U14 {
    NrpnAddr::of(a).unwrap().get()
}

/// The place a 7-bit value lands on, full span.
fn place7(v: u8) -> f32 {
    v as f32 / 127.0
}

#[test]
fn pickup_never_moves_before_engaging() {
    let mut r = Rig::algo();
    r.bind(P1, cc_input(20, CcKind::Absolute(Takeover::Pickup)), CUTOFF);
    r.set(P1, CUTOFF, place7(90));
    r.passes(2);
    let at = r.stored(P1, CUTOFF);
    let rev = r.ui.project().rev();
    // Below and climbing, never within reach: nothing moves.
    for v in 0..=80 {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
        assert_eq!(r.stored(P1, CUTOFF), at, "moved at {v} before engaging");
    }
    assert_eq!(r.ui.project().rev(), rev, "nothing written");
    // Through the param's place: it engages and follows.
    for v in 81..=110 {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
    }
    r.passes(40);
    assert!((r.place(P1, CUTOFF) - place7(110)).abs() < 1e-4);
}

#[test]
fn pickup_rearms_after_panel_turn() {
    let mut r = Rig::algo();
    r.bind(P1, cc_input(20, CcKind::Absolute(Takeover::Pickup)), CUTOFF);
    r.set(P1, CUTOFF, place7(64));
    r.go_to_page(FILTER.id);
    // Engaged at the param's place, then moved a little.
    for v in [64, 66, 68, 70] {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
    }
    r.passes(40);
    assert!((r.place(P1, CUTOFF) - place7(70)).abs() < 1e-4, "engaged");
    // The panel takes CUTOFF well away.
    for _ in 0..30 {
        r.pass_with(&[], Some(Key::turn(EncoderId::F, 3)));
    }
    r.passes(5);
    let turned = r.stored(P1, CUTOFF);
    assert!(r.place(P1, CUTOFF) > place7(80), "the hand moved it up");
    // The controller carries on from where it was: it must pick up again.
    for v in 71..=75 {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
    }
    r.passes(40);
    assert_eq!(r.stored(P1, CUTOFF), turned, "re-armed: waits for the knob");
}

#[test]
fn scale_converges() {
    let mut r = Rig::algo();
    r.bind(P1, cc_input(20, CcKind::Absolute(Takeover::Scale)), CUTOFF);
    r.set(P1, CUTOFF, 0.75);
    r.passes(2);
    // The controller starts at the bottom: moving up scales the rest of
    // the way, never a jump, and lands on the top with the knob.
    let mut was = r.place(P1, CUTOFF);
    for v in 0..=127 {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
        r.passes(30);
        let now = r.place(P1, CUTOFF);
        assert!(now >= was - 1e-6, "scaled up only: {was} -> {now} at {v}");
        assert!(now - was < 0.02, "no jump: {was} -> {now} at {v}");
        was = now;
    }
    assert_eq!(r.place(P1, CUTOFF), 1.0, "converged at the top");
    // And down: it meets the bottom with the knob.
    for v in (0..=126).rev() {
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
    }
    r.passes(40);
    assert_eq!(r.place(P1, CUTOFF), 0.0, "converged at the bottom");
    // Pure: a first message is no move.
    let b = Binding {
        input: cc_input(20, CcKind::Absolute(Takeover::Scale)),
        target: Mappable::new(CUTOFF).unwrap(),
        span: Span::FULL,
        feedback: Feedback::Off,
    };
    let mut rt = resolve::BindRuntime::NEW;
    let c = Ctl::Cc {
        cc: FreeCc::new(20).unwrap(),
        value: u7(0),
    };
    assert_eq!(
        resolve::resolve(&b, &mut rt, &c, Place::new(8000).unwrap()),
        None
    );
}

#[test]
fn relative_turns_match_panel_detents() {
    let mut cc = Rig::algo();
    let mut hand = Rig::algo();
    cc.bind(
        P1,
        cc_input(20, CcKind::Relative(ByteEncoding::BinaryOffset)),
        CUTOFF,
    );
    hand.go_to_page(FILTER.id);
    let start = cc.stored(P1, CUTOFF);
    assert_eq!(hand.stored(P1, CUTOFF), start);
    for (byte, d) in [(0x41u8, 1i8); 20]
        .into_iter()
        .chain([(0x3F, -1); 30])
        .chain([(0x43, 3); 5])
    {
        let e = cc.cc(P1, 20, byte);
        cc.pass(&[e]);
        hand.pass_with(&[], Some(Key::turn(EncoderId::F, d)));
        assert_eq!(cc.stored(P1, CUTOFF), hand.stored(P1, CUTOFF), "{byte:#x}");
    }
    assert_ne!(cc.stored(P1, CUTOFF), start);
}

/// Eight continuous knobs Part 1's ALGO reads, the filter's aside.
fn eight_knobs() -> Vec<ParamAddr> {
    let k: Vec<_> = KNOBS
        .iter()
        .filter(|k| {
            k.addr.block != BlockRef::Filter
                && k.spec.kind == ParamKind::Continuous
                && k.addr.block.voice_reads()
                && Mappable::new(k.addr).is_some_and(|m| m.live(EngineType::Algo))
        })
        .map(|k| k.addr)
        .take(8)
        .collect();
    assert_eq!(k.len(), 8);
    k
}

#[test]
fn relative_flood_never_lands() {
    // A flood on eight knobs: never past the budget, and every knob turns.
    let mut r = Rig::algo();
    let knobs = eight_knobs();
    for (i, &a) in knobs.iter().enumerate() {
        r.bind(
            P1,
            cc_input(20 + i as u8, CcKind::Relative(ByteEncoding::TwosComplement)),
            a,
        );
        r.set(P1, a, 0.5);
    }
    r.passes(2);
    let before: Vec<f32> = knobs.iter().map(|&a| r.stored(P1, a)).collect();
    r.most = 0;
    for _ in 0..50 {
        let flood: Vec<_> = (0..8)
            .flat_map(|i| [r.cc(P1, 20 + i, 1), r.cc(P1, 20 + i, 1)])
            .collect();
        r.pass(&flood);
        assert!((r.most as usize) < MOVES, "{} moves a publish", r.most);
    }
    r.passes(20);
    for (i, &a) in knobs.iter().enumerate() {
        assert!(r.stored(P1, a) > before[i], "{a:?} turned");
    }
    // During a busy load: nothing lands, then or after.
    let mut s = MemStore::new(1);
    let f = screen::put_with(&mut s, "NEXT", |p| {
        let s = p.edit_part(P1).sound;
        s.map
            .learn(
                cc_input(20, CcKind::Relative(ByteEncoding::TwosComplement)),
                Mappable::new(CUTOFF).unwrap(),
            )
            .unwrap();
    });
    let mut ui = Box::new(screen::stack_ui());
    let ch = ui.project().part(P1).mix.channel;
    let up = ev(
        ch,
        Ctl::Cc {
            cc: FreeCc::new(20).unwrap(),
            value: u7(1),
        },
    );
    let mut rig = screen::Rig::new(&mut ui, s);
    rig.to(&["PROJECT", "LOAD PROJECT"]);
    let l = rig.ui.listing();
    let i = (0..l.len())
        .find(|&i| l.entry(i).unwrap().file() == f)
        .expect("listed");
    rig.bar_to(i);
    screen::tap(rig.ui, ButtonId::Edit);
    assert!(rig.ui.job_queued_for_test());
    for t in 0..20 {
        rig.ui
            .hear_ccs([up; 16].into_iter(), Ms(2_000 + t * PASS_MS), true);
    }
    rig.work();
    assert_eq!(rig.ui.project().meta().file(), Some(f), "loaded");
    let loaded = {
        let p = rig.ui.project().read_part(P1);
        p.block(BlockRef::Filter).unwrap().get(FilterParams::CUTOFF)
    };
    for t in 0..20 {
        rig.ui.hear_ccs(
            core::iter::empty::<CtrlEvent>(),
            Ms(3_000 + t * PASS_MS),
            true,
        );
    }
    let p = rig.ui.project().read_part(P1);
    assert_eq!(
        p.block(BlockRef::Filter).unwrap().get(FilterParams::CUTOFF),
        loaded,
        "no detent heard while busy landed"
    );
}

#[test]
fn cc14_msb_applies_without_waiting() {
    let mut r = Rig::algo();
    let msb = Cc14Msb::new(2).unwrap();
    r.bind(P1, Input::Cc14(msb, Takeover::Jump), CUTOFF);
    let e = r.cc(P1, 2, 100);
    r.pass(&[e]);
    r.passes(40);
    let want = f32::from(100u16 << 7 | 100) / 16383.0;
    assert!((r.place(P1, CUTOFF) - want).abs() < 1e-4, "the MSB alone");
    let e = r.cc(P1, 34, 5);
    r.pass(&[e]);
    r.passes(40);
    let want = f32::from(100u16 << 7 | 5) / 16383.0;
    assert!(
        (r.place(P1, CUTOFF) - want).abs() < 1e-4,
        "refined by the LSB"
    );
}

#[test]
fn button_momentary_and_toggle() {
    let mut r = Rig::algo();
    r.bind(
        P1,
        cc_input(20, CcKind::Button(ButtonMode::Momentary)),
        RESO,
    );
    r.bind(P1, cc_input(21, CcKind::Button(ButtonMode::Toggle)), CUTOFF);
    r.set(P1, RESO, 0.5);
    r.set(P1, CUTOFF, 0.5);
    let press = |r: &mut Rig, n: u8, v: u8| {
        let e = r.cc(P1, n, v);
        r.pass(&[e]);
        r.passes(40);
    };
    press(&mut r, 20, 127);
    assert_eq!(r.place(P1, RESO), 1.0, "held is max");
    press(&mut r, 20, 0);
    assert_eq!(r.place(P1, RESO), 0.0, "released is min");
    press(&mut r, 21, 127);
    assert_eq!(r.place(P1, CUTOFF), 1.0, "a press flips to max");
    press(&mut r, 21, 0);
    assert_eq!(r.place(P1, CUTOFF), 1.0, "a release keeps it");
    press(&mut r, 21, 127);
    assert_eq!(r.place(P1, CUTOFF), 0.0, "the next press flips back");
    // It flips the param, not its own memory: moved to max meanwhile, a
    // press takes it down (#820).
    press(&mut r, 21, 0);
    r.set(P1, CUTOFF, 1.0);
    press(&mut r, 21, 127);
    assert_eq!(r.place(P1, CUTOFF), 0.0, "max flips to min");
}

#[test]
fn pressure_moves_bound_param() {
    let mut r = Rig::algo();
    r.bind(P1, Input::Pressure(Takeover::Jump), CUTOFF);
    r.set(P1, CUTOFF, 0.5);
    let e = ev(r.ch(P1), Ctl::Pressure(u7(0)));
    r.pass(&[e]);
    r.passes(40);
    assert_eq!(r.stored(P1, CUTOFF), spec(CUTOFF).min);
    let e = ev(r.ch(P1), Ctl::Pressure(u7(127)));
    r.pass(&[e]);
    r.passes(40);
    assert_eq!(r.stored(P1, CUTOFF), spec(CUTOFF).max);
}

#[test]
fn canonical_nrpn_moves_mappable_only() {
    let mut r = Rig::algo();
    r.set(P1, CUTOFF, 0.5);
    // A continuous param, no binding: Place14, ramped there.
    let e = r.nrpn(P1, addr_of(CUTOFF), DataValue::Coarse(u7(0)));
    r.pass(&[e]);
    let mid = r.place(P1, CUTOFF);
    assert!(mid < 0.5 && mid > 0.0, "ramped, not snapped: {mid}");
    r.passes(40);
    assert_eq!(r.stored(P1, CUTOFF), spec(CUTOFF).min);
    let fine = U14::from_parts(u7(64), u7(0));
    let e = r.nrpn(P1, addr_of(CUTOFF), DataValue::Fine(fine));
    r.pass(&[e]);
    r.passes(40);
    let want = f32::from(fine.get()) / 16383.0;
    assert!((r.place(P1, CUTOFF) - want).abs() < 1e-4, "14-bit");
    // The Part's LEVEL is mappable; FX TYPE isn't, nor the reverb's and a
    // ring's switches (#512).
    let level = ParamAddr::new(BlockRef::Part, PartParams::LEVEL);
    let e = r.nrpn(P1, addr_of(level), DataValue::Coarse(u7(0)));
    r.pass(&[e]);
    r.passes(40);
    assert_eq!(r.place(P1, level), 0.0, "LEVEL moved");
    let rev = r.ui.project().rev();
    let kind = ParamAddr::new(BlockRef::VoiceFx, VoiceFxParams::TYPE);
    for a in [kind]
        .into_iter()
        .chain(chimera_core::nrpn::chart().filter_map(|l| match l {
            chimera_core::nrpn::ChartLine::Live { block, spec, .. }
                if matches!(block, BlockRef::Reverb | BlockRef::OrbitRing(_)) =>
            {
                Some(ParamAddr::new(block, spec.id))
            }
            _ => None,
        }))
        .filter(|&a| Mappable::new(a).is_none())
    {
        let e = r.nrpn(P1, addr_of(a), DataValue::Coarse(u7(3)));
        r.pass(&[e]);
    }
    r.passes(10);
    assert_eq!(r.ui.project().rev(), rev, "nothing outside Mappable moved");
    // A stepped one is binned through the edit path: VA's MODE by its code.
    let mut va = Rig::new(Sound::plain_init(EngineType::Va));
    let s = spec(VA_MODE);
    let was = va.stored(P1, VA_MODE);
    let to = if was == s.min { s.min + 1.0 } else { s.min };
    let code = {
        let mut p = va.ui.project().part(P1).sound.params.clone();
        let mut b = p.block_mut(BlockRef::Va).unwrap();
        b.set(VaParams::MODE, to);
        b.enum_code(VaParams::MODE).unwrap()
    };
    let e = va.nrpn(P1, addr_of(VA_MODE), DataValue::Coarse(u7(code)));
    va.pass(&[e]);
    va.passes(5);
    assert_eq!(va.stored(P1, VA_MODE), to, "MODE set by its code");
}

/// `n` continuous params with an NRPN address, mappable and live on ALGO,
/// CUTOFF last.
fn canonical_params(n: usize) -> Vec<ParamAddr> {
    let mut k: Vec<_> = chimera_core::nrpn::chart()
        .filter_map(|l| match l {
            chimera_core::nrpn::ChartLine::Live { block, spec, .. } => {
                Some(ParamAddr::new(block, spec.id))
            }
            _ => None,
        })
        .filter(|&a| {
            a != CUTOFF
                && spec(a).kind == ParamKind::Continuous
                && Mappable::new(a).is_some_and(|m| m.live(EngineType::Algo))
        })
        .take(n - 1)
        .collect();
    k.push(CUTOFF);
    assert_eq!(k.len(), n);
    k
}

#[test]
fn canonical_nrpns_past_the_slots_all_land() {
    // Twenty canonical NRPNs in one pass, more than a Part has slots: each
    // ramps to its value in turn, none lost, never past the budget.
    let mut r = Rig::algo();
    let params = canonical_params(20);
    for &a in &params {
        r.set(P1, a, 0.5);
    }
    r.passes(2);
    let v = |i: usize| 10 + 5 * i as u8;
    let ev: Vec<_> = params
        .iter()
        .enumerate()
        .map(|(i, &a)| r.nrpn(P1, addr_of(a), DataValue::Coarse(u7(v(i)))))
        .collect();
    let (first, last) = (r.place(P1, params[0]), r.stored(P1, CUTOFF));
    r.most = 0;
    r.pass(&ev);
    r.pass(&[]);
    let mid = r.place(P1, params[0]);
    assert!(
        mid < first && mid > place7(v(0)),
        "ramped, not snapped: {mid}"
    );
    assert_eq!(r.stored(P1, CUTOFF), last, "the last waits its turn");
    for _ in 0..200 {
        r.pass(&[]);
        assert!((r.most as usize) <= MOVES, "{} moves a publish", r.most);
    }
    for (i, &a) in params.iter().enumerate() {
        let got = r.place(P1, a);
        assert!((got - place7(v(i))).abs() < 1e-4, "{a:?} at {got}");
    }
}

#[test]
fn canonical_nrpns_past_the_line_keep_the_newest() {
    // Thirty canonical NRPNs in one pass, more than the slots and the line
    // hold: the oldest waiting gives way, the newest (CUTOFF) still lands.
    let mut r = Rig::algo();
    let params = canonical_params(30);
    for &a in &params {
        r.set(P1, a, 0.5);
    }
    r.passes(2);
    let ev: Vec<_> = params
        .iter()
        .map(|&a| r.nrpn(P1, addr_of(a), DataValue::Coarse(u7(10))))
        .collect();
    r.pass(&ev);
    r.passes(200);
    for &a in &params[30 - 16..] {
        let got = r.place(P1, a);
        assert!((got - place7(10)).abs() < 1e-4, "{a:?} at {got}");
    }
}

#[test]
fn canonical_nrpn_sweeps_past_the_slots_take_turns() {
    // Twelve params swept at once, every pass: each moves while the sweep
    // lasts (none starves behind the slots), and each lands on its last.
    let mut r = Rig::algo();
    let params = canonical_params(12);
    for &a in &params {
        r.set(P1, a, 0.0);
    }
    r.passes(2);
    let start: Vec<f32> = params.iter().map(|&a| r.stored(P1, a)).collect();
    for v in 1..=100u8 {
        let ev: Vec<_> = params
            .iter()
            .map(|&a| r.nrpn(P1, addr_of(a), DataValue::Coarse(u7(v))))
            .collect();
        r.pass(&ev);
    }
    for (i, &a) in params.iter().enumerate() {
        assert!(r.stored(P1, a) > start[i], "{a:?} starved");
    }
    r.passes(200);
    for &a in &params {
        let got = r.place(P1, a);
        assert!((got - place7(100)).abs() < 1e-4, "{a:?} at {got}");
    }
}

#[test]
fn canonical_nrpn_waiting_yields_to_the_hand() {
    // CUTOFF waits behind nineteen others; the panel turns it: the value
    // it waited with never lands.
    let mut r = Rig::algo();
    let params = canonical_params(20);
    for &a in &params {
        r.set(P1, a, 0.5);
    }
    r.go_to_page(FILTER.id);
    r.passes(2);
    let ev: Vec<_> = params
        .iter()
        .map(|&a| r.nrpn(P1, addr_of(a), DataValue::Coarse(u7(0))))
        .collect();
    let was = r.stored(P1, CUTOFF);
    r.pass(&ev);
    assert_eq!(r.stored(P1, CUTOFF), was, "waiting");
    r.pass_with(&[], Some(Key::turn(EncoderId::F, 3)));
    r.passes(5);
    let turned = r.stored(P1, CUTOFF);
    assert!(turned > was, "the hand moved it up");
    r.passes(200);
    assert_eq!(r.stored(P1, CUTOFF), turned, "the hand won");
}

#[test]
fn canonical_nrpn_dead_engine_ignored() {
    let mut r = Rig::algo();
    let rev = r.ui.project().rev();
    let before = r.ui.project().part(P1).sound.clone();
    let e = r.nrpn(P1, addr_of(STRUCTURE), DataValue::Coarse(u7(0)));
    r.pass(&[e]);
    r.passes(20);
    assert!(r.ui.project().part(P1).sound.bits_eq(&before));
    assert_eq!(r.ui.project().rev(), rev, "MODAL's block is dead on ALGO");
}

#[test]
fn explicit_binding_beats_canonical() {
    let mut r = Rig::algo();
    r.set(P1, CUTOFF, 0.5);
    r.set(P1, RESO, 0.5);
    let n = addr_of(CUTOFF);
    r.bind(P1, Input::Nrpn(n, NrpnKind::Absolute(Takeover::Jump)), RESO);
    let cut = r.stored(P1, CUTOFF);
    let e = r.nrpn(P1, n, DataValue::Coarse(u7(127)));
    r.pass(&[e]);
    r.passes(40);
    assert_eq!(r.place(P1, RESO), 1.0, "the binding's target moved");
    assert_eq!(r.stored(P1, CUTOFF), cut, "the canonical one didn't");
}

#[test]
fn rpn0_sets_bend_clamped() {
    let mut r = Rig::algo();
    let ch = r.ch(P1);
    r.ui.project_mut().edit_part(P2).mix.channel = ch;
    let rpn = |c| ev(ch, c);
    let data = |m| {
        rpn(Ctl::Data {
            sel: Sel::Rpn(U14::new(0).unwrap()),
            value: DataValue::Coarse(u7(m)),
        })
    };
    let step = |dir| {
        rpn(Ctl::Step {
            sel: Sel::Rpn(U14::new(0).unwrap()),
            dir,
        })
    };
    let bend = |r: &Rig, p| r.stored(p, BEND);
    r.pass(&[data(30)]);
    assert_eq!(bend(&r, P1), 24.0, "clamped to two octaves");
    assert_eq!(bend(&r, P2), 24.0, "every Part on the channel");
    r.pass(&[data(5)]);
    assert_eq!(bend(&r, P1), 5.0);
    r.pass(&[step(Dir::Up)]);
    assert_eq!(bend(&r, P1), 6.0);
    r.pass(&[step(Dir::Down), step(Dir::Down)]);
    assert_eq!(bend(&r, P2), 4.0);
    // RPN 1 (fine tune) is ignored in Phase 1.
    let rev = r.ui.project().rev();
    r.pass(&[rpn(Ctl::Data {
        sel: Sel::Rpn(U14::new(1).unwrap()),
        value: DataValue::Coarse(u7(70)),
    })]);
    assert_eq!(r.ui.project().rev(), rev);
}

const MORPH: ParamAddr = ParamAddr::new(
    BlockRef::Algo,
    chimera_core::dsp::algo::params::AlgoParams::MORPH,
);

/// Stepped, ≤ 128 values: a canonical NRPN's coarse byte is the step
/// (Index7); a 14-bit pair bins by its MSB (#820).
#[test]
fn stepped_targets_index_and_bin() {
    let mut r = Rig::algo();
    let s = spec(MORPH);
    let e = r.nrpn(P1, addr_of(MORPH), DataValue::Coarse(u7(5)));
    r.pass(&[e]);
    r.passes(5);
    assert_eq!(r.stored(P1, MORPH), s.min + 5.0, "Index7: step 5");

    let m = Cc14Msb::new(2).unwrap();
    r.bind(P1, Input::Cc14(m, Takeover::Jump), MORPH);
    let e = r.cc(P1, 2, 127);
    r.pass(&[e]);
    r.passes(5);
    assert_eq!(r.stored(P1, MORPH), s.max, "the MSB's top bin");
}

/// An NRPN bound relative turns as detents; one bound IncDec steps on
/// CC 96 / 97 (#820).
#[test]
fn nrpn_relative_and_inc_dec_turn() {
    for kind in [
        NrpnKind::Relative(ByteEncoding::BinaryOffset),
        NrpnKind::IncDec,
    ] {
        let mut r = Rig::algo();
        r.set(P1, CUTOFF, 0.5);
        r.passes(2);
        let n = addr_of(CUTOFF);
        r.bind(P1, Input::Nrpn(n, kind), CUTOFF);
        let start = r.stored(P1, CUTOFF);
        let up = if kind == NrpnKind::IncDec {
            ev(
                r.ch(P1),
                Ctl::Step {
                    sel: Sel::Nrpn(n),
                    dir: Dir::Up,
                },
            )
        } else {
            r.nrpn(P1, n, DataValue::Coarse(u7(0x41)))
        };
        for _ in 0..5 {
            r.pass(&[up]);
        }
        r.passes(5);
        assert!(r.stored(P1, CUTOFF) > start, "{kind:?} turned it up");
    }
}

/// After a ramp lands on CUTOFF (Octaves: its stored value reads back a
/// hair off the place it landed on), a ±1 reversal inside `DEAD_MS` is
/// still jitter (#873 review).
#[test]
fn jitter_after_landing_is_still_rejected() {
    let mut bad = Vec::new();
    for v in 1..126u8 {
        let mut r = Rig::algo();
        r.bind(P1, cc_input(20, CcKind::Absolute(Takeover::Jump)), CUTOFF);
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
        r.passes(60);
        let e = r.cc(P1, 20, v + 1);
        r.pass(&[e]);
        let e = r.cc(P1, 20, v);
        r.pass(&[e]);
        r.passes(60);
        if (r.place(P1, CUTOFF) - place7(v + 1)).abs() > 1e-3 {
            bad.push(v);
        }
    }
    assert!(
        bad.is_empty(),
        "v → v+1 → v inside DEAD_MS let back through: {bad:?}"
    );
}

/// Moved by something else, a key heard before moves it again (#820).
#[test]
fn a_moved_param_hears_a_key_again() {
    let mut r = Rig::algo();
    r.bind(P1, cc_input(20, CcKind::Absolute(Takeover::Jump)), CUTOFF);
    let e = r.cc(P1, 20, 64);
    r.pass(&[e]);
    r.passes(60);
    r.set(P1, CUTOFF, 0.9);
    r.passes(2);
    let e = r.cc(P1, 20, 64);
    r.pass(&[e]);
    r.passes(60);
    assert!(
        (r.place(P1, CUTOFF) - place7(64)).abs() < 1e-3,
        "{}",
        r.place(P1, CUTOFF)
    );
}
