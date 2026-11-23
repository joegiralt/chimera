// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The LFO page's pictures (#649, ADR 0177): a glyph a control in the
//! focus band's 64 × 48 box, and the scope's one cycle of the LFO itself.
//!
//! Functional core: `Wave` is an LFO's quantised set values, each a step
//! that moves under a pixel, and `draw` holds pure functions of it: the
//! glyph into a `Canvas` of `Ink`s, the scope's curve into its columns.
//! The UI paints them in its palette. The focus band keys on
//! `LfoGauge::key`, the scope on `Wave::curve_key`: quantised inputs and
//! no frame term, so neither redraws at idle and a dirty redraw is a full
//! one. Only the scope's playhead moves, while a voice sounds.
//!
//! With `bank2`, `draw` is linked into bank 2 (`bank2.x`) and bank 1
//! reaches it only through the door (`paint`, `curve`); a host draws in
//! place; a firmware without bank 2 (the bench sets) draws none.

use crate::dsp::clampf;
use crate::dsp::lfo::{LFO_SPECS, LfoShape};
use crate::dsp::modulator::LfoForm;
use crate::dsp::modulator::law::LFO_RATE;

pub mod draw;

/// The glyph box, px: the focus band's gauge box.
pub const W: i32 = 64;
pub const H: i32 = 48;

/// The scope's columns: the viz band, `VIZ_LEFT` to `VIZ_RIGHT`.
pub const COLS: usize = 217;
/// The scope's half height, px: its curve is `mid − v·SCOPE_AMP`.
pub const SCOPE_AMP: i32 = 24;

/// A theme token a picture draws in; the UI's palette colours each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ink {
    /// The ground.
    Bg = 0,
    /// Marks: keys, the start dot.
    Ink,
    /// Ghosts and secondary marks.
    Mid,
    /// Rules and rails.
    Faint,
    /// What the knob moves.
    Accent,
    /// The fill under it.
    Soft,
}

impl Ink {
    pub const ALL: [Ink; 6] = [
        Ink::Bg,
        Ink::Ink,
        Ink::Mid,
        Ink::Faint,
        Ink::Accent,
        Ink::Soft,
    ];
}

/// The glyph box's pixels, row by row: 3 KB, the UI's to hold for a paint.
pub struct Canvas(pub [Ink; (W * H) as usize]);

impl Canvas {
    pub const fn new() -> Self {
        Canvas([Ink::Bg; (W * H) as usize])
    }

    pub fn at(&self, x: i32, y: i32) -> Ink {
        self.0[(y * W + x) as usize]
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

/// A set value, normalized, in 254 steps: under a pixel across the
/// scope's 217 columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Q(u8);

impl Q {
    pub const MAX: u8 = 254;

    /// `v` (0..1) to its nearest step.
    pub fn of(v: f32) -> Self {
        Self(libm::roundf(clampf(v, 0.0, 1.0) * Self::MAX as f32) as u8)
    }

    pub const fn step(self) -> u8 {
        self.0
    }

    /// 0..1.
    pub fn unit(self) -> f32 {
        self.0 as f32 / Self::MAX as f32
    }
}

/// An LFO's set values, as its pictures read them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Classic {
        /// RATE's knob, linear over 0.01–20 Hz.
        rate: Q,
        shape: LfoShape,
        /// SYNC RETRIG: each note restarts the wave at PHASE.
        retrig: bool,
        phase: Q,
        depth: Q,
    },
    Func {
        /// RATE's knob, `law::LFO_RATE`.
        rate: Q,
        form: LfoForm,
        /// PHASE, or DELTA under LFV.
        fall: Q,
        /// TILT, or SLEW under LFV.
        tilt: Q,
    },
}

impl Wave {
    /// Cycles a second.
    pub fn hz(&self) -> f32 {
        match *self {
            Wave::Classic { rate, .. } => {
                let s = &LFO_SPECS[0];
                s.min + rate.unit() * (s.max - s.min)
            }
            Wave::Func { rate, .. } => LFO_RATE.at(rate.unit()),
        }
    }

    /// S&H and LFV: a new level each cycle, so the scope draws a fixed
    /// eight-step picture of one.
    pub fn random(&self) -> bool {
        matches!(
            *self,
            Wave::Classic {
                shape: LfoShape::Random,
                ..
            } | Wave::Func {
                form: LfoForm::Lfv,
                ..
            }
        )
    }

