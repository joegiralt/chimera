//! The oscilloscope screensaver: after SCREENSAVER's idle time the screen
//! shows the master scope large, with the last note in, the pitch it
//! sounds at, the pitch measured off the scope, and the keys held with
//! their chord. Any button or encoder wakes it and that touch is
//! swallowed. MIDI does not wake it.
//!
//! `Idle` is the pure timer; naming and measuring are pure functions;
//! `draw` takes a `Readout`, so goldens need no audio.

use core::fmt::Write;

use chimera_theory::{Chord, Note};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::played::{self, NoteIn, NoteOut};
use crate::scope::{self, SCOPE_LEN};
use crate::ui::fmt::FmtBuf;
use crate::ui::{draw, theme};

/// The screensaver redraws every this many UI frames (20 Hz): ~7 Hz.
pub const REDRAW_FRAMES: u8 = 3;

/// What a touch did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    Pass,
    /// The touch woke the screen or continues the waking press: not for
    /// the UI.
    Swallow,
}

/// Idle time, asleep or not, and the wake press still down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Idle {
    idle_ms: u32,
    asleep: bool,
    swallowing: bool,
}

impl Idle {
    pub const fn new() -> Self {
        Self {
            idle_ms: 0,
            asleep: false,
            swallowing: false,
        }
    }

    pub const fn asleep(&self) -> bool {
        self.asleep
    }

    /// `dt_ms` of time passes; `timeout` is SCREENSAVER's (None: OFF).
    pub fn tick(&mut self, dt_ms: u32, timeout: Option<u32>) {
        let Some(t) = timeout else {
            *self = Self::new();
            return;
        };
        self.idle_ms = self.idle_ms.saturating_add(dt_ms);
        self.asleep |= self.idle_ms >= t;
    }

    /// Time that doesn't count: the clock restarts, and an asleep screen
    /// stays so.
    pub fn hold(&mut self) {
        self.idle_ms = 0;
    }

    /// One frame of input: `turned` an encoder moved, `down` a button is
    /// down. The touch that wakes the screen is swallowed, and a waking
    /// button's press, hold and release with it.
    pub fn touch(&mut self, turned: bool, down: bool) -> Touch {
        let active = turned || down;
        if active {
            self.idle_ms = 0;
        }
        if self.asleep && active {
            self.asleep = false;
            self.swallowing = down;
            return Touch::Swallow;
        }
        if self.swallowing {
            self.swallowing = down;
            // The release frame is still the wake press's.
            return Touch::Swallow;
        }
        Touch::Pass
    }
}

impl Default for Idle {
    fn default() -> Self {
        Self::new()
    }
}

// --- Naming ---

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// `total` cents above MIDI note 0 as the nearest note and the cents off
/// it, −50..=49: exactly between two notes is the upper one at −50.
pub fn nearest(total: i32) -> (i32, i32) {
    let n = (total + 50).div_euclid(100);
    (n, total - n * 100)
}

/// "C4", "C#-1": middle C (60) is C4. Notes past MIDI's 0..=127 name
/// on, down to C-2 and up.
pub fn note_name(b: &mut FmtBuf, n: i32) {
    let _ = write!(
        b,
        "{}{}",
        NAMES[n.rem_euclid(12) as usize],
        n.div_euclid(12) - 1
    );
}

/// "+12ct", "-3ct", "+0ct". ASCII: the fonts have no cent sign.
pub fn cents(b: &mut FmtBuf, c: i32) {
    let _ = write!(b, "{c:+}ct");
}

/// "OUT C#4 +12ct".
pub fn out_line(b: &mut FmtBuf, o: NoteOut) {
    let (n, c) = nearest(o.total_cents());
    let _ = b.write_str("OUT ");
    note_name(b, n);
    let _ = b.write_str(" ");
    cents(b, c);
}

/// "IN  C4  v96  P1", then "  +2" for two more keys held.
pub fn in_line(b: &mut FmtBuf, i: NoteIn, held: usize) {
    let _ = b.write_str("IN  ");
    note_name(b, i.note as i32);
    let _ = write!(b, "  v{}  P{}", i.vel, i.part + 1);
    if held > 1 {
        let _ = write!(b, "  +{}", held - 1);
    }
}

/// "(heard +15ct)" against `reference` (OUT's note) while within a
/// semitone of it; else "(heard D4 -20ct)" against the nearest note.
pub fn heard_line(b: &mut FmtBuf, hz: f32, reference: Option<i32>) {
    let total = libm::roundf(6900.0 + 1200.0 * libm::log2f(hz / 440.0)) as i32;
    let (n, c) = nearest(total);
    let _ = b.write_str("(heard ");
    match reference.map(|r| total - r * 100).filter(|d| d.abs() < 100) {
        Some(d) => cents(b, d),
        None => {
            note_name(b, n);
            let _ = b.write_str(" ");
            cents(b, c);
        }
    }
    let _ = b.write_str(")");
}

