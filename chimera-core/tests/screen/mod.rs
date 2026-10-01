//! Screen test harness (UI refresh spec § Testing): an in-memory 240×320
//! RGB565 display, the named screens the goldens lock, and a hash.
//!
//! `SCREEN_DUMP=<dir>` writes every rendered case to `<dir>/<case>.ppm`
//! for eyeballing against the mockups (`magick x.ppm x.png`).

#![allow(dead_code)]

use chimera_core::clock_plan::SiliconRev;
use chimera_core::params::EngineType;
use chimera_core::perf::load::AudioStats;
use chimera_core::preset::{POOL_SIZE, Sound};
use chimera_core::project::SlotId;
use chimera_core::reset::ResetCause;
use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::busy::{draw_busy, draw_toast};
use chimera_core::ui::hold::HOLD_MS;
use chimera_core::ui::perf::PerfStats;
use chimera_hal::{ButtonId, ButtonState, ChimeraDisplay, Controls, Edges, EncoderId, Ms};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::prelude::*;

pub const W: usize = 240;
pub const H: usize = 320;

/// A 240×320 framebuffer that counts its pixel writes.
pub struct Fb {
    pub px: Vec<u16>,
    /// Pixels drawn outside 240×320 (must stay 0).
    pub oob: usize,
    /// Pixels drawn through `draw_iter`, on screen or off.
    pub drawn: usize,
}

