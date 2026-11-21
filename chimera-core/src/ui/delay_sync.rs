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
use crate::dsp::delay::{DelayNote, DelayParams, DelaySync};
use crate::dsp::rev::sync::FoldState;
use crate::ui::feel::Nudged;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::view::{DELAY_NOTE, DELAY_TIME, View, Words};

/// The words this module draws, named so they link into bank 2 with it
/// (`.rodata.*delay_sync*`), not into bank 1's unnamed rodata.
static WORDS: [u8; 85] =
    *b"MIX+TURNSYNCFREEHALVED TO FIT 500 MSDOUBLED TO FIT 10 MSTIME TURNS NOTES OF THE TEMPO";

/// `WORDS[at..at + len]`; checked at compile time (`WORDS` is UTF-8).
const fn word_at(at: usize, len: usize) -> &'static str {
    let (_, b) = WORDS.split_at(at);
    let (b, _) = b.split_at(len);
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("WORDS is UTF-8"),
    }
}

/// MIX + a turn right from FREE is SYNC; unspaced, as the fine turn's,
/// so learn's words fit beside.
const TO_SYNC: Gesture<'static> = Gesture(word_at(0, 8), word_at(8, 4));
/// And left from SYNC, FREE.
const TO_FREE: Gesture<'static> = Gesture(word_at(0, 8), word_at(12, 4));

/// The TIME cell's band, by SYNC and whether a CC may drive the cell:
/// its flip, then learn's short form on a mapped cell; on an unmapped
/// one FREE's ms keeps the fine turn beside it (the note has none: a
/// detent is a note already).
pub static FREE_LEARN: [Gesture<'static>; 2] = [TO_SYNC, crate::ui::view::LEARN_SHORT];
pub static FREE_FINE: [Gesture<'static>; 2] = [TO_SYNC, crate::ui::feel::FINE_TURN];
pub static SYNC_LEARN: [Gesture<'static>; 2] = [TO_FREE, crate::ui::view::LEARN_SHORT];
pub static SYNC_PLAIN: [Gesture<'static>; 1] = [TO_FREE];
/// The note plays halved, or doubled, to stay on the line.
pub static HALVED: &str = word_at(16, 20);
pub static DOUBLED: &str = word_at(36, 20);
/// What SYNC does.
pub static SYNC_WORDS: &str = word_at(56, 29);

/// The note at `v` (NOTE's 0..1) as it plays: halved or doubled as the
/// engine last folded it (`meter::delay_fold`), with the reverb's marks,
/// `1/8D`, `▾1/8`.
#[inline(never)]
pub fn note_text(buf: &mut FmtBuf, v: f32) {
    note_text_at(buf, v, crate::meter::delay_fold());
}

/// The note at `v` folded `fold`: `ed`'s, at the fold its answer read.
#[inline(never)]
pub fn note_text_at(buf: &mut FmtBuf, v: f32, fold: FoldState) {
    DelayNote::at(v).division().write(fold, buf);
}

/// The band's hint on the delay's TIME cell, the ms or the note, with
/// words `w`: its flip, and learn's or the fine turn beside it. Any other
/// cell has none here.
#[inline(never)]
pub fn hint(v: &View, w: Words) -> Option<Hint> {
    let learn = w == Words::Learn;
    match v.addr() {
        Some(a) if a == DELAY_TIME => Some(if learn { &FREE_LEARN } else { &FREE_FINE }),
        Some(a) if a == DELAY_NOTE && learn => Some(&SYNC_LEARN),
        Some(a) if a == DELAY_NOTE => Some(&SYNC_PLAIN),
        _ => None,
    }
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
        assert_eq!(FREE_LEARN[0], Gesture("MIX+TURN", "SYNC"));
        assert_eq!(SYNC_LEARN[0], Gesture("MIX+TURN", "FREE"));
        assert_eq!(FREE_FINE[1], Gesture("SEQ+TURN", "FINE"));
        assert_eq!(FREE_LEARN[1], Gesture("MIX + SEQ", "LEARN"));
        assert_eq!(HALVED, "HALVED TO FIT 500 MS");
        assert_eq!(DOUBLED, "DOUBLED TO FIT 10 MS");
        assert_eq!(SYNC_WORDS, "TIME TURNS NOTES OF THE TEMPO");
        assert_eq!(WORDS.len(), 56 + SYNC_WORDS.len());
    }
}
