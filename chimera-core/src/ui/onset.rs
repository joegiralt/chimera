// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! START and END on TAPE (#698 finding 6): a turn moves them in time, not
//! in 128ths of the source, and MIX + turn snaps to the next onset the way
//! the turn goes (an MPC's and Octatrack's "snap to transient" while
//! trimming a sample start). PREP isn't built, so the onsets are a cheap
//! energy rise found on what RAM holds: a whole row, or a streamed
//! source's heads (the rest is on the card).

/// Ms a detent moves at `ticks` of the encoder's acceleration
/// (`encoder_accel`: 1, 2, 4, 5, 10): their square, so a slow detent is
/// 1 ms and a fast one 100 ms.
pub fn step_ms(ticks: i8) -> f32 {
    let a = f32::from(ticks.unsigned_abs());
    a * a
}

/// START or END at `v` (of the source) turned `ticks` over a source of
/// `frames` at `rate`: `step_ms` of it, a full-speed detent a hundredth of
/// the source at least, so a long bed crosses in a few turns.
pub fn stepped(v: f32, ticks: i8, rate: u16, frames: u32) -> f32 {
    let secs = frames as f32 / f32::from(rate.max(1));
    if secs <= 0.0 || ticks == 0 {
        return v;
    }
    let mut ms = step_ms(ticks);
    if ticks.unsigned_abs() >= 10 {
        ms = ms.max(secs * 10.0);
    }
    let d = f32::from(ticks.signum()) * ms / (1_000.0 * secs);
    crate::dsp::clampf(v + d, 0.0, 1.0)
}

mod detect;
/// The detector on one view: bank 2's, for the tests.
pub use detect::onset_in;
pub(crate) use detect::onset_in_bank2;

/// The next onset in `held` (a source of `frames`) from `from` the turn's
/// way (`fwd`): bank 2's detector through the door; `None` with it shut.
pub fn onset(held: &[u8], frames: u32, from: u32, fwd: bool) -> Option<u32> {
    crate::bank2::ui_door(|d| d.onset(held, frames, from, fwd)).flatten()
}

/// START or END at `v` as time into the source: `137 MS` under a second,
/// else `12.345 S`, to the ms a slow detent moves.
pub fn time_text(buf: &mut crate::ui::fmt::FmtBuf, v: f32, rate: u16, frames: u32) {
    let ms =
        (crate::dsp::clampf(v, 0.0, 1.0) * frames as f32 * 1_000.0 / f32::from(rate.max(1))) as u32;
    let _ = if ms < 1_000 {
        write!(buf, "{ms} MS")
    } else {
        write!(buf, "{}.{:03} S", ms / 1_000, ms % 1_000)
    };
}
