// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A pad's MACHINE can be NONE (ADR 0318): no machine on the pad. Its
//! code is 13, never reused; MACHINE lists it last, so every machine
//! keeps its value. A NONE pad drops out of its kit's bill, a strike on
//! it takes no voice and steals none, and its other cells are inert.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

mod common;

use std::sync::{LazyLock, Mutex, MutexGuard};

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::drum::groups::VoiceGroups;
use chimera_core::dsp::drum::params::PadParams;
use chimera_core::dsp::drum::sound::{
    Amount, KitBill, PadRoute, RouteDest, RouteId, RouteSource, kit_bill,
};
use chimera_core::dsp::drum::types::{
    ChokeGroup, DrumHit, Kit, Machine, Pad, PadId, PadMachine, Tonal,
};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::hw::MAX_VOICES;
use chimera_core::midi_map::Mappable;
use chimera_core::nrpn::NrpnAddr;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::project::{PartFrom, PartId, PartSource};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{
    DRUM_AMP, DRUM_FLT, DRUM_GATE, DRUM_KEYS, DRUM_KIT, DRUM_MCH, DRUM_MORE, DRUM_PENV, DRUM_PLFO,
    DRUM_ROUTE,
};
use chimera_core::ui::kit_band::{self, Family, Instrument, Tone};
use chimera_core::ui::view::{self, View};
use common::InstRig;
use common::drum::{PLAY_BUDGET, drum_hits, drum_rig};

static WRITER: LazyLock<Mutex<KitWriter>> =
    LazyLock::new(|| Mutex::new(KitWriter::take().expect("the kit writer, once")));

const P1: PartId = PartId::ALL[0];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).expect("pads 1–12")
}

fn machine_addr() -> ParamAddr {
    ParamAddr::new(BlockRef::Pad, PadParams::MACHINE)
}

/// `Kit::DEFAULT` with pad `n` on NONE.
fn none_on(n: u8) -> Kit {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(n)).set_machine(PadMachine::None);
    k
}

// ---- codes and values ----

/// NONE's code is the next free one, 13; every machine keeps its code and
/// its MACHINE value, NONE's value past theirs.
#[test]
fn none_is_code_13_and_listed_last() {
    assert_eq!(PadMachine::None.code(), 13);
    assert_eq!(PadMachine::None.disk_code(), 13);
    assert_eq!(PadMachine::None.disk_ident(), "NONE");
    assert_eq!(PadMachine::from_disk_code(13), Some(PadMachine::None));
    assert_eq!(PadMachine::ALL.len(), Machine::ALL.len() + 1);
    for (i, m) in Machine::ALL.into_iter().enumerate() {
        let pm = PadMachine::from(m);
        assert_eq!(PadMachine::ALL[i], pm);
        assert_eq!(pm.index(), m.index(), "{m:?} keeps its value");
        assert_eq!(pm.code(), m.code(), "{m:?} keeps its code");
        assert_eq!(pm.disk_ident(), m.disk_ident());
        assert_eq!(PadMachine::from_disk_code(m.code()), Some(pm));
        assert_eq!(pm.machine(), Some(m));
    }
    assert_eq!(PadMachine::None.index(), Machine::ALL.len());
    assert_eq!(*PadMachine::ALL.last().unwrap(), PadMachine::None);
    assert_eq!(PadMachine::None.machine(), None);
    for (i, m) in PadMachine::ALL.into_iter().enumerate() {
        assert_eq!(m.index(), i);
        assert_eq!(PadMachine::from_index(i), Some(m));
    }
    assert_eq!(PadMachine::from_index(PadMachine::ALL.len()), None);
    assert_eq!(PadMachine::from_disk_code(0), None);
    assert_eq!(PadMachine::from_disk_code(14), None);
}

/// A NONE pad's bytes carry code 13 and read back NONE, its other bytes
/// kept.
#[test]
fn a_none_pad_round_trips_its_bytes() {
    let mut p = *Kit::DEFAULT.pad(pad(3));
    p.set_machine(PadMachine::None);
    p.set_level(chimera_midi::value::U7::new(90).unwrap());
    let b = p.to_bytes();
    assert_eq!(b[0], 13);
    let back = Pad::from_bytes(b, &Kit::default_pad(pad(3)));
    assert_eq!(back.machine(), PadMachine::None);
    assert_eq!(back, p);
    assert_eq!(back.to_bytes(), b);
}

