//! What ORBIT's pages draw: RHYTHM's pattern unrolled, VOICE's gates as
//! bars, NOTES' chips, CHRD's segments as chips over the loop's bars, and
//! the CUSTOM circle; the text of ORBIT's
//! own cells and focus band; and the keys that redraw them. Drawn from the
//! live scene and the runner's last view, so a drift shows as it plays.

use core::fmt::Write;
use core::hash::{Hash, Hasher};

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use super::{OrbitCell, OrbitUi};
use crate::addr::{BlockRef, ParamAddr};
use crate::orbit::RingIdx;
use crate::orbit::chords::{ChordRing, harmony};
use crate::orbit::clock::{Bpm, ClockSource};
use crate::orbit::euclid::HitMask;
use crate::orbit::ring::{ACCENT_BOOST, NoteMode, Ring};
use crate::orbit::runner::Capture;
use crate::orbit::scene::{OrbitSong, Scene, SceneIdx, SceneRing};
use crate::ui::block_def::{BlockDef, VizType};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::Gauge;
use crate::ui::region::Fnv;
use crate::ui::theme::{self, VIZ_BAND_MID as MID, VIZ_LEFT, VIZ_RIGHT};
use crate::ui::view::{SlotCtx, View, view};
use chimera_theory::{Interval, Note, Stack};
use u8g2_fonts::FontRenderer;

/// What ORBIT's pages draw from: the scene being edited and the UI's
/// selections and last view of the runner.
#[derive(Clone, Copy)]
pub struct OrbitFrame<'a> {
    /// The scenes, which plays and which waits.
    pub song: &'a OrbitSong,
    pub scene: &'a Scene,
    /// The song's tempo, as BPM sets it.
    pub bpm: Bpm,
    pub ui: &'a OrbitUi,
}

