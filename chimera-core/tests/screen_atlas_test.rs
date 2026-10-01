//! Screen atlas: every screen the UI can show, found from the chain
//! registry and reached through the real controls. Nothing is pinned: it
//! is a smoke test (every screen renders, on screen) that also writes
//! `docs/screens` through `just screens` (`SCREEN_DUMP`, as the goldens).
//!
//! A page a golden case already lands on keeps the golden's name and
//! picture; the atlas adds the rest, named `<chain>_<map label>`:
//! `algo_crs`, `modal_bank_exc` (Modal pages that follow MODEL carry it),
//! `algo_pit_cut` (a choice that changes the page, at its other values).

mod screen;

use std::collections::HashSet;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::filter::KIND_NAMES;
use chimera_core::dsp::modal::{MODEL_NAMES, ModalParams};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot, PitchParams, STEAL_NAMES};
use chimera_core::project::{Differ, PartId, ProjectNote, SlotId, Subject};
use chimera_core::storage::{FileError, ProjectId};
use chimera_core::ui::block_def::{BlockDef, SlotBinding};
use chimera_core::ui::busy::{ToastStep, draw_toast};
use chimera_core::ui::chain::ChainId;
use chimera_core::ui::{UiMode, UiState, splash};
use chimera_hal::store::{StoreError, VolumeId};
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

/// A choice whose value changes what its page shows.
struct Choice {
    addr: ParamAddr,
    names: &'static [&'static str],
    now: fn(&ParamSnapshot) -> usize,
}

const MODEL: Choice = Choice {
    addr: ParamAddr::new(BlockRef::Modal, ModalParams::MODE),
    names: &MODEL_NAMES,
    now: |p| p.modal.mode as usize,
};

/// Shown on their page at every value.
const CHOICES: [Choice; 2] = [
    Choice {
        addr: ParamAddr::new(BlockRef::Filter, FilterParams::KIND),
        names: &KIND_NAMES,
        now: |p| p.filter.kind() as usize,
    },
    Choice {
        addr: ParamAddr::new(BlockRef::Pitch, PitchParams::STEAL),
        names: &STEAL_NAMES,
        now: |p| p.pitch.steal as usize,
    },
];

fn slot_of(def: &BlockDef, addr: ParamAddr) -> Option<usize> {
    def.params
        .iter()
        .position(|s| s.binding == SlotBinding::Param(addr))
}

/// A page whose cells follow the Modal MODEL.
fn follows_model(def: &BlockDef) -> bool {
    def.params.iter().any(|s| match s.binding {
        SlotBinding::ModalPanel(..) => true,
        SlotBinding::Param(a) => a.block == BlockRef::Modal,
        _ => false,
    })
}

/// Where a walk starts.
#[derive(Clone, Copy, Debug)]
enum Ctx {
    /// Part 1 on the engine's init Sound.
    Part(EngineType),
    /// MIX + B1. The other Parts' mixers differ only in the header.
    Mixer,
    /// MENU.
    System,
    /// MIX + B6.
    Demo,
}

impl Ctx {
    const ALL: [Ctx; 5] = [
        Ctx::Part(EngineType::Algo),
        Ctx::Part(EngineType::Modal),
        Ctx::Mixer,
        Ctx::System,
        Ctx::Demo,
    ];

