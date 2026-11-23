// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each Part's tuning (ADR 0240): a table of 128 per-key pitch offsets
//! from 12-TET, semitones, read once at a voice's note-on and retune.
//! Global, as `played`'s: the UI writes a Part's table when its TUNING
//! changes, MIDI in writes the MTS table, the audio reads one key at a
//! time. Relaxed atomics: a note during a rewrite plays the old or the new
//! offset of its key, never a torn one. All zero is 12-TET, so a unit
//! with no tuning plays bit for bit as before.
//!
//! The writers are bank 2's (`bank2.x` links this module there); the
//! audio's read, `offset`, is inlined into bank 1's note-on.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Bumped by each console `put` or `rm` of a tuning file: the UI reads
/// the Parts on `TUNEn` again (`files_put`).
static FILES_PUT: AtomicU8 = AtomicU8::new(0);

/// How many tuning files a `put` has written, wrapping: a change says the
/// card's `TUNEn` files may differ from the tables.
pub fn files_put() -> u8 {
    FILES_PUT.load(Ordering::Relaxed)
}

/// A `put` wrote, or an `rm` removed, a tuning file.
pub fn file_put() {
    FILES_PUT.fetch_add(1, Ordering::Relaxed);
}

use crate::hw::MAX_PARTS;

pub use crate::scala_name::ScalaName;

/// The tables: one a Part, then the MTS table.
pub struct Tunings {
    keys: [[AtomicU32; 128]; MAX_PARTS + 1],
    /// Each Part's silent keys, a bit a key: KEYBOARD's spare ones (ADR
    /// 0259). A note-on there is refused.
    silent: [[AtomicU32; 4]; MAX_PARTS],
    /// A bit a Part that plays the MTS table, not its own.
    mts: AtomicU8,
}

const MTS: usize = MAX_PARTS;
const _: () = assert!(MAX_PARTS <= u8::BITS as usize);

/// Only with `tuning`: the bench builds keep their AXI (ADR 0087).
#[cfg(feature = "tuning")]
pub static TUNINGS: Tunings = Tunings::new();

/// Part `part`'s offset at `key`, semitones: `TUNINGS`', 12-TET without
/// `tuning`.
#[inline(always)]
pub fn offset(part: usize, key: u8) -> f32 {
    #[cfg(feature = "tuning")]
    return TUNINGS.offset(part, key);
    #[cfg(not(feature = "tuning"))]
    {
        let _ = (part, key);
        0.0
    }
}

/// `Tunings::set` on `TUNINGS`; nothing without `tuning`.
pub fn set(part: usize, table: &[f32; 128]) {
    #[cfg(feature = "tuning")]
    TUNINGS.set(part, table);
    #[cfg(not(feature = "tuning"))]
    let _ = (part, table);
}

/// Whether Part `part`'s `key` is silent (`Tunings::silent`); never
/// without `tuning`.
#[inline(always)]
pub fn silent(part: usize, key: u8) -> bool {
    #[cfg(feature = "tuning")]
    return TUNINGS.silent(part, key);
    #[cfg(not(feature = "tuning"))]
    {
        let _ = (part, key);
        false
    }
}

/// `Tunings::silence` on `TUNINGS`; nothing without `tuning`.
pub fn silence(part: usize, keys: u128) {
    #[cfg(feature = "tuning")]
    TUNINGS.silence(part, keys);
    #[cfg(not(feature = "tuning"))]
    let _ = (part, keys);
}

/// `Tunings::follow_mts` on `TUNINGS`; nothing without `tuning`.
pub fn follow_mts(part: usize, on: bool) {
    #[cfg(feature = "tuning")]
    TUNINGS.follow_mts(part, on);
    #[cfg(not(feature = "tuning"))]
    let _ = (part, on);
}

/// `Tunings::set_mts` on `TUNINGS`; nothing without `tuning`.
pub fn set_mts(key: u8, offset: f32) {
    #[cfg(feature = "tuning")]
    TUNINGS.set_mts(key, offset);
    #[cfg(not(feature = "tuning"))]
    let _ = (key, offset);
}

impl Default for Tunings {
    fn default() -> Self {
        Self::new()
    }
}

impl Tunings {
    pub const fn new() -> Self {
        Self {
            keys: [const { [const { AtomicU32::new(0) }; 128] }; MAX_PARTS + 1],
            silent: [const { [const { AtomicU32::new(0) }; 4] }; MAX_PARTS],
            mts: AtomicU8::new(0),
        }
    }

    /// Part `part`'s offset at `key`, semitones: the audio's one read.
    #[inline(always)]
    pub fn offset(&self, part: usize, key: u8) -> f32 {
        let part = part % MAX_PARTS;
        let t = if self.mts.load(Ordering::Relaxed) >> part & 1 != 0 {
            MTS
        } else {
            part
        };
        f32::from_bits(self.keys[t][usize::from(key & 127)].load(Ordering::Relaxed))
    }

