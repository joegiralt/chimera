// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A pad as the UI edits it (ADR 0265): `BlockRef::Pad`, the Part's
//! selected pad, and its sound's blocks (ADR 0269): `BlockRef::PadFilter`,
//! `PadAmp`, `PadEnv`, `PadLfo` and `PadRoute`. Not a Sound's: the kit
//! record stores it, and a hit reads it whole, so nothing glides.

use chimera_midi::value::U7;

use crate::block::{Block, Change, Motion, ParamId, ParamSpec, Smoothing, ValFmt, find_spec};
use crate::dsp::Unit;
use crate::dsp::lfo::LfoShape;
use crate::dsp::modulator::EnvSpeed;
use crate::dsp::tape_engine::params::Retrig;
use crate::params::{ENV_SPECS, EnvParams, FILTER_SPECS, FilterParams, OUT_SPECS, OutParams};

use super::laws::spec;
use super::sound::{
    Amount, AmpMode, EnvDecay, EnvPos, LfoMode, PadAmp, PadEnv, PadFilter, PadLfo, PadRoute,
    RouteDest, RouteSource,
};
use super::types::{
    ChokeGroup, Kit, Knob, KnobSet, KnobValue, Machine, Pad, PadId, PadMachine, Pan, Tonal,
};

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

/// MACHINE's names, in `PadMachine::ALL`'s order: NONE last (ADR 0318).
static MACHINE_NAMES: [&str; PadMachine::ALL.len()] = {
    let mut n = [PadMachine::NONE_NAME; PadMachine::ALL.len()];
    let mut i = 0;
    while i < Machine::ALL.len() {
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
        (PadMachine::ALL.len() - 1) as f32,
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
            if let PadMachine::Machine(m) = self.machine()
                && spec(m).knob(k).is_some()
            {
                self.knobs_mut().set(k, KnobValue::new(u7(v)));
            }
            return;
        }
        match id {
            PadParams::MACHINE => {
                if let Some(m) = PadMachine::from_index(v as usize)
                    && m != self.machine()
                {
                    self.set_machine(m);
                    // NONE reads no knob: all 0, as a machine's unused
                    // ones (ADR 0318).
                    *self.knobs_mut() = match m {
                        PadMachine::Machine(m) => KnobSet::defaults(m),
                        PadMachine::None => KnobSet::ZERO,
                    };
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

/// `v` as a U7, held to 0..=127.
fn u7(v: f32) -> U7 {
    U7::new(v as u8).unwrap_or(U7::MAX)
}

/// `f` as the Sound's filter, the rest of it `FilterParams`' default: its
/// get, write and MODE spec are a pad's.
fn as_filter(f: &PadFilter) -> FilterParams {
    let mut p = FilterParams::default();
    f.apply(&mut p);
    p
}

impl Block for PadFilter {
    /// The Sound filter's own table: `spec` admits only `PadFilter::IDS`.
    fn specs(&self) -> &'static [ParamSpec] {
        &FILTER_SPECS
    }

    /// MODE's range and names are the kind's, as the Sound filter's.
    #[optimize(size)]
    fn spec(&self, id: ParamId) -> Option<&'static ParamSpec> {
        if PadFilter::IDS.contains(&id) {
            as_filter(self).spec(id)
        } else {
            None
        }
    }

    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        if PadFilter::IDS.contains(&id) {
            as_filter(self).get(id)
        } else {
            0.0
        }
    }

    /// The Sound filter's rule: a KIND keeps MODE only if it has it.
    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        if PadFilter::IDS.contains(&id) {
            let mut p = as_filter(self);
            p.write(id, v);
            *self = PadFilter::of(&p);
        }
    }
}

/// `PAD_ENV_SPECS`' ids, and `PAD_AMP_SPECS`' envelope's.
pub enum PadEnvParams {}

impl PadEnvParams {
    pub const ATTACK: ParamId = ParamId(0);
    pub const HOLD: ParamId = ParamId(1);
    pub const DECAY: ParamId = ParamId(2);
    pub const SPEED: ParamId = ParamId(3);
}

