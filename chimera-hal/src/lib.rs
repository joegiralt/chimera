#![no_std]

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::pixelcolor::Rgb565;

pub mod midi;
pub mod store;
#[cfg(feature = "testkit")]
pub mod testkit;

pub const BLOCK_SIZE: usize = 64;
pub const SCREEN_WIDTH: u16 = 240;
pub const SCREEN_HEIGHT: u16 = 320;
pub const SAMPLE_RATE: u32 = 48_000;

/// The PreenFM3's six encoders, one per parameter cell.
pub const NUM_ENCODERS: usize = 6;
pub const NUM_BUTTONS: usize = 12; // 6 param + 6 nav

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

/// A button's level now and the edges since the last frame, in ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges {
    pub down: bool,
    pub pressed_at: Option<u32>,
    pub released_at: Option<u32>,
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
    pub fn level(&mut self, down: bool, now_ms: u32) {
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
    fn encoder_delta(&self, id: EncoderId) -> i8;
    fn button_state(&self, id: ButtonId) -> ButtonState;

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
    fn now_ms(&self) -> u32 {
        0
    }
}

/// MIDI note number, 0..=127. Built at the MIDI trust boundary (the parser,
/// the desktop keyboard), so the audio path only ever sees valid notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MidiNote(u8);

impl MidiNote {
    /// A4 (440 Hz).
    pub const A4: MidiNote = MidiNote(69);

    pub const fn new(n: u8) -> Option<Self> {
        if n <= 127 { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// MIDI channel, 0..=15 (the low nibble of the status byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MidiChannel(u8);

impl MidiChannel {
    pub const fn new(c: u8) -> Option<Self> {
        if c <= 15 { Some(Self(c)) } else { None }
    }

    /// `c` limited to 15: for values already clamped by a param spec.
    pub const fn clamped(c: u8) -> Self {
        Self(if c > 15 { 15 } else { c })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Note-on velocity, 1..=127. Zero means note-off and is not representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(u8);

impl Velocity {
    pub const MAX: Velocity = Velocity(127);
    /// Velocity of the desktop keyboard and the firmware test note.
    pub const DEFAULT: Velocity = Velocity(100);

    pub const fn new(v: u8) -> Option<Self> {
        if v >= 1 && v <= 127 {
            Some(Self(v))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Velocity as 0..1, exactly `v as f32 / 127.0` (what the engines used).
    pub fn unit(self) -> f32 {
        self.0 as f32 / 127.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOn {
        channel: MidiChannel,
        note: MidiNote,
        velocity: Velocity,
    },
    /// Release velocity may be 0.
    NoteOff {
        channel: MidiChannel,
        note: MidiNote,
        velocity: u8,
    },
    ControlChange {
        channel: MidiChannel,
        cc: u8,
        value: u8,
    },
    PitchBend {
        channel: MidiChannel,
        value: i16,
    },
}

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
        l.level(true, 10);
        l.level(false, 40);
        let e = l.take();
        assert_eq!(
            e,
            Edges {
                down: false,
                pressed_at: Some(10),
                released_at: Some(40)
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
        l.level(true, 3);
        assert_eq!(ButtonState::from_edges(l.take()), ButtonState::Pressed);
        l.level(true, 5); // no change: no edge
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
        l.level(true, 0);
        l.take();
        l.level(false, 5);
        l.level(true, 9);
        let e = l.take();
        assert_eq!(
            e,
            Edges {
                down: true,
                pressed_at: Some(9),
                released_at: Some(5)
            }
        );
        assert_eq!(ButtonState::from_edges(e), ButtonState::Pressed);
    }

    #[test]
    fn a_lone_release_is_released() {
        let mut l = Latch::new();
        l.level(true, 0);
        l.take();
        l.level(false, 7);
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
        fn now_ms(&self) -> u32 {
            77
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
            Some(77)
        );
        assert_eq!(
            Fixed(ButtonState::Released)
                .edges(ButtonId::Menu)
                .released_at,
            Some(77)
        );
        assert_eq!(
            Fixed(ButtonState::Held).edges(ButtonId::Menu).pressed_at,
            None
        );
    }
}