impl Fb {
    pub fn new() -> Self {
        Self {
            px: vec![0; W * H],
            oob: 0,
            drawn: 0,
        }
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
        let Some(dir) = std::env::var_os("SCREEN_DUMP") else {
            return;
        };
        let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
        for &p in &self.px {
            let c: Rgb565 = RawU16::new(p).into();
            out.extend([
                (c.r() << 3) | (c.r() >> 2),
                (c.g() << 2) | (c.g() >> 4),
                (c.b() << 3) | (c.b() >> 2),
            ]);
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

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            self.drawn += 1;
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
    at_ms: u32,
    buttons: Vec<(ButtonId, ButtonState)>,
    /// Latched edges that a `ButtonState` can't say (a tap inside one frame).
    edges: Vec<(ButtonId, Edges)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl Input {
    /// The frame's clock; press and release edges are stamped with it.
    pub fn at(mut self, ms: u32) -> Self {
        self.at_ms = ms;
        self
    }
    pub fn release(b: ButtonId) -> Self {
        Self {
            buttons: vec![(b, ButtonState::Released)],
            ..Self::default()
        }
    }
    /// A frame with `b` still down.
    pub fn held(b: ButtonId) -> Self {
        Self {
            buttons: vec![(b, ButtonState::Held)],
            ..Self::default()
        }
    }
    pub fn press(b: ButtonId) -> Self {
        Self {
            buttons: vec![(b, ButtonState::Pressed)],
            ..Self::default()
        }
    }
    /// `held` down while `b` is pressed (MIX + B1, EDIT + B1, MIX + PLUS).
    pub fn chord(held: ButtonId, b: ButtonId) -> Self {
        Self {
            buttons: vec![(held, ButtonState::Held), (b, ButtonState::Pressed)],
            ..Self::default()
        }
    }
    /// `b` pressed and released inside this one frame (a stalled frame).
    pub fn tap_in_frame(b: ButtonId) -> Self {
        Self {
            edges: vec![(
                b,
                Edges {
                    down: false,
                    pressed_at: Some(Ms(0)),
                    released_at: Some(Ms(50)),
                },
            )],
            ..Self::default()
        }
    }
    /// This frame with `b` held down too.
    pub fn and_held(mut self, b: ButtonId) -> Self {
        self.buttons.push((b, ButtonState::Held));
        self
    }
    pub fn turn(e: EncoderId, delta: i8) -> Self {
        Self {
            encoders: vec![(e, delta)],
            ..Self::default()
        }
    }
}

impl Controls for Input {
    fn now_ms(&self) -> Ms {
        Ms(self.at_ms)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        if let Some(e) = self.edges.iter().find(|e| e.0 == id) {
            return ButtonState::from_edges(e.1);
        }
        self.buttons
            .iter()
            .find(|b| b.0 == id)
            .map_or(ButtonState::Up, |b| b.1)
    }
    fn edges(&self, id: ButtonId) -> Edges {
        if let Some(e) = self.edges.iter().find(|e| e.0 == id) {
            return e.1;
        }
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
}

pub fn feed(ui: &mut UiState, input: Input) {
    ui.handle_input(&input);
}

/// Press, then release 100 ms later.
pub fn tap(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::press(b).at(0));
    feed(ui, Input::release(b).at(100));
}

/// Press, a frame at `HOLD_MS`, then release.
pub fn hold(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::press(b).at(0));
    feed(ui, Input::held(b).at(HOLD_MS));
    feed(ui, Input::release(b).at(HOLD_MS + 33));
}

/// Let every lerp settle (the goldens lock the resting screen).
pub fn settle(ui: &mut UiState) {
    for _ in 0..120 {
        ui.update();
    }
}

/// A SETTINGS leaf: its row labels and crumbs from the top, and its chain.
pub struct Leaf {
    pub labels: Vec<&'static str>,
    pub crumbs: Vec<&'static str>,
    pub chain: &'static chimera_core::ui::block_def::ChainDef2,
}

impl Leaf {
    /// `settings_<crumbs>`, lowercased: the atlas's name for its first page.
    pub fn name(&self) -> String {
        let mut n = String::from("settings");
        for c in &self.crumbs {
            n.push('_');
            n.push_str(&c.to_lowercase().replace(' ', "_"));
        }
        n
    }
}

/// Every leaf of the SETTINGS tree, depth first.
pub fn leaves() -> Vec<Leaf> {
    use chimera_core::ui::settings::{Kind, rows};
    fn walk(
        path: &mut Vec<u8>,
        labels: &mut Vec<&'static str>,
        crumbs: &mut Vec<&'static str>,
        out: &mut Vec<Leaf>,
    ) {
        for (i, r) in rows(path).iter().enumerate() {
            path.push(i as u8);
            labels.push(r.label);
            crumbs.push(r.crumb);
            match r.kind {
                Kind::Leaf(chain) => out.push(Leaf {
                    labels: labels.clone(),
                    crumbs: crumbs.clone(),
                    chain,
                }),
                Kind::List(_) => walk(path, labels, crumbs, out),
                _ => {}
            }
            path.pop();
            labels.pop();
            crumbs.pop();
        }
    }
    let mut out = Vec::new();
    walk(&mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &mut out);
    out
}

/// The def id of the page shown.
pub fn page_def_id(ui: &UiState) -> u16 {
    use chimera_core::ui::page::{PageId, PageKey};
    match ui.page() {
        PageKey::Part { def, .. } => def,
        PageKey::Legacy(PageId::System(id) | PageId::Demo(id)) => id,
    }
}

/// Out of SETTINGS, MENU, then down the tree by row label: PLUS to each
/// row, EDIT.
pub fn to_leaf(ui: &mut UiState, labels: &[&str]) {
    use chimera_core::ui::settings::rows;
    for _ in 0..8 {
        if !ui.in_settings() {
            break;
        }
        tap(ui, ButtonId::Menu);
    }
    tap(ui, ButtonId::Menu);
    let mut path = Vec::new();
    for l in labels {
        let i = rows(&path)
            .iter()
            .position(|r| r.label == *l)
            .unwrap_or_else(|| panic!("no row {l} under {path:?}"));
        plus(ui, i);
        feed(ui, Input::press(ButtonId::Edit));
        path.push(i as u8);
    }
    assert_eq!(
        ui.location(),
        chimera_core::ui::nav::Location::settings_at(&path, 0),
        "{labels:?}"
    );
}

/// The shared FX page `node` of the mixer chain, through real presses:
/// MIX+B6 (Part 6's SENDS), then PLUS past it.
pub fn to_fx(ui: &mut UiState, node: usize) {
    use chimera_core::ui::nav::{Location, MixPage};
    assert!(node > reg::MIXER_HOME, "{node} is not an FX node");
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::B6));
    // The mixer reopens on the FX page last left: MINUS back to SENDS.
    let sends = Location::mixer(chimera_core::project::PartId::ALL[5], MixPage::Sends);
    for _ in 0..reg::MIXER_CHANNEL_CHAIN.len() {
        if ui.location() == sends {
            break;
        }
        feed(ui, Input::press(ButtonId::Minus));
    }
    assert_eq!(ui.location(), sends);
    plus(ui, node - reg::MIXER_HOME);
    let def = reg::MIXER_CHANNEL_CHAIN.blocks[node].def;
    assert_eq!(
        ui.page(),
        chimera_core::ui::page::PageKey::Part {
            def: def.id,
            op: chimera_core::addr::Op::A
        },
        "{}",
        def.name
    );
}

