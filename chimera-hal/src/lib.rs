#![no_std]

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::pixelcolor::Rgb565;

pub mod card_bus;
pub mod store;
#[cfg(feature = "testkit")]
pub mod testkit;
pub mod units;

pub const BLOCK_SIZE: usize = 64;
pub const SCREEN_WIDTH: u16 = 240;
pub const SCREEN_HEIGHT: u16 = 320;
pub const SAMPLE_RATE: u32 = 48_000;

/// The PreenFM3's six encoders, one per parameter cell.
pub const NUM_ENCODERS: usize = 6;
pub const NUM_BUTTONS: usize = 12; // 6 param + 6 nav

/// The ticks the `n`-th detent of a burst moves: 1, 2, 4, 5, 10, 10…
pub const fn encoder_accel(n: u8) -> i8 {
    const CURVE: [i8; 5] = [1, 2, 4, 5, 10];
    if (n as usize) < CURVE.len() {
        CURVE[n as usize]
    } else {
        10
    }
}

/// One frame's signed detents on an encoder, within ±127: −128 has no
/// negation, so a turn is never it (ADR 0279).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Detents(i8);

impl Detents {
    /// `n` detents, −128 taken as −127.
    pub const fn new(n: i8) -> Self {
        Detents(if n == i8::MIN { -127 } else { n })
    }

    pub const fn get(self) -> i8 {
        self.0
    }

    /// How many, unsigned.
    pub const fn count(self) -> u8 {
        self.0.unsigned_abs()
    }

    /// A tick each: a fine turn's (ADR 0279).
    pub const fn ticks(self) -> Ticks {
        Ticks(self.0)
    }

    /// Without its first: one fewer, the same way.
    const fn less_one(self) -> Self {
        Detents(self.0 - self.0.signum())
    }
}

/// Where in its burst a detent falls: 0 after a rest (ADR 0279).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BurstPos(u8);

impl BurstPos {
    /// A burst's first detent.
    pub const START: Self = BurstPos(0);

    /// A test's place in a burst; the board starts at `START`.
    #[cfg(any(test, feature = "testkit"))]
    pub const fn new(n: u8) -> Self {
        BurstPos(n)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// `n` detents on, saturating.
    pub const fn after(self, n: u8) -> Self {
        BurstPos(self.0.saturating_add(n))
    }

    /// Where this frame's detents `now` start: from the start after a rest
    /// past `ENCODER_BURST_GAP_MS`, when SEQ went down or up (`seq`'s
    /// edges: a fine turn starts or ends, and the plain turn after it
    /// accelerates from its first detent again), or when `now` turns back
    /// from `last`, the burst's way (a correction never jumps rows).
    pub const fn next(self, rested: bool, seq: Edges, last: Detents, now: Detents) -> Self {
        let back = (last.get() as i16) * (now.get() as i16) < 0;
        if rested || back || seq.pressed_at.is_some() || seq.released_at.is_some() {
            Self::START
        } else {
            self
        }
    }
}

/// Signed ticks a turn moves, within ±127 (`Accel::ticks`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ticks(i8);

impl Ticks {
    /// `n` ticks, −128 taken as −127.
    pub const fn new(n: i8) -> Self {
        Ticks(if n == i8::MIN { -127 } else { n })
    }

    pub const fn get(self) -> i8 {
        self.0
    }
}

/// One encoder's frame: its detents and the burst position of the first
/// of them; no position from a shell that does not accelerate, whose
/// detents are its ticks. The board counts; the UI picks the acceleration
/// (`Accel`, ADR 0279).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    detents: Detents,
    burst: Option<BurstPos>,
}

impl Turn {
    /// `d` detents from `burst`: the board's.
    pub const fn at(d: Detents, burst: BurstPos) -> Self {
        Turn {
            detents: d,
            burst: Some(burst),
        }
    }

    /// `d` from a shell with no acceleration (the desktop, the console's
    /// hand, a test): ticks as they are.
    pub const fn steady(d: i8) -> Self {
        Turn {
            detents: Detents::new(d),
            burst: None,
        }
    }

    pub const fn detents(self) -> Detents {
        self.detents
    }

    pub const fn burst(self) -> Option<BurstPos> {
        self.burst
    }

    /// Without its first detent (the one that wakes the screen): one
    /// fewer, the rest one further into the burst.
    pub const fn consume_first(self) -> Self {
        Turn {
            detents: self.detents.less_one(),
            burst: match self.burst {
                Some(b) => Some(b.after(1)),
                None => None,
            },
        }
    }
}

