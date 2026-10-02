//! Screen atlas: every screen the UI can show, found from the chain
//! registry and reached through the real controls. Nothing is pinned: it
//! is a smoke test (every screen renders, on screen) that also writes
//! `docs/screens` through `just screens` (`SCREEN_DUMP`, as the goldens).
//!
//! A page a golden case already lands on keeps the golden's name and
//! picture; the atlas adds the rest, named `<chain>_<map label>`:
//! `algo_crs`, `modal_bank_exc` (Modal pages that follow MODEL carry it),
//! `algo_pit_cut` (a choice that changes the page, at its other values).
//! A SETTINGS list or leaf is `settings_<crumbs joined by _>` (the top,
//! `settings_top`): `settings_personal_theme`,
//! `settings_system_diag_aud_load`, `settings_system_diag_demo_<short>`.
//! A screen in some state adds it:
//! `settings_project_load_no_card`, `settings_footer_saved`, and each
//! prompt, `settings_prompt_<kind>`.

mod screen;

use std::collections::HashSet;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::filter::KIND_NAMES;
use chimera_core::dsp::modal::{MODEL_NAMES, ModalParams};
use chimera_core::name::ProjectName;
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot, PitchParams, STEAL_NAMES};
use chimera_core::project::{Differ, PartId, ProjectFile, ProjectNote, SlotId, Subject};
use chimera_core::storage::{FileError, ProjectId};
use chimera_core::ui::block_def::{BlockDef, ChainDef2, SlotBinding};
use chimera_core::ui::block_registry::{MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART};
use chimera_core::ui::busy::{ToastStep, draw_toast};
use chimera_core::ui::nav::{Location, chain_def_for};
use chimera_core::ui::settings::manage::Command;
use chimera_core::ui::settings::part::{PartCmd, SAVE_ROWS};
use chimera_core::ui::settings::view::{Footer, draw_footer};
use chimera_core::ui::settings::{AskKind, Kind};
use chimera_core::ui::theme;
use chimera_core::ui::{UiState, splash};
use chimera_hal::store::{StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use screen::*;

const MAP_TOP: usize = chimera_core::ui::theme::MAP_TOP as usize;

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
    /// MIX + B1's PART and SENDS, then the FX past Part 6's SENDS. The
    /// other Parts' mixers differ only in the header.
    Mixer,
    /// A SETTINGS leaf, by its index in `leaves()`.
    Settings(usize),
}

impl Ctx {
    fn all() -> Vec<Ctx> {
        let mut all = vec![
            Ctx::Part(EngineType::Algo),
            Ctx::Part(EngineType::Modal),
            Ctx::Mixer,
        ];
        all.extend((0..leaves().len()).map(Ctx::Settings));
        all
    }

    fn prefix(self) -> String {
        match self {
            Ctx::Part(EngineType::Algo) => "algo".into(),
            Ctx::Part(EngineType::Modal) => "modal".into(),
            Ctx::Mixer => "mixer".into(),
            Ctx::Settings(i) => leaves()[i].name(),
        }
    }

    fn chain(self) -> &'static ChainDef2 {
        match self {
            Ctx::Part(e) => chain_def_for(e),
            Ctx::Mixer => &MIXER_CHANNEL_CHAIN,
            Ctx::Settings(i) => leaves()[i].of.chain(),
        }
    }

    /// The name of `def`: a leaf's page is the leaf.
    fn name(self, def: &BlockDef) -> String {
        let mut n = self.prefix();
        let tail = match self {
            Ctx::Settings(_) => return n,
            _ => def.short.to_lowercase(),
        };
        n += "_";
        n += &tail;
        n
    }

    fn start(self) -> UiState {
        let mut ui = UiState::new();
        if let Ctx::Part(engine) = self {
            load_init(&mut ui, engine);
        }
        ui
    }

    /// To `node` and EDIT down to `sub`, through the real controls.
    fn go(self, ui: &mut UiState, node: usize, sub: usize) {
        match self {
            Ctx::Part(_) => {
                feed(ui, Input::press(ButtonId::B2));
                feed(ui, Input::press(ButtonId::B1));
                for _ in 0..self.chain().len() {
                    feed(ui, Input::press(ButtonId::Minus));
                }
                plus(ui, node);
            }
            Ctx::Mixer if node > MIXER_HOME => to_fx(ui, node),
            Ctx::Mixer => {
                feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
                if node == MIXER_PART {
                    feed(ui, Input::press(ButtonId::Minus));
                }
            }
            Ctx::Settings(i) => to_leaf(ui, &leaves()[i].labels),
        }
        for _ in 0..sub {
            feed(ui, Input::press(ButtonId::Edit));
        }
        let def = self.chain().active_def(node, sub).unwrap();
        assert_eq!(page_def_id(ui), def.id, "{self:?} {node} {sub}");
    }
}