    /// Every step a picture draws from, packed: 29 bits.
    pub fn key(&self) -> u32 {
        let s = |q: Q, at: u32| (q.0 as u32) << at;
        match *self {
            Wave::Classic {
                rate,
                shape,
                retrig,
                phase,
                depth,
            } => {
                s(rate, 0)
                    | s(phase, 8)
                    | s(depth, 16)
                    | (shape as u32) << 24
                    | (retrig as u32) << 27
            }
            Wave::Func {
                rate,
                form,
                fall,
                tilt,
            } => s(rate, 0) | s(fall, 8) | s(tilt, 16) | (form as u32) << 24 | 1 << 28,
        }
    }

    /// What the scope's curve draws from: all but RATE, which it never
    /// reads (one cycle, whatever its length).
    pub fn curve_key(&self) -> u32 {
        self.key() & !0xff
    }

    /// Where the scope's playhead is, 0..1 across its picture, for a voice
    /// at `phase` (0..1) after `wraps` whole cycles: the clock under
    /// CLASSIC FREE, the cycle from PHASE under RETRIG, FUNC's φ; S&H and
    /// LFV walk the eight steps.
    pub fn head(&self, phase: f32, wraps: u8) -> f32 {
        let frac = |x: f32| x - libm::floorf(x);
        if self.random() {
            return ((wraps % 8) as f32 + frac(phase)) / 8.0;
        }
        match *self {
            Wave::Classic {
                retrig: true,
                phase: p,
                ..
            } => frac(phase - p.unit()),
            _ => frac(phase),
        }
    }
}

/// Which control's glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// RATE, both TYPEs: the wave over two seconds, a one-second ruler.
    Rate,
    /// SHAPE: the shape itself.
    Shape,
    /// SYNC and FORM: two keys under the wave; what each does to it.
    Lane,
    /// PHASE, both TYPEs: the wave slides under a fixed start.
    Phase,
    /// DEPTH: the scaled wave over its full-scale ghost.
    Depth,
    /// TILT: one cycle, the rise lit, a rise:fall bar.
    Tilt,
    /// DELTA: the walk, its targets dotted, the ±1 rails.
    Delta,
    /// SLEW: the walk slewed over its straight ghost.
    Slew,
}

impl Glyph {
    pub const ALL: [Glyph; 8] = [
        Glyph::Rate,
        Glyph::Shape,
        Glyph::Lane,
        Glyph::Phase,
        Glyph::Depth,
        Glyph::Tilt,
        Glyph::Delta,
        Glyph::Slew,
    ];
}

/// A glyph and the LFO it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LfoGauge {
    pub glyph: Glyph,
    pub wave: Wave,
}

impl LfoGauge {
    /// The wave's steps and the glyph: equal keys draw alike. RATE alone
    /// reads the rate; the others leave it out, so turning RATE never
    /// redraws them.
    pub fn key(&self) -> u32 {
        let wave = match self.glyph {
            Glyph::Rate => self.wave.key(),
            _ => self.wave.curve_key(),
        };
        wave | (self.glyph as u32) << 29
    }

    /// The words drawn over the canvas, in the label face: RATE's ruler.
    pub fn ruler(&self) -> bool {
        self.glyph == Glyph::Rate
    }
}

/// `g` into `out`: true if drawn. With `bank2` it is bank 2's, through
/// the door; closed, nothing is drawn and `out` keeps its ground.
pub fn paint(g: &LfoGauge, out: &mut Canvas) -> bool {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL
            .with(|d| (d.lfo.render)(g, out))
            .is_some()
    }
    // A host (the desktop, the tests) draws in place.
    #[cfg(all(not(feature = "bank2"), not(target_os = "none")))]
    {
        draw::render(g, out);
        true
    }
    // A firmware without bank 2 (the bench sets) links no raster.
    #[cfg(all(not(feature = "bank2"), target_os = "none"))]
    {
        let _ = (g, out);
        false
    }
}

/// The scope's curve of `w` into `out`: each column's offset from the
/// band's middle, up positive. True if drawn; closed, `out` is flat.
pub fn curve(w: &Wave, out: &mut [i8; COLS]) -> bool {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL
            .with(|d| (d.lfo.curve)(w, out))
            .is_some()
    }
    // A host (the desktop, the tests) draws in place.
    #[cfg(all(not(feature = "bank2"), not(target_os = "none")))]
    {
        draw::curve(w, out);
        true
    }
    // A firmware without bank 2 (the bench sets) links no raster.
    #[cfg(all(not(feature = "bank2"), target_os = "none"))]
    {
        let _ = (w, out);
        false
    }
}

/// The pictures' entries in the door's table (ADR 0177).
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct LfoDoor {
    render: fn(&LfoGauge, &mut Canvas),
    curve: fn(&Wave, &mut [i8; COLS]),
}

#[cfg(feature = "bank2")]
impl LfoDoor {
    pub const DOOR: LfoDoor = LfoDoor {
        render: draw::render,
        curve: draw::curve,
    };
}