    /// Whether Part `part`'s `key` sounds nothing: the audio's note-on
    /// check. Never on the MTS table.
    #[inline(always)]
    pub fn silent(&self, part: usize, key: u8) -> bool {
        let part = part % MAX_PARTS;
        let k = usize::from(key & 127);
        self.mts.load(Ordering::Relaxed) >> part & 1 == 0
            && self.silent[part][k / 32].load(Ordering::Relaxed) >> (k % 32) & 1 != 0
    }

    /// Part `part`'s silent keys are `keys`, a bit a key.
    pub fn silence(&self, part: usize, keys: u128) {
        for (i, w) in self.silent[part % MAX_PARTS].iter().enumerate() {
            w.store((keys >> (32 * i)) as u32, Ordering::Relaxed);
        }
    }

    /// Part `part` plays `table` (12-TET is all zero), every key sounding.
    pub fn set(&self, part: usize, table: &[f32; 128]) {
        self.follow_mts(part, false);
        self.silence(part, 0);
        for (k, &o) in self.keys[part % MAX_PARTS].iter().zip(table) {
            k.store(o.to_bits(), Ordering::Relaxed);
        }
    }

    /// Part `part` plays the MTS table (`on`), or its own.
    pub fn follow_mts(&self, part: usize, on: bool) {
        let bit = 1 << (part % MAX_PARTS);
        if on {
            self.mts.fetch_or(bit, Ordering::Relaxed);
        } else {
            self.mts.fetch_and(!bit, Ordering::Relaxed);
        }
    }

    /// The MTS table's `key` is `offset` from 12-TET.
    pub fn set_mts(&self, key: u8, offset: f32) {
        self.keys[MTS][usize::from(key & 127)].store(offset.to_bits(), Ordering::Relaxed);
    }
}

/// What a one-pole of time constant `tau_s` seconds keeps of its distance
/// each block: the glide's own pace, as the engine's pitch glide runs.
pub fn keep_for(tau_s: f32, sample_rate: u32) -> f32 {
    let blocks = tau_s * sample_rate as f32 / chimera_hal::BLOCK_SIZE as f32;
    if blocks > 0.0 {
        libm::expf(-1.0 / blocks)
    } else {
        0.0
    }
}

/// Each voice's tuning offset, semitones (ADR 0240): where it is, where it
/// eases to, and what it keeps of the distance each block. A ZST without
/// `tuning`: every read is 12-TET's 0 and the bench builds keep their AXI.
#[derive(Clone, Copy, Debug)]
pub struct VoiceTunes {
    #[cfg(feature = "tuning")]
    v: [(f32, f32, f32); crate::hw::MAX_VOICES],
}

impl VoiceTunes {
    pub const NEW: Self = Self {
        #[cfg(feature = "tuning")]
        v: [(0.0, 0.0, 0.0); crate::hw::MAX_VOICES],
    };

    /// Voice `v` strikes at `to`: it starts there.
    #[inline(always)]
    pub fn strike(&mut self, v: usize, to: f32) -> f32 {
        #[cfg(feature = "tuning")]
        {
            self.v[v % crate::hw::MAX_VOICES] = (to, to, 0.0);
            to
        }
        #[cfg(not(feature = "tuning"))]
        {
            let _ = (v, to);
            0.0
        }
    }

    /// Voice `v` glides to `to`, keeping `keep` of the distance a block
    /// (`keep_for`): where it is now.
    #[inline(always)]
    pub fn glide(&mut self, v: usize, to: f32, keep: f32) -> f32 {
        #[cfg(feature = "tuning")]
        {
            let t = &mut self.v[v % crate::hw::MAX_VOICES];
            (t.1, t.2) = (to, keep);
            t.0
        }
        #[cfg(not(feature = "tuning"))]
        {
            let _ = (v, to, keep);
            0.0
        }
    }

    /// Voice `v`'s offset now, as `strike` or `glide` left it.
    #[inline(always)]
    pub fn now(&self, v: usize) -> f32 {
        #[cfg(feature = "tuning")]
        {
            self.v[v % crate::hw::MAX_VOICES].0
        }
        #[cfg(not(feature = "tuning"))]
        {
            let _ = v;
            0.0
        }
    }

    /// Voice `v`'s offset this block, a block nearer its target.
    #[inline(always)]
    pub fn step(&mut self, v: usize) -> f32 {
        #[cfg(feature = "tuning")]
        {
            let t = &mut self.v[v % crate::hw::MAX_VOICES];
            t.0 = t.1 + (t.0 - t.1) * t.2;
            t.0
        }
        #[cfg(not(feature = "tuning"))]
        {
            let _ = v;
            0.0
        }
    }
}
