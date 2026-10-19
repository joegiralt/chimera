//! TAPE's machine glyphs (#641, ADR 0148): one 64 × 48 picture a machine,
//! still on MODEL and MODE (its identity), live on every other machine
//! cell (drawn from the Part's set values).
//!
//! Functional core: `TapeGauge` is the quantised inputs, each a step that
//! moves about a pixel, and `draw::render` a pure function of it into a
//! `Canvas` of `Ink`s, which the UI paints in its palette
//! (`chimera_core::ui::tape_glyph`). The focus band keys on
//! `TapeGauge::key`, which holds those same steps and no frame term, so a
//! glyph redraws only while a knob or its ease moves it, never at idle,
//! and a dirty redraw is a full one.
//!
//! With `bank2`, `draw` is linked into bank 2 (`bank2.x`) and bank 1
//! reaches it only through the door (`paint`); the inputs, the key and
//! the canvas stay in bank 1.

use crate::dsp::algo::math::clampf;
use crate::dsp::tape_engine::{PlayMode, TapeModel};

/// With TAPE's DSP only: a build without it (the bench's) draws no glyph.
#[cfg(feature = "tape")]
pub mod draw;

/// The box, px: the focus band's 64 × 48 gauge box.
pub const W: i32 = 64;
pub const H: i32 = 48;

/// A theme token a glyph draws in; the UI's palette colours each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ink {
    /// The ground.
    Bg = 0,
    /// Small highlights.
    Ink,
    /// Structure.
    Ink2,
    /// Secondary marks.
    Mid,
    /// Tracks and rules.
    Faint,
    /// The one thing the knob moves.
    Accent,
    /// Fills under it.
    Soft,
}

impl Ink {
    pub const ALL: [Ink; 7] = [
        Ink::Bg,
        Ink::Ink,
        Ink::Ink2,
        Ink::Mid,
        Ink::Faint,
        Ink::Accent,
        Ink::Soft,
    ];

    /// A dropout's ink for `self`: a step toward the ground.
    pub const fn dim(self) -> Ink {
        match self {
            Ink::Ink | Ink::Ink2 | Ink::Accent => Ink::Mid,
            Ink::Mid => Ink::Faint,
            Ink::Soft => Ink::Bg,
            Ink::Bg | Ink::Faint => self,
        }
    }
}

/// The box's pixels, row by row: 3 KB, the UI's to hold for a paint.
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

/// A set value, normalized, in `2^B − 2` steps: an even count, so a
/// bipolar knob's centre is a step of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Q<const B: u8>(u8);

impl<const B: u8> Q<B> {
    pub const MAX: u8 = (1 << B) - 2;

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

    /// −1..1, the centre 0.
    pub fn bi(self) -> f32 {
        2.0 * self.unit() - 1.0
    }
}

/// Six bits: 62 steps, about a pixel across the 60 px window.
pub type Q6 = Q<6>;
/// Five bits: 30 steps, for marks of 30 px or less.
pub type Q5 = Q<5>;
/// Four bits: 14 steps, for marks of 14 px or less.
pub type Q4 = Q<4>;
/// Three bits: 6 steps.
pub type Q3 = Q<3>;

/// ONE's run: the window START to END, as it plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub start: Q6,
    pub end: Q6,
}

/// LOOP's and PING's loop in the window: POSITION, LENGTH, WALK and
/// (LOOP's only: PING has none) XFADE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loop {
    pub position: Q6,
    pub length: Q6,
    /// Bipolar.
    pub walk: Q6,
    pub xfade: Q3,
}

/// TRON's blind: LENGTH pulls it down, FADE snaps it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blind {
    pub length: Q6,
    pub fade: Q5,
}

/// STRETCH's accordion: SPEED opens or squeezes it, D-TIME folds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bellows {
    /// Bipolar: MOTION.
    pub speed: Q6,
    pub d_time: Q5,
}

/// TRANSWAVE's film: FRAME in the gate, RANGE the gate's width, SCAN's
/// rate under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Film {
    pub frame: Q6,
    pub range: Q6,
    /// Bipolar.
    pub scan: Q5,
}

/// SCRUB's sponge: FINGER moves it, SPEED foams it, LENGTH is the loop
/// outlined under it, XFADE its dashed end, DRAG its wet trail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sponge {
    pub finger: Q6,
    /// Bipolar.
    pub speed: Q4,
    pub length: Q5,
    pub drag: Q4,
    pub xfade: Q3,
}