    fn prefix(self) -> &'static str {
        match self {
            Ctx::Part(EngineType::Algo) => "algo",
            Ctx::Part(EngineType::Modal) => "modal",
            Ctx::Mixer => "mixer",
            Ctx::System => "system",
            Ctx::Demo => "demo",
        }
    }

    fn start(self) -> UiState {
        let mut ui = UiState::new();
        if let Ctx::Part(engine) = self {
            load_init(&mut ui, engine);
        }
        self.home(&mut ui);
        ui
    }

    /// The chain's first node, sub-page 0.
    fn home(self, ui: &mut UiState) {
        match self {
            Ctx::Part(_) => {
                feed(ui, Input::press(ButtonId::B2));
                feed(ui, Input::press(ButtonId::B1));
            }
            Ctx::Mixer => {
                feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
                for _ in 0..ui.nav.active_chain().len() {
                    feed(ui, Input::press(ButtonId::Minus));
                }
            }
            Ctx::System => feed(ui, Input::press(ButtonId::Menu)),
            Ctx::Demo => feed(ui, Input::chord(ButtonId::Mix, ButtonId::B6)),
        }
    }

    /// Home, then PLUS to `node` and EDIT down to `sub`.
    fn go(self, ui: &mut UiState, node: usize, sub: usize) {
        self.home(ui);
        for _ in 0..node {
            feed(ui, Input::press(ButtonId::Plus));
        }
        for _ in 0..sub {
            feed(ui, Input::press(ButtonId::Edit));
        }
        assert_eq!((ui.nav.node, ui.nav.sub_page), (node, sub), "{self:?}");
    }
}

/// Turn the focused page's slot for `c` to value `i`.
fn set(ui: &mut UiState, c: &Choice, i: usize) {
    let slot = slot_of(ui.nav.active_block_def(), c.addr).expect("on the choice's page");
    feed(ui, Input::turn(ENCODERS[slot], -127));
    feed(ui, Input::turn(ENCODERS[slot], i as i8));
    assert_eq!((c.now)(ui.params()), i, "{}", c.names[i]);
}

/// Go to the first page with `c`'s slot and turn it to `i`.
fn set_anywhere(ui: &mut UiState, ctx: Ctx, c: &Choice, i: usize) {
    let chain = ui.nav.active_chain();
    let (node, sub) = (0..chain.len())
        .flat_map(|n| (0..chain.blocks[n].sub_page_count().max(1)).map(move |s| (n, s)))
        .find(|&(n, s)| slot_of(chain.active_def(n, s).unwrap(), c.addr).is_some())
        .expect("a page holds the choice");
    ctx.go(ui, node, sub);
    set(ui, c, i);
}

struct Shot {
    name: String,
    ctx: Ctx,
    /// The Modal MODEL, for a page that follows it.
    model: Option<usize>,
    node: usize,
    sub: usize,
    /// A `CHOICES` entry at a value other than INIT's.
    choice: Option<(usize, usize)>,
}

impl Shot {
    fn ui(&self) -> UiState {
        let mut ui = self.ctx.start();
        if let Some(m) = self.model {
            set_anywhere(&mut ui, self.ctx, &MODEL, m);
        }
        self.ctx.go(&mut ui, self.node, self.sub);
        if let Some((c, i)) = self.choice {
            set(&mut ui, &CHOICES[c], i);
        }
        settle(&mut ui);
        ui
    }
}

/// Every page of every chain: Modal's model pages once per MODEL, and each
/// page with a `CHOICES` slot once more per other value.
fn atlas() -> Vec<Shot> {
    let mut shots = Vec::new();
    for ctx in Ctx::ALL {
        let ui = ctx.start();
        let init = ui.params();
        let chain = ui.nav.active_chain();
        let models = matches!(ctx, Ctx::Part(EngineType::Modal));
        for (node, block) in chain.blocks.iter().enumerate() {
            for sub in 0..block.sub_page_count().max(1) {
                let def = chain.active_def(node, sub).unwrap();
                let per_model = models && follows_model(def);
                let model_list: Vec<Option<usize>> = if per_model {
                    (0..MODEL.names.len()).map(Some).collect()
                } else {
                    vec![None]
                };
                for model in model_list {
                    let mut base = String::from(ctx.prefix());
                    if let Some(m) = model {
                        base += "_";
                        base += &MODEL.names[m].to_lowercase();
                    }
                    base += "_";
                    base += &def.short.to_lowercase();
                    let shot = |name: String, choice| Shot {
                        name,
                        ctx,
                        model,
                        node,
                        sub,
                        choice,
                    };
                    for (ci, c) in CHOICES.iter().enumerate() {
                        if slot_of(def, c.addr).is_none() {
                            continue;
                        }
                        for i in (0..c.names.len()).filter(|&i| i != (c.now)(init)) {
                            let name = format!("{base}_{}", c.names[i].to_lowercase());
                            shots.push(shot(name, Some((ci, i))));
                        }
                    }
                    shots.push(shot(base, None));
                }
            }
        }
    }
    shots
}

