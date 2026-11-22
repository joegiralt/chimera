//! Direction A shared components (UI refresh spec § Shared components):
//! header, focus band, cells. The map lives in `dungeon_map`.

use crate::block::Motion;
use crate::dsp::clampf;
use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::FontRenderer;

use crate::dsp::filter::FilterKind;
use crate::dsp::modal::{EXCITER_NAMES, ModalPage};
use crate::part::DacPair;
use crate::project::{PartId, PartSet};
use crate::ui::PrimeStatus;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::block_registry::FILTER_MOD;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::{
    self, Accidental, Braid, BraidPart, Cube, CubePart, Gauge, Rings, RingsPart, TAIL_COLS, Tail,
};
use crate::ui::tape_glyph;
use crate::ui::theme;

/// `s` in upper case (names are stored mixed case: "Filter", "4opFM").
pub fn upper(s: &str) -> FmtBuf {
    let mut buf = FmtBuf::new();
    for ch in s.chars() {
        let _ = buf.write_char(ch.to_ascii_uppercase());
    }
    buf
}

/// Whether `def`'s cells are the exciter's (EXC): its name is the exciter's.
fn is_exciter(def: &BlockDef) -> bool {
    def.params
        .iter()
        .any(|s| matches!(s.binding, SlotBinding::ModalPanel(ModalPage::Exciter, _)))
}

/// Whose page a header names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    Sound(PartId),
    /// A Part's mixer, or the FX entered from it.
    Mix(PartId),
    /// SETTINGS: its leaves draw a breadcrumb instead.
    Settings,
    /// ORBIT's RING pages, on the ring they edit.
    Ring(crate::orbit::RingIdx),
    /// ORBIT's other pages, on the scene they play.
    Orbit(crate::orbit::scene::SceneIdx),
}

/// A header's text: context, page name, and the OUT warning.
pub struct HeaderText {
    pub context: FmtBuf,
    pub name: FmtBuf,
    pub warn: Option<&'static str>,
}

/// `PART 2 · SOUND` or `PART 2 · MIX` and the page (`FILTER`, `SENDS`),
/// with `models`' pad (the caller's: a page that edits it)
/// `PART 2 · 02 TONE SNARE` (`02 SNARE` where the short page name leaves
/// it too long),
/// or `SETTINGS`; `suffix` follows the name (`/ B`). A page of the
/// exciter's cells is named after MODAL's exciter (PLUCK, STRIKE, BOW),
/// TAPE's model page after its machine (STRETCH); FLT › MOD on DIRTY or
/// LADDER, which speak CLEAN's cells, names its type (`kind`).
/// `OUT P2`/`OUT P3` warns on a Part's pages when `out` isn't P1 (ADR
/// 0057); `closed` (bank 2's `UPDATE`, ADR 0141) before it. A name too long
/// for the line falls back to the page's short one.
pub fn header_text(
    head: Head,
    def: &BlockDef,
    models: impl Into<crate::ui::view::Models>,
    kind: FilterKind,
    suffix: &str,
    out: DacPair,
    closed: Option<&'static str>,
) -> HeaderText {
    let models = models.into();
    // A pad's page (the caller's `pad`) names the pad and its machine in
    // full where it fits.
    let pad = models.pad;
    let context = |full: bool| {
        let mut context = FmtBuf::new();
        let _ = match (head, pad) {
            (Head::Sound(p), Some((n, m))) => {
                let name = match full {
                    true => crate::dsp::drum::laws::spec(m).name,
                    false => crate::ui::kit_band::word(m),
                };
                let n = crate::ui::view::PAD_NUMBERS[n.index()];
                write!(context, "PART {} · {n} {name}", p.index() + 1)
            }
            (Head::Sound(p), None) => write!(context, "PART {} · SOUND", p.index() + 1),
            (Head::Mix(p), _) => write!(context, "PART {} · MIX", p.index() + 1),
            (Head::Settings, _) => context.write_str("SETTINGS"),
            (Head::Ring(r), _) => write!(context, "RING {}", r.get() + 1),
            (Head::Orbit(n), _) => write!(context, "SCENE {}", n.get() + 1),
        };
        context
    };
    let warn = match (head, out) {
        (Head::Settings | Head::Ring(_) | Head::Orbit(_), _) => None,
        _ if closed.is_some() => closed,
        (_, DacPair::P1) => None,
        (_, DacPair::P2) => Some("OUT P2"),
        (_, DacPair::P3) => Some("OUT P3"),
    };
    let named = |full: bool| {
        let mut name = match (full, is_exciter(def)) {
            (true, true) => upper(EXCITER_NAMES[models.modal as usize]),
            _ if def.id == crate::ui::block_registry::TAPE_MODEL.id => {
                upper(crate::dsp::tape_engine::MACHINE_NAMES[usize::from(models.tape.index())])
            }
            (true, false) if def.id == FILTER_MOD.id && kind == FilterKind::Dirty => {
                upper("Dirty Mod")
            }
            (true, false) if def.id == FILTER_MOD.id && kind == FilterKind::Ladder => {
                upper("Ladder Mod")
            }
            (true, false) if def.id == FILTER_MOD.id && kind == FilterKind::Sem => upper("SEM Mod"),
            (true, false) => upper(def.name),
            (false, _) => upper(def.short),
        };
        let _ = name.write_str(suffix);
        name
    };
    let mut h = HeaderText {
        context: context(true),
        name: named(true),
        warn,
    };
    if !header_fits(&h) {
        h.name = named(false);
    }
    if !header_fits(&h) && pad.is_some() {
        h.context = context(false);
    }
    h
}

/// Right edge of the CPU readout and of the OUT warning.
const HEADER_RIGHT: i32 = theme::HEADER_DOT_X - 8;
/// How far the CPU readout moves left for ORBIT's beat dot: one dot.
const ORBIT_DOT_SHIFT: i32 = theme::HEADER_DOT_X - theme::ORBIT_DOT_X;

/// Gap between the context and the name.
const HEADER_GAP: i32 = 7;
/// Least gap before what the header draws on the right.
const RIGHT_GAP: i32 = 6;

/// Where the context and name end.
fn header_end(context: &str, name: &str) -> i32 {
    theme::MARGIN_X
        + draw::text_width(&theme::FONT_LABEL, context, theme::LABEL_TRACKING)
        + HEADER_GAP
        + draw::text_width(&theme::FONT_LABEL_BOLD, name, theme::LABEL_TRACKING)
}

/// Whether the context and name end `RIGHT_GAP` short of the OUT warning,
/// or of ORBIT's beat dot, which may show on any page.
pub fn header_fits(h: &HeaderText) -> bool {
    let right = match h.warn {
        Some(warn) => {
            HEADER_RIGHT - draw::text_width(&theme::FONT_LABEL, warn, theme::LABEL_TRACKING)
        }
        None => theme::ORBIT_DOT_X - theme::HEADER_DOT_R,
    };
    header_end(h.context.as_str(), h.name.as_str()) + RIGHT_GAP <= right
}

/// The Part dots: which Part the page is on, and which have voices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartDots {
    pub current: PartId,
    pub live: PartSet,
}

/// What a header shows of the instrument beside its words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderStatus {
    /// A Part's page: its dots.
    pub parts: Option<PartDots>,
    /// The live output is above silence: the sounding dot.
    pub sounding: bool,
    /// Audio load (0 = not measured).
    pub load_pct: u8,
    /// ORBIT's beat dot: its fill while ORBIT runs.
    pub orbit_dot: Option<u8>,
}