/// A stage's position, 0..=127, read at the hit.
const fn stage(id: ParamId, label: &'static str, fmt: ValFmt, default: f32) -> ParamSpec {
    ParamSpec::stepped(
        id.0,
        label,
        fmt,
        0.0,
        127.0,
        default,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident(label)
}

/// ATTACK, HOLD, DECAY (127 holds: `INF`) and SPEED, ENV A's names.
const fn env_specs(decay: f32) -> [ParamSpec; 4] {
    let speed = match find_spec(&ENV_SPECS, EnvParams::SPEED) {
        Some(s) => *s,
        None => panic!("ENV's SPEED"),
    };
    [
        stage(PadEnvParams::ATTACK, "ATTACK", ValFmt::Uni, 0.0),
        stage(PadEnvParams::HOLD, "HOLD", ValFmt::Uni, 0.0),
        stage(PadEnvParams::DECAY, "DECAY", ValFmt::Inf, decay),
        speed.with_id(PadEnvParams::SPEED.0),
    ]
}

/// The mod envelope: `PadSound::DEFAULT`'s decay 64.
pub static PAD_ENV_SPECS: [ParamSpec; 4] = env_specs(64.0);

impl Block for PadEnv {
    fn specs(&self) -> &'static [ParamSpec] {
        &PAD_ENV_SPECS
    }

    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            PadEnvParams::ATTACK => f32::from(self.attack().get().get()),
            PadEnvParams::HOLD => f32::from(self.hold().get().get()),
            PadEnvParams::DECAY => f32::from(self.decay().code().get()),
            PadEnvParams::SPEED => self.speed() as u8 as f32,
            _ => 0.0,
        }
    }

    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            PadEnvParams::ATTACK => self.set_attack(EnvPos::new(u7(v))),
            PadEnvParams::HOLD => self.set_hold(EnvPos::new(u7(v))),
            PadEnvParams::DECAY => self.set_decay(EnvDecay::from(u7(v))),
            PadEnvParams::SPEED => {
                if let Some(&s) = EnvSpeed::ALL.get(v as usize) {
                    self.set_speed(s);
                }
            }
            _ => {}
        }
    }
}

/// `PAD_AMP_SPECS`' ids: the envelope's (`PadEnvParams`), then the Part
/// AMP's VEL, LPG, DECAY and STRIKE, then MODE.
pub enum PadAmpParams {}

impl PadAmpParams {
    pub const ATTACK: ParamId = PadEnvParams::ATTACK;
    pub const HOLD: ParamId = PadEnvParams::HOLD;
    pub const DECAY: ParamId = PadEnvParams::DECAY;
    pub const SPEED: ParamId = PadEnvParams::SPEED;
    pub const VEL: ParamId = ParamId(4);
    pub const LPG: ParamId = ParamId(5);
    pub const GATE_DECAY: ParamId = ParamId(6);
    pub const STRIKE: ParamId = ParamId(7);
    pub const MODE: ParamId = ParamId(8);
}

/// `OUT_SPECS`' entry `id` under the pad AMP's id `to`.
const fn out_spec(id: ParamId, to: ParamId) -> ParamSpec {
    match find_spec(&OUT_SPECS, id) {
        Some(s) => s.with_id(to.0),
        None => panic!("the Part AMP's"),
    }
}

/// AMP MODE's names, by `AmpMode::ALL`: ONE-SHOT outruns its cell.
const AMP_MODE_NAMES: [&str; AmpMode::ALL.len()] = ["1-SHOT", "GATED"];

