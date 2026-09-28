#![no_std]

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::pixelcolor::Rgb565;

pub mod midi;
pub mod store;

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

pub trait Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8;
    fn button_state(&self, id: ButtonId) -> ButtonState;
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
