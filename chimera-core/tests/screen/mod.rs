//! Screen test harness (UI refresh spec § Testing): an in-memory 240×320
//! RGB565 display, the named screens the goldens lock, and a hash.
//!
//! `SCREEN_DUMP=<dir>` writes every rendered case to `<dir>/<case>.ppm`
//! for eyeballing against the mockups (`magick x.ppm x.png`).

#![allow(dead_code)]

use chimera_core::clock_plan::SiliconRev;
use chimera_core::dsp::filter::{FilterKind, FilterMode};
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::dsp::tape_engine::wave::frame_len;
use chimera_core::dsp::tape_engine::{Direction, Heads, SliceCode, TapeMachine, Way, fade_default};
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument, PartHeads};
use chimera_core::name::ProjectName;
use chimera_core::orbit::engine::OrbitEvent;
use chimera_core::orbit::runner::{OrbitRunner, OrbitView, orbit_buffer};
use chimera_core::params::EngineType;
use chimera_core::perf::load::AudioStats;
use chimera_core::preset::Sound;
use chimera_core::project::SlotId;
use chimera_core::project::{
    LoadLink, PartFrom, PartId, PartSource, Project, ProjectFile, ProjectNote, ProjectSource,
    ReplaceGuard, SaveTo, save_project,
};
use chimera_core::reset::ResetCause;
use chimera_core::scope::SCOPE_LEN;
use chimera_core::storage::twelve::pack12;
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::storage::{SourceRef, TapeSource};
use chimera_core::triple::{Reader, TripleBuffer, Writer};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::browser::{HEAD, INIT_TYPES};
use chimera_core::ui::busy::{draw_busy, draw_toast};
use chimera_core::ui::hold::HOLD_MS;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::settings::CardCx;
use chimera_hal::testkit::MemStore;
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
    /// `b` released inside this frame, at `ms`: the release a stalled
    /// frame latched.
    pub fn released_at(b: ButtonId, ms: u32) -> Self {
        Self {
            at_ms: ms,
            edges: vec![(
                b,
                Edges {
                    down: false,
                    pressed_at: None,
                    released_at: Some(Ms(ms)),
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
    /// This frame with `e` turned `delta` too.
    pub fn and_turn(mut self, e: EncoderId, delta: i8) -> Self {
        self.encoders.push((e, delta));
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

/// On the Sound rung, the cursor turned to list entry `entry`.
pub fn to_row(ui: &mut UiState, entry: usize) {
    let at = ui
        .location()
        .browse()
        .expect("on the Sound rung")
        .1
        .cursor();
    feed(ui, Input::turn(EncoderId::A, entry as i8 - at as i8));
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
        ui.update(UiTick::for_test());
    }
}

/// A SETTINGS list or leaf: its row labels and crumbs from the top.
pub struct Node<T> {
    pub labels: Vec<&'static str>,
    pub crumbs: Vec<&'static str>,
    pub of: T,
}

/// A leaf, and its page.
pub type Leaf = Node<&'static chimera_core::ui::settings::leaves::OnePage>;

impl<T> Node<T> {
    /// `settings_<crumbs>`, lowercased; the top list is `settings_top`.
    pub fn name(&self) -> String {
        if self.crumbs.is_empty() {
            return "settings_top".into();
        }
        let mut n = String::from("settings");
        for c in &self.crumbs {
            n.push('_');
            n.push_str(&c.to_lowercase().replace(' ', "_"));
        }
        n
    }
}

/// Every list and leaf of the SETTINGS tree, depth first from `ROOT`.
pub fn tree() -> Vec<Node<chimera_core::ui::settings::Kind>> {
    use chimera_core::ui::settings::{Kind, ROOT, rows};
    fn walk(
        path: &mut Vec<u8>,
        labels: &mut Vec<&'static str>,
        crumbs: &mut Vec<&'static str>,
        out: &mut Vec<Node<Kind>>,
    ) {
        for (i, r) in rows(path).iter().enumerate() {
            path.push(i as u8);
            labels.push(r.label);
            crumbs.push(r.crumb);
            if matches!(r.kind, Kind::Leaf(_) | Kind::List(_)) {
                out.push(Node {
                    labels: labels.clone(),
                    crumbs: crumbs.clone(),
                    of: r.kind,
                });
            }
            walk(path, labels, crumbs, out);
            path.pop();
            labels.pop();
            crumbs.pop();
        }
    }
    let mut out = vec![Node {
        labels: vec![],
        crumbs: vec![],
        of: ROOT.kind,
    }];
    walk(&mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &mut out);
    out
}

/// Every leaf of the SETTINGS tree, depth first.
pub fn leaves() -> Vec<Leaf> {
    use chimera_core::ui::settings::Kind;
    tree()
        .into_iter()
        .filter_map(|n| match n.of {
            Kind::Leaf(chain) => Some(Node {
                labels: n.labels,
                crumbs: n.crumbs,
                of: chain,
            }),
            _ => None,
        })
        .collect()
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

/// A UI with a card in the slot: `MemStore` behind the shell's
/// `card_work`, run after every frame of input.
pub struct Rig<'u> {
    pub ui: &'u mut UiState,
    pub s: MemStore,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl<'u> Rig<'u> {
    pub fn new(ui: &'u mut UiState, mut s: MemStore) -> Self {
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        Rig {
            ui,
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
        }
    }

    pub fn work(&mut self) {
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.s,
            sync: &mut self.sync,
            settings: &mut self.set,
        };
        let _ = self
            .ui
            .card_work(chimera_core::errand::Errand::HOST, cx, link, |swap, _| {
                let _ = swap.settle(link, || false);
            });
    }

    /// The load epoch: what a snapshot published now carries.
    pub fn epoch(&self) -> u32 {
        self.link.epoch()
    }

    pub fn tap(&mut self, b: ButtonId) {
        tap(self.ui, b);
        self.work();
    }

    pub fn hold(&mut self, b: ButtonId) {
        hold(self.ui, b);
        self.work();
    }

    pub fn feed(&mut self, i: Input) {
        feed(self.ui, i);
        self.work();
    }

    /// Out of SETTINGS, MENU, then down the tree by row label.
    pub fn to(&mut self, labels: &[&str]) {
        use chimera_core::ui::settings::rows;
        while self.ui.in_settings() {
            self.tap(ButtonId::Menu);
        }
        self.tap(ButtonId::Menu);
        let mut path = Vec::new();
        for l in labels {
            let i = rows(&path).iter().position(|r| r.label == *l).unwrap();
            for _ in 0..i {
                self.feed(Input::press(ButtonId::Plus));
            }
            self.feed(Input::press(ButtonId::Edit));
            path.push(i as u8);
        }
    }

    /// The bar to row `i`.
    pub fn bar_to(&mut self, i: usize) {
        let row = |r: &Rig| r.ui.location().settings().unwrap().row() as usize;
        while row(self) != i {
            let k = if row(self) < i {
                ButtonId::Plus
            } else {
                ButtonId::Minus
            };
            self.feed(Input::press(k));
        }
    }

    /// `f` loaded through LOAD, then out of SETTINGS.
    pub fn load(&mut self, f: ProjectFile) {
        self.to(&["PROJECT", "LOAD PROJECT"]);
        let l = self.ui.listing();
        let i = (0..l.len())
            .find(|&i| l.entry(i).unwrap().file() == f)
            .expect("listed");
        self.bar_to(i);
        self.tap(ButtonId::Edit);
        assert_eq!(self.ui.project().meta().file(), Some(f));
        while self.ui.in_settings() {
            self.tap(ButtonId::Menu);
        }
        self.ui.update(UiTick::for_test());
    }
}

/// The id above the highest on the card, demo range included: these tests
/// keep small ids on screen, which a real save's floor (10) would not.
pub fn next_low(_: &mut Card, s: &mut MemStore) -> ProjectFile {
    use chimera_hal::store::{Dir, Store};
    let vol = s.mount().unwrap();
    let mut top = 0;
    let _ = s.list(vol, Dir::Projects, &mut |f, _| {
        let n = f.stem()[1..]
            .iter()
            .fold(0, |n, d| n * 10 + u32::from(d - b'0'));
        top = top.max(n);
    });
    ProjectFile::for_test(chimera_core::storage::ProjectId::new(top + 1).unwrap(), vol)
}

/// A project named `n` saved to the card at its next id.
pub fn put(s: &mut MemStore, n: &str) -> ProjectFile {
    put_with(s, n, |_| {})
}

/// `put`, with `edit` run on the new project first.
pub fn put_with(s: &mut MemStore, n: &str, edit: impl FnOnce(&mut Project)) -> ProjectFile {
    let mut card = Card::new();
    let (mut p, _) = Project::boxed();
    edit(&mut p);
    p.set_name(ProjectName::new(n).unwrap());
    let f = next_low(&mut card, s);
    let fresh = chimera_core::project::FreshFile::for_test(f);
    assert!(matches!(
        save_project(&mut card, s, &mut p, SaveTo::Fresh(fresh)).out,
        ProjectNote::Saved(_)
    ));
    f
}

/// A card with three projects.
pub fn three() -> (MemStore, [ProjectFile; 3]) {
    let mut s = MemStore::new(1);
    let f = ["ALPHA", "BETA", "GAMMA"].map(|n| put(&mut s, n));
    (s, f)
}

/// An edit that leaves the project Modified, its name kept.
pub fn modify(ui: &mut UiState) {
    let p = chimera_core::project::PartId::ALL[0];
    ui.project_mut().edit_part(p).sound.params.filter.cutoff *= 0.5;
    ui.update(UiTick::for_test());
    assert_eq!(
        ui.project_status(),
        chimera_core::project::ProjectStatus::Modified
    );
}

/// ALPHA loaded and edited, then LOAD on BETA: LOAD BETA?
pub fn prompt_load(ui: &mut UiState) {
    let (s, [a, b, _]) = three();
    let mut r = Rig::new(ui, s);
    r.load(a);
    modify(r.ui);
    r.to(&["PROJECT", "LOAD PROJECT"]);
    let l = r.ui.listing();
    let i = (0..l.len()).find(|&i| l.entry(i).unwrap().file() == b);
    r.bar_to(i.unwrap());
    r.tap(ButtonId::Edit);
    assert!(r.ui.prompt_open());
}

/// SETTINGS › PROJECT › SAVE PROJECT AS on a fresh project: NAMING.
pub fn naming_save_as(ui: &mut UiState) {
    let mut r = Rig::new(ui, MemStore::new(1));
    r.to(&["PROJECT"]);
    r.bar_to(1);
    r.tap(ButtonId::Edit);
    assert!(r.ui.naming().is_some());
}

/// The shared FX page `node` of the mixer chain, through real presses:
/// MIX+B6 (Part 6's SENDS), then PLUS past it into Part 6's FX.
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

/// SETTINGS › SYSTEM › DIAGNOSTICS › DEMO's row `node` (debug builds: a
/// release build has no DEMO row).
pub fn to_demo(ui: &mut UiState, node: usize) {
    let row = reg::DEMO_BLOCKS[node].def.short;
    to_leaf(ui, &["SYSTEM", "DIAGNOSTICS", "DEMO", row]);
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

/// `f` of a bar as a clock phase.
pub fn phase(f: f64) -> chimera_core::orbit::clock::Phase {
    chimera_core::orbit::clock::Phase::new((f * 4_294_967_296.0) as u32)
}

/// The runner's view of the init scene at `f` of the first bar, playing:
/// what the ORBIT goldens draw from.
pub fn orbit_view(f: f64) -> chimera_core::orbit::runner::OrbitView {
    use chimera_core::orbit::clock::step_at;
    let mut v = chimera_core::orbit::runner::OrbitView {
        running: true,
        ..Default::default()
    };
    v.pos.phase = phase(f);
    for r in &mut v.rings {
        r.step = step_at(v.pos.phase, r.hits.steps());
    }
    v
}

/// WALK armed on CHRD's segment and `notes` captured into it: the
/// runner's `count`th capture, a third into the bar.
pub fn played(ui: &mut UiState, count: u8, notes: &[u8]) {
    use chimera_core::orbit::runner::{Capture, Captured};
    feed(ui, Input::turn(EncoderId::C, 1));
    let Capture::Segment(seg) = ui.orbit().capture else {
        panic!("PLAY arms the segment");
    };
    // Every command applied: the runner armed, nothing taken yet.
    let mut v = orbit_view(0.3);
    v.acked = ui.orbit().log().head();
    v.armed = Capture::Segment(seg);
    v.captured.count = ui.orbit().took;
    ui.set_orbit_view(&v);
    assert_eq!(ui.orbit().capture, Capture::Segment(seg), "armed");
    // Then the capture, and the runner disarmed by its take.
    v.armed = Capture::Off;
    let mut c = Captured {
        count,
        target: Capture::Segment(seg),
        scene: ui.project().orbit().current(),
        notes: [chimera_theory::Note::MIN; 8],
        len: notes.len() as u8,
    };
    for (n, &k) in c.notes.iter_mut().zip(notes) {
        *n = chimera_theory::Note::new(k).unwrap();
    }
    v.captured = c;
    ui.set_orbit_view(&v);
    assert_eq!(ui.orbit().took, count, "taken");
    assert_eq!(ui.orbit().capture, Capture::Off, "disarmed");
}

/// `orbit_view(f)` with ring 1 just fired: what ORB's golden draws.
pub fn orb_view(f: f64) -> chimera_core::orbit::runner::OrbitView {
    let mut v = orbit_view(f);
    v.rings[0].fired = 1;
    v
}

/// `orb_view(f)` with a MIDI clock leading at 133 BPM.
pub fn midi_view(f: f64) -> chimera_core::orbit::runner::OrbitView {
    use chimera_core::orbit::clock::{Bpm, ClockSource};
    chimera_core::orbit::runner::OrbitView {
        source: ClockSource::Midi,
        bpm: Bpm::new(133).unwrap(),
        ..orb_view(f)
    }
}

/// Part `p` on INIT Algo, where NEW has a factory Sound: for the tests
/// whose subject is an INIT Part, not NEW's.
#[allow(dead_code)]
pub fn init_part(ui: &mut UiState, p: PartId) {
    init_part_engine(ui, p, EngineType::Algo);
}

/// Part `p` on INIT `engine`.
#[allow(dead_code)]
pub fn init_part_engine(ui: &mut UiState, p: PartId, engine: EngineType) {
    let src = PartSource {
        part: p,
        from: PartFrom::Init(engine),
    };
    let c = ReplaceGuard::check(ui.project(), ui.template(), src).expect("a Clean Part");
    ui.project_mut().replace_part(c).expect("INIT loads");
}

/// `note` held on Part 1's channel for `blocks` blocks through a real
/// `Instrument` on the UI's Performance, and the voices' formant views
/// handed to the UI, as the shells do each frame.
pub fn hold_note(ui: &mut UiState, note: u8, blocks: usize) {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::instrument::DacBlocks;
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    use chimera_core::scope::{ScopeWriter, scope_buffer};
    let shared = Box::new(AudioShared::from_performance(ui.project().perf()));
    let mut inst = Box::new(Instrument::new(
        chimera_hal::SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    inst.handle(
        NoteEvent {
            channel: ui.project().part(PartId::ALL[0]).mix.channel,
            note: chimera_core::MidiNote::new(note).unwrap(),
            kind: NoteKind::On(chimera_core::Velocity::new(100).unwrap()),
        },
        &shared,
    );
    for _ in 0..blocks {
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
    }
    ui.set_formant_views(&inst.formant_views());
}

/// Hold SEQ into ORBIT, then B`n`'s tap: RING `n`'s RHYTHM.
pub fn to_ring(ui: &mut UiState, n: usize) {
    hold(ui, ButtonId::Seq);
    tap(ui, PART_KEYS[n - 1]);
}

const PART_KEYS: [ButtonId; 5] = [
    ButtonId::B1,
    ButtonId::B2,
    ButtonId::B3,
    ButtonId::B4,
    ButtonId::B5,
];

/// Load `engine`'s init Sound into Part 1 through the sound browser (EDIT + B1,
/// scroll to the init row, EDIT), then MINUS from the engine's home to its
/// first node.
pub fn load_init(ui: &mut UiState, engine: EngineType) {
    feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    match INIT_TYPES.iter().position(|&c| c == engine) {
        Some(row) => {
            feed(ui, Input::turn(EncoderId::A, row as i8 - HEAD as i8));
            feed(ui, Input::press(ButtonId::Edit));
        }
        // TAPE: coming soon, so not listed (ADR 0132).
        None => ui.load_for_test(chimera_core::project::PartSource {
            part: chimera_core::project::PartId::ALL[0],
            from: chimera_core::project::PartFrom::Init(engine),
        }),
    }
    let home = chimera_core::ui::nav::chain_def_for(engine).home();
    assert_eq!(
        ui.location(),
        chimera_core::ui::nav::Location::pages(chimera_core::project::PartId::ALL[0], home),
        "a load lands on the engine's home"
    );
    for _ in 0..home.node() {
        feed(ui, Input::press(ButtonId::Minus));
    }
}

/// TAPE's init on Part 1, at DECK, MODEL turned from LOOP to `m`.
pub fn tape_deck(ui: &mut UiState, m: Way) {
    load_init(ui, EngineType::Tape);
    plus(ui, 1); // SRC · DECK
    to_machine(ui, m);
}

/// MODEL turned from WALK in LOOP, the neutral, to `m`'s machine; on WALK
/// DIRECTION (cell 4) turned to `m`'s, and TRON switched on.
fn to_machine(ui: &mut UiState, m: Way) {
    let (machine, d, tron) = m.parts();
    let by = machine.index() as i8 - TapeMachine::Walk.index() as i8;
    if by != 0 {
        feed(ui, Input::turn(EncoderId::A, by));
    }
    let by = d as i8 - Direction::Loop as i8;
    if machine == TapeMachine::Walk && by != 0 {
        feed(ui, Input::turn(EncoderId::D, by));
    }
    if tron == chimera_core::dsp::tape_engine::Tron::On {
        // TRON is the WALK page's: set it, as its toggle would.
        let p = chimera_core::project::PartId::ALL[0];
        ui.project_mut().edit_part(p).sound.params.tape.tron = tron;
        feed(ui, Input::default());
    }
}

/// Samples as the arena holds them: a descriptor, then 12 bits each.
fn held(shape: Shape, count: u8, rate: u16, x: &[f32]) -> Vec<u8> {
    let d = Descriptor::whole(shape, count, rate).expect("a descriptor");
    let s: Vec<i16> = x
        .iter()
        .map(|&v| (chimera_core::dsp::clampf(v, -1.0, 1.0) * 2047.0).round() as i16)
        .collect();
    let mut h = d.to_bytes().to_vec();
    h.resize(DESC_LEN + 3 * s.len().div_ceil(2), 0);
    pack12(&s, &mut h[DESC_LEN..]);
    h
}

/// The strip's source, held in the pool and on Part 1's Sound: two
/// plucks, 1.5 s at 24 kHz; or (`wave`) a transwave of 8 frames, each
/// louder than the last.
pub fn strip_source(ui: &mut UiState, wave: bool) {
    let (shape, count, rate, x): (_, _, u16, Vec<f32>) = if wave {
        let len = frame_len(512) as usize;
        let x = (0..8 * len)
            .map(|i| {
                (0.3 + 0.1 * (i / len) as f32) * (i as f32 * core::f32::consts::FRAC_PI_4).sin()
            })
            .collect();
        (Shape::Transwave, 8, 48_000, x)
    } else {
        let x = (0..36_000)
            .map(|i| {
                let t = i as f32 / 24_000.0;
                let env = (-5.0 * t).exp()
                    + if t < 0.75 {
                        0.0
                    } else {
                        0.8 * (-5.0 * (t - 0.75)).exp()
                    };
                0.95 * env * (core::f32::consts::TAU * 180.0 * t).sin()
            })
            .collect();
        (Shape::Plain, 1, 24_000, x)
    };
    let r = SourceRef {
        crc: 0x5791_0001 + u32::from(wave),
        frames: x.len() as u32,
        rate,
    };
    let held = held(shape, count, rate, &x);
    ui.project_mut().source_store(r, &held).unwrap();
    ui.project_mut().edit_part(PartId::ALL[0]).sound.source = Some(TapeSource { r, tune: None });
}

/// What SOURCE's card goldens do once SOURCE is on the CHOIR's row.
#[derive(Clone, Copy, PartialEq)]
pub enum CardRow {
    /// Shown: its name, the hint, the meter's preview.
    On,
    /// The pool filled first: WON'T FIT in ALERT.
    Full,
    /// MIX + PLUS, every pass: the Part plays it, named.
    Loaded,
    /// MIX + PLUS, then the card gone: CARD READ FAILED.
    Failed,
}

/// TAPE's init on Part 1 at SRC, the CHOIR (1.2 s at 24 kHz, 41K) on the
/// card, and SOURCE turned onto its row; then `then`.
pub fn src_card(ui: &mut UiState, then: CardRow) {
    use chimera_core::name::SoundName;
    use chimera_core::storage::{SourceHead, WholeShape, source_crc, write_source};
    load_init(ui, EngineType::Tape);
    let x: Vec<f32> = (0..27_648)
        .map(|i| {
            let t = i as f32 / 24_000.0;
            let swell = (core::f32::consts::PI * t / 1.152).sin();
            0.8 * swell * (core::f32::consts::TAU * 220.0 * t).sin()
        })
        .collect();
    let h = held(Shape::Plain, 1, 24_000, &x);
    let data = &h[DESC_LEN..];
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, data),
            frames: x.len() as u32,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 5_700,
        period_q16: 109 << 16,
        name: SoundName::new("CHOIR").unwrap(),
    };
    let mut s = MemStore::new(1);
    let w = Card::new().run(&mut s, |s, r| write_source(s, r, &head, data));
    w.unwrap().result.unwrap();
    if then == CardRow::Full {
        // PING takes its source whole: a row too big won't stream instead
        // (slice 1 § 5.6).
        let p1 = chimera_core::project::PartId::ALL[0];
        let t = &mut ui.project_mut().edit_part(p1).sound.params.tape;
        t.set_way(chimera_core::dsp::tape_engine::Way::Ping);
        strip_source(ui, false);
        strip_source(ui, true);
        let mut crc = 0x0F11_0000;
        while ui.project().pool().free().bytes() > 40_000 {
            let r = SourceRef {
                crc,
                frames: 20_000,
                rate: 24_000,
            };
            let x = vec![0.0; 20_000];
            ui.project_mut()
                .source_store(r, &held(Shape::Plain, 1, 24_000, &x))
                .unwrap();
            crc += 1;
        }
    }
    let mut r = Rig::new(ui, s);
    r.feed(Input::turn(EncoderId::A, 100)); // the card's row, the last
    r.work(); // its head
    match then {
        CardRow::On | CardRow::Full => {}
        CardRow::Loaded => {
            feed(r.ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
            while r.ui.card_pending() {
                r.work();
            }
        }
        CardRow::Failed => {
            feed(r.ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
            r.work();
            r.s.eject();
            r.work();
        }
    }
}

/// Part 1's WALK in `mode`, its FADE the MODE edit's default.
pub fn play_mode(ui: &mut UiState, mode: Direction) {
    let p = chimera_core::project::PartId::ALL[0];
    let t = &mut ui.project_mut().edit_part(p).sound.params.tape;
    (t.machine, t.direction, t.drag) = (TapeMachine::Walk, mode, fade_default(mode));
}

/// TAPE's init on Part 1 with the strip's source, at DECK, MODEL turned
/// to `m`, the audio's heads `heads`.
pub fn tape_strip(ui: &mut UiState, m: Way, heads: Heads) {
    load_init(ui, EngineType::Tape);
    strip_source(ui, m == Way::Wave);
    plus(ui, 1); // SRC · DECK
    to_machine(ui, m);
    let from: &'static PartHeads = Box::leak(Box::new(PartHeads::new()));
    from.publish(0, heads);
    ui.hear_heads(from);
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
    to_osc(ui);
    to_osc_sub(ui, &chimera_core::ui::block_registry::ALGO_LEVEL);
}

/// From OSC or an earlier OSC sub-page, EDIT on down to `def`.
pub fn to_osc_sub(ui: &mut UiState, def: &chimera_core::ui::block_def::BlockDef) {
    use chimera_core::ui::block_registry::ALGO_CHAIN;
    let subs = ALGO_CHAIN.blocks[osc_node()].sub_pages;
    let n = subs.iter().position(|d| d.id == def.id).map_or_else(
        || {
            assert_eq!(def.id, chimera_core::ui::block_registry::ALGO_FORMANT.id);
            1
        },
        |i| i + 1,
    );
    while ui.page_def().id != def.id {
        let before = ui.page_def().id;
        feed(ui, Input::press(ButtonId::Edit));
        assert_ne!(
            before,
            ui.page_def().id,
            "EDIT moves on toward sub-page {n}"
        );
    }
}

/// From Part 1's first node, on to the engine's node and EDIT down its
/// sub-list to PIT.
pub fn to_pitch(ui: &mut UiState, engine: EngineType) {
    use chimera_core::ui::block_registry::{
        ALGO_CHAIN, MODAL_PLUCK_CHAIN, PITCH, TAPE_CHAIN, VA_CHAIN,
    };
    let chain = match engine {
        EngineType::Algo => &ALGO_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
        EngineType::Tape => &TAPE_CHAIN,
        EngineType::Va => &VA_CHAIN,
        EngineType::Drum => panic!("DRUM has no PIT page"),
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

pub fn plus(ui: &mut UiState, n: usize) {
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
    plus(ui, 4);
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

/// ENV1→CUTOFF +20, ENV1→FX −30, LFO1→CUTOFF +42 (selected).
/// FREQ (CUTOFF) and FX are permanent columns, 0 and 3: no priming.
fn mod_matrix(ui: &mut UiState) {
    plus(ui, 4);
    to_matrix(ui);
    feed(ui, Input::turn(EncoderId::E, 20)); // ENV → FREQ
    feed(ui, Input::turn(EncoderId::B, 3));
    feed(ui, Input::turn(EncoderId::E, -30)); // ENV → FX
    feed(ui, Input::turn(EncoderId::A, 3)); // LFO1
    feed(ui, Input::turn(EncoderId::B, -3));
    feed(ui, Input::turn(EncoderId::E, 42)); // LFO → FREQ, selected
}

/// Prime the focused slot for modulation (MIX + PLUS).
fn prime(ui: &mut UiState) {
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
}

/// Part 1 on INIT Algo as filter `kind`, on FLT (or FLT › MOD with `sub`).
/// LFO 1 at 3 Hz, full depth, into FREQ at +64.
pub fn lfo_on_freq(ui: &mut UiState) {
    let l = &mut ui.params_mut().lfos[0];
    (l.depth, l.rate) = (1.0, 3.0);
    ui.mod_state_mut().set_fixed(
        chimera_core::modulation::ModSource::Lfo1.index(),
        chimera_core::modulation::FixedCol::Freq,
        64,
    );
}

pub fn on_flt(ui: &mut UiState, kind: chimera_core::dsp::filter::FilterKind, sub: bool) {
    init_part(ui, PartId::ALL[0]);
    ui.params_mut().filter.set_kind(kind);
    plus(ui, 2);
    if sub {
        feed(ui, Input::press(ButtonId::Edit)); // FLT › MOD
    }
}

/// Part 1 on INIT Algo as filter `kind` with FX `fx`, on FLT › FX.
pub fn on_flt_fx(
    ui: &mut UiState,
    kind: chimera_core::dsp::filter::FilterKind,
    fx: chimera_core::dsp::voice_fx::params::FxType,
) {
    on_flt(ui, kind, true);
    ui.params_mut().fx.set_type(fx);
    feed(ui, Input::press(ButtonId::Edit)); // FLT › FX
}

/// The LED cases: the clip each shows, taken after the lerps settle
/// (`UiState::show_clip`, the meter's read with no time passed).
pub const LEDS: &[(&str, f32)] = &[
    ("flt_led_0", 0.0),
    ("flt_led_half", 0.5),
    ("flt_led_full", 1.0),
];

/// From Part 1's home to the MOD node on `engine`'s chain (its matrix).
fn to_matrix_on(ui: &mut UiState, engine: EngineType) {
    let node = chimera_core::ui::nav::chain_def_for(engine)
        .blocks
        .iter()
        .position(|b| b.def.id == reg::MOD_MATRIX.id)
        .expect("MOD on every Part chain");
    plus(ui, node);
    to_matrix(ui);
}

/// A named screen-golden case: a case name paired with the input sequence
/// that produces it.
pub type ScreenCase = (&'static str, fn(&mut UiState));

/// Every screen the goldens lock, one or more per page type (spec § Testing).
/// MST's place on the Mix chain: after TAPE only with `master-tape`
/// (ADR 0055). The mixer opens on SENDS, node 1 (ADR 0057); the FX come
/// after the Part's SENDS.
const MST: usize = if cfg!(feature = "master-tape") { 6 } else { 5 };
pub const CHO: usize = 2;
pub const DLY: usize = 3;
pub const REV: usize = 4;
#[cfg(feature = "master-tape")]
const TAPE: usize = 5;

/// The build the LIBRARY screens compare stamps with.
const LIB_THIS: chimera_core::storage::BuildStamp =
    chimera_core::storage::BuildStamp::new("0.1.0", "02e440e2", false);

/// `name` in the library as `engine`, stamped by `stamp`.
pub fn lib_sound(
    st: &mut MemStore,
    name: &str,
    engine: EngineType,
    stamp: &chimera_core::storage::BuildStamp,
) {
    let mut s = Sound::init(engine);
    s.name = chimera_core::name::SoundName::new(name).unwrap();
    chimera_core::project::save_library_sound(&mut Card::new(), st, &s, s.name, stamp)
        .out
        .unwrap();
}

/// A library file `S<id>.A` cut short at 20 bytes.
pub fn lib_cut_short(st: &mut MemStore, id: u32) {
    use chimera_hal::store::{ByteSink, Dir, FileName, Store};
    let v = st.mount().unwrap();
    let _ = st.make_dir(v, Dir::Chimera);
    let _ = st.make_dir(v, Dir::Sounds);
    let id = chimera_core::storage::LibId::new(id).unwrap();
    let f = FileName::new(Dir::Sounds, &id.stem(), b"A").unwrap();
    st.write(v, f, &mut |s: &mut dyn ByteSink| s.put(&[0x43; 20]))
        .unwrap();
}

/// Four Sounds and a cut-short file, newest first: FM BELL (a dirty
/// build), GLASS PLUCK (this build), the cut-short one, WARM BASS.
pub fn lib_card() -> MemStore {
    use chimera_core::storage::BuildStamp;
    let mut st = MemStore::new(1);
    let old = BuildStamp::new("0.1.0", "12345678", false);
    lib_sound(&mut st, "WARM BASS", EngineType::Algo, &old);
    lib_cut_short(&mut st, 2);
    lib_sound(&mut st, "GLASS PLUCK", EngineType::Modal, &LIB_THIS);
    let dirty = BuildStamp::new("0.1.0", "aabbccdd", true);
    lib_sound(&mut st, "FM BELL", EngineType::Algo, &dirty);
    st
}

/// Part 1's LIBRARY tab on `st`, listed; `f` drives it from there.
pub fn library_tab(ui: &mut UiState, st: MemStore, f: impl FnOnce(&mut Rig)) {
    ui.set_this_build_for_test(LIB_THIS);
    let mut r = Rig::new(ui, st);
    r.feed(Input::chord(ButtonId::Edit, ButtonId::B1));
    r.feed(Input::turn(EncoderId::B, 1));
    for _ in 0..3 {
        r.feed(Input::turn(EncoderId::A, 0));
    }
    f(&mut r);
}

/// `n` Sounds in the library, SOUND 1 the oldest.
fn lib_many(n: u32) -> MemStore {
    let mut st = MemStore::new(1);
    for i in 1..=n {
        let name = format!("SOUND {i}");
        lib_sound(&mut st, &name, EngineType::Algo, &LIB_THIS);
    }
    st
}

/// The build a factory set was made on: line 3 of FACTORY.
const FACTORY_STAMP: chimera_core::storage::BuildStamp =
    chimera_core::storage::BuildStamp::new("0.1.0", "1a2b3c4d", false);

/// `name` installed as factory file `S<id>.A`, described by `note`.
pub fn factory_file(st: &mut MemStore, id: u32, name: &str, engine: EngineType, note: &str) {
    use chimera_core::storage::{
        FileKind, Generation, Header, LibId, Note, encode_library_sound, write_file,
    };
    use chimera_hal::store::{Dir, FileName, Store};
    let mut s = Sound::init(engine);
    s.name = chimera_core::name::SoundName::new(name).unwrap();
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    let note = Note::new(note).unwrap();
    let v = st.mount().unwrap();
    let _ = st.make_dir(v, Dir::Chimera);
    let _ = st.make_dir(v, Dir::Sounds);
    let id = LibId::new(id).filter(|id| id.is_factory()).unwrap();
    let f = FileName::new(Dir::Sounds, &id.stem(), b"A").unwrap();
    st.write(v, f, &mut |k| {
        write_file(k, &h, &mut |w| {
            encode_library_sound(&s, &FACTORY_STAMP, Some(&note), w)
        })
    })
    .unwrap();
}

/// Three showcase Sounds, each described.
pub fn factory_card() -> MemStore {
    let mut st = MemStore::new(1);
    factory_file(
        &mut st,
        9_100_001,
        "NEON BRASS",
        EngineType::Va,
        "A PAD SOFT, BRASS PLAYED HARD",
    );
    factory_file(
        &mut st,
        9_100_002,
        "VOX CHOIR",
        EngineType::Algo,
        "FM VOWELS, SUNG BY VELOCITY",
    );
    factory_file(
        &mut st,
        9_100_003,
        "GLASS BAR",
        EngineType::Modal,
        "A STRUCK BAR OVER A SLOW SWELL",
    );
    st
}

/// Part 1's FACTORY tab on `st`, listed; `f` drives it from there.
pub fn factory_tab(ui: &mut UiState, st: MemStore, f: impl FnOnce(&mut Rig)) {
    library_tab(ui, st, |r| {
        r.feed(Input::turn(EncoderId::B, 1));
        for _ in 0..3 {
            r.feed(Input::turn(EncoderId::A, 0));
        }
        f(r)
    })
}

/// A pool with one free slot and 16 B: every library Sound won't fit.
fn pool_nearly_full(ui: &mut UiState) {
    use chimera_core::project::arena::Cost;
    chimera_core::project::test_support::set_free_to(ui.project_mut(), Cost::of(16));
}

pub const CASES: &[ScreenCase] = &[
    // The Algo cases walk from INIT Algo on Part 1, not NEW's TX BASS.
    ("engine_algo", |ui| {
        init_part(ui, PartId::ALL[0]);
        feed(ui, Input::turn(EncoderId::A, 2))
    }),
    ("algo_alg", |ui| {
        init_part(ui, PartId::ALL[0]);
        feed(ui, Input::turn(EncoderId::B, 16)); // ALG B = A17, from INIT's A1
        feed(ui, Input::turn(EncoderId::C, 50)); // MORPH 50: the diagrams blend
    }),
    ("algo_wave", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        feed(ui, Input::turn(EncoderId::B, 5)); // operator 2's wave, a few steps in
    }),
    // Operators 5 and 6 are formants, so OSC gains FRM; the table ones dim.
    ("algo_formant_page", |ui| {
        use chimera_core::dsp::algo::kinds::OpKind;
        init_part(ui, PartId::ALL[0]);
        let algo = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.params.algo;
        for op in 4..6 {
            algo.try_set_kind(op, OpKind::Formant).unwrap();
        }
        to_osc(ui);
        to_osc_sub(ui, &chimera_core::ui::block_registry::ALGO_FORMANT);
        feed(ui, Input::turn(EncoderId::E, 9)); // operator 5 FRMT
    }),
    ("algo_level", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_level_page(ui);
        feed(ui, Input::turn(EncoderId::B, 60)); // operator 2 LEVEL
    }),
    ("algo_ar", |ui| {
        use chimera_core::ui::block_registry::ALGO_AR;
        to_osc(ui);
        to_osc_sub(ui, &ALGO_AR);
        feed(ui, Input::turn(EncoderId::A, -20)); // operator 1 AR 11
    }),
    ("algo_d1l", |ui| {
        use chimera_core::ui::block_registry::{ALGO_D1L, ALGO_D1R};
        to_osc(ui);
        to_osc_sub(ui, &ALGO_D1R);
        feed(ui, Input::turn(EncoderId::B, 14)); // operator 2 D1R 14
        to_osc_sub(ui, &ALGO_D1L);
        feed(ui, Input::turn(EncoderId::B, -5)); // D1L 10
    }),
    ("algo_d2r", |ui| {
        use chimera_core::ui::block_registry::{ALGO_D1L, ALGO_D1R, ALGO_D2R};
        to_osc(ui);
        to_osc_sub(ui, &ALGO_D1R);
        feed(ui, Input::turn(EncoderId::A, 18)); // D1R 18
        to_osc_sub(ui, &ALGO_D1L);
        feed(ui, Input::turn(EncoderId::A, -3)); // D1L 12
        to_osc_sub(ui, &ALGO_D2R);
        feed(ui, Input::turn(EncoderId::A, 8)); // D2R 8
    }),
    ("algo_osc_last", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        for _ in 0..12 {
            feed(ui, Input::press(ButtonId::Edit)); // FEEDBACK, the last sub-page
        }
        feed(ui, Input::turn(EncoderId::D, 3));
    }),
    // FINE on OP2 with DETUN off 0: the band reads the ratio and the cents
    // (ADR 0298).
    ("algo_fine_ratio", |ui| {
        use chimera_core::ui::block_registry::{ALGO_DETUNE, ALGO_FINE};
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        to_osc_sub(ui, &ALGO_FINE);
        to_osc_sub(ui, &ALGO_DETUNE);
        feed(ui, Input::turn(EncoderId::B, 2));
        tap(ui, ButtonId::Seq); // back up to FINE
        feed(ui, Input::turn(EncoderId::B, 7));
    }),
    // ALG's chart after FDBK was turned on FBK's page (ADR 0298): OP1 at
    // 0.8 wears a soft loop, OP4 at 4.5 a bright one.
    ("algo_alg_loops", |ui| {
        use chimera_core::ui::block_registry::ALGO_FEEDBACK;
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        to_osc_sub(ui, &ALGO_FEEDBACK);
        feed(ui, Input::turn(EncoderId::A, 8));
        feed(ui, Input::turn(EncoderId::D, 45));
        feed(ui, Input::press(ButtonId::Minus)); // back to ALG
    }),
    // OP3's page on INIT: its envelope and wave in the band.
    ("algo_op_page_3", |ui| {
        use chimera_core::ui::block_registry::ALGO_OP_PAGES;
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        to_osc_sub(ui, &ALGO_OP_PAGES[2]);
    }),
    // OP2 a formant under a held C3: its cells, and the live grain with its
    // FC and BW_eff as the voice plays them.
    ("algo_op_page_frmt", |ui| {
        use chimera_core::dsp::algo::kinds::OpKind;
        use chimera_core::ui::block_registry::ALGO_OP_PAGES;
        init_part(ui, PartId::ALL[0]);
        let algo = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.params.algo;
        algo.try_set_kind(1, OpKind::Formant).unwrap();
        to_osc(ui);
        to_osc_sub(ui, &ALGO_OP_PAGES[1]);
        hold_note(ui, 48, 75);
    }),
    // OP3 a noise band, no note: its cells, and the bell at the stored
    // centre (C4).
    ("algo_op_page_nois", |ui| {
        use chimera_core::dsp::algo::kinds::OpKind;
        use chimera_core::ui::block_registry::ALGO_OP_PAGES;
        init_part(ui, PartId::ALL[0]);
        let algo = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.params.algo;
        algo.try_set_kind(2, OpKind::Noise).unwrap();
        to_osc(ui);
        to_osc_sub(ui, &ALGO_OP_PAGES[2]);
        feed(ui, Input::turn(EncoderId::C, 24)); // WIDTH
        feed(ui, Input::turn(EncoderId::D, 30)); // RES
    }),
    // The same with its LEVEL cell mid-turn: the UI hears no held note
    // until #37, so the band has no playhead to show.
    ("algo_op_page_3_turned", |ui| {
        use chimera_core::ui::block_registry::ALGO_OP_PAGES;
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        to_osc_sub(ui, &ALGO_OP_PAGES[2]);
        feed(ui, Input::turn(EncoderId::D, -20));
    }),
    ("bigviz_filter", |ui| {
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::C, 80)); // resonance
        feed(ui, Input::turn(EncoderId::F, -60)); // cutoff, focused
    }),
    ("flt_mode", |ui| {
        plus(ui, 2);
        feed(ui, Input::press(ButtonId::Edit)); // FLT › MOD
        feed(ui, Input::turn(EncoderId::C, 30)); // FM3: LFO 1 → FREQ
    }),
    // Each type's FLT and FLT › MOD (filter-types spec § 5).
    ("flt_clean", |ui| {
        on_flt(ui, FilterKind::Svf, false);
        ui.params_mut().filter.set_mode(FilterMode::Notch); // NOTCH1: 2 × FREQ
        feed(ui, Input::turn(EncoderId::F, -80)); // FREQ
        feed(ui, Input::turn(EncoderId::C, 40)); // Q
    }),
    ("flt_mod_clean", |ui| {
        on_flt(ui, FilterKind::Svf, true);
        feed(ui, Input::turn(EncoderId::E, 40)); // QM: ENV 3 → Q
    }),
    ("flt_2040", |ui| {
        on_flt(ui, FilterKind::K2040, false);
        feed(ui, Input::turn(EncoderId::F, -70)); // CUTOFF
        feed(ui, Input::turn(EncoderId::C, 90)); // RES: the pass band falls
    }),
    // KIND focused on the 2040: its readout names the mode it has no cell
    // for (layout A).
    ("flt_2040_kind", |ui| {
        on_flt(ui, FilterKind::K2040, false);
        feed(ui, Input::turn(EncoderId::A, 0)); // KIND focused, unmoved
    }),
    // Modulation draws a ghost beside the set curve (#575): ENV AMT +40 on
    // the 2040, an LFO on CLEAN's FREQ.
    // ENV AMT on FLT › MOD, then back round to FLT (EDIT wraps).
    ("flt_2040_env_amt", |ui| {
        on_flt(ui, FilterKind::K2040, true);
        ui.params_mut().envelopes[0].sustain = 0.7; // held mid-note
        feed(ui, Input::turn(EncoderId::A, 40)); // ENV AMT
        feed(ui, Input::press(ButtonId::Edit)); // FLT › FX
        feed(ui, Input::press(ButtonId::Edit)); // FLT
        feed(ui, Input::turn(EncoderId::F, -80)); // CUTOFF
    }),
    ("flt_clean_lfo", |ui| {
        on_flt(ui, FilterKind::Svf, false);
        feed(ui, Input::turn(EncoderId::F, -80)); // FREQ
        lfo_on_freq(ui);
    }),
    ("flt_mod_2040", |ui| {
        on_flt(ui, FilterKind::K2040, true);
        feed(ui, Input::turn(EncoderId::B, 1)); // KEYBD HALF, stepped
    }),
    ("flt_2040_exp", |ui| {
        on_flt(ui, FilterKind::K2040, false);
        feed(ui, Input::turn(EncoderId::D, 1)); // VOICE EXP
    }),
    ("flt_dirty", |ui| {
        on_flt(ui, FilterKind::Dirty, false);
        ui.params_mut().filter.set_mode(FilterMode::Notch); // NOTCH1: at FREQ
        feed(ui, Input::turn(EncoderId::F, -70)); // FREQ
        feed(ui, Input::turn(EncoderId::C, 60)); // RES
    }),
    ("flt_mod_dirty", |ui| {
        // CLEAN's cells and words (spec § 4); flt_mode's FM3 turn would
        // draw the same screen.
        on_flt(ui, FilterKind::Dirty, true);
        feed(ui, Input::turn(EncoderId::A, 20)); // FM1
    }),
    // LADDER (ADR 0104): CLEAN's cells, its LP4's bass loss in the viz.
    ("flt_ladder", |ui| {
        on_flt(ui, FilterKind::Ladder, false);
        feed(ui, Input::turn(EncoderId::F, -70)); // FREQ
        feed(ui, Input::turn(EncoderId::C, 90)); // Q: the pass band falls
    }),
    // #574: the resonance peak at its real height, a 2040 at RES 0.3 and a
    // LADDER at RES 0 (no peak).
    ("flt_2040_res_30", |ui| {
        on_flt(ui, FilterKind::K2040, false);
        ui.params_mut().filter.resonance = 0.3;
        feed(ui, Input::turn(EncoderId::D, -70)); // VOICE
    }),
    ("flt_clean_bp2", |ui| {
        on_flt(ui, FilterKind::Svf, false);
        ui.params_mut().filter.set_mode(FilterMode::Bp12);
        ui.params_mut().filter.resonance = 0.3;
        feed(ui, Input::turn(EncoderId::D, -70)); // MODE
    }),
    ("flt_ladder_res_0", |ui| {
        on_flt(ui, FilterKind::Ladder, false);
        feed(ui, Input::turn(EncoderId::D, -70)); // MODE
    }),
    ("flt_mod_ladder", |ui| {
        on_flt(ui, FilterKind::Ladder, true);
        feed(ui, Input::turn(EncoderId::A, 20)); // FM1
    }),
    // SEM (ADR 0192): MODE at the notch, RES high; OUTPUT on FLT › MOD's F
    // turned to BP, whose curve FLT then draws.
    ("flt_sem", |ui| {
        on_flt(ui, FilterKind::Sem, false);
        feed(ui, Input::turn(EncoderId::F, -70)); // FREQ
        feed(ui, Input::turn(EncoderId::C, 110)); // RES
        feed(ui, Input::turn(EncoderId::D, 64)); // MODE: the notch at FREQ
    }),
    ("flt_mod_sem", |ui| {
        on_flt(ui, FilterKind::Sem, true);
        feed(ui, Input::turn(EncoderId::F, 1)); // OUTPUT: BP
    }),
    ("flt_sem_bp", |ui| {
        on_flt(ui, FilterKind::Sem, false);
        ui.params_mut().filter.set_mode(FilterMode::Bp12); // OUTPUT: BP
        feed(ui, Input::turn(EncoderId::F, -70)); // FREQ
        feed(ui, Input::turn(EncoderId::C, 120)); // RES: the peak
    }),
    // The LEVEL LED (`LEDS`): beside INPUT on FLT › FX, and in FLT's viz
    // corner.
    ("flt_led_0", |ui| {
        on_flt_fx(ui, FilterKind::Svf, FxType::Off);
        feed(ui, Input::turn(EncoderId::A, 30)); // INPUT past unity
    }),
    ("flt_led_half", |ui| {
        on_flt_fx(ui, FilterKind::Svf, FxType::Off);
        feed(ui, Input::turn(EncoderId::A, 30));
    }),
    ("flt_led_full", |ui| {
        on_flt(ui, FilterKind::K2040, false);
        feed(ui, Input::turn(EncoderId::F, -60)); // CUTOFF
    }),
    ("flt_mod_repatched", |ui| {
        on_flt(ui, FilterKind::Svf, true);
        feed(ui, Input::turn(EncoderId::C, 30)); // FM3 on LFO 1
        feed(ui, Input::turn(EncoderId::E, 40)); // QM on ENV 3
        feed(ui, Input::press(ButtonId::Edit)); // FLT › FX
        feed(ui, Input::press(ButtonId::Edit)); // FLT › JACK (ADR 0190)
        feed(ui, Input::turn(EncoderId::C, 1)); // FM3 IN = LFO 2
        feed(ui, Input::turn(EncoderId::E, 20)); // QM IN = OP 6, the list's end
    }),
    // FB turned up on the 2040 (ADR 0190).
    ("flt_mod_fb", |ui| {
        on_flt(ui, FilterKind::K2040, true);
        feed(ui, Input::turn(EncoderId::D, 24));
    }),
    // FLT › FX (ADR 0155): OFF's dashes and dimmed MIX; SOFT's and FOLD's
    // words, a pot turned. FX and PLACE turned on FLT's front.
    ("flt_fx_off", |ui| {
        on_flt_fx(ui, FilterKind::Svf, FxType::Off)
    }),
    ("flt_fx_soft", |ui| {
        on_flt_fx(ui, FilterKind::Svf, FxType::Soft);
        feed(ui, Input::turn(EncoderId::B, 40)); // DRIVE
    }),
    ("flt_fx_fold", |ui| {
        on_flt_fx(ui, FilterKind::K2040, FxType::Fold);
        feed(ui, Input::turn(EncoderId::C, -20)); // SYMMETRY, its cell SYM
    }),
    // STABILITY in FLT › FX's F on LADDER, turned (ADR 0168).
    ("flt_fx_stab", |ui| {
        on_flt_fx(ui, FilterKind::Ladder, FxType::Off);
        feed(ui, Input::turn(EncoderId::F, -40));
    }),
    // FLT › FX with the set curve and the strip: SOFT PRE, FOLD POST.
    ("flt_fx_soft_pre", |ui| {
        on_flt_fx(ui, FilterKind::Svf, FxType::Soft);
        feed(ui, Input::turn(EncoderId::B, 60)); // DRIVE
    }),
    ("flt_fx_fold_post", |ui| {
        on_flt_fx(ui, FilterKind::Svf, FxType::Fold);
        feed(ui, Input::turn(EncoderId::B, 40)); // FOLD
    }),
    ("flt_front_fx", |ui| {
        on_flt(ui, FilterKind::Svf, false);
        feed(ui, Input::turn(EncoderId::B, 3)); // FX FOLD (past FUZZ): PLACE POST
        feed(ui, Input::turn(EncoderId::E, -1)); // PLACE PRE, focused
    }),
    // THEME's PAGES ROW (ADR 0145): FLT's three pages, no arrows; RES's
    // three; MOD at ENV2, cut on the right; OSC at D1R, cut both ways, and
    // at OP6, cut on the left; AMP, no pages, no row. Walked with
    // STACK's keys, then drawn under ROW.
    ("row_flt", |ui| {
        on_flt(ui, FilterKind::Svf, false);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    ("row_res", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 1);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    ("row_mod", |ui| {
        to_mod_sub(ui, 2, &reg::ENV_2);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    ("row_osc_mid", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        to_osc_sub(ui, &chimera_core::ui::block_registry::ALGO_D1R);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    ("row_osc_last", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_osc(ui);
        to_osc_sub(ui, &chimera_core::ui::block_registry::ALGO_OP_PAGES[5]);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    // ALGO PAGES at OPS (ADR 0300): OSC opens on OP1, the row runs
    // OPERATOR 1, 2, 3.
    ("row_osc_ops", |ui| {
        use chimera_core::ui::theme_settings::{AlgoFirst, ThemeSettings};
        init_part(ui, PartId::ALL[0]);
        ui.set_theme(ThemeSettings {
            algo_first: AlgoFirst::Operator,
            ..ui.theme()
        });
        to_osc(ui);
        feed(ui, Input::turn(EncoderId::E, 12)); // FEEDBACK 1.2, spelled out
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    ("row_amp", |ui| {
        init_part(ui, PartId::ALL[0]);
        plus(ui, 3);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    // VA's OSC1 and OSC2 pages: the chain bar says OSC1 / OSC2, not OSCILLATOR n.
    ("va_osc1", |ui| {
        stack_map(ui);
        load_init(ui, EngineType::Va);
        tap(ui, ButtonId::Edit);
    }),
    ("va_osc2", |ui| {
        stack_map(ui);
        load_init(ui, EngineType::Va);
        tap(ui, ButtonId::Edit);
        tap(ui, ButtonId::Edit);
    }),
    ("row_va_osc1", |ui| {
        stack_map(ui);
        load_init(ui, EngineType::Va);
        tap(ui, ButtonId::Edit);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
    }),
    ("row_va_osc2", |ui| {
        stack_map(ui);
        load_init(ui, EngineType::Va);
        tap(ui, ButtonId::Edit);
        tap(ui, ButtonId::Edit);
        set_pages(ui, chimera_core::ui::theme_settings::Pages::Row);
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
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::A, 1)); // VEL focused, dimmed
    }),
    // AMP's gate (ADR 0195): LPG, DECAY and STRIKE up, STRIKE focused; VEL
    // live under the strike.
    ("amp_gate", |ui| {
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::B, 96));
        feed(ui, Input::turn(EncoderId::C, 64));
        feed(ui, Input::turn(EncoderId::D, 127));
    }),
    // AMP › STEREO (ADR 0294): PAN R20, WIDTH 32, WIDTH focused.
    ("amp_stereo", |ui| {
        plus(ui, 3);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::A, 20));
        feed(ui, Input::turn(EncoderId::B, -32));
    }),
    // AMP › SENDS: the mixer's SENDS under AMP (after STEREO), REVERB up
    // and focused.
    ("amp_sends", |ui| {
        plus(ui, 3);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::C, 40));
    }),
    ("amp_vel_live", |ui| {
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::A, 1));
        prime(ui); // VEL primes the VCA: a permanent column, ALREADY ROUTED
        plus(ui, 1);
        to_matrix(ui); // ENV1 → FREQ
        feed(ui, Input::turn(EncoderId::A, 1)); // ENV2
        feed(ui, Input::turn(EncoderId::B, 2)); // VCA, the third fixed column
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
    ("tape_pitch_musical", |ui| {
        load_init(ui, EngineType::Tape);
        to_pitch(ui, EngineType::Tape);
        feed(ui, Input::turn(EncoderId::E, 1)); // SNAP MUSICAL
        feed(ui, Input::turn(EncoderId::A, 3)); // PITCH +19, focused
    }),
    // PIT's tuner (ADR 0201): middle C sounding at PITCH +7 and FINE -25.
    ("algo_pitch_note", |ui| {
        to_pitch(ui, EngineType::Algo);
        ui.hear_note(
            chimera_core::project::PartId::ALL[0],
            chimera_core::MidiNote::new(60),
            true,
        );
        feed(ui, Input::turn(EncoderId::A, 7));
        feed(ui, Input::turn(EncoderId::B, -25));
    }),
    // A TAPE source with no known root, silent: ROOT ? and the shift, dim.
    ("tape_pitch_root_unknown", |ui| {
        load_init(ui, EngineType::Tape);
        let sound = &mut ui
            .project_mut()
            .edit_part(chimera_core::project::PartId::ALL[0])
            .sound;
        sound.source = Some(chimera_core::storage::TapeSource {
            r: chimera_core::storage::SourceRef {
                crc: 0x5EED,
                frames: 36_000,
                rate: 24_000,
            },
            tune: None,
        });
        sound.params.tape.period = 0.0;
        to_pitch(ui, EngineType::Tape);
        ui.hear_note(
            chimera_core::project::PartId::ALL[0],
            chimera_core::MidiNote::new(60),
            false,
        );
        feed(ui, Input::turn(EncoderId::A, 7));
    }),
    // VA under LARGE: the corner keeps to it.
    ("large_va_pitch_note", |ui| {
        load_init(ui, EngineType::Va);
        to_pitch(ui, EngineType::Va);
        ui.hear_note(
            chimera_core::project::PartId::ALL[0],
            chimera_core::MidiNote::new(57),
            true,
        );
        feed(ui, Input::turn(EncoderId::B, 13));
        large(ui);
    }),
    // TAPE's pages (TAPE Task 15): SRC, then DECK and the model page per
    // machine, MODEL turned from WALK (index 0, MODE LOOP).
    ("tape_src", |ui| load_init(ui, EngineType::Tape)),
    // A source the pool doesn't hold: SOURCE MISSING, focused.
    ("tape_src_missing", |ui| {
        load_init(ui, EngineType::Tape);
        let r = chimera_core::storage::SourceRef {
            crc: 0x5EED,
            frames: 36_000,
            rate: 24_000,
        };
        ui.project_mut()
            .edit_part(chimera_core::project::PartId::ALL[0])
            .sound
            .source = Some(chimera_core::storage::TapeSource { r, tune: None });
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    ("tape_deck_oneshot", |ui| {
        tape_deck(ui, Way::One);
    }),
    ("tape_page_oneshot", |ui| {
        tape_deck(ui, Way::One);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    ("tape_deck_loop", |ui| tape_deck(ui, Way::Loop)),
    // Bank 2 closed: the header warns UPDATE, MODEL reads NEEDS UPDATE (ADR 0141).
    ("tape_deck_needs_update", |ui| {
        ui.set_bank2(chimera_core::bank2::Bank2Status::Empty);
        tape_deck(ui, Way::Loop);
    }),
    ("tape_page_loop", |ui| {
        tape_deck(ui, Way::Loop);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    ("tape_deck_tron", |ui| tape_deck(ui, Way::Tron)),
    ("tape_page_tron", |ui| {
        tape_deck(ui, Way::Tron);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    ("tape_deck_stretch", |ui| tape_deck(ui, Way::Stretch)),
    ("tape_page_stretch", |ui| {
        tape_deck(ui, Way::Stretch);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    // An unpitched source's TYPE in focus: too wide for the numerals.
    ("tape_page_stretch_type", |ui| {
        tape_deck(ui, Way::Stretch);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::E, 1));
    }),
    ("tape_deck_transwave", |ui| tape_deck(ui, Way::Wave)),
    ("tape_page_transwave", |ui| {
        tape_deck(ui, Way::Wave);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    ("tape_deck_scrub", |ui| tape_deck(ui, Way::Scrub)),
    ("tape_page_scrub", |ui| {
        tape_deck(ui, Way::Scrub);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    ("tape_deck_pingpong", |ui| {
        tape_deck(ui, Way::Ping);
    }),
    // WALK (ADR 0203): REVERSE's footsteps, and DRIFT's scatter on LOOP.
    ("tape_deck_reverse", |ui| {
        tape_deck(ui, Way::One);
        feed(ui, Input::turn(EncoderId::D, 1));
    }),
    ("tape_deck_drift", |ui| {
        tape_deck(ui, Way::Loop);
        feed(ui, Input::turn(EncoderId::F, 64));
    }),
    ("tape_page_pingpong", |ui| {
        tape_deck(ui, Way::Ping);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    // The strip (TAPE Task 16): each machine over the plucks (a transwave
    // for TRANSWAVE), with the heads the audio would store.
    ("tape_strip_oneshot", |ui| {
        tape_strip(ui, Way::One, Heads::new(9_000, None));
    }),
    // A loop's fade: the second head plays on.
    ("tape_strip_loop", |ui| {
        tape_strip(ui, Way::Loop, Heads::new(20_000, Some(2_000)))
    }),
    ("tape_strip_tron", |ui| {
        tape_strip(ui, Way::Tron, Heads::new(6_000, Some(30_000)))
    }),
    ("tape_strip_stretch", |ui| {
        tape_strip(ui, Way::Stretch, Heads::new(12_000, Some(13_200)))
    }),
    ("tape_strip_transwave", |ui| {
        tape_strip(ui, Way::Wave, Heads::new(2, Some(3)))
    }),
    ("tape_strip_scrub", |ui| {
        tape_strip(ui, Way::Scrub, Heads::new(16_000, None));
        let p = chimera_core::project::PartId::ALL[0];
        ui.project_mut().edit_part(p).sound.params.tape.position = 0.4; // FINGER
    }),
    ("tape_strip_pingpong", |ui| {
        tape_strip(ui, Way::Ping, Heads::new(25_000, None));
    }),
    // END before START on SRC: LOOP plays reversed.
    ("tape_strip_loop_reversed", |ui| {
        tape_strip(ui, Way::Loop, Heads::new(20_000, None));
        feed(ui, Input::press(ButtonId::Minus)); // SRC
        feed(ui, Input::turn(EncoderId::B, 100)); // START
        feed(ui, Input::turn(EncoderId::C, -80)); // END
    }),
    // SLICE 5 (a loaded Sound's: no cell sets it now).
    ("tape_strip_oneshot_slice", |ui| {
        tape_strip(ui, Way::One, Heads::new(10_000, None)); // in slice 5
        let p = chimera_core::project::PartId::ALL[0];
        ui.project_mut().edit_part(p).sound.params.tape.slice = SliceCode::new(6);
    }),
    // END before START: ONESHOT's slice 5 counts from START, PING-PONG
    // plays its sorted window.
    ("tape_strip_oneshot_slice_reversed", |ui| {
        tape_strip(ui, Way::One, Heads::NONE);
        let p = &mut ui
            .project_mut()
            .edit_part(chimera_core::project::PartId::ALL[0])
            .sound
            .params
            .tape;
        (p.start, p.end, p.slice) = (0.9, 0.1, SliceCode::new(6));
    }),
    ("tape_strip_pingpong_reversed", |ui| {
        tape_strip(ui, Way::Ping, Heads::new(20_000, None));
        let p = &mut ui
            .project_mut()
            .edit_part(chimera_core::project::PartId::ALL[0])
            .sound
            .params
            .tape;
        (p.start, p.end, p.size) = (0.9, 0.1, 0.8);
    }),
    // No note: TRON's LENGTH 0 (0.25 s) at ROOT.
    ("tape_strip_tron_idle", |ui| {
        tape_strip(ui, Way::Tron, Heads::NONE);
        let p = &mut ui
            .project_mut()
            .edit_part(chimera_core::project::PartId::ALL[0])
            .sound
            .params
            .tape;
        p.size = 0.0;
    }),
    // SRC on a pool row, SOURCE focused.
    ("tape_src_pool", |ui| {
        load_init(ui, EngineType::Tape);
        strip_source(ui, false);
        feed(ui, Input::turn(EncoderId::A, 0));
    }),
    // SOURCE on a card row (TAPE Task 17): CHOIR's name and cost, MIX +
    // PLUS's hint and the meter's preview; full, WON'T FIT; loaded, the
    // Part plays it; the card gone mid-load, CARD READ FAILED.
    ("tape_src_card", |ui| src_card(ui, CardRow::On)),
    ("tape_src_wont_fit", |ui| src_card(ui, CardRow::Full)),
    ("tape_src_loaded", |ui| src_card(ui, CardRow::Loaded)),
    ("tape_src_read_failed", |ui| src_card(ui, CardRow::Failed)),
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
    // PRT › VOICES: a cap of 4, a reserve of 2 (ADR 0281).
    ("mixer_part_voices", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::press(ButtonId::Minus)); // SENDS → PART
        feed(ui, Input::press(ButtonId::Edit)); // PART → VOICES
        feed(ui, Input::turn(EncoderId::B, 4));
        feed(ui, Input::turn(EncoderId::C, 2));
    }),
    ("mixer_sends", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::C, 40));
    }),
    // The chorus's FLANGER comb and PHASER dips (ADR 0317), MODE in focus.
    ("mixer_fx_chorus_flanger", |ui| {
        to_fx(ui, CHO);
        feed(ui, Input::turn(EncoderId::A, 3)); // MODE I → FLANGER
    }),
    ("mixer_fx_chorus_phaser", |ui| {
        to_fx(ui, CHO);
        feed(ui, Input::turn(EncoderId::A, 4)); // MODE I → PHASER
    }),
    ("mixer_fx_delay", |ui| to_fx(ui, DLY)),
    ("mixer_fx_reverb", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::E, 40)); // SMEAR
    }),
    ("mixer_fx_reverb_more", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::turn(EncoderId::D, 20)); // RATE
    }),
    // HALL at SIZE 0: the SIZE cell reads PLATE (reverb TYPEs T10).
    ("mixer_fx_reverb_hall", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 2)); // TYPE → HALL
        feed(ui, Input::turn(EncoderId::B, -128)); // SIZE 0: PLATE
    }),
    ("mixer_fx_reverb_hall_more", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 2)); // TYPE → HALL
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::turn(EncoderId::B, 20)); // DAMP
    }),
    // REV › MORE 2: HALL's DENSITY alone (ADR 0095 § HALL, as built).
    ("mixer_fx_reverb_hall_more2", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 2)); // TYPE → HALL
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::press(ButtonId::Edit)); // REV › DUCK
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE 2
        feed(ui, Input::turn(EncoderId::A, 20)); // DENSITY
    }),
    // REV › DUCK (ADR 0295): AMOUNT 64, RELEASE as set.
    ("mixer_fx_reverb_duck", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::press(ButtonId::Edit)); // REV › DUCK
        feed(ui, Input::turn(EncoderId::A, 64)); // AMOUNT
    }),
    ("mixer_fx_reverb_gate", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 4)); // TYPE → GATE
        feed(ui, Input::turn(EncoderId::C, 10)); // TAIL
    }),
    ("mixer_fx_reverb_gate_more", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 4)); // TYPE → GATE
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::turn(EncoderId::B, 20)); // SHAPE
    }),
    // SYNC TIME, then LENGTH a note up: 1/8 (reverb TYPEs T12).
    ("mixer_fx_reverb_gate_sync", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 4)); // TYPE → GATE
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::turn(EncoderId::E, 1)); // SYNC TIME: LENGTH 1/16D
        feed(ui, Input::press(ButtonId::Edit)); // REV › DUCK
        feed(ui, Input::press(ButtonId::Edit)); // back to REV
        feed(ui, Input::turn(EncoderId::B, 1)); // LENGTH 1/8
    }),
    // BLOOM: SWELL first, and MORE's two empty cells (reverb TYPEs T11).
    ("mixer_fx_reverb_bloom", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 3)); // TYPE → BLOOM
        feed(ui, Input::turn(EncoderId::B, -10)); // SWELL
    }),
    ("mixer_fx_reverb_bloom_more", |ui| {
        to_fx(ui, REV);
        feed(ui, Input::turn(EncoderId::A, 3)); // TYPE → BLOOM
        feed(ui, Input::press(ButtonId::Edit)); // REV › MORE
        feed(ui, Input::turn(EncoderId::A, 20)); // PRE-DLY
    }),
    ("mixer_fx_delay_char", |ui| {
        to_fx(ui, DLY);
        feed(ui, Input::press(ButtonId::Edit)); // DLY › CHAR
        feed(ui, Input::turn(EncoderId::A, 20)); // WOW
    }),
    // GRAIN (ADR 0202): DLY with TYPE GRAIN, then DLY › GRAIN.
    ("mixer_fx_delay_grain", |ui| {
        to_fx(ui, DLY);
        feed(ui, Input::turn(EncoderId::A, 1)); // TYPE → GRAIN
    }),
    ("mixer_fx_delay_grain_page", |ui| {
        to_fx(ui, DLY);
        feed(ui, Input::turn(EncoderId::A, 1)); // TYPE → GRAIN
        feed(ui, Input::press(ButtonId::Edit)); // DLY › CHAR
        feed(ui, Input::press(ButtonId::Edit)); // DLY › GRAIN
        feed(ui, Input::turn(EncoderId::C, 12)); // PITCH +12
    }),
    // Bank 2 closed: TYPE reads UPDATE (GRAIN plays as TAPE).
    ("mixer_fx_delay_grain_needs_update", |ui| {
        ui.set_bank2(chimera_core::bank2::Bank2Status::Empty);
        to_fx(ui, DLY);
        feed(ui, Input::turn(EncoderId::A, 1)); // TYPE → GRAIN
    }),
    // SYNC (ADR 0302): MIX + turn on TIME, which then reads its note.
    ("mixer_fx_delay_sync", |ui| {
        to_fx(ui, DLY);
        feed(ui, Input::held(ButtonId::Mix).and_turn(EncoderId::B, 1));
        feed(ui, Input::release(ButtonId::Mix));
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
    // KEY at LOW (ADR 0303): named under the curve, its hint over it.
    ("mixer_master_key", |ui| {
        to_fx(ui, MST);
        feed(ui, Input::turn(EncoderId::B, 6)); // RATIO 10:1
        feed(ui, Input::turn(EncoderId::E, 1)); // KEY LOW
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
        init_part(ui, PartId::ALL[0]);
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::press(ButtonId::Minus));
        feed(ui, Input::turn(EncoderId::C, 2)); // OUT P3
        feed(ui, Input::press(ButtonId::B1)); // back to the sound pages
    }),
    ("mod_matrix", mod_matrix),
    ("mod_matrix_wide", |ui| {
        mod_matrix(ui);
        // FLT › FX's INPUT, and SOFT's TONE, LEVEL and MIX: eight columns.
        for _ in 0..2 {
            feed(ui, Input::press(ButtonId::Minus)); // → FLT
        }
        feed(ui, Input::turn(EncoderId::B, 1)); // FX SOFT
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::press(ButtonId::Edit)); // FLT › FX
        for enc in [EncoderId::A, EncoderId::C, EncoderId::D, EncoderId::E] {
            feed(ui, Input::turn(enc, 1));
            prime(ui);
        }
        plus(ui, 2);
        to_matrix(ui);
        feed(ui, Input::turn(EncoderId::B, 5)); // col 5: scrolled one, `<` and `>`
        feed(ui, Input::turn(EncoderId::A, 3)); // LFO1 → VELO
        feed(ui, Input::turn(EncoderId::E, -127));
    }),
    // ALGO's twelve rows: OP5 → FREQ routed, the cursor on OP5's X cell
    // under INPUT's primed column, its hint (filter-types spec § 7).
    ("mod_matrix_algo_taps", |ui| {
        init_part(ui, PartId::ALL[0]);
        plus(ui, 2); // FLT
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::press(ButtonId::Edit)); // FLT › FX
        feed(ui, Input::turn(EncoderId::A, 1)); // INPUT
        prime(ui);
        plus(ui, 2);
        to_matrix(ui);
        feed(ui, Input::turn(EncoderId::A, 11)); // OP5
        feed(ui, Input::turn(EncoderId::E, 40)); // OP5 → FREQ
        feed(ui, Input::turn(EncoderId::B, 4)); // INPUT's column: an X cell
    }),
    // MODAL's eleven rows and the four fixed columns.
    ("mod_matrix_modal", |ui| {
        load_init(ui, EngineType::Modal);
        to_matrix_on(ui, EngineType::Modal);
        feed(ui, Input::turn(EncoderId::E, 30)); // ENV 1 → FREQ
    }),
    ("algo_alg_morph_dimmed", |ui| {
        init_part(ui, PartId::ALL[0]);
        feed(ui, Input::turn(EncoderId::B, -8)); // ALG B = T1 = ALG A
        feed(ui, Input::turn(EncoderId::C, 1)); // MORPH focused, dimmed
    }),
    // ENV1 → MORPH primed while it was live; then ALG B = ALG A (#188).
    ("mod_matrix_morph_inert", |ui| {
        init_part(ui, PartId::ALL[0]);
        feed(ui, Input::turn(EncoderId::C, 1));
        prime(ui);
        plus(ui, 4);
        to_matrix(ui);
        feed(ui, Input::turn(EncoderId::B, 4)); // past the fixed four to MORPH
        feed(ui, Input::turn(EncoderId::E, 60));
        for _ in 0..4 {
            feed(ui, Input::press(ButtonId::Minus));
        }
        feed(ui, Input::turn(EncoderId::B, -8));
        plus(ui, 4);
    }),
    ("sound_browser", |ui| {
        let mut s = Sound::init(EngineType::Algo);
        s.name = chimera_core::name::Name::new("WARM BASS").unwrap();
        ui.project_mut().pool_store(SlotId::ALL[0], &s).unwrap();
        let mut s = Sound::init(EngineType::Modal);
        s.name = chimera_core::name::Name::new("GLASS PLUCK").unwrap();
        ui.project_mut().pool_store(SlotId::ALL[1], &s).unwrap();
        feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    // Part 1 edited past the pool's free bytes: MIX + EDIT is refused,
    // and the band says by how much (sound-pool plan Task 6).
    ("sound_browser_pool_full", |ui| {
        use chimera_core::project::test_support::dense;
        feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
        dense(ui.project_mut().edit_part(PartId::ALL[0]).sound, 7);
        for s in SlotId::ALL {
            if ui.project().pool().get(s).is_some() {
                continue;
            }
            let mut x = Sound::init(EngineType::Algo);
            dense(&mut x, s.index() as u64);
            let p = ui.project_mut();
            if p.pool_store(s, &x).is_err()
                && p.pool_store(s, &Sound::init(EngineType::Algo)).is_err()
            {
                break;
            }
        }
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::Edit));
    }),
    ("library_empty", |ui| {
        library_tab(ui, MemStore::new(1), |_| {})
    }),
    ("library_no_card", |ui| {
        let mut st = MemStore::new(1);
        st.eject();
        library_tab(ui, st, |_| {})
    }),
    // The newest row: a dirty build's stamp.
    ("library_sounds", |ui| library_tab(ui, lib_card(), |_| {})),
    // The second: this build.
    ("library_this_build", |ui| {
        library_tab(ui, lib_card(), |r| r.feed(Input::turn(EncoderId::A, 1)))
    }),
    // The third, cut short: greyed, its message, no stamp.
    ("library_cut_short", |ui| {
        library_tab(ui, lib_card(), |r| r.feed(Input::turn(EncoderId::A, 2)))
    }),
    // Page 1 of 60, the cursor on OLDER ›.
    ("library_older", |ui| {
        library_tab(ui, lib_many(60), |r| r.feed(Input::turn(EncoderId::A, 60)))
    }),
    // Page 2, the cursor on ‹ NEWER.
    ("library_newer", |ui| {
        library_tab(ui, lib_many(60), |r| {
            r.feed(Input::turn(EncoderId::A, 60));
            r.feed(Input::press(ButtonId::Edit));
            for _ in 0..3 {
                r.feed(Input::turn(EncoderId::A, 0));
            }
        })
    }),
    ("library_wont_fit", |ui| {
        pool_nearly_full(ui);
        library_tab(ui, lib_card(), |_| {})
    }),
    ("library_pool_full", |ui| {
        pool_nearly_full(ui);
        library_tab(ui, lib_card(), |r| r.feed(Input::press(ButtonId::Edit)))
    }),
    ("library_naming", |ui| {
        library_tab(ui, lib_card(), |r| {
            r.feed(Input::chord(ButtonId::Mix, ButtonId::Edit))
        })
    }),
    // FACTORY: three tabs in the title, the row's description on line 2
    // and the set's stamp on line 3.
    ("factory_described", |ui| {
        factory_tab(ui, factory_card(), |_| {})
    }),
    ("factory_empty", |ui| {
        factory_tab(ui, MemStore::new(1), |_| {})
    }),
    ("factory_no_card", |ui| {
        let mut st = MemStore::new(1);
        st.eject();
        factory_tab(ui, st, |_| {})
    }),
    // A TAPE factory file, lowest id first: listed like any.
    ("factory_tape_row", |ui| {
        let mut st = factory_card();
        factory_file(
            &mut st,
            9_000_001,
            "OLD TAPE",
            EngineType::Tape,
            "A TAPE SOUND FROM THE TAPE SET",
        );
        factory_tab(ui, st, |_| {})
    }),
    ("settings_personal_theme", |ui| {
        to_leaf(ui, &["PERSONALIZE", "THEME"]);
        feed(ui, Input::turn(EncoderId::C, 1)); // ACCENT AMBER, focused
    }),
    // PERSONALIZE › CHAIN MAP (ADR 0145): PAGES turned to ROW, focused.
    ("settings_personal_chain_map", |ui| {
        to_leaf(ui, &["PERSONALIZE", "CHAIN MAP"]);
        feed(ui, Input::turn(EncoderId::A, -1));
    }),
    // Long user names (ADR 0154's NORMAL pair for the LARGE case).
    ("sound_browser_long", long_names),
    // PERSONALIZE › TEXT SIZE (ADR 0154): SIZE at NORMAL, the default.
    ("settings_personal_text_size", |ui| {
        to_leaf(ui, &["PERSONALIZE", "TEXT SIZE"]);
    }),
    // PERSONALIZE › PAD FOLLOW (ADR 0268): IDLE, the default, focused.
    ("settings_personal_pad_follow", |ui| {
        to_leaf(ui, &["PERSONALIZE", "PAD FOLLOW"]);
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::A, -1));
    }),
    // PERSONALIZE › ALGO PAGES (ADR 0300): FIRST turned to OPERATOR.
    ("settings_personal_algo", |ui| {
        to_leaf(ui, &["PERSONALIZE", "ALGO PAGES"]);
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    // MIDI CONFIG › SYNC (ADR 0080): EXT, the default, focused.
    ("settings_midi_sync", |ui| {
        to_leaf(ui, &["MIDI CONFIG", "SYNC"]);
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    // MIDI CONFIG › MPE: the lower zone on 15 channels, LOWER focused.
    ("settings_midi_mpe", |ui| {
        to_leaf(ui, &["MIDI CONFIG", "MPE"]);
        feed(ui, Input::turn(EncoderId::A, 15));
    }),
    // AUDIO ROUTING › USB AUDIO: USB IN at -6 dB, focused.
    #[cfg(feature = "usb-audio")]
    ("settings_audio_usb", |ui| {
        to_leaf(ui, &["AUDIO ROUTING", "USB AUDIO"]);
        feed(ui, Input::turn(EncoderId::A, -2));
    }),
    ("settings_system_diag_aud_load", |ui| {
        to_leaf(ui, &["SYSTEM", "DIAGNOSTICS", "AUDIO LOAD"])
    }),
    // PROJECT › POOL on NEW: the bar, SOUNDS, SOURCES, FREE.
    ("settings_project_pool", |ui| {
        to_leaf(ui, &["PROJECT", "POOL"])
    }),
    ("settings_top", |ui| tap(ui, ButtonId::Menu)),
    // Part 3's LIVE SNAP (plan Task 15): SNAP CHORD, HELD RE-SNAP focused.
    ("settings_part_live_snap", |ui| {
        tap(ui, ButtonId::B3);
        to_leaf(ui, &["PART", "LIVE SNAP"]);
        feed(ui, Input::turn(EncoderId::A, 2));
        feed(ui, Input::turn(EncoderId::B, 1));
    }),
    // Part 1's RANDOMIZE (ADR 0232): SEED turned to 42, its hint in the band.
    ("settings_part_randomize", |ui| {
        tap(ui, ButtonId::B1);
        to_leaf(ui, &["PART", "RANDOMIZE"]);
        feed(ui, Input::turn(EncoderId::E, 42));
    }),
    // SETTINGS › ORBIT (plan Task 17): the bar on CLEAR, RENAME later,
    // RELOAD dimmed with nothing to drop.
    ("settings_orbit", |ui| {
        to_leaf(ui, &["ORBIT"]);
        plus(ui, 1);
    }),
    // Its SAVE TO PROJ, slot 3 saved: the bar on the playing one.
    ("settings_orbit_save_to", |ui| {
        use chimera_core::orbit::scene::SceneIdx;
        use chimera_core::project::{Confirmed, SceneSlot};
        let c = Confirmed::<SceneSlot>::unasked(ui.project().orbit(), SceneIdx::ALL[2]);
        ui.project_mut().orbit_mut().overwrite_scene(c).unwrap();
        to_leaf(ui, &["ORBIT"]);
        plus(ui, 2);
        feed(ui, Input::press(ButtonId::Edit));
    }),
    // ALPHA loaded, the bar on BETA.
    ("settings_load", |ui| {
        let (s, [a, ..]) = three();
        let mut r = Rig::new(ui, s);
        r.load(a);
        r.to(&["PROJECT", "LOAD PROJECT"]);
        r.bar_to(1);
    }),
    ("settings_prompt_load", prompt_load),
    ("settings_naming", naming_save_as),
    // RING 3's RHYTHM at a third of the bar, HITS focused: 1/1, the
    // pad's one held note a bar.
    ("orbit_rhythm", |ui| {
        to_ring(ui, 3);
        feed(ui, Input::turn(EncoderId::B, -1)); // down first: 1/1 is HITS' top
        feed(ui, Input::turn(EncoderId::B, 1));
        ui.set_orbit_view(&orbit_view(0.3));
    }),
    // CHRD: segment 1 playing, a third into bar 1 of 4; segment 3 picked
    // (MIX + DEGREE).
    ("orbit_chrd", |ui| {
        to_ring(ui, 3);
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::B, 2).and_held(ButtonId::Mix));
        ui.set_orbit_view(&orbit_view(0.3));
    }),
    // CHRD with two chords played in through WALK: Bm7b5 on segment 2,
    // picked, and C on segment 4. Each a name, no numeral.
    ("orbit_chrd_played", |ui| {
        to_ring(ui, 3);
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::B, 3).and_held(ButtonId::Mix));
        played(ui, 1, &[60, 64, 67]);
        feed(ui, Input::turn(EncoderId::B, -2).and_held(ButtonId::Mix));
        played(ui, 2, &[59, 62, 65, 69]);
    }),
    // ORB on ring 3 at a third of the bar, ring 1 just fired.
    ("orbit_orb", |ui| {
        to_ring(ui, 3);
        feed(ui, Input::press(ButtonId::Minus));
        ui.set_orbit_view(&orb_view(0.3));
    }),
    // ORB under a MIDI clock at 133 BPM: the header reads the source, and
    // F, the internal BPM, is dimmed.
    ("orbit_orb_midi", |ui| {
        to_ring(ui, 3);
        feed(ui, Input::press(ButtonId::Minus));
        ui.set_orbit_view(&midi_view(0.3));
    }),
    // ORB › CUSTOM from MINOR, the tritone set under the cursor.
    ("orbit_custom", |ui| {
        to_ring(ui, 3);
        feed(ui, Input::press(ButtonId::Minus));
        feed(ui, Input::turn(EncoderId::C, 9));
        feed(ui, Input::turn(EncoderId::A, 6));
        feed(ui, Input::turn(EncoderId::B, 1));
    }),
    // ORB › CUSTOM by EDIT, SCALE still MINOR: its notes, the tritone
    // under the cursor, nothing toggled.
    ("orbit_custom_named", |ui| {
        to_ring(ui, 3);
        feed(ui, Input::press(ButtonId::Minus));
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::A, 6));
    }),
    // RING 3's VOICE, its pad note made E(5,8) on RHYTHM: GATE 60, two
    // accents, a third into the bar, playing.
    ("orbit_voice", |ui| {
        to_ring(ui, 3);
        feed(ui, Input::turn(EncoderId::A, 7)); // STEPS 8
        feed(ui, Input::turn(EncoderId::B, 4)); // HITS 5
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::A, -40)); // GATE 60
        feed(ui, Input::turn(EncoderId::C, 2)); // ACCENT 2
        let mut v = orbit_view(0.3);
        v.rings[2].step = 2;
        ui.set_orbit_view(&v);
    }),
    // RING 3's NOTES: CLEAR down's eight notes (MIX held), the fourth picked and
    // raised a tone, past two others: the sixth now.
    ("orbit_notes", |ui| {
        to_ring(ui, 3);
        tap(ui, ButtonId::Edit);
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::E, -1).and_held(ButtonId::Mix));
        feed(ui, Input::turn(EncoderId::F, 3).and_held(ButtonId::Mix));
        feed(ui, Input::turn(EncoderId::F, 2));
    }),
    // SCN on ring 3: scene 5 saved, SAVE on 3, scene 2 queued, the
    // playing scene's loop past half way, playing.
    ("orbit_scn", |ui| {
        to_ring(ui, 3);
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::B, 4));
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::B, -2));
        feed(ui, Input::turn(EncoderId::A, 1));
        let mut v = orbit_view(0.3);
        v.pos.bar = 2;
        ui.set_orbit_view(&v);
    }),
    // RING 2 as a STEP ring, CLEAR down's notes, step 4 picked; a third
    // into the bar, playing.
    ("orbit_notes_step", |ui| {
        to_ring(ui, 2);
        tap(ui, ButtonId::Edit);
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::A, -1));
        feed(ui, Input::turn(EncoderId::E, -1).and_held(ButtonId::Mix));
        feed(ui, Input::turn(EncoderId::F, 3).and_held(ButtonId::Mix));
        ui.set_orbit_view(&orbit_view(0.3));
    }),
    // RING 2 as a 32-step STEP ring, CLEAR down's notes, step 32 picked:
    // the chip row scrolled to its last eight, NOTE's gestures under it.
    ("orbit_notes_step32", |ui| {
        to_ring(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 40)); // STEPS 32
        tap(ui, ButtonId::Edit);
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::A, -1));
        feed(ui, Input::turn(EncoderId::E, -1).and_held(ButtonId::Mix));
        feed(ui, Input::turn(EncoderId::F, 40).and_held(ButtonId::Mix));
    }),
    // TEXT SIZE LARGE (ADR 0154): each its NORMAL case's screen, the
    // cells, the focus label, the hint and the lists a size up where each
    // fits.
    ("large_engine_algo", |ui| {
        init_part(ui, PartId::ALL[0]);
        feed(ui, Input::turn(EncoderId::A, 2));
        large(ui);
    }),
    ("large_algo_level", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_level_page(ui);
        feed(ui, Input::turn(EncoderId::B, 60));
        large(ui);
    }),
    ("large_bigviz_filter", |ui| {
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::C, 80));
        feed(ui, Input::turn(EncoderId::B, -60));
        large(ui);
    }),
    ("large_modal_amp", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 3);
        large(ui);
    }),
    // VA's NOISE page (ADR 0197): EDIT three times from VA; then LEVEL
    // up, its level bar in the band; then KIND on DIGITAL.
    ("va_noise", |ui| {
        load_init(ui, EngineType::Va);
        for _ in 0..3 {
            tap(ui, ButtonId::Edit);
        }
    }),
    ("va_noise_level", |ui| {
        load_init(ui, EngineType::Va);
        for _ in 0..3 {
            tap(ui, ButtonId::Edit);
        }
        feed(ui, Input::turn(EncoderId::B, 60));
    }),
    ("va_noise_digital", |ui| {
        load_init(ui, EngineType::Va);
        for _ in 0..3 {
            tap(ui, ButtonId::Edit);
        }
        feed(ui, Input::turn(EncoderId::A, 3));
    }),
    ("large_va_drive", |ui| {
        load_init(ui, EngineType::Va);
        plus(ui, 1);
        large(ui);
    }),
    ("large_mixer_sends", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::C, 40));
        large(ui);
    }),
    ("large_settings_top", |ui| {
        tap(ui, ButtonId::Menu);
        large(ui);
    }),
    ("large_settings_personal", |ui| {
        to_leaf(ui, &["PERSONALIZE"]);
        large(ui);
    }),
    ("large_sound_browser", |ui| {
        let mut s = Sound::init(EngineType::Algo);
        s.name = chimera_core::name::Name::new("WARM BASS").unwrap();
        ui.project_mut().pool_store(SlotId::ALL[0], &s).unwrap();
        let mut s = Sound::init(EngineType::Modal);
        s.name = chimera_core::name::Name::new("GLASS PLUCK").unwrap();
        ui.project_mut().pool_store(SlotId::ALL[1], &s).unwrap();
        feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::A, 1));
        large(ui);
    }),
    // Long user names: the selected one takes its row whole at LARGE,
    // the other is cut with an ellipsis, one face down the column.
    ("large_sound_browser_long", |ui| {
        long_names(ui);
        large(ui);
    }),
    ("large_library_sounds", |ui| {
        library_tab(ui, lib_card(), |_| {});
        large(ui);
    }),
    // The leaf itself, SIZE turned from NORMAL to LARGE.
    ("large_settings_text_size", |ui| {
        to_leaf(ui, &["PERSONALIZE", "TEXT SIZE"]);
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    // MIDI MAP (spec § 8.2): every kind of control, a dead target, the
    // bar on the relative encoder, spelled out with its range and echo.
    #[cfg(feature = "midi-map")]
    ("midi_map_mixed", midi_map_mixed),
    // MIDI MAP's row 0 (ADR 0226): the Part's controller map, NONE, B to
    // pick, EDIT to save.
    #[cfg(feature = "midi-map")]
    ("midi_map_pick", |ui| {
        midi_map_mixed(ui);
        feed(ui, Input::turn(EncoderId::A, -6));
    }),
    // MIDI MAP with controller map EC4 picked (ADR 0226, 0255): row 0
    // names it, its rows (one shadowed by the Sound's CC 74) above the
    // Sound's, the bar on its first row.
    #[cfg(feature = "midi-map")]
    ("midi_map_layer", midi_map_layer),
    #[cfg(feature = "midi-map")]
    ("midi_map_layer_row0", |ui| {
        midi_map_layer(ui);
        feed(ui, Input::turn(EncoderId::A, -1));
    }),
    // MIDI MAP's last row (ADR 0288): `+ NEW BINDING` under INIT's map,
    // the bar on it, EDIT to point at a cell.
    #[cfg(feature = "midi-map")]
    ("midi_map_new_row", |ui| {
        init_part(ui, PartId::ALL[0]);
        midi_map_new_row(ui);
    }),
    // Pointing from it (ADR 0288): PIT, FINE turned to point at it, NEW
    // BINDING in the value's place, EDIT BINDS  MENU CANCELS under it.
    #[cfg(feature = "midi-map")]
    ("midi_map_pointing", |ui| {
        init_part(ui, PartId::ALL[0]);
        to_pitch(ui, EngineType::Algo);
        midi_map_new_row(ui);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::B, 1));
    }),
    // INIT Algo's PIT, FINE bound to a PICKUP CC that waits below it: the
    // focus band says which way to turn (spec § 8.3).
    #[cfg(feature = "midi-map")]
    ("pickup_hint", pickup_hint),
    // DRUM's pages (ADR 0265): KIT on pad 2, LEVEL touched: the twelve
    // pads, pad 2 framed, the header naming it.
    ("drum_kit", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::C, -10));
    }),
    // PAD touched: its machine's icon large, its hint, MIX + -/+ PAD.
    ("drum_kit_hint", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
    }),
    // MACHINE touched on pad 11: TONE COWBELL beside its icon, large; the
    // cell BELL, its short name, as COWBELL runs into LEVEL.
    ("drum_kit_machine", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 10));
        feed(ui, Input::turn(EncoderId::B, 1));
        feed(ui, Input::turn(EncoderId::B, -1));
    }),
    // TONAL on pad 3, touched (ADR 0270): C1 up plays it, tuned at C4.
    ("drum_kit_tonal", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::E, 3));
    }),
    // TONAL on pad 4, CLAP: its band notes CLAP has no PITCH.
    ("drum_kit_tonal_clap", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::E, 4));
    }),
    // KEYS on pad 3, CHOKE A, CHOKE touched.
    ("drum_keys", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 2));
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::B, 1));
        feed(ui, Input::turn(EncoderId::B, -1));
    }),
    // KEYS on pad 3, CHOKE turned A to OFF: a 9th voice group, held
    // dimmed with its reason (ADR 0280).
    ("drum_keys_ninth", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 2));
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::B, -1));
    }),
    // MCH on pad 1, TONE KICK: its six knobs in its own words.
    ("drum_mch_kick", |ui| {
        load_init(ui, EngineType::Drum);
        plus(ui, 1);
    }),
    // MCH on PLAIN NOISE: DECAY and TONE, the rest empty.
    ("drum_mch_noise", |ui| {
        load_init(ui, EngineType::Drum);
        let to = chimera_core::dsp::drum::types::Machine::PlainNoise.index();
        feed(ui, Input::turn(EncoderId::B, to as i8));
        plus(ui, 1);
    }),
    // MORE on TONE KICK: MACHINE, START, HARMONICS.
    ("drum_more", |ui| {
        load_init(ui, EngineType::Drum);
        plus(ui, 1);
        tap(ui, ButtonId::Edit);
    }),
    // DRUM's FLT on pad 2, LADDER (ADR 0269): the pad's curve, Q up and
    // FREQ down, FREQ touched; FX TYPE and FX PLACE the Part's.
    ("drum_flt_ladder", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 3));
        feed(ui, Input::turn(EncoderId::C, 60));
        feed(ui, Input::turn(EncoderId::F, -80));
    }),
    // A SEM pad, FREQ down: MODE (the LP-notch-HP pot) mid-way and
    // touched, the curve a notch.
    ("drum_flt_sem", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 4));
        feed(ui, Input::turn(EncoderId::F, -80));
        feed(ui, Input::turn(EncoderId::D, 64));
    }),
    // KIND touched: the band's hint, MIX + -/+ PAD.
    ("drum_flt_hint", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::A, -1));
    }),
    // FX TYPE touched, the Part's on the pad's page: MIX + -/+ steps the
    // pad there too, said in its band.
    ("drum_flt_fx_hint", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::B, 1));
        feed(ui, Input::turn(EncoderId::B, -1));
    }),
    // FLT › MOD on a SEM pad: OUTPUT the pad's, touched, the header the pad.
    ("drum_fmd_sem", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 4));
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::F, 1));
    }),
    // AMP on pad 2: its AHD, DECAY touched down from INF.
    ("drum_amp", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::C, -40));
    }),
    // AMP › GATE: LPG up, touched.
    ("drum_gate", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 3);
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::B, 40));
    }),
    // MOD › PENV: the pad's mod envelope, ATTACK touched.
    ("drum_penv", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 4);
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::A, 20));
    }),
    // MOD › PLFO: WAVE SQUARE, touched, in full.
    ("drum_plfo", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 4);
        tap(ui, ButtonId::Edit);
        tap(ui, ButtonId::Edit);
        feed(ui, Input::turn(EncoderId::A, 3));
    }),
    // MOD › ROUTE: route 2, SOURCE touched.
    ("drum_route", |ui| {
        load_init(ui, EngineType::Drum);
        feed(ui, Input::turn(EncoderId::A, 1));
        plus(ui, 4);
        for _ in 0..3 {
            tap(ui, ButtonId::Edit);
        }
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::B, 2));
    }),
];