const INTERVALS: [&str; 12] = [
    "P8", "m2", "M2", "m3", "M3", "P4", "TT", "P5", "m6", "M6", "m7", "M7",
];

/// The held keys (lowest first) as a name: nothing for none, the note for
/// one, the interval (lowest to highest, folded into an octave) for two,
/// else the chord: tones over the lowest note, which is tried as the root
/// first so root position wins (C E G A is C6, not Am7/C), then the other
/// tones rising; slash chords show the lowest note; "?" when no chord
/// quality fits the tones exactly.
pub fn chord_name(b: &mut FmtBuf, keys: &[u8]) {
    match keys {
        [] => {}
        [k] => note_name(b, *k as i32),
        [lo, .., hi] if keys.len() == 2 => {
            let _ = b.write_str(INTERVALS[((hi - lo) % 12) as usize]);
        }
        _ => {
            let mut notes = [Note::MIN; 16];
            let n = keys.len().min(16);
            for (d, &k) in notes.iter_mut().zip(keys) {
                *d = Note::new(k).unwrap_or(Note::MIN);
            }
            match Chord::from_notes(&notes[..n]) {
                Some(c) if c.label().is_exact() => {
                    let _ = write!(b, "{c}");
                }
                _ => {
                    let _ = b.write_str("?");
                }
            }
        }
    }
}

// --- Measuring ---

/// The scope's window is 5 ms at 48 kHz: the correlation compares 79
/// samples with themselves `TAU_MAX` later, so the lowest pitch is
/// 48000 / 160 = 300 Hz (about D4).
const WIN: usize = 79;
const TAU_MIN: usize = 16;
const TAU_MAX: usize = SCOPE_LEN - WIN - 1;
/// A dip this far below the lag's running mean is a period (YIN: de
/// Cheveigné & Kawahara 2002). Noise sits near 1.
const DIP: f32 = 0.1;
/// Below this mean square (about −46 dBFS RMS) there is nothing to hear.
const MIN_POWER: f32 = 2.5e-5;

