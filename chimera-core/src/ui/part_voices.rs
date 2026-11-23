// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MAX VOICES and RESERVE on MIXER › PRT › VOICES (ADR 0281), bank 2's:
//! `bank2.x` links this module there and bank 1 reaches it only through
//! the UI door's one entry (`UiDoor::voices`), so a closed door holds no
//! turn and lets an edit land; the page then says the rule is off
//! (`view::voices_note`). The rule itself is `Shares::refusal`, shared
//! with the allocator's types (`voice_limit`).

use core::fmt::Write;

use super::UiState;
use super::held::{Held, VoiceCell, VoiceTurn, VoicesHold};
use super::part_page::Edit;
use crate::block::Block;
use crate::hw::MAX_PARTS;
use crate::project::{Line, PartId, Project};
use crate::voice_limit::{Asked, Refusal, Shares, VoiceCount};

/// Every Part, as the renderer holds them.
type Parts = [crate::preset::Part; MAX_PARTS];

/// What bank 1 asks. Each answer goes to the ask's own slot, typed by
/// it and empty (`None`) until bank 2 answers: with the door closed it
/// stays empty.
pub(crate) enum Ask<'a> {
    /// A turn or an edit, or a held choice's check.
    Judge(Job<'a>),
    /// A refusal's words.
    Why(Refusal, &'a mut Option<Line>),
    /// RESERVE's focus band: the reserves booked.
    Note(&'a Parts, &'a mut Option<Line>),
}

/// What is judged against the rule.
pub(crate) enum Job<'a> {
    /// An encoder turn or an edit on Part `at`'s `cell`.
    Act {
        ui: &'a mut UiState,
        at: PartId,
        cell: VoiceCell,
        act: Act,
        out: &'a mut Option<Turned>,
    },
    /// A held choice on Part `at`: why it is refused, if it still is and
    /// what it was judged against is unchanged.
    Held {
        ui: &'a UiState,
        at: PartId,
        hold: VoicesHold,
        out: &'a mut Option<Result<(), Refusal>>,
    },
}

/// A turn or an edit.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Act {
    /// An encoder turn `delta`, to the end that way with `shift`: onto a
    /// value that can't be honoured it is held, dimmed with its reason,
    /// and nothing is written (ADR 0151's rule).
    Turn { delta: i8, shift: bool },
    /// An edit no turn held (a CC, the console): one that can't be
    /// honoured is refused with a toast, nothing written.
    Edit(Edit),
}

/// What a turn or an edit did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turned {
    /// Written.
    Wrote,
    /// Already there: nothing written.
    Unchanged,
    /// A turn held it dimmed with its reason.
    Held,
    /// An edit refused aloud.
    Refused,
}

impl Turned {
    /// The cell's value moved.
    pub const fn moved(self) -> bool {
        matches!(self, Turned::Wrote)
    }
}

/// The words, byte statics named so they link into bank 2 with this
/// module (`bank2.x`'s `.rodata.*part_voices*`), not into bank 1's
/// unnamed rodata. `text` checks them as UTF-8 when built: a const
/// `from_utf8` that panics at compile time, intentionally, as
/// `kit_groups`' do (ADR 0281).
static RESERVE_BYTES: [u8; 8] = *b"RESERVE ";
static MAX_BYTES: [u8; 11] = *b" \xc2\xb7 VOICES ";
static RESERVES_BYTES: [u8; 9] = *b"RESERVES ";
static POOL_BYTES: [u8; 15] = *b" \xc2\xb7 POOL HOLDS ";
static RESERVE_W: &str = text(&RESERVE_BYTES);
static MAX_W: &str = text(&MAX_BYTES);
static RESERVES_W: &str = text(&RESERVES_BYTES);
static POOL_W: &str = text(&POOL_BYTES);

const fn text(b: &'static [u8]) -> &'static str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("the words are UTF-8"),
    }
}

/// The door's entry.
pub(crate) fn ask_in(a: Ask<'_>) {
    match a {
        Ask::Judge(j) => judge(j),
        Ask::Why(r, out) => *out = Some(why(r)),
        Ask::Note(parts, out) => {
            *out = Some(note(&Shares::from_parts(
                &parts.each_ref().map(|p| p.mix.voices()),
            )))
        }
    }
}

/// Every Part's pair in `project`.
pub fn shares(project: &Project) -> Shares {
    Shares::from_parts(&project.perf().parts().each_ref().map(|p| p.mix.voices()))
}

