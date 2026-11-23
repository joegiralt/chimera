// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI CONFIG › CONTROL (ADR 0254): the MIXER and SEQUENCER channels'
//! fixed maps, and AUTO's route for the pipes. A channel a Part's own
//! takes is the Part's: CONTROL's is heard only where none does, MIXER
//! before SEQUENCER before AUTO where two share one. The charts are
//! `docs/guide/MIDI.md`'s; `midi_control_test` pins them.

use chimera_hal::Ms;
use chimera_midi::ctl::{Ctl, DataValue, Sel};
use chimera_midi::value::U14;

use super::ramp::bin;
use crate::MidiChannel;
use crate::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use crate::midi_in::ROUTES;
use crate::nrpn::NrpnAddr;
use crate::orbit::RingIdx;
use crate::orbit::chords::ChordRing;
use crate::orbit::scene::SceneRing;
use crate::params::OutParams;
use crate::part::PartParams;
use crate::project::{PartId, Project};
use crate::ui::UiState;
use crate::ui::midi_settings::{Control, MidiSettings};

/// MIXER's first CC of each per-Part block: LEVEL, PAN, then the CHORUS,
/// DELAY and REVERB sends, Part 1 first. Every chart CC is a `FreeCc`:
/// never 0, 6, 32, 38, 64 or 96-101, which MIDI in keeps.
const LEVEL_CC: u8 = 20;
const PAN_CC: u8 = 26;
const SEND_CC: u8 = 41;
/// MIXER's FX CCs: CHORUS, DELAY and REVERB's from `FX_CC`, COMP's from
/// `COMP_CC` (`FX[COMP_AT..]`), one CC each.
const FX_CC: u8 = 70;
const COMP_CC: u8 = 102;
const COMP_AT: u8 = 25;

/// An NRPN number from its MSB (the block's disk code) and LSB.
const fn n(msb: u16, lsb: u16) -> u16 {
    msb << 7 | lsb
}

/// MIXER's FX targets, `FX_CC` and `COMP_CC` up: CHORUS, DELAY, REVERB and COMP's knobs by
/// their NRPN (ADR 0216), the chart's order, TYPEs and choices left to
/// the hand. Fixed: a new param takes a CC past them, never one of these.
static FX: [u16; 32] = [
    // `FX_CC` up.
    // CHORUS RATE, DEPTH, MIX.
    n(20, 1),
    n(20, 2),
    n(20, 3),
    // DELAY TIME, FDBK, WOW, SAT, TONE, MIX, REV, SIZE, DENSITY, PITCH,
    // TEXTURE.
    n(21, 0),
    n(21, 1),
    n(21, 2),
    n(21, 3),
    n(21, 4),
    n(21, 5),
    n(21, 6),
    n(21, 8),
    n(21, 9),
    n(21, 10),
    n(21, 11),
    // REVERB TIME, DAMP, SIZE, MIX, TONE, DIFFUSE, PREDELAY, SHAPE, SMEAR,
    // RATE, DENSITY.
    n(22, 1),
    n(22, 2),
    n(22, 3),
    n(22, 4),
    n(22, 7),
    n(22, 8),
    n(22, 9),
    n(22, 10),
    n(22, 11),
    n(22, 12),
    n(22, 14),
    // `COMP_CC` up (`COMP_AT`): COMP THRESH, ATK, REL, MAKEUP, MIX, VOL, PAN.
    n(24, 0),
    n(24, 2),
    n(24, 3),
    n(24, 4),
    n(24, 5),
    n(24, 6),
    n(24, 7),
];

/// `a`'s NRPN number, at build time.
const fn nrpn_of(a: ParamAddr) -> u16 {
    match NrpnAddr::of(a) {
        Some(n) => n.get().get(),
        None => panic!("a CONTROL target with no NRPN"),
    }
}

const LEVEL: u16 = nrpn_of(ParamAddr::new(BlockRef::Part, PartParams::LEVEL));
const PAN: u16 = nrpn_of(ParamAddr::new(BlockRef::Part, PartParams::PAN));
const SENDS: [u16; 3] = {
    let s = OutParams::SENDS;
    [
        nrpn_of(ParamAddr::new(BlockRef::Out, s[0])),
        nrpn_of(ParamAddr::new(BlockRef::Out, s[1])),
        nrpn_of(ParamAddr::new(BlockRef::Out, s[2])),
    ]
};

