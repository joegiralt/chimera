//! DRUM's pad cells (ADR 0265): `BlockRef::Pad` edits the Part's selected
//! pad, and a knob cell takes its label, short and readout from that pad's
//! machine, empty where the machine has no such knob.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::drum::laws::spec;
use chimera_core::dsp::drum::params::PadParams;
use chimera_core::dsp::drum::types::{ChokeGroup, Kit, Knob, KnobSet, Machine, PadId, Tonal};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::VizType;
use chimera_core::ui::block_def::{BlockDef, ParamSlot};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::page::{PageLayout, ValFmt};
use chimera_core::ui::view::{self, View};
use chimera_midi::value::U7;

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).unwrap()
}

/// A page of MCH's six knobs (Task 8 builds the real one).
static KNOBS: BlockDef = BlockDef {
    id: 9_999,
    name: "Knobs",
    short: "KNB",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::knob(Knob::A),
        ParamSlot::knob(Knob::B),
        ParamSlot::knob(Knob::C),
        ParamSlot::knob(Knob::D),
        ParamSlot::knob(Knob::E),
        ParamSlot::knob(Knob::F),
    ],
};

fn knob_addr(k: Knob) -> ParamAddr {
    ParamAddr::new(BlockRef::Pad, PadParams::knob(k))
}

fn machine_addr() -> ParamAddr {
    ParamAddr::new(BlockRef::Pad, PadParams::MACHINE)
}

/// A UI on Part 1 with pad `n` selected.
fn ui_on(n: u8) -> UiState {
    let mut ui = UiState::new();
    ui.select_pad(P1, pad(n));
    ui
}

fn cell(ui: &UiState, k: Knob) -> View {
    view::view(&KNOBS, k as usize, &ui.slot_ctx_for_test())
}

/// Turns MACHINE until pad `n` of Part 1 plays `m`.
fn set_machine(ui: &mut UiState, m: Machine) {
    let i = Machine::ALL.iter().position(|&x| x == m).unwrap() as i8;
    let now = ui.project().kit(P1).pad(ui.selected_pad(P1)).machine();
    let at = Machine::ALL.iter().position(|&x| x == now).unwrap() as i8;
    ui.edit_addr(P1, machine_addr(), i - at, false);
}

#[test]
fn the_snare_s_third_knob_reads_bump() {
    let ui = ui_on(2);
    assert_eq!(ui.selected_pad(P1), pad(2));
    assert_eq!(
        ui.project().kit(P1).pad(pad(2)).machine(),
        Machine::ToneSnare
    );
    let v = cell(&ui, Knob::C);
    assert_eq!(v.label(), "BUMP");
    assert_eq!(v.short(), Some("BUMP"));
    assert_eq!(v.addr(), Some(knob_addr(Knob::C)));
    assert_eq!(v.fmt(), ValFmt::Knob(Machine::ToneSnare, Knob::C));
    assert_eq!(cell(&ui, Knob::E).label(), "SNAP DECAY");
    assert_eq!(cell(&ui, Knob::E).short(), Some("SDEC"));
}

#[test]
fn a_plain_noise_has_no_third_knob_to_turn() {
    let mut ui = ui_on(2);
    set_machine(&mut ui, Machine::PlainNoise);
    let p = *ui.project().kit(P1).pad(pad(2));
    assert_eq!(p.machine(), Machine::PlainNoise);
    assert_eq!(cell(&ui, Knob::A).label(), "DECAY");
    for k in [Knob::C, Knob::D, Knob::E, Knob::F] {
        assert_eq!(cell(&ui, k), View::Empty, "{k:?}");
    }
    // Nor through its address: the machine reads no knob C.
    assert!(!ui.edit_addr(P1, knob_addr(Knob::C), 5, false));
    assert_eq!(*ui.project().kit(P1).pad(pad(2)), p);
}

