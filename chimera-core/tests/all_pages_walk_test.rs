//! All-pages walk: every page the UI can show, driven hard, rendered both
//! ways. On every frame the dirty-region render must equal a full render,
//! and neither may draw outside 240×320.
//!
//! Each page is visited through the real controls (chain buttons, PLUS,
//! EDIT for sub-pages), its focused slot primed for modulation (MIX + PLUS,
//! so the matrix fills up), every encoder turned to both extremes, and the
//! header's audio load shown at 0, 65 and 85 %; each step renders several
//! frames while the lerps run and the live output moves.
//!
//! The exhaustive walk is slow in a debug build, so it is ignored by default:
//!
//!     cargo test -p chimera-core --test all_pages_walk_test -- --ignored --nocapture
//!
//! `representative_pages_walk` runs a trimmed subset on every `just check`.

mod screen;

use chimera_core::params::EngineType;
use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::ChainDef2;
use chimera_core::ui::block_registry::{DEMO_CHAIN, MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART};
use chimera_core::ui::nav::chain_def_for;
use chimera_core::ui::perf::PerfStats;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const ENCODERS: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];
const B: [ButtonId; 6] = [
    ButtonId::B1,
    ButtonId::B2,
    ButtonId::B3,
    ButtonId::B4,
    ButtonId::B5,
    ButtonId::B6,
];

/// Which pages a walk visits.
#[derive(Clone, Copy, Debug)]
enum Context {
    /// Part 1 with `EngineType`'s init Sound loaded.
    Part(EngineType),
    /// MIX + B<n> (0-based): its PART and SENDS.
    Mixer(usize),
    /// The shared FX, after Part 6's SENDS.
    Fx,
    /// SETTINGS › SYSTEM › DEMO (debug builds).
    Demo,
    /// Every other SETTINGS leaf, by its index in `leaves()`.
    Leaf(usize),
}

impl Context {
    fn chain(self) -> &'static ChainDef2 {
        match self {
            Context::Part(e) => chain_def_for(e),
            Context::Mixer(_) | Context::Fx => &MIXER_CHANNEL_CHAIN,
            Context::Demo => &DEMO_CHAIN,
            Context::Leaf(i) => leaves()[i].of,
        }
    }

    /// The nodes this context owns on its chain.
    fn owns(self, node: usize) -> bool {
        match self {
            Context::Mixer(_) => node == MIXER_PART || node == MIXER_HOME,
            Context::Fx => node > MIXER_HOME,
            _ => true,
        }
    }

    /// Drive the UI to `node`, `sub` through the real controls.
    fn reach(self, ui: &mut UiState, node: usize, sub: usize) {
        match self {
            Context::Part(_) => {
                feed(ui, Input::press(ButtonId::B2));
                feed(ui, Input::press(ButtonId::B1));
                for _ in 0..self.chain().len() {
                    feed(ui, Input::press(ButtonId::Minus));
                }
                plus(ui, node);
            }
            Context::Mixer(n) => {
                // From outside its last place, the mixer opens on SENDS.
                tap(ui, ButtonId::Menu);
                feed(ui, Input::chord(ButtonId::Mix, B[n]));
                if node == MIXER_PART {
                    feed(ui, Input::press(ButtonId::Minus));
                }
            }
            Context::Fx => to_fx(ui, node),
            Context::Demo => to_demo(ui, node),
            Context::Leaf(i) => {
                to_leaf(ui, &leaves()[i].labels);
                plus(ui, node);
            }
        }
        for _ in 0..sub {
            feed(ui, Input::press(ButtonId::Edit));
        }
        let def = self.chain().active_def(node, sub).unwrap();
        assert_eq!(page_def_id(ui), def.id, "{self:?} reached {}", def.name);
    }

    fn start(self) -> UiState {
        let mut ui = UiState::new();
        if let Context::Part(engine) = self {
            load_init(&mut ui, engine);
        }
        ui
    }
}