/// SETTINGS › SYSTEM › DEMO, then PLUS × `node` (debug builds: a release
/// build has no DEMO row).
pub fn to_demo(ui: &mut UiState, node: usize) {
    to_leaf(ui, &["SYSTEM", "DEMO"]);
    plus(ui, node);
}

/// Live output the goldens draw: two periods of a lopsided triangle, peak 0.5.
pub fn scope_fixture() -> [f32; SCOPE_LEN] {
    core::array::from_fn(|i| {
        let p = (i as f32 / 120.0) % 1.0;
        let s = 0.7;
        0.5 * if p < s {
            -1.0 + 2.0 * p / s
        } else {
            1.0 - 2.0 * (p - s) / (1.0 - s)
        }
    })
}

/// Load `engine`'s init Sound into Part 1 through the sound browser (EDIT + B1,
/// scroll to the init row, EDIT), then MINUS from the engine's home to its
/// first node.
pub fn load_init(ui: &mut UiState, engine: EngineType) {
    let row = POOL_SIZE + EngineType::ALL.iter().position(|&c| c == engine).unwrap();
    feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    feed(ui, Input::turn(EncoderId::A, row as i8));
    feed(ui, Input::press(ButtonId::Edit));
    let home = chimera_core::ui::nav::home(engine);
    assert_eq!(
        ui.location(),
        chimera_core::ui::nav::Location::pages(chimera_core::project::PartId::ALL[0], home),
        "a load lands on the engine's home"
    );
    for _ in 0..home.node {
        feed(ui, Input::press(ButtonId::Minus));
    }
}

/// The OSC node's index on the Algo chain.
pub fn osc_node() -> usize {
    use chimera_core::ui::block_registry::{ALGO_CHAIN, ALGO_WAVE};
    ALGO_CHAIN
        .blocks
        .iter()
        .position(|b| b.def.id == ALGO_WAVE.id)
        .expect("OSC is on the Algo chain")
}

/// From Part 1's home (ALGO), PLUS to the OSC node.
pub fn to_osc(ui: &mut UiState) {
    plus(ui, osc_node());
}

/// From Part 1's home, to OSC, then EDIT down to the LEVEL sub-page.
pub fn to_level_page(ui: &mut UiState) {
    use chimera_core::ui::block_registry::{ALGO_CHAIN, ALGO_LEVEL};
    to_osc(ui);
    let subs = ALGO_CHAIN.blocks[osc_node()].sub_pages;
    let n = subs
        .iter()
        .position(|d| d.id == ALGO_LEVEL.id)
        .expect("LEVEL is an OSC sub-page")
        + 1;
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Edit));
    }
}

/// From Part 1's first node, on to the engine's node and EDIT down its
/// sub-list to PIT.
pub fn to_pitch(ui: &mut UiState, engine: EngineType) {
    use chimera_core::ui::block_registry::{ALGO_CHAIN, MODAL_PLUCK_CHAIN, PITCH};
    let chain = match engine {
        EngineType::Algo => &ALGO_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
    };
    let node = chain.engine_node();
    plus(ui, node);
    let n = chain.blocks[node]
        .sub_pages
        .iter()
        .position(|d| d.id == PITCH.id)
        .unwrap()
        + 1;
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Edit));
    }
    assert_eq!(
        ui.page(),
        chimera_core::ui::page::PageKey::Part {
            def: PITCH.id,
            op: chimera_core::addr::Op::A
        },
        "{engine:?}"
    );
}