/// How a turn accelerates (ADR 0279): a parameter's sweep, or a list's,
/// menu's or name's rows, which must not skip past the one wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accel {
    /// `encoder_accel`: 1, 2, 4, 5, 10, then 10 ticks a detent.
    Param,
    /// 1 for four detents, 2 for four, then 3.
    List,
}

impl Accel {
    /// The ticks of the burst's `n`-th detent.
    #[inline(always)]
    const fn per_detent(self, n: u8) -> i8 {
        match self {
            Accel::Param => encoder_accel(n),
            Accel::List => match n {
                0..=3 => 1,
                4..=7 => 2,
                _ => 3,
            },
        }
    }

    /// `turn`'s ticks, saturating at ±127. Inlined: a caller's constant
    /// curve folds, so the board's `Param` carries no `List`.
    #[inline(always)]
    pub const fn ticks(self, turn: Turn) -> Ticks {
        let Some(burst) = turn.burst else {
            return Ticks(turn.detents.get());
        };
        let mut total: i8 = 0;
        let mut k: u8 = 0;
        while k < turn.detents.count() {
            total = total.saturating_add(self.per_detent(burst.after(k).get()));
            k += 1;
        }
        Ticks(if turn.detents.get() < 0 {
            -total
        } else {
            total
        })
    }
}

/// A burst ends after this long with no detent.
pub const ENCODER_BURST_GAP_MS: u32 = 150;

/// The HC165 bits a push read adds: a control board with push encoders
/// (v1.2) chains a fourth register, the 32-bit read's top byte. A, B, C,
/// D, E, F's switches; active low.
pub const PUSH_BITS: [u32; NUM_ENCODERS] = [1 << 31, 1 << 30, 1 << 29, 1 << 27, 1 << 26, 1 << 28];

/// Each encoder's push in a 32-bit read, pressed = true. A top byte of all
/// zeros is a board with no fourth register (its serial input is tied
/// low), not six pushes: none is pressed.
pub const fn push_levels(bits: u32) -> [bool; NUM_ENCODERS] {
    let mut out = [false; NUM_ENCODERS];
    if bits & 0xFF00_0000 == 0 {
        return out;
    }
    let mut i = 0;
    while i < NUM_ENCODERS {
        out[i] = bits & PUSH_BITS[i] == 0;
        i += 1;
    }
    out
}

/// A switch's level, settled: it follows the raw level only once it has
/// held `DEBOUNCE_TICKS` control ticks running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Debounce {
    level: bool,
    run: u8,
}

/// 6 ms at the controls' 500 Hz.
pub const DEBOUNCE_TICKS: u8 = 3;

impl Debounce {
    pub const fn new() -> Self {
        Self {
            level: false,
            run: 0,
        }
    }

    /// One tick's raw level in; the settled level out.
    pub fn step(&mut self, raw: bool) -> bool {
        if raw == self.level {
            self.run = 0;
        } else {
            self.run += 1;
            if self.run >= DEBOUNCE_TICKS {
                self.level = raw;
                self.run = 0;
            }
        }
        self.level
    }
}

/// A switch from its first read: one down then (the key held at power-on
/// that picked the bootloader's slot) reads up until it is released once,
/// press and release alike (#1000).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartHold {
    #[default]
    Unread,
    Held,
    Free,
}

impl StartHold {
    pub const fn new() -> Self {
        StartHold::Unread
    }

