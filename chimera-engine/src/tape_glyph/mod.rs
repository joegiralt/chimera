//! TAPE's machine glyphs (#641, ADR 0173): one 64 × 48 picture a machine,
//! still on MODEL (its identity), live on every other machine
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
use crate::dsp::tape_engine::{Direction, MachineIndex, TapeMachine, Tron, Way};

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

// `ALL` is in discriminant order: a palette indexes by `ink as usize`.
const _: () = {
    let mut i = 0;
    while i < Ink::ALL.len() {
        assert!(Ink::ALL[i] as usize == i);
        i += 1;
    }
};

/// The box's pixels, row by row: 3 KB, the UI's to hold for a paint.
pub struct Canvas(pub [Ink; (W * H) as usize]);

impl Canvas {
    pub const fn new() -> Self {
        Canvas([Ink::Bg; (W * H) as usize])
    }

    /// The ink at (`x`, `y`); `None` outside the box.
    pub fn at(&self, x: i32, y: i32) -> Option<Ink> {
        let inside = (0..W).contains(&x) && (0..H).contains(&y);
        inside.then(|| self.0[(y * W + x) as usize])
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

/// LOOP's loop in the window: POSITION, LENGTH, DRIFT and XFADE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loop {
    pub position: Q6,
    pub length: Q6,
    /// Bipolar.
    pub walk: Q6,
    pub xfade: Q3,
}

/// PING's loop in the window: POSITION, LENGTH and DRIFT. It has no
/// crossfade.
///
/// ```compile_fail,E0560
/// use chimera_engine::tape_glyph::{Bounce, Q3, Q6};
/// let _ = Bounce { position: Q6::of(0.0), length: Q6::of(0.0), walk: Q6::of(0.0), xfade: Q3::of(1.0) };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounce {
    pub position: Q6,
    pub length: Q6,
    /// Bipolar.
    pub walk: Q6,
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

/// WALK's footsteps (ADR 0203): DIRECTION lays them (a line forward or
/// back, a loop, there and back), POSITION and LENGTH the stretch they
/// cover, DRIFT scatters them, FADE fades the last ones out, and TRON
/// marks where its tape ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    /// DIRECTION's value, 0 to 3.
    pub dir: u8,
    pub position: Q6,
    pub length: Q6,
    pub drift: Q4,
    pub fade: Q3,
    pub tron: bool,
}

impl Steps {
    pub const fn direction(self) -> Direction {
        match self.dir {
            0 => Direction::Forward,
            1 => Direction::Reverse,
            2 => Direction::Loop,
            _ => Direction::Ping,
        }
    }
}

/// How WALK's footsteps are drawn: the owner picks one (ADR 0203's
/// listening page); `render` draws `StepStyle::DEFAULT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStyle {
    /// A sole and a toe, left and right.
    Prints,
    /// Boot treads: a chevron a step.
    Treads,
    /// A dotted trail, a stride's mark a step.
    Trail,
}

impl StepStyle {
    pub const ALL: [StepStyle; 3] = [StepStyle::Prints, StepStyle::Treads, StepStyle::Trail];
    pub const DEFAULT: StepStyle = StepStyle::Prints;
}

/// A machine and its inputs. ONE's match, LOOP's wheel, PING's court and
/// TRON's blind are kept (never throw work away); WALK draws `Walk`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    One(Run),
    Loop(Loop),
    Ping(Bounce),
    Tron(Blind),
    Stretch(Bellows),
    Wave(Film),
    Scrub(Sponge),
    Walk(Steps),
}

/// The glyph a machine shows: the deck's path, so WALK lays its
/// footsteps by DIRECTION and TRON (ADR 0203).
pub type Kind = Way;

impl Way {
    /// Its still pose: fixed inputs that show what the machine is.
    pub fn still(self) -> Machine {
        let steps = |dir: Direction, tron: bool| {
            Machine::Walk(Steps {
                dir: dir as u8,
                position: Q6::of(0.1),
                length: Q6::of(0.8),
                drift: Q4::of(0.25),
                fade: Q3::of(0.0),
                tron,
            })
        };
        match self {
            Kind::One => steps(Direction::Forward, false),
            Kind::Loop => steps(Direction::Loop, false),
            Kind::Ping => steps(Direction::Ping, false),
            Kind::Tron => steps(Direction::Forward, true),
            Kind::Stretch | Kind::Wave | Kind::Scrub => self.kept(),
        }
    }