/// Part 1 on INIT Algo with a mixed map, MIDI MAP open on its sixth
/// binding (row 6: row 0 is the controller map).
#[cfg(feature = "midi-map")]
fn midi_map_mixed(ui: &mut UiState) {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modal::ModalParams;
    use chimera_core::midi_map::{
        Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input as In, LearnCc, Mappable, MidiMap,
        NrpnKind, Span,
    };
    use chimera_core::params::FilterParams;
    use chimera_core::part::PartParams;
    use chimera_midi::rel::ByteEncoding;
    use chimera_midi::take::Takeover;
    use chimera_midi::value::{U7, U14};
    init_part(ui, PartId::ALL[0]);
    let at = |b, id| Mappable::new(ParamAddr::new(b, id)).unwrap();
    let cc = |n| LearnCc::new(n).unwrap();
    let nrpn = U14::from_parts(U7::new(10).unwrap(), U7::new(3).unwrap());
    let e = |input, target, (min, max), feedback| Binding {
        input,
        target,
        span: Span { min, max },
        feedback,
    };
    let entries = [
        e(
            In::cc(74).unwrap(),
            at(BlockRef::Filter, FilterParams::CUTOFF),
            (0, 255),
            Feedback::Off,
        ),
        e(
            In::Cc14(Cc14Msb::new(2).unwrap(), Takeover::Pickup),
            at(BlockRef::Part, PartParams::PAN),
            (0, 255),
            Feedback::Value,
        ),
        e(
            In::Nrpn(nrpn, NrpnKind::IncDec),
            at(BlockRef::Filter, FilterParams::RESONANCE),
            (0, 255),
            Feedback::Value,
        ),
        e(
            In::Pressure(Takeover::Scale),
            at(BlockRef::Filter, FilterParams::STAB),
            (255, 0),
            Feedback::Off,
        ),
        e(
            In::Cc(cc(16), CcKind::Button(ButtonMode::Toggle)),
            at(BlockRef::Modal, ModalParams::STRUCTURE),
            (0, 255),
            Feedback::Off,
        ),
        e(
            In::Cc(cc(21), CcKind::Relative(ByteEncoding::TwosComplement)),
            at(BlockRef::Part, PartParams::LEVEL),
            (40, 200),
            Feedback::Value,
        ),
    ];
    let mut m = MidiMap::EMPTY;
    for x in entries {
        m.learn(x.input, x.target).unwrap();
        let n = m.len() - 1;
        m.set(n, x);
    }
    ui.project_mut().edit_part(PartId::ALL[0]).sound.map = m;
    to_leaf(ui, &["PART"]);
    plus(ui, chimera_core::ui::settings::tree::PART.len() - 1);
    feed(ui, Input::press(ButtonId::Edit));
    feed(ui, Input::turn(EncoderId::A, 6));
}