/// The pad's AMP: its AHD envelope, open by default (`PadEnv::OPEN`), the
/// Part AMP's VEL, LPG, DECAY and STRIKE, and MODE.
pub static PAD_AMP_SPECS: [ParamSpec; 9] = {
    let [a, h, d, sp] = env_specs(127.0);
    [
        a,
        h,
        d,
        sp,
        out_spec(OutParams::VCA_VEL, PadAmpParams::VEL),
        out_spec(OutParams::LPG, PadAmpParams::LPG),
        // Its own ident: the pad's AHD has a DECAY (ADR 0285).
        out_spec(OutParams::DECAY, PadAmpParams::GATE_DECAY).ident("GATE_DECAY"),
        out_spec(OutParams::STRIKE, PadAmpParams::STRIKE),
        ParamSpec::choice(
            PadAmpParams::MODE.0,
            "MODE",
            ValFmt::Names(&AMP_MODE_NAMES),
            (AmpMode::ALL.len() - 1) as f32,
            0.0,
            Change::NoteOn,
        )
        .ident("MODE")
        .glyph(crate::glyph::FocusGlyph::None),
    ]
};

impl Block for PadAmp {
    fn specs(&self) -> &'static [ParamSpec] {
        &PAD_AMP_SPECS
    }

    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        let g = self.gate();
        match id {
            PadAmpParams::VEL => self.vel().get(),
            PadAmpParams::LPG => g.lpg.get(),
            PadAmpParams::GATE_DECAY => g.decay.get(),
            PadAmpParams::STRIKE => g.strike.get(),
            PadAmpParams::MODE => AmpMode::ALL
                .iter()
                .position(|&m| m == self.mode())
                .unwrap_or(0) as f32,
            id => self.env().get(id),
        }
    }

    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        let mut g = self.gate();
        match id {
            PadAmpParams::VEL => self.set_vel(Unit::new(v)),
            PadAmpParams::LPG => g.lpg = Unit::new(v),
            PadAmpParams::GATE_DECAY => g.decay = Unit::new(v),
            PadAmpParams::STRIKE => g.strike = Unit::new(v),
            PadAmpParams::MODE => {
                if let Some(&m) = AmpMode::ALL.get(v as usize) {
                    self.set_mode(m);
                }
            }
            id => self.env_mut().write(id, v),
        }
        self.set_gate(g);
    }
}

/// `PAD_LFO_SPECS`' ids.
pub enum PadLfoParams {}

impl PadLfoParams {
    pub const WAVE: ParamId = ParamId(0);
    pub const SPEED: ParamId = ParamId(1);
    pub const DEPTH: ParamId = ParamId(2);
    pub const PHASE: ParamId = ParamId(3);
    pub const MODE: ParamId = ParamId(4);
}

/// WAVE's names, by `LfoShape`'s value: full words but TRIANGLE, which
/// outruns its cell.
const WAVE_NAMES: [&str; 5] = ["SINE", "TRI", "SAW", "SQUARE", "RANDOM"];
/// The LFO's MODE, by `LfoMode::ALL`: the Part LFO's SYNC words.
const LFO_MODE_NAMES: [&str; LfoMode::ALL.len()] = ["FREE", "RETRIG"];

/// A 0..=127 cell, read at the hit.
const fn level(id: ParamId, label: &'static str, default: f32) -> ParamSpec {
    stage(id, label, ValFmt::Uni, default)
}

/// WAVE, SPEED, DEPTH, PHASE and MODE: `PadLfo::DEFAULT`'s defaults.
pub static PAD_LFO_SPECS: [ParamSpec; 5] = [
    ParamSpec::choice(
        PadLfoParams::WAVE.0,
        "WAVE",
        ValFmt::Names(&WAVE_NAMES),
        (WAVE_NAMES.len() - 1) as f32,
        0.0,
        Change::NoteOn,
    )
    .ident("WAVE")
    .glyph(crate::glyph::FocusGlyph::None),
    level(PadLfoParams::SPEED, "SPEED", 6.0),
    level(PadLfoParams::DEPTH, "DEPTH", 127.0),
    level(PadLfoParams::PHASE, "PHASE", 0.0),
    ParamSpec::choice(
        PadLfoParams::MODE.0,
        "MODE",
        ValFmt::Names(&LFO_MODE_NAMES),
        (LfoMode::ALL.len() - 1) as f32,
        0.0,
        Change::NoteOn,
    )
    .ident("MODE")
    .glyph(crate::glyph::FocusGlyph::None),
];