/// What tells two screens apart for the goldens' cover: the page, and on
/// a Part its engine and model.
#[derive(PartialEq)]
struct Key {
    chain: ChainId,
    engine: Option<EngineType>,
    model: Option<usize>,
    node: usize,
    sub: usize,
    browser: bool,
}

fn key(ui: &UiState) -> Key {
    let part = matches!(ui.nav.chain_id, ChainId::Part(_));
    let engine = part.then_some(ui.nav.engine);
    Key {
        chain: ui.nav.chain_id,
        engine,
        model: (engine == Some(EngineType::Modal)).then(|| (MODEL.now)(ui.params())),
        node: ui.nav.node,
        sub: ui.nav.sub_page,
        browser: !matches!(ui.ui_mode, UiMode::Normal),
    }
}

/// Boot and project notes as the shell shows them: through the UI's toast.
fn notes() -> Vec<(&'static str, ProjectNote)> {
    let file = Subject::File(ProjectId::new(7).unwrap());
    let vol = VolumeId {
        serial: 0x1234_5678,
        label: *b"CHIMERA    ",
    };
    vec![
        ("toast_new_project", ProjectNote::NewProject),
        (
            "toast_no_card",
            ProjectNote::Card {
                err: StoreError::NoCard,
                subject: None,
            },
        ),
        (
            "toast_card_changed",
            ProjectNote::Card {
                err: StoreError::VolumeChanged(vol),
                subject: Some(file),
            },
        ),
        (
            "toast_file_damaged",
            ProjectNote::File {
                err: FileError::BadCrc,
                subject: file,
            },
        ),
        ("toast_load_failed", ProjectNote::LoadFailed(file)),
        ("toast_not_found", ProjectNote::Missing(file)),
        (
            "toast_saved_differs",
            ProjectNote::Saved(Differ::One(PartId::ALL[0], SlotId::ALL[2])),
        ),
    ]
}

#[test]
fn every_screen_renders() {
    let covered: Vec<Key> = CASES.iter().map(|c| key(&ui_for(c.0))).collect();
    let mut names: HashSet<String> = case_names().map(String::from).collect();
    let mut shown = 0;
    for shot in atlas() {
        let ui = shot.ui();
        if shot.choice.is_none() && covered.contains(&key(&ui)) {
            continue;
        }
        assert!(names.insert(shot.name.clone()), "{} twice", shot.name);
        let fb = render_ui(&ui);
        assert_eq!(fb.oob, 0, "{} draws off screen", shot.name);
        assert!(fb.px.iter().any(|&p| p != 0), "{} is blank", shot.name);
        fb.dump(&shot.name);
        shown += 1;
    }
    assert!(shown > 0);

    let mut fb = Fb::new();
    splash::draw(&mut fb).unwrap();
    assert_eq!(fb.oob, 0);
    fb.dump("boot_splash");

    for (name, note) in notes() {
        let mut ui = UiState::new();
        ui.show_note(note);
        let ToastStep::Show(line) = ui.step_toast(0) else {
            panic!("{name}: no toast");
        };
        let mut fb = Fb::new();
        draw_toast(&mut fb, line.as_str());
        assert_eq!(fb.oob, 0, "{name}");
        assert!(names.insert(name.into()), "{name} twice");
        fb.dump(name);
    }
}
