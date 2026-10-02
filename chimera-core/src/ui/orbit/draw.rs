//! What ORBIT's pages draw: RHYTHM's pattern unrolled, CHRD's segments as
//! chips over the loop's bars, and the CUSTOM circle; the text of ORBIT's
//! own cells and focus band; and the keys that redraw them. Drawn from the
//! live scene and the runner's last view, so a drift shows as it plays.

use core::f32::consts::TAU;
use core::fmt::Write;
use core::hash::{Hash, Hasher};

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use super::{OrbitCell, OrbitUi};
use crate::addr::{BlockRef, ParamAddr};
use crate::orbit::RingIdx;
use crate::orbit::chords::{ChordRing, harmony};
use crate::orbit::euclid::{HitMask, accents};
use crate::orbit::scene::{Scene, SceneRing};
use crate::ui::block_def::{BlockDef, VizType};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::Gauge;
use crate::ui::theme::{self, VIZ_BAND_MID as MID, VIZ_LEFT, VIZ_RIGHT};
use crate::ui::view::{SlotCtx, View, view};
use chimera_theory::{Interval, Stack};

/// What ORBIT's pages draw from: the scene being edited and the UI's
/// selections and last view of the runner.
#[derive(Clone, Copy)]
pub struct OrbitFrame<'a> {
    pub scene: &'a Scene,
    pub ui: &'a OrbitUi,
}

impl OrbitFrame<'_> {
    /// Ring `r`'s pattern: the runner's, drift and all, while it plays
    /// the rhythm the scene has; else the scene's (an edit not yet heard).
    pub fn hits(&self, r: RingIdx) -> HitMask {
        let s = &self.scene.rings[r.get() as usize];
        let seen = &self.ui.status.rings[r.get() as usize];
        let ours = s.ring.hits_mask();
        if seen.rhythm_rev == s.rhythm_rev && seen.hits.steps() == ours.steps() {
            seen.hits
        } else {
            ours
        }
    }

    /// The step under the clock, while it runs.
    fn now(&self, r: RingIdx) -> Option<usize> {
        let st = &self.ui.status;
        st.running
            .then_some(st.rings[r.get() as usize].step as usize)
    }

    fn chords(&self) -> &ChordRing {
        &self.scene.chords
    }
}

/// The band's width a step or a chip shares.
const BAND_W: f32 = (VIZ_RIGHT - VIZ_LEFT) as f32;

/// RING · RHYTHM: every step a dot on the band's line, a hit filled as far
/// as CHANCE lets it sound, an accent ringed, the step under the clock lit;
/// all dark while MUTE holds it. A tick marks step 1, so ROTATE reads.
pub fn rhythm<D>(d: &mut D, f: &OrbitFrame, r: RingIdx)
where
    D: DrawTarget<Color = Rgb565>,
{
    let ring = &f.scene.rings[r.get() as usize].ring;
    let mask = f.hits(r);
    let n = mask.steps().get() as usize;
    let now = f.now(r);
    let w = BAND_W / n as f32;
    let x = |at: f32| VIZ_LEFT + (w * at) as i32;
    let hit_r = ((w / 2.0) as i32 - 1).clamp(2, 5);
    let rest_r = if w >= 6.0 { 2 } else { 1 };
    let accented = accents(mask, ring.accent);
    let odds = ring.chance.get() as f32 / 100.0;
    let (hit, rest, accent) = if ring.mute {
        (theme::PILL_EDGE, theme::FAINT, theme::PILL_EDGE)
    } else {
        (theme::ACCENT, theme::BAR_REST, theme::INK2)
    };
    for i in 0..n {
        let cx = x(i as f32 + 0.5);
        if now == Some(i) {
            let x0 = x(i as f32);
            draw::fill_rect(
                d,
                x0,
                MID - 16,
                x(i as f32 + 1.0) - x0,
                32,
                theme::ACCENT_SOFT,
            );
        }
        if !mask.is_hit(i) {
            draw::dot(d, cx, MID, rest_r, rest);
            continue;
        }
        let c = if now == Some(i) && !ring.mute {
            theme::INK
        } else {
            hit
        };
        if odds >= 1.0 {
            draw::dot(d, cx, MID, hit_r, c);
        } else {
            draw::ring(d, cx, MID, hit_r, c, 1);
            let core = libm::roundf(hit_r as f32 * libm::sqrtf(odds)) as i32;
            if core > 0 {
                draw::dot(d, cx, MID, core, c);
            }
        }
        if accented.is_hit(i) {
            draw::ring(d, cx, MID, hit_r + 3, accent, 1);
        }
    }
    draw::fill_rect(d, x(0.5), MID + 12, 1, 4, theme::MID);
}