impl Block for PadLfo {
    fn specs(&self) -> &'static [ParamSpec] {
        &PAD_LFO_SPECS
    }

    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            PadLfoParams::WAVE => self.wave() as u8 as f32,
            PadLfoParams::SPEED => f32::from(self.speed().get()),
            PadLfoParams::DEPTH => f32::from(self.depth().get()),
            PadLfoParams::PHASE => f32::from(self.phase().get()),
            PadLfoParams::MODE => LfoMode::ALL
                .iter()
                .position(|&m| m == self.mode())
                .unwrap_or(0) as f32,
            _ => 0.0,
        }
    }

    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            PadLfoParams::WAVE if (v as usize) < WAVE_NAMES.len() => {
                self.set_wave(LfoShape::from_u8(v as u8))
            }
            PadLfoParams::SPEED => self.set_speed(u7(v)),
            PadLfoParams::DEPTH => self.set_depth(u7(v)),
            PadLfoParams::PHASE => self.set_phase(u7(v)),
            PadLfoParams::MODE => {
                if let Some(&m) = LfoMode::ALL.get(v as usize) {
                    self.set_mode(m);
                }
            }
            _ => {}
        }
    }
}

/// `PAD_ROUTE_SPECS`' ids.
pub enum PadRouteParams {}

impl PadRouteParams {
    pub const SOURCE: ParamId = ParamId(0);
    pub const DEST: ParamId = ParamId(1);
    pub const AMOUNT: ParamId = ParamId(2);
}

/// SOURCE's names, by `RouteSource::index`: VELOCITY outruns its cell.
const SOURCE_NAMES: [&str; RouteSource::ALL.len()] =
    ["MOD ENV", "LFO", "VEL", "LFO 1", "LFO 2", "LFO 3"];
/// DEST's names, by `RouteDest::ALL`: RESONANCE, LFO SPEED and LFO DEPTH
/// outrun their cells.
const DEST_NAMES: [&str; RouteDest::ALL.len()] = ["CUTOFF", "RESO", "VOLUME", "LFO SPD", "LFO DEP"];

/// A route: SOURCE, DEST and AMOUNT, −127..=127 shown as the matrix's.
pub static PAD_ROUTE_SPECS: [ParamSpec; 3] = [
    ParamSpec::choice(
        PadRouteParams::SOURCE.0,
        "SOURCE",
        ValFmt::Names(&SOURCE_NAMES),
        (SOURCE_NAMES.len() - 1) as f32,
        0.0,
        Change::NoteOn,
    )
    .ident("SOURCE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        PadRouteParams::DEST.0,
        "DEST",
        ValFmt::Names(&DEST_NAMES),
        (DEST_NAMES.len() - 1) as f32,
        0.0,
        Change::NoteOn,
    )
    .ident("DEST")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::stepped(
        PadRouteParams::AMOUNT.0,
        "AMOUNT",
        ValFmt::Route,
        -127.0,
        127.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("AMOUNT"),
];

impl Block for PadRoute {
    fn specs(&self) -> &'static [ParamSpec] {
        &PAD_ROUTE_SPECS
    }

    #[optimize(size)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            PadRouteParams::SOURCE => self.source().index() as f32,
            PadRouteParams::DEST => RouteDest::ALL
                .iter()
                .position(|&d| d == self.dest())
                .unwrap_or(0) as f32,
            PadRouteParams::AMOUNT => f32::from(self.amount().get()),
            _ => 0.0,
        }
    }

    #[optimize(size)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            PadRouteParams::SOURCE => {
                if let Some(&s) = RouteSource::ALL.get(v as usize) {
                    self.set_source(s);
                }
            }
            PadRouteParams::DEST => {
                if let Some(&d) = RouteDest::ALL.get(v as usize) {
                    self.set_dest(d);
                }
            }
            PadRouteParams::AMOUNT => self.set_amount(Amount::clamped(v as i8)),
            _ => {}
        }
    }
}