/// A machine and its inputs: PLAY's three MODEs each their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    One(Run),
    Loop(Loop),
    Ping(Loop),
    Tron(Blind),
    Stretch(Bellows),
    Wave(Film),
    Scrub(Sponge),
}

/// The glyph a machine shows, by MODEL and PLAY's MODE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    One,
    Loop,
    Ping,
    Tron,
    Stretch,
    Wave,
    Scrub,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::One,
        Kind::Loop,
        Kind::Ping,
        Kind::Tron,
        Kind::Stretch,
        Kind::Wave,
        Kind::Scrub,
    ];

    pub const fn of(model: TapeModel, mode: PlayMode) -> Self {
        match (model, mode) {
            (TapeModel::Play, PlayMode::One) => Kind::One,
            (TapeModel::Play, PlayMode::Loop) => Kind::Loop,
            (TapeModel::Play, PlayMode::Ping) => Kind::Ping,
            (TapeModel::Tron, _) => Kind::Tron,
            (TapeModel::Stretch, _) => Kind::Stretch,
            (TapeModel::Transwave, _) => Kind::Wave,
            (TapeModel::Scrub, _) => Kind::Scrub,
        }
    }

    /// Its still pose: fixed inputs that show what the machine is.
    pub fn still(self) -> Machine {
        let q6 = Q6::of;
        let q5 = Q5::of;
        let walk = |v: f32| q6(0.5 + 0.5 * v);
        match self {
            Kind::One => Machine::One(Run {
                start: q6(0.25),
                end: q6(0.6),
            }),
            Kind::Loop => Machine::Loop(Loop {
                position: q6(0.3),
                length: q6(0.5),
                walk: walk(0.45),
                xfade: Q3::of(0.5),
            }),
            Kind::Ping => Machine::Ping(Loop {
                position: q6(0.5),
                length: q6(0.7),
                walk: walk(0.3),
                xfade: Q3::of(0.0),
            }),
            Kind::Tron => Machine::Tron(Blind {
                length: q6(0.8),
                fade: q5(0.25),
            }),
            Kind::Stretch => Machine::Stretch(Bellows {
                speed: walk(0.25),
                d_time: q5(0.45),
            }),
            Kind::Wave => Machine::Wave(Film {
                frame: q6(0.45),
                range: q6(0.12),
                scan: Q5::of(0.5 + 0.5 * 0.4),
            }),
            Kind::Scrub => Machine::Scrub(Sponge {
                finger: q6(0.45),
                speed: Q4::of(0.5 + 0.5 * 0.6),
                length: q5(0.4),
                drag: Q4::of(0.5),
                xfade: Q3::of(0.5),
            }),
        }
    }

    const fn tag(self) -> u32 {
        self as u32
    }
}

/// TAPE (wow) and AGE, which every glyph wears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wear {
    pub tape: Q<3>,
    pub age: Q<4>,
}

impl Wear {
    pub const NONE: Wear = Wear {
        tape: Q(0),
        age: Q(0),
    };
}

/// What the glyph draws: a machine's inputs and its wear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapeGauge {
    pub machine: Machine,
    pub wear: Wear,
}

impl TapeGauge {
    /// `kind`'s identity: its still pose, unworn.
    pub fn still(kind: Kind) -> Self {
        Self {
            machine: kind.still(),
            wear: Wear::NONE,
        }
    }