fn plus(ui: &mut UiState, n: usize) {
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// At the MOD node: its home is the matrix.
fn to_matrix(ui: &mut UiState) {
    assert_eq!(
        ui.page(),
        chimera_core::ui::page::PageKey::Part {
            def: chimera_core::ui::block_registry::MOD_MATRIX.id,
            op: chimera_core::addr::Op::A
        }
    );
}

/// From Part 1's home to the MOD node (MTX), then EDIT ×`n` down its sub-list
/// to `def`.
pub fn to_mod_sub(ui: &mut UiState, n: usize, def: &chimera_core::ui::block_def::BlockDef) {
    plus(ui, 5);
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Edit));
    }
    assert_eq!(
        ui.page(),
        chimera_core::ui::page::PageKey::Part {
            def: def.id,
            op: chimera_core::addr::Op::A
        },
        "{}",
        def.name
    );
}

/// From Part 1's home to E3: type B, ENV · AD by default.
fn to_e3(ui: &mut UiState) {
    to_mod_sub(ui, 3, &reg::ENV_3);
}

/// ENV1→CUTOFF +20, ENV1→FOLD −30, LFO1→CUTOFF +42 (selected).
fn mod_matrix(ui: &mut UiState) {
    plus(ui, 3);
    feed(ui, Input::turn(EncoderId::B, 1)); // focus CUTOFF
    prime(ui);
    plus(ui, 1);
    feed(ui, Input::turn(EncoderId::A, 1)); // focus FOLD
    prime(ui);
    plus(ui, 1);
    to_matrix(ui);
    feed(ui, Input::turn(EncoderId::E, 20)); // ENV → CUTOFF
    feed(ui, Input::turn(EncoderId::B, 1));
    feed(ui, Input::turn(EncoderId::E, -30)); // ENV → FOLD
    feed(ui, Input::turn(EncoderId::A, 3)); // LFO1
    feed(ui, Input::turn(EncoderId::B, -1));
    feed(ui, Input::turn(EncoderId::E, 42)); // LFO → CUTOFF, selected
}

/// Prime the focused slot for modulation (MIX + PLUS).
fn prime(ui: &mut UiState) {
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
}