/// Header band (y 0..28): grey context label, bold name, the OUT warning
/// or else the audio load when measured, and the dots (`header_dots`).
/// The load moves left by one dot while ORBIT's shows.
pub fn header<D>(d: &mut D, context: &str, name: &str, warn: Option<&str>, s: HeaderStatus)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            context,
            theme::MARGIN_X,
            y,
            theme::MID,
            theme::LABEL_TRACKING,
        );
    draw::text_tracked(
        d,
        &theme::FONT_LABEL_BOLD,
        name,
        x + HEADER_GAP,
        y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    if let Some(warn) = warn {
        draw::text_right(
            d,
            &theme::FONT_LABEL,
            warn,
            HEADER_RIGHT,
            y,
            theme::WARN,
            theme::LABEL_TRACKING,
        );
    } else if s.load_pct > 0 {
        let mut buf = FmtBuf::new();
        let _ = write!(buf, "CPU {}%", s.load_pct);
        let color = match s.load_pct {
            81.. => theme::ALERT,
            61..=80 => theme::WARN,
            _ => theme::MID,
        };
        // The bench readout yields to a long page name.
        let right = HEADER_RIGHT
            - if s.orbit_dot.is_some() {
                ORBIT_DOT_SHIFT
            } else {
                0
            };
        let w = draw::text_width(&theme::FONT_LABEL, buf.as_str(), 0);
        if header_end(context, name) + RIGHT_GAP <= right - w {
            draw::text_right(d, &theme::FONT_LABEL, buf.as_str(), right, y, color, 0);
        }
    }
    header_dots(d, s);
}

/// The header's dots: the sounding dot, accent while the instrument
/// sounds; left of it while ORBIT runs, its beat dot, an outline filled
/// as far as `s.orbit_dot` (255 full, 0 an outline). Palette colours only,
/// so the fill is a radius, not a shade.
pub fn header_dots<D>(d: &mut D, s: HeaderStatus)
where
    D: DrawTarget<Color = Rgb565>,
{
    if let Some(p) = s.parts {
        part_dots(d, p);
    }
    let (y, r) = (theme::HEADER_DOT_Y, theme::HEADER_DOT_R);
    if s.sounding {
        draw::dot(d, theme::HEADER_DOT_X, y, r, theme::ACCENT);
    }
    if let Some(fill) = s.orbit_dot {
        let x = theme::ORBIT_DOT_X;
        draw::ring(d, x, y, r, theme::ACCENT, 1);
        let filled = (r * fill as i32 + 127) / 255;
        if filled > 0 {
            draw::dot(d, x, y, filled, theme::ACCENT);
        }
    }
}

/// Six dots, one a Part: the current one filled bright green, larger, a sounding
/// one a ring, the rest small and faint.
fn part_dots<D>(d: &mut D, p: PartDots)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (y, r) = (theme::PART_DOT_Y, theme::PART_DOT_R);
    for id in PartId::ALL {
        let x = theme::PART_DOT_X + id.index() as i32 * theme::PART_DOT_PITCH;
        match (id == p.current, p.live.contains(id)) {
            (true, _) => draw::dot(d, x, y, r + 1, theme::PART_LIVE),
            (false, true) => draw::ring(d, x, y, r, theme::ACCENT, 1),
            (false, false) => draw::dot(d, x, y, r - 1, theme::FAINT),
        }
    }
}

/// Overlay title in the header band: grey context, an arrow, bold name
/// (`LOAD SOUND → PART 1`).
pub fn title_to<D>(d: &mut D, context: &str, name: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            context,
            theme::MARGIN_X,
            y,
            theme::MID,
            theme::LABEL_TRACKING,
        )
        + 6;
    let x = x + draw::arrow(d, x, y, theme::MID) + 5;
    draw::text_tracked(
        d,
        &theme::FONT_LABEL_BOLD,
        name,
        x,
        y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
}

/// Focus band (y 28..118): the focused slot's label, its value large, and
/// its `gauge`. A dimmed or absent slot reads as its cell does, with no
/// gauge: a dimmed value in MID, an absent route's dash in INK2. A TAPE
/// cell its machine ignores keeps the machine's glyph beside its dimmed
/// value (ADR 0173), and an LFO cell its mode ignores (CLASSIC PHASE under
/// FREE) its glyph (ADR 0177).
///
/// While a MIX+PLUS `status` is pending (issue #21) the value readout — the
/// large numerals and the arc gauge — is replaced by the status word(s) at
/// the mid-size value font, so the longest message (`NOT MODULATABLE`)
/// still fits the full row width; the label above is unchanged, so the
/// message still reads against the parameter it was tried on.
///
/// The label takes `size`'s face where it fits left of the gauge (ADR 0154).
pub fn focus_band<D>(
    d: &mut D,
    label: &str,
    value_text: &str,
    gauge: Gauge,
    look: Look,
    status: Option<PrimeStatus>,
    size: theme::TextSize,
) where
    D: DrawTarget<Color = Rgb565>,
{
    focus_label(d, label, &gauge, size);
    let layout = layout(value_text, &gauge);
    // A value too wide for the large numerals (`NEEDS A WAVE TABLE`) reads at
    // the status size, the whole row its own.
    let wide = layout == Layout::Small;
    match status {
        // VA's MODE scope keeps its place: a long MODE name reads in the
        // value font beside it
        None if layout == Layout::SmallBeside => {
            let live = look == Look::Live;
            draw::text_tracked(
                d,
                &theme::FONT_VALUE,
                value_text,
                theme::MARGIN_X,
                theme::FOCUS_VALUE_Y,
                if live { theme::INK } else { theme::MID },
                theme::LABEL_TRACKING,
            );
            if live {
                draw_gauge(d, gauge);
            }
        }
        None if wide => {
            draw::text_tracked(
                d,
                &theme::FONT_VALUE,
                value_text,
                theme::MARGIN_X,
                theme::FOCUS_VALUE_Y,
                if look == Look::Live {
                    theme::INK
                } else {
                    theme::MID
                },
                theme::LABEL_TRACKING,
            );
        }
        Some(status) => {
            let text;
            let status = if cfg!(feature = "midi-map") {
                text = status.text(label);
                text.as_str()
            } else {
                status.label()
            };
            draw::text_tracked(
                d,
                status_font(status),
                status,
                theme::MARGIN_X,
                theme::FOCUS_VALUE_Y,
                theme::INK,
                theme::LABEL_TRACKING,
            );
        }
        // A TAPE cell its machine ignores, an LFO cell its mode ignores:
        // dimmed, its glyph as it was.
        None if look == Look::Dimmed && matches!(gauge, Gauge::Tape(_) | Gauge::Lfo(_)) => {
            focus_value(d, value_text, gauge, layout, theme::MID)
        }
        None if look != Look::Live => {
            let color = match look {
                Look::Absent => theme::INK2,
                _ => theme::MID,
            };
            big_value(d, value_text, color, color);
        }
        None => focus_value(d, value_text, gauge, layout, theme::INK),
    }
}

/// A BigViz page's prime status (issue #21): one line at the top of the viz
/// band, in the focus band's status style, between the header and
/// `viz::PLOT_TOP` — above every curve and the touched-value readout, which
/// is clamped to the plot. The strip behind the text is cleared first so a
/// curve point at the plot's top edge can't run into the letters.
/// Its width drawn, in px.
pub fn viz_status<D>(d: &mut D, status: PrimeStatus, label: &str) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let text;
    let label = if cfg!(feature = "midi-map") {
        text = status.text(label);
        text.as_str()
    } else {
        status.label()
    };
    let font = status_font(label);
    let w = draw::text_width(font, label, theme::LABEL_TRACKING);
    draw::fill_rect(
        d,
        theme::MARGIN_X - 2,
        theme::HEADER_BOTTOM,
        w + 4,
        crate::ui::viz::PLOT_TOP - theme::HEADER_BOTTOM,
        theme::BG,
    );
    draw::text_tracked(
        d,
        font,
        label,
        theme::MARGIN_X,
        theme::BIGVIZ_STATUS_Y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    w
}

/// A status's face: the value font, or the bold label's where that runs
/// past the row (learn's `CC 3 ON CH 2, PART ON CH 1`).
pub fn status_font(s: &str) -> &'static FontRenderer {
    // Only learn's words run long.
    if !cfg!(feature = "midi-map")
        || draw::text_width(&theme::FONT_VALUE, s, theme::LABEL_TRACKING) <= theme::CONTENT_W
    {
        &theme::FONT_VALUE
    } else {
        &theme::FONT_LABEL_BOLD
    }
}

