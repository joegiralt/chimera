//! A pad as the UI edits it (ADR 0265): `BlockRef::Pad`, the Part's
//! selected pad. Not a Sound's: the kit record stores it, and a hit reads
//! it whole, so nothing glides.

use chimera_midi::value::U7;

use crate::block::{Block, Change, Motion, ParamId, ParamSpec, Smoothing, ValFmt};
use crate::dsp::tape_engine::params::Retrig;

use super::laws::spec;
use super::types::{ChokeGroup, Kit, Knob, KnobSet, KnobValue, Machine, Pad, PadId, Pan, Tonal};

/// `PAD_SPECS`' ids.
pub enum PadParams {}

impl PadParams {
    pub const MACHINE: ParamId = ParamId(0);
    pub const LEVEL: ParamId = ParamId(1);
    pub const PAN: ParamId = ParamId(2);
    pub const CHOKE: ParamId = ParamId(3);
    pub const RETRIG: ParamId = ParamId(4);
    pub const KNOB_A: ParamId = ParamId(5);
    pub const KNOB_B: ParamId = ParamId(6);
    pub const KNOB_C: ParamId = ParamId(7);
    pub const KNOB_D: ParamId = ParamId(8);
    pub const KNOB_E: ParamId = ParamId(9);
    pub const KNOB_F: ParamId = ParamId(10);
    pub const KNOB_G: ParamId = ParamId(11);
    pub const KNOB_H: ParamId = ParamId(12);

    /// Knob `k`'s id.
    pub const fn knob(k: Knob) -> ParamId {
        ParamId(Self::KNOB_A.0 + k as u8)
    }

    /// The knob `id` is, if it is one.
    pub const fn knob_of(id: ParamId) -> Option<Knob> {
        match id.0.checked_sub(Self::KNOB_A.0) {
            Some(i) if (i as usize) < Knob::ALL.len() => Some(Knob::ALL[i as usize]),
            _ => None,
        }
    }

    /// `id`'s default on a pad of machine `m`, where it is the machine's
    /// own: a knob's, which `PAD_SPECS` can't hold. `None` for the rest,
    /// whose spec default stands.
    pub fn default_on(m: Machine, id: ParamId) -> Option<f32> {
        Self::knob_of(id).map(|k| f32::from(KnobSet::defaults(m).get(k).get().get()))
    }
}

/// MACHINE's names, in `Machine::ALL`'s order.
static MACHINE_NAMES: [&str; Machine::ALL.len()] = {
    let mut n = [""; Machine::ALL.len()];
    let mut i = 0;
    while i < n.len() {
        n[i] = spec(Machine::ALL[i]).name;
        i += 1;
    }
    n
};

const CHOKE_NAMES: [&str; 5] = ["OFF", "A", "B", "C", "D"];
const RETRIG_NAMES: [&str; 2] = ["STACK", "CUT"];

/// A knob: its stored position. Label, short and readout are the pad's
/// machine's (`laws::spec`), which the UI's cell shows.
const fn knob(k: Knob, label: &'static str, ident: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        PadParams::knob(k).0,
        label,
        ValFmt::Uni,
        0.0,
        127.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::NoteOn),
    )
    .ident(ident)
}

/// Each read at the hit: a change is heard on the next.
pub static PAD_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(
        PadParams::MACHINE.0,
        "MACHINE",
        ValFmt::Names(&MACHINE_NAMES),
        (Machine::ALL.len() - 1) as f32,
        0.0,
        Change::NoteOn,
    )
    .short("MCH")
    .ident("MACHINE")
    .glyph(crate::glyph::FocusGlyph::Drum),
    ParamSpec::stepped(
        PadParams::LEVEL.0,
        "LEVEL",
        ValFmt::Uni,
        0.0,
        127.0,
        127.0,
        false,
        Motion::Continuous(Smoothing::NoteOn),
    )
    .short("LVL")
    .ident("LEVEL"),
    ParamSpec::stepped(
        PadParams::PAN.0,
        "PAN",
        ValFmt::Pan,
        -64.0,
        63.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::NoteOn),
    )
    .ident("PAN"),
    ParamSpec::choice(
        PadParams::CHOKE.0,
        "CHOKE",
        ValFmt::Names(&CHOKE_NAMES),
        4.0,
        0.0,
        Change::NoteOn,
    )
    .short("CHK")
    .ident("CHOKE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        PadParams::RETRIG.0,
        "RETRIG",
        ValFmt::Names(&RETRIG_NAMES),
        1.0,
        1.0,
        Change::NoteOn,
    )
    .short("RTG")
    .ident("RETRIG")
    .glyph(crate::glyph::FocusGlyph::None),
    knob(Knob::A, "A", "KNOB_A"),
    knob(Knob::B, "B", "KNOB_B"),
    knob(Knob::C, "C", "KNOB_C"),
    knob(Knob::D, "D", "KNOB_D"),
    knob(Knob::E, "E", "KNOB_E"),
    knob(Knob::F, "F", "KNOB_F"),
    knob(Knob::G, "G", "KNOB_G"),
    knob(Knob::H, "H", "KNOB_H"),
];

