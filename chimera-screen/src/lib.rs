//! The browser editor's glyphs: one Part's `UiState`, headless, drawn by
//! the firmware's own `Renderer` into a 240x320 RGB565 buffer. Built for
//! wasm32 (`just editor-screen`); the `extern "C"` ABI is in `abi`, and
//! `tests/screen.rs` holds it to the unit's paths.
//!
//! Values reach the mirror as the stored values `ed since` reports: each
//! is checked as `ed set` checks it (the address `describe` lists for the
//! engine, a choice's `#n` or a number by the cell's own kind), then
//! written as the knob's turn lands it (`UiState::edit_to`, a pad's on its
//! pad), never through the box's refusals or cues: the unit already took
//! it. A pad's CHOKE is written to the pad itself, past the 8-group check
//! the turn makes: two legal kits can differ by steps that each make a 9th. A write can move others (a MACHINE takes its knobs' defaults), so a
//! batch is written until every value holds (`Mirror::apply`), whatever
//! its order.

use chimera_core::console::{
    Console, EdAddr, EdState, Request, SetRefusal, StoredRefusal, ed_stored,
};
use chimera_core::dsp::drum::types::{ChokeGroup, PadId};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{Origin, PartFrom, PartId, PartSource};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::held;
use chimera_core::ui::kit_band;
use chimera_core::ui::nav::{Location, PartChain};
use chimera_core::ui::page::PageLayout;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::palette;
use chimera_core::ui::view::{self, View};
use chimera_hal::{ALL_ENCODERS, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};
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
    /// A pad's machine icon, as the focus band draws it large
    /// (`kit_band::big_icon`'s scale and inks), alone.
    PadIcon = 3,
}

impl Region {
    pub fn from_u8(n: u8) -> Option<Region> {
        [Region::Full, Region::Screen, Region::Gauge, Region::PadIcon]
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

    /// A pad icon's, drawn at the top left.
    pub const PAD_ICON: Rect = Rect::span(
        0,
        0,
        kit_band::ICON_W * kit_band::Scale::FOCUS.px(),
        kit_band::ICON_H * kit_band::Scale::FOCUS.px(),
    );

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
            (Region::PadIcon, _) => Some(Rect::PAD_ICON),
        }
    }
}

/// A page's cell, by its knob: a-f, 0-5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot(u8);

impl Slot {
    pub const fn new(n: u8) -> Option<Slot> {
        if (n as usize) < ALL_ENCODERS.len() {
            Some(Slot(n))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// An `ed` address, as `describe` writes it (`FILTER.CUTOFF`,
/// `PAD3_FLT.KIND`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addr(EdAddr);

impl Addr {
    pub fn parse(text: &str) -> Option<Addr> {
        EdAddr::parse(text.as_bytes()).map(Addr)
    }
}

impl core::fmt::Display for Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Why a stored value didn't land, typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `describe` lists no such address for the Part's engine.
    NotHere(Addr),
    /// NaN or an infinity.
    NotFinite(Addr),
    /// What `ed set` says of it: no such option, or a number for a choice.
    Set(Addr, SetRefusal),
    /// Written, and the Part holds another value: a write the rest of the
    /// batch undid every pass.
    Kept(Addr),
}

impl From<(Addr, StoredRefusal)> for Refused {
    fn from((a, r): (Addr, StoredRefusal)) -> Refused {
        match r {
            StoredRefusal::NotHere => Refused::NotHere(a),
            StoredRefusal::NotFinite => Refused::NotFinite(a),
            StoredRefusal::Set(s) => Refused::Set(a, s),
        }
    }
}

/// What `Mirror::apply` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Values written: a value the Part held already is never written.
    pub writes: usize,
    /// Passes over the batch, the last writing nothing.
    pub passes: usize,
    /// What never landed.
    pub refused: Vec<Refused>,
}

/// How a page is shown: the cell in its focus band, the pad selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// The focus band's cell, as its last turn leaves it; `None`: slot a,
    /// the unit's until a turn. ALGO's algorithm page morphs its chart
    /// with MORPH only off ALG A and B: `Slot::new(2)`, MORPH's own.
    pub focus: Option<Slot>,
    /// DRUM's selected pad: what KIT's pads draw selected, and what a
    /// pad's page shows. `None`: the page's own pad (`PAD5_KIT` is pad
    /// 5's), else pad 1.
    pub pad: Option<PadId>,
}

/// `held` is the value `ed since` printed as `sent`: it prints seven
/// decimals (`describe::num`), so a continuous value comes back within
/// half a unit of the seventh place, and within an f32's rounding of it.
pub fn same(held: f32, sent: f32) -> bool {
    (held - sent).abs() <= 1e-7_f32.max(sent.abs() * 2.0 * f32::EPSILON)
}