/// Focus label at the margin, at `size` where it fits left of `gauge`.
fn focus_label<D>(d: &mut D, label: &str, gauge: &Gauge, size: theme::TextSize)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text_fit(
        d,
        theme::Face::Value,
        size,
        label,
        theme::MARGIN_X,
        theme::FOCUS_LABEL_Y,
        theme::MID,
        theme::LABEL_TRACKING,
        focus_label_slot(gauge),
    );
}

/// The focus label's slot: the band's top to clear of the large value,
/// the margin to the gauge (less `GAUGE_GAP`) or the far margin.
pub fn focus_label_slot(gauge: &Gauge) -> draw::Slot {
    let far = theme::SCREEN_W - theme::MARGIN_X;
    draw::Slot {
        left: theme::MARGIN_X,
        top: theme::HEADER_BOTTOM,
        right: gauge_left(gauge).map_or(far, |l| l - GAUGE_GAP),
        bottom: theme::FOCUS_LABEL_Y + 4,
    }
}

/// The least gap between the value and a TAPE glyph, px.
pub const TAPE_GAP: i32 = 8;

/// The least gap between the value and an LFO glyph, px.
pub const GLYPH_GAP: i32 = 8;

/// The value large and its gauge; with none, the text has the band.
fn focus_value<D>(d: &mut D, value_text: &str, gauge: Gauge, layout: Layout, ink: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if let Gauge::Tape(_) | Gauge::Braid(_) = gauge {
        // Beside a TAPE glyph or the braid a long word (CAPSTAN, FLANGER)
        // drops to the readout size.
        if layout == Layout::NoGauge {
            draw::text(
                d,
                &theme::FONT_READOUT,
                value_text,
                theme::FOCUS_VALUE_X,
                theme::FOCUS_VALUE_Y,
                ink,
            );
        } else {
            big_value(d, value_text, ink, theme::MID);
        }
        draw_gauge(d, gauge);
        return;
    }
    if let Gauge::Lfo(g) = gauge {
        // Beside an LFO glyph a long word (RETRIG, CLASSIC) drops to the
        // readout size, as TAPE's do.
        if layout == Layout::NoGauge {
            let (number, unit) = split_unit(value_text);
            let x = theme::FOCUS_VALUE_X
                + draw::text(
                    d,
                    &theme::FONT_READOUT,
                    number,
                    theme::FOCUS_VALUE_X,
                    theme::FOCUS_VALUE_Y,
                    ink,
                );
            if !unit.is_empty() {
                draw::text(
                    d,
                    &theme::FONT_VALUE,
                    unit,
                    x + UNIT_GAP,
                    theme::FOCUS_VALUE_Y,
                    theme::MID,
                );
            }
        } else {
            big_value(d, value_text, ink, theme::MID);
        }
        return crate::ui::lfo_glyph::draw(d, &g);
    }
    big_value(d, value_text, ink, theme::MID);
    if layout == Layout::WithGauge {
        draw_gauge(d, gauge);
    }
}

/// `value_text` as a large number with its unit small beside it, on the
/// number's baseline; a word is all large.
fn big_value<D>(d: &mut D, value_text: &str, color: Rgb565, unit_color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (number, unit) = split_unit(value_text);
    let x = theme::FOCUS_VALUE_X
        + draw::text(
            d,
            &theme::FONT_FOCUS,
            number,
            theme::FOCUS_VALUE_X,
            theme::FOCUS_VALUE_Y,
            color,
        );
    if !unit.is_empty() {
        draw::text(
            d,
            &theme::FONT_VALUE,
            unit,
            x + UNIT_GAP,
            theme::FOCUS_VALUE_Y,
            unit_color,
        );
    }
}

/// Clear air between a large number and its small unit.
pub const UNIT_GAP: i32 = 4;

/// `2.14 kHz` as `("2.14", "kHz")`: a number and its trailing unit (`ms`,
/// `%`, `BT`); a word, or a number with none, is `(text, "")`.
pub fn split_unit(text: &str) -> (&str, &str) {
    let cut = text
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%')
        .len();
    let number = text[..cut].trim_end();
    if cut == text.len() || !number.ends_with(|c: char| c.is_ascii_digit()) {
        return (text, "");
    }
    // A number holds no letters: `A5 dB` is a word.
    if number.chars().any(|c| c.is_ascii_alphabetic()) {
        return (text, "");
    }
    (number, &text[cut..])
}

/// How the band sets a value against its gauge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The large value, its gauge beside it.
    WithGauge,
    /// The large value alone: it clears the row but not the gauge.
    NoGauge,
    /// The value font, the whole row: past the large value's reach.
    Small,
    /// The value font with VA's MODE scope beside it: a word too wide for
    /// the large numerals there.
    SmallBeside,
}

/// The advance of the large value as `big_value` sets it.
pub fn big_width(value_text: &str) -> i32 {
    let (number, unit) = split_unit(value_text);
    let n = draw::text_width(&theme::FONT_FOCUS, number, 0);
    if unit.is_empty() {
        n
    } else {
        n + UNIT_GAP + draw::text_width(&theme::FONT_VALUE, unit, 0)
    }
}

/// The layout of `value_text` beside `gauge`, in order of fallback: both
/// large, the value alone, the value font. Never overlapping.
pub fn layout(value_text: &str, gauge: &Gauge) -> Layout {
    let w = big_width(value_text);
    // VA's MODE scope and DRUM's icon keep their place for a word the
    // large numerals can't hold
    let beside = |left: i32| {
        let small = draw::text_width(&theme::FONT_VALUE, value_text, theme::LABEL_TRACKING);
        matches!(gauge, Gauge::VaMix(_) | Gauge::Drum(_))
            && theme::MARGIN_X + small + gap(gauge) <= left
    };
    let left = gauge_left(gauge);
    if w > theme::CONTENT_W {
        return match left {
            Some(left) if beside(left) => Layout::SmallBeside,
            _ => Layout::Small,
        };
    }
    match left {
        Some(left) if theme::FOCUS_VALUE_X + w + gap(gauge) > left => {
            if beside(left) {
                Layout::SmallBeside
            } else {
                Layout::NoGauge
            }
        }
        _ => Layout::WithGauge,
    }
}

/// Clear air between the large value and its gauge.
pub const GAUGE_GAP: i32 = 4;

/// The clear air `gauge` keeps from the large value.
fn gap(gauge: &Gauge) -> i32 {
    match gauge {
        Gauge::Tape(_) => TAPE_GAP,
        Gauge::Lfo(_) => GLYPH_GAP,
        _ => GAUGE_GAP,
    }
}

/// The leftmost column `gauge` draws in; `None` for no gauge.
pub fn gauge_left(gauge: &Gauge) -> Option<i32> {
    let end = theme::SCREEN_W - theme::MARGIN_X;
    match gauge {
        Gauge::None => None,
        Gauge::Arc { .. } => Some(theme::ARC_CX - theme::ARC_R - theme::ARC_WIDTH as i32),
        Gauge::Switch { .. } => Some(theme::SWITCH_CX - theme::SWITCH_W / 2),
        Gauge::LevelBar { .. } => Some(theme::LEVEL_TICK_X - 1),
        Gauge::Crossfader { .. } => Some(end - theme::XF_W - theme::XF_CAP_W / 2),
        Gauge::Stability { .. } => Some(end - theme::VIAL_W),
        Gauge::Feedback { .. } => Some(theme::WAVE_X),
        Gauge::Blend { .. } => Some(end - theme::BLEND_W - theme::XF_CAP_W / 2),
        Gauge::Staff { .. } => Some(theme::STAFF_X),
        Gauge::Wave(_) | Gauge::VaWave(_) | Gauge::VaLoop(_) | Gauge::Lfo(_) => Some(theme::WAVE_X),
        Gauge::VaMix(_) => Some(end - theme::VA_MIX_W),
        Gauge::Braid(_) => Some(theme::BRAID_X),
        Gauge::Rings(_) => Some(theme::RINGS_X),
        Gauge::Cube(_) | Gauge::Tail(_) => Some(theme::CUBE_X),
        Gauge::Tape(_) => Some(theme::WAVE_X),
        Gauge::Drum(_) => Some(crate::ui::kit_band::BIG_LEFT),
    }
}