/// The pitch of a stable periodic signal in the scope window, Hz; None
/// for silence, noise or anything below ~300 Hz. YIN's cumulative-mean
/// difference with parabolic interpolation: a UI-loop cost of ~12 k
/// multiply-adds, off the audio thread.
pub fn measure(buf: &[f32; SCOPE_LEN]) -> Option<f32> {
    let mean = buf.iter().sum::<f32>() / SCOPE_LEN as f32;
    let power = buf.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / SCOPE_LEN as f32;
    if power < MIN_POWER {
        return None;
    }
    let mut norm = [1.0f32; TAU_MAX + 2];
    let mut sum = 0.0f32;
    for tau in 1..=TAU_MAX + 1 {
        let v: f32 = (0..WIN)
            .map(|i| {
                let e = buf[i] - buf[i + tau];
                e * e
            })
            .sum();
        sum += v;
        if sum > 0.0 {
            norm[tau] = v * tau as f32 / sum;
        }
    }
    // The first dip under DIP, followed down to its floor.
    let mut t = (TAU_MIN..=TAU_MAX).find(|&t| norm[t] < DIP)?;
    while t < TAU_MAX && norm[t + 1] < norm[t] {
        t += 1;
    }
    // The coarse period, sharpened: a rising zero crossing, linearly
    // interpolated, to the same crossing k periods on. (A parabola
    // through the floor reads a saw's cusp up to 20 cents off.)
    let (a, b, c) = (norm[t - 1], norm[t], norm[t + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-9 {
        0.5 * (a - c) / den
    } else {
        0.0
    };
    let coarse = t as f32 + crate::dsp::clampf(off, -1.0, 1.0);
    let period = crossing_period(buf, mean, coarse).unwrap_or(coarse);
    Some(crate::hw::SAMPLE_RATE as f32 / period)
}

/// Sample position of the rising crossing of `mean` after `from`.
fn rising(buf: &[f32; SCOPE_LEN], mean: f32, from: usize) -> Option<(f32, usize)> {
    (from.max(1)..SCOPE_LEN).find_map(|i| {
        let (a, b) = (buf[i - 1] - mean, buf[i] - mean);
        (a <= 0.0 && b > 0.0).then(|| ((i - 1) as f32 + -a / (b - a), i))
    })
}

/// The span between the first rising crossing and the one nearest
/// `k` coarse periods on, over `k`; the widest `k` that finds one.
fn crossing_period(buf: &[f32; SCOPE_LEN], mean: f32, coarse: f32) -> Option<f32> {
    let (c0, _) = rising(buf, mean, 1)?;
    let kmax = ((SCOPE_LEN - 1) as f32 - c0) / coarse;
    (1..=kmax as usize).rev().find_map(|k| {
        let want = c0 + k as f32 * coarse;
        let mut at = (want - coarse / 8.0).max(1.0) as usize;
        while let Some((c, i)) = rising(buf, mean, at) {
            if c > want + coarse / 8.0 {
                break;
            }
            if (c - want).abs() <= coarse / 8.0 {
                return Some((c - c0) / k as f32);
            }
            // Past this crossing: from `i` it is found again, forever.
            at = i + 1;
        }
        None
    })
}

/// OUT below this (D4, about the window's 300 Hz floor) has no reliable
/// "heard": the period is past what the window holds.
const MIN_HEARD_CENTS: i32 = 6200;

/// A measurement worth showing: one note at most is held (a chord has no
/// one pitch), OUT is in the window's reach, and the measure is within a
/// semitone of OUT, so an octave error (a strong 2nd harmonic reads an
/// octave up) is never shown as heard.
pub fn trusted(hz: f32, out: Option<NoteOut>, held: usize) -> bool {
    let Some(o) = out.filter(|o| held <= 1 && o.total_cents() >= MIN_HEARD_CENTS) else {
        return false;
    };
    let total = 6900.0 + 1200.0 * libm::log2f(hz / 440.0);
    (total - o.total_cents() as f32).abs() < 100.0
}

// --- Drawing ---

/// Up to this many held keys are named.
const SHOWN: usize = 6;
const MAX_HELD: usize = 16;
const SCOPE_MID: i32 = 160;
const SCOPE_AMP: i32 = 80;

/// What the readout says: the audio thread's notes (`played`) and the
/// pitch measured off the scope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readout {
    pub input: Option<NoteIn>,
    pub out: Option<NoteOut>,
    /// The first held keys, lowest first, and how many are held in all.
    pub keys: [u8; MAX_HELD],
    pub held: usize,
    pub heard: Option<f32>,
}

impl Readout {
    pub const NONE: Readout = Readout {
        input: None,
        out: None,
        keys: [0; MAX_HELD],
        held: 0,
        heard: None,
    };

    pub fn capture(scope: &[f32; SCOPE_LEN]) -> Self {
        let (keys, held) = played::held::<MAX_HELD>();
        let out = played::last_out();
        Self {
            input: played::last_in(),
            out,
            keys,
            held,
            heard: measure(scope).filter(|&hz| trusted(hz, out, held)),
        }
    }
}

/// The scope scaled to its peak (floored, so a whisper stays small).
fn trace(buf: &[f32; SCOPE_LEN], i: usize, gain: f32) -> i32 {
    SCOPE_MID - libm::roundf(buf[i] * gain) as i32
}

pub fn draw<D>(d: &mut D, scope: &[f32; SCOPE_LEN], r: &Readout)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::fill_rect(d, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    for y in [SCOPE_MID - SCOPE_AMP, SCOPE_MID, SCOPE_MID + SCOPE_AMP] {
        draw::fill_rect(d, 0, y, theme::SCREEN_W, 1, theme::FAINT);
    }
    let gain = SCOPE_AMP as f32 / scope::peak(scope).max(0.1);
    for i in 1..SCOPE_LEN {
        let x = i as i32;
        draw::thick_line(
            d,
            x - 1,
            trace(scope, i - 1, gain),
            x,
            trace(scope, i, gain),
            theme::ACCENT,
        );
    }

    let mut line = FmtBuf::new();
    if let Some(i) = r.input {
        in_line(&mut line, i, r.held);
        text(d, line.as_str(), 24, theme::INK);
        line.clear();
        if let Some(o) = r.out {
            out_line(&mut line, o);
        }
        if let Some(hz) = r.heard {
            let _ = line.write_str(" ");
            heard_line(&mut line, hz, r.out.map(|o| nearest(o.total_cents()).0));
        }
        text(d, line.as_str(), 44, theme::INK2);
        line.clear();
    }
    let keys = &r.keys[..r.held.min(MAX_HELD)];
    for (n, &k) in keys.iter().take(SHOWN).enumerate() {
        if n > 0 {
            let _ = line.write_str(" ");
        }
        note_name(&mut line, k as i32);
    }
    if r.held > SHOWN {
        let _ = write!(line, " +{}", r.held - SHOWN);
    }
    text(d, line.as_str(), 276, theme::INK2);
    line.clear();
    chord_name(&mut line, keys);
    draw::text_center(
        d,
        &theme::FONT_READOUT,
        line.as_str(),
        theme::SCREEN_W / 2,
        308,
        theme::ACCENT,
        0,
    );
}

/// A line in the widest face that fits the content width.
fn text<D>(d: &mut D, s: &str, y: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let font = draw::fit_font(
        &[&theme::FONT_VALUE, &theme::FONT_LABEL],
        s,
        theme::CONTENT_W,
    )
    .unwrap_or(&theme::FONT_LABEL);
    draw::text(d, font, s, theme::MARGIN_X, y, color);
}