/// Passes `apply` makes at most: a selector, the params it resets, and a
/// CHOKE that waits on another pad's, each a pass.
const PASSES: usize = 8;

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

    /// The value `a` holds, a pad's on its pad.
    pub fn stored(&self, a: Addr) -> Option<f32> {
        ed_stored(&self.ui, a.0)
    }

    /// `a` to the stored value `v`: checked as `ed set` checks it, then
    /// written as the knob's turn lands it. Whether it moved.
    pub fn set(&mut self, a: Addr, v: f32) -> Result<bool, Refused> {
        let v = self
            .ed
            .stored_set(&self.ui, a.0, v)
            .map_err(|r| Refused::from((a, r)))?;
        let (p, at) = (self.part(), a.0.param());
        Ok(match a.0.pad() {
            None => self.ui.edit_to(p, at, v),
            // Past the group check: the unit holds this kit, and a diff
            // between two 8-group kits can pass through a 9th.
            Some(n) if at == held::CHOKE => {
                let to = ChokeGroup::from_index(v as usize)
                    .ok_or(Refused::Set(a, SetRefusal::NoSuchOption))?;
                let pad = self.ui.project().kit(p).pad(n);
                let moved = pad.choke() != to;
                if moved {
                    self.ui.project_mut().kit_mut(p).pad_mut(n).set_choke(to);
                }
                moved
            }
            // On its pad, the selection put back: as `ed set`'s turn.
            Some(n) => {
                let was = self.ui.selected_pad(p);
                self.ui.project_mut().select_pad(p, n);
                let moved = self.ui.edit_to(p, at, v);
                self.ui.project_mut().select_pad(p, was);
                moved
            }
        })
    }

    /// Every value in `vals` to what it says, in any order: each pass
    /// writes the values the Part doesn't hold, until a pass moves
    /// nothing. A selector written after the params it resets (a MACHINE
    /// after its knobs) costs one more pass; a value the Part holds is
    /// never written, so a batch the mirror has mostly seen is its diff.
    pub fn apply(&mut self, vals: &[(Addr, f32)]) -> Applied {
        let mut done = Applied::default();
        while done.passes < PASSES {
            done.passes += 1;
            done.refused.clear();
            let mut moved = false;
            for &(a, v) in vals {
                if self.stored(a).is_some_and(|h| same(h, v)) {
                    continue;
                }
                match self.set(a, v) {
                    Ok(m) => {
                        done.writes += 1;
                        moved |= m;
                    }
                    Err(r) => done.refused.push(r),
                }
            }
            if !moved {
                break;
            }
        }
        for &(a, v) in vals {
            let held = self.stored(a);
            let said = done.refused.iter().any(|r| match *r {
                Refused::NotHere(b) | Refused::NotFinite(b) | Refused::Set(b, _) => b == a,
                Refused::Kept(_) => false,
            });
            if !said && held.is_some_and(|h| !same(h, v)) {
                done.refused.push(Refused::Kept(a));
            }
        }
        done
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

    /// Page `n` of `ed describe`'s walk on the Part's engine, shown as
    /// `how` says, drawn into `fb` in the unit's colours. `None`: the
    /// engine has no such page, the Part's chain doesn't show it (an ALGO
    /// formant page on a Sound with no formant operator), or the focus
    /// names an empty cell.
    pub fn render(&mut self, n: PageNo, how: Shown, fb: &mut Fb) -> Option<()> {
        self.show(n, how)?;
        self.ui.update(UiTick::for_test());
        self.ui.render_with_scope(fb, &PerfStats::zero(), &SCOPE);
        self.paint(fb);
        Some(())
    }

    /// The unit's palette over `fb`, as the desktop's window maps it.
    fn paint(&self, fb: &mut Fb) {
        let pal = palette(&self.ui.theme());
        fb.0.iter_mut().for_each(|px| *px = pal.map_raw(*px));
    }

    /// The Part on page `n`, shown as `how` says: its pad selected (a
    /// pad's page's own), entered as its key enters it, the focus set.
    pub fn show(&mut self, n: PageNo, how: Shown) -> Option<()> {
        let e = self.engine();
        let (def, own) = pages::page_at(e, n)?;
        let p = self.part();
        if e == EngineType::Drum {
            let pad = how.pad.or(own).unwrap_or(PadId::ALL[0]);
            self.ui.select_pad(p, pad);
        }
        let chain = PartChain::of(&self.ui.project().part(p).sound).def();
        let at = chain.locate(def.id)?;
        self.ui.go_for_test(Location::pages(p, at));
        self.shown = Some(def);
        let slot = how.focus.map_or(0, Slot::index);
        let ctx = self.ui.slot_ctx_for_test();
        if how.focus.is_some() && view::view(def, slot, &ctx) == View::Empty {
            return None;
        }
        self.ui.focus_for_test(slot);
        // A still: the display's own motion at rest, so the picture is
        // this Sound's on this page, whatever was drawn before (#1035).
        self.ui.still_for_test();
        Some(())
    }

    /// Pad `pad`'s machine icon, as the focus band draws it large, at the
    /// top left of `fb` (`Rect::PAD_ICON`) on the page's ground.
    pub fn pad_icon(&mut self, pad: PadId, fb: &mut Fb) {
        let m = self.ui.project().kit(self.part()).pad(pad).machine();
        let r = Rect::PAD_ICON;
        let ground = RawU16::from(theme::BG).into_inner();
        for y in 0..usize::from(r.h) {
            let row = y * usize::from(SCREEN_WIDTH);
            fb.0[row..row + usize::from(r.w)].fill(ground);
        }
        kit_band::icon(
            fb,
            m,
            0,
            0,
            kit_band::Scale::FOCUS,
            theme::ACCENT,
            theme::INK2,
        );
        self.paint(fb);
    }

    /// `r` on the page last shown.
    pub fn region(&self, r: Region) -> Option<Rect> {
        Rect::of(r, self.shown?.layout)
    }
}
