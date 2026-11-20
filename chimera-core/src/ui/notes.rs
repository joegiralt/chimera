//! The band's words for 0.4's tester cells: the matrix's VCA column says
//! its routes are the amp, AMP's BASE says why it is dimmed (ADR 0292),
//! REV › DUCK's cells what they do (ADR 0295), AMP › STEREO's (ADR 0294). Bank 2's (`bank2.x`'s `ui::notes` group): the matrix's words
//! through `UiDoor`'s `notes` entry, a cell's through `band_note`'s; bank
//! 1 keeps the stub in `view`. ALGO's operator cells (ADR 0298): the
//! envelope stages' names in the focus label, RATE SCALE's words and
//! DECAY 1 LEVEL's under a held decay.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::ParamId;
use crate::dsp::algo::params::AlgoOpParams;
use crate::dsp::reverb::ReverbParams;
use crate::modulation::{FixedCol, ModSource, VCA};
use crate::params::OutParams;
use crate::preset::Sound;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::mod_grid::{Col, MatrixState};

/// What bank 1 asks through `UiDoor`'s one `notes` entry, each with its
/// answer's slot.
pub enum Ask<'a> {
    /// `matrix`.
    Matrix(&'a MatrixState, &'a mut Option<Hint>),
    /// `op_label`: the focus label of an operator's cell, into `label`.
    OpLabel {
        param: ParamId,
        page: OpCells,
        label: &'a mut FmtBuf,
    },
}

/// The page an operator's cell is on: one parameter across OP1-OP6, the
/// cells named by operator, or one operator's own, named by parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCells {
    Group,
    Operator,
}

/// `UiDoor`'s entry.
pub(crate) fn ask_in(a: Ask<'_>) {
    match a {
        Ask::Matrix(m, out) => *out = matrix(m),
        Ask::OpLabel { param, page, label } => op_label(param, page, label),
    }
}

/// Each word `WORDS` holds.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum W {
    Vca,
    Replaces,
    SumStarts,
    NeedsRoute,
    DuckAmount,
    DuckRelease,
    StereoPan,
    StereoWidth,
    AttackRate,
    Decay1Rate,
    Decay1Level,
    Decay2Rate,
    ReleaseRate,
    Feedback,
    RateScale,
    D1Holds,
}

/// Each word with its text, in `W`'s order (checked below): one table, so
/// a word and its text can't part. Read only at compile time: the bytes
/// reach the build through `WORDS` alone.
const TABLE: [(W, &str); 16] = [
    (W::Vca, "VCA:"),
    (W::Replaces, "ROUTES REPLACE CONTOUR"),
    (W::SumStarts, "SUM STARTS AT AMP BASE"),
    (W::NeedsRoute, "NEEDS A ROUTE INTO VCA"),
    (W::DuckAmount, "DUCKS THE TAIL UNDER ITS INPUT"),
    (W::DuckRelease, "HOW FAST THE TAIL RETURNS"),
    (W::StereoPan, "PLACES EACH NOTE, ROUTABLE"),
    (W::StereoWidth, "0 MONO, 64 AS PANNED, 127 WIDE"),
    (W::AttackRate, "ATTACK RATE"),
    (W::Decay1Rate, "DECAY 1 RATE"),
    (W::Decay1Level, "DECAY 1 LEVEL"),
    (W::Decay2Rate, "DECAY 2 RATE"),
    (W::ReleaseRate, "RELEASE RATE"),
    (W::Feedback, "FEEDBACK"),
    (W::RateScale, "HIGH NOTES, FASTER ENVELOPES"),
    (W::D1Holds, "DECAY 1 RATE 0: NEVER REACHED"),
];

// Row `i` is word `i`: `word` indexes the table by `W`.
const _: () = {
    let mut i = 0;
    while i < TABLE.len() {
        assert!(TABLE[i].0 as usize == i, "TABLE is in W's order");
        i += 1;
    }
};

/// The texts alone.
const TEXT: [&str; TABLE.len()] = {
    let mut t = [""; TABLE.len()];
    let mut i = 0;
    while i < TABLE.len() {
        t[i] = TABLE[i].1;
        i += 1;
    }
    t
};

/// Where word `i` starts in `WORDS`: the lengths before it.
const fn start(i: usize) -> usize {
    let (mut at, mut j) = (0, 0);
    while j < i {
        at += TEXT[j].len();
        j += 1;
    }
    at
}

const LEN: usize = start(TEXT.len());

/// `TEXT` end to end.
const fn joined() -> [u8; LEN] {
    let mut out = [0u8; LEN];
    let (mut i, mut at) = (0, 0);
    while i < TEXT.len() {
        let b = TEXT[i].as_bytes();
        let mut k = 0;
        while k < b.len() {
            out[at] = b[k];
            at += 1;
            k += 1;
        }
        i += 1;
    }
    out
}

/// The words this module draws, named so they link into bank 2 with it
/// (`.rodata.*notes*`), not into bank 1's unnamed rodata.
static WORDS: [u8; LEN] = joined();

/// Word `w`, cut from `WORDS` where `TEXT` puts it; checked at compile
/// time (`WORDS` is UTF-8).
const fn word(w: W) -> &'static str {
    let i = w as usize;
    let (_, b) = WORDS.split_at(start(i));
    let (b, _) = b.split_at(TEXT[i].len());
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("WORDS is UTF-8"),
    }
}

