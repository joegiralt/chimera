// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Batch D3's per-Part Sound work (#14, #164; ADR 0316): a new Sound's
//! fade and the Parts whose fast pitch routes ramp ALGO. Bank 2's, reached
//! only through `AdmitDoor`'s `fade_replaced` and `ramps`; `bank2.x` links
//! this module into the S4 group beside `sym_admit`.

use super::{AudioShared, Instrument};
use crate::dsp::algo::engine::AlgoEngine;
use crate::hw::{Cost, MAX_PARTS};
use crate::params::EngineType;
use crate::project::PartId;
use crate::voice_alloc::VoiceIdx;

/// The Parts whose fast pitch routes ramp ALGO (`engines::pitch_ramps`), a
/// bit each, and what the ramp adds to each of their voices' bills
/// (`AlgoEngine::ramp_cost`): taken once a Part, a publish (#164).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ramps {
    pub(crate) parts: u8,
    pub(crate) bills: [Cost; MAX_PARTS],
}

impl Ramps {
    /// None ramps: the door closed.
    pub(crate) const NONE: Ramps = Ramps {
        parts: 0,
        bills: [Cost::ZERO; MAX_PARTS],
    };
}

impl Instrument {
    /// A Part's new Sound (its `loads` moved, #14): its held voices (a key
    /// or the pedal) fade out over `Voice::FADE` and end, as CC 120's do.
    /// None restarts on the new Sound, nor plays on under it; one fading
    /// toward a restart stops restarting. A released tail rings on, landed
    /// on the new Sound's knobs, as before. Called before a block's notes
    /// and its render, so a note after the load plays the new Sound.
    pub(super) fn fade_replaced_in(&mut self, shared: &AudioShared) {
        for (q, part) in PartId::ALL.into_iter().zip(&shared.parts) {
            if self.faded[q.index()] == part.loads {
                continue;
            }
            self.faded[q.index()] = part.loads;
            for vi in VoiceIdx::ALL {
                let (v, s) = (&self.voices[vi.index()], self.alloc.slot(vi));
                if self.sounding[vi.index()] == Some(q)
                    && !s.is_free()
                    && (s.held() || self.sustained[vi.index()])
                    && v.is_active()
                    && (!v.fading() || v.restarts())
                {
                    self.sound_off(vi);
                }
            }
        }
    }

    /// Which Parts ramp, and their bills (`Ramps`): an ALGO Part whose
    /// ENV and LFO routes into PITCH and FINE can pass a semitone.
    pub(super) fn ramps_in(shared: &AudioShared) -> Ramps {
        let mut r = Ramps::NONE;
        for (p, part) in shared.parts.iter().enumerate() {
            if part.params.engine() == EngineType::Algo
                && crate::dsp::engines::pitch_ramps(&part.mod_state)
            {
                r.parts |= 1 << p;
                r.bills[p] = AlgoEngine::ramp_cost(&part.params.algo);
            }
        }
        r
    }
}

const _: () = assert!(MAX_PARTS <= u8::BITS as usize);
