//! Direction A shared components (UI refresh spec § Shared components):
//! header, focus band, cells. The map lives in `dungeon_map`.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::modal::{EXCITER_NAMES, ModalPage, ResonatorMode};
use crate::part::DacPair;
use crate::project::PartId;
use crate::ui::PrimeStatus;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::{Braid, BraidPart, Cube, CubePart, Gauge, Rings, RingsPart};
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
}

/// A header's text: context, page name, and the OUT warning.
pub struct HeaderText {
    pub context: FmtBuf,
    pub name: FmtBuf,
    pub warn: Option<&'static str>,
}

/// `PART 2 · SOUND` or `PART 2 · MIX` and the page (`FILTER`, `SENDS`),
/// or `SETTINGS`; `suffix` follows the name (`/ B`). A page of the
/// exciter's cells is named after `model`'s exciter (PLUCK, STRIKE, BOW).
/// `OUT P2`/`OUT P3` warns on a Part's pages when `out` isn't P1 (ADR
/// 0057). A name too long for the line falls back to the page's short one.
pub fn header_text(
    head: Head,
    def: &BlockDef,
    model: ResonatorMode,
    suffix: &str,
    out: DacPair,
) -> HeaderText {
    let mut context = FmtBuf::new();
    let _ = match head {
        Head::Sound(p) => write!(context, "PART {} · SOUND", p.index() + 1),
        Head::Mix(p) => write!(context, "PART {} · MIX", p.index() + 1),
        Head::Settings => context.write_str("SETTINGS"),
    };
    let warn = match (head, out) {
        (Head::Settings, _) | (_, DacPair::P1) => None,
        (_, DacPair::P2) => Some("OUT P2"),
        (_, DacPair::P3) => Some("OUT P3"),
    };
    let named = |full: bool| {
        let mut name = match (full, is_exciter(def)) {
            (true, true) => upper(EXCITER_NAMES[model as usize]),
            (true, false) => upper(def.name),
            (false, _) => upper(def.short),
        };
        let _ = name.write_str(suffix);
        name
    };
    let mut h = HeaderText {
        context,
        name: named(true),
        warn,
    };
    if !header_fits(&h) {
        h.name = named(false);
    }
    h
}

/// Right edge of the CPU readout and of the OUT warning.
const HEADER_RIGHT: i32 = theme::HEADER_DOT_X - 8;

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
/// or of the sounding dot.
pub fn header_fits(h: &HeaderText) -> bool {
    let right = match h.warn {
        Some(warn) => {
            HEADER_RIGHT - draw::text_width(&theme::FONT_LABEL, warn, theme::LABEL_TRACKING)
        }
        None => theme::HEADER_DOT_X - theme::HEADER_DOT_R,
    };
    header_end(h.context.as_str(), h.name.as_str()) + RIGHT_GAP <= right
}

