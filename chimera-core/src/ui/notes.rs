//! The band's words for 0.4's tester cells: the matrix's VCA column says
//! its routes are the amp, AMP's BASE says why it is dimmed (ADR 0292),
//! REV › DUCK's cells what they do (ADR 0295), AMP › STEREO's (ADR 0294). Bank 2's (`bank2.x`'s `ui::notes` group): the matrix's words
//! through `UiDoor`'s `notes` entry, a cell's through `band_note`'s; bank
//! 1 keeps the stub in `view`. ALGO's operator cells (ADR 0298): the
//! envelope stages' names in the focus label, RATE SCALE's words and
//! DECAY 1 LEVEL's under a held decay, the ratio CRSE, FINE and DETUN
//! come to, the chart's feedback loops; ALGO PAGES' operator-first
//! chains (ADR 0300). The compressor's KEY (ADR 0303): what each choice
//! listens to, on MST's status line.

use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
use crate::comp_key::{CompKey, KEY_NAMES, KEYS};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams, Fdbk};
use crate::dsp::algo::plan::OPS;
use crate::dsp::algo::{kinds, tx};
use crate::dsp::comp::CompParams;
use crate::dsp::modulator::EnvType;
use crate::dsp::modulator::law::speed_ranges;
use crate::dsp::reverb::ReverbParams;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource, VCA};
use crate::params::OutParams;
use crate::preset::Sound;
use crate::project::Line;
use crate::ui::alg_layout::AlgLayout;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, SlotBinding};
use crate::ui::block_registry as reg;
use crate::ui::fmt::{FmtBuf, fixed};
use crate::ui::hint::{Gesture, Hint};
use crate::ui::mod_grid::{Col, MatrixState};
use crate::ui::renderer::Frame;
use crate::ui::tape_strip::Paint;
use crate::ui::theme;
use embedded_graphics::pixelcolor::Rgb565;

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
    /// `ratio`: where CRSE, FINE and DETUN put operator `op`.
    Ratio {
        algo: &'a AlgoParams,
        op: Op,
        param: ParamId,
        out: &'a mut Option<Line>,
    },
    /// `loops`: the chart's feedback loops, under its nodes, clear of
    /// the `links` drawn (bit `dst` of `links[src]`).
    Loops {
        on: &'a mut dyn Paint,
        algo: &'a AlgoParams,
        at: &'a AlgLayout,
        links: [u8; OPS],
    },
    /// ALGO's chain with OSC laid out by operator (ADR 0300), with
    /// FORMANT's page or without.
    OpsChain {
        formant: bool,
        out: &'a mut Option<&'static ChainDef2>,
    },
    /// `key`: KEY's hint on the frame's status line.
    Key(&'a Frame<'a>, &'a mut Option<Hint>),
    /// `stage`: an ENV A stage cell's bar under the TIME or LEVEL routes
    /// it primes (#149); `None` for any other cell, or none routed.
    Stage {
        addr: ParamAddr,
        value: f32,
        sound: &'a Sound,
        sources: &'a [f32; MAX_MOD_SOURCES],
        out: &'a mut Option<f32>,
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
        Ask::Ratio {
            algo,
            op,
            param,
            out,
        } => *out = ratio(algo, op, param),
        Ask::Loops {
            on,
            algo,
            at,
            links,
        } => loops(on, algo, at, &links),
        Ask::OpsChain { formant, out } => *out = Some(ops_chain(formant)),
        Ask::Key(f, out) => *out = key(f),
        Ask::Stage {
            addr,
            value,
            sound,
            sources,
            out,
        } => *out = stage(addr, value, sound, sources),
    }
}

