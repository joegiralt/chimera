//! The browser editor's glyphs: one Part's `UiState`, headless, drawn by
//! the firmware's own `Renderer` into a 240x320 RGB565 buffer. Built for
//! wasm32 (`just editor-screen`); the `extern "C"` ABI is in `abi`.
//!
//! The mirror takes values as the unit does: each set is an `ed set` line
//! answered by the console's own `ed` and applied as a knob turn
//! (`EdState::apply`), so a refusal here is the unit's refusal.

use chimera_core::addr::BlockRead;
use chimera_core::console::{Console, EdState, Request};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{Origin, PartFrom, PartId, PartSource};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::nav::{Location, PartChain};
use chimera_core::ui::page::PageLayout;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::palette;
use chimera_hal::{
    ALL_ENCODERS, ButtonId, ButtonState, Controls, EncoderId, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH,
};
use embedded_graphics_core::Pixel;
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};

pub mod abi;
pub mod pages;

pub use pages::PageNo;

/// The live output the pages draw: silence, so a glyph is the Sound's
/// alone.
const SCOPE: [f32; chimera_core::scope::SCOPE_LEN] = [0.0; chimera_core::scope::SCOPE_LEN];

/// The screen's pixels, drawn as the desktop's window draws them
/// (`DesktopDisplay`): a clear is a fill, the rest `draw_into_fb`.
pub struct Fb(pub Box<[u16; FB_SIZE]>);

impl Fb {
    pub fn new() -> Fb {
        Fb(vec![0; FB_SIZE].try_into().expect("FB_SIZE pixels"))
    }
}

impl Default for Fb {
    fn default() -> Self {
        Self::new()
    }
}

impl DrawTarget for Fb {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        self.0.fill(RawU16::from(color).into_inner());
        Ok(())
    }

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        chimera_hal::draw_into_fb(&mut self.0, pixels);
        Ok(())
    }
}

impl OriginDimensions for Fb {
    fn size(&self) -> Size {
        Size::new(u32::from(SCREEN_WIDTH), u32::from(SCREEN_HEIGHT))
    }
}

/// A part of the screen the editor crops to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Region {
    /// The whole 240x320.
    Full = 0,
    /// The page's own art: BigViz's plot, CellGrid's viz band, the matrix.
    Screen = 1,
    /// CellGrid's focus band, right of the big value: the focused cell's
    /// gauge (arc, switch, scope, crossfader...).
    Gauge = 2,
}

impl Region {
    pub fn from_u8(n: u8) -> Option<Region> {
        [Region::Full, Region::Screen, Region::Gauge]
            .into_iter()
            .find(|r| *r as u8 == n)
    }
}

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    const fn span(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect {
            x: x0 as u16,
            y: y0 as u16,
            w: (x1 - x0) as u16,
            h: (y1 - y0) as u16,
        }
    }

    /// `r` on a page laid out `layout`; `None` where it has none.
    pub fn of(r: Region, layout: PageLayout) -> Option<Rect> {
        use theme::*;
        match (r, layout) {
            (Region::Full, _) => Some(Rect::span(0, 0, SCREEN_W, SCREEN_H)),
            (Region::Screen, PageLayout::BigViz) => {
                Some(Rect::span(0, HEADER_BOTTOM, SCREEN_W, BIGVIZ_BOTTOM))
            }
            (Region::Screen, PageLayout::CellGrid) => {
                Some(Rect::span(0, VIZ_BAND_TOP, SCREEN_W, VIZ_BAND_BOTTOM))
            }
            (Region::Screen, PageLayout::Matrix) => {
                Some(Rect::span(0, HEADER_BOTTOM, SCREEN_W, CELLS_BOTTOM))
            }
            // The widest gauge (VA's BLEND) and its end words, to the edge.
            (Region::Gauge, PageLayout::CellGrid) => Some(Rect::span(
                SCREEN_W - MARGIN_X - BLEND_W - MARGIN_X,
                HEADER_BOTTOM,
                SCREEN_W,
                FOCUS_BOTTOM,
            )),
            (Region::Gauge, PageLayout::BigViz | PageLayout::Matrix) => None,
        }
    }
}

/// A page's cell, by its knob: a-f, 0-5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot(EncoderId);

impl Slot {
    pub const fn new(n: u8) -> Option<Slot> {
        if (n as usize) < ALL_ENCODERS.len() {
            Some(Slot(ALL_ENCODERS[n as usize]))
        } else {
            None
        }
    }
}

/// One frame of the controls: knob `0` turned `1` detents, nothing else.
struct Turn(Slot, i8);

impl Controls for Turn {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        if id == self.0.0 { self.1 } else { 0 }
    }

    fn button_state(&self, _: ButtonId) -> ButtonState {
        ButtonState::Up
    }
}

/// Why a set didn't land: the unit's own `ERR` line, kept for the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

/// One Part of the unit, headless: Part 1 on an INIT Sound.
pub struct Mirror {
    ui: Box<UiState>,
    ed: EdState,
    /// The last page drawn, for its regions.
    shown: Option<&'static BlockDef>,
}

impl Default for Mirror {
    fn default() -> Self {
        Self::new()
    }
}

impl Mirror {
    pub fn new() -> Mirror {
        let mut m = Mirror {
            ui: Box::new(UiState::new()),
            ed: EdState::new(),
            shown: None,
        };
        // NEW's Part is the template's, unedited: this load never asks,
        // and from here the Part's origin is an INIT.
        m.load(EngineType::Algo);
        m
    }