    /// From `TAPE_GLYPH_PARAMS`' set values, normalized: MODEL, MODE,
    /// START, END, POSITION, MOTION, SIZE, DRAG, TAPE, AGE, XFADE. `still`
    /// for MODEL's and MODE's cells, which show the machine, not its
    /// knobs. A machine reads only what it plays (`reads_in`), so a cell it
    /// ignores leaves the glyph as it was.
    pub fn from_set(set: [f32; 11], still: bool) -> Self {
        let [
            model,
            mode,
            start,
            end,
            pos,
            motion,
            size,
            drag,
            tape,
            age,
            xfade,
        ] = set;
        let pick = |v: f32, n: u8| libm::roundf(clampf(v, 0.0, 1.0) * n as f32) as u8;
        let model = TapeModel::from_index(pick(model, TapeModel::ALL.len() as u8 - 1));
        let mode = match pick(mode, 2) {
            0 => PlayMode::One,
            1 => PlayMode::Loop,
            _ => PlayMode::Ping,
        };
        let kind = Kind::of(model, mode);
        if still {
            return Self::still(kind);
        }
        let (q6, q5) = (Q6::of, Q5::of);
        let machine = match kind {
            Kind::One => Machine::One(Run {
                start: q6(start),
                end: q6(end),
            }),
            Kind::Loop | Kind::Ping => {
                let l = Loop {
                    position: q6(pos),
                    length: q6(size),
                    walk: q6(motion),
                    // PING has no crossfade.
                    xfade: Q3::of(if kind == Kind::Loop { xfade } else { 0.0 }),
                };
                if kind == Kind::Loop {
                    Machine::Loop(l)
                } else {
                    Machine::Ping(l)
                }
            }
            Kind::Tron => Machine::Tron(Blind {
                length: q6(size),
                fade: q5(drag),
            }),
            Kind::Stretch => Machine::Stretch(Bellows {
                speed: q6(motion),
                d_time: q5(size),
            }),
            Kind::Wave => Machine::Wave(Film {
                frame: q6(pos),
                range: q6(size),
                scan: q5(motion),
            }),
            Kind::Scrub => Machine::Scrub(Sponge {
                finger: q6(pos),
                speed: Q4::of(motion),
                length: q5(size),
                drag: Q4::of(drag),
                xfade: Q3::of(xfade),
            }),
        };
        Self {
            machine,
            wear: Wear {
                tape: Q::of(tape),
                age: Q::of(age),
            },
        }
    }

    pub const fn kind(&self) -> Kind {
        match self.machine {
            Machine::One(_) => Kind::One,
            Machine::Loop(_) => Kind::Loop,
            Machine::Ping(_) => Kind::Ping,
            Machine::Tron(_) => Kind::Tron,
            Machine::Stretch(_) => Kind::Stretch,
            Machine::Wave(_) => Kind::Wave,
            Machine::Scrub(_) => Kind::Scrub,
        }
    }

    /// Every step the glyph draws from, packed: the machine's (22 bits at
    /// most), its kind (3) and its wear (7). Equal keys draw alike.
    pub fn key(&self) -> u32 {
        let s = |v: u8, at: u32| (v as u32) << at;
        let m = match self.machine {
            Machine::One(r) => s(r.start.0, 0) | s(r.end.0, 6),
            Machine::Loop(l) | Machine::Ping(l) => {
                s(l.position.0, 0) | s(l.length.0, 6) | s(l.walk.0, 12) | s(l.xfade.0, 18)
            }
            Machine::Tron(b) => s(b.length.0, 0) | s(b.fade.0, 6),
            Machine::Stretch(b) => s(b.speed.0, 0) | s(b.d_time.0, 6),
            Machine::Wave(f) => s(f.frame.0, 0) | s(f.range.0, 6) | s(f.scan.0, 12),
            Machine::Scrub(p) => {
                s(p.finger.0, 0)
                    | s(p.speed.0, 6)
                    | s(p.length.0, 10)
                    | s(p.drag.0, 15)
                    | s(p.xfade.0, 19)
            }
        };
        m | self.kind().tag() << 22 | s(self.wear.tape.0, 25) | s(self.wear.age.0, 28)
    }
}

/// `g` into `out`: true if drawn. With `bank2` it is bank 2's, through
/// the door; closed, nothing is drawn and `out` keeps its ground. Without
/// `tape` (the bench) nothing is drawn.
pub fn paint(g: &TapeGauge, out: &mut Canvas) -> bool {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL
            .with(|d| (d.glyph.render)(g, out))
            .is_some()
    }
    #[cfg(all(feature = "tape", not(feature = "bank2")))]
    {
        draw::render(g, out);
        true
    }
    #[cfg(not(feature = "tape"))]
    {
        let _ = (g, out);
        false
    }
}

/// The glyph's entry in the door's table (ADR 0148).
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct GlyphDoor {
    pub render: fn(&TapeGauge, &mut Canvas),
}

#[cfg(feature = "bank2")]
impl GlyphDoor {
    pub const DOOR: GlyphDoor = GlyphDoor {
        render: draw::render,
    };
}