const _: () = assert!(CHOKE_NAMES.len() == ChokeGroup::ALL.len());
const _: () = assert!(RETRIG_NAMES.len() == Retrig::ALL.len());

impl Block for Pad {
    fn specs(&self) -> &'static [ParamSpec] {
        &PAD_SPECS
    }

    #[inline(never)]
    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        if let Some(k) = PadParams::knob_of(id) {
            return f32::from(self.knobs().get(k).get().get());
        }
        match id {
            PadParams::MACHINE => self.machine().index() as f32,
            PadParams::LEVEL => f32::from(self.level().get()),
            PadParams::PAN => f32::from(self.pan().get()),
            PadParams::CHOKE => self.choke().index() as f32,
            PadParams::RETRIG => self.retrig().index() as f32,
            _ => 0.0,
        }
    }

    /// A new MACHINE takes its own knob defaults; a knob the machine
    /// lacks is never written.
    #[inline(never)]
    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        let u7 = |v: f32| U7::new(v as u8).unwrap_or(U7::MAX);
        if let Some(k) = PadParams::knob_of(id) {
            if spec(self.machine()).knob(k).is_some() {
                self.knobs_mut().set(k, KnobValue::new(u7(v)));
            }
            return;
        }
        match id {
            PadParams::MACHINE => {
                if let Some(&m) = Machine::ALL.get(v as usize)
                    && m != self.machine()
                {
                    self.set_machine(m);
                    *self.knobs_mut() = KnobSet::defaults(m);
                }
            }
            PadParams::LEVEL => self.set_level(u7(v)),
            PadParams::PAN => self.set_pan(Pan::clamped(v as i8)),
            PadParams::CHOKE => {
                if let Some(c) = ChokeGroup::from_index(v as usize) {
                    self.set_choke(c);
                }
            }
            PadParams::RETRIG => {
                if let Some(r) = Retrig::from_index(v as usize) {
                    self.set_retrig(r);
                }
            }
            _ => {}
        }
    }
}

/// `KIT_SPECS`' ids: the kit's own params, not a pad's (`BlockRef::Kit`).
pub enum KitParams {}

impl KitParams {
    pub const TONAL: ParamId = ParamId(0);
}

/// TONAL's names, by code (`Tonal::code`): OFF, then the pads as PAD
/// shows them.
const TONAL_NAMES: [&str; PadId::ALL.len() + 1] = [
    "OFF", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

/// TONAL (ADR 0270): which pad C1 up plays, tuned at middle C; OFF
/// repeats the kit every octave.
pub static KIT_SPECS: [ParamSpec; 1] = [ParamSpec::choice(
    KitParams::TONAL.0,
    "TONAL",
    ValFmt::Names(&TONAL_NAMES),
    PadId::ALL.len() as f32,
    0.0,
    Change::NoteOn,
)
.ident("TONAL")
.glyph(crate::glyph::FocusGlyph::None)];

impl Block for Kit {
    fn specs(&self) -> &'static [ParamSpec] {
        &KIT_SPECS
    }

    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            KitParams::TONAL => f32::from(self.tonal().code()),
            _ => 0.0,
        }
    }

    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        if id == KitParams::TONAL
            && let Some(t) = Tonal::from_code(v as u8)
        {
            self.set_tonal(t);
        }
    }
}