/// INIT's Part 1, its default map, MIDI MAP open with the bar on its last
/// row, `+ NEW BINDING` (ADR 0288).
#[cfg(feature = "midi-map")]
fn midi_map_new_row(ui: &mut UiState) {
    to_leaf(ui, &["PART"]);
    plus(ui, chimera_core::ui::settings::tree::PART.len() - 1);
    feed(ui, Input::press(ButtonId::Edit));
    feed(ui, Input::turn(EncoderId::A, -1));
}

/// `midi_map_mixed`'s Part with map EC4 picked: CC 30 MORPH and CC 74
/// RESO (shadowed by the Sound's CC 74), read from a card; the bar on the
/// map's first row.
#[cfg(feature = "midi-map")]
fn midi_map_layer(ui: &mut UiState) {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::algo::params::AlgoParams;
    use chimera_core::midi_map::ctl::{self, ControllerMap};
    use chimera_core::midi_map::{Input as In, Mappable, MidiMap};
    use chimera_core::name::SoundName;
    use chimera_core::params::FilterParams;
    midi_map_mixed(ui);
    let n = SoundName::new("EC4").unwrap();
    let mut own = MidiMap::EMPTY;
    for (cc, a) in [
        (30, ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH)),
        (
            74,
            ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
        ),
    ] {
        own.learn(In::cc(cc).unwrap(), Mappable::new(a).unwrap())
            .unwrap();
    }
    let mut m = ControllerMap::EMPTY;
    m.absorb(&own, EngineType::Algo).unwrap();
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let saved = card.run(&mut s, |s, r| ctl::save(s, r, &n, &m));
    saved.unwrap().result.unwrap();
    let mut rig = Rig::new(ui, s);
    rig.ui.project_mut().set_controller(PartId::ALL[0], Some(n));
    rig.work();
    // From the top, so row 0 stays in view.
    rig.bar_to(0);
    rig.bar_to(1);
}