/// `gauge`, or none where `layout` leaves it out.
pub fn fit_gauge(value_text: &str, gauge: Gauge) -> Gauge {
    match layout(value_text, &gauge) {
        Layout::WithGauge | Layout::SmallBeside => gauge,
        // A TAPE or LFO glyph or the braid stays: its long word drops to
        // the readout size.
        Layout::NoGauge if matches!(gauge, Gauge::Tape(_) | Gauge::Lfo(_) | Gauge::Braid(_)) => {
            gauge
        }
        _ => Gauge::None,
    }
}

/// The box an animated gauge redraws alone each frame; `None` for the
/// still ones.
pub fn gauge_rect(gauge: &Gauge) -> Option<(i32, i32, i32, i32)> {
    match gauge {
        Gauge::Braid(_) => Some((
            theme::BRAID_X,
            theme::BRAID_Y,
            theme::BRAID_W,
            theme::BRAID_H,
        )),
        Gauge::Rings(_) => Some((
            theme::RINGS_X,
            theme::RINGS_Y,
            theme::RINGS_W,
            theme::RINGS_H,
        )),
        Gauge::Cube(_) | Gauge::Tail(_) => {
            Some((theme::CUBE_X, theme::CUBE_Y, theme::CUBE_W, theme::CUBE_H))
        }
        Gauge::Arc { .. }
        | Gauge::None
        | Gauge::Switch { .. }
        | Gauge::LevelBar { .. }
        | Gauge::Crossfader { .. }
        | Gauge::Staff { .. }
        | Gauge::Wave(_)
        | Gauge::VaWave(_)
        | Gauge::VaLoop(_)
        | Gauge::VaMix(_)
        | Gauge::Blend { .. }
        | Gauge::Tape(_)
        | Gauge::Lfo(_)
        | Gauge::Stability { .. }
        | Gauge::Feedback { .. }
        | Gauge::Drum(_) => None,
    }
}

/// `gauge` alone, at the focus band's right.
pub fn draw_gauge<D>(d: &mut D, gauge: Gauge)
where
    D: DrawTarget<Color = Rgb565>,
{
    match gauge {
        Gauge::None => {}
        Gauge::Braid(b) => braid(d, &b),
        Gauge::Rings(r) => rings(d, &r),
        Gauge::Cube(c) => cube(d, &c),
        Gauge::Tail(t) => tail(d, &t),
        Gauge::Switch { on } => switch(d, on),
        Gauge::LevelBar { value, ticks } => level_bar(d, value, ticks),
        Gauge::Crossfader { value } => crossfader(d, value, theme::XF_W),
        Gauge::Staff { step, accidental } => staff(d, step, accidental),
        Gauge::Wave(k) => scope(d, &glyph::kind_trace::<SCOPE_N>(k, SCOPE_HALF)),
        Gauge::VaWave(m) => scope(d, &glyph::va_wave_trace::<SCOPE_N>(m, SCOPE_HALF)),
        Gauge::VaLoop(b0) => scope(d, &glyph::va_loop_trace::<SCOPE_N>(b0, SCOPE_HALF)),
        Gauge::VaMix(v) => scope(d, &glyph::va_mix_trace::<VA_MIX_N>(&v, SCOPE_HALF)),
        Gauge::Blend { value, mode } => {
            crossfader(d, value, theme::BLEND_W);
            blend_ends(d, glyph::blend_ends(mode));
        }
        Gauge::Tape(t) => tape_glyph::draw(d, &t, &tape_glyph::Palette::THEME),
        Gauge::Lfo(g) => crate::ui::lfo_glyph::draw(d, &g),
        Gauge::Stability { offset, reach } => spirit_level(d, offset.into(), reach.into()),
        Gauge::Feedback { q } => feedback_loop(d, q.into()),
        Gauge::Drum(m) => crate::ui::kit_band::big_icon(d, m),
        Gauge::Arc { value, bipolar } => draw::arc_gauge(
            d,
            theme::ARC_CX,
            theme::ARC_CY,
            theme::ARC_R,
            theme::ARC_WIDTH,
            value,
            bipolar,
            theme::FAINT,
            theme::ACCENT,
        ),
    }
}

/// What a cell shows in place of its value text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiniGlyph {
    /// PLACE: `post` lights the FLT box.
    Place { post: bool },
}

/// An FX box and a FLT box joined by an arrow from `x`, centred on `cy`,
/// in signal order: the effect's box (lit, ACCENT) left for PRE, right for
/// POST. Returns the width.
fn place<D>(d: &mut D, x: i32, cy: i32, post: bool) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let (w, h, gap, r) = (14, 10, 12, 2);
    for i in 0..2 {
        let (bx, y) = (x + i * (w + gap), cy - h / 2);
        if (i == 1) == post {
            draw::round_rect(d, bx, y, w, h, r, theme::ACCENT);
        } else {
            draw::round_outline(d, bx, y, w, h, r, theme::MID);
        }
    }
    draw::arrow(d, x + w + (gap - 8) / 2, cy + 3, theme::MID);
    2 * w + gap
}

/// The SWITCH glyph at the band's right: a faint pill track, lit in the
/// accent up to the knob, which slides with `on` (0..1) and reads INK on,
/// MID off.
fn switch<D>(d: &mut D, on: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (w, h) = (theme::SWITCH_W, theme::SWITCH_H);
    let (x, y) = (theme::SWITCH_CX - w / 2, theme::ARC_CY - h / 2);
    let on = clampf(on, 0.0, 1.0);
    let knob = x + h / 2 + libm::roundf(on * (w - h) as f32) as i32;
    draw::pill(d, x, y, w, h, theme::FAINT);
    if on > 0.0 {
        draw::pill(d, x, y, knob + h / 2 - x, h, theme::ACCENT);
    }
    let color = if on >= 0.5 { theme::INK } else { theme::MID };
    draw::dot(d, knob, theme::ARC_CY, h / 2 - 4, color);
}

/// The LEVEL BAR glyph near the band's right: a faint track filled in the
/// accent from the bottom to an accent handle at `value` (0..1), and
/// `ticks` dots beside it, MID up to the level, FAINT above, each where
/// the handle's centre sits at that step.
fn level_bar<D>(d: &mut D, value: f32, ticks: u8)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x, w) = (theme::LEVEL_X, theme::LEVEL_W);
    let (top, bottom) = (theme::LEVEL_TOP, theme::LEVEL_BOTTOM);
    let travel = bottom - top - w;
    let v = clampf(value, 0.0, 1.0);
    let rise = libm::roundf(v * travel as f32) as i32;
    draw::pill(d, x - w / 2, top, w, bottom - top, theme::FAINT);
    draw::pill(d, x - w / 2, bottom - w - rise, w, w + rise, theme::ACCENT);
    draw::dot(d, x, bottom - w / 2 - rise, w, theme::ACCENT);
    let n = ticks.max(2) as i32;
    for i in 0..n {
        let y = bottom - w / 2 - i * travel / (n - 1);
        let lit = i as f32 / (n - 1) as f32 <= v + 0.001;
        let color = if lit { theme::MID } else { theme::FAINT };
        draw::dot(d, theme::LEVEL_TICK_X, y, 1, color);
    }
}