/// A kit from before NONE loads exactly as it did: every machine's code
/// is itself, and a code this firmware doesn't know is still the pad's
/// default.
#[test]
fn an_old_kit_loads_as_before() {
    for p in PadId::ALL {
        let d = Kit::default_pad(p);
        let b = d.to_bytes();
        assert!((1..=12).contains(&b[0]), "{p:?}");
        assert_eq!(Pad::from_bytes(b, &d), d, "{p:?}");
        let mut unknown = b;
        unknown[0] = 14;
        let got = Pad::from_bytes(unknown, &d);
        assert_eq!(got.machine(), d.machine(), "{p:?}");
        assert_eq!(got.knobs(), d.knobs(), "{p:?}");
    }
}

/// MACHINE's value: NONE at the end of the list, written and read back
/// through the pad's cell; leaving NONE takes the new machine's knobs.
#[test]
fn machine_s_cell_reads_and_writes_none() {
    let mut p = *Kit::DEFAULT.pad(pad(1));
    let none = PadMachine::None.index() as f32;
    let spec = p.spec(PadParams::MACHINE).unwrap();
    assert_eq!(spec.max, none);
    p.write(PadParams::MACHINE, none);
    assert_eq!(p.machine(), PadMachine::None);
    assert_eq!(*p.knobs(), chimera_core::dsp::drum::types::KnobSet::ZERO);
    assert_eq!(p.get(PadParams::MACHINE), none);
    p.write(PadParams::MACHINE, Machine::MetalHat.index() as f32);
    assert_eq!(p.machine(), PadMachine::Machine(Machine::MetalHat));
    assert_eq!(
        *p.knobs(),
        chimera_core::dsp::drum::types::KnobSet::defaults(Machine::MetalHat)
    );
    // Past NONE: nothing written.
    p.write(PadParams::MACHINE, none + 1.0);
    assert_eq!(p.machine(), PadMachine::Machine(Machine::MetalHat));
}

/// Pads are no NRPN, MIDI-map or dump address (a pad is the UI's
/// selection, not a Sound's param): NONE needs no code on those wires.
/// The editor sets it by its option (`console_ed_engines_test`).
#[test]
fn machine_has_no_nrpn_or_map_address() {
    assert_eq!(NrpnAddr::of(machine_addr()), None);
    assert!(Mappable::new(machine_addr()).is_none());
    assert!(!BlockRef::ALL.contains(&BlockRef::Pad));
}

// ---- the bill ----

/// A kit on TOM everywhere, CLEAN, no routes, no gate: the cheap kit.
fn cheap() -> Kit {
    let mut k = Kit::DEFAULT;
    for p in PadId::ALL {
        let d = Kit::default_pad(pad(5));
        *k.pad_mut(p) = d.with_sound(*Kit::DEFAULT.pad(pad(5)).sound());
    }
    k
}

/// `cheap` with pad 7 the costliest there is: JINGLE on LADDER, a CUTOFF
/// route, its gate open.
fn dear() -> Kit {
    let mut k = cheap();
    let p = k.pad_mut(pad(7));
    p.write(PadParams::MACHINE, Machine::MetalJingle.index() as f32);
    let s = p.sound_mut();
    s.filter_mut().set_kind(FilterKind::Ladder);
    s.routes_mut().set(
        RouteId::ALL[0],
        PadRoute::new(
            RouteSource::Lfo,
            RouteDest::Cutoff,
            Amount::new(40).unwrap(),
        ),
    );
    s.amp_mut()
        .set_gate(chimera_core::dsp::lpg::Knobs::of([0.5, 0.0, 0.0]));
    k
}

#[test]
fn a_none_pad_drops_out_of_the_kit_bill() {
    let (cheap, dear) = (cheap(), dear());
    assert_ne!(kit_bill(&dear), kit_bill(&cheap), "pad 7 is dear");
    let mut k = dear;
    k.pad_mut(pad(7)).set_machine(PadMachine::None);
    assert_eq!(kit_bill(&k), kit_bill(&cheap));
}