/// Header band (y 0..28): grey context label, bold name, the OUT warning
/// or else the audio load when measured, and an accent dot while the
/// instrument is sounding.
pub fn header<D>(
    d: &mut D,
    context: &str,
    name: &str,
    warn: Option<&str>,
    sounding: bool,
    load_pct: u8,
) where
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
    } else if load_pct > 0 {
        let mut buf = FmtBuf::new();
        let _ = write!(buf, "CPU {}%", load_pct);
        let color = match load_pct {
            81.. => theme::ALERT,
            61..=80 => theme::WARN,
            _ => theme::MID,
        };
        // The bench readout yields to a long page name.
        let w = draw::text_width(&theme::FONT_LABEL, buf.as_str(), 0);
        if header_end(context, name) + RIGHT_GAP <= HEADER_RIGHT - w {
            draw::text_right(
                d,
                &theme::FONT_LABEL,
                buf.as_str(),
                HEADER_RIGHT,
                y,
                color,
                0,
            );
        }
    }
    if sounding {
        draw::dot(
            d,
            theme::HEADER_DOT_X,
            theme::HEADER_DOT_Y,
            theme::HEADER_DOT_R,
            theme::ACCENT,
        );
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
/// gauge: a dimmed value in MID, an absent route's dash in INK2.
///
/// While a MIX+PLUS `status` is pending (issue #21) the value readout — the
/// large numerals and the arc gauge — is replaced by the status word(s) at
/// the mid-size value font, so the longest message (`NOT MODULATABLE`)
/// still fits the full row width; the label above is unchanged, so the
/// message still reads against the parameter it was tried on.
pub fn focus_band<D>(
    d: &mut D,
    label: &str,
    value_text: &str,
    gauge: Gauge,
    look: Look,
    status: Option<PrimeStatus>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    focus_label(d, label, theme::MARGIN_X);
    match status {
        Some(status) => {
            draw::text_tracked(
                d,
                &theme::FONT_VALUE,
                status.label(),
                theme::MARGIN_X,
                theme::FOCUS_VALUE_Y,
                theme::INK,
                theme::LABEL_TRACKING,
            );
        }
        None if look != Look::Live => {
            let color = match look {
                Look::Absent => theme::INK2,
                _ => theme::MID,
            };
            draw::text(
                d,
                &theme::FONT_FOCUS,
                value_text,
                theme::FOCUS_VALUE_X,
                theme::FOCUS_VALUE_Y,
                color,
            );
        }
        None => focus_value(d, value_text, gauge),
    }
}

/// A BigViz page's prime status (issue #21): one line at the top of the viz
/// band, in the focus band's status style, between the header and
/// `viz::PLOT_TOP` — above every curve and the touched-value readout, which
/// is clamped to the plot. The strip behind the text is cleared first so a
/// curve point at the plot's top edge can't run into the letters.
pub fn viz_status<D>(d: &mut D, status: PrimeStatus)
where
    D: DrawTarget<Color = Rgb565>,
{
    let label = status.label();
    let w = draw::text_width(&theme::FONT_VALUE, label, theme::LABEL_TRACKING);
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
        &theme::FONT_VALUE,
        label,
        theme::MARGIN_X,
        theme::BIGVIZ_STATUS_Y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
}

/// Focus label at `x`; returns where it ends.
fn focus_label<D>(d: &mut D, label: &str, x: i32) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    x + draw::text_tracked(
        d,
        &theme::FONT_VALUE,
        label,
        x,
        theme::FOCUS_LABEL_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    )
}

/// The value large and its gauge; with none, the text has the band.
fn focus_value<D>(d: &mut D, value_text: &str, gauge: Gauge)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text(
        d,
        &theme::FONT_FOCUS,
        value_text,
        theme::FOCUS_VALUE_X,
        theme::FOCUS_VALUE_Y,
        theme::INK,
    );
    draw_gauge(d, gauge);
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
        Gauge::Cube(_) => Some((theme::CUBE_X, theme::CUBE_Y, theme::CUBE_W, theme::CUBE_H)),
        Gauge::Arc { .. }
        | Gauge::None
        | Gauge::Switch { .. }
        | Gauge::LevelBar { .. }
        | Gauge::Crossfader { .. } => None,
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
        Gauge::Switch { on } => switch(d, on),
        Gauge::LevelBar { value, ticks } => level_bar(d, value, ticks),
        Gauge::Crossfader { value } => crossfader(d, value),
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