/// The CROSSFADER glyph at the band's right: a faint horizontal track `w`
/// px long, a MID centre detent, and an accent cap with a groove at
/// `value` (0 left, 1 right).
fn crossfader<D>(d: &mut D, value: f32, w: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cy = theme::ARC_CY;
    let (cap_w, cap_h, track_h) = (theme::XF_CAP_W, theme::XF_CAP_H, theme::XF_TRACK_H);
    let x0 = theme::SCREEN_W - theme::MARGIN_X - w;
    draw::pill(d, x0, cy - track_h / 2, w, track_h, theme::FAINT);
    draw::fill_rect(d, x0 + w / 2 - 1, cy - 7, 2, 14, theme::MID);
    let travel = (w - cap_w) as f32;
    let cx = x0 + cap_w / 2 + libm::roundf(clampf(value, 0.0, 1.0) * travel) as i32;
    draw::round_rect(
        d,
        cx - cap_w / 2,
        cy - cap_h / 2,
        cap_w,
        cap_h,
        3,
        theme::ACCENT,
    );
    draw::fill_rect(d, cx - cap_w / 2 + 2, cy - 1, cap_w - 4, 2, theme::BG);
}

/// STABILITY's spirit level (#593): a FAINT vial, the wander's reach a
/// FAINT ghost of the bubble's path, two MID marks at the middle and the
/// bubble in the accent `offset` px right of them.
fn spirit_level<D>(d: &mut D, offset: i32, reach: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (w, h) = (theme::VIAL_W, theme::VIAL_H);
    let (bw, bh) = (theme::BUBBLE_W, theme::BUBBLE_H);
    let x0 = theme::SCREEN_W - theme::MARGIN_X - w;
    let (cx, cy) = (x0 + w / 2, theme::ARC_CY);
    draw::round_outline(d, x0, cy - h / 2, w, h, (h / 2) as u32, theme::FAINT);
    if reach > 0 {
        draw::pill(
            d,
            cx - reach - bw / 2,
            cy - 2,
            2 * reach + bw,
            4,
            theme::FAINT,
        );
    }
    // The marks just clear of the bubble at rest, through the vial's top.
    for x in [cx - bw / 2 - 3, cx + bw / 2 + 2] {
        draw::fill_rect(d, x, cy - h / 2 - 4, 1, 7, theme::MID);
    }
    draw::pill(d, cx + offset - bw / 2, cy - bh / 2, bw, bh, theme::ACCENT);
}

/// FEEDBACK's glyph (ADR 0190): a low-pass curve with a loop from its
/// output back under the cutoff corner. The loop thickens and the corner
/// wobbles with `|q|` (steps of an eighth): positive a sharp bright
/// zig-zag, negative a soft rounded dip. At 0 the curve is plain and the
/// loop a faint dotted line.
fn feedback_loop<D>(d: &mut D, q: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x0, y0, w, h) = (theme::WAVE_X, theme::WAVE_Y, theme::WAVE_W, theme::WAVE_H);
    draw::round_outline(d, x0, y0, w, h, 4, theme::FAINT);
    let m = q.abs();
    let (cx, cy) = (x0 + 30, y0 + 14);
    let (ex, ey) = (x0 + 58, cy + 18);
    let (by, tip) = (y0 + 41, cy + 8);
    // The loop: output down, back along the bottom, up to the corner.
    let legs = [(ex, ey, ex, by), (ex, by, cx, by), (cx, by, cx, tip)];
    if m == 0 {
        for (ax, ay, bx, bye) in legs {
            let n = (bx - ax).abs().max((bye - ay).abs());
            for i in (0..=n).step_by(3) {
                let (px, py) = (ax + (bx - ax).signum() * i, ay + (bye - ay).signum() * i);
                draw::fill_rect(d, px, py, 1, 1, theme::MID);
            }
        }
    } else {
        let t = 1 + m as u32 / 3;
        for (ax, ay, bx, bye) in legs {
            draw::line(d, ax, ay, bx, bye, theme::MID, t);
        }
    }
    let head = if m == 0 { theme::MID } else { theme::ACCENT };
    draw::line(d, cx, tip, cx - 3, tip + 4, head, 1);
    draw::line(d, cx, tip, cx + 3, tip + 4, head, 1);
    // The curve: flat, the corner, then the roll-off.
    let a = m * 5 / crate::ui::glyph::FEEDBACK_STEPS;
    draw::line(d, x0 + 6, cy, cx - 9, cy, theme::ACCENT, 2);
    if q > 0 {
        let pts = [
            (-9, 0),
            (-6, -a),
            (-3, a / 2),
            (0, -a),
            (3, a / 2),
            (6, -a),
            (9, 0),
        ];
        for i in 1..pts.len() {
            let (p, n) = (pts[i - 1], pts[i]);
            draw::line(d, cx + p.0, cy + p.1, cx + n.0, cy + n.1, theme::INK, 2);
        }
    } else {
        let dip = |dx: i32| cy + a * (81 - dx * dx) / 81;
        for dx in (-9..9).step_by(3) {
            draw::line(
                d,
                cx + dx,
                dip(dx),
                cx + dx + 3,
                dip(dx + 3),
                theme::ACCENT,
                2,
            );
        }
    }
    draw::line(d, cx + 9, cy, cx + 19, cy + 8, theme::ACCENT, 2);
    draw::line(d, cx + 19, cy + 8, ex, ey, theme::ACCENT, 2);
}

/// The scope's points: one a pixel across its frame, less the padding.
pub const SCOPE_N: usize = (theme::WAVE_W - 2 * theme::WAVE_PAD) as usize + 1;
/// The MODE scope's points.
pub const VA_MIX_N: usize = (theme::VA_MIX_W - 2 * theme::WAVE_PAD) as usize + 1;
/// A trace's peak, px from the centre line.
pub const SCOPE_HALF: i32 = theme::WAVE_H / 2 - theme::WAVE_PAD;

/// The scope at the band's right (WAVE's table, a VA oscillator, MODE): a
/// FAINT frame round `trace` (px up from the centre, one a pixel), a
/// dotted FAINT centre line and the trace in the accent.
fn scope<D, const N: usize>(d: &mut D, trace: &[i32; N])
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = N as i32 - 1 + 2 * theme::WAVE_PAD;
    let (x0, y0, h) = (
        theme::SCREEN_W - theme::MARGIN_X - w,
        theme::WAVE_Y,
        theme::WAVE_H,
    );
    let cy = y0 + h / 2;
    draw::round_outline(d, x0, y0, w, h, 4, theme::FAINT);
    for x in (x0 + 3..x0 + w - 3).step_by(2) {
        draw::fill_rect(d, x, cy, 1, 1, theme::FAINT);
    }
    let px = |i: usize| x0 + theme::WAVE_PAD + i as i32;
    for i in 1..N {
        draw::line(
            d,
            px(i - 1),
            cy - trace[i - 1],
            px(i),
            cy - trace[i],
            theme::ACCENT,
            2,
        );
    }
}

/// BLEND's end words under its track's ends, MID: the left one from the
/// track's left, the right one to its right.
fn blend_ends<D>(d: &mut D, [left, right]: [&str; 2])
where
    D: DrawTarget<Color = Rgb565>,
{
    let end = theme::SCREEN_W - theme::MARGIN_X;
    let rw = draw::text_width(&theme::FONT_LABEL, right, theme::LABEL_TRACKING);
    for (word, x) in [(left, end - theme::BLEND_W), (right, end - rw)] {
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            word,
            x,
            theme::XF_ENDS_Y,
            theme::MID,
            theme::LABEL_TRACKING,
        );
    }
}

/// The flat's bowl beside its stem, `(dx, dy, w)` spans from the
/// stem's right and the note's centre: round at the top, tapering in.
const FLAT_BOWL: [(i32, i32, i32); 6] = [
    (0, -3, 3),
    (2, -2, 2),
    (2, -1, 2),
    (1, 0, 2),
    (0, 1, 2),
    (0, 2, 1),
];