/// The VCA column with no ENV route: its routes are the whole amp.
pub static VCA_REPLACES: [Gesture<'static>; 1] = [Gesture(word(W::Vca), word(W::Replaces))];
/// The VCA column with an ENV route: the sum starts at AMP's BASE.
pub static VCA_BASE: [Gesture<'static>; 1] = [Gesture(word(W::Vca), word(W::SumStarts))];
/// BASE dimmed: it waits for a route into VCA.
pub static BASE_UNROUTED: &str = word(W::NeedsRoute);
/// REV › DUCK's AMOUNT and RELEASE (ADR 0295).
pub static DUCK_AMOUNT: &str = word(W::DuckAmount);
pub static DUCK_RELEASE: &str = word(W::DuckRelease);
/// AMP › STEREO's PAN and WIDTH (ADR 0294), WIDTH on DRUM's GATE too.
pub static STEREO_PAN: &str = word(W::StereoPan);
pub static STEREO_WIDTH: &str = word(W::StereoWidth);

/// An operator param's name in full, where its page's cell can't say it:
/// the TX81Z's envelope stages (#956, #987) and FDBK.
static OP_NAMES: [(ParamId, &str); 6] = [
    (AlgoOpParams::AR, word(W::AttackRate)),
    (AlgoOpParams::D1R, word(W::Decay1Rate)),
    (AlgoOpParams::D1L, word(W::Decay1Level)),
    (AlgoOpParams::D2R, word(W::Decay2Rate)),
    (AlgoOpParams::RR, word(W::ReleaseRate)),
    (AlgoOpParams::FEEDBACK, word(W::Feedback)),
];

/// `param`'s name in full, if `OP_NAMES` has one.
pub fn op_name(param: ParamId) -> Option<&'static str> {
    OP_NAMES.iter().find(|n| n.0 == param).map(|n| n.1)
}

/// The focus label of an operator's cell where its own label can't say
/// what it edits: an envelope page's cells read OP1-OP6, so the band
/// names the stage (`ATTACK RATE`: the lit cell names the operator, and
/// both don't fit beside the gauge); the operator's own page spells FDBK
/// out. Nothing written: the cell's label stands (FBK's group page, whose
/// header already says FEEDBACK, and every other param).
pub fn op_label(param: ParamId, page: OpCells, label: &mut FmtBuf) {
    use core::fmt::Write;
    let Some(name) = op_name(param) else {
        return;
    };
    match (page, param == AlgoOpParams::FEEDBACK) {
        (OpCells::Group, true) => {}
        (OpCells::Group, false) | (OpCells::Operator, true | false) => {
            let _ = label.write_str(name);
        }
    }
}

/// RATE SCALE (#988): what it does, as on the TX81Z.
pub static RATE_SCALE: &str = word(W::RateScale);
/// DECAY 1 LEVEL under a DECAY 1 RATE of 0 (#987): the envelope holds
/// at full, so the level it would fall to is never reached.
pub static D1_HOLDS: &str = word(W::D1Holds);

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
/// 2's `walk::band_note_in` asks it after TAPE's notes. Out of line, so it
/// stays in the notes' S4 group, not inlined into the walk's S0-S3 code.
#[inline(never)]
pub fn cell(addr: ParamAddr, s: &Sound) -> Option<&'static str> {
    match (addr.block, addr.param) {
        (BlockRef::Out, OutParams::VCA) => {
            (s.mod_state.routes_into(VCA) == 0).then_some(BASE_UNROUTED)
        }
        (BlockRef::Reverb, ReverbParams::DUCK) => Some(DUCK_AMOUNT),
        (BlockRef::Reverb, ReverbParams::DUCK_RELEASE) => Some(DUCK_RELEASE),
        (BlockRef::Out, OutParams::PAN) => Some(STEREO_PAN),
        (BlockRef::Out, OutParams::WIDTH) => Some(STEREO_WIDTH),
        (BlockRef::AlgoOp(_), AlgoOpParams::RATE_SCALE) => Some(RATE_SCALE),
        // D1L 15 meets D1 at once and D2 runs (`OpEnv`).
        (BlockRef::AlgoOp(op), AlgoOpParams::D1L) => {
            let o = &s.params.algo.ops[op.index()];
            (o.d1r == 0 && o.d1l < 15).then_some(D1_HOLDS)
        }
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
        assert_eq!(STEREO_PAN, "PLACES EACH NOTE, ROUTABLE");
        assert_eq!(STEREO_WIDTH, "0 MONO, 64 AS PANNED, 127 WIDE");
        assert_eq!(op_name(AlgoOpParams::D1L), Some("DECAY 1 LEVEL"));
        assert_eq!(op_name(AlgoOpParams::FEEDBACK), Some("FEEDBACK"));
        assert_eq!(op_name(AlgoOpParams::LEVEL), None);
        assert_eq!(RATE_SCALE, "HIGH NOTES, FASTER ENVELOPES");
        assert_eq!(D1_HOLDS, "DECAY 1 RATE 0: NEVER REACHED");
        assert_eq!(WORDS.len(), 181 + 68 + 57);
        assert_eq!(
            STEREO_WIDTH.as_ptr(),
            WORDS[151..].as_ptr(),
            "cut from WORDS"
        );
    }
}