/// INIT Algo's PIT, FINE touched, bound to CC 21 with PICKUP; CC 21 at 0
/// heard, below FINE: the knob must go up.
#[cfg(feature = "midi-map")]
fn pickup_hint(ui: &mut UiState) {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::midi_in::CtrlEvent;
    use chimera_core::midi_map::{
        Binding, CcKind, Feedback, Input as In, LearnCc, Mappable, MidiMap, Span,
    };
    use chimera_midi::ctl::{Ctl, FreeCc};
    use chimera_midi::take::Takeover;
    use chimera_midi::value::U7;
    init_part(ui, PartId::ALL[0]);
    to_pitch(ui, EngineType::Algo);
    feed(ui, Input::turn(EncoderId::B, -25)); // FINE -25, focused
    let fine = ParamAddr::new(BlockRef::Pitch, chimera_core::block::ParamId(1));
    let e = Binding {
        input: In::Cc(
            LearnCc::new(21).unwrap(),
            CcKind::Absolute(Takeover::Pickup),
        ),
        target: Mappable::new(fine).unwrap(),
        span: Span::FULL,
        feedback: Feedback::Off,
    };
    let mut m = MidiMap::EMPTY;
    m.learn(e.input, e.target).unwrap();
    ui.project_mut().edit_part(PartId::ALL[0]).sound.map = m;
    let ch = ui.project().part(PartId::ALL[0]).mix.channel;
    let c = Ctl::Cc {
        cc: FreeCc::new(21).unwrap(),
        value: U7::new(0).unwrap(),
    };
    ui.hear_ccs(core::iter::once(CtrlEvent::Ctl(ch, c)), Ms(10_000), true);
    assert_eq!(ui.pickup(), Some(chimera_midi::ctl::Dir::Up));
}