#[test]
fn an_all_none_kit_bills_as_the_empty_bill() {
    let mut k = dear();
    for p in PadId::ALL {
        k.pad_mut(p).set_machine(PadMachine::None);
    }
    assert_eq!(kit_bill(&k), KitBill::NONE);
}

// ---- strikes ----

fn rig(kit: &Kit) -> (MutexGuard<'static, KitWriter>, InstRig) {
    let mut w = common::drum::locked(&WRITER);
    let r = drum_rig(&mut w, PLAY_BUDGET, &Kit::DEFAULT, kit);
    (w, r)
}

fn hits(r: &InstRig) -> Vec<DrumHit> {
    drum_hits(&r.inst).map(|(_, h)| h).collect()
}

fn part_1_slots(r: &InstRig) -> usize {
    r.inst
        .allocator()
        .slots()
        .iter()
        .filter(|s| s.part() == Some(P1))
        .count()
}

/// The view strikes no hit on a NONE pad.
#[test]
fn the_view_has_no_hit_on_none() {
    let (_w, _r) = rig(&none_on(2));
    let note = |n| chimera_core::MidiNote::new(n).unwrap();
    assert!(KIT_VIEW.hit(P1, note(1)).is_none());
    assert!(KIT_VIEW.hit(P1, note(13)).is_none(), "every octave");
    assert_eq!(KIT_VIEW.hit(P1, note(0)).map(|h| h.pad()), Some(pad(1)));
}

#[test]
fn a_none_strike_takes_no_voice() {
    let (_w, mut r) = rig(&none_on(2));
    r.note_on(0, 1);
    r.render(2);
    assert!(hits(&r).is_empty());
    assert_eq!(part_1_slots(&r), 0);
    assert_eq!(r.inst.sounding(), 0);
    assert_eq!(r.inst.allocator().refused(), 0);
    // TONAL on a NONE pad: C1 up plays nothing either.
    let mut k = none_on(2);
    k.set_tonal(Tonal::Pad(pad(2)));
    drop((_w, r));
    let (_w, mut r) = rig(&k);
    r.note_on(0, 60);
    r.render(2);
    assert!(hits(&r).is_empty());
    assert_eq!(part_1_slots(&r), 0);
}

/// A full pool of STACK kicks: a NONE strike steals none and fades none.
#[test]
fn a_none_strike_never_steals() {
    let mut k = none_on(2);
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    let (_w, mut r) = rig(&k);
    for _ in 0..MAX_VOICES {
        r.note_on(0, 0);
        r.render(1);
    }
    assert_eq!(hits(&r).len(), MAX_VOICES);
    let slots = format!("{:?}", r.inst.allocator().slots());
    r.note_on(0, 1);
    r.render(1);
    assert_eq!(hits(&r).len(), MAX_VOICES);
    assert!(hits(&r).iter().all(|h| h.pad() == pad(1)));
    assert_eq!(
        format!("{:?}", r.inst.allocator().slots()),
        slots,
        "no slot touched"
    );
}

/// A NONE pad chokes nothing: its group's ringing pads ring on.
#[test]
fn a_none_strike_chokes_nothing() {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(9)).set_choke(ChokeGroup::C);
    k.pad_mut(pad(8)).set_machine(PadMachine::None);
    k.pad_mut(pad(8)).set_choke(ChokeGroup::C);
    let (_w, mut r) = rig(&k);
    r.note_on(0, 8);
    r.render(1);
    r.note_on(0, 7);
    r.render(2);
    let h = hits(&r);
    assert_eq!(h.len(), 1, "{h:?}");
    assert_eq!(h[0].pad(), pad(9));
}

/// A hit waiting out a steal whose pad turns NONE before it starts is
/// dropped unheard: the kit as it is now.
#[test]
fn a_waiting_hit_whose_pad_became_none_is_dropped() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    let mut stack = Kit::DEFAULT;
    stack.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    w.publish(PartId::ALL[1], &stack);
    *r.shared.parts[1].params = ParamSnapshot::for_engine(EngineType::Drum);
    r.shared.parts[1].mix.channel = chimera_core::MidiChannel::new(1).unwrap();
    for _ in 0..MAX_VOICES {
        r.note_on(1, 0);
        r.render(1);
    }
    r.note_on(0, 1);
    w.publish(P1, &none_on(2));
    r.render(4);
    assert_eq!(part_1_slots(&r), 0);
}