    /// The other machines' still poses, and the kept pictures'.
    pub fn kept(self) -> Machine {
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
            Kind::Ping => Machine::Ping(Bounce {
                position: q6(0.5),
                length: q6(0.7),
                walk: walk(0.3),
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

/// Which pose a glyph takes: `Still` on MODEL's cell, which shows the
/// machine, not its knobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Still,
    Live,
}

/// What a glyph is drawn from: the machine and the Part's set values,
/// normalized (eased, as the cells show them).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct GlyphInputs {
    pub machine: Way,
    pub start: f32,
    pub end: f32,
    pub position: f32,
    pub motion: f32,
    pub size: f32,
    pub drag: f32,
    pub tape: f32,
    pub age: f32,
    pub xfade: f32,
    /// WALK's DIRECTION (REVERSE draws ONE's run backward) and DRIFT.
    pub direction: Direction,
    pub drift: f32,
    pub tron: bool,
    pub pose: Pose,
}

impl GlyphInputs {
    /// From `TAPE_GLYPH_PARAMS`' set values, normalized; MODEL's to its
    /// nearest machine.
    pub fn from_set(set: [f32; 13], pose: Pose) -> Self {
        let [
            model,
            start,
            end,
            position,
            motion,
            size,
            drag,
            tape,
            age,
            xfade,
            direction,
            tron,
            drift,
        ] = set;
        let index_of = crate::dsp::rev::sync::index_of;
        let m =
            TapeMachine::from_index(MachineIndex(index_of(model, TapeMachine::ALL.len()) as u8));
        let direction = Direction::ALL[index_of(direction, Direction::ALL.len())];
        let on = tron > 0.5;
        let tron = if on { Tron::On } else { Tron::Off };
        Self {
            machine: Way::of(m, direction, tron),
            start,
            end,
            position,
            motion,
            size,
            drag,
            tape,
            age,
            xfade,
            direction,
            drift,
            tron: on,
            pose,
        }
    }
}

// `from_set`'s order.
const _: () = {
    use crate::dsp::tape_engine::TapeEngineParams as T;
    let order = [
        T::MODEL,
        T::START,
        T::END,
        T::POSITION,
        T::MOTION,
        T::SIZE,
        T::DRAG,
        T::TAPE,
        T::AGE,
        T::XFADE,
        T::MODE,
        T::TRON,
        T::DRIFT,
    ];
    let mut i = 0;
    while i < order.len() {
        assert!(crate::glyph::TAPE_GLYPH_PARAMS[i].param.0 == order[i].0);
        i += 1;
    }
};

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

    /// `i`'s gauge (`draw`'s `inputs`, bank 2's with `bank2`). Closed,
    /// and without `tape`, ONE's still pose: no glyph is drawn then anyway
    /// (`paint`).
    pub fn of(i: &GlyphInputs) -> Self {
        #[cfg(feature = "bank2")]
        {
            crate::bank2::DOOR_CELL
                .with(|d| (d.glyph.inputs)(i))
                .unwrap_or(Self::still(Kind::One))
        }
        #[cfg(all(feature = "tape", not(feature = "bank2")))]
        {
            draw::inputs(i)
        }
        #[cfg(not(feature = "tape"))]
        {
            let _ = i;
            Self::still(Kind::One)
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
            Machine::Walk(s) => match (s.direction(), s.tron) {
                (Direction::Forward | Direction::Reverse, false) => Kind::One,
                (Direction::Forward | Direction::Reverse, true) => Kind::Tron,
                (Direction::Loop, _) => Kind::Loop,
                (Direction::Ping, _) => Kind::Ping,
            },
        }
    }

    /// Every step the glyph draws from, packed: the machine's (22 bits at
    /// most), its kind (3) and its wear (7). Equal keys draw alike.
    pub fn key(&self) -> u32 {
        let s = |v: u8, at: u32| (v as u32) << at;
        let m = match self.machine {
            Machine::One(r) => s(r.start.0, 0) | s(r.end.0, 6),
            Machine::Loop(l) => {
                s(l.position.0, 0) | s(l.length.0, 6) | s(l.walk.0, 12) | s(l.xfade.0, 18)
            }
            Machine::Ping(b) => s(b.position.0, 0) | s(b.length.0, 6) | s(b.walk.0, 12),
            Machine::Tron(b) => s(b.length.0, 0) | s(b.fade.0, 6),
            Machine::Stretch(b) => s(b.speed.0, 0) | s(b.d_time.0, 6),
            Machine::Wave(f) => s(f.frame.0, 0) | s(f.range.0, 6) | s(f.scan.0, 12),
            Machine::Walk(w) => {
                s(w.dir, 0)
                    | s(w.position.0, 2)
                    | s(w.length.0, 8)
                    | s(w.drift.0, 14)
                    | s(w.fade.0, 18)
                    | s(u8::from(w.tron), 21)
            }
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

/// The glyph's entry in the door's table (ADR 0173).
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct GlyphDoor {
    render: fn(&TapeGauge, &mut Canvas),
    inputs: fn(&GlyphInputs) -> TapeGauge,
}

#[cfg(feature = "bank2")]
impl GlyphDoor {
    pub const DOOR: GlyphDoor = GlyphDoor {
        render: draw::render,
        inputs: draw::inputs,
    };
}