/// The chips' top and height, and the gap between them.
const CHIP_TOP: i32 = 124;
const CHIP_H: i32 = 38;
const CHIP_GAP: i32 = 4;
/// The loop's line under the chips.
const LOOP_Y: i32 = CHIP_TOP + CHIP_H + 8;

/// CHRD: a chip per segment, its degree over its chord, the one playing
/// lit, the one DEGREE edits outlined, a played chord dotted; under them
/// the loop's bars and where the clock is in them.
pub fn chords<D>(d: &mut D, f: &OrbitFrame)
where
    D: DrawTarget<Color = Rgb565>,
{
    let c = f.chords();
    let st = &f.ui.status;
    let n = c.segs.get() as i32;
    let cw = (VIZ_RIGHT - VIZ_LEFT - CHIP_GAP * (n - 1)) / n;
    let playing = st.running.then_some(st.seg as usize);
    for i in 0..n as usize {
        let x = VIZ_LEFT + i as i32 * (cw + CHIP_GAP);
        let lit = playing == Some(i);
        let (fill, ink, sub) = if lit {
            (theme::ACCENT, theme::BG, theme::BG)
        } else {
            (theme::ACCENT_SOFT, theme::INK, theme::MID)
        };
        draw::round_rect(d, x, CHIP_TOP, cw, CHIP_H, 5, fill);
        if i == f.ui.seg_of(c) {
            draw::round_outline(d, x, CHIP_TOP, cw, CHIP_H, 5, theme::INK2);
        }
        if c.played[i].is_some() {
            draw::dot(d, x + cw - 6, CHIP_TOP + 6, 2, theme::INK2);
        }
        let mid = x + cw / 2;
        let top = numeral(c, i);
        let font = if draw::text_width(&theme::FONT_VALUE, top.as_str(), 0) <= cw - 4 {
            &theme::FONT_VALUE
        } else {
            &theme::FONT_LABEL_BOLD
        };
        draw::text_center(d, font, top.as_str(), mid, CHIP_TOP + 18, ink, 0);
        let name = chord_name(c, i);
        if draw::text_width(&theme::FONT_LABEL, name.as_str(), 0) <= cw - 4 {
            draw::text_center(
                d,
                &theme::FONT_LABEL,
                name.as_str(),
                mid,
                CHIP_TOP + 32,
                sub,
                0,
            );
        }
    }
    let bars = c.bars.get() as i32;
    draw::fill_rect(d, VIZ_LEFT, LOOP_Y, VIZ_RIGHT - VIZ_LEFT, 1, theme::FAINT);
    for b in 0..=bars {
        let x = VIZ_LEFT + (VIZ_RIGHT - VIZ_LEFT) * b / bars;
        draw::fill_rect(d, x, LOOP_Y - 2, 1, 5, theme::MID);
    }
    if let Some(x) = loop_x(f) {
        draw::dot(d, x, LOOP_Y, 3, theme::ACCENT);
    }
}

/// Where the clock is along the loop's line, while it runs.
fn loop_x(f: &OrbitFrame) -> Option<i32> {
    let st = &f.ui.status;
    let bars = f.chords().bars.get() as u32;
    let into = (st.pos.bar % bars) as f32 + st.pos.phase.get() as f32 / 4_294_967_296.0;
    st.running
        .then(|| VIZ_LEFT + (BAND_W * into / bars as f32) as i32)
}

/// The circle's centre and radius, and its note names' radius.
const CIRCLE_X: i32 = theme::SCREEN_W / 2;
const CIRCLE_Y: i32 = 108;
const CIRCLE_R: f32 = 50.0;
const NAMES_R: f32 = 66.0;