    fn load(&mut self, engine: EngineType) {
        let part = self.part();
        self.ui.load_for_test(PartSource {
            part,
            from: PartFrom::Init(engine),
        });
    }

    pub fn ui(&self) -> &UiState {
        &self.ui
    }

    fn part(&self) -> PartId {
        self.ui.active_part
    }

    pub fn engine(&self) -> EngineType {
        self.ui.project().part(self.part()).sound.engine()
    }

    /// `engine`'s INIT Sound on the Part, as EDIT's load of INIT lands it.
    /// The Sound is put back to its own INIT first, so the load never
    /// stops at SAVE PART FIRST.
    pub fn init(&mut self, engine: EngineType) {
        let p = self.part();
        if let Origin::Init(was) = self.ui.project().part(p).origin() {
            **self.ui.project_mut().edit_part(p).sound = Sound::init(was);
        }
        self.load(engine);
    }

    /// `addr` (an `ed` address, `FILTER.CUTOFF`) to the value stored
    /// `stored`, through `ed set` and the knob's turn. A whole number goes
    /// as `#n` first, a choice's option; a param that takes a number gets
    /// it as a decimal.
    pub fn set(&mut self, addr: &str, stored: f32) -> Result<(), Refused> {
        if !stored.is_finite() {
            return Err(Refused(format!("ERR {addr} {stored} is no value")));
        }
        let whole = stored.fract() == 0.0 && (0.0..=255.0).contains(&stored);
        let mut got = if whole {
            self.ed_line(&format!("ed set {addr} #{}", stored as u8))
        } else {
            String::new()
        };
        if !whole || got.starts_with("ERR ed set ") && got.ends_with("takes a number\n") {
            got = self.ed_line(&format!("ed set {addr} {stored}"));
        }
        chimera_core::bank2::ed_apply(&mut self.ed, &mut self.ui);
        match got.as_str() {
            "OK queued\n" => Ok(()),
            _ => Err(Refused(got.trim_end().to_string())),
        }
    }

    /// One console line's answer, from the console's own `ed`.
    pub fn ed_line(&mut self, line: &str) -> String {
        let mut c = Console::new();
        let mut req = None;
        for &b in line.as_bytes().iter().chain(b"\n") {
            req = c.push(b);
        }
        let mut out = String::new();
        if let Some(Ok(Request::Ed(l))) = req {
            let _ = chimera_core::bank2::ed_door(|d| {
                d.answer(&l, &self.ui, Some(&mut self.ed), &mut out)
            });
        }
        if out.is_empty() {
            out = format!("ERR {line} is no ed line\n");
        }
        out
    }

    /// Page `n` of `ed describe`'s walk on the Part's engine, drawn into
    /// `fb` in the unit's colours. `None`: the engine has no such page, or
    /// the Part's chain doesn't show it (an ALGO formant page on a Sound
    /// with no formant operator).
    pub fn render(&mut self, n: PageNo, fb: &mut Fb) -> Option<()> {
        self.go(n)?;
        self.ui.update(UiTick::for_test());
        self.ui.render_with_scope(fb, &PerfStats::zero(), &SCOPE);
        let pal = palette(&self.ui.theme());
        fb.0.iter_mut().for_each(|px| *px = pal.map_raw(*px));
        Some(())
    }

    /// The Part's pages on page `n` (a pad's selected for its pages).
    pub fn go(&mut self, n: PageNo) -> Option<()> {
        let e = self.engine();
        let (def, pad) = pages::page_at(e, n)?;
        let p = self.part();
        if let Some(pad) = pad {
            self.ui.select_pad(p, pad);
        }
        let chain = PartChain::of(&self.ui.project().part(p).sound).def();
        let at = chain.locate(def.id)?;
        self.ui.go_for_test(Location::pages(p, at));
        self.shown = Some(def);
        Some(())
    }

    /// Page `n` with its cell `slot` (knob a-f) in the focus band, as the
    /// unit's last-turned cell: what the band's gauge shows, and ALG's
    /// chart morphing rather than showing ALG A. A turn is the only way
    /// the box focuses a cell, so it is one detent up and one down, then
    /// the value put back by the knob's path if the round trip moved it
    /// (a clamp at an end). `None`: no such page, or an empty cell.
    pub fn focus(&mut self, n: PageNo, slot: Slot) -> Option<()> {
        self.go(n)?;
        let def = self.shown?;
        let p = self.part();
        let ctx = self.ui.slot_ctx_for_test();
        let view = chimera_core::ui::view::view(def, slot.0 as usize, &ctx);
        if view == chimera_core::ui::view::View::Empty {
            return None;
        }
        let stored = |ui: &UiState| {
            let a = view.addr()?;
            ui.project()
                .part(p)
                .sound
                .params
                .block(a.block)
                .map(|b| b.get(a.param))
        };
        let before = stored(&self.ui);
        for d in [1, -1] {
            self.ui.handle_input(&Turn(slot, d));
        }
        if let (Some(a), Some(v)) = (view.addr(), before)
            && stored(&self.ui) != before
        {
            self.ui.edit_to(p, a, v);
        }
        Some(())
    }

    /// `r` on the page last drawn.
    pub fn region(&self, r: Region) -> Option<Rect> {
        Rect::of(r, self.shown?.layout)
    }
}
