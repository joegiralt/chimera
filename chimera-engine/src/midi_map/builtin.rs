// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in layer (ADR 0263): GM's and RP-021's sound controllers on
//! each Part's own channel, below the Part's map. A fixed CC → canonical
//! NRPN table, so each move ramps as an unbound NRPN does. Bank 1's (ADR
//! 0251): `inline(never)`, and outside bank 2's `midi_map` globs.

use chimera_midi::value::U7;

use crate::MidiChannel;
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
use crate::dsp::algo::params::AlgoOpParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::modulator::EnvSlot;
use crate::midi_settings::MidiSettings;
use crate::mpe::{LOWER_MASTER, UPPER_MASTER};
use crate::nrpn::NrpnAddr;
use crate::params::{EngineType, EnvParams, FilterParams, OutParams};
use crate::part::PartParams;

/// The CCs the layer answers: the guide's chart.
pub const BUILT_IN: [u8; 10] = [7, 10, 11, 71, 72, 73, 74, 91, 93, 94];

/// What the layer does with one CC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    None,
    /// CC 11: the Part's expression (`Part::expression`).
    Expression(U7),
    /// The canonical NRPN `first` to coarse value `value`.
    Move {
        first: u16,
        value: U7,
    },
    /// CC 73 (`which` 0) or 72 (1), RP-021's way: `value`'s offset from 64
    /// shifts `at` (on ALGO, every operator's) from the Sound's own value
    /// (`UiState::cc_shift`). An address, not an NRPN: bank 1 never reads
    /// the NRPN table, which is bank 2's.
    Shift {
        which: u8,
        at: ParamAddr,
        value: U7,
    },
}

const fn nrpn(b: BlockRef, p: ParamId) -> u16 {
    match NrpnAddr::of(ParamAddr::new(b, p)) {
        Some(n) => n.get().get(),
        None => panic!("a built-in target with no NRPN"),
    }
}

const LEVEL: u16 = nrpn(BlockRef::Part, PartParams::LEVEL);
const PAN: u16 = nrpn(BlockRef::Part, PartParams::PAN);
const RESO: u16 = nrpn(BlockRef::Filter, FilterParams::RESONANCE);
const CUTOFF: u16 = nrpn(BlockRef::Filter, FilterParams::CUTOFF);
const CHORUS: u16 = nrpn(BlockRef::Out, OutParams::SEND1);
const DELAY: u16 = nrpn(BlockRef::Out, OutParams::SEND2);
const REVERB: u16 = nrpn(BlockRef::Out, OutParams::SEND3);

/// The engine-wide CCs and their NRPNs.
static FIXED: [(u8, u16); 7] = [
    (7, LEVEL),
    (10, PAN),
    (71, RESO),
    (74, CUTOFF),
    (91, REVERB),
    (93, CHORUS),
    (94, DELAY),
];

/// CC 73 and 72 by engine (`EngineType`'s order), 0 none: ALGO every
/// operator's AR and RR; MODAL rings till DAMP stops it, its strike has
/// no attack; ENV 1 is TAPE's and VA's VCA (`Sound::plain_init`).
const fn env(p: ParamId) -> Option<ParamAddr> {
    Some(ParamAddr::new(BlockRef::Env(EnvSlot::Env1), p))
}

static ENV: [[Option<ParamAddr>; 2]; 4] = {
    [
        [
            Some(ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::AR)),
            Some(ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::RR)),
        ],
        [
            None,
            Some(ParamAddr::new(BlockRef::Modal, ModalParams::DAMP)),
        ],
        [env(EnvParams::ATTACK), env(EnvParams::RELEASE)],
        [env(EnvParams::ATTACK), env(EnvParams::RELEASE)],
    ]
};
const _: () = assert!(
    EngineType::Algo as u8 == 0
        && EngineType::Modal as u8 == 1
        && EngineType::Tape as u8 == 2
        && EngineType::Va as u8 == 3
);

/// CC `cc` at `v` on channel `ch`, heard by a Part playing `e` whose map
/// doesn't bind it. Nothing with MIDI CONFIG's BUILT-IN off, and no
/// CUTOFF on an MPE zone's master, where CC 74 is SLIDE (ADR 0252).
#[inline(never)]
pub fn builtin(m: &MidiSettings, ch: MidiChannel, cc: u8, v: U7, e: EngineType) -> Builtin {
    if !m.built_in() {
        return Builtin::None;
    }
    if cc == 11 {
        return Builtin::Expression(v);
    }
    if cc == 74 && master(m, ch) {
        return Builtin::None;
    }
    if let Some(&(_, first)) = FIXED.iter().find(|f| f.0 == cc) {
        return Builtin::Move { first, value: v };
    }
    let which = match cc {
        73 => 0,
        72 => 1,
        _ => return Builtin::None,
    };
    match shift_target(e, which) {
        None => Builtin::None,
        Some(at) => Builtin::Shift {
            which,
            at,
            value: v,
        },
    }
}

/// CC 73's (`which` 0) or 72's (1) cell on engine `e` (ALGO: operator
/// A's, standing for every operator's).
pub fn shift_target(e: EngineType, which: u8) -> Option<ParamAddr> {
    ENV[e as usize & 3][usize::from(which & 1)]
}

/// A binding a Part's map holds that a zone's master leaves to SLIDE (ADR
/// 0252, 0263): CC 74 on FILTER CUTOFF, the row INIT and the factory
/// Sounds ship, under an active zone's master.
#[inline(never)]
pub fn slide_only(m: &MidiSettings, ch: MidiChannel, b: &crate::midi_map::Binding) -> bool {
    master(m, ch)
        && matches!(b.input, crate::midi_map::Input::Cc(c, _) if c.get() == 74)
        && b.target.addr() == ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)
}

/// `ch` is an active MPE zone's master channel.
fn master(m: &MidiSettings, ch: MidiChannel) -> bool {
    (ch == LOWER_MASTER && m.mpe.lower() > 0) || (ch == UPPER_MASTER && m.mpe.upper() > 0)
}