/// A NONE pad still counts toward the kit's voice groups (ADR 0280):
/// else NONE, a new group, then a machine would take a kit past 8.
#[test]
fn a_none_pad_keeps_its_voice_group() {
    let mut k = Kit::DEFAULT;
    assert_eq!(VoiceGroups::of(&k), VoiceGroups::MAX);
    k.pad_mut(pad(3)).set_machine(PadMachine::None);
    assert_eq!(VoiceGroups::of(&k), VoiceGroups::MAX);
    // Pad 3 out of A (with pad 4): a 9th group, held as on a machine's pad.
    assert!(VoiceGroups::choke(&k, pad(3), ChokeGroup::Off).is_err());
}

/// A live strike on a NONE pad still selects it on the KIT page: the
/// player finds the pad they hit.
#[test]
fn a_live_none_strike_selects_its_pad() {
    let (mut w, mut r) = rig(&none_on(2));
    assert_eq!(w.take_hit(P1), None);
    r.note_on(0, 1);
    assert_eq!(w.take_hit(P1).map(|s| s.last()), Some(pad(2)));
    assert_eq!(part_1_slots(&r), 0);
}

// ---- the UI ----

fn drum_on(n: u8) -> UiState {
    let mut ui = UiState::new();
    ui.load_for_test(PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Drum),
    });
    ui.select_pad(P1, pad(n));
    ui
}

/// Turns pad `n`'s MACHINE to NONE through its cell.
fn to_none(ui: &mut UiState) {
    assert!(ui.edit_addr(P1, machine_addr(), 100, false));
}

#[test]
fn machine_turns_to_none_and_its_cell_reads_none() {
    let mut ui = drum_on(3);
    to_none(&mut ui);
    assert_eq!(ui.project().kit(P1).pad(pad(3)).machine(), PadMachine::None);
    let ctx = ui.slot_ctx_for_test();
    assert_eq!(ctx.pad, Some(PadMachine::None));
    let v = view::view(&DRUM_KIT, 1, &ctx);
    assert_eq!(v.addr(), Some(machine_addr()));
    assert_eq!(
        chimera_core::ui::renderer::pad_machine_cell(PadMachine::None),
        "NONE"
    );
    assert_eq!(kit_band::name(PadMachine::None), "NONE");
    // Pad 3's other cells are the knobs' empty and the rest inert;
    // MACHINE, the pad picker, TONAL and the Part's FX stay live.
    let s = &ui.project().part(P1).sound;
    for def in [
        &DRUM_KIT,
        &DRUM_KEYS,
        &DRUM_MCH,
        &DRUM_MORE,
        &DRUM_FLT,
        &DRUM_AMP,
        &DRUM_GATE,
        &DRUM_PENV,
        &DRUM_PLFO,
        &DRUM_ROUTE,
    ] {
        for i in 0..def.params.len() {
            let v = view::view(def, i, &ctx);
            let dim = view::is_dimmed(&v, s, &ctx);
            match v.addr() {
                Some(a) if a == machine_addr() => assert!(!dim, "{} {i}", def.short),
                Some(a) if a.block.is_pad() => assert!(dim, "{} {i}: {a:?}", def.short),
                Some(a) => assert!(!dim, "{} {i}: {a:?}", def.short),
                None => {
                    if let SlotKnob::Knob = slot_kind(def, i) {
                        assert_eq!(v, View::Empty, "{} {i}", def.short);
                    }
                }
            }
        }
    }
}

enum SlotKnob {
    Knob,
    Other,
}

fn slot_kind(def: &chimera_core::ui::block_def::BlockDef, i: usize) -> SlotKnob {
    match def.params[i].binding {
        chimera_core::ui::block_def::SlotBinding::Knob(_) => SlotKnob::Knob,
        _ => SlotKnob::Other,
    }
}

