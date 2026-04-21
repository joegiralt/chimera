#![no_std]

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::pixelcolor::Rgb565;

pub const BLOCK_SIZE: usize = 64;
pub const SCREEN_WIDTH: u16 = 240;
pub const SCREEN_HEIGHT: u16 = 320;
pub const SAMPLE_RATE: u32 = 48_000;

pub const NUM_ENCODERS: usize = 7; // 6 param + 1 main
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
    Main = 6,
}

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

pub trait Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8;
    fn button_state(&self, id: ButtonId) -> ButtonState;
}

pub trait MidiIn {
    fn read(&mut self) -> Option<MidiMessage>;
}

#[derive(Clone, Copy, Debug)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    ControlChange { channel: u8, cc: u8, value: u8 },
    PitchBend { channel: u8, value: i16 },
}

/// RGB565 framebuffer pixel type
pub type Pixel = Rgb565;

/// Framebuffer size for 240x320 RGB565
pub const FB_SIZE: usize = (SCREEN_WIDTH as usize) * (SCREEN_HEIGHT as usize);

pub trait ChimeraDisplay: DrawTarget<Color = Rgb565> {
    /// Push framebuffer to hardware
    fn flush(&mut self);

    /// Push a horizontal band of the framebuffer (y_start inclusive, y_end exclusive)
    fn flush_region(&mut self, y_start: u16, y_end: u16);

    /// Raw pixel access for custom rendering
    fn pixel_buffer(&mut self) -> &mut [u16];
}