/// An ENV A stage cell's bar as the envelope plays it (#149): ATTACK,
/// DECAY, RELEASE and HOLD × 2^(−5·Σ) of the slot's TIME routes, as
/// `ACoefs` takes them, in each law's octaves; SUS × the peak a LEVEL
/// route sets. The cells prime those (`prime_target`), so no route is
/// their own.
fn stage(
    addr: ParamAddr,
    value: f32,
    sound: &Sound,
    sources: &[f32; MAX_MOD_SOURCES],
) -> Option<f32> {
    use crate::params::EnvParams as E;
    let BlockRef::Env(slot) = addr.block else {
        return None;
    };
    let e = &sound.params.envelopes[slot.index()];
    if e.env_type != EnvType::A {
        return None;
    }
    let m = &sound.mod_state;
    let routed = |p: ParamId| {
        let a = ParamAddr::new(addr.block, p);
        m.find(a)
            .filter(|&d| m.present(d) != 0)
            .map(|_| m.offset_for(a, sources))
    };
    let r = speed_ranges(e.speed);
    let oct = match addr.param {
        E::ATTACK => r.attack.oct,
        E::HOLD => r.hold.oct,
        E::DECAY | E::RELEASE => r.dec_rel.oct,
        E::SUSTAIN => {
            let peak = routed(E::LEVEL)?;
            return Some(value * peak.clamp(0.0, 1.0));
        }
        _ => return None,
    };
    let sum = routed(E::TIME)?.clamp(-1.0, 1.0);
    Some((value - 5.0 * sum / oct).clamp(0.0, 1.0))
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
    Ratio,
    Detune,
    Cents,
    KeyFull,
    KeyLow,
    KeyHigh,
    KeyHit,
    KeyPart,
}

/// Each word with its text, in `W`'s order (checked below): one table, so
/// a word and its text can't part. Read only at compile time: the bytes
/// reach the build through `WORDS` alone.
const TABLE: [(W, &str); 24] = [
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
    (W::Ratio, "RATIO "),
    (W::Detune, ", DETUNE "),
    (W::Cents, " CENTS"),
    (W::KeyFull, "HEARS THE WHOLE MIX"),
    (W::KeyLow, "THE KICK PUMPS THE MIX"),
    (W::KeyHigh, "HEARS ALL BUT THE BASS"),
    (W::KeyHit, "EVERY HIT PUSHES IT ALIKE"),
    (W::KeyPart, "DUCKS ALL BUT ITSELF"),
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
    let mut k = 0;
    while k < OP_NAMES.len() {
        if OP_NAMES[k].0 == param {
            return Some(OP_NAMES[k].1);
        }
        k += 1;
    }
    None
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

/// What the band says on operator `op`'s CRSE, FINE or DETUN (#986): the
/// ratio CRSE and FINE give, from the engine's own tables (`tx::ratio`),
/// and DETUN in cents (`tx::DETUNE_CENTS` a step): `RATIO 1.41`,
/// `RATIO 1.41, DETUNE +4.5 CENTS`. None on any other param, or on a
/// noise band, which has no pitch.
pub fn ratio(algo: &AlgoParams, op: Op, param: ParamId) -> Option<Line> {
    use core::fmt::Write;
    let tuned = matches!(
        param,
        AlgoOpParams::COARSE | AlgoOpParams::FINE | AlgoOpParams::DETUNE
    );
    if !tuned || !kinds::reads(algo.kinds().get(op.index()), param) {
        return None;
    }
    let o = &algo.ops[op.index()];
    let mut b = FmtBuf::new();
    let _ = b.write_str(word(W::Ratio));
    fixed(&mut b, tx::ratio(o.coarse, o.fine), 2, "");
    if o.detune != 0 {
        let _ = b.write_str(word(W::Detune));
        if o.detune > 0 {
            let _ = b.write_char('+');
        }
        fixed(
            &mut b,
            f32::from(o.detune) * tx::DETUNE_CENTS,
            1,
            word(W::Cents),
        );
    }
    Some(Line::new(b.as_str()))
}

/// How hard an operator feeds back, as its loop is drawn (#989).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loop {
    /// FDBK 0, or a noise band, which has none.
    None,
    /// Grit: under the TX81Z's step 4, a quarter cycle.
    Soft,
    /// From step 4: the sine bends toward a saw, then noise.
    Hard,
}

/// The step a loop turns `Hard` at: the TX81Z's 4.
const HARD: Fdbk = Fdbk::tx(4);

impl Loop {
    /// Operator `op` of `algo`.
    pub fn of(algo: &AlgoParams, op: Op) -> Loop {
        let i = op.index();
        let fdbk = algo.ops[i].feedback;
        if fdbk.is_off() || !kinds::reads(algo.kinds().get(i), AlgoOpParams::FEEDBACK) {
            Loop::None
        } else if fdbk < HARD {
            Loop::Soft
        } else {
            Loop::Hard
        }
    }

    /// The loop's colour; none draws nothing.
    pub const fn color(self) -> Option<Rgb565> {
        match self {
            Loop::None => None,
            Loop::Soft => Some(theme::MID),
            Loop::Hard => Some(theme::ACCENT),
        }
    }
}

/// A loop's reach past its node's rim, and its half-height: an ear
/// `LOOP_R` round, its centre `LOOP_OUT` outside the rim.
pub const LOOP_R: i32 = 3;
pub const LOOP_OUT: i32 = 2;