    /// One tick's raw level and `Debounce`'s settled one in; the level
    /// the UI sees out.
    pub fn step(&mut self, raw: bool, settled: bool) -> bool {
        match *self {
            StartHold::Unread if raw => *self = StartHold::Held,
            StartHold::Held if !raw && !settled => *self = StartHold::Free,
            StartHold::Unread => *self = StartHold::Free,
            StartHold::Held | StartHold::Free => {}
        }
        match self {
            StartHold::Free => settled,
            StartHold::Unread | StartHold::Held => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EncoderId {
    A = 0,
    B = 1,
    C = 2,
    D = 3,
    E = 4,
    F = 5,
}

pub const ALL_ENCODERS: [EncoderId; NUM_ENCODERS] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ButtonId {
    B1 = 0,
    B2 = 1,
    B3 = 2,
    B4 = 3,
    B5 = 4,
    B6 = 5,
    Menu = 6,
    Minus = 7,
    Plus = 8,
    Mix = 9,
    Edit = 10,
    Seq = 11,
}

/// B1–B6: Part n's button, or with MIX its mixer.
pub const PART_BUTTONS: [ButtonId; 6] = [
    ButtonId::B1,
    ButtonId::B2,
    ButtonId::B3,
    ButtonId::B4,
    ButtonId::B5,
    ButtonId::B6,
];

pub const ALL_BUTTONS: [ButtonId; NUM_BUTTONS] = [
    ButtonId::B1,
    ButtonId::B2,
    ButtonId::B3,
    ButtonId::B4,
    ButtonId::B5,
    ButtonId::B6,
    ButtonId::Menu,
    ButtonId::Minus,
    ButtonId::Plus,
    ButtonId::Mix,
    ButtonId::Edit,
    ButtonId::Seq,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Up,
    Pressed,
    Held,
    Released,
}

impl ButtonState {
    /// From the button's level last frame and this frame.
    pub const fn from_levels(prev: bool, cur: bool) -> Self {
        match (prev, cur) {
            (false, true) => ButtonState::Pressed,
            (true, true) => ButtonState::Held,
            (true, false) => ButtonState::Released,
            (false, false) => ButtonState::Up,
        }
    }
}

impl ButtonState {
    /// From a frame's latched edges: a press this frame is `Pressed` even if
    /// it was already released, so a tap inside a stalled frame is kept.
    pub const fn from_edges(e: Edges) -> Self {
        if e.pressed_at.is_some() {
            ButtonState::Pressed
        } else if e.released_at.is_some() {
            ButtonState::Released
        } else if e.down {
            ButtonState::Held
        } else {
            ButtonState::Up
        }
    }
}

/// A millisecond timestamp. It wraps, so compare with `since`, never `<`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ms(pub u32);

impl Ms {
    /// Ms elapsed since `earlier`, across a wrap.
    pub fn since(self, earlier: Ms) -> u32 {
        self.0.wrapping_sub(earlier.0)
    }

    /// `since`, signed: negative when `earlier` is later (a stamp taken
    /// after this clock read).
    pub fn signed_since(self, earlier: Ms) -> i32 {
        i32::from_ne_bytes(self.since(earlier).to_ne_bytes())
    }

    /// `since`, as a duration.
    pub fn elapsed_since(self, earlier: Ms) -> units::Millis {
        units::Millis::new(self.since(earlier))
    }
}

/// A button's level now and the edges since the last frame, in ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges {
    pub down: bool,
    pub pressed_at: Option<Ms>,
    pub released_at: Option<Ms>,
}

/// Latches a button's edges between frames: the control tick feeds it
/// levels, the frame takes the edges. Several of one edge in a frame keep
/// the last.
#[derive(Clone, Copy, Debug, Default)]
pub struct Latch(Edges);

impl Latch {
    pub const fn new() -> Self {
        Self(Edges {
            down: false,
            pressed_at: None,
            released_at: None,
        })
    }

    /// Records a press or release when `down` differs from the last level.
    pub fn level(&mut self, down: bool, now_ms: Ms) {
        if down == self.0.down {
            return;
        }
        self.0.down = down;
        if down {
            self.0.pressed_at = Some(now_ms);
        } else {
            self.0.released_at = Some(now_ms);
        }
    }

    /// The frame's edges; clears them and keeps the level.
    pub fn take(&mut self) -> Edges {
        let e = self.0;
        self.0 = Edges {
            down: e.down,
            ..Edges::default()
        };
        e
    }
}

pub trait Controls {
    /// Encoder `id`'s ticks this frame, a parameter's acceleration in them.
    fn encoder_delta(&self, id: EncoderId) -> i8;
    fn button_state(&self, id: ButtonId) -> ButtonState;

    /// Encoder `id`'s raw detents this frame and where in its burst they
    /// fell (ADR 0279). Shells with no acceleration: `encoder_delta`, as
    /// ticks.
    fn turn(&self, id: EncoderId) -> Turn {
        Turn::steady(self.encoder_delta(id))
    }

    /// The button's edges this frame. Shells that latch override it; this
    /// one stamps `button_state`'s edge at `now_ms`.
    fn edges(&self, id: ButtonId) -> Edges {
        let now = Some(self.now_ms());
        match self.button_state(id) {
            ButtonState::Up => Edges::default(),
            ButtonState::Pressed => Edges {
                down: true,
                pressed_at: now,
                released_at: None,
            },
            ButtonState::Held => Edges {
                down: true,
                ..Edges::default()
            },
            ButtonState::Released => Edges {
                released_at: now,
                ..Edges::default()
            },
        }
    }

    /// The controls' clock, in ms.
    fn now_ms(&self) -> Ms {
        Ms(0)
    }

    /// Encoder `id`'s push this frame, as a button's edges. Shells without
    /// push encoders, or with them switched off, never press one.
    fn push(&self, _id: EncoderId) -> Edges {
        Edges::default()
    }
}

pub use chimera_midi::value::{Channel as MidiChannel, Key as MidiNote, Velocity};

/// RGB565 framebuffer pixel type
pub type Pixel = Rgb565;

/// Framebuffer size for 240x320 RGB565
pub const FB_SIZE: usize = (SCREEN_WIDTH as usize) * (SCREEN_HEIGHT as usize);

/// Write `pixels` into a row-major 240×320 RGB565 framebuffer, dropping any
/// off screen: both displays' `DrawTarget::draw_iter`.
pub fn draw_into_fb(
    fb: &mut [u16; FB_SIZE],
    pixels: impl IntoIterator<Item = embedded_graphics_core::Pixel<Rgb565>>,
) {
    use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
    for embedded_graphics_core::Pixel(p, color) in pixels {
        if (0..SCREEN_WIDTH as i32).contains(&p.x) && (0..SCREEN_HEIGHT as i32).contains(&p.y) {
            fb[p.y as usize * SCREEN_WIDTH as usize + p.x as usize] =
                RawU16::from(color).into_inner();
        }
    }
}

pub trait ChimeraDisplay: DrawTarget<Color = Rgb565> {
    /// Push the whole framebuffer to hardware.
    fn flush(&mut self) {
        self.flush_region(0, SCREEN_HEIGHT);
    }

    /// Push a horizontal band of the framebuffer (y_start inclusive, y_end exclusive)
    fn flush_region(&mut self, y_start: u16, y_end: u16);

    /// Raw pixel access for custom rendering
    fn pixel_buffer(&mut self) -> &mut [u16];
}

#[cfg(test)]
mod latch_tests {
    use super::*;

    #[test]
    fn latch_keeps_a_tap_between_takes() {
        let mut l = Latch::new();
        l.level(true, Ms(10));
        l.level(false, Ms(40));
        let e = l.take();
        assert_eq!(
            e,
            Edges {
                down: false,
                pressed_at: Some(Ms(10)),
                released_at: Some(Ms(40))
            }
        );
        assert_eq!(ButtonState::from_edges(e), ButtonState::Pressed);
        let e = l.take();
        assert_eq!(e, Edges::default());
        assert_eq!(ButtonState::from_edges(e), ButtonState::Up);
    }

    #[test]
    fn latch_held_across_takes() {
        let mut l = Latch::new();
        l.level(true, Ms(3));
        assert_eq!(ButtonState::from_edges(l.take()), ButtonState::Pressed);
        l.level(true, Ms(5)); // no change: no edge
        let e = l.take();
        assert_eq!(
            e,
            Edges {
                down: true,
                pressed_at: None,
                released_at: None
            }
        );
        assert_eq!(ButtonState::from_edges(e), ButtonState::Held);
    }

    #[test]
    fn latch_release_then_press() {
        let mut l = Latch::new();
        l.level(true, Ms(0));
        l.take();
        l.level(false, Ms(5));
        l.level(true, Ms(9));
        let e = l.take();
        assert_eq!(
            e,
            Edges {
                down: true,
                pressed_at: Some(Ms(9)),
                released_at: Some(Ms(5))
            }
        );
        assert_eq!(ButtonState::from_edges(e), ButtonState::Pressed);
    }

    #[test]
    fn a_lone_release_is_released() {
        let mut l = Latch::new();
        l.level(true, Ms(0));
        l.take();
        l.level(false, Ms(7));
        assert_eq!(ButtonState::from_edges(l.take()), ButtonState::Released);
    }

    struct Fixed(ButtonState);

    impl Controls for Fixed {
        fn encoder_delta(&self, _: EncoderId) -> i8 {
            0
        }
        fn button_state(&self, _: ButtonId) -> ButtonState {
            self.0
        }
        fn now_ms(&self) -> Ms {
            Ms(77)
        }
    }

    #[test]
    fn default_edges_follow_button_state() {
        for s in [
            ButtonState::Up,
            ButtonState::Pressed,
            ButtonState::Held,
            ButtonState::Released,
        ] {
            let e = Fixed(s).edges(ButtonId::Menu);
            assert_eq!(ButtonState::from_edges(e), s, "{s:?}");
        }
        assert_eq!(
            Fixed(ButtonState::Pressed).edges(ButtonId::Menu).pressed_at,
            Some(Ms(77))
        );
        assert_eq!(
            Fixed(ButtonState::Released)
                .edges(ButtonId::Menu)
                .released_at,
            Some(Ms(77))
        );
        assert_eq!(
            Fixed(ButtonState::Held).edges(ButtonId::Menu).pressed_at,
            None
        );
    }

    #[test]
    fn ms_since_wraps() {
        assert_eq!(Ms(5).since(Ms(u32::MAX - 4)), 10);
    }
}