#[test]
fn a_knob_turn_moves_only_the_selected_pad() {
    let mut ui = ui_on(2);
    let before = *ui.project().kit(P1);
    let other = *ui.project().kit(P2);
    assert!(ui.edit_addr(P1, knob_addr(Knob::C), 3, false));
    let after = ui.project().kit(P1);
    for p in PadId::ALL {
        if p == pad(2) {
            let (b, a) = (before.pad(p).knobs(), after.pad(p).knobs());
            assert_eq!(a.get(Knob::C).get().get(), b.get(Knob::C).get().get() + 3);
            for k in Knob::ALL.into_iter().filter(|&k| k != Knob::C) {
                assert_eq!(a.get(k), b.get(k), "{k:?}");
            }
        } else {
            assert_eq!(after.pad(p), before.pad(p), "{p:?}");
        }
    }
    assert_eq!(*ui.project().kit(P2), other);
}

#[test]
fn a_machine_change_resets_that_pad_s_knobs() {
    let mut ui = ui_on(5);
    ui.edit_addr(P1, knob_addr(Knob::A), 9, false);
    set_machine(&mut ui, Machine::MetalHat);
    let p = ui.project().kit(P1).pad(pad(5));
    assert_eq!(p.machine(), Machine::MetalHat);
    assert_eq!(*p.knobs(), KnobSet::defaults(Machine::MetalHat));
    // Its other settings stay.
    assert_eq!(p.choke(), Kit::DEFAULT.pad(pad(5)).choke());
}

#[test]
fn level_pan_choke_and_retrig_edit_the_selected_pad() {
    let mut ui = ui_on(4);
    let at = |id| ParamAddr::new(BlockRef::Pad, id);
    ui.edit_addr(P1, at(PadParams::LEVEL), -27, false);
    ui.edit_addr(P1, at(PadParams::PAN), -10, false);
    ui.edit_addr(P1, at(PadParams::CHOKE), 2, false);
    ui.edit_addr(P1, at(PadParams::RETRIG), -1, false);
    let p = ui.project().kit(P1).pad(pad(4));
    assert_eq!(p.level(), U7::new(100).unwrap());
    assert_eq!(p.pan().get(), -10);
    assert_eq!(p.choke(), ChokeGroup::C);
    assert_eq!(
        p.retrig(),
        chimera_core::dsp::tape_engine::params::Retrig::Stack
    );
}

#[test]
fn pad_params_are_the_ui_s_not_a_sound_s() {
    assert!(!BlockRef::Pad.in_sound());
    assert!(!BlockRef::Pad.voice_reads());
    assert!(!BlockRef::ALL.contains(&BlockRef::Pad));
    assert!(!knob_addr(Knob::A).modulatable());
}

#[test]
fn a_knob_reads_in_its_machine_s_unit() {
    let show = |m: Machine, k: Knob, v: u8| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, f32::from(v) / 127.0, ValFmt::Knob(m, k));
        b.as_str().to_owned()
    };
    // The law's value, in its fmt: expo 100..400 Hz at 0 and 127.
    assert_eq!(show(Machine::ToneSnare, Knob::A, 0), "100 Hz");
    assert_eq!(show(Machine::ToneSnare, Knob::A, 127), "400 Hz");
    assert_eq!(show(Machine::ToneSnare, Knob::B, 0), "40 ms");
    assert_eq!(show(Machine::ToneKick, Knob::B, 127), "3.00 s");
    assert_eq!(show(Machine::ToneSnare, Knob::C, 127), "+12.0");
    assert_eq!(show(Machine::ToneClap, Knob::C, 127), "8");
    assert_eq!(show(Machine::ToneKick, Knob::F, 127), "100");
    // A knob the machine lacks reads nothing.
    assert_eq!(show(Machine::PlainNoise, Knob::C, 64), "");
    assert!(spec(Machine::PlainNoise).knob(Knob::C).is_none());
}

/// ADR 0270: the kit's TONAL reaches the audio; the selected pad never
/// decides what a note plays.
#[test]
fn tonal_reaches_the_audio() {
    let mut w = KitWriter::take().expect("the kit writer, once");
    let mut ui = ui_on(7);
    ui.select_pad(P2, pad(11));
    ui.project_mut().kit_mut(P2).set_tonal(Tonal::Pad(pad(4)));
    w.publish_all(ui.project().kits());
    let c4 = chimera_core::MidiNote::new(60).unwrap();
    assert_eq!(KIT_VIEW.hit(P1, c4).pad(), pad(1), "OFF: note 60 is pad 1");
    assert_eq!(KIT_VIEW.hit(P2, c4).pad(), pad(4));
}