/// The STAFF glyph at the band's right: three MID lines and an accent
/// notehead `step` diatonic steps above the middle one, MID ledger lines
/// past the staff, and its accidental in the accent to the left.
fn staff<D>(d: &mut D, step: f32, accidental: Accidental)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (cy, gap, nx) = (theme::ARC_CY, theme::STAFF_GAP, theme::STAFF_NOTE_X);
    for k in -1..=1 {
        draw::fill_rect(
            d,
            theme::STAFF_X,
            cy + k * gap,
            theme::STAFF_W,
            1,
            theme::MID,
        );
    }
    let y = cy - libm::roundf(step * (gap / 2) as f32) as i32;
    let reach = libm::roundf(libm::fabsf(step)) as i32;
    let side = if step < 0.0 { -1 } else { 1 };
    for s in (4..=reach).step_by(2) {
        draw::fill_rect(d, nx - 10, cy - side * s * gap / 2, 21, 1, theme::MID);
    }
    draw::round_rect(d, nx - 6, y - gap / 2, 13, gap + 1, 4, theme::ACCENT);
    let (ax, c) = (nx - 16, theme::ACCENT);
    match accidental {
        Accidental::Natural => {}
        Accidental::Sharp => {
            draw::fill_rect(d, ax - 2, y - 7, 1, 15, c);
            draw::fill_rect(d, ax + 2, y - 7, 1, 15, c);
            draw::fill_rect(d, ax - 5, y - 3, 11, 2, c);
            draw::fill_rect(d, ax - 5, y + 2, 11, 2, c);
        }
        Accidental::Flat => {
            draw::fill_rect(d, ax - 2, y - 10, 1, 13, c);
            for (dx, dy, w) in FLAT_BOWL {
                draw::fill_rect(d, ax - 1 + dx, y + dy, w, 1, c);
            }
        }
    }
}

/// The chorus braid in its box: a dry centre line, and `strands` strands
/// twisting round it, each a sine `turns` times along the box shifted by
/// `twist`; behind the line FAINT, in front ACCENT. MIX weighs the strands
/// against the line (1 to 3 px; the line INK2, MID or FAINT); DEPTH is the
/// swing. The focused param is marked: MODE a dot per strand at the left,
/// RATE a bead riding the twist, DEPTH the swing's bounds, MIX the line
/// in INK. Palette colours only, so the theme's ACCENT swap applies.
fn braid<D>(d: &mut D, b: &Braid)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::dsp::fast_sin;
    use core::f32::consts::{FRAC_PI_2, TAU};
    const STEP: i32 = 4;
    // FLANGER's moiré and PHASER's ray gun are bank 2's (ADR 0323); closed,
    // the braid.
    if crate::bank2::chorus(&mut crate::ui::tape_strip::On(d), b) {
        return;
    }
    // Strands kept 2 px inside the box, so their thickness never leaves it.
    let (x0, w, h) = (theme::BRAID_X + 2, theme::BRAID_W - 4, theme::BRAID_H);
    let cy = theme::BRAID_Y + h / 2;
    let amp = 3.0 + b.depth * (h / 2 - 6) as f32;
    let k = TAU * b.turns() / w as f32;
    let twist = b.twist();
    let level = ((b.mix * 3.0) as usize).min(2);
    let dry = match b.focus {
        Some(BraidPart::Mix) => theme::INK,
        _ => [theme::INK2, theme::MID, theme::FAINT][level],
    };
    draw::fill_rect(d, x0, cy, w, 1, dry);
    if b.focus == Some(BraidPart::Depth) {
        for x in (x0..x0 + w).step_by(4) {
            for y in [cy - amp as i32, cy + amp as i32] {
                draw::fill_rect(d, x, y, 1, 1, theme::MID);
            }
        }
    }
    let n = b.strands();
    let width = level as u32 + 1;
    let at = |s: usize, x: i32| {
        let a = k * x as f32 - twist + s as f32 * TAU / n.max(1) as f32;
        let y = cy + libm::roundf(amp * fast_sin(a)) as i32;
        (y, fast_sin(a + FRAC_PI_2) >= 0.0)
    };
    // Behind the line first, then in front.
    for front in [false, true] {
        let color = if front { theme::ACCENT } else { theme::FAINT };
        for s in 0..n {
            let mut prev = at(s, 0);
            for x in (STEP..=w).step_by(STEP as usize) {
                let next = at(s, x);
                if prev.1 == front {
                    let px = x0 + x - STEP;
                    draw::line(
                        d,
                        px,
                        prev.0,
                        (x0 + x).min(x0 + w - 1),
                        next.0,
                        color,
                        width,
                    );
                }
                prev = next;
            }
        }
    }
    match b.focus {
        Some(BraidPart::Mode) => {
            for s in 0..n.max(1) {
                let y = if n == 0 { cy } else { at(s, 0).0 };
                draw::dot(d, x0 + 2, y, 2, theme::INK);
            }
        }
        Some(BraidPart::Rate) if n > 0 => {
            // Where strand 0 crests: it travels as the twist does.
            let x = libm::fmodf((twist + FRAC_PI_2) / k, w as f32) as i32;
            draw::dot(d, x0 + x.clamp(2, w - 3), at(0, x).0, 2, theme::INK);
        }
        _ => {}
    }
}

/// The delay rings in their box: a source dot at the centre and a ring
/// per surviving repeat spreading from it, `spacing` apart. A repeat's
/// ring is ACCENT while it is at least 0.35 of the first, then MID; the
/// first `crisp` are solid, older ones dotted (dark TONE blurs sooner).
/// MIX weighs rings (1 to 3 px) against the source dot (3 to 1 px);
/// MECHANICS wobbles each ring, more as it grows; SAT thickens the newest;
/// REV dots the edge where the rings leave. The focused param is marked:
/// TIME ticks at each ring's radius, FDBK a dot per survivor along the
/// top, TONE the crisp rings in INK, MIX a ring round the source,
/// MECHANICS dots at the newest ring's wobble peaks, SAT the newest in INK
/// with a ring inside it, REV its dots in INK. Palette colours only, so the theme's ACCENT swap applies.
fn rings<D>(d: &mut D, r: &Rings)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::dsp::fast_sin;
    use core::f32::consts::{FRAC_PI_2, TAU};
    const N: usize = 24;
    let (w, h) = (theme::RINGS_W, theme::RINGS_H);
    let (cx, cy) = (theme::RINGS_X + w / 2, theme::RINGS_Y + h / 2);
    // Room for the wobble (3) and the thickest ring inside the box.
    let r_max = (h / 2 - 7) as f32;
    let unit: [(f32, f32); N] = core::array::from_fn(|i| {
        let a = i as f32 * TAU / N as f32;
        (fast_sin(a + FRAC_PI_2), fast_sin(a))
    });
    let level = ((r.mix * 3.0) as usize).min(2);
    let wobble = r.mech * 3.0;
    let wob_phase = r.wobble_phase();
    let ring = |d: &mut D, rad: f32, wob: f32, color: Rgb565, width: u32, dotted: bool| {
        let pt = |i: usize| {
            let (c, s) = unit[i % N];
            let a = i as f32 * TAU / N as f32;
            let rr = rad + wob * fast_sin(3.0 * a + wob_phase) * (rad / r_max);
            (
                cx + libm::roundf(rr * c) as i32,
                cy + libm::roundf(rr * s) as i32,
            )
        };
        for i in (0..N).filter(|i| !dotted || i % 2 == 0) {
            let (a, b) = (pt(i), pt(i + 1));
            draw::line(d, a.0, a.1, b.0, b.1, color, width);
        }
    };
    // The radii of the rings in the box, newest first.
    let mut radii = [0.0f32; 12];
    let mut alive = 0;
    for k in 0..r.survivors() {
        let rad = (r.age() + k as f32 * r.period()) * Rings::SPEED;
        if rad > r_max {
            break;
        }
        radii[alive] = rad;
        alive += 1;
    }
    for (k, &rad) in radii[..alive].iter().enumerate().rev() {
        if rad < 1.0 {
            continue;
        }
        let amp = libm::powf(r.fdbk, k as f32);
        let crisp = k < r.crisp();
        let ink = match r.focus {
            Some(RingsPart::Tone) => crisp,
            Some(RingsPart::Sat) => k == 0,
            _ => false,
        };
        let color = if ink {
            theme::INK
        } else if amp >= 0.35 {
            theme::ACCENT
        } else {
            theme::MID
        };
        let sat = if k == 0 {
            libm::roundf(r.sat * 2.0) as u32
        } else {
            0
        };
        ring(d, rad, wobble, color, level as u32 + 1 + sat, !crisp);
        if k == 0 && r.focus == Some(RingsPart::Sat) && rad > 4.0 {
            ring(d, rad - 3.0, wobble, theme::ACCENT, 1, true);
        }
    }
    match r.focus {
        // How many repeats survive: a dot each along the top.
        Some(RingsPart::Fdbk) => {
            for i in 0..r.survivors() as i32 {
                draw::fill_rect(
                    d,
                    theme::RINGS_X + 2 + 4 * i,
                    theme::RINGS_Y + 1,
                    2,
                    2,
                    theme::INK,
                );
            }
        }
        // Where the wobble peaks on the newest ring.
        Some(RingsPart::Mech) if alive > 0 => {
            for p in 0..3 {
                let a = (FRAC_PI_2 - wob_phase) / 3.0 + p as f32 * TAU / 3.0;
                // Kept inside the box: at most r_max + 3, a 2 px dot.
                let rr = (radii[0] + wobble * (radii[0] / r_max) + 3.0).min(r_max + 3.0);
                let (c, s) = (fast_sin(a + FRAC_PI_2), fast_sin(a));
                draw::fill_rect(
                    d,
                    cx + libm::roundf(rr * c) as i32,
                    cy + libm::roundf(rr * s) as i32,
                    2,
                    2,
                    theme::INK,
                );
            }
        }
        _ => {}
    }
    if r.focus == Some(RingsPart::Time) {
        for &rad in &radii[..alive] {
            draw::fill_rect(d, cx + rad as i32, theme::RINGS_Y + h - 4, 1, 3, theme::INK);
        }
    }
    let dots = libm::roundf(r.rev * 16.0) as usize;
    let rev = if r.focus == Some(RingsPart::Rev) {
        theme::INK
    } else {
        theme::MID
    };
    for i in 0..dots {
        let (c, s) = unit[i * N / 16];
        let edge = r_max + 4.0;
        draw::fill_rect(
            d,
            cx + libm::roundf(edge * c) as i32,
            cy + libm::roundf(edge * s) as i32,
            1,
            1,
            rev,
        );
    }
    let src = 3 - level as i32;
    draw::dot(d, cx, cy, src, theme::INK);
    if r.focus == Some(RingsPart::Mix) {
        draw::ring(d, cx, cy, src + 3, theme::INK, 1);
    }
}