/// The point `at` twelfths round from the top, `r` out.
fn round(at: f32, r: f32) -> (i32, i32) {
    let a = at * TAU / 12.0 - TAU / 4.0;
    (
        CIRCLE_X + libm::roundf(r * libm::cosf(a)) as i32,
        CIRCLE_Y + libm::roundf(r * libm::sinf(a)) as i32,
    )
}

/// CHRD › CUSTOM: the twelve notes from the tonic round a circle, named;
/// the set's lit and joined into its shape, the root locked on; the cursor
/// ringed at `cursor` (0..1, eased); the set's name and size in the middle.
pub fn circle<D>(d: &mut D, f: &OrbitFrame, cursor: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let c = f.chords();
    let set = c.custom;
    let ins = || (0..12).filter(move |&i| set.contains(Interval::ALL[i]));
    let count = ins().count();
    if count > 1 {
        for (a, b) in ins().zip(ins().cycle().skip(1)) {
            let ((x0, y0), (x1, y1)) = (round(a as f32, CIRCLE_R), round(b as f32, CIRCLE_R));
            draw::line(d, x0, y0, x1, y1, theme::FAINT, 1);
        }
    }
    for i in 0..12 {
        let (x, y) = round(i as f32, CIRCLE_R);
        let on = set.contains(Interval::ALL[i]);
        match (i, on) {
            (0, _) => draw::dot(d, x, y, 6, theme::INK),
            (_, true) => draw::dot(d, x, y, 5, theme::ACCENT),
            (_, false) => {
                draw::dot(d, x, y, 4, theme::BG);
                draw::ring(d, x, y, 4, theme::MID, 1);
            }
        }
        let mut name = FmtBuf::new();
        let _ = write!(name, "{}", c.tonic.up(Interval::ALL[i]));
        let (lx, ly) = round(i as f32, NAMES_R);
        let ink = if on { theme::INK2 } else { theme::MID };
        draw::text_center(d, &theme::FONT_LABEL, name.as_str(), lx, ly + 4, ink, 0);
    }
    let (x, y) = round(cursor.clamp(0.0, 1.0) * 11.0, CIRCLE_R);
    draw::ring(d, x, y, 9, theme::INK, 1);
    let mut name = FmtBuf::new();
    let _ = write!(name, "{set}");
    draw::text_center(
        d,
        &theme::FONT_VALUE,
        name.as_str(),
        CIRCLE_X,
        CIRCLE_Y + 2,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    let mut size = FmtBuf::new();
    let _ = write!(size, "{count} NOTES");
    draw::text_center(
        d,
        &theme::FONT_LABEL,
        size.as_str(),
        CIRCLE_X,
        CIRCLE_Y + 16,
        theme::MID,
        theme::LABEL_TRACKING,
    );
}

/// Segment `seg`'s degree as a numeral: "iv", "V7".
fn numeral(c: &ChordRing, seg: usize) -> FmtBuf {
    let mut b = FmtBuf::new();
    let _ = write!(b, "{}", c.key().degree_label(c.degrees[seg], Stack::Triad));
    b
}

/// Segment `seg`'s chord by name, a played one's if it has one.
fn chord_name(c: &ChordRing, seg: usize) -> FmtBuf {
    let mut b = FmtBuf::new();
    let _ = write!(b, "{}", harmony(c, seg).chord);
    b
}

/// The note `cursor` names, above the tonic.
fn cursor_note(f: &OrbitFrame) -> FmtBuf {
    let mut b = FmtBuf::new();
    let _ = write!(b, "{}", f.chords().tonic.up(f.ui.interval()));
    b
}

/// An ORBIT cell's value as its cell shows it.
pub fn cell_text(cell: OrbitCell, f: &OrbitFrame) -> FmtBuf {
    let c = f.chords();
    match cell {
        OrbitCell::Degree => {
            let seg = f.ui.seg_of(c);
            let mut b = numeral(c, seg);
            let _ = write!(b, " {}", chord_name(c, seg).as_str());
            if draw::text_width(&theme::FONT_VALUE, b.as_str(), 0) <= theme::CELL_COL_W - 6 {
                b
            } else {
                numeral(c, seg)
            }
        }
        OrbitCell::Cursor => cursor_note(f),
        OrbitCell::Set => {
            let mut b = FmtBuf::new();
            let _ = b.write_str(
                match (f.ui.interval(), c.custom.contains(f.ui.interval())) {
                    (Interval::UNISON, _) => "ROOT",
                    (_, true) => "ON",
                    (_, false) => "OFF",
                },
            );
            b
        }
    }
}

/// What the focus band shows in place of a slot's own reading.
pub struct Focus {
    pub label: FmtBuf,
    pub text: FmtBuf,
    pub gauge: Gauge,
}

impl Focus {
    /// What the band draws, for its region's key.
    pub fn key(&self) -> u32 {
        let mut h = Fnv::new();
        h.write(self.label.as_str().as_bytes());
        h.write(self.text.as_str().as_bytes());
        let g = match self.gauge {
            Gauge::Arc { value, .. } | Gauge::Switch { on: value } => value,
            _ => 0.0,
        };
        h.write_u16(crate::ui::region::quantize(g));
        h.finish() as u32
    }
}

/// ORBIT's reading of `slot` on `def`, from the slots' eased `values`:
/// HITS as HITS/STEPS, its arc the fill; DEGREE as its segment's chord;
/// CUSTOM's note and whether it is in. `None` elsewhere.
pub fn focus(
    def: &BlockDef,
    slot: usize,
    ctx: &SlotCtx,
    f: &OrbitFrame,
    values: impl Fn(usize) -> f32,
) -> Option<Focus> {
    let mut label = FmtBuf::new();
    let mut text = FmtBuf::new();
    let gauge = match view(def, slot, ctx) {
        View::Orbit(OrbitCell::Degree) => {
            let c = f.chords();
            let seg = f.ui.seg_of(c);
            let _ = write!(label, "SEGMENT {} · {}", seg + 1, numeral(c, seg).as_str());
            text = chord_name(c, seg);
            Gauge::None
        }
        View::Orbit(OrbitCell::Cursor) => {
            let _ = label.write_str(OrbitCell::Cursor.label());
            text = cursor_note(f);
            Gauge::None
        }
        View::Orbit(OrbitCell::Set) => {
            let _ = label.write_str(OrbitCell::Set.label());
            text = cell_text(OrbitCell::Set, f);
            Gauge::Switch { on: values(slot) }
        }
        View::Param { addr, label: l, .. }
            if matches!(addr.block, BlockRef::OrbitRing(_)) && addr.param == SceneRing::HITS =>
        {
            let steps = ParamAddr::new(addr.block, SceneRing::STEPS);
            let at = (0..def.params.len()).find(|&i| view(def, i, ctx).addr() == Some(steps))?;
            let real = |a: ParamAddr, i: usize| {
                let s = a.spec()?;
                Some(s.min + values(i) * (s.max - s.min))
            };
            let (hits, steps) = (real(addr, slot)?, real(steps, at)? + 1.0);
            let _ = label.write_str(l);
            let _ = write!(text, "{}/{}", libm::roundf(hits), libm::roundf(steps));
            Gauge::Arc {
                value: (hits / steps).clamp(0.0, 1.0),
                bipolar: false,
            }
        }
        _ => return None,
    };
    Some(Focus { label, text, gauge })
}

/// What `def`'s viz draws of ORBIT, on ring `r`: its region redraws when
/// this moves. 0 on any other viz.
pub fn viz_key(def: &BlockDef, f: &OrbitFrame, r: RingIdx) -> u32 {
    let mut h = Fnv::new();
    let st = &f.ui.status;
    match def.viz {
        VizType::OrbitRhythm => {
            f.scene.rings[r.get() as usize].ring.hash(&mut h);
            f.hits(r).hash(&mut h);
            f.now(r).hash(&mut h);
        }
        VizType::OrbitChords => {
            f.chords().hash(&mut h);
            f.ui.seg_of(f.chords()).hash(&mut h);
            st.running.then_some(st.seg).hash(&mut h);
            loop_x(f).hash(&mut h);
        }
        VizType::OrbitCircle => f.chords().hash(&mut h),
        _ => return 0,
    }
    h.finish() as u32
}

/// FNV-1a, 32 bits wide in a `Hasher`.
struct Fnv(u32);

impl Fnv {
    const fn new() -> Self {
        Fnv(0x811c_9dc5)
    }
}

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0 as u64
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u32).wrapping_mul(0x0100_0193);
        }
    }
}