fn plus(ui: &mut UiState, n: usize) {
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// One UiState under test, a framebuffer only ever updated by
/// `render_dirty`, and counters.
struct Walk {
    ui: UiState,
    dirty: Fb,
    frames: usize,
    tick: usize,
    load_pct: u8,
    frames_per_step: usize,
}

impl Walk {
    fn new(ctx: Context, frames_per_step: usize) -> Self {
        Self {
            ui: ctx.start(),
            dirty: Fb::new(),
            frames: 0,
            tick: 0,
            load_pct: 0,
            frames_per_step,
        }
    }

    /// The fixture's waveform, shifted each frame so the live output moves.
    fn scope(&self) -> [f32; SCOPE_LEN] {
        let base = scope_fixture();
        core::array::from_fn(|i| base[(i + self.tick * 7) % SCOPE_LEN])
    }

    /// Several frames: advance the lerps, render dirty and full, compare.
    fn frames(&mut self, what: &str) {
        for _ in 0..self.frames_per_step {
            self.ui.update();
            let scope = self.scope();
            let perf = PerfStats {
                audio_load_pct: self.load_pct,
                ..PerfStats::zero()
            };
            self.ui
                .render_dirty_with_scope(&mut self.dirty, &perf, &scope);
            let mut full = Fb::new();
            self.ui.render_with_scope(&mut full, &perf, &scope);
            let page = format!("{:?}", self.ui.location());
            assert_eq!(full.oob, 0, "{what} on {page}: full render drew off screen");
            assert_eq!(
                self.dirty.oob, 0,
                "{what} on {page}: dirty render drew off screen"
            );
            if self.dirty.px != full.px {
                let y = (0..H)
                    .find(|&y| self.dirty.px[y * W..(y + 1) * W] != full.px[y * W..(y + 1) * W])
                    .unwrap();
                panic!(
                    "{what} on {page} (frame {}): dirty render != full render, first at row {y}",
                    self.frames
                );
            }
            self.frames += 1;
            self.tick += 1;
        }
    }

    fn step(&mut self, input: Input, what: &str) {
        feed(&mut self.ui, input);
        self.frames(what);
    }

    /// Drive the current page: prime it, every encoder in `encoders` to both
    /// extremes, then the three audio loads.
    fn exercise(&mut self, encoders: &[EncoderId]) {
        self.frames("arrive");
        self.step(Input::chord(ButtonId::Mix, ButtonId::Plus), "prime");
        for &e in encoders {
            self.step(Input::turn(e, 127), "encoder max");
            self.step(Input::turn(e, -127), "encoder toward min");
            self.step(Input::turn(e, -127), "encoder min");
        }
        for pct in [0, 65, 85] {
            self.load_pct = pct;
            self.frames("audio load");
        }
        self.load_pct = 0;
    }
}

/// Visit every node and sub-page `ctx` owns (or only those `keep`
/// accepts), exercising each. Returns the frames rendered.
fn walk(
    ctx: Context,
    frames_per_step: usize,
    encoders: &[EncoderId],
    keep: impl Fn(usize, usize) -> bool,
) -> usize {
    let mut w = Walk::new(ctx, frames_per_step);
    for (node, block) in ctx.chain().blocks.iter().enumerate() {
        for sub in 0..block.sub_page_count().max(1) {
            if !ctx.owns(node) || !keep(node, sub) {
                continue;
            }
            ctx.reach(&mut w.ui, node, sub);
            w.exercise(encoders);
        }
    }
    w.frames
}

/// Every page reached by PLUS and EDIT alone, rendered on arrival before any
/// encoder moves (#78): pages that differ only by their def must still
/// redraw. Returns the frames rendered.
fn arrive_untouched(ctx: Context, frames_per_step: usize) -> usize {
    let mut w = Walk::new(ctx, frames_per_step);
    for (node, block) in ctx.chain().blocks.iter().enumerate() {
        if !ctx.owns(node) {
            continue;
        }
        ctx.reach(&mut w.ui, node, 0);
        w.frames("arrive, untouched");
        for _ in 1..block.sub_page_count().max(1) {
            w.step(Input::press(ButtonId::Edit), "EDIT, untouched");
        }
    }
    w.frames
}

fn every_context() -> Vec<Context> {
    let mut all: Vec<Context> = EngineType::ALL
        .iter()
        .map(|&engine| Context::Part(engine))
        .collect();
    all.extend((0..6).map(Context::Mixer));
    all.push(Context::Fx);
    if cfg!(debug_assertions) {
        all.push(Context::Demo);
    }
    let leaves = leaves();
    all.extend(
        (0..leaves.len())
            .filter(|&i| !core::ptr::eq(leaves[i].of, &DEMO_CHAIN))
            .map(Context::Leaf),
    );
    all
}

#[test]
#[ignore = "slow: run with --ignored"]
fn every_page_walk() {
    let t = std::time::Instant::now();
    let mut frames = 0;
    for ctx in every_context() {
        frames += arrive_untouched(ctx, 4);
        frames += walk(ctx, 4, &ENCODERS, |_, _| true);
    }
    println!(
        "all-pages walk: {frames} frames, dirty == full, 0 off-screen writes, {:.1?}",
        t.elapsed()
    );
}

/// A trimmed walk: the Algo chain (CellGrid, BigViz filter and envelope, the matrix),
/// the Mixer on Part 1, the FX, the first DEMO page and THEME; two encoders,
/// two frames per step.
#[test]
fn representative_pages_walk() {
    let enc = [EncoderId::A, EncoderId::E];
    walk(Context::Part(EngineType::Algo), 2, &enc, |_, sub| sub <= 1);
    walk(Context::Mixer(0), 2, &enc, |_, _| true);
    walk(Context::Fx, 2, &enc, |_, sub| sub == 0);
    if cfg!(debug_assertions) {
        walk(Context::Demo, 2, &enc, |node, _| node == 0);
    }
    let theme = leaves()
        .iter()
        .position(|l| l.labels == ["PERSONALIZE", "THEME"]);
    walk(Context::Leaf(theme.unwrap()), 2, &enc, |_, _| true);
    for ctx in every_context() {
        arrive_untouched(ctx, 1);
    }
}

/// Each SETTINGS leaf redraws on arrival from another (#69): they share a
/// layout and some read no values, so only their def tells them apart.
#[test]
fn settings_leaves_redraw_on_arrival() {
    let (perf, scope, audio) = (PerfStats::zero(), scope_fixture(), audio_fixture());
    let mut ui = UiState::new();
    let mut fb = Fb::new();
    for leaf in leaves() {
        to_leaf(&mut ui, &leaf.labels);
        settle(&mut ui);
        ui.render_dirty_with_audio(&mut fb, &perf, Some(&audio), &scope);
        let mut full = Fb::new();
        ui.render_with_audio(&mut full, &perf, Some(&audio), &scope);
        assert!(fb.px == full.px, "{}", leaf.name());
    }
}
