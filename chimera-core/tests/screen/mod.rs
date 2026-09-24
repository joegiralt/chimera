//! Screen test harness (UI refresh spec § Testing): an in-memory 240×320
//! RGB565 display, the named screens the goldens lock, and a hash.
//!
//! `SCREEN_DUMP=<dir>` writes every rendered case to `<dir>/<case>.ppm`
//! for eyeballing against the mockups (`magick x.ppm x.png`).

#![allow(dead_code)]

use chimera_core::preset::{ChainType, Sound, POOL_SIZE};
use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, ButtonState, ChimeraDisplay, Controls, EncoderId};
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

pub const W: usize = 240;
pub const H: usize = 320;

/// A 240×320 framebuffer that counts writes outside the screen.
pub struct Fb {
    pub px: Vec<u16>,
    /// Pixels drawn outside 240×320 (must stay 0).
    pub oob: usize,
}

impl Fb {
    pub fn new() -> Self {
        Self { px: vec![0; W * H], oob: 0 }
    }

    pub fn at(&self, x: i32, y: i32) -> Rgb565 {
        RawU16::new(self.px[y as usize * W + x as usize]).into()
    }

    /// FNV-1a 64 over every pixel.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &p in &self.px {
            for b in p.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// Write a binary PPM to `$SCREEN_DUMP/<name>.ppm` when the variable is set.
    pub fn dump(&self, name: &str) {
        let Some(dir) = std::env::var_os("SCREEN_DUMP") else { return };
        let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
        for &p in &self.px {
            let c: Rgb565 = RawU16::new(p).into();
            out.extend([(c.r() << 3) | (c.r() >> 2), (c.g() << 2) | (c.g() >> 4), (c.b() << 3) | (c.b() >> 2)]);
        }
        let path = std::path::Path::new(&dir).join(format!("{name}.ppm"));
        std::fs::write(&path, out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

impl OriginDimensions for Fb {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Fb {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if (0..W as i32).contains(&p.x) && (0..H as i32).contains(&p.y) {
                self.px[p.y as usize * W + p.x as usize] = RawU16::from(c).into_inner();
            } else {
                self.oob += 1;
            }
        }
        Ok(())
    }
}

impl ChimeraDisplay for Fb {
    fn flush(&mut self) {}
    fn flush_region(&mut self, _y_start: u16, _y_end: u16) {}
    fn pixel_buffer(&mut self) -> &mut [u16] {
        &mut self.px
    }
}

/// One frame of input.
#[derive(Default)]
pub struct Input {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl Input {
    pub fn press(b: ButtonId) -> Self {
        Self { buttons: vec![(b, ButtonState::Pressed)], ..Self::default() }
    }
    /// `held` down while `b` is pressed (MIX + B1, EDIT + B1, MIX + PLUS).
    pub fn chord(held: ButtonId, b: ButtonId) -> Self {
        Self { buttons: vec![(held, ButtonState::Held), (b, ButtonState::Pressed)], ..Self::default() }
    }
    pub fn turn(e: EncoderId, delta: i8) -> Self {
        Self { encoders: vec![(e, delta)], ..Self::default() }
    }
}

impl Controls for Input {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        self.buttons.iter().find(|b| b.0 == id).map_or(ButtonState::Up, |b| b.1)
    }
}

pub fn feed(ui: &mut UiState, input: Input) {
    ui.handle_input(&input);
}

/// Let every lerp settle (the goldens lock the resting screen).
pub fn settle(ui: &mut UiState) {
    for _ in 0..120 {
        ui.update();
    }
}

/// Live output the goldens draw: two periods of a lopsided triangle, peak 0.5.
pub fn scope_fixture() -> [f32; SCOPE_LEN] {
    core::array::from_fn(|i| {
        let p = (i as f32 / 120.0) % 1.0;
        let s = 0.7;
        0.5 * if p < s { -1.0 + 2.0 * p / s } else { 1.0 - 2.0 * (p - s) / (1.0 - s) }
    })
}

/// Load `ct`'s init Sound into Part 1 through the sound browser (EDIT + B1,
/// scroll to the init row, EDIT).
pub fn load_init(ui: &mut UiState, ct: ChainType) {
    let row = POOL_SIZE + ChainType::ALL.iter().position(|&c| c == ct).unwrap();
    feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    feed(ui, Input::turn(EncoderId::Main, row as i8));
    feed(ui, Input::press(ButtonId::Edit));
}

fn plus(ui: &mut UiState, n: usize) {
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// Prime the focused slot for modulation (MIX + PLUS).
fn prime(ui: &mut UiState) {
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
}

/// Every screen the goldens lock, one or more per page type (spec § Testing).
pub const CASES: &[(&str, fn(&mut UiState))] = &[
    ("engine_pizza", |ui| feed(ui, Input::turn(EncoderId::A, 2))),
    ("engine_fm_alg", |ui| {
        load_init(ui, ChainType::Fm);
        feed(ui, Input::turn(EncoderId::A, 3));
    }),
    ("engine_fm_op", |ui| {
        load_init(ui, ChainType::Fm);
        feed(ui, Input::press(ButtonId::Edit)); // Operator sub-page
        feed(ui, Input::turn(EncoderId::A, 2)); // select operator 3
    }),
    ("bigviz_filter", |ui| {
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::B, 80)); // resonance
        feed(ui, Input::turn(EncoderId::A, -60)); // cutoff, focused
    }),
    ("bigviz_env", |ui| {
        plus(ui, 4);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::B, 6));
    }),
    ("bigviz_fm_op_env", |ui| {
        load_init(ui, ChainType::Fm);
        plus(ui, 4);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::C, -4));
    }),
    ("mixer_part", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::D, -8));
    }),
    ("mixer_sends", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::C, 40));
    }),
    ("mixer_fx_delay", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        plus(ui, 3);
    }),
    ("mod_matrix", |ui| {
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 1)); // focus CUTOFF
        prime(ui);
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::A, 1)); // focus FOLD
        prime(ui);
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::E, 20)); // ENV → CUTOFF
        feed(ui, Input::turn(EncoderId::B, 1));
        feed(ui, Input::turn(EncoderId::E, -30)); // ENV → FOLD
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::B, -1));
        feed(ui, Input::turn(EncoderId::E, 42)); // LFO → CUTOFF, selected
    }),
    ("sound_browser", |ui| {
        let mut s = Sound::init(ChainType::Fm);
        s.name = [0; 16];
        s.name[..9].copy_from_slice(b"WARM BASS");
        ui.pool.store(0, s);
        let mut s = Sound::init(ChainType::Modal);
        s.name = [0; 16];
        s.name[..11].copy_from_slice(b"GLASS PLUCK");
        ui.pool.store(1, s);
        feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::Main, 1));
    }),
    ("system", |ui| feed(ui, Input::press(ButtonId::Menu))),
];

/// Build case `name`'s screen: a fresh UiState, the case's input, settled lerps.
pub fn ui_for(name: &str) -> UiState {
    let (_, setup) = CASES.iter().find(|c| c.0 == name).unwrap_or_else(|| panic!("no case {name}"));
    let mut ui = UiState::new();
    setup(&mut ui);
    settle(&mut ui);
    ui
}

/// Full render of case `name`.
pub fn render(name: &str) -> Fb {
    let ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb.dump(name);
    fb
}

/// Render case `name` through `render_dirty` from a fresh region set.
pub fn render_dirty(name: &str) -> Fb {
    let mut ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb
}
