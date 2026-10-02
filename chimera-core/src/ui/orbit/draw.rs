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
use crate::orbit::clock::{Bpm, ClockSource};
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
    /// The song's tempo, as BPM sets it.
    pub bpm: Bpm,
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

    /// The tempo the header reads and its source: BPM's own, or the MIDI
    /// clock's measured one.
    pub fn tempo(&self) -> (Bpm, &'static str) {
        match self.ui.status.source {
            ClockSource::Internal => (self.bpm, "INT"),
            ClockSource::Midi => (self.ui.status.bpm, "MIDI"),
        }
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

/// ORB's centre.
pub const ORB_CX: i32 = theme::SCREEN_W / 2;
pub const ORB_CY: i32 = 116;
/// The progression band's middle radius, and its width.
pub const BAND_R: i32 = 66;
/// Ring 1's radius, and the step in from one ring to the next.
const RING1_R: i32 = 56;
const RING_STEP: i32 = 9;

/// Ring `r`'s radius on ORB: ring 1 outermost.
pub const fn ring_r(r: RingIdx) -> i32 {
    RING1_R - RING_STEP * r.get() as i32
}

/// The point `turn` (0..1, clockwise from the top) round ORB, `radius` out.
pub fn orb_xy(turn: f32, radius: f32) -> (i32, i32) {
    let a = turn * TAU - TAU / 4.0;
    (
        ORB_CX + libm::roundf(radius * libm::cosf(a)) as i32,
        ORB_CY + libm::roundf(radius * libm::sinf(a)) as i32,
    )
}

/// The progression band's width and the gap between its segments.
const BAND_W_PX: u32 = 5;
const BAND_GAP_DEG: f32 = 4.0;
/// The sweep runs from here out to inside the band.
const HAND_IN: f32 = 10.0;

/// ORB: the key, the loop and the tempo over everything; the progression
/// round the outside, the segment playing lit; the selected ring's track;
/// the sweep at the bar's phase; inside, each ring's steps round its own
/// circle (ring 1 outermost), a hit filled, an accent ringed, the step
/// under the clock lit and flashing as it fires; a muted ring dark.
pub fn orb<D>(d: &mut D, f: &OrbitFrame, sel: RingIdx)
where
    D: DrawTarget<Color = Rgb565>,
{
    let c = f.chords();
    let st = &f.ui.status;
    let bars = c.bars.get();
    let mut line = FmtBuf::new();
    let plural = if bars == 1 { "" } else { "S" };
    let (bpm, source) = f.tempo();
    let _ = write!(
        line,
        "{} {} · {bars} BAR{plural} · {} {source}",
        c.tonic,
        c.scale.name(),
        bpm.get()
    );
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        line.as_str(),
        theme::MARGIN_X,
        theme::BIGVIZ_STATUS_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    );

    let segs = c.segs.get() as usize;
    let each = 360.0 / segs as f32;
    let gap = if segs > 1 { BAND_GAP_DEG } else { 0.0 };
    let playing = st.running.then_some(st.seg as usize);
    for s in 0..segs {
        let lit = if playing == Some(s) {
            theme::ACCENT
        } else {
            theme::ACCENT_SOFT
        };
        let start = -90.0 + each * s as f32 + gap / 2.0;
        draw::arc(d, ORB_CX, ORB_CY, BAND_R, BAND_W_PX, start, each - gap, lit);
    }

    draw::ring(d, ORB_CX, ORB_CY, ring_r(sel), theme::FAINT, 1);
    let turn = f.ui.anim.sweep();
    let (x0, y0) = orb_xy(turn, HAND_IN);
    let (x1, y1) = orb_xy(turn, (BAND_R - BAND_W_PX as i32) as f32);
    draw::line(d, x0, y0, x1, y1, theme::MID, 1);

    for r in RingIdx::ALL {
        orb_ring(d, f, r);
    }
}