/// The ear's outline round its centre, joined in order and closed. A
/// static, so it links into bank 2 with the module.
static EAR: [(i8, i8); 8] = [
    (-1, -3),
    (1, -3),
    (3, -1),
    (3, 1),
    (1, 3),
    (-1, 3),
    (-3, 1),
    (-3, -1),
];

/// A drawn link (`links[src]` bit `dst`) runs through the ear centred
/// `(cx, cy)`, outside operator `own`'s node, which covers what is under
/// it. Plain loops, here and in `loop_centre`: an iterator adaptor's
/// instance is `core`'s by name, which the linker keeps in bank 1, calling
/// past the door.
fn crossed(at: &AlgLayout, links: &[u8; OPS], own: usize, (cx, cy): (i32, i32)) -> bool {
    const STEPS: i32 = 48;
    let reach = LOOP_R + 1;
    let (ox, oy) = at.pos[own];
    let mut src = 0;
    while src < OPS {
        let mut dst = 0;
        while dst < OPS {
            if links[src] & (1 << dst) != 0 {
                let ((x0, y0), (x1, y1)) = (at.pos[src], at.pos[dst]);
                let mut k = 0;
                while k <= STEPS {
                    let (x, y) = (x0 + (x1 - x0) * k / STEPS, y0 + (y1 - y0) * k / STEPS);
                    let under = (x - ox) * (x - ox) + (y - oy) * (y - oy) <= at.r * at.r;
                    if !under && (x - cx).abs() <= reach && (y - cy).abs() <= reach {
                        return true;
                    }
                    k += 1;
                }
            }
            dst += 1;
        }
        src += 1;
    }
    false
}

/// An ear centred `(cx, cy)` is inside the viz band.
fn inside((cx, cy): (i32, i32)) -> bool {
    cx - LOOP_R >= theme::VIZ_LEFT
        && cx + LOOP_R <= theme::VIZ_RIGHT
        && cy - LOOP_R > theme::VIZ_BAND_TOP
        && cy + LOOP_R < theme::VIZ_BAND_BOTTOM
}

/// Where operator `op`'s loop is centred on `at`: beside its node, on the
/// first of right, left, above and below that stays in the band and
/// clear of every drawn link; else the first in the band; else the right.
pub fn loop_centre(at: &AlgLayout, op: Op, links: &[u8; OPS]) -> (i32, i32) {
    let (x, y) = at.pos[op.index()];
    let out = at.r + LOOP_OUT;
    let sides = [(x + out, y), (x - out, y), (x, y - out), (x, y + out)];
    let mut fits = None;
    let mut k = 0;
    while k < sides.len() {
        if inside(sides[k]) {
            if !crossed(at, links, op.index(), sides[k]) {
                return sides[k];
            }
            if fits.is_none() {
                fits = Some(sides[k]);
            }
        }
        k += 1;
    }
    match fits {
        Some(c) => c,
        None => sides[0],
    }
}

/// The chart's feedback loops (#989): an ear on each operator that feeds
/// back into itself, as every operator can here, brighter from `HARD`.
/// Drawn before the nodes, which cover each ear's inner half.
pub fn loops(on: &mut dyn Paint, algo: &AlgoParams, at: &AlgLayout, links: &[u8; OPS]) {
    for op in Op::ALL {
        let Some(c) = Loop::of(algo, op).color() else {
            continue;
        };
        let (cx, cy) = loop_centre(at, op, links);
        let p = |k: usize| {
            let (dx, dy) = EAR[k % EAR.len()];
            (cx + i32::from(dx), cy + i32::from(dy))
        };
        for k in 0..EAR.len() {
            let ((x0, y0), (x1, y1)) = (p(k), p(k + 1));
            on.line(x0, y0, x1, y1, c);
        }
    }
}

/// OSC's sub-pages by operator (ADR 0300): OP2-OP6 after OP1, the node's
/// home, then WAVE and the group pages in their order. Built from
/// `ALGO_OSC_SUB_PAGES`, so every page of it is here. Statics, so the
/// tables link into bank 2 with this module.
const OPS_SUBS: usize = reg::ALGO_OSC_SUB_PAGES.len();
const OWN: usize = reg::ALGO_OP_PAGES.len();
static ALGO_OPS_OSC_SUB_PAGES: [&BlockDef; OPS_SUBS] = {
    let mut a = [&reg::ALGO_WAVE; OPS_SUBS];
    let mut i = 1;
    while i < OWN {
        a[i - 1] = &reg::ALGO_OP_PAGES[i];
        i += 1;
    }
    // a[OWN - 1] is WAVE; the groups follow it.
    let mut g = 0;
    while g < OPS_SUBS - OWN {
        a[OWN + g] = reg::ALGO_OSC_SUB_PAGES[g];
        g += 1;
    }
    a
};

