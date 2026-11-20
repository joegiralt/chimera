//! The delay's tempo sync on the box (ADR 0302): the TIME cell's note as
//! it plays, MIX + turn flipping SYNC with its hint, and the band's
//! words. Bank 2's (`bank2.x`'s `ui::delay_sync` group, S4), reached
//! through doors that were there: the note's text by `law_text`
//! (`walk::law_text_in`), the hint and the flip by `feel`, the words by
//! `band_note` (`notes::cell`). Bank 1 keeps the specs, the slot's binding
//! (`view::view`) and the snap's stub.

use chimera_hal::Ticks;

use crate::addr::ParamAddr;
use crate::block::{Block, ParamId};
use crate::dsp::delay::sync::NOTES;
use crate::dsp::delay::{DelayNote, DelayParams, DelaySync};
use crate::ui::feel::Nudged;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::view::{DELAY_NOTE, DELAY_TIME, View};

/// The words this module draws, named so they link into bank 2 with it
/// (`.rodata.*delay_sync*`), not into bank 1's unnamed rodata.
static WORDS: [u8; 90] = *b"MIX + TURNFREE / SYNCHALVED TO FIT 500 MSDOUBLED TO FIT 10 MSTIME TURNS NOTES OF THE TEMPO";

/// `WORDS[at..at + len]`; checked at compile time (`WORDS` is UTF-8).
const fn word_at(at: usize, len: usize) -> &'static str {
    let (_, b) = WORDS.split_at(at);
    let (b, _) = b.split_at(len);
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("WORDS is UTF-8"),
    }
}

/// The TIME cell's gesture, FREE or synced: MIX + a turn right is SYNC,
/// left FREE.
pub static FLIP: [Gesture<'static>; 1] = [Gesture(word_at(0, 10), word_at(10, 11))];
/// The note plays halved, or doubled, to stay on the line.
pub static HALVED: &str = word_at(21, 20);
pub static DOUBLED: &str = word_at(41, 20);
/// What SYNC does.
pub static SYNC_WORDS: &str = word_at(61, 29);

/// The note at `v` (NOTE's 0..1) as it plays: halved or doubled as the
/// engine last folded it (`meter::delay_fold`), with the reverb's marks,
/// `1/8D`, `▾1/8`.
#[inline(never)]
pub fn note_text(buf: &mut FmtBuf, v: f32) {
    NOTES[DelayNote::at(v).index()].write(crate::meter::delay_fold(), buf);
}

/// The band's hint on the delay's TIME cell, the ms or the note: its
/// gesture. Any other cell has none here.
#[inline(never)]
pub fn hint(v: &View) -> Option<Hint> {
    matches!(v.addr(), Some(a) if a == DELAY_TIME || a == DELAY_NOTE).then_some(&FLIP[..])
}

/// MIX + turn on the delay's TIME cell: SYNC on a turn right, FREE on a
/// turn left; the ms and the note stay as they were. Any other cell
/// snaps by its spec.
#[inline(never)]
pub fn snap(b: &mut dyn Block, a: ParamAddr, d: Ticks) -> Nudged {
    if (a != DELAY_TIME && a != DELAY_NOTE) || d.get() == 0 {
        return Nudged::Spec;
    }
    let to = if d.get() > 0 {
        DelaySync::Sync
    } else {
        DelaySync::Free
    };
    b.write(DelayParams::SYNC, to.value());
    Nudged::Sync
}

/// What the band says in the hint's place on the delay's cell `id`: why
/// the note reads marked, and what SYNC does.
#[inline(never)]
pub fn word(id: ParamId) -> Option<&'static str> {
    match id {
        DelayParams::NOTE => match crate::meter::delay_fold().0 {
            0 => None,
            n if n > 0 => Some(HALVED),
            _ => Some(DOUBLED),
        },
        DelayParams::SYNC => Some(SYNC_WORDS),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_cut_where_they_end() {
        assert_eq!(FLIP[0], Gesture("MIX + TURN", "FREE / SYNC"));
        assert_eq!(HALVED, "HALVED TO FIT 500 MS");
        assert_eq!(DOUBLED, "DOUBLED TO FIT 10 MS");
        assert_eq!(SYNC_WORDS, "TIME TURNS NOTES OF THE TEMPO");
        assert_eq!(WORDS.len(), 61 + SYNC_WORDS.len());
    }
}