/// One ring's steps round its circle on ORB.
fn orb_ring<D>(d: &mut D, f: &OrbitFrame, r: RingIdx)
where
    D: DrawTarget<Color = Rgb565>,
{
    let ring = &f.scene.rings[r.get() as usize].ring;
    let mask = f.hits(r);
    let n = mask.steps().get() as usize;
    let now = f.now(r);
    let accented = accents(mask, ring.accent);
    let flash = f.ui.anim.flash(r) as i32;
    let (hit, rest, accent) = if ring.mute {
        (theme::PILL_EDGE, theme::FAINT, theme::PILL_EDGE)
    } else {
        (theme::ACCENT, theme::BAR_REST, theme::INK2)
    };
    let radius = ring_r(r) as f32;
    for i in 0..n {
        let (x, y) = orb_xy(i as f32 / n as f32, radius);
        if !mask.is_hit(i) {
            draw::dot(d, x, y, 1, rest);
            continue;
        }
        let under = now == Some(i) && !ring.mute;
        // A flash grows the dot by up to 2 px, fading with it.
        let grow = if under { (2 * flash + 127) / 255 } else { 0 };
        let c = if under { theme::INK } else { hit };
        draw::dot(d, x, y, 2 + grow, c);
        if accented.is_hit(i) {
            draw::ring(d, x, y, 4, accent, 1);
        }
    }
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

/// The note `to` (0..12) nearest `from` round the circle, in unwrapped
/// twelfths: what CUSTOM's cursor eases toward, the short way.
pub fn nearest_turn(from: f32, to: u8) -> f32 {
    nearest(from, to as f32, 12.0)
}

/// `x` into 0..period.
pub fn wrap(x: f32, period: f32) -> f32 {
    let r = libm::fmodf(x, period);
    if r < 0.0 { r + period } else { r }
}

/// `to`, or `to` some whole `period`s away, whichever is nearest `from`:
/// a value round a circle, unwrapped so it eases the short way.
pub fn nearest(from: f32, to: f32, period: f32) -> f32 {
    let half = period / 2.0;
    let d = libm::fmodf(to - from, period);
    let d = if d < -half {
        d + period
    } else if d >= half {
        d - period
    } else {
        d
    };
    from + d
}

/// CHRD › CUSTOM: the twelve notes from the tonic round a circle, named;
/// the set's lit and joined into its shape, the root locked on; the cursor
/// ringed at `cursor` (twelfths from the root, eased, any turn); the set's
/// name and size in the middle.
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
    let (x, y) = round(cursor, CIRCLE_R);
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

/// An ORBIT cell's value as its cell shows it, on ring `ring`.
pub fn cell_text(cell: OrbitCell, f: &OrbitFrame, ring: RingIdx) -> FmtBuf {
    let c = f.chords();
    match cell {
        OrbitCell::Ring => {
            let mut b = FmtBuf::new();
            let _ = write!(b, "{}", ring.get() + 1);
            b
        }
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
            text = cell_text(OrbitCell::Set, f, ctx.ring);
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

/// What `def`'s viz draws of ORBIT, on ring `r` with CUSTOM's eased
/// `cursor`: its region redraws when this moves. 0 on any other viz.
pub fn viz_key(def: &BlockDef, f: &OrbitFrame, r: RingIdx, cursor: f32) -> u32 {
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
        VizType::Orbit => {
            for ring in RingIdx::ALL {
                f.scene.rings[ring.get() as usize].ring.hash(&mut h);
                f.hits(ring).hash(&mut h);
                f.now(ring).hash(&mut h);
                f.ui.anim.flash(ring).hash(&mut h);
            }
            f.chords().hash(&mut h);
            f.tempo().hash(&mut h);
            st.running.then_some(st.seg).hash(&mut h);
            r.hash(&mut h);
            h.write_u16(crate::ui::region::quantize(f.ui.anim.sweep() * 64.0));
        }
        VizType::OrbitCircle => {
            f.chords().hash(&mut h);
            h.write_u16(crate::ui::region::quantize(
                libm::fmodf(cursor, 12.0) + 12.0,
            ));
        }
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
