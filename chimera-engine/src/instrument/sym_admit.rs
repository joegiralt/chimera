// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Batch E's Sympathetic pacing and DRUM bills (#306, #962, #977; ADR
//! 0311, 0320): bank 2's, reached only through `AdmitDoor`'s entries.
//! `bank2.x` links this module into an S4 group (`10instrument9sym_admit`),
//! beside `ed`'s: S0-S3 is short of room (ADR 0251).

use super::Instrument;
use crate::MidiNote;
use crate::hw::{Cost, MAX_VOICES};
use crate::project::PartId;
use crate::voice_alloc::VoiceIdx;

impl Instrument {
    /// The bytes this block may clear in all (#306, ADR 0311), asked at
    /// each clear: what is left of the deadline (the audio share over
    /// `AUDIO_BUDGET_PERCENT`) once the bookings and the reserve and ADR
    /// 0262's overhead for each Part with a voice booked are spent, at the
    /// clear's rate; at most one worst note-on. Every clear runs at the
    /// render, after the block's note-ons have all booked (`Plays::Strike`),
    /// so the bookings are the block's. At a full pool with six Parts rev V
    /// clears 7,680 B a block, rev Y 4,344 B; a light pool clears more.
    pub(super) fn clear_room_in(&self) -> usize {
        use super::{BLOCK_OVERHEAD, PART_OVERHEAD, SYM_CLEAR_BUDGET, SYM_CLEAR_CYCLES_PER_BYTE};
        let budget = self.alloc.budget();
        let now = self.alloc.sounding_cost() + self.reserved();
        // Judged at the render, after every note-on of the block has
        // booked (`Plays::Strike`, ADR 0311): the bookings as they stand.
        let booked = now;
        let parts = self
            .alloc
            .slots()
            .iter()
            .filter_map(|s| s.part())
            .fold(crate::project::PartSet::EMPTY, |s, p| s.with(p))
            .len();
        let parts = parts as u32;
        // A sample's cycles stay far under `u32::MAX / 100` at any clock.
        let deadline = budget.as_cost().0 * 100 / crate::hw::AUDIO_BUDGET_PERCENT;
        let used = booked
            .0
            .saturating_add(BLOCK_OVERHEAD.0 + PART_OVERHEAD.0 * parts);
        let room = deadline.saturating_sub(used)
            * (chimera_hal::BLOCK_SIZE as u32 / SYM_CLEAR_CYCLES_PER_BYTE);
        (room as usize).min(SYM_CLEAR_BUDGET)
    }

    /// What this block may still clear (#306, ADR 0311): its room as the
    /// bookings stand less what it has cleared, within the one worst
    /// note-on a block `clear_left` keeps.
    pub(super) fn clear_avail_in(&self) -> usize {
        use super::SYM_CLEAR_BUDGET;
        self.clear_room_in()
            .saturating_sub(SYM_CLEAR_BUDGET - self.clear_left)
            .min(self.clear_left)
    }

    /// Whether voice `v` may start Sympathetic `note` on `params` this
    /// block, the clear it takes paid (#306, ADR 0311): a strike that adds
    /// to what rings fits the room or waits; else none waits before it and
    /// its clear fits, or the room clears its lines ahead and it waits.
    pub(super) fn admit_sym_in(
        &mut self,
        vi: VoiceIdx,
        params: &crate::params::ParamSnapshot,
        note: MidiNote,
    ) -> bool {
        use super::SYM_CLEAR_BUDGET;
        let v = vi.index();
        if let Some(bytes) = self.voices[v].sym_strike_adds(&self.sym, note, params) {
            // A re-strike, or STEAL GLIDE's growth, adds to what rings: it
            // fits the block's room, or the note takes the deferred path
            // (a fade, then a paced start). A CUT is a fresh note.
            if bytes > self.clear_avail_in() {
                return false;
            }
            self.clear_left -= bytes;
            return true;
        }
        if self.clear_backlog {
            return false;
        }
        let bytes = self.voices[v].sym_note_on_clear(&self.sym, note, params);
        debug_assert!(bytes <= SYM_CLEAR_BUDGET);
        let avail = self.clear_avail_in();
        if bytes > avail {
            // The block's room clears its lines ahead; it waits for the
            // rest (#306).
            let done = self.voices[v].sym_pre_clear(&mut self.sym, avail);
            self.clear_left -= done;
            self.clear_backlog = true;
            return false;
        }
        self.clear_left -= bytes;
        true
    }

    /// A pending strike on voice `v`'s sounding engine (`Plays::Strike`):
    /// whether it starts now. One whose engine went stale, or whose clear
    /// doesn't fit, fades out and waits as a note.
    pub(super) fn strike_ready_in(
        &mut self,
        vi: VoiceIdx,
        params: &crate::params::ParamSnapshot,
        note: MidiNote,
        w: super::Waiting,
    ) -> bool {
        let v = vi.index();
        let starts = super::SlotKind::of(params) == super::SYMPATHETIC
            && self.voices[v].is_active()
            && self.voices[v].starts_now(params, &self.sym);
        if starts && self.admit_sym_in(vi, params, note) {
            return true;
        }
        self.waiting[v] = Some(super::Waiting {
            plays: super::Plays::Note,
            ..w
        });
        let _ = self.voices[v].kill();
        false
    }

    /// Each voice's DRUM hit's live follow next block (#962), from the kit
    /// of the Part it sounds for (a steal across Parts hasn't handed it on).
    pub(super) fn follows_in(&self) -> [Cost; MAX_VOICES] {
        core::array::from_fn(|v| {
            self.sounding[v].map_or(Cost::ZERO, |s| {
                self.voices[v].drum_follow_extra(crate::dsp::drum::view::KIT_VIEW.seq(s))
            })
        })
    }

    /// Each Part's kit bill as published, with each ringing hit's struck
    /// runs folded in (`KitBill::with`, #977).
    pub(super) fn ringing_kits_in(
        &self,
    ) -> [crate::dsp::drum::sound::KitBill; crate::hw::MAX_PARTS] {
        let mut kits = PartId::ALL.map(|p| crate::dsp::drum::view::KIT_VIEW.bill(p));
        for (v, p) in self.voices.iter().zip(&self.sounding) {
            if let (Some(r), Some(p)) = (v.drum_runs(), p) {
                let k = &mut kits[p.index()];
                *k = k.with(r);
            }
        }
        kits
    }
}