/// MIXER's target for CC `cc`: the Part whose canonical slot moves it and
/// the NRPN it names. The FX are every Part's, so Part 1's slots move them.
pub fn mixer_target(cc: u8) -> Option<(PartId, U14)> {
    let part = |base: u8| PartId::ALL.get(usize::from(cc.wrapping_sub(base))).copied();
    let (p, nrpn) = match cc {
        LEVEL_CC..PAN_CC => (part(LEVEL_CC)?, LEVEL),
        PAN_CC..32 => (part(PAN_CC)?, PAN),
        FX_CC..95 => (PartId::ALL[0], FX[usize::from(cc - FX_CC)]),
        COMP_CC..109 => (PartId::ALL[0], FX[usize::from(cc - COMP_CC + COMP_AT)]),
        SEND_CC..59 => {
            let i = cc - SEND_CC;
            let p = PartId::ALL[usize::from(i % 6)];
            (p, SENDS[usize::from(i / 6)])
        }
        _ => return None,
    };
    Some((p, U14::new(nrpn)?))
}

/// SEQUENCER's CCs: PLAY/STOP, each ring on and off, each ring's CHANCE
/// and STEPS, then KEY and SCALE.
const PLAY_CC: u8 = 20;
const RING_CC: u8 = 21;
const CHANCE_CC: u8 = 26;
const STEPS_CC: u8 = 102;
const KEY_CC: u8 = 107;
const SCALE_CC: u8 = 108;

/// What a SEQUENCER CC does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqAct {
    /// Past 63 after below 64, as B6.
    PlayStop,
    /// Past 63 after below 64, as MIX + B*n*.
    Ring(RingIdx),
    /// A stepped param of ORBIT's, binned over its range.
    Set(ParamAddr),
}

pub fn seq_target(cc: u8) -> Option<SeqAct> {
    let ring = |base: u8| RingIdx::new(cc.wrapping_sub(base));
    Some(match cc {
        PLAY_CC => SeqAct::PlayStop,
        RING_CC..CHANCE_CC => SeqAct::Ring(ring(RING_CC)?),
        CHANCE_CC..31 => SeqAct::Set(ParamAddr::new(
            BlockRef::OrbitRing(ring(CHANCE_CC)?),
            SceneRing::CHANCE,
        )),
        STEPS_CC..KEY_CC => SeqAct::Set(ParamAddr::new(
            BlockRef::OrbitRing(ring(STEPS_CC)?),
            SceneRing::STEPS,
        )),
        KEY_CC => SeqAct::Set(ParamAddr::new(BlockRef::OrbitChords, ChordRing::KEY)),
        SCALE_CC => SeqAct::Set(ParamAddr::new(BlockRef::OrbitChords, ChordRing::SCALE)),
        _ => return None,
    })
}

/// Who among CONTROL's has channel `ch`: `None` where a Part's own does,
/// or an MPE zone's member channel (ADR 0252).
pub fn control_on(midi: &MidiSettings, project: &Project, ch: MidiChannel) -> Option<Control> {
    if midi.mpe.member(ch).is_some()
        || PartId::ALL
            .iter()
            .any(|&p| project.part(p).mix.channel == ch)
    {
        return None;
    }
    Control::ALL
        .into_iter()
        .find(|&c| midi.channel_of(c) == Some(ch))
}

/// Who takes each CONTROL channel that loses its channel, three bits each
/// in `Control`'s order: 0 it holds it (or is OFF), 1 a Part, 2 MIXER, 3
/// SEQUENCER, 4 an MPE zone (ADR 0252). The settings page's clash marks
/// (`SlotCtx::clash`).
pub fn clashes(midi: &MidiSettings, project: &Project) -> u16 {
    Control::ALL.into_iter().fold(0, |m, c| {
        let w = match midi
            .channel_of(c)
            .map(|ch| (ch, control_on(midi, project, ch)))
        {
            Some((ch, None)) if midi.mpe.member(ch).is_some() => 4,
            Some((_, None)) => 1,
            // Only an earlier one wins: AUTO never takes another's.
            Some((_, Some(Control::Mixer))) if c != Control::Mixer => 2,
            Some((_, Some(Control::Sequencer))) if c != Control::Sequencer => 3,
            _ => 0,
        };
        m | w << (3 * c as u8)
    })
}

/// The focus band's words on a CONTROL channel another takes, by who: a
/// named table, so its bytes are bank 2's.
static CLASH: [u8; 94] = *b"A PART HAS THIS CHANNEL|MIXER HAS THIS CHANNEL|SEQUENCER HAS THIS CHANNEL|MPE HAS THIS CHANNEL";

/// Why `a`'s cell is dimmed under `clash` (`clashes`), if it is a CONTROL
/// channel another takes: the door's `clash_note`.
pub fn clash_note(a: ParamAddr, clash: u16) -> Option<&'static str> {
    let i = a.param.0.wrapping_sub(MidiSettings::MIXER.0);
    if a.block != BlockRef::Midi || i >= 3 {
        return None;
    }
    let w = (clash >> (3 * i) & 7) as u8;
    (w != 0).then(|| super::pick(&CLASH, w - 1))
}

