//! The band's words for 0.4's tester cells: the matrix's VCA column says
//! its routes are the amp, AMP's BASE says why it is dimmed (ADR 0292),
//! REV › DUCK's cells what they do (ADR 0295). Bank 2's (`bank2.x`'s `ui::notes` group): the matrix's words
//! through `UiDoor`'s `notes` entry, a cell's through `band_note`'s; bank
//! 1 keeps the stub in `view`.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::reverb::ReverbParams;
use crate::modulation::{FixedCol, ModSource, VCA};
use crate::params::OutParams;
use crate::preset::Sound;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::mod_grid::{Col, MatrixState};

/// What bank 1 asks through `UiDoor`'s one `notes` entry, each with its
/// answer's slot.
pub enum Ask<'a> {
    /// `matrix`.
    Matrix(&'a MatrixState, &'a mut Option<Hint>),
}

/// `UiDoor`'s entry.
pub(crate) fn ask_in(a: Ask<'_>) {
    match a {
        Ask::Matrix(m, out) => *out = matrix(m),
    }
}

/// The words this module draws, named so they link into bank 2 with it
/// (`.rodata.*notes*`), not into bank 1's unnamed rodata.
static WORDS: [u8; 125] =
    *b"VCA:ROUTES REPLACE CONTOURSUM STARTS AT AMP BASENEEDS A ROUTE INTO VCADUCKS THE TAIL UNDER ITS INPUTHOW FAST THE TAIL RETURNS";

/// `WORDS[at..at + len]`; checked at compile time (`WORDS` is UTF-8).
const fn word(at: usize, len: usize) -> &'static str {
    let (_, b) = WORDS.split_at(at);
    let (b, _) = b.split_at(len);
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("WORDS is UTF-8"),
    }
}

/// The VCA column with no ENV route: its routes are the whole amp.
pub static VCA_REPLACES: [Gesture<'static>; 1] = [Gesture(word(0, 4), word(4, 22))];
/// The VCA column with an ENV route: the sum starts at AMP's BASE.
pub static VCA_BASE: [Gesture<'static>; 1] = [Gesture(word(0, 4), word(26, 22))];
/// BASE dimmed: it waits for a route into VCA.
pub const BASE_UNROUTED: &str = word(48, 22);
/// REV › DUCK's AMOUNT and RELEASE (ADR 0295).
pub const DUCK_AMOUNT: &str = word(70, 30);
pub const DUCK_RELEASE: &str = word(100, 25);

/// The matrix's hint on `m`'s cursor in the VCA column, on a cell with no
/// route (a route's own cell keeps its DELETE): the routes replace the
/// engine's contour, or, with an ENV among them, they start at BASE.
pub fn matrix(m: &MatrixState) -> Option<Hint> {
    if m.col(m.sel_col) != Col::Fixed(FixedCol::Vca)
        || m.is_x(m.sel_row, m.sel_col)
        || m.is_present(m.sel_source(), m.sel_col)
    {
        return None;
    }
    let present = m.fixed_present[FixedCol::Vca.index()];
    let env = [ModSource::Env1, ModSource::Env2, ModSource::Env3]
        .iter()
        .any(|s| present & (1 << s.index()) != 0);
    Some(if env { &VCA_BASE } else { &VCA_REPLACES })
}

/// What the focus band says on `addr` of `s` in its hint's place: bank
/// 2's `walk::band_note_in` asks it after TAPE's notes.
pub fn cell(addr: ParamAddr, s: &Sound) -> Option<&'static str> {
    match (addr.block, addr.param) {
        (BlockRef::Out, OutParams::VCA) => {
            (s.mod_state.routes_into(VCA) == 0).then_some(BASE_UNROUTED)
        }
        (BlockRef::Reverb, ReverbParams::DUCK) => Some(DUCK_AMOUNT),
        (BlockRef::Reverb, ReverbParams::DUCK_RELEASE) => Some(DUCK_RELEASE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_cut_where_they_end() {
        assert_eq!(VCA_REPLACES[0], Gesture("VCA:", "ROUTES REPLACE CONTOUR"));
        assert_eq!(VCA_BASE[0], Gesture("VCA:", "SUM STARTS AT AMP BASE"));
        assert_eq!(BASE_UNROUTED, "NEEDS A ROUTE INTO VCA");
        assert_eq!(DUCK_AMOUNT, "DUCKS THE TAIL UNDER ITS INPUT");
        assert_eq!(DUCK_RELEASE, "HOW FAST THE TAIL RETURNS");
        assert_eq!(WORDS.len(), 125);
    }
}