impl<'a> OrbitFrame<'a> {
    pub fn new(song: &'a OrbitSong, ui: &'a OrbitUi) -> Self {
        OrbitFrame {
            song,
            scene: &song.live,
            bpm: song.bpm,
            ui,
        }
    }

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
    let accented = ring.accents_on(mask);
    let odds = ring.chance.get() as f32 / 100.0;
    // A hit's filled core: as wide as CHANCE lets it sound.
    let core = libm::roundf(hit_r as f32 * libm::sqrtf(odds)) as i32;
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

/// The line atop VOICE's and NOTES' bands.
const LINE_Y: i32 = theme::VIZ_BAND_TOP + 10;

/// A line atop the band, in the labels' voice, from its parts (a
/// `FmtBuf` holds 32 bytes); untracked when tracked it would not fit.
fn band_line<D>(d: &mut D, parts: &[&str])
where
    D: DrawTarget<Color = Rgb565>,
{
    let font = &theme::FONT_LABEL;
    let width = |t| {
        parts
            .iter()
            .map(|s| draw::text_width(font, s, t))
            .sum::<i32>()
    };
    let tracking = if width(theme::LABEL_TRACKING) <= VIZ_RIGHT - VIZ_LEFT {
        theme::LABEL_TRACKING
    } else {
        0
    };
    let mut x = VIZ_LEFT;
    for s in parts {
        draw::text_tracked(d, font, s, x, LINE_Y, theme::MID, tracking);
        x += draw::text_width(font, s, tracking);
    }
}

/// VOICE's bars stand on this line, at most this tall.
const BARS_BASE: i32 = theme::VIZ_BAND_BOTTOM - 6;
const BARS_H: f32 = 40.0;

/// RING · VOICE: each hit a bar as long as GATE holds it, as tall as its
/// velocity, an accent taller (VEL + 30) and in INK2; a rest a dot; the
/// step under the clock lit; all dark while MUTE holds it. Above, the
/// register OCTAVE and RANGE span, and what an accent adds.
pub fn voice<D>(d: &mut D, f: &OrbitFrame, r: RingIdx)
where
    D: DrawTarget<Color = Rgb565>,
{
    let ring = &f.scene.rings[r.get() as usize].ring;
    let mut line = FmtBuf::new();
    let (lo, hi) = register(ring);
    let _ = write!(line, "{lo} TO {hi}");
    if ring.accent() > 0 {
        let _ = write!(line, " · ACCENT +{ACCENT_BOOST} VEL");
    }
    band_line(d, &[line.as_str()]);

    let mask = f.hits(r);
    let n = mask.steps().get() as usize;
    let now = f.now(r);
    let w = BAND_W / n as f32;
    let x = |at: f32| VIZ_LEFT + (w * at) as i32;
    let accented = ring.accents_on(mask);
    let gate = ring.gate.get() as f32 / 100.0;
    let (hit, accent, rest) = if ring.mute {
        (theme::PILL_EDGE, theme::PILL_EDGE, theme::FAINT)
    } else {
        (theme::ACCENT, theme::INK2, theme::BAR_REST)
    };
    let top = BARS_BASE - BARS_H as i32 - 2;
    for i in 0..n {
        let x0 = x(i as f32);
        let cell = x(i as f32 + 1.0) - x0;
        if now == Some(i) {
            draw::fill_rect(d, x0, top, cell, BARS_BASE + 3 - top, theme::ACCENT_SOFT);
        }
        if !mask.is_hit(i) {
            draw::dot(d, x0 + cell / 2, BARS_BASE, 1, rest);
            continue;
        }
        let acc = accented.is_hit(i);
        let vel = if acc {
            ring.velocity.get().saturating_add(ACCENT_BOOST).min(127)
        } else {
            ring.velocity.get()
        };
        let h = libm::roundf(BARS_H * vel as f32 / 127.0).max(1.0) as i32;
        let bw = libm::roundf((cell - 2) as f32 * gate).max(1.0) as i32;
        let c = match (now == Some(i) && !ring.mute, acc) {
            (true, _) => theme::INK,
            (false, true) => accent,
            (false, false) => hit,
        };
        draw::fill_rect(d, x0 + 1, BARS_BASE - h, bw, h, c);
    }
    draw::fill_rect(
        d,
        VIZ_LEFT,
        BARS_BASE + 1,
        VIZ_RIGHT - VIZ_LEFT,
        1,
        theme::FAINT,
    );
}

/// The lowest and highest notes of a ring's register: OCTAVE's C, up
/// RANGE + 1 octaves.
fn register(ring: &Ring) -> (Note, Note) {
    let lo = ring.octave.low();
    let hi = lo.get() as u16 + 12 * (ring.range.octaves() as u16 + 1) - 1;
    (lo, Note::new(hi.min(127) as u8).unwrap_or(Note::MAX))
}

/// What NOTES says of a ring: a pool's size against the hits, and how
/// many hits until the walk and the hits line up again (`-` for RANDOM,
/// which never does); a STEP ring's steps and hits, which come round
/// with the ring. In two parts, each within a `FmtBuf`.
pub fn notes_line(r: &Ring) -> (FmtBuf, FmtBuf) {
    let (mut head, mut tail) = (FmtBuf::new(), FmtBuf::new());
    let hits = r.hits_mask().hits();
    match r.mode {
        NoteMode::Pool => {
            let _ = write!(head, "POOL {} · HITS {hits} · ", r.pool.len());
            let _ = match r.comes_round_in() {
                Some(n) => write!(tail, "COMES ROUND IN {n}"),
                None => tail.write_str("COMES ROUND IN -"),
            };
        }
        NoteMode::Step => {
            let _ = write!(head, "STEPS {} · HITS {hits}", r.fill().steps().get());
        }
    }
    (head, tail)
}

/// NOTES' chips: their top and height.
const NOTE_TOP: i32 = theme::VIZ_BAND_TOP + 18;
const NOTE_H: i32 = 34;

/// RING · NOTES: the line, then the pool as chips, each named; a STEP
/// ring's steps instead, a hit's filled, a rest's outlined, the step under
/// the clock lit. The chip NOTE edits is outlined in INK2. Chips too
/// narrow for their names mark each pitch in the register by height.
pub fn notes<D>(d: &mut D, f: &OrbitFrame, r: RingIdx)
where
    D: DrawTarget<Color = Rgb565>,
{
    let sr = &f.scene.rings[r.get() as usize];
    let ring = &sr.ring;
    let (head, tail) = notes_line(ring);
    band_line(d, &[head.as_str(), tail.as_str()]);
    let n = sr.chips() as i32;
    let gap = match n {
        17.. => 1,
        9..=16 => 2,
        _ => 4,
    };
    // Each chip a slot of the band, laid at fractional x so the row
    // spans it; `cw` the narrowest, for the font and the corners.
    let slot = (BAND_W + gap as f32) / n as f32;
    let left = |i: i32| VIZ_LEFT + libm::roundf(slot * i as f32) as i32;
    let cw = libm::floorf(slot) as i32 - gap;
    let radius = (cw / 2).clamp(1, 5) as u32;
    let mask = f.hits(r);
    let now = f.now(r);
    let (lo, hi) = register(ring);
    let step = ring.mode == NoteMode::Step;
    // One font for every chip: the largest every name fits.
    let fits = |font| {
        (0..n as usize).all(|i| draw::text_width(font, note_name(sr.chip(i)).as_str(), 0) <= cw - 4)
    };
    let font = [&theme::FONT_VALUE, &theme::FONT_LABEL_BOLD]
        .into_iter()
        .find(|&font| fits(font));
    for i in 0..n as usize {
        let x = left(i as i32);
        let cw = left(i as i32 + 1) - gap - x;
        let (fill, ink) = match (step, step && mask.is_hit(i), now == Some(i)) {
            (true, _, true) => (Some(theme::ACCENT), theme::BG),
            (true, false, _) => (None, theme::MID),
            _ => (Some(theme::ACCENT_SOFT), theme::INK),
        };
        match fill {
            Some(c) => draw::round_rect(d, x, NOTE_TOP, cw, NOTE_H, radius, c),
            None => draw::round_outline(d, x, NOTE_TOP, cw, NOTE_H, radius, theme::FAINT),
        }
        if i == f.ui.chip_of(sr) {
            draw::round_outline(d, x, NOTE_TOP, cw, NOTE_H, radius, theme::INK2);
        }
        let note = sr.chip(i);
        match font {
            Some(font) => {
                let y = NOTE_TOP + NOTE_H / 2 + 5;
                let name = note_name(note);
                draw::text_center(d, font, name.as_str(), x + cw / 2, y, ink, 0);
            }
            None => {
                let span = (hi.get() - lo.get()).max(1) as f32;
                let at = (note.get().clamp(lo.get(), hi.get()) - lo.get()) as f32 / span;
                let y = NOTE_TOP + NOTE_H - 4 - libm::roundf(at * (NOTE_H - 8) as f32) as i32;
                draw::fill_rect(d, x + 1, y, (cw - 2).max(1), 2, ink);
            }
        }
    }
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
    draw::polar(ORB_CX, ORB_CY, radius, turn)
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
    let accented = ring.accents_on(mask);
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
/// A played chord's name's baseline, over its dot.
const PLAYED_Y: i32 = CHIP_TOP + 22;
/// A played chord's name, largest first.
static CHIP_FONTS: [&FontRenderer; 3] = [
    &theme::FONT_VALUE,
    &theme::FONT_LABEL_BOLD,
    &theme::FONT_LABEL,
];
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
        let mid = x + cw / 2;
        let name = chord_name(c, i);
        if c.played[i].is_some() {
            // A played chord has no degree: its name is the main line, in
            // the largest font it fits, else its root's; its dot under it,
            // centred, clear of any text however narrow the chip.
            let mut root = FmtBuf::new();
            let _ = write!(root, "{}", harmony(c, i).chord.root());
            let fit = [name.as_str(), root.as_str()].into_iter().find_map(|t| {
                let font = CHIP_FONTS
                    .iter()
                    .find(|&&f| draw::text_width(f, t, 0) <= cw - 4)?;
                Some((t, *font))
            });
            if let Some((t, font)) = fit {
                draw::text_center(d, font, t, mid, PLAYED_Y, ink, 0);
            }
            draw::dot(d, mid, CHIP_TOP + CHIP_H - 7, 2, theme::INK2);
            continue;
        }
        let top = numeral(c, i);
        let font = if draw::text_width(&theme::FONT_VALUE, top.as_str(), 0) <= cw - 4 {
            &theme::FONT_VALUE
        } else {
            &theme::FONT_LABEL_BOLD
        };
        draw::text_center(d, font, top.as_str(), mid, CHIP_TOP + 18, ink, 0);
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

/// SCN's slots: two rows of four in the band.
const SLOT_COLS: i32 = 4;
const SLOT_GAP: i32 = 4;
const SLOT_TOP: i32 = theme::VIZ_BAND_TOP + 4;
const SLOT_H: i32 = 28;
const SLOT_W: i32 = (VIZ_RIGHT - VIZ_LEFT - SLOT_GAP * (SLOT_COLS - 1)) / SLOT_COLS;
const _: () = assert!(SLOT_TOP + 2 * SLOT_H + SLOT_GAP <= theme::VIZ_BAND_BOTTOM);

/// How far through its loop the playing scene is, 0..1, while it runs, by
/// the runner's own BARS: CHRD's loop dot, and how full a queued slot is.
fn loop_done(f: &OrbitFrame) -> Option<f32> {
    let st = &f.ui.status;
    let bars = st.bars.get() as u32;
    let into = (st.pos.bar % bars) as f32 + st.pos.phase.turns();
    st.running.then_some(into / bars as f32)
}

/// A queued slot's fill, in pixels, `done` of the way through the loop.
fn fill_w(done: f32) -> i32 {
    ((SLOT_W - 4) as f32 * done) as i32
}

/// SCN: the eight slots, a saved one filled and a neutral one an outline,
/// the playing one ringed, a queued one filling as the loop runs out, and
/// SAVE's slot dotted.
pub fn scenes<D>(d: &mut D, f: &OrbitFrame)
where
    D: DrawTarget<Color = Rgb565>,
{
    let song = f.song;
    let save = f.ui.save_slot(song);
    let saved_slots = song.saved();
    for n in SceneIdx::ALL {
        let i = n.get() as i32;
        let x = VIZ_LEFT + (i % SLOT_COLS) * (SLOT_W + SLOT_GAP);
        let y = SLOT_TOP + (i / SLOT_COLS) * (SLOT_H + SLOT_GAP);
        let saved = saved_slots.contains(n);
        if saved {
            draw::round_rect(d, x, y, SLOT_W, SLOT_H, 5, theme::ACCENT_SOFT);
        } else {
            draw::round_outline(d, x, y, SLOT_W, SLOT_H, 5, theme::PILL_EDGE);
        }
        if song.queued() == Some(n) {
            let w = fill_w(loop_done(f).unwrap_or(0.0));
            draw::fill_rect(d, x + 2, y + 2, w, SLOT_H - 4, theme::ACCENT);
        }
        if song.current() == n {
            draw::round_outline(d, x, y, SLOT_W, SLOT_H, 5, theme::ACCENT);
            draw::round_outline(d, x + 1, y + 1, SLOT_W - 2, SLOT_H - 2, 4, theme::ACCENT);
        }
        let ink = if saved || song.queued() == Some(n) {
            theme::INK
        } else {
            theme::MID
        };
        let mut b = FmtBuf::new();
        let _ = write!(b, "{}", i + 1);
        draw::text_center(
            d,
            &theme::FONT_VALUE,
            b.as_str(),
            x + SLOT_W / 2,
            y + 18,
            ink,
            0,
        );
        if save == n {
            draw::dot(d, x + SLOT_W - 8, y + SLOT_H - 8, 2, theme::INK2);
        }
    }
}

/// Where the clock is along the loop's line, while it runs.
fn loop_x(f: &OrbitFrame) -> Option<i32> {
    loop_done(f).map(|d| VIZ_LEFT + (BAND_W * d) as i32)
}

/// The circle's centre and radius, and its note names' radius.
const CIRCLE_X: i32 = theme::SCREEN_W / 2;
const CIRCLE_Y: i32 = 108;
const CIRCLE_R: f32 = 50.0;
const NAMES_R: f32 = 66.0;

/// The point `at` twelfths round from the top, `r` out.
fn round(at: f32, r: f32) -> (i32, i32) {
    draw::polar(CIRCLE_X, CIRCLE_Y, r, at / 12.0)
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

/// Segment `seg`'s degree's chord by name, under any played one: what
/// DEGREE edits, and what plays once PLAY clears.
fn degree_name(c: &ChordRing, seg: usize) -> FmtBuf {
    let mut c = *c;
    c.played[seg] = None;
    chord_name(&c, seg)
}

/// Segment `seg`'s chord by name, a played one's if it has one.
fn chord_name(c: &ChordRing, seg: usize) -> FmtBuf {
    let mut b = FmtBuf::new();
    let _ = write!(b, "{}", harmony(c, seg).chord);
    b
}

/// A note by name: "A4", "C#5".
fn note_name(n: Note) -> FmtBuf {
    let mut b = FmtBuf::new();
    let _ = write!(b, "{n}");
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
            let _ = write!(b, " {}", degree_name(c, seg).as_str());
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
        OrbitCell::Note => {
            let r = &f.scene.rings[ring.get() as usize];
            note_name(r.chip(f.ui.chip_of(r)))
        }
        OrbitCell::Learn => {
            let mut b = FmtBuf::new();
            let on = f.ui.capture == Capture::Ring(ring);
            let _ = b.write_str(if on { "ON" } else { "OFF" });
            b
        }
        OrbitCell::Play => {
            let seg = f.ui.seg_of(c);
            let mut b = FmtBuf::new();
            match (f.ui.capture == Capture::Segment(seg as u8), c.played[seg]) {
                (true, _) => {
                    let _ = b.write_str("HOLD");
                }
                (false, Some(ch)) => {
                    let _ = write!(b, "{ch}");
                }
                // No em dash in the fonts.
                (false, None) => {
                    let _ = b.write_str("-");
                }
            }
            b
        }
        OrbitCell::Clear => {
            let r = &f.scene.rings[ring.get() as usize].ring;
            let mut b = note_name(r.octave.low());
            let _ = b.write_str(" / RND");
            b
        }
        OrbitCell::Scene => {
            let mut b = FmtBuf::new();
            let _ = write!(
                b,
                "{}",
                f.song.queued().unwrap_or(f.song.current()).get() + 1
            );
            b
        }
        // No arrow in the fonts.
        OrbitCell::Save => {
            let mut b = FmtBuf::new();
            let _ = write!(b, "TO {}", f.ui.save_slot(f.song).get() + 1);
            b
        }
        OrbitCell::Switch => {
            let mut b = FmtBuf::new();
            let _ = b.write_str("LOOP");
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
        h.get()
    }
}

/// ORBIT's reading of `slot` on `def`, from the slots' eased `values`:
/// HITS as HITS/STEPS, its arc the fill; DEGREE as its segment's chord;
/// CUSTOM's note and whether it is in; NOTES' chip, LEARN and CLEAR; SCN's
/// cells. `None` elsewhere.
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
            text = degree_name(c, seg);
            Gauge::None
        }
        View::Orbit(OrbitCell::Cursor) => {
            let _ = label.write_str(OrbitCell::Cursor.label());
            text = cursor_note(f);
            Gauge::None
        }
        View::Orbit(OrbitCell::Note) => {
            let r = &f.scene.rings[ctx.ring.get() as usize];
            let what = match r.ring.mode {
                NoteMode::Pool => "NOTE",
                NoteMode::Step => "STEP",
            };
            let _ = write!(label, "{what} {} OF {}", f.ui.chip_of(r) + 1, r.chips());
            text = cell_text(OrbitCell::Note, f, ctx.ring);
            Gauge::None
        }
        View::Orbit(OrbitCell::Learn) => {
            let _ = label.write_str(OrbitCell::Learn.label());
            text = cell_text(OrbitCell::Learn, f, ctx.ring);
            Gauge::Switch { on: values(slot) }
        }
        View::Orbit(OrbitCell::Play) => {
            let seg = f.ui.seg_of(f.chords()) + 1;
            let _ = write!(label, "SEGMENT {seg} · PLAY");
            text = cell_text(OrbitCell::Play, f, ctx.ring);
            Gauge::None
        }
        View::Orbit(OrbitCell::Clear) => {
            let _ = label.write_str("MIX + UP / DOWN");
            text = cell_text(OrbitCell::Clear, f, ctx.ring);
            Gauge::None
        }
        View::Orbit(OrbitCell::Set) => {
            let _ = label.write_str(OrbitCell::Set.label());
            text = cell_text(OrbitCell::Set, f, ctx.ring);
            Gauge::Switch { on: values(slot) }
        }
        View::Orbit(OrbitCell::Scene) => {
            let when = match f.song.queued() {
                Some(_) => "ON THE LOOP",
                None => "PLAYING",
            };
            let _ = write!(label, "SCENE · {when}");
            text = cell_text(OrbitCell::Scene, f, ctx.ring);
            Gauge::None
        }
        View::Orbit(OrbitCell::Save) => {
            let _ = label.write_str("EDIT SAVES");
            text = cell_text(OrbitCell::Save, f, ctx.ring);
            Gauge::None
        }
        View::Orbit(OrbitCell::Switch) => {
            let _ = label.write_str("A SWITCH LANDS ON THE");
            text = cell_text(OrbitCell::Switch, f, ctx.ring);
            Gauge::None
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
            let _ = write!(
                text,
                "{}/{}",
                libm::roundf(hits) as u32,
                libm::roundf(steps) as u32
            );
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
        VizType::OrbitRhythm | VizType::OrbitVoice => ring_key(&mut h, f, r),
        VizType::OrbitNotes => {
            let sr = &f.scene.rings[r.get() as usize];
            sr.ring.hash(&mut h);
            f.ui.chip_of(sr).hash(&mut h);
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
                ring_key(&mut h, f, ring);
                f.ui.anim.flash(ring).hash(&mut h);
            }
            f.chords().hash(&mut h);
            f.tempo().hash(&mut h);
            st.running.then_some(st.seg).hash(&mut h);
            r.hash(&mut h);
            h.write_u16(crate::ui::region::quantize(f.ui.anim.sweep() * 64.0));
        }
        VizType::OrbitScenes => {
            let song = f.song;
            (song.saved(), song.current(), song.queued()).hash(&mut h);
            f.ui.save_slot(song).hash(&mut h);
            // The queued fill, to the pixel.
            loop_done(f).map(fill_w).hash(&mut h);
        }
        VizType::OrbitCircle => {
            f.chords().hash(&mut h);
            h.write_u16(crate::ui::region::quantize(crate::ui::animation::wrap(
                cursor, 12.0,
            )));
        }
        _ => return 0,
    }
    h.get()
}

/// What RHYTHM, VOICE and ORB draw of ring `r`: its hits as heard and
/// their accents, the step under the clock, and the knobs they read; not
/// its notes, which none of them shows.
fn ring_key(h: &mut Fnv, f: &OrbitFrame, r: RingIdx) {
    let ring = &f.scene.rings[r.get() as usize].ring;
    let mask = f.hits(r);
    (mask, ring.accents_on(mask), f.now(r)).hash(h);
    (ring.mute, ring.chance, ring.gate, ring.velocity).hash(h);
    (ring.accent(), ring.octave, ring.range).hash(h);
}