/// A named screen-golden case: a case name paired with the input sequence
/// that produces it.
pub type ScreenCase = (&'static str, fn(&mut UiState));

/// Every screen the goldens lock, one or more per page type (spec § Testing).
/// MST's place on the Mix chain: after TAPE only with `master-tape`
/// (ADR 0055). The mixer opens on SENDS, node 1 (ADR 0057); the FX come
/// after Part 6's SENDS.
const MST: usize = if cfg!(feature = "master-tape") { 6 } else { 5 };
const DLY: usize = 3;
const REV: usize = 4;
#[cfg(feature = "master-tape")]
const TAPE: usize = 5;

pub const CASES: &[ScreenCase] = &[
    ("engine_algo", |ui| feed(ui, Input::turn(EncoderId::A, 2))),
    ("algo_alg", |ui| {
        feed(ui, Input::turn(EncoderId::B, 16)); // ALG B = A17, from INIT's A1
        feed(ui, Input::turn(EncoderId::C, 50)); // MORPH 50: the diagrams blend
    }),
    ("algo_wave", |ui| {
        to_osc(ui);
        feed(ui, Input::turn(EncoderId::B, 5)); // operator 2's wave, a few steps in
    }),
    ("algo_level", |ui| {
        to_level_page(ui);
        feed(ui, Input::turn(EncoderId::B, 60)); // operator 2 LEVEL
    }),
    ("algo_osc_last", |ui| {
        to_osc(ui);
        for _ in 0..12 {
            feed(ui, Input::press(ButtonId::Edit)); // FEEDBACK, the last sub-page
        }
        feed(ui, Input::turn(EncoderId::D, 3));
    }),
    ("bigviz_filter", |ui| {
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::C, 80)); // resonance
        feed(ui, Input::turn(EncoderId::B, -60)); // cutoff, focused
    }),
    ("flt_mode", |ui| {
        plus(ui, 3);
        feed(ui, Input::press(ButtonId::Edit)); // FLT › MODE
        feed(ui, Input::turn(EncoderId::A, 3)); // MODE: BP12
    }),
    ("env_a", |ui| {
        to_mod_sub(ui, 1, &reg::ENVELOPE);
        feed(ui, Input::turn(EncoderId::B, 6));
    }),
    ("env_b_env_ad", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::B, 10)); // RISE
    }),
    ("env_b_env_ahr", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::E, 1));
    }),
    ("env_b_env_cycle", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::E, 2));
    }),
    ("env_b_lfo_free", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    ("env_b_lfo_sync", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::E, 1));
    }),
    ("env_b_lfo_lfv", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::E, 2));
    }),
    ("env_b_burst_ad", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::A, 2));
    }),
    ("env_b_burst_ahr", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::A, 2));
        feed(ui, Input::turn(EncoderId::E, 1));
    }),
    ("env_b_burst_cycle", |ui| {
        to_e3(ui);
        feed(ui, Input::turn(EncoderId::A, 2));
        feed(ui, Input::turn(EncoderId::E, 2));
    }),
    ("spd", |ui| {
        to_mod_sub(ui, 4, &reg::ENV_SPEED);
        feed(ui, Input::turn(EncoderId::B, -1)); // E2 SPEED → FAST
    }),
    ("lfo_classic", |ui| {
        to_mod_sub(ui, 5, &reg::LFO);
        feed(ui, Input::turn(EncoderId::A, 5)); // RATE
    }),
    ("lfo_func", |ui| {
        to_mod_sub(ui, 5, &reg::LFO);
        feed(ui, Input::turn(EncoderId::F, 1)); // TYPE → FUNC
    }),
    ("amp_vel_dimmed", |ui| {
        plus(ui, 4);
        feed(ui, Input::turn(EncoderId::A, 20)); // FOLD
        feed(ui, Input::turn(EncoderId::D, 1)); // VEL focused, dimmed
    }),
    ("amp_vel_live", |ui| {
        plus(ui, 4);
        feed(ui, Input::turn(EncoderId::D, 1));
        prime(ui); // VEL primes the VCA
        plus(ui, 1);
        to_matrix(ui); // ENV1 → FLT CUTOFF
        feed(ui, Input::turn(EncoderId::A, 1)); // ENV2
        feed(ui, Input::turn(EncoderId::B, 1)); // OUT VCA
        feed(ui, Input::turn(EncoderId::E, 100)); // E2 → VCA
        feed(ui, Input::press(ButtonId::Minus)); // back to AMP: VEL live
    }),
    ("algo_pitch", |ui| {
        to_pitch(ui, EngineType::Algo);
        feed(ui, Input::turn(EncoderId::A, 7)); // PITCH +7
        feed(ui, Input::turn(EncoderId::B, -25)); // FINE -25, focused
    }),
    ("modal_pitch", |ui| {
        load_init(ui, EngineType::Modal);
        to_pitch(ui, EngineType::Modal);
        feed(ui, Input::turn(EncoderId::A, -12)); // PITCH -12, focused
    }),
    ("modal_exc", |ui| load_init(ui, EngineType::Modal)), // PLUCK: EXCITE, COLOR
    ("modal_exc_bank", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 1); // RES
        feed(ui, Input::turn(EncoderId::A, 1)); // MODEL → BANK
        feed(ui, Input::press(ButtonId::Minus)); // STRIKE: EXCITE, BURST
    }),
    ("modal_exc_bowed", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 1); // RES
        feed(ui, Input::turn(EncoderId::A, 2)); // MODEL → BOWED
        feed(ui, Input::press(ButtonId::Minus)); // BOW: FORCE, SPEED
    }),
    ("modal_home", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 1); // EXC · RES
    }),
    ("modal_mdl2_symp", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 1); // RES
        feed(ui, Input::turn(EncoderId::A, 3)); // MODEL → SYMP
        feed(ui, Input::press(ButtonId::Edit)); // MDL2
    }),
    ("modal_home_bowed", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 1); // RES
        feed(ui, Input::turn(EncoderId::A, 2)); // MODEL → BOWED: STRUCT dims
    }),
    ("modal_amp", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 3); // EXC · RES · FLT · AMP
    }),
    ("mixer_part", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::press(ButtonId::Minus)); // SENDS → PART
        feed(ui, Input::turn(EncoderId::D, -8));
    }),
    ("mixer_sends", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::C, 40));
    }),
    ("mixer_fx_delay", |ui| to_fx(ui, DLY)),
    ("mixer_fx_reverb", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 20)); // GRIT
    }),
    ("mixer_fx_delay_char", |ui| {
        to_fx(ui, DLY);
        feed(ui, Input::press(ButtonId::Edit)); // DLY › CHAR
        feed(ui, Input::turn(EncoderId::A, 20)); // WOW
    }),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", |ui| {
        to_fx(ui, TAPE);
        feed(ui, Input::turn(EncoderId::A, 40)); // DRIVE
    }),
    ("mixer_master", |ui| {
        to_fx(ui, MST);
        feed(ui, Input::turn(EncoderId::B, 4)); // RATIO 4:1: the curve bends
    }),
    ("mixer_master_level", |ui| {
        to_fx(ui, MST);
        feed(ui, Input::press(ButtonId::Edit)); // MST › LEVEL
    }),
    // Part 1 on P2: the OUT warning on its mixer (ADR 0057).
    ("mixer_out_p2", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::press(ButtonId::Minus)); // → PART
        feed(ui, Input::turn(EncoderId::C, 1)); // OUT P2
        feed(ui, Input::press(ButtonId::Plus)); // → SENDS
    }),
    // Part 1 on P3: the warning on its sound pages; ALGORITHM falls back to ALG.
    ("algo_out_p3", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::press(ButtonId::Minus));
        feed(ui, Input::turn(EncoderId::C, 2)); // OUT P3
        feed(ui, Input::press(ButtonId::B1)); // back to the sound pages
    }),
    ("mod_matrix", mod_matrix),
    ("mod_matrix_wide", |ui| {
        mod_matrix(ui);
        // DRV's three knobs, FLT's C, and B and C where FOLD is: eight
        // columns.
        for _ in 0..3 {
            feed(ui, Input::press(ButtonId::Minus)); // → DRV
        }
        let prime_all = |ui: &mut UiState, encs: &[EncoderId]| {
            for &enc in encs {
                feed(ui, Input::turn(enc, 1));
                prime(ui);
            }
        };
        prime_all(ui, &[EncoderId::A, EncoderId::B, EncoderId::C]);
        plus(ui, 1);
        prime_all(ui, &[EncoderId::C]);
        plus(ui, 1);
        prime_all(ui, &[EncoderId::B, EncoderId::C]);
        plus(ui, 1);
        to_matrix(ui);
        feed(ui, Input::turn(EncoderId::B, 5)); // col 5: scrolled one, `<` and `>`
        feed(ui, Input::turn(EncoderId::A, 3)); // LFO1 → VELO
        feed(ui, Input::turn(EncoderId::E, -127));
    }),
    ("algo_alg_morph_dimmed", |ui| {
        feed(ui, Input::turn(EncoderId::B, -8)); // ALG B = T1 = ALG A
        feed(ui, Input::turn(EncoderId::C, 1)); // MORPH focused, dimmed
    }),
    // ENV1 → MORPH primed while it was live; then ALG B = ALG A (#188).
    ("mod_matrix_morph_inert", |ui| {
        feed(ui, Input::turn(EncoderId::C, 1));
        prime(ui);
        plus(ui, 5);
        to_matrix(ui);
        feed(ui, Input::turn(EncoderId::B, 1)); // past FLT CUTOFF to MORPH
        feed(ui, Input::turn(EncoderId::E, 60));
        for _ in 0..5 {
            feed(ui, Input::press(ButtonId::Minus));
        }
        feed(ui, Input::turn(EncoderId::B, -8));
        plus(ui, 5);
    }),
    ("sound_browser", |ui| {
        let mut s = Sound::init(EngineType::Algo);
        s.name = chimera_core::name::Name::new("WARM BASS").unwrap();
        ui.project_mut().pool_store(SlotId::ALL[0], s);
        let mut s = Sound::init(EngineType::Modal);
        s.name = chimera_core::name::Name::new("GLASS PLUCK").unwrap();
        ui.project_mut().pool_store(SlotId::ALL[1], s);
        feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    ("settings_personal_theme", |ui| {
        to_leaf(ui, &["PERSONALIZE", "THEME"]);
        feed(ui, Input::turn(EncoderId::C, 1)); // ACCENT AMBER, focused
    }),
    ("settings_system_about_audio", |ui| {
        to_leaf(ui, &["SYSTEM", "ABOUT"]);
        feed(ui, Input::press(ButtonId::Edit));
    }),
];

/// `AudioStats` fixture for the AUDIO sub-page's goldens and tests.
pub fn audio_fixture() -> AudioStats {
    let mut s = AudioStats::new(SiliconRev::V, 480_000_000, ResetCause::Watchdog);
    s.load_avg = 23;
    s.load_peak = 41;
    s.overruns = 2;
    s.desyncs = 1;
    s.drops = [0, 3];
    s.sources = 2;
    s.stack_used = 12_000;
    s
}

/// Build case `name`'s screen: a fresh UiState, the case's input, settled lerps.
pub fn ui_for(name: &str) -> UiState {
    let (_, setup) = CASES
        .iter()
        .find(|c| c.0 == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    let mut ui = UiState::new();
    setup(&mut ui);
    settle(&mut ui);
    ui
}

/// Overlays the goldens lock, each drawn alone on a blank screen: on the
/// device it lands on whatever the last frame left.
pub const OVERLAYS: &[(&str, Overlay)] = &[
    ("busy", Overlay::Busy),
    ("toast_saved", Overlay::Toast("SAVED")),
    ("toast_exfat", Overlay::Toast("CARD IS EXFAT: FORMAT FAT32")),
];

/// The two faces of the one overlay component.
#[derive(Clone, Copy, Debug)]
pub enum Overlay {
    Busy,
    Toast(&'static str),
}

/// Every case name: the screens, then the overlays.
pub fn case_names() -> impl Iterator<Item = &'static str> {
    CASES
        .iter()
        .map(|c| c.0)
        .chain(OVERLAYS.iter().map(|o| o.0))
}

/// Overlay `o` on a blank screen, and the band it reports.
pub fn render_overlay(o: Overlay) -> (Fb, (u16, u16)) {
    let mut fb = Fb::new();
    let band = match o {
        Overlay::Busy => draw_busy(&mut fb),
        Overlay::Toast(text) => draw_toast(&mut fb, text),
    };
    (fb, band)
}

/// Full render of case `name`.
pub fn render(name: &str) -> Fb {
    if let Some(&(_, label)) = OVERLAYS.iter().find(|o| o.0 == name) {
        let (fb, _) = render_overlay(label);
        fb.dump(name);
        return fb;
    }
    let fb = render_ui(&ui_for(name));
    fb.dump(name);
    fb
}

/// Full render of `ui` with the goldens' audio and scope fixtures.
pub fn render_ui(ui: &UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_audio(
        &mut fb,
        &PerfStats::zero(),
        Some(&audio_fixture()),
        &scope_fixture(),
    );
    fb
}

/// Render case `name` through `render_dirty` from a fresh region set.
pub fn render_dirty(name: &str) -> Fb {
    let mut ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_dirty_with_audio(
        &mut fb,
        &PerfStats::zero(),
        Some(&audio_fixture()),
        &scope_fixture(),
    );
    fb
}
