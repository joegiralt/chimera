//! What a Sound is, for the owner's page: its engine, its engine's key
//! settings, its filter and every route, read from the built `Sound`.
//! Values read as the unit shows them (`view::shown_fmt`, the pages'
//! cells); labels are the specs' and pages', abbreviations spelled out.

use chimera_core::addr::{BlockRead, BlockRef, Op, ParamAddr};
use chimera_core::block::ValFmt;
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modal::{EXCITER_NAMES, ModalPage, ModalParams, page_cells};
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::dsp::va::VaParams;
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::{EngineType, EnvParams, FilterParams, PitchParams, Steal};
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::taps::TapOp;
use chimera_core::ui::block_def::{BlockDef, SlotBinding};
use chimera_core::ui::block_registry::{MODAL_1, VA_HOME, VA_OSC1, VA_OSC2};
use chimera_core::ui::filter_panel;
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::mod_panel::{PanelSlot, env_panel};
use chimera_core::ui::view::{LfoKind, SlotCtx, dimmed, shown_fmt};

use super::source::{Source, to_sound};

/// A labelled fact and its lines: `("FILTER", ["2040 · LP4 · …"])`.
pub type Fact = (&'static str, Vec<String>);

/// The unit's short label words, spelled out where a page has the room
/// (the specs carry no longer label).
pub const WORDS: [(&str, &str); 17] = [
    ("AMNT", "BLEND"),
    ("STAB", "STABILITY"),
    ("CRSE", "COARSE"),
    ("RESO", "RESONANCE"),
    ("STRUCT", "STRUCTURE"),
    ("POS", "POSITION"),
    ("BRIGHT", "BRIGHTNESS"),
    ("DAMP", "DAMPING"),
    ("SYM", "SYMMETRY"),
    ("ATK", "ATTACK"),
    ("DEC", "DECAY"),
    ("SUS", "SUSTAIN"),
    ("REL", "RELEASE"),
    ("ENS.D", "ENSEMBLE DEPTH"),
    ("ENS.R", "ENSEMBLE RATE"),
    ("ENS.M", "ENSEMBLE MIX"),
    ("TRNSP", "TRANSPOSE"),
];

/// The unit's short value words (a curve, an operator's wave), spelled
/// out word by word in any value.
pub const VALUES: [(&str, &str); 19] = [
    ("LIN", "LINEAR"),
    ("LOG", "LOGARITHMIC"),
    ("EXP", "EXPONENTIAL"),
    ("SYMP", "SYMPATHETIC"),
    ("TRI", "TRIANGLE"),
    ("SQR", "SQUARE"),
    ("W1", "SINE"),
    ("W2", "SHARP SINE"),
    ("W3", "HALF-WAVE SINE"),
    ("W4", "HALF-WAVE SHARP SINE"),
    ("W5", "HALF-CYCLE SINE"),
    ("W6", "HALF-CYCLE SHARP SINE"),
    ("W7", "HALF-CYCLE DOUBLED SINE"),
    ("W8", "HALF-CYCLE DOUBLED SHARP SINE"),
    ("P25", "PULSE 25%"),
    ("P12", "PULSE 12.5%"),
    ("TSAW", "TILTED SAW"),
    ("RSQR", "ROUNDED SQUARE"),
    ("SSAW", "SOFT SAW"),
];

/// `WAVE1` → `WAVE 1`, `AMNT` → `BLEND`.
fn full(label: &str) -> String {
    let (word, n) = label.split_at(label.trim_end_matches(|c: char| c.is_ascii_digit()).len());
    let word = WORDS.iter().find(|w| w.0 == word).map_or(word, |w| w.1);
    if n.is_empty() || word.is_empty() {
        format!("{word}{n}")
    } else {
        format!("{word} {n}")
    }
}

/// A shown value with its short words spelled out: `LIN` → `LINEAR`.
fn spelled(v: &str) -> String {
    v.split(' ')
        .map(|t| VALUES.iter().find(|w| w.0 == t).map_or(t, |w| w.1))
        .collect::<Vec<_>>()
        .join(" ")
}

fn text(v: f32, fmt: ValFmt) -> String {
    let mut b = FmtBuf::new();
    fmt_val(&mut b, v, fmt);
    b.as_str().into()
}

fn get(snd: &Sound, a: ParamAddr) -> f32 {
    snd.params.block(a.block).map_or(0.0, |b| b.get(a.param))
}

/// `a`'s value as the unit shows it on `snd`'s pages.
fn shown(snd: &Sound, ctx: &SlotCtx, a: ParamAddr) -> String {
    match (a.spec(), shown_fmt(a, ctx)) {
        (Some(spec), Some(fmt)) => spelled(&text(spec.normalize(get(snd, a)), fmt)),
        _ => "?".into(),
    }
}

/// A param's label and value, `BLEND 44`.
fn cell(snd: &Sound, ctx: &SlotCtx, label: &str, a: ParamAddr) -> String {
    format!("{} {}", full(label), shown(snd, ctx, a))
}

/// A page's bound cells, `keep` choosing which.
fn page(
    snd: &Sound,
    ctx: &SlotCtx,
    def: &BlockDef,
    keep: impl Fn(ParamAddr) -> bool,
) -> Vec<String> {
    def.params
        .iter()
        .filter_map(|s| match s.binding {
            SlotBinding::Param(a) if keep(a) => Some(cell(snd, ctx, s.label(), a)),
            _ => None,
        })
        .collect()
}

/// A VA oscillator's own page cell, `(1, COARSE)`, as OSC 1 and OSC 2
/// label it.
fn osc_cell(a: ParamAddr) -> Option<(u8, &'static str)> {
    [(1, &VA_OSC1), (2, &VA_OSC2)]
        .into_iter()
        .find_map(|(n, def)| {
            def.params
                .iter()
                .find(|s| s.binding == SlotBinding::Param(a))
                .map(|s| (n, s.label()))
        })
}

/// A destination in words: `CUTOFF`, `WAVE 1`, `OSC 2 FINE`, `OP 5
/// LEVEL`, `PITCH FINE`.
fn dest(a: ParamAddr) -> String {
    if let Some((n, label)) = osc_cell(a) {
        return format!("OSC {n} {}", full(label));
    }
    let label = a.spec().map_or("?", |s| s.label);
    let word = full(label);
    match a.block {
        BlockRef::AlgoOp(op) => format!("OP {} {word}", op.index() + 1),
        BlockRef::Env(s) => format!("ENV {} {word}", s.index() + 1),
        BlockRef::Lfo(s) => format!("LFO {} {word}", s.index() + 1),
        b if matches!(label, "LEVEL" | "MIX" | "FINE" | "TONE") => {
            let block = match b {
                BlockRef::Filter => "FILTER ",
                BlockRef::Out => "OUT ",
                BlockRef::Pitch => "PITCH ",
                BlockRef::Folder => "FOLD ",
                BlockRef::Drive => "DRIVE ",
                _ => "",
            };
            format!("{block}{word}")
        }
        _ => word,
    }
}

/// A tap's column: the filter's FREQ or Q, or the VCA or FOLD.
fn column(c: FixedCol) -> String {
    match c {
        FixedCol::Freq => "FILTER FREQUENCY".into(),
        FixedCol::Q => "FILTER RESONANCE".into(),
        _ => c.tag().into(),
    }
}

/// A source with its settings as its page shows them: `LFO 1 (SINE, RATE
/// 3 = 0.5 Hz)`, `ENV 2 (ATTACK 26 ms · …)`, `VELOCITY`.
fn modulator(snd: &Sound, ctx: &SlotCtx, src: ModSource) -> String {
    if let Some(e) = src.env_slot() {
        let b = BlockRef::Env(e);
        let cells: Vec<String> = env_panel(ctx.envs[e.index()])
            .slots
            .iter()
            .flatten()
            .filter_map(|s| match *s {
                PanelSlot::Param {
                    id,
                    label,
                    fmt: ValFmt::Law(_),
                } => {
                    let a = ParamAddr::new(b, id);
                    // A HOLD of 0 holds nothing.
                    (id != EnvParams::HOLD || get(snd, a) != 0.0)
                        .then(|| format!("{label} {}", shown(snd, ctx, a)))
                }
                _ => None,
            })
            .collect();
        return format!("{} ({})", src.name(), cells.join(" · "));
    }
    let Some(l) = LfoSlot::ALL
        .into_iter()
        .find(|&l| ModSource::of_lfo(l) == src)
    else {
        return src.name().into();
    };
    let b = BlockRef::Lfo(l);
    let at = |id| ParamAddr::new(b, id);
    match ctx.lfos[l.index()] {
        LfoKind::Classic => {
            // The wave by its full name (TRIANGLE, not TRI).
            let shape = snd
                .params
                .block(b)
                .and_then(|blk| blk.enum_ident(LfoParams::SHAPE))
                .unwrap_or("?");
            let retrig = if shown(snd, ctx, at(LfoParams::SYNC)) == "RETRIG" {
                ", RETRIG"
            } else {
                ""
            };
            format!(
                "{} ({shape}, RATE {} = {} Hz{retrig})",
                src.name(),
                shown(snd, ctx, at(LfoParams::RATE)),
                snd.params.lfos[l.index()].rate
            )
        }
        LfoKind::Func(_) => format!(
            "{} (FUNC, RATE {})",
            src.name(),
            shown(snd, ctx, at(LfoParams::RISE))
        ),
    }
}

/// Where `src` routes in `snd`'s matrix, with each amount (none at 0).
fn dests(snd: &Sound, src: ModSource) -> Vec<(ParamAddr, i8)> {
    let m = &snd.mod_state;
    let (i, bit) = (src.index(), 1 << src.index());
    if i >= m.num_sources() {
        return Vec::new();
    }
    let fixed = FixedCol::ALL
        .into_iter()
        .filter(|&c| m.fixed_present(c) & bit != 0)
        .map(|c| (c.addr(), m.fixed_amount(i, c)));
    let reg = (0..m.num_dests())
        .filter(|&d| m.present(d) & bit != 0)
        .map(|d| (m.dest(d), m.amount(i, d)));
    fixed.chain(reg).filter(|&(_, a)| a != 0).collect()
}

/// Each routed CLASSIC LFO at SYNC FREE, which plays its Part's clock
/// (ADR 0135): `LFO 2 on CUTOFF`, `LFO 1 on OSC 1 SHAPE, OSC 2 SHAPE`.
pub fn free_lfos(snd: &Sound) -> Vec<String> {
    let ctx = SlotCtx::read(&snd.params, Op::A);
    LfoSlot::ALL
        .into_iter()
        .filter(|l| ctx.lfos[l.index()] == LfoKind::Classic && snd.params.lfos[l.index()].sync == 0)
        .filter_map(|l| {
            let to: Vec<String> = dests(snd, ModSource::of_lfo(l))
                .into_iter()
                .map(|(d, _)| dest(d))
                .collect();
            (!to.is_empty()).then(|| format!("LFO {} on {}", l.index() + 1, to.join(", ")))
        })
        .collect()
}

/// Every matrix route, a source a line, then each tap, STABILITY's drift
/// and the Part's glide.
fn routes(snd: &Sound, ctx: &SlotCtx, demo: PartMode) -> Vec<String> {
    use ModSource::*;
    let mut out = Vec::new();
    for src in [Env1, Env2, Env3, Lfo1, Lfo2, Lfo3, Vel, Note] {
        let to: Vec<String> = dests(snd, src)
            .into_iter()
            .map(|(d, a)| format!("{} {a:+}", dest(d)))
            .collect();
        if !to.is_empty() {
            out.push(format!("{} → {}", modulator(snd, ctx, src), to.join(", ")));
        }
    }
    for op in TapOp::ALL {
        let to: Vec<String> = snd
            .params
            .taps
            .entries()
            .filter(|e| e.0 == op && e.2 != 0)
            .map(|(_, c, a)| format!("{} {a:+}", column(c)))
            .collect();
        if !to.is_empty() {
            out.push(format!("{} → {} (audio rate)", op.name(), to.join(", ")));
        }
    }
    if snd.engine() == EngineType::Va && snd.params.va.stability < 1.0 {
        out.push(format!(
            "STABILITY {}: the oscillators drift (127 holds them steady)",
            shown(snd, ctx, ParamAddr::new(BlockRef::Va, VaParams::STAB))
        ));
    }
    let glide = (snd.params.pitch.steal == Steal::Glide).then(|| {
        let t = ParamAddr::new(BlockRef::Pitch, PitchParams::GLIDE_TIME);
        format!("GLIDE {}", shown(snd, ctx, t))
    });
    match (demo, glide) {
        (PartMode::Mono, Some(g)) => out.push(format!("MONO PART, {g}: overlapping notes slide")),
        (PartMode::Mono, None) => out.push("MONO PART".into()),
        (_, Some(g)) => out.push(g),
        _ => {}
    }
    out
}

/// The kind, its mode and its voice where it has one, then cutoff and
/// resonance: `2040 · LP4 · REV2 · CUTOFF 1.20 kHz · RESONANCE 38`.
fn filter(snd: &Sound, ctx: &SlotCtx) -> String {
    let f = |id| ParamAddr::new(BlockRef::Filter, id);
    let mut v = vec![
        shown(snd, ctx, f(FilterParams::KIND)),
        shown(snd, ctx, f(FilterParams::MODE)),
    ];
    if filter_panel::applies(ctx.kind, FilterParams::VOICE) {
        v.push(shown(snd, ctx, f(FilterParams::VOICE)));
    }
    v.push(cell(snd, ctx, "CUTOFF", f(FilterParams::CUTOFF)));
    v.push(cell(snd, ctx, "RESO", f(FilterParams::RESONANCE)));
    v.join(" · ")
}

/// An operator's wave, ratio and level, `(SAW, RATIO 1.00, LEVEL 99)`, or
/// `(OFF)` at level 0.
fn op(snd: &Sound, ctx: &SlotCtx, o: Op) -> String {
    let at = |id| ParamAddr::new(BlockRef::AlgoOp(o), id);
    if get(snd, at(AlgoOpParams::LEVEL)) == 0.0 {
        return "(OFF)".into();
    }
    format!(
        "({}, RATIO {}, LEVEL {})",
        shown(snd, ctx, at(AlgoOpParams::WAVE)),
        shown(snd, ctx, at(AlgoOpParams::COARSE)),
        shown(snd, ctx, at(AlgoOpParams::LEVEL))
    )
}

/// The engine's key settings.
fn engine(snd: &Sound, ctx: &SlotCtx) -> Vec<Fact> {
    match snd.engine() {
        EngineType::Va => {
            let neutral = Sound::neutral(EngineType::Va);
            let moved = |a: ParamAddr| get(snd, a) != get(&neutral, a);
            let oscs: Vec<String> = [(1, &VA_OSC1), (2, &VA_OSC2)]
                .into_iter()
                .flat_map(|(n, def)| {
                    page(snd, ctx, def, moved)
                        .into_iter()
                        .map(move |c| format!("OSC {n} {c}"))
                })
                .collect();
            let mut v = vec![page(snd, ctx, &VA_HOME, |_| true).join(" · ")];
            if !oscs.is_empty() {
                v.push(oscs.join(" · "));
            }
            vec![("VIRTUAL ANALOG", v)]
        }
        EngineType::Modal => {
            let mode = snd.params.modal.mode;
            let m = |id| ParamAddr::new(BlockRef::Modal, id);
            let exc: Vec<String> = page_cells(ModalPage::Exciter, mode)
                .into_iter()
                .flatten()
                .map(|id| cell(snd, ctx, m(id).spec().map_or("?", |s| s.label), m(id)))
                .collect();
            let model = format!(
                "MODEL {} · EXCITER {}: {}",
                shown(snd, ctx, m(ModalParams::MODE)),
                EXCITER_NAMES[mode as usize],
                exc.join(" · ")
            );
            // The macros MODEL reads (BOWED has no STRUCTURE).
            let macros = page(snd, ctx, &MODAL_1, |a| {
                a != m(ModalParams::MODE) && !dimmed(a, snd)
            });
            vec![("MODAL", vec![model, macros.join(" · ")])]
        }
        EngineType::Algo => {
            let (alg_a, alg_b) = snd.params.algo.algs();
            let alg = alg_a.algorithm();
            let a = |id| ParamAddr::new(BlockRef::Algo, id);
            let mut head = format!("ALGORITHM {}", shown(snd, ctx, a(AlgoParams::ALG_A)));
            if alg_b != alg_a {
                head += &format!(
                    ", MORPH {} toward {}",
                    shown(snd, ctx, a(AlgoParams::MORPH)),
                    shown(snd, ctx, a(AlgoParams::ALG_B))
                );
            }
            let carriers: Vec<String> = Op::ALL
                .into_iter()
                .filter(|o| alg.carriers & (1 << o.index()) != 0)
                .map(|o| format!("OP {} {}", o.index() + 1, op(snd, ctx, o)))
                .collect();
            let mods: Vec<String> = Op::ALL
                .into_iter()
                .flat_map(|from| {
                    Op::ALL
                        .into_iter()
                        .filter(move |to| alg.mods[from.index()] & (1 << to.index()) != 0)
                        .map(move |to| (from, to))
                })
                .map(|(from, to)| {
                    let (f, t) = (from.index() + 1, to.index() + 1);
                    format!("OP {f} → OP {t} {}", op(snd, ctx, from))
                })
                .collect();
            vec![(
                "FM ALGO",
                vec![
                    head,
                    format!("CARRIERS: {}", carriers.join(", ")),
                    format!("MODULATORS: {}", mods.join(", ")),
                ],
            )]
        }
        _ => Vec::new(),
    }
}

/// What `snd`, played on a `demo` Part, is.
pub fn about(snd: &Sound, demo: PartMode) -> Vec<Fact> {
    let ctx = SlotCtx::read(&snd.params, Op::A);
    let name = match snd.engine() {
        EngineType::Va => "VIRTUAL ANALOG",
        EngineType::Modal => "MODAL",
        EngineType::Algo => "FM ALGO",
        _ => "TAPE",
    };
    let mut v: Vec<Fact> = vec![("ENGINE", vec![name.into()])];
    v.extend(engine(snd, &ctx));
    v.push(("FILTER", vec![filter(snd, &ctx)]));
    v.push(("MODULATED", routes(snd, &ctx, demo)));
    v
}

/// `about` the Sound `src` builds.
pub fn of(src: &Source) -> Result<Vec<Fact>, String> {
    Ok(about(&to_sound(src)?, src.demo))
}