impl UiState {
    /// The pipes' routes this pass (`ROUTES`): AUTO to the focused Part's
    /// channel, SEQUENCER's keys, each where it holds its channel.
    pub(super) fn control_routes(&self) {
        let won = |c| {
            self.midi
                .channel_of(c)
                .filter(|&ch| control_on(&self.midi, &self.project, ch) == Some(c))
        };
        let to = self.project.part(self.active_part).mix.channel;
        ROUTES.set(won(Control::Auto).map(|a| (a, to)), won(Control::Sequencer));
    }

    /// Channel `ch` is MIXER's or SEQUENCER's: whether it is (and so took
    /// `c`, mapped or not).
    pub(super) fn cc_dedicated(&mut self, ch: MidiChannel, c: &Ctl, now: Ms) -> bool {
        match control_on(&self.midi, &self.project, ch) {
            Some(Control::Mixer) => self.cc_mixer(c, now),
            Some(Control::Sequencer) => self.cc_seq(c),
            _ => return false,
        }
        true
    }

    /// MIXER: a CC by its chart, an NRPN of the FX's by its address; each
    /// moves as a Part channel's unbound NRPN does (`cc_canonical`).
    fn cc_mixer(&mut self, c: &Ctl, now: Ms) {
        match *c {
            Ctl::Cc { cc, value } => {
                if let Some((p, n)) = mixer_target(cc.get()) {
                    self.cc_canonical(p, n, DataValue::Coarse(value), now);
                }
            }
            Ctl::Data {
                sel: Sel::Nrpn(n),
                value,
            } if NrpnAddr::new(n).addr().is_some_and(|a| {
                matches!(
                    a.block,
                    BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb | BlockRef::Comp
                )
            }) =>
            {
                self.cc_canonical(PartId::ALL[0], n, value, now)
            }
            _ => {}
        }
    }

    /// SEQUENCER: a CC by its chart.
    fn cc_seq(&mut self, c: &Ctl) {
        let Ctl::Cc { cc, value } = *c else {
            return;
        };
        let Some(act) = seq_target(cc.get()) else {
            return;
        };
        let v = value.get();
        let edge = |m: &mut u8, bit: u8| {
            let was = *m >> bit & 1 != 0;
            let high = v >= 64;
            *m = *m & !(1 << bit) | u8::from(high) << bit;
            high && !was
        };
        let m = &mut self.cc.ui().seq_high;
        match act {
            SeqAct::PlayStop if edge(m, 0) => self.orbit.play_stop(),
            // MIX + B*n*'s toggle, written here: a call to the panel's
            // own would leave it out of line in bank 1.
            SeqAct::Ring(r) if edge(m, 1 + r.get()) => {
                let at = ParamAddr::new(BlockRef::OrbitRing(r), SceneRing::MUTE);
                let on = self.seq_get(at) == Some(0.0);
                self.seq_write(at, f32::from(on));
            }
            SeqAct::Set(a) => self.seq_set(a, v),
            _ => {}
        }
    }

    /// ORBIT's `a` to 7-bit `v`, binned over its steps as a mapped stepped
    /// param is (ADR 0139), on `live` and its slot alike.
    fn seq_set(&mut self, a: ParamAddr, v: u8) {
        let Some(spec) = a.spec() else {
            return;
        };
        let Some(was) = self.seq_get(a) else {
            return;
        };
        let steps = (spec.max - spec.min) as u16 + 1;
        let cur = (was - spec.min + 0.5) as u16;
        let to = bin(v, steps, 0, 255, cur);
        if to == cur {
            return;
        }
        // KEY, SCALE, CHANCE and STEPS: no SEGS, so nothing to refit.
        self.seq_write(a, spec.min + f32::from(to));
    }

    /// ORBIT's `a` on `live`.
    fn seq_get(&self, a: ParamAddr) -> Option<f32> {
        let live = self.project.orbit().live();
        live.block(a.block).map(|b| b.get(a.param))
    }

    /// ORBIT's `a` to `v`, on `live` and its slot alike.
    fn seq_write(&mut self, a: ParamAddr, v: f32) {
        self.project.orbit_mut().edit(&mut |s| {
            if let Some(mut b) = s.block_mut(a.block) {
                b.set(a.param, v);
            }
        });
    }

    /// A key on SEQUENCER's channel: ORBIT's KEY is its pitch class.
    pub(super) fn seq_key(&mut self, k: crate::MidiNote) {
        let ours = self.midi.channel_of(Control::Sequencer).is_some_and(|ch| {
            control_on(&self.midi, &self.project, ch) == Some(Control::Sequencer)
        });
        if !ours {
            return;
        }
        let key = ParamAddr::new(BlockRef::OrbitChords, ChordRing::KEY);
        self.seq_write(key, f32::from(k.get() % 12));
    }
}

const _: () = assert!(FX_CC as usize + COMP_AT as usize == 95 && FX.len() - COMP_AT as usize == 7);