/// A machine's pad keeps every cell live: nothing above dims a pad that
/// plays.
#[test]
fn a_machine_pad_dims_nothing_of_its_own() {
    let ui = drum_on(3);
    let ctx = ui.slot_ctx_for_test();
    let s = &ui.project().part(P1).sound;
    for i in 1..4 {
        let v = view::view(&DRUM_KIT, i, &ctx);
        assert!(!view::is_dimmed(&v, s, &ctx), "KIT {i}");
    }
}

/// TONAL on a NONE pad: its band says nothing plays from C1 up.
#[test]
fn tonal_on_a_none_pad_says_it_plays_nothing() {
    use chimera_core::dsp::drum::params::KitParams;
    let a = ParamAddr::new(BlockRef::Kit, KitParams::TONAL);
    let mut ui = drum_on(1);
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(2)));
    ui.select_pad(P1, pad(2));
    to_none(&mut ui);
    let note = view::tonal_note(a, &ui.slot_ctx_for_test());
    assert_eq!(note.unwrap().as_str(), "NONE PLAYS NOTHING");
}

// ---- the glyph ----

/// Every MACHINE value has its own mark; NONE's (bank 2's, `ui::notes`)
/// is a shaded box with an X in the middle and no family badge, drawn
/// in the icon's mark column.
#[test]
fn every_machine_value_has_its_own_glyph() {
    use chimera_core::ui::notes::BLANK;
    let marks: Vec<_> = Machine::ALL
        .into_iter()
        .map(|m| *kit_band::mark(kit_band::instrument(m)))
        .chain([BLANK])
        .collect();
    for (a, x) in marks.iter().enumerate() {
        for y in &marks[a + 1..] {
            assert_ne!(x, y);
        }
    }
    for m in PadMachine::ALL {
        match m {
            PadMachine::Machine(m) => {
                let _: Instrument = kit_band::instrument(m);
                let _: Family = kit_band::family(m);
            }
            PadMachine::None => {}
        }
    }
    let (w, h) = (kit_band::MARK_W, kit_band::MARK_H);
    // The box's edges full, its inside shaded, the X full through the
    // middle.
    let (l, r, t, b) = (1, w - 2, 1, h - 2);
    for x in l..=r {
        assert_eq!(BLANK.at(x, t), Tone::Full, "top {x}");
        assert_eq!(BLANK.at(x, b), Tone::Full, "bottom {x}");
    }
    for y in t..=b {
        assert_eq!(BLANK.at(l, y), Tone::Full, "left {y}");
        assert_eq!(BLANK.at(r, y), Tone::Full, "right {y}");
    }
    let (cx, cy) = (w / 2, h / 2);
    for (x, y) in [(cx - 1, cy - 1), (cx, cy), (cx - 1, cy), (cx, cy - 1)] {
        assert_eq!(BLANK.at(x, y), Tone::Full, "centre {x},{y}");
    }
    assert_eq!(BLANK.at(l + 1, t + 1), Tone::Dim, "shaded");
    // The icon draws it in the mark's column, nothing in the badge's.
    let mut fb = Fb::default();
    let s = kit_band::Scale::ONE;
    let ink = chimera_core::ui::theme::ACCENT;
    kit_band::icon(&mut fb, PadMachine::None, 0, 0, s, ink, ink);
    for y in 0..h {
        for x in 0..kit_band::ICON_W {
            let inked = fb.at(x, y).is_some();
            let want = x >= kit_band::MARK_X && BLANK.at(x - kit_band::MARK_X, y) != Tone::None;
            assert_eq!(inked, want, "{x},{y}");
        }
    }
}

/// A small frame for one icon: the pixels drawn.
#[derive(Default)]
struct Fb(std::collections::HashMap<(i32, i32), embedded_graphics::pixelcolor::Rgb565>);

impl Fb {
    fn at(&self, x: i32, y: i32) -> Option<embedded_graphics::pixelcolor::Rgb565> {
        self.0.get(&(x, y)).copied()
    }
}

impl embedded_graphics::geometry::OriginDimensions for Fb {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(240, 320)
    }
}

impl embedded_graphics::draw_target::DrawTarget for Fb {
    type Color = embedded_graphics::pixelcolor::Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        for embedded_graphics::Pixel(p, c) in pixels {
            self.0.insert((p.x, p.y), c);
        }
        Ok(())
    }
}
