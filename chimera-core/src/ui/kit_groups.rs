//! DRUM's voice groups on the KEYS page (ADR 0280), bank 2's: `bank2.x`
//! links this module there and bank 1 reaches it only through the UI
//! door (`UiDoor`), so a closed door holds no CHOKE turn and shows no
//! count.

use core::fmt::Write;

use crate::dsp::drum::groups::VoiceGroups;
use crate::dsp::drum::types::{ChokeGroup, Kit, PadId};
use crate::project::Line;

/// The words, named so they link into bank 2 with this module
/// (`bank2.x`'s `.rodata.*kit_groups*`), not into bank 1's unnamed
/// rodata.
static WORDS: [u8; 28] = *b"GROUPS  GROUPS \xc2\xb7 KIT HOLDS ";

fn words(r: core::ops::Range<usize>) -> &'static str {
    core::str::from_utf8(&WORDS[r]).unwrap_or_default()
}

/// Whether pad `p` of `kit` in choke group `code` grows the kit past 8:
/// the count it would make (`held::Held::why`).
pub(crate) fn choke_held_in(kit: &Kit, p: PadId, code: u8) -> Option<VoiceGroups> {
    let c = ChokeGroup::from_index(usize::from(code))?;
    VoiceGroups::choke(kit, p, c).err()
}

/// A CHOKE turn `delta` (to an end with `shift`) on pad `p` of `kit`,
/// from its held code or its group: the code it lands on, and whether
/// that is held (a 9th group) rather than written.
pub(crate) fn choke_turn_in(
    kit: &Kit,
    p: PadId,
    held: Option<u8>,
    delta: i8,
    shift: bool,
) -> (u8, bool) {
    let from = held.unwrap_or(kit.pad(p).choke().index() as u8);
    let last = (ChokeGroup::ALL.len() - 1) as i16;
    let to = match (shift, delta > 0) {
        (true, true) => last,
        (true, false) => 0,
        _ => (i16::from(from) + i16::from(delta)).clamp(0, last),
    } as u8;
    (to, choke_held_in(kit, p, to).is_some())
}

/// Pad `p`'s choke just written over `was`: put back if the edit grew the
/// kit past 8, with the count it would have made (a CC's or `ed set`'s
/// edit, which no turn held).
pub(crate) fn choke_undo_in(kit: &mut Kit, p: PadId, was: ChokeGroup) -> Option<VoiceGroups> {
    let now = kit.pad(p).choke();
    kit.pad_mut(p).set_choke(was);
    match VoiceGroups::choke(kit, p, now) {
        Ok(_) => {
            kit.pad_mut(p).set_choke(now);
            None
        }
        Err(n) => Some(n),
    }
}

/// CHOKE's focus band: `GROUPS 6/8`, over 8 on a kit from before the
/// bound.
pub(crate) fn groups_note_in(kit: &Kit) -> Line {
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
pub(crate) fn groups_why_in(n: VoiceGroups) -> Line {
    let mut l = Line::new("");
    let _ = write!(l, "{}{}{}", n.get(), words(7..28), VoiceGroups::MAX.get());
    l
}