/// `job` against the rule. `Shares::refusal` is called once, here: a
/// call repeated in bank-2 code is one the machine outliner may share
/// from bank 1 (ADR 0175). Built for speed, as `kit_groups`' checks.
#[optimize(speed)]
fn judge(job: Job<'_>) {
    let asked = proposed(&job);
    let refused = asked.and_then(|(s, at, to)| s.refusal(at, to));
    match job {
        Job::Act {
            ui,
            at,
            cell,
            act,
            out,
        } => {
            *out = Some(match (asked, act) {
                (None, _) => Turned::Unchanged,
                (Some((s, _, to)), Act::Turn { .. }) => turn(ui, at, (cell, to, s), refused),
                (Some((_, _, to)), Act::Edit(_)) => edit(ui, at, to, refused),
            });
        }
        Job::Held { out, .. } => *out = Some(refused.map_or(Ok(()), Err)),
    }
}

/// The pair `job` proposes for its Part, with the shares it is judged
/// against; `None` for an edit that moves nothing, or a held choice
/// judged against what has since changed.
fn proposed(job: &Job<'_>) -> Option<(Shares, PartId, Asked)> {
    match job {
        Job::Act {
            ui, at, cell, act, ..
        } => {
            let s = shares(&ui.project);
            let now = s.ask(*at);
            match *act {
                Act::Turn { delta, shift } => {
                    let from = match ui.held {
                        Some((h, Held::Voices(hold)))
                            if h == *at && hold.to.cell() == *cell && hold.seen == s.seen(*at) =>
                        {
                            hold.to
                        }
                        Some(_) | None => VoiceTurn::of(*cell, now),
                    };
                    Some((s, *at, from.step(delta, shift).on(now)))
                }
                Act::Edit(how) => {
                    let mut m = ui.project.part(*at).mix;
                    let b: &mut dyn Block = &mut m;
                    let id = cell.addr().param;
                    match how {
                        Edit::Snap(d) => b.snap(id, d),
                        #[cfg(feature = "push-encoders")]
                        Edit::Fine(d) => crate::block::nudge_fine(b, id, d),
                        Edit::To(v) => b.set(id, v),
                        Edit::Nudge(d) => b.nudge(id, d),
                    }
                    (m.voices() != now).then_some((s, *at, m.voices()))
                }
            }
        }
        Job::Held { ui, at, hold, .. } => {
            let s = shares(&ui.project);
            (hold.seen == s.seen(*at)).then_some((s, *at, hold.to.on(s.ask(*at))))
        }
    }
}

/// A turn on Part `at`'s `cell` onto `to`: held while `refused`, else
/// written.
fn turn(
    ui: &mut UiState,
    at: PartId,
    (cell, to, s): (VoiceCell, Asked, Shares),
    refused: Option<Refusal>,
) -> Turned {
    if refused.is_some() {
        let hold = VoicesHold {
            to: VoiceTurn::of(cell, to),
            seen: s.seen(at),
        };
        ui.held = Some((at, Held::Voices(hold)));
        return Turned::Held;
    }
    ui.held = None;
    if to == s.ask(at) {
        return Turned::Unchanged;
    }
    ui.project.edit_part(at).mix.set_voices(to);
    Turned::Wrote
}

/// An edit onto `to`: written where it can be honoured, else refused
/// aloud, once while its toast is up.
fn edit(ui: &mut UiState, at: PartId, to: Asked, refused: Option<Refusal>) -> Turned {
    match refused {
        None => {
            ui.project.edit_part(at).mix.set_voices(to);
            Turned::Wrote
        }
        Some(r) => {
            let l = why(r);
            if !ui.toast.shows(&l) {
                ui.error(l);
            }
            Turned::Refused
        }
    }
}

/// RESERVE's focus band: `RESERVES 3/8`, over 8 on a project saved past
/// the rule.
fn note(s: &Shares) -> Line {
    let mut l = Line::new(RESERVES_W);
    let _ = write!(l, "{}/{}", s.reserved().get(), VoiceCount::POOL.get());
    l
}

/// Why: `RESERVE 3 · VOICES 2`, `RESERVES 9 · POOL HOLDS 8`.
fn why(r: Refusal) -> Line {
    let mut l = Line::new("");
    let _ = match r {
        Refusal::OverCap(o) => write!(
            l,
            "{RESERVE_W}{}{MAX_W}{}",
            o.reserve().code(),
            o.cap().count().get()
        ),
        Refusal::Overbooked(b) => write!(
            l,
            "{RESERVES_W}{}{POOL_W}{}",
            b.sum().get(),
            VoiceCount::POOL.get()
        ),
    };
    l
}