/// The cube's twelve edges, by corner (bit 0 x, bit 1 y, bit 2 z).
const CUBE_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// A 0..255 grain, the same for the same frame, edge and piece.
fn grain(frame: u32, edge: usize, piece: i32) -> u32 {
    let mut h = frame
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add((edge as u32) << 16)
        .wrapping_add(piece as u32);
    h ^= h << 13;
    h ^= h >> 17;
    h ^= h << 5;
    h & 255
}

/// The reverb cube in its box: a wireframe room in perspective, tilted to
/// show its top, turning once in 16 s. Near edges (towards the viewer) in
/// ACCENT, 1 to 3 px by MIX; far edges 1 px, by DAMP ACCENT, MID, then
/// dotted MID, so the highs die in the back of the room. TIME trails up
/// to three afterimages behind the turn in FAINT; SMEAR jitters the far
/// corners by up to 2 px, a new random offset every 16 frames at RATE 0
/// down to every 4 at 1, so the back of the room wanders. The focused
/// param is marked: SIZE dots the corners, TIME draws the afterimages in
/// MID, DAMP the far edges in INK, MIX the near edges in INK, SMEAR dots
/// the far corners in INK and RATE in MID. Palette colours only, so the
/// The reverb's TAIL in the cube's box: a polyline of `tail_shape`, the
/// focused knob's columns in the accent, the playhead a dot on it.
fn tail<D>(d: &mut D, t: &Tail)
where
    D: DrawTarget<Color = Rgb565>,
{
    let x0 = theme::CUBE_X + (theme::CUBE_W - TAIL_COLS as i32) / 2;
    let base = theme::CUBE_Y + theme::CUBE_H - 4;
    let h = theme::CUBE_H - 8;
    let at = |c: usize| (x0 + c as i32, base - t.shape[c] as i32 * h / 255);
    for c in 1..TAIL_COLS {
        let ((ax, ay), (bx, by)) = (at(c - 1), at(c));
        let lit = (t.accent.0 as usize..t.accent.1 as usize).contains(&c);
        let color = if lit { theme::ACCENT } else { theme::MID };
        draw::line(d, ax, ay, bx, by, color, 1);
    }
    if let Some(c) = t.head {
        let (x, y) = at((c as usize).min(TAIL_COLS - 1));
        draw::dot(d, x, y, 2, theme::INK);
    }
}

/// theme's ACCENT swap applies.
fn cube<D>(d: &mut D, c: &Cube)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::dsp::fast_sin;
    use core::f32::consts::FRAC_PI_2;
    /// Viewer distance, px: the perspective.
    const VIEW: f32 = 100.0;
    const TILT: f32 = 0.45;
    let (cx, cy) = (
        theme::CUBE_X + theme::CUBE_W / 2,
        theme::CUBE_Y + theme::CUBE_H / 2,
    );
    let s = c.half();
    let (tc, ts) = (fast_sin(TILT + FRAC_PI_2), fast_sin(TILT));
    let project = |turn: f32| -> [(i32, i32, f32); 8] {
        let (rc, rs) = (fast_sin(turn + FRAC_PI_2), fast_sin(turn));
        core::array::from_fn(|i| {
            let sign = |bit: usize| if i >> bit & 1 == 1 { s } else { -s };
            let (x, y, z) = (sign(0), sign(1), sign(2));
            let (x1, z1) = (x * rc + z * rs, -x * rs + z * rc);
            let (y2, z2) = (y * tc - z1 * ts, y * ts + z1 * tc);
            let k = VIEW / (VIEW - z2);
            (
                cx + libm::roundf(x1 * k) as i32,
                cy + libm::roundf(y2 * k) as i32,
                z2,
            )
        })
    };
    // An edge, or with `dotted` every other piece of about 3 px.
    let edge =
        |d: &mut D, p: &[(i32, i32, f32); 8], e: usize, color: Rgb565, width: u32, dotted: bool| {
            let ((ax, ay, _), (bx, by, _)) = (p[CUBE_EDGES[e].0], p[CUBE_EDGES[e].1]);
            if !dotted {
                draw::line(d, ax, ay, bx, by, color, width);
                return;
            }
            let n = ((ax - bx).abs().max((ay - by).abs()) / 3).max(1);
            for i in 0..n {
                if i % 2 == 1 {
                    continue;
                }
                let at = |t: i32| (ax + (bx - ax) * t / n, ay + (by - ay) * t / n);
                let ((x0, y0), (x1, y1)) = (at(i), at(i + 1));
                draw::line(d, x0, y0, x1, y1, color, width);
            }
        };
    let ghost = if c.focus == Some(CubePart::Time) {
        theme::MID
    } else {
        theme::FAINT
    };
    for k in (1..=c.trails()).rev() {
        let p = project(c.turn() - k as f32 * c.lag());
        for e in 0..CUBE_EDGES.len() {
            edge(d, &p, e, ghost, 1, false);
        }
    }
    let mut p = project(c.turn());
    let near =
        |p: &[(i32, i32, f32); 8], e: usize| p[CUBE_EDGES[e].0].2 + p[CUBE_EDGES[e].1].2 >= 0.0;
    let reach = 2.0 * c.smear;
    let hold = 16 - (12.0 * c.rate) as u32;
    if reach > 0.0 {
        for (i, v) in p.iter_mut().enumerate().filter(|(_, v)| v.2 < 0.0) {
            let jitter = |k: usize| {
                let g = grain(c.frame / hold, 14 + k, i as i32) as f32 / 127.5 - 1.0;
                libm::roundf(reach * g) as i32
            };
            (v.0, v.1) = (v.0 + jitter(0), v.1 + jitter(1));
        }
    }
    let damp = ((c.damp * 3.0) as usize).min(2);
    let far = match (c.focus, damp) {
        (Some(CubePart::Damp), _) => theme::INK,
        (_, 0) => theme::ACCENT,
        _ => theme::MID,
    };
    for e in (0..CUBE_EDGES.len()).filter(|&e| !near(&p, e)) {
        edge(d, &p, e, far, 1, damp == 2);
    }
    let weight = ((c.mix * 3.0) as u32).min(2) + 1;
    let front = if c.focus == Some(CubePart::Mix) {
        theme::INK
    } else {
        theme::ACCENT
    };
    for e in (0..CUBE_EDGES.len()).filter(|&e| near(&p, e)) {
        edge(d, &p, e, front, weight, false);
    }
    match c.focus {
        Some(CubePart::Size) => {
            for &(x, y, _) in &p {
                draw::fill_rect(d, x - 1, y - 1, 2, 2, theme::INK);
            }
        }
        Some(part @ (CubePart::Smear | CubePart::Rate)) => {
            let dot = if part == CubePart::Smear {
                theme::INK
            } else {
                theme::MID
            };
            for &(x, y, _) in p.iter().filter(|v| v.2 < 0.0) {
                draw::fill_rect(d, x - 1, y - 1, 2, 2, dot);
            }
        }
        _ => {}
    }
}