/// The same with FORMANT's page after WAVE, as it follows WAVE in
/// PARAMS' order.
static ALGO_OPS_FORMANT_OSC_SUB_PAGES: [&BlockDef; OPS_SUBS + 1] = {
    let mut a = [&reg::ALGO_FORMANT; OPS_SUBS + 1];
    let mut i = 0;
    while i < OPS_SUBS {
        a[if i < OWN { i } else { i + 1 }] = ALGO_OPS_OSC_SUB_PAGES[i];
        i += 1;
    }
    a
};

/// ALGO's nodes with OSC opening on OP1, still OSC on the map.
const fn ops_blocks(subs: &'static [&'static BlockDef]) -> [ChainBlock; 5] {
    let mut b = [reg::ALGO_BLOCKS[0]; 5];
    let mut i = 1;
    while i < 5 {
        b[i] = reg::ALGO_BLOCKS[i];
        i += 1;
    }
    b[1] = ChainBlock {
        def: &reg::ALGO_OP_PAGES[0],
        sub_pages: subs,
        map: Some(reg::ALGO_WAVE.short),
    };
    b
}

static ALGO_OPS_BLOCKS: [ChainBlock; 5] = ops_blocks(&ALGO_OPS_OSC_SUB_PAGES);
static ALGO_OPS_FORMANT_BLOCKS: [ChainBlock; 5] = ops_blocks(&ALGO_OPS_FORMANT_OSC_SUB_PAGES);

/// ALGO's chain under ALGO PAGES' OPS, and while an operator is a
/// formant or a noise band.
pub static ALGO_OPS_CHAIN: ChainDef2 = ChainDef2::new(
    reg::ALGO_CHAIN.name,
    &ALGO_OPS_BLOCKS,
    &reg::PART_MOD_SOURCES,
);
pub static ALGO_OPS_FORMANT_CHAIN: ChainDef2 = ChainDef2::new(
    reg::ALGO_CHAIN.name,
    &ALGO_OPS_FORMANT_BLOCKS,
    &reg::PART_MOD_SOURCES,
);

/// The operator-first chain, with FORMANT's page or without.
pub fn ops_chain(formant: bool) -> &'static ChainDef2 {
    if formant {
        &ALGO_OPS_FORMANT_CHAIN
    } else {
        &ALGO_OPS_CHAIN
    }
}

/// What each of KEY's choices listens to (ADR 0303), by its code: the
/// choice's name, then what it does to the mix.
static KEY_HINTS: [[Gesture<'static>; 1]; KEYS] = {
    let mut h = [[Gesture("", "")]; KEYS];
    let mut c = 0;
    while c < KEYS {
        let does = match CompKey::ALL[c] {
            CompKey::Full => word(W::KeyFull),
            CompKey::Low => word(W::KeyLow),
            CompKey::High => word(W::KeyHigh),
            CompKey::Hit => word(W::KeyHit),
            CompKey::Part(_) => word(W::KeyPart),
        };
        h[c] = [Gesture(KEY_NAMES[c], does)];
        c += 1;
    }
    h
};

/// `key`'s hint.
pub fn key_hint(key: CompKey) -> Hint {
    &KEY_HINTS[usize::from(key.code())]
}

/// MST's status line while KEY's cell is the one touched: what its
/// choice listens to. None on any other cell or page, or with HINTS off.
pub fn key(f: &Frame<'_>) -> Option<Hint> {
    const KEY: ParamAddr = ParamAddr::new(BlockRef::Comp, CompParams::KEY);
    let on_key = matches!(f.def.params.get(f.focus)?.binding, SlotBinding::Param(a) if a == KEY);
    (on_key && f.hints).then(|| key_hint(f.fx.comp.key))
}

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
        (BlockRef::Delay, id) => crate::ui::delay_sync::word(id),
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
        assert_eq!(WORDS.len(), 181 + 68 + 57 + 21 + 108);
        assert_eq!(
            STEREO_WIDTH.as_ptr(),
            WORDS[151..].as_ptr(),
            "cut from WORDS"
        );
    }
}