/// CHOKE and RETRIG: every choice reads back as its own cell value, writes
/// back to itself, and shows its own name.
#[test]
fn choke_and_retrig_cells_round_trip() {
    use chimera_core::block::{Block, DiskCode};
    use chimera_core::dsp::tape_engine::params::Retrig;
    fn shown(p: &chimera_core::dsp::drum::types::Pad, id: chimera_core::block::ParamId) -> String {
        let s = p.specs().iter().find(|s| s.id == id).unwrap();
        let ValFmt::Names(n) = s.fmt else {
            panic!("{} not names", s.label)
        };
        n[p.get(id) as usize].to_owned()
    }
    let base = *Kit::DEFAULT.pad(pad(1));
    for c in ChokeGroup::ALL {
        let mut p = base;
        p.set_choke(c);
        let mut q = base;
        q.write(PadParams::CHOKE, p.get(PadParams::CHOKE));
        assert_eq!(q.choke(), c);
        assert_eq!(shown(&p, PadParams::CHOKE), c.disk_ident());
    }
    for r in Retrig::ALL {
        let mut p = base;
        p.set_retrig(r);
        let mut q = base;
        q.write(PadParams::RETRIG, p.get(PadParams::RETRIG));
        assert_eq!(q.retrig(), r);
        assert_eq!(shown(&p, PadParams::RETRIG), r.disk_ident());
    }
}

/// A pad's MACHINE cell (owner rule: full words when they fit): the full
/// name, else its last word (the instrument), else the machine's short
/// name; never wider than the column, so COWBELL can't run into LEVEL.
#[test]
fn machine_cells_fit_their_column() {
    use chimera_core::ui::renderer::{CELL_VALUE_W, pad_machine_cell};
    use chimera_core::ui::{draw, theme};
    let w = |s: &str| draw::text_width(&theme::FONT_VALUE, s, 0);
    for m in Machine::ALL {
        let s = spec(m);
        let last = s.name.rsplit(' ').next().unwrap();
        let cell = pad_machine_cell(m);
        assert!(w(cell) <= CELL_VALUE_W, "{m:?}: {cell} is {} wide", w(cell));
        let want = if w(s.name) <= CELL_VALUE_W {
            s.name
        } else if w(last) <= CELL_VALUE_W {
            last
        } else {
            s.short
        };
        assert_eq!(cell, want, "{m:?}");
    }
    assert_eq!(pad_machine_cell(Machine::ToneCowbell), "BELL");
}

/// Machines with no PITCH play the same hit on every TONAL note: exactly
/// those `transposed` leaves alone.
#[test]
fn pitched_is_what_transposes() {
    use chimera_core::dsp::drum::laws::resolve;
    use chimera_core::dsp::drum::types::Transpose;
    let up = Transpose::new(12).unwrap();
    for m in Machine::ALL {
        let p = resolve(m, &KnobSet::defaults(m));
        assert_eq!(m.pitched(), p.transposed(up) != p, "{m:?}");
    }
    assert!(!Machine::ToneClap.pitched());
    assert!(!Machine::BodyShaker.pitched());
    assert!(!Machine::PlainNoise.pitched());
}

/// TONAL's band says so when its pad's machine has no PITCH (ADR 0270).
#[test]
fn tonal_on_an_unpitched_pad_notes_it() {
    use chimera_core::dsp::drum::params::KitParams;
    let a = ParamAddr::new(BlockRef::Kit, KitParams::TONAL);
    let mut ui = ui_on(1);
    let note = |ui: &UiState| view::tonal_note(a, &ui.slot_ctx_for_test());
    assert!(note(&ui).is_none(), "OFF");
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(1)));
    assert!(note(&ui).is_none(), "KICK has PITCH");
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(4)));
    assert_eq!(note(&ui).unwrap().as_str(), "NO PITCH ON CLAP");
    ui.project_mut().kit_mut(P1).set_tonal(Tonal::Pad(pad(12)));
    assert_eq!(note(&ui).unwrap().as_str(), "NO PITCH ON SHAKER");
    let other = ParamAddr::new(BlockRef::Pad, PadParams::MACHINE);
    assert!(view::tonal_note(other, &ui.slot_ctx_for_test()).is_none());
}