/// How a cell reads (spec § UI). The discriminants pack into the Cells
/// region's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Look {
    Live = 0,
    /// A route knob with no route: a dash where the value goes, no bar.
    Absent = 1,
    /// Fixed or inapplicable: label and value in MID, no bar.
    Dimmed = 2,
}

/// One cell of the 3×2 grid.
pub struct Cell<'a> {
    pub label: &'a str,
    /// Formatted value.
    pub text: &'a str,
    /// Animated 0..1 value for the bar.
    pub value: f32,
    pub fmt: crate::block::ValFmt,
    /// The focused slot: accent label and bar.
    pub active: bool,
    /// Summed mod amount (−1..1) when the param is a mod destination.
    pub mod_amount: Option<f32>,
    pub look: Look,
    /// How the param moves (ADR 0112), when it is one: a continuous one
    /// shows a bar even with a discrete readout (FDBK, FINE), a stepped one
    /// only its value (PITCH, CRSE, ORBIT's CHANCE). Without one (a page's
    /// own cells), by its readout.
    pub motion: Option<Motion>,
    /// Drawn in place of the value text and bar.
    pub mini: Option<MiniGlyph>,
}

/// Draw cell `i` (knob order a–f, 3×2) with its label baseline `top + row·36`,
/// its lines and faces at `size` (ADR 0154). `None` is an empty slot: a dim
/// dash. Returns the value's advance (0 with none).
pub fn cell<D>(d: &mut D, i: usize, top: i32, cell: Option<&Cell>, size: theme::TextSize) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = theme::MARGIN_X + (i % 3) as i32 * theme::CELL_COL_W;
    let y = top + (i / 3) as i32 * theme::CELL_ROW_H;
    let Some(c) = cell else {
        draw::fill_rect(d, x, y - 3, 8, 1, theme::FAINT);
        return 0;
    };
    let lines = theme::cell_lines(size);
    let (label_slot, value_slot) = cell_slots(x, y, lines);
    let dim = c.look == Look::Dimmed;
    let label_color = if c.active && !dim {
        theme::ACCENT
    } else {
        theme::MID
    };
    draw::text_fit(
        d,
        theme::Face::Label,
        size,
        c.label,
        x,
        y + lines.label,
        label_color,
        theme::LABEL_TRACKING,
        label_slot,
    );
    if c.look == Look::Absent {
        draw::fill_rect(d, x, y + lines.value - 5, 12, 2, theme::INK2);
        return 0;
    }
    let value_color = match (dim, c.active) {
        (true, _) => theme::MID,
        (false, true) => theme::INK,
        (false, false) => theme::INK2,
    };
    if let Some(MiniGlyph::Place { post }) = c.mini {
        return place(d, x, y + lines.value - 4, post);
    }
    let advance = cell_value(d, c.text, x, y + lines.value, value_color, value_slot, size);
    let bar = c.motion.map_or(!c.fmt.is_discrete(), |m| m.is_continuous());
    if bar && !dim {
        let fill = if c.active {
            theme::ACCENT
        } else {
            theme::BAR_REST
        };
        draw::bar(
            d,
            x,
            y + lines.bar,
            theme::CELL_BAR_W,
            theme::CELL_BAR_H,
            c.value,
            c.fmt.is_bipolar(),
            theme::FAINT,
            fill,
        );
    }
    if let Some(m) = c.mod_amount.filter(|_| !dim) {
        let mid = x + theme::CELL_BAR_W / 2;
        let len = (clampf(m, -1.0, 1.0) * (theme::CELL_BAR_W / 2) as f32) as i32;
        let (x0, x1) = if len >= 0 {
            (mid, mid + len)
        } else {
            (mid + len, mid)
        };
        draw::fill_rect(d, mid, y + lines.modl - 1, 1, 3, theme::MID);
        draw::fill_rect(d, x0, y + lines.modl, (x1 - x0).max(1), 1, theme::INK2);
    }
    advance
}

/// A cell's value from `x` on baseline `y`. LARGE: whole in the large
/// face where it fits; else its number large and its unit in NORMAL's
/// value face beside it, as the focus value sets them (`3.54` large,
/// `kHz` small); else whole in NORMAL's (ADR 0154). Returns the advance.
fn cell_value<D>(
    d: &mut D,
    text: &str,
    x: i32,
    y: i32,
    color: Rgb565,
    slot: draw::Slot,
    size: theme::TextSize,
) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    use theme::{Face, TextSize};
    let room = slot.right - x;
    let whole = theme::fit(size, room, |z| {
        draw::text_width(theme::font(Face::Value, z), text, 0)
    });
    let (number, unit) = split_unit(text);
    let unit_w = draw::text_width(&theme::FONT_VALUE, unit, 0) + UNIT_GAP;
    let split = whole == TextSize::Normal
        && !unit.is_empty()
        && theme::fit(size, room - unit_w, |z| {
            draw::text_width(theme::font(Face::Value, z), number, 0)
        }) == TextSize::Large;
    if !split {
        // Tracked by 0: the same glyphs, and the marks (RING's "3 › P3").
        return draw::text_fit(d, Face::Value, whole, text, x, y, color, 0, slot);
    }
    let head = draw::text_fit(
        d,
        Face::Value,
        TextSize::Large,
        number,
        x,
        y,
        color,
        0,
        slot,
    );
    head + UNIT_GAP + draw::text(d, &theme::FONT_VALUE, unit, x + head + UNIT_GAP, y, color)
}

/// A cell's label and value slots, the cell at `x` with its row's label
/// baseline at `y`: a column less a 4 px gutter wide; the label from the
/// row's top to its baseline, the value from there to the bar.
pub fn cell_slots(x: i32, y: i32, lines: theme::CellLines) -> (draw::Slot, draw::Slot) {
    let right = x + theme::CELL_COL_W - 4;
    let split = y + lines.label + 1;
    let label = draw::Slot {
        left: x,
        top: y - (theme::CELL_LABEL_Y - theme::VIZ_BAND_BOTTOM),
        right,
        bottom: split,
    };
    let value = draw::Slot {
        left: x,
        top: split,
        right,
        bottom: y + lines.bar,
    };
    (label, value)
}
