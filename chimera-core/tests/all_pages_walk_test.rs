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

use chimera_core::preset::ChainType;
use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::UiState;
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

/// Which chain a walk starts from.
#[derive(Clone, Copy, Debug)]
enum Context {
    /// Part 1 with `ChainType`'s init Sound loaded.
    Part(ChainType),
    /// MIX + B<n> (0-based).
    Mixer(usize),
    /// MIX + B6.
    Demo,
    /// MENU.
    System,
}

impl Context {
    /// Press the chain's own button: enters it, or snaps home when already there.
    fn home(self, ui: &mut UiState) {
        match self {
            Context::Part(_) => feed(ui, Input::press(ButtonId::B1)),
            Context::Mixer(n) => feed(ui, Input::chord(ButtonId::Mix, B[n])),
            Context::Demo => feed(ui, Input::chord(ButtonId::Mix, ButtonId::B6)),
            Context::System => feed(ui, Input::press(ButtonId::Menu)),
        }
    }

    fn start(self) -> UiState {
        let mut ui = UiState::new();
        if let Context::Part(ct) = self {
            load_init(&mut ui, ct);
        }
        self.home(&mut ui);
        ui
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
            let page = self.ui.nav.active_block_def().name;
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

/// Visit every node and sub-page of `ctx`'s chain (or only those `keep`
/// accepts), exercising each. Returns the frames rendered.
fn walk(
    ctx: Context,
    frames_per_step: usize,
    encoders: &[EncoderId],
    keep: impl Fn(usize, usize) -> bool,
) -> usize {
    let mut w = Walk::new(ctx, frames_per_step);
    let chain = w.ui.nav.active_chain();
    for (node, block) in chain.blocks.iter().enumerate() {
        for sub in 0..block.sub_page_count().max(1) {
            if !keep(node, sub) {
                continue;
            }
            ctx.home(&mut w.ui);
            for _ in 0..node {
                feed(&mut w.ui, Input::press(ButtonId::Plus));
            }
            for _ in 0..sub {
                feed(&mut w.ui, Input::press(ButtonId::Edit));
            }
            assert_eq!(
                (w.ui.nav.node, w.ui.nav.sub_page),
                (node, sub),
                "{ctx:?} reached"
            );
            w.exercise(encoders);
        }
    }
    w.frames
}

fn every_context() -> Vec<Context> {
    let mut all: Vec<Context> = ChainType::ALL.iter().map(|&ct| Context::Part(ct)).collect();
    all.extend((0..5).map(Context::Mixer));
    all.extend([Context::Demo, Context::System]);
    all
}

#[test]
#[ignore = "slow: run with --ignored"]
fn every_page_walk() {
    let t = std::time::Instant::now();
    let mut frames = 0;
    for ctx in every_context() {
        frames += walk(ctx, 4, &ENCODERS, |_, _| true);
    }
    println!(
        "all-pages walk: {frames} frames, dirty == full, 0 off-screen writes, {:.1?}",
        t.elapsed()
    );
}

/// A trimmed walk: the FM chain (CellGrid, BigViz envelopes, the matrix),
/// the Mixer on Part 1, and the first Demo and System pages; two encoders,
/// two frames per step.
#[test]
fn representative_pages_walk() {
    let enc = [EncoderId::A, EncoderId::E];
    walk(Context::Part(ChainType::Fm), 2, &enc, |_, sub| sub <= 1);
    walk(Context::Mixer(0), 2, &enc, |_, _| true);
    walk(Context::Demo, 2, &enc, |node, _| node == 0);
    walk(Context::System, 2, &enc, |node, _| node == 0);
}