/// The SWITCH glyph at the band's right: a faint pill track, lit in the
/// accent up to the knob, which slides with `on` (0..1) and reads INK on,
/// MID off.
fn switch<D>(d: &mut D, on: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (w, h) = (theme::SWITCH_W, theme::SWITCH_H);
    let (x, y) = (theme::SWITCH_CX - w / 2, theme::ARC_CY - h / 2);
    let on = on.clamp(0.0, 1.0);
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
    let v = value.clamp(0.0, 1.0);
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

/// The CROSSFADER glyph at the band's right: a faint horizontal track, a
/// MID centre detent, and an accent cap with a groove at `value` (0 left,
/// 1 right).
fn crossfader<D>(d: &mut D, value: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (w, cy) = (theme::XF_W, theme::ARC_CY);
    let (cap_w, cap_h, track_h) = (theme::XF_CAP_W, theme::XF_CAP_H, theme::XF_TRACK_H);
    let x0 = theme::XF_CX - w / 2;
    draw::pill(d, x0, cy - track_h / 2, w, track_h, theme::FAINT);
    draw::fill_rect(d, theme::XF_CX - 1, cy - 7, 2, 14, theme::MID);
    let travel = (w - cap_w) as f32;
    let cx = x0 + cap_w / 2 + libm::roundf(value.clamp(0.0, 1.0) * travel) as i32;
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
/// to three afterimages behind the turn in FAINT; GRIT drops grains from
/// every edge, new each frame. The focused param is marked: SIZE dots the
/// corners, TIME draws the afterimages in MID, DAMP the far edges in INK,
/// MIX the near edges in INK, GRIT specks the room. Palette colours only,
/// so the theme's ACCENT swap applies.
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
    // An edge in pieces of about 3 px: GRIT drops some, `dotted` every other.
    let edge =
        |d: &mut D, p: &[(i32, i32, f32); 8], e: usize, color: Rgb565, width: u32, dotted: bool| {
            let ((ax, ay, _), (bx, by, _)) = (p[CUBE_EDGES[e].0], p[CUBE_EDGES[e].1]);
            let drop = (c.grit * 128.0) as u32;
            if drop == 0 && !dotted {
                draw::line(d, ax, ay, bx, by, color, width);
                return;
            }
            let n = ((ax - bx).abs().max((ay - by).abs()) / 3).max(1);
            for i in 0..n {
                if (dotted && i % 2 == 1) || grain(c.frame, e, i) < drop {
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
    let p = project(c.turn());
    let near = |e: usize| p[CUBE_EDGES[e].0].2 + p[CUBE_EDGES[e].1].2 >= 0.0;
    let damp = ((c.damp * 3.0) as usize).min(2);
    let far = match (c.focus, damp) {
        (Some(CubePart::Damp), _) => theme::INK,
        (_, 0) => theme::ACCENT,
        _ => theme::MID,
    };
    for e in (0..CUBE_EDGES.len()).filter(|&e| !near(e)) {
        edge(d, &p, e, far, 1, damp == 2);
    }
    let weight = ((c.mix * 3.0) as u32).min(2) + 1;
    let front = if c.focus == Some(CubePart::Mix) {
        theme::INK
    } else {
        theme::ACCENT
    };
    for e in (0..CUBE_EDGES.len()).filter(|&e| near(e)) {
        edge(d, &p, e, front, weight, false);
    }
    match c.focus {
        Some(CubePart::Size) => {
            for &(x, y, _) in &p {
                draw::fill_rect(d, x - 1, y - 1, 2, 2, theme::INK);
            }
        }
        Some(CubePart::Grit) => {
            let n = 3 + (c.grit * 10.0) as i32;
            for i in 0..n {
                let g = grain(c.frame, 12, i) as i32 | (grain(c.frame, 13, i) as i32) << 8;
                let x = theme::CUBE_X + 2 + (g & 0xff) % (theme::CUBE_W - 4);
                let y = theme::CUBE_Y + 2 + (g >> 8) % (theme::CUBE_H - 4);
                draw::fill_rect(d, x, y, 1, 1, theme::INK);
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
}

/// Draw cell `i` (knob order a–f, 3×2) with its label baseline `top + row·36`.
/// `None` is an empty slot: a dim dash.
pub fn cell<D>(d: &mut D, i: usize, top: i32, cell: Option<&Cell>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = theme::MARGIN_X + (i % 3) as i32 * theme::CELL_COL_W;
    let y = top + (i / 3) as i32 * theme::CELL_ROW_H;
    let Some(c) = cell else {
        draw::fill_rect(d, x, y - 3, 8, 1, theme::FAINT);
        return;
    };
    let dim = c.look == Look::Dimmed;
    let label_color = if c.active && !dim {
        theme::ACCENT
    } else {
        theme::MID
    };
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        c.label,
        x,
        y,
        label_color,
        theme::LABEL_TRACKING,
    );
    if c.look == Look::Absent {
        draw::fill_rect(d, x, y + theme::CELL_VALUE_DY - 5, 12, 2, theme::INK2);
        return;
    }
    let value_color = match (dim, c.active) {
        (true, _) => theme::MID,
        (false, true) => theme::INK,
        (false, false) => theme::INK2,
    };
    draw::text(
        d,
        &theme::FONT_VALUE,
        c.text,
        x,
        y + theme::CELL_VALUE_DY,
        value_color,
    );
    if !c.fmt.is_discrete() && !dim {
        let fill = if c.active {
            theme::ACCENT
        } else {
            theme::BAR_REST
        };
        draw::bar(
            d,
            x,
            y + theme::CELL_BAR_DY,
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
        let len = (m.clamp(-1.0, 1.0) * (theme::CELL_BAR_W / 2) as f32) as i32;
        let (x0, x1) = if len >= 0 {
            (mid, mid + len)
        } else {
            (mid + len, mid)
        };
        draw::fill_rect(d, mid, y + theme::CELL_MOD_DY - 1, 1, 3, theme::MID);
        draw::fill_rect(
            d,
            x0,
            y + theme::CELL_MOD_DY,
            (x1 - x0).max(1),
            1,
            theme::INK2,
        );
    }
}