/// `AudioStats` fixture for AUDIO LOAD's and ABOUT's goldens and tests.
pub fn audio_fixture() -> AudioStats {
    let mut s = AudioStats::new(
        SiliconRev::V,
        chimera_core::board::BoardId::Lqfp176,
        480_000_000,
        ResetCause::Watchdog,
    );
    s.load_avg = 23;
    s.load_peak = 41;
    s.overruns = 2;
    s.desyncs = 1;
    s.drops[..2].copy_from_slice(&[0, 3]);
    s.sources = 2;
    s.stack_used = 12_000;
    s
}

/// THEME's PAGES at `pages`.
pub fn set_pages(ui: &mut UiState, pages: chimera_core::ui::theme_settings::Pages) {
    let t = ui.theme();
    ui.set_theme(chimera_core::ui::theme_settings::ThemeSettings { pages, ..t });
}

/// Two long user names in the pool, the browser open on the first.
fn long_names(ui: &mut UiState) {
    for (slot, name, engine) in [
        (0, "CATHEDRAL ORGANS", EngineType::Algo),
        (1, "SUBMERGED STRING", EngineType::Modal),
    ] {
        let mut s = Sound::init(engine);
        s.name = chimera_core::name::Name::new(name).unwrap();
        ui.project_mut().pool_store(SlotId::ALL[slot], &s).unwrap();
    }
    feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    feed(ui, Input::turn(EncoderId::A, 0));
}

