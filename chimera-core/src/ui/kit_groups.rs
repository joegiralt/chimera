//! DRUM's voice groups on the KEYS page (ADR 0280), bank 2's: `bank2.x`
//! links this module there and bank 1 reaches it only through the UI
//! door's one entry (`UiDoor::groups`), so a closed door holds no CHOKE
//! turn, shows no count and lets a set land.

use core::fmt::Write;

use super::UiState;
use super::held::{self, ChokeHold, Held};
use super::part_page::Edit;
use crate::block::Block;
use crate::dsp::drum::groups::{KitChokes, VoiceGroups};
use crate::dsp::drum::types::{ChokeGroup, Kit, PadId};
use crate::project::{Line, PartId};

/// What bank 1 asks.
pub(crate) enum Ask<'a> {
    /// An encoder turn on CHOKE: onto a 9th group it is held, dimmed with
    /// its reason, and nothing is written (ADR 0151's rule).
    Turn(&'a mut UiState, PartId, i8, bool),
    /// A CHOKE edit no turn held (a CC): a step skips the groups that
    /// would make a 9th; one that can't land is refused with a toast, the
    /// kit and its revision untouched.
    Edit(&'a mut UiState, PartId, Edit),
    /// A held CHOKE on Part `at`'s selected pad: why it is refused, if it
    /// still is and the kit's chokes are as they were at the hold.
    Held(&'a UiState, PartId, ChokeHold),
    /// The refusal's words for a count.
    Why(VoiceGroups),
    /// CHOKE's focus band: the kit's count.
    Note(&'a Kit),
}

/// The answer to each `Ask`.
pub(crate) enum Said {
    /// `Turn` (handled) and `Edit` (whether it moved).
    Moved(bool),
    /// `Held`: refused, with the count it would make.
    Refused(VoiceGroups),
    /// `Held`: it would land, or the kit changed since.
    Fits,
    /// `Why` and `Note`.
    Line(Line),
}

/// The words, byte statics named so they link into bank 2 with this
/// module (`bank2.x`'s `.rodata.*kit_groups*`), not into bank 1's
/// unnamed rodata; read as text checked when built.
static GROUPS_BYTES: [u8; 7] = *b"GROUPS ";
static HOLDS_BYTES: [u8; 21] = *b" GROUPS \xc2\xb7 KIT HOLDS ";
static GROUPS: &str = text(&GROUPS_BYTES);
static HOLDS: &str = text(&HOLDS_BYTES);

const fn text(b: &'static [u8]) -> &'static str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("the words are UTF-8"),
    }
}

/// The door's entry. This module's functions that check a choke are
/// built for speed, not size: from minsize code the machine outliner
/// shares their calls to `VoiceGroups::choke` as a fragment it places in
/// bank 1 (ADR 0175).
#[optimize(speed)]
pub(crate) fn ask_in(a: Ask<'_>) -> Said {
    match a {
        Ask::Turn(ui, at, delta, shift) => Said::Moved(turn(ui, at, delta, shift)),
        Ask::Edit(ui, at, how) => Said::Moved(edit(ui, at, how)),
        Ask::Held(ui, at, hold) => {
            let (kit, p) = (ui.project.kit(at), ui.project.selected_pad(at));
            match VoiceGroups::choke(kit, p, hold.to) {
                Err(n) if hold.kit == KitChokes::of(kit) => Said::Refused(n),
                _ => Said::Fits,
            }
        }
        Ask::Why(n) => Said::Line(why(n)),
        Ask::Note(kit) => Said::Line(note(kit)),
    }
}

/// A CHOKE turn `delta` (to an end with `shift`) on Part `at`'s selected
/// pad, from its held group (while the kit's chokes are as they were at
/// the hold) or its own.
#[optimize(speed)]
fn turn(ui: &mut UiState, at: PartId, delta: i8, shift: bool) -> bool {
    let (kit, p) = (ui.project.kit(at), ui.project.selected_pad(at));
    let chokes = KitChokes::of(kit);
    let from = match ui.held {
        Some((h, Held::Choke(c))) if h == at && c.kit == chokes => c.to,
        _ => kit.pad(p).choke(),
    };
    let to = if shift {
        ChokeGroup::end(delta)
    } else {
        from.step(delta)
    };
    let refused = VoiceGroups::choke(kit, p, to).is_err();
    ui.held = refused.then_some((at, Held::Choke(ChokeHold { to, kit: chokes })));
    if !refused && to != kit.pad(p).choke() {
        ui.project.kit_mut(at).pad_mut(p).set_choke(to);
    }
    true
}

/// `how` on Part `at`'s selected pad's CHOKE: written only where it
/// lands. A step (a relative CC, a stepped CC's bin) skips the groups
/// that would make a 9th, on to the next that fits its way; a set lands
/// where it points. A refused one says why, once while its toast is up.
#[optimize(speed)]
fn edit(ui: &mut UiState, at: PartId, how: Edit) -> bool {
    let (kit, p) = (ui.project.kit(at), ui.project.selected_pad(at));
    let now = kit.pad(p).choke();
    let id = held::CHOKE.param;
    let set = |edit: &dyn Fn(&mut dyn Block)| {
        let mut pad = *kit.pad(p);
        edit(&mut pad);
        let to = pad.choke();
        VoiceGroups::choke(kit, p, to).map(|_| to)
    };
    let to = match how {
        // A pushed turn on a list moves a whole step, as a turn does.
        #[cfg(feature = "push-encoders")]
        Edit::Fine(d) => step(kit, p, now, d),
        Edit::Nudge(d) => step(kit, p, now, d),
        Edit::Snap(d) => set(&|b| b.snap(id, d)),
        Edit::To(v) => set(&|b| b.set(id, v)),
    };
    match to {
        Ok(to) if to == now => false,
        Ok(to) => {
            ui.project.kit_mut(at).pad_mut(p).set_choke(to);
            true
        }
        Err(n) => {
            let l = why(n);
            if !ui.toast.shows(&l) {
                ui.error(l);
            }
            false
        }
    }
}

/// Pad `p` of `kit` stepped `d` from `now`, on past each group that would
/// make a 9th to the first that fits that way; else the count the first
/// step would make.
#[optimize(speed)]
fn step(kit: &Kit, p: PadId, now: ChokeGroup, d: i8) -> Result<ChokeGroup, VoiceGroups> {
    let mut to = now.step(d);
    let mut first = None;
    loop {
        match VoiceGroups::choke(kit, p, to) {
            Ok(_) => return Ok(to),
            Err(n) => {
                let n = *first.get_or_insert(n);
                let next = to.step(d.signum());
                if next == to {
                    return Err(n);
                }
                to = next;
            }
        }
    }
}

/// CHOKE's focus band: `GROUPS 6/8`, over 8 on a kit from before the
/// bound.
fn note(kit: &Kit) -> Line {
    let mut l = Line::new(GROUPS);
    let _ = write!(
        l,
        "{}/{}",
        VoiceGroups::of(kit).get(),
        VoiceGroups::MAX.get()
    );
    l
}

/// Why a choke is refused: `9 GROUPS · KIT HOLDS 8`.
fn why(n: VoiceGroups) -> Line {
    let mut l = Line::new("");
    let _ = write!(l, "{}{}{}", n.get(), HOLDS, VoiceGroups::MAX.get());
    l
}