fn plus(ui: &mut UiState, n: usize) {
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// Turn the focused page's slot for `c` to value `i`.
fn set(ui: &mut UiState, c: &Choice, i: usize) {
    let slot = slot_of(ui.page_def(), c.addr).expect("on the choice's page");
    feed(ui, Input::turn(ENCODERS[slot], -127));
    feed(ui, Input::turn(ENCODERS[slot], i as i8));
    assert_eq!((c.now)(ui.params()), i, "{}", c.names[i]);
}

/// Go to the first page with `c`'s slot and turn it to `i`.
fn set_anywhere(ui: &mut UiState, ctx: Ctx, c: &Choice, i: usize) {
    let chain = ctx.chain();
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
    for ctx in Ctx::all() {
        let ui = ctx.start();
        let init = ui.params();
        let chain = ctx.chain();
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
                    let base = match model {
                        Some(m) => {
                            let named = ctx.name(def);
                            let tail = &named[ctx.prefix().len()..];
                            format!("{}_{}{tail}", ctx.prefix(), MODEL.names[m].to_lowercase())
                        }
                        None => ctx.name(def),
                    };
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

/// What tells two screens apart for the goldens' cover: where the UI is,
/// and the edited Part's engine and model.
#[derive(PartialEq)]
struct Key {
    at: Location,
    engine: EngineType,
    model: Option<usize>,
}

fn key(ui: &UiState) -> Key {
    let engine = ui.project().part(ui.active_part).sound.engine();
    Key {
        at: ui.location(),
        engine,
        model: (engine == EngineType::Modal).then(|| (MODEL.now)(ui.params())),
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

/// A SETTINGS screen in some state: its name and how to get there.
type Setup = Box<dyn Fn(&mut UiState)>;

const P1: PartId = PartId::ALL[0];
const P4: PartId = PartId::ALL[3];
/// SLOT 03.
const S3: SlotId = SlotId::ALL[2];

/// Part `p`'s Sound rung, the cursor on slot `s`.
fn rung(ui: &mut UiState, p: PartId, s: SlotId) {
    let b = [ButtonId::B1, ButtonId::B2, ButtonId::B3, ButtonId::B4];
    feed(ui, Input::chord(ButtonId::Edit, b[p.index()]));
    feed(ui, Input::turn(EncoderId::A, s.index() as i8));
}

/// Slot `s` into Part `p`, unedited, through the Sound rung.
fn load_slot(ui: &mut UiState, p: PartId, s: SlotId) {
    rung(ui, p, s);
    feed(ui, Input::press(ButtonId::Edit));
    assert!(!ui.prompt_open());
}

fn edit(ui: &mut UiState, p: PartId) {
    ui.project_mut().edit_part(p).sound.params.filter.cutoff *= 0.5;
}

/// SETTINGS › PART for Part 1, the bar on `row`.
fn part_row(ui: &mut UiState, row: usize) {
    to_leaf(ui, &["PART"]);
    plus(ui, row);
}

/// ALPHA, BETA, GAMMA on the card, ALPHA loaded, MANAGE open.
fn manage(ui: &mut UiState, f: impl FnOnce(&mut Rig, [ProjectFile; 3])) {
    let (s, files) = three();
    let mut r = Rig::new(ui, s);
    r.load(files[0]);
    r.to(&["PROJECT", "MANAGE PROJECTS"]);
    f(&mut r, files);
}

/// The loaded project's row in MANAGE, and another's.
const ALPHA: usize = 0;
const BETA: usize = 1;

/// MANAGE's command `c` on row `row`.
fn manage_command(ui: &mut UiState, row: usize, c: Command, modified: bool) {
    manage(ui, |r, _| {
        if modified {
            modify(r.ui);
        }
        r.bar_to(row);
        r.feed(Input::press(ButtonId::Edit));
        for _ in 0..c as usize {
            r.feed(Input::press(ButtonId::Plus));
        }
    });
}

/// Prompt `k`, opened by the keys that open it.
fn prompt(k: AskKind, ui: &mut UiState) {
    match k {
        AskKind::ReplacePart => {
            edit(ui, P1);
            part_row(ui, 1); // CLEAR
            tap(ui, ButtonId::Seq);
        }
        AskKind::ReloadPart => {
            load_slot(ui, P1, S3);
            edit(ui, P1);
            part_row(ui, 3); // RELOAD FROM PROJ
            tap(ui, ButtonId::Seq);
        }
        AskKind::UpdateStale => {
            load_slot(ui, P1, S3);
            load_slot(ui, P4, S3);
            feed(ui, Input::press(ButtonId::B1));
            edit(ui, P1);
            part_row(ui, 2); // SAVE TO PROJ
            feed(ui, Input::press(ButtonId::Edit));
            let over = SAVE_ROWS.iter().position(|&c| c == PartCmd::OverSlot);
            plus(ui, over.unwrap());
            tap(ui, ButtonId::Seq);
        }
        AskKind::ClearSlot => {
            rung(ui, P1, S3);
            feed(ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
        }
        AskKind::LoadProject => prompt_load(ui),
        AskKind::NameExists => {
            // The name SAVE AS proposes, taken.
            let mut s = MemStore::new(1);
            put(&mut s, "drift-002");
            let mut r = Rig::new(ui, s);
            r.ui.project_mut()
                .set_name(ProjectName::new("SKETCH").unwrap());
            r.to(&["PROJECT"]);
            r.bar_to(1);
            r.tap(ButtonId::Seq);
            r.tap(ButtonId::Seq);
        }
        AskKind::CardChanged => {
            let mut s = MemStore::new(1);
            let a = put(&mut s, "ALPHA");
            let mut r = Rig::new(ui, s);
            r.load(a);
            r.s.swap(2);
            r.hold(ButtonId::Menu);
        }
        AskKind::Delete => {
            manage_command(ui, BETA, Command::Delete, false);
            tap(ui, ButtonId::Seq);
        }
        AskKind::Clear => {
            manage_command(ui, BETA, Command::Clear, false);
            tap(ui, ButtonId::Seq);
        }
        AskKind::SaveOver => {
            manage_command(ui, BETA, Command::SaveTo, true);
            tap(ui, ButtonId::Seq);
        }
    }
    assert_eq!(ui.prompt_kind_for_test(), Some(k));
}

/// SETTINGS past its leaves: every list, LOAD and MANAGE, PART's screens,
/// NAMING, each prompt, and the footer in each status (NEW is
/// `settings_top`).
fn settings_shots() -> Vec<(String, Setup)> {
    let mut v: Vec<(String, Setup)> = Vec::new();
    for n in tree() {
        if let Kind::List(_) = n.of {
            let labels = n.labels.clone();
            v.push((n.name(), Box::new(move |ui| to_leaf(ui, &labels))));
        }
    }
    let states: [ScreenCase; 12] = [
        ("settings_project_load_no_card", |ui| {
            let mut s = MemStore::new(1);
            put(&mut s, "ALPHA");
            s.eject();
            Rig::new(ui, s).to(&["PROJECT", "LOAD PROJECT"]);
        }),
        ("settings_project_manage", |ui| {
            manage(ui, |r, _| r.bar_to(1))
        }),
        ("settings_project_manage_commands", |ui| {
            manage_command(ui, BETA, Command::Rename, false)
        }),
        ("settings_part_save_to", |ui| {
            load_slot(ui, P1, S3);
            edit(ui, P1);
            part_row(ui, 2);
            feed(ui, Input::press(ButtonId::Edit));
        }),
        ("settings_part_rename", |ui| {
            part_row(ui, 0);
            tap(ui, ButtonId::Seq);
            assert!(ui.naming().is_some());
        }),
        ("settings_naming", naming_save_as),
        ("settings_footer_saved", |ui| {
            let (s, [a, ..]) = three();
            Rig::new(ui, s).load(a);
            tap(ui, ButtonId::Menu);
        }),
        ("settings_project_manage_delete_loaded", |ui| {
            manage_command(ui, ALPHA, Command::Delete, false)
        }),
        ("settings_project_manage_rename", |ui| {
            manage_command(ui, ALPHA, Command::Rename, false);
            tap(ui, ButtonId::Seq);
            assert!(ui.naming().is_some());
        }),
        // SAVE AS inside a load: SAVE THEN LOAD on an edited NEW project.
        ("settings_naming_save_then_load", |ui| {
            let mut s = MemStore::new(1);
            put(&mut s, "ALPHA");
            let mut r = Rig::new(ui, s);
            modify(r.ui);
            r.to(&["PROJECT", "LOAD PROJECT"]);
            r.tap(ButtonId::Seq);
            r.tap(ButtonId::Seq);
            assert!(r.ui.naming().is_some());
        }),
        ("settings_prompt_clear_loaded", |ui| {
            manage_command(ui, ALPHA, Command::Clear, false);
            tap(ui, ButtonId::Seq);
            assert_eq!(ui.prompt_kind_for_test(), Some(AskKind::Clear));
        }),
        ("settings_footer_modified", |ui| {
            let (s, [a, ..]) = three();
            Rig::new(ui, s).load(a);
            modify(ui);
            tap(ui, ButtonId::Menu);
        }),
    ];
    for (name, go) in states {
        v.push((name.into(), Box::new(go)));
    }
    for &k in AskKind::ALL {
        let name = format!("settings_prompt_{}", k.slug());
        v.push((name, Box::new(move |ui| prompt(k, ui))));
    }
    v
}

fn settled(go: &dyn Fn(&mut UiState)) -> UiState {
    let mut ui = UiState::new();
    go(&mut ui);
    settle(&mut ui);
    ui
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

    for (name, go) in settings_shots() {
        if CASES.iter().any(|c| c.0 == name) {
            continue;
        }
        assert!(names.insert(name.clone()), "{name} twice");
        let fb = render_ui(&settled(&go));
        assert_eq!(fb.oob, 0, "{name} draws off screen");
        fb.dump(&name);
    }

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

/// Every SETTINGS frame's map band is its footer, drawn alone.
#[test]
fn atlas_settings_never_shows_the_map() {
    let leaves = atlas()
        .into_iter()
        .filter(|s| matches!(s.ctx, Ctx::Settings(_)))
        .map(|s| (s.name.clone(), s.ui()));
    let goldens = CASES
        .iter()
        .filter(|c| c.0.starts_with("settings_"))
        .map(|c| (c.0.to_string(), ui_for(c.0)));
    let rest = settings_shots()
        .into_iter()
        .map(|(n, go)| (n, settled(&go)));
    let mut seen = 0;
    for (name, ui) in leaves.chain(goldens).chain(rest) {
        if !ui.in_settings() {
            continue;
        }
        let name_up = chimera_core::ui::components::upper(ui.project().meta().name().as_str());
        let mut want = Fb::new();
        want.px.fill(RawU16::from(theme::BG).into_inner());
        draw_footer(
            &mut want,
            &Footer {
                name: name_up.as_str(),
                status: ui.project_status(),
                legend: ui.legend_for_test().unwrap(),
            },
        );
        let band = MAP_TOP * W..H * W;
        let got = render_ui(&ui);
        let rows: Vec<usize> = (MAP_TOP..H)
            .filter(|y| got.px[y * W..(y + 1) * W] != want.px[y * W..(y + 1) * W])
            .collect();
        assert!(
            got.px[band.clone()] == want.px[band],
            "{name}: rows {rows:?}"
        );
        seen += 1;
    }
    assert!(seen > 30, "{seen}");
}