/// TEXT SIZE LARGE, set after a case's keys: each large case is its
/// NORMAL golden's screen, a size up.
fn large(ui: &mut UiState) {
    set_text(ui, chimera_core::ui::theme::TextSize::Large);
}

/// TEXT SIZE at `text` (ADR 0154).
pub fn set_text(ui: &mut UiState, text: chimera_core::ui::theme::TextSize) {
    let t = ui.theme();
    ui.set_theme(chimera_core::ui::theme_settings::ThemeSettings { text, ..t });
}

/// A fresh UI under STACK: ADR 0081's keys, EDIT down and SEQ up, which
/// the suites walk pages with (ROW flips them, ADR 0145).
pub fn stack_ui() -> UiState {
    let mut ui = UiState::new();
    stack_map(&mut ui);
    ui
}

/// The map as the goldens before ROW drew it.
pub fn stack_map(ui: &mut UiState) {
    set_pages(ui, chimera_core::ui::theme_settings::Pages::Stack);
}

/// Build case `name`'s screen: a fresh UiState, the case's input, settled lerps.
pub fn ui_for(name: &str) -> UiState {
    let (_, setup) = CASES
        .iter()
        .find(|c| c.0 == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    let mut ui = UiState::new();
    // The LFO page's playhead from a view no audio publishes: parked.
    static QUIET: chimera_core::instrument::LfoView = chimera_core::instrument::LfoView::new();
    ui.hear_lfo(&QUIET);
    // PIT's tuner from no note, whatever another test plays.
    ui.hear_note(chimera_core::project::PartId::ALL[0], None, false);
    // Recorded under STACK (ADR 0145); a `row_` case sets ROW itself.
    stack_map(&mut ui);
    setup(&mut ui);
    settle(&mut ui);
    if let Some(&(_, clip)) = LEDS.iter().find(|l| l.0 == name) {
        ui.show_clip(clip);
    }
    ui
}

/// Overlays the goldens lock, each drawn alone on a blank screen: on the
/// device it lands on whatever the last frame left.
pub const OVERLAYS: &[(&str, Overlay)] = &[
    ("busy", Overlay::Busy),
    ("toast_saved", Overlay::Toast("SAVED")),
    ("toast_exfat", Overlay::Toast("CARD IS EXFAT: FORMAT FAT32")),
    ("installing", Overlay::Installing("0.2.0 02e440e2")),
    ("installing_writing", Overlay::Writing("0.2.0 02e440e2")),
    (
        "install_refused",
        Overlay::Refused(chimera_image::Slot::Mixer),
    ),
];

/// The two faces of the one overlay component.
#[derive(Clone, Copy, Debug)]
pub enum Overlay {
    Busy,
    Toast(&'static str),
    /// Bank 2's install, the whole screen, with the version it installs.
    Installing(&'static str),
    /// The same, its phase line at WRITING.
    Writing(&'static str),
    /// The install refused: the slot holds other code.
    Refused(chimera_image::Slot),
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
        Overlay::Installing(line) => {
            chimera_core::ui::installing::draw(&mut fb, line);
            (0, chimera_hal::SCREEN_HEIGHT)
        }
        Overlay::Writing(line) => {
            use chimera_core::ui::installing;
            installing::draw(&mut fb, line);
            installing::phase(&mut fb, chimera_image::Phase::Writing);
            (0, chimera_hal::SCREEN_HEIGHT)
        }
        Overlay::Refused(slot) => {
            chimera_core::ui::installing::refused(&mut fb, slot);
            (0, chimera_hal::SCREEN_HEIGHT)
        }
    };
    (fb, band)
}

/// Full render of case `name`.
pub fn render(name: &str) -> Fb {
    // The audio's process-wide state a screen reads (the held keys, PRT's
    // meters and their watch, ADR 0293): one case at a time, each from
    // rest, whatever ran before or beside it.
    static GLOBALS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
    chimera_core::played::clear_held();
    chimera_core::part_peaks::watch(chimera_core::part_peaks::Watch::Down);
    for m in &chimera_core::part_peaks::PART_PEAKS {
        m.take();
    }
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
/// Whether the first cell's value line lights `text`'s pixels and no
/// others, a column wide.
#[allow(dead_code)]
pub fn first_cell_reads(shot: &Fb, text: &str) -> bool {
    cell_reads(shot, 0, text)
}

/// Whether cell `col` of the top row reads `text` exactly, in its value's
/// place.
pub fn cell_reads(shot: &Fb, col: i32, text: &str) -> bool {
    use chimera_core::ui::theme::{CELL_COL_W, CELL_LABEL_Y, CELL_ROW_H, CELL_VALUE_DY, MARGIN_X};
    // Cells 3 to 5 are the second row's.
    let x = MARGIN_X + (col % 3) * CELL_COL_W;
    reads_at(
        shot,
        text,
        x,
        CELL_LABEL_Y + CELL_VALUE_DY + (col / 3) * CELL_ROW_H,
        CELL_COL_W - 4,
        0,
    )
}

/// Whether the focus band's value reads `text` at the status size (a
/// value too wide for the numerals: `NEEDS UPDATE`).
pub fn band_reads(shot: &Fb, text: &str) -> bool {
    use chimera_core::ui::theme::{FOCUS_VALUE_Y, LABEL_TRACKING, MARGIN_X, SCREEN_W};
    reads_at(
        shot,
        text,
        MARGIN_X,
        FOCUS_VALUE_Y,
        SCREEN_W - 2 * MARGIN_X,
        LABEL_TRACKING,
    )
}

/// `text` in the value font at `(x, y)`: every row it lights, across
/// `x..x + w`, lit in `shot` exactly where it is.
fn reads_at(shot: &Fb, text: &str, x: i32, y: i32, w: i32, tracking: i32) -> bool {
    use chimera_core::ui::theme::{BG, FONT_VALUE, INK, SCREEN_H};
    let mut want = Fb::new();
    chimera_core::ui::draw::text_tracked(&mut want, &FONT_VALUE, text, x, y, INK, tracking);
    let cols = x..x + w;
    let lit = |x, y| want.at(x, y) != Rgb565::BLACK;
    (0..SCREEN_H)
        .filter(|&y| cols.clone().any(|x| lit(x, y)))
        .all(|y| cols.clone().all(|x| (shot.at(x, y) != BG) == lit(x, y)))
}

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

/// The UI and the runner through their triple buffers, as the desktop
/// runs them: the view read, the keys, the publish; then the blocks, each
/// publishing its view.
pub struct ShellRig {
    pub ui: Box<UiState>,
    pub w: Writer<AudioShared>,
    pub r: Reader<AudioShared>,
    view_w: Writer<OrbitView>,
    view_r: Reader<OrbitView>,
    pub inst: Box<Instrument>,
    pub runner: Box<OrbitRunner>,
    /// Each block's events.
    pub sent: Vec<Vec<OrbitEvent>>,
    pub views: Vec<OrbitView>,
    /// A card for `load_new`, and the load's epoch every publish carries.
    card: Card,
    store: MemStore,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl ShellRig {
    pub fn new(ui: Box<UiState>) -> ShellRig {
        let buf = Box::leak(Box::new(TripleBuffer::new(
            AudioShared::default(),
            AudioShared::default(),
            AudioShared::default(),
        )));
        let (w, r) = buf.split();
        let (view_w, view_r) = Box::leak(Box::new(orbit_buffer())).split();
        let sr = core::num::NonZeroU32::new(chimera_hal::SAMPLE_RATE).unwrap();
        let mut card = Card::new();
        let mut store = MemStore::new(1);
        let (sync, set, _) = SystemSync::boot(&mut card, &mut store);
        ShellRig {
            ui,
            w,
            r,
            view_w,
            view_r,
            inst: Box::new(Instrument::new(
                chimera_hal::SAMPLE_RATE,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            runner: Box::new(OrbitRunner::new(sr)),
            sent: Vec::new(),
            views: Vec::new(),
            card,
            store,
            sync,
            set,
            link: LoadLink::new(),
        }
    }

    pub fn frame(&mut self, input: Input, blocks: usize) {
        self.late(0, input, blocks);
    }

    /// A frame the audio thread interrupts: `first` blocks run between
    /// the UI's view read and its keys, so the keys act on a view already
    /// stale; then the publish and `then` blocks.
    pub fn late(&mut self, first: usize, input: Input, then: usize) {
        self.ui.set_orbit_view(self.view_r.read());
        self.blocks(first);
        feed(&mut self.ui, input);
        let (ui, epoch) = (&self.ui, self.link.epoch());
        self.w.publish(|b| {
            b.update_from(ui.project().perf(), epoch);
            ui.publish_orbit(&mut b.orbit);
        });
        self.blocks(then);
    }

    /// NEW loaded as the shells load a project: `card_work`'s publish,
    /// after the settle, with no block between (the card read). The
    /// blocks of the next frame take it.
    pub fn load_new(&mut self) {
        let ui = &mut *self.ui;
        let go = match ReplaceGuard::check(ui.project(), ui.template(), ProjectSource::New) {
            Ok(go) => go,
            Err(ask) => ask.into_pending().anyway(ui.project()),
        };
        let (w, link) = (&mut self.w, &self.link);
        let loaded = ui.load_project(
            &mut self.card,
            &mut self.store,
            &mut self.sync,
            &mut self.set,
            go,
            link,
            |swap, u| {
                let _ = swap.settle(link, || false);
                w.publish(|b| {
                    b.update_from(u.project().perf(), link.epoch());
                    u.publish_orbit(&mut b.orbit);
                });
            },
        );
        assert!(loaded.is_some(), "NEW loads");
    }

    /// `n` blocks, each publishing its view: with no frame between, the
    /// UI is busy (a card read, a stall).
    pub fn blocks(&mut self, n: usize) {
        for _ in 0..n {
            let ev = self.runner.block(&mut self.inst, self.r.read());
            self.sent.push(ev.iter().collect());
            let v = self.runner.view();
            self.view_w.publish(|o| *o = v);
            self.views.push(v);
        }
    }

    pub fn idle(&mut self, blocks: usize) {
        self.frame(Input::default(), blocks);
    }

    pub fn b6(&mut self) {
        self.frame(Input::press(chimera_hal::ButtonId::B6), 1);
        self.frame(Input::release(chimera_hal::ButtonId::B6), 1);
    }

    pub fn tap(&mut self, b: chimera_hal::ButtonId) {
        self.frame(Input::press(b).at(0), 1);
        self.frame(Input::release(b).at(100), 1);
    }

    pub fn view(&self) -> OrbitView {
        self.runner.view()
    }

    /// Frames of one block until `f` holds of the view.
    pub fn until(&mut self, f: impl Fn(&OrbitView) -> bool) {
        for _ in 0..200_000 {
            if f(&self.view()) {
                return;
            }
            self.idle(1);
        }
        panic!("never");
    }
}
