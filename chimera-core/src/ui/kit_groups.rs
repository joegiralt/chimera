//! DRUM's voice groups on the KEYS page (ADR 0280), bank 2's: `bank2.x`
//! links this module there and bank 1 reaches it only through the UI
//! door's one entry (`UiDoor::groups`), so a closed door holds no CHOKE
//! turn, shows no count and lets a set land.

use core::fmt::Write;

use super::part_page::Edit;
use super::{UiState, held};
use crate::block::Block;
use crate::dsp::drum::groups::VoiceGroups;
use crate::dsp::drum::types::{ChokeGroup, Kit, PadId};
use crate::project::{Line, PartId};

/// What bank 1 asks.
pub(crate) enum Ask<'a> {
    /// An encoder turn on CHOKE: onto a 9th group it is held, dimmed with
    /// its reason, and nothing is written (ADR 0151's rule).
    Turn(&'a mut UiState, PartId, i8, bool),
    /// A CHOKE edit no turn held (a CC): one onto a 9th group is refused
    /// with a toast, the kit and its revision untouched.
    Edit(&'a mut UiState, PartId, Edit),
    /// The count a held code on a pad would make, where refused.
    Held(&'a Kit, PadId, u8),
    /// The refusal's words for that count.
    Why(VoiceGroups),
    /// CHOKE's focus band: the kit's count.
    Note(&'a Kit),
}

/// The answer: whether an edit moved, a refused count, or the words.
pub(crate) struct Said {
    pub moved: bool,
    pub groups: Option<VoiceGroups>,
    pub line: Option<Line>,
}

/// The words, named so they link into bank 2 with this module
/// (`bank2.x`'s `.rodata.*kit_groups*`), not into bank 1's unnamed
/// rodata.
static WORDS: [u8; 28] = *b"GROUPS  GROUPS \xc2\xb7 KIT HOLDS ";

fn words(r: core::ops::Range<usize>) -> &'static str {
    core::str::from_utf8(&WORDS[r]).unwrap_or_default()
}

/// The door's entry.
pub(crate) fn ask_in(a: Ask<'_>) -> Said {
    let mut s = Said {
        moved: false,
        groups: None,
        line: None,
    };
    match a {
        Ask::Turn(ui, at, delta, shift) => s.moved = turn(ui, at, delta, shift),
        Ask::Edit(ui, at, how) => s.moved = edit(ui, at, how),
        Ask::Held(kit, p, code) => s.groups = refused(kit, p, code),
        Ask::Why(n) => s.line = Some(why(n)),
        Ask::Note(kit) => s.line = Some(note(kit)),
    }
    s
}

/// The count pad `p` of `kit` in choke group `code` would make, where
/// that grows the kit past 8.
fn refused(kit: &Kit, p: PadId, code: u8) -> Option<VoiceGroups> {
    let c = ChokeGroup::from_index(usize::from(code))?;
    VoiceGroups::choke(kit, p, c).err()
}

/// A CHOKE turn `delta` (to an end with `shift`) on Part `at`'s selected
/// pad, from its held code or its group.
fn turn(ui: &mut UiState, at: PartId, delta: i8, shift: bool) -> bool {
    let (kit, p) = (ui.project.kit(at), ui.project.selected_pad(at));
    let held = ui
        .held
        .filter(|&(h, c)| h == at && c.addr == held::CHOKE)
        .map(|(_, c)| c.code);
    let from = held.unwrap_or(kit.pad(p).choke().index() as u8);
    let last = (ChokeGroup::ALL.len() - 1) as i16;
    let to = match (shift, delta > 0) {
        (true, true) => last,
        (true, false) => 0,
        _ => (i16::from(from) + i16::from(delta)).clamp(0, last),
    } as u8;
    let no = refused(kit, p, to).is_some();
    ui.held = no.then_some((
        at,
        held::Held {
            addr: held::CHOKE,
            code: to,
        },
    ));
    if !no {
        edit(ui, at, Edit::To(f32::from(to)));
    }
    true
}

/// `how` on Part `at`'s selected pad's CHOKE, worked out on a copy: it is
/// written only where it lands. A refused one says why, once while its
/// toast is up (a controller sweep sends many).
fn edit(ui: &mut UiState, at: PartId, how: Edit) -> bool {
    let (kit, p) = (ui.project.kit(at), ui.project.selected_pad(at));
    let mut pad = *kit.pad(p);
    let b: &mut dyn Block = &mut pad;
    let id = held::CHOKE.param;
    match how {
        Edit::Nudge(d) => b.nudge(id, d),
        Edit::Snap(d) => b.snap(id, d),
        #[cfg(feature = "push-encoders")]
        Edit::Fine(d) => crate::block::nudge_fine(b, id, d),
        Edit::To(v) => b.set(id, v),
    }
    let to = pad.choke();
    if to == kit.pad(p).choke() {
        return false;
    }
    match VoiceGroups::choke(kit, p, to) {
        Ok(_) => {
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

/// CHOKE's focus band: `GROUPS 6/8`, over 8 on a kit from before the
/// bound.
fn note(kit: &Kit) -> Line {
    let mut l = Line::new(words(0..7));
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
    let _ = write!(l, "{}{}{}", n.get(), words(7..28), VoiceGroups::MAX.get());
    l
}
