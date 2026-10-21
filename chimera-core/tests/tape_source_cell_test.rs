//! TAPE's SOURCE cell (spec § 3, ADR 0093, TAPE Task 17): turned through
//! the pool's sources, a divider, then the card's; MIX + PLUS loads a card
//! row 4 KiB a UI pass behind BUSY, and MENU cancels it; per-source memory
//! in RAM. The Digitakt's SAMPLE SLOT, with the Octatrack's audio pool.

mod common;
mod screen;

use core::ops::ControlFlow;

use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::instrument::AudioShared;
use chimera_core::dsp::tape_engine::heads::{Bounds, Sizing, window_end, window_start};
use chimera_core::dsp::tape_engine::params::TapeEngineParams;
use chimera_core::dsp::tape_engine::read::Shape;
use chimera_core::dsp::tape_engine::read::{HeadSpec, head_table};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::SourceId;
use chimera_core::project::arena::Cost;
use chimera_core::project::{
    LoadLink, PartId, ProjectSource, ReplaceGuard, SourceRef, project_crc,
};
use chimera_core::storage::source_file;
use chimera_core::storage::{
    Card, SourceHead, SourceLayout, SystemSettings, SystemSync, TapeSource, WholeShape,
    held_for_file_len, source_crc, source_file_min_len, write_descriptor, write_source,
};
use chimera_core::stream::layout::StreamFile;
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::stage::heads_for;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::{FRAME_MS, UiTick};
use chimera_core::ui::meter::MeterView;
use chimera_core::ui::settings::CardCx;
use chimera_core::ui::source_cell::{
    FRAGMENTED, LoadKind, Memo, PING_NEEDS_RAM, READ_FAILED, SOURCE_MISSING, STREAMS_FULL,
    SourceMemo, SourceRow, card_cost, load_kind, rows, wont_fit_words,
};
use chimera_core::ui::view::SourceState;
use chimera_hal::card_bus::CardFault;
use chimera_hal::store::synthetic_runs;
use chimera_hal::store::{
    ByteSink, Dir, FileName, FileRuns, MAX_RUNS, ReadSink, Runs, RunsError, Store, StoreError,
    VolumeId,
};
use chimera_hal::testkit::MemStore;
use chimera_hal::units::Bytes;
use chimera_hal::units::{BlockCount, Lba};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, load_init, tap};

const P1: PartId = PartId::ALL[0];

/// A plain source of `frames` at 24 kHz.
fn source(seed: u8, frames: u32, name: &str) -> (SourceHead, Vec<u8>) {
    // Under the firmware's features streamed loads go through bank 2's door.
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let data: Vec<u8> = (0..3 * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(37).wrapping_add(seed as u32) as u8)
        .collect();
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 5_700 + seed as u16,
        period_q16: 91 << 16,
        name: SoundName::new(name).unwrap(),
    };
    (head, data)
}

/// What the arena holds for it.
fn held(h: &SourceHead, data: &[u8]) -> Vec<u8> {
    let l = SourceLayout::whole(*h).unwrap();
    let mut b = vec![0; l.held_len()];
    let at = write_descriptor(&l, &mut b);
    b[at..].copy_from_slice(data);
    b
}

/// The spec's CHOIR: 1.2 s at 24 kHz, 41K on the meter.
fn choir() -> (SourceHead, Vec<u8>) {
    source(9, 27_648, "CHOIR")
}

/// MemStore, its source reads logged (offset, bytes the sink took), and
/// failing with `Io` while `fail_reads`.
struct Log {
    inner: MemStore,
    reads: Vec<(u32, u32)>,
    fail_reads: bool,
    /// Source reads from here on give no bytes: a file shorter than listed.
    ends_at: u32,
    /// `runs` finds every file in 33 runs.
    fragmented: bool,
    /// The card IRQ's read fault, as `read_fault` reports it.
    fault: CardFault,
    /// What a `recover` gives: `Ok` mounts, clearing the fault.
    recovery: Result<(), StoreError>,
    recovers: u32,
}

struct Counted<'a>(&'a mut dyn ReadSink, u32);

impl ReadSink for Counted<'_> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.0.begin(len)
    }
    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.1 += b.len() as u32;
        self.0.chunk(b)
    }
}

impl Store for Log {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }
    fn read_fault(&self) -> CardFault {
        self.fault
    }
    fn recover(&mut self) -> Result<VolumeId, StoreError> {
        self.recovers += 1;
        self.recovery?;
        self.fault = CardFault::Clear;
        self.inner.mount()
    }
    fn list(
        &mut self,
        v: VolumeId,
        d: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.inner.list(v, d, f)
    }
    fn read(&mut self, v: VolumeId, f: FileName, s: &mut dyn ReadSink) -> Result<(), StoreError> {
        self.read_at(v, f, 0, s)
    }
    fn read_at(
        &mut self,
        v: VolumeId,
        f: FileName,
        at: u32,
        s: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if f.dir() == Dir::Tapes && self.fail_reads {
            return Err(StoreError::Io);
        }
        let mut c = Counted(s, 0);
        if f.dir() == Dir::Tapes && at >= self.ends_at {
            let _ = c.begin(0);
            self.reads.push((at, 0));
            return Ok(());
        }
        let r = self.inner.read_at(v, f, at, &mut c);
        if f.dir() == Dir::Tapes {
            self.reads.push((at, c.1));
        }
        r
    }
    fn write(
        &mut self,
        v: VolumeId,
        f: FileName,
        b: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.inner.write(v, f, b)
    }
    fn delete(&mut self, v: VolumeId, f: FileName) -> Result<(), StoreError> {
        self.inner.delete(v, f)
    }
    fn make_dir(&mut self, v: VolumeId, d: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(v, d)
    }
    fn seal(&mut self, v: VolumeId, f: FileName) -> Result<(), StoreError> {
        self.inner.seal(v, f)
    }
    fn runs(&mut self, v: VolumeId, f: FileName) -> Result<FileRuns, RunsError> {
        if !self.fragmented {
            return self.inner.runs(v, f);
        }
        let mut r = Runs::new();
        for i in 0..MAX_RUNS as u32 + 1 {
            r.push(Lba::new(10 * i), BlockCount::ONE);
        }
        Err(RunsError::Fragmented(r.finish().unwrap_err()))
    }
}

struct Rig {
    ui: Box<UiState>,
    s: Log,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl Rig {
    /// Part 1 on TAPE's init at SRC, `on_card` written to the card.
    fn new(on_card: &[(SourceHead, Vec<u8>)]) -> Rig {
        let mut s = Log {
            inner: MemStore::new(1),
            reads: Vec::new(),
            fail_reads: false,
            ends_at: u32::MAX,
            fragmented: false,
            fault: CardFault::Clear,
            recovery: Ok(()),
            recovers: 0,
        };
        let mut card = Card::new();
        for (h, d) in on_card {
            let w = card.run(&mut s, |s, r| write_source(s, r, h, d)).unwrap();
            w.result.unwrap();
        }
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        let mut ui = Box::new(UiState::new());
        load_init(&mut ui, EngineType::Tape);
        s.reads.clear();
        Rig {
            ui,
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
        }
    }

    /// One frame's card work.
    fn work(&mut self) {
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.s,
            sync: &mut self.sync,
            settings: &mut self.set,
        };
        let _ = self.ui.card_work(cx, link, |swap, _| {
            let _ = swap.settle(link, || false);
        });
    }

    /// `i`, then card work until none is queued but a load.
    fn feed(&mut self, i: Input) {
        feed(&mut self.ui, i);
        while self.ui.card_pending() && !self.loading() {
            self.work();
        }
    }

    fn turn(&mut self, d: i8) {
        self.feed(Input::turn(EncoderId::A, d));
    }

    fn mix_plus(&mut self) {
        feed(&mut self.ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    }

    fn loading(&self) -> bool {
        self.ui.source_loading()
    }

    /// Passes until the load ends.
    fn load(&mut self) -> usize {
        self.mix_plus();
        let mut n = 0;
        while self.ui.card_pending() {
            self.work();
            n += 1;
        }
        n
    }

    fn load_new(&mut self) {
        let ui = &mut *self.ui;
        let go = match ReplaceGuard::check(ui.project(), ui.template(), ProjectSource::New) {
            Ok(go) => go,
            Err(ask) => ask.into_pending().anyway(ui.project()),
        };
        let link = &self.link;
        let _ = ui.load_project(
            &mut self.card,
            &mut self.s,
            &mut self.sync,
            &mut self.set,
            go,
            link,
            |swap, _| {
                let _ = swap.settle(link, || false);
            },
        );
    }

    fn sound(&self) -> &Sound {
        &self.ui.project().part(P1).sound
    }

    fn band(&self) -> String {
        let mut b = chimera_core::ui::fmt::FmtBuf::new();
        self.ui.source_state().text(&mut b);
        b.as_str().into()
    }

    /// Pool sources `held` directly, as a project load leaves them.
    fn pool(&mut self, src: &[&(SourceHead, Vec<u8>)]) {
        for (h, d) in src {
            self.ui
                .project_mut()
                .source_store(h.sref, &held(h, d))
                .unwrap();
        }
    }
}

fn file_len(h: &SourceHead) -> u32 {
    source_file_min_len(h.sref).unwrap()
}

#[test]
fn rows_pool_then_card() {
    let (a, b, c, d) = (
        source(1, 600, "A"),
        source(2, 700, "B"),
        source(3, 800, "C"),
        source(4, 900, "D"),
    );
    let mut rig = Rig::new(&[]);
    rig.pool(&[&a, &b]);
    let card = [
        (c.0.sref.stem(), file_len(&c.0)),
        (a.0.sref.stem(), file_len(&a.0)),
        (d.0.sref.stem(), file_len(&d.0)),
    ];
    let got: Vec<_> = rows(rig.ui.project().pool(), &card).collect();
    let pool = rig.ui.project().pool();
    let id = |h: &SourceHead| pool.find_source(h.sref).unwrap();
    let cost = |h: &SourceHead| card_cost(file_len(h)).unwrap();
    assert_eq!(
        got,
        [
            SourceRow::Pool(id(&a.0)),
            SourceRow::Pool(id(&b.0)),
            SourceRow::Divider,
            SourceRow::Card {
                stem: c.0.sref.stem(),
                cost: cost(&c.0)
            },
            SourceRow::Card {
                stem: d.0.sref.stem(),
                cost: cost(&d.0)
            },
        ]
    );
    // No card row, no divider.
    assert_eq!(rows(pool, &card[1..2]).count(), 2);
}

#[test]
fn card_row_costs_from_size() {
    for frames in [64, 1_000, 27_648, 100_001] {
        let (h, d) = source(5, frames, "X");
        let size = file_len(&h);
        let want = Cost::of(16 + held_for_file_len(size).unwrap() as usize);
        assert_eq!(card_cost(size), Some(want), "{frames}");
        assert_eq!(
            want,
            Cost::of(16 + held(&h, &d).len()),
            "{frames}: the pool's cost"
        );
    }
    // Listing them reads no file.
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    assert_eq!(rig.s.reads.len(), 1, "one head read, nothing else");
    assert_eq!(rig.s.reads[0], (0, 4_096));
}

#[test]
fn focus_band_reads_the_source() {
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    assert!(matches!(rig.ui.source_state(), SourceState::Card { .. }));
    assert_eq!(rig.band(), "CHOIR · 24K · 1.2 S · 41K");
    // Another page and back: off the card row, the Part's source again.
    feed(&mut rig.ui, Input::press(ButtonId::Plus));
    feed(&mut rig.ui, Input::press(ButtonId::Minus));
    assert_eq!(rig.ui.source_state(), SourceState::None);
    rig.turn(1);
    rig.load();
    assert!(matches!(rig.ui.source_state(), SourceState::Held { .. }));
    assert_eq!(rig.band(), "CHOIR · 24K · 1.2 S · 41K");
}

#[test]
fn load_steps_4k_a_pass() {
    let (h, _) = choir();
    let size = file_len(&h);
    assert!(
        (40 * 1024..44 * 1024).contains(&size),
        "a 41 KB source: {size}"
    );
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    rig.s.reads.clear();
    assert_eq!(rig.load(), 11);
    assert_eq!(rig.s.reads.len(), 11);
    for (k, &(at, took)) in rig.s.reads.iter().enumerate() {
        assert_eq!(at, k as u32 * 4_096, "pass {k}");
        assert!(took <= 4_096, "pass {k}: {took}");
    }
}

#[test]
fn loaded_row_moves_above_divider() {
    let (h, _) = choir();
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    rig.load();
    let pool = rig.ui.project().pool();
    let id = pool.find_source(h.sref).expect("published");
    let card = [(h.sref.stem(), file_len(&h))];
    assert_eq!(rows(pool, &card).collect::<Vec<_>>(), [SourceRow::Pool(id)]);
    let s = rig.sound();
    assert_eq!(s.source_ref(), Some(h.sref), "the Part plays it");
    assert_eq!(s.params.tape.root, f32::from(h.root));
    assert_eq!(s.params.tape.period, 91.0);
    assert!(pool.source_view(h.sref).is_some());
}

/// #742: SOURCE on a card row it hasn't loaded plays nothing, as its
/// label says; off the row, the Part's own source plays again.
#[test]
fn an_unloaded_card_row_plays_nothing() {
    let arp = source(1, 6_000, "ARP");
    let mut rig = Rig::new(&[choir()]);
    rig.pool(&[&arp]);
    let id = rig.ui.project().pool().find_source(arp.0.sref).unwrap();
    let heard = |rig: &Rig| {
        let mut out = AudioShared::default();
        out.update_from_project(rig.ui.project(), 0, &mut KnobPublisher::new());
        rig.ui.publish_ui(&mut out);
        out.parts[0].source
    };
    rig.turn(1); // ARP
    assert_eq!(heard(&rig), Some(id));
    rig.turn(1); // CHOIR, on the card
    assert_eq!(rig.band(), "CHOIR · 24K · 1.2 S · 41K");
    assert_eq!(heard(&rig), None, "an unloaded row plays nothing");
    rig.turn(-1); // ARP again
    assert_eq!(heard(&rig), Some(id));
    rig.turn(1);
    // Another page: off the row, ARP reads and plays.
    feed(&mut rig.ui, Input::press(ButtonId::Plus));
    assert_eq!(heard(&rig), Some(id));
}

/// A card row that won't fit whole, on a Part that needs it whole (PING:
/// slice 1 § 5.6), alerts; MIX + PLUS loads nothing and says why.
#[test]
fn wont_fit_alerts() {
    let mut rig = Rig::new(&[choir()]);
    rig.tape(|t| t.machine = TapeMachine::Ping);
    // Fill the pool until the CHOIR won't fit.
    let mut seed = 20;
    while rig.ui.project().pool().free() >= card_cost(file_len(&choir().0)).unwrap() {
        let filler = source(seed, 20_000, "FILL");
        rig.pool(&[&filler]);
        seed += 1;
    }
    rig.turn(100); // past the fillers, the last row
    let SourceState::Card { cost, free, .. } = rig.ui.source_state() else {
        panic!("on the card row: {:?}", rig.ui.source_state());
    };
    assert!(cost > free);
    let words = wont_fit_words(cost, free);
    assert!(
        words.as_str().starts_with("WON'T FIT · NEEDS 41K, "),
        "{}",
        words.as_str()
    );
    // The band draws it in ALERT.
    let fb = screen::render_ui(&rig.ui);
    let alert = chimera_core::ui::theme::ALERT;
    let y = chimera_core::ui::hint::FOCUS_Y;
    assert!((y - 8..=y).any(|y| (12..200).any(|x| fb.at(x, y) == alert)));
    let before = MeterView::of(rig.ui.project().pool(), None);
    rig.mix_plus();
    assert!(!rig.ui.card_pending(), "loads nothing");
    assert_eq!(MeterView::of(rig.ui.project().pool(), None), before);
    assert_eq!(rig.band(), PING_NEEDS_RAM);
}

/// Review Focus 1: a load that doesn't finish (MENU at each pass; the card
/// failing its reads or gone at each pass) leaves the gap and the meter as
/// they were, publishes nothing, and the Part plays its old source; an
/// error reads CARD READ FAILED.
#[test]
fn unfinished_load_leaves_the_arena() {
    let (h, _) = choir();
    let old = source(40, 3_000, "OLD");
    for how in ["menu", "fail", "absent", "short"] {
        for pass in 0..11 {
            let mut rig = Rig::new(&[choir()]);
            rig.pool(&[&old]);
            rig.ui.project_mut().edit_part(P1).sound.source = Some(TapeSource::from(old.0.sref));
            rig.turn(1);
            assert!(matches!(rig.ui.source_state(), SourceState::Card { .. }));
            let pool = rig.ui.project().pool();
            let (free, meter) = (pool.free(), MeterView::of(pool, None));
            rig.mix_plus();
            for _ in 0..pass {
                rig.work();
            }
            assert!(rig.ui.card_pending(), "{how} {pass}: still loading");
            match how {
                "menu" => tap(&mut rig.ui, ButtonId::Menu),
                "fail" => rig.s.fail_reads = true,
                "short" => rig.s.ends_at = 0,
                _ => rig.s.inner.eject(),
            }
            rig.work();
            assert!(!rig.ui.card_pending(), "{how} {pass}: ended");
            let pool = rig.ui.project().pool();
            assert_eq!(pool.free(), free, "{how} {pass}: the gap");
            assert_eq!(MeterView::of(pool, None), meter, "{how} {pass}: the meter");
            assert_eq!(
                pool.find_source(h.sref),
                None,
                "{how} {pass}: nothing published"
            );
            assert_eq!(rig.sound().source_ref(), Some(old.0.sref), "{how} {pass}");
            if how != "menu" {
                assert_eq!(rig.band(), READ_FAILED, "{how} {pass}");
            } else {
                assert!(!rig.ui.in_settings(), "MENU's tap only cancels");
            }
        }
    }
}

#[test]
fn source_memory_recalls() {
    let (a, b) = (source(1, 6_000, "A"), source(2, 7_000, "B"));
    // START 0.2, END 0.6; then END before START (reversed).
    for (start, end) in [(0.2, 0.6), (0.7, 0.1)] {
        let mut rig = Rig::new(&[a.clone(), b.clone()]);
        rig.pool(&[&a, &b]);
        rig.turn(1); // A
        assert_eq!(rig.sound().source_ref(), Some(a.0.sref));
        {
            let p = rig.ui.project_mut().edit_part(P1);
            (p.sound.params.tape.start, p.sound.params.tape.end) = (start, end);
            (p.sound.params.pitch.pitch, p.sound.params.pitch.fine) = (7.0, -3.0);
        }
        rig.turn(1); // B, unseen
        let s = rig.sound();
        assert_eq!(s.source_ref(), Some(b.0.sref));
        assert_eq!((s.params.tape.start, s.params.tape.end), (0.0, 1.0));
        assert_eq!(
            (s.params.pitch.pitch, s.params.pitch.fine),
            (7.0, -3.0),
            "kept"
        );
        rig.ui.project_mut().edit_part(P1).sound.params.pitch.pitch = -5.0;
        rig.turn(-1); // A again
        let s = rig.sound();
        assert_eq!(s.source_ref(), Some(a.0.sref));
        assert_eq!((s.params.tape.start, s.params.tape.end), (start, end));
        assert_eq!((s.params.pitch.pitch, s.params.pitch.fine), (7.0, -3.0));
        // A known head's ROOT comes with it.
        assert_eq!(s.params.tape.root, f32::from(a.0.root));
    }
}

#[test]
fn source_memory_is_ram_only() {
    // 33 sources turned: the first one's entry is the one gone.
    let mut m = SourceMemo::EMPTY;
    let mut s = Sound::init(EngineType::Tape);
    for crc in 1..=33 {
        let r = SourceRef {
            crc,
            frames: 64,
            rate: 24_000,
        };
        m.turn(&mut s, r, None);
        s.params.tape.start = crc as f32 / 100.0;
    }
    m.keep(&s);
    assert_eq!(
        m.get(chimera_core::storage::Stem::new(1)),
        None,
        "the oldest went"
    );
    assert_eq!(
        m.get(chimera_core::storage::Stem::new(2)),
        Some(Memo {
            crc: core::num::NonZeroU32::new(2).unwrap(),
            start: 0.02,
            end: 1.0,
            pitch: 0,
            fine: 0
        })
    );
    assert!(m.get(chimera_core::storage::Stem::new(33)).is_some());

    // A save holds no memory, and a load starts it empty.
    let (a, b) = (source(1, 6_000, "A"), source(2, 7_000, "B"));
    let at_a = || {
        let mut rig = Rig::new(&[a.clone(), b.clone()]);
        rig.pool(&[&a, &b]);
        rig.turn(1);
        rig
    };
    let plain = at_a();
    let mut kept = at_a();
    kept.ui.project_mut().edit_part(P1).sound.params.tape.start = 0.5;
    kept.turn(1);
    kept.turn(-1);
    assert_eq!(kept.sound().params.tape.start, 0.5, "remembered");
    kept.ui.project_mut().edit_part(P1).sound.params.tape.start = 0.0;
    assert_eq!(
        project_crc(plain.ui.project()),
        project_crc(kept.ui.project()),
        "no new record"
    );
    // NEW loaded: A → B → A on Part 1 finds no memory of A.
    kept.ui.project_mut().edit_part(P1).sound.params.tape.start = 0.5;
    kept.turn(1);
    kept.load_new();
    load_init(&mut kept.ui, EngineType::Tape);
    kept.pool(&[&a, &b]);
    kept.turn(1);
    assert_eq!(kept.sound().params.tape.start, 0.0, "the memo starts empty");
}

/// A card row that takes the gap exactly fits: MIX + PLUS loads it.
#[test]
fn exactly_free_fits() {
    let mut rig = Rig::new(&[choir()]);
    let cost = card_cost(file_len(&choir().0)).unwrap();
    let free = rig.ui.project().pool().free();
    let fill = (free.bytes() - cost.bytes()) as usize - 16;
    let r = SourceRef {
        crc: 0xF111,
        frames: 64,
        rate: 24_000,
    };
    rig.ui
        .project_mut()
        .source_store(r, &vec![0; fill])
        .unwrap();
    assert_eq!(rig.ui.project().pool().free(), cost);
    rig.turn(100);
    assert!(matches!(rig.ui.source_state(), SourceState::Card { .. }));
    let fb = screen::render_ui(&rig.ui);
    let y = chimera_core::ui::hint::FOCUS_Y;
    let alert = chimera_core::ui::theme::ALERT;
    assert!(
        !(y - 8..=y).any(|y| (12..200).any(|x| fb.at(x, y) == alert)),
        "no WON'T FIT"
    );
    rig.load();
    assert_eq!(rig.sound().source_ref(), Some(choir().0.sref));
}

/// The gap a load stages in never moves under it: a pending hole stays
/// open while it loads (no compaction), and a source stored mid-load
/// fails it, publishing nothing.
#[test]
fn a_moved_gap_fails_the_load() {
    use chimera_core::ui::animation::UiTick;
    let (h, _) = choir();
    let (a, b) = (source(1, 6_000, "A"), source(2, 7_000, "B"));
    let mut rig = Rig::new(&[choir()]);
    rig.pool(&[&a, &b]);
    let purged = rig.ui.project_mut().purge_unused_sources(0);
    assert!(purged.freed.bytes() > 0, "A and B pending holes");
    rig.turn(100);
    rig.mix_plus();
    while rig.ui.card_pending() {
        rig.ui.update(UiTick::for_test());
        rig.work();
    }
    assert!(
        rig.ui.project().pool().find_source(h.sref).is_some(),
        "loaded beside the holes"
    );

    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    rig.mix_plus();
    rig.work();
    rig.pool(&[&a]);
    rig.work();
    assert!(!rig.ui.card_pending());
    assert_eq!(rig.ui.project().pool().find_source(h.sref), None);
    assert_eq!(rig.band(), READ_FAILED);
}

/// Library spec § Load: the last chunk stages the source and says what
/// it is; only `adopt` publishes it.
#[test]
fn the_last_chunk_does_not_publish_before_adopt() {
    use chimera_core::ui::source_cell::{SourceLoad, StagedStep};
    let (h, d) = choir();
    let mut rig = Rig::new(&[choir()]);
    let srcs = rig.ui.project().pool().source_count();
    let mut l = SourceLoad::start(rig.ui.project_mut(), h.sref.stem(), file_len(&h)).unwrap();
    let ui = &mut rig.ui;
    let last = loop {
        let step = rig
            .card
            .run(&mut rig.s, |s, r| {
                Ok::<_, StoreError>(l.step_staged(s, r, ui.project_mut()))
            })
            .unwrap()
            .result
            .unwrap();
        if step != StagedStep::More {
            break step;
        }
    };
    let StagedStep::Staged(need) = last else {
        panic!("{last:?}");
    };
    assert_eq!((need.r, need.held), (h.sref, held(&h, &d).len()));
    assert!(need.readable());
    let pool = rig.ui.project().pool();
    assert_eq!(pool.source_count(), srcs);
    assert_eq!(pool.find_source(h.sref), None);
}

// Streamed loads (slice 1 § 5.6): a source too big for the arena loads as
// its descriptor, head table and heads; the rest stays on the card.

/// 6.25 s at 24 kHz: 225 KB whole, more than the arena.
fn bed(seed: u8) -> (SourceHead, Vec<u8>) {
    source(seed, 150_000, "BED")
}

impl Rig {
    fn tape(&mut self, f: impl FnOnce(&mut TapeEngineParams)) {
        f(&mut self.ui.project_mut().edit_part(P1).sound.params.tape);
    }

    fn params(&self) -> TapeEngineParams {
        self.sound().params.tape
    }

    fn registry(&self) -> &'static StreamRegistry {
        self.ui.project().pool().streams().expect("a registry")
    }

    /// One UI frame: its tick, then its card work.
    fn frame(&mut self) {
        self.ui.update(UiTick::for_test());
        self.work();
    }
}

/// The file `write_source` makes for `src`, byte for byte.
fn file_bytes(src: &(SourceHead, Vec<u8>)) -> Vec<u8> {
    let mut s = MemStore::new(9);
    let mut card = Card::new();
    let w = card
        .run(&mut s, |s, r| write_source(s, r, &src.0, &src.1))
        .unwrap();
    w.result.unwrap();
    let mut out = Vec::new();
    card.run(&mut s, |s, r| {
        struct Sink<'a>(&'a mut Vec<u8>);
        impl ReadSink for Sink<'_> {
            fn begin(&mut self, _: u32) -> ControlFlow<()> {
                ControlFlow::Continue(())
            }
            fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
                self.0.extend_from_slice(b);
                ControlFlow::Continue(())
            }
        }
        s.read(r.volume(), source_file(src.0.sref), &mut Sink(&mut out))
    })
    .unwrap()
    .result
    .unwrap();
    out
}

/// The streamed source's row: its head table's heads, each holding the
/// file's samples at its frames.
fn assert_heads(rig: &Rig, src: &(SourceHead, Vec<u8>), want: &[HeadSpec]) {
    let pool = rig.ui.project().pool();
    assert!(pool.streamed(src.0.sref), "streamed");
    let held = pool.held(src.0.sref).expect("held");
    let t = head_table(held).expect("a head table");
    assert_eq!(t.iter().collect::<Vec<_>>(), want);
    for i in 0..t.len() {
        let (h, at) = t.get(i).unwrap();
        let o = 3 * (h.first() / 2) as usize;
        assert_eq!(
            &held[at..at + h.byte_len()],
            &src.1[o..o + h.byte_len()],
            "head {i}"
        );
    }
}

#[test]
fn fits_loads_whole() {
    let (h, _) = choir();
    let mut rig = Rig::new(&[choir()]);
    let free = rig.ui.project().pool().free();
    assert_eq!(load_kind(&h, free, &rig.params()), LoadKind::Whole);
    rig.turn(1);
    rig.load();
    let pool = rig.ui.project().pool();
    assert!(pool.source_view(h.sref).is_some(), "held whole");
    assert!(!pool.streamed(h.sref));
    let id = pool.find_source(h.sref).unwrap();
    assert!(rig.registry().get(id).is_none(), "no stream row");
}

#[test]
fn too_big_play_one_streams_with_start_head() {
    let b = bed(1);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    let heads = heads_for(b.0.sref, &rig.params()).unwrap();
    assert_eq!(heads.as_slice().len(), 1);
    let free = rig.ui.project().pool().free();
    assert_eq!(
        load_kind(&b.0, free, &rig.params()),
        LoadKind::Streamed(heads)
    );
    rig.turn(1);
    // The meter previews the heads' cost, and MIX + PLUS loads.
    let SourceState::Card { cost, free, .. } = rig.ui.source_state() else {
        panic!("{:?}", rig.ui.source_state());
    };
    assert_eq!(cost, Cost::of(16 + heads.held_len()));
    assert!(cost <= free);
    rig.load();
    assert_heads(&rig, &b, heads.as_slice());
    let pool = rig.ui.project().pool();
    let id = pool.find_source(b.0.sref).unwrap();
    let f = rig.registry().get(id).expect("its stream row");
    assert_eq!(f.file(), source_file(b.0.sref));
    assert_eq!(f.frames().get(), b.0.sref.frames);
    assert_eq!(
        rig.sound().source_ref(),
        Some(b.0.sref),
        "the Part plays it"
    );
    // Streamed, the band gives its polyphony at ROOT (#698).
    assert_eq!(rig.band(), "BED · 24K · 7 STREAMS");
}

#[test]
fn too_big_loop_streams_with_two_heads() {
    let b = bed(2);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| {
        t.machine = TapeMachine::Loop;
        (t.size, t.position, t.xfade) = (0.7, 0.6, 0.1);
    });
    let heads = heads_for(b.0.sref, &rig.params()).unwrap();
    rig.turn(1);
    rig.load();
    assert_heads(&rig, &b, heads.as_slice());
    let p = rig.params();
    let frames = b.0.sref.frames;
    let ws = window_start(p.start, core::num::NonZeroU32::new(frames).unwrap());
    let rate = chimera_core::stream::units::SourceRate::new(24_000).unwrap();
    let lp = Bounds::of(
        ws,
        window_end(ws, p.end, frames, rate),
        Sizing {
            size: p.size,
            place: p.position,
            xfade: p.xfade,
        },
        rate,
    );
    let lh = heads.as_slice()[1];
    assert!(
        lp.lx() > 0 && lh.first() < lp.ls() - lp.lx(),
        "the loop head begins XFADE early"
    );
}

/// A source too big for the arena that the Part plays in a way that needs
/// it whole: refused with the Part's reason, nothing published.
fn refused(f: impl FnOnce(&mut TapeEngineParams), words: &str) {
    let b = bed(3);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(f);
    rig.turn(1);
    let pool = rig.ui.project().pool();
    let (free, count) = (pool.free(), pool.source_count());
    rig.load();
    assert_eq!(rig.band(), words);
    let pool = rig.ui.project().pool();
    assert_eq!((pool.free(), pool.source_count()), (free, count), "{words}");
    assert_eq!(
        pool.find_source(b.0.sref),
        None,
        "{words}: nothing published"
    );
}

#[test]
fn too_big_ping_refused() {
    refused(|t| t.machine = TapeMachine::Ping, PING_NEEDS_RAM);
}

#[test]
fn too_big_scrub_refused() {
    refused(|t| t.machine = TapeMachine::Scrub, "SCRUB NEEDS RAM");
}

#[test]
fn fragmented_refused() {
    let b = bed(4);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.s.fragmented = true;
    let free = rig.ui.project().pool().free();
    rig.load();
    assert_eq!(rig.band(), FRAGMENTED);
    assert_eq!(rig.ui.project().pool().free(), free, "the gap as it was");
    assert_eq!(rig.ui.project().pool().find_source(b.0.sref), None);
    assert!(
        SourceId::ALL
            .iter()
            .all(|&s| rig.registry().get(s).is_none())
    );
}

/// Review Focus 3: eight streams fill the registry; the ninth refuses
/// with STREAMS FULL and publishes nothing, and the eight play on.
#[test]
fn ninth_streamed_source_refuses() {
    let beds: Vec<_> = (10..19).map(bed).collect();
    let mut rig = Rig::new(&beds);
    rig.tape(|t| t.machine = TapeMachine::One);
    let mut streamed = Vec::new();
    for _ in 0..9 {
        rig.turn(100);
        let SourceState::Card { stem, .. } = rig.ui.source_state() else {
            panic!("{:?}", rig.ui.source_state());
        };
        rig.load();
        streamed.push(stem);
    }
    let pool = rig.ui.project().pool();
    let held: Vec<_> = beds.iter().filter(|b| pool.streamed(b.0.sref)).collect();
    assert_eq!(held.len(), 8);
    let ninth = beds
        .iter()
        .find(|b| b.0.sref.stem() == streamed[8])
        .unwrap();
    assert_eq!(pool.find_source(ninth.0.sref), None, "the ninth: nothing");
    assert_eq!(rig.band(), STREAMS_FULL);
}

/// Review Focus 2, ruled: a streamed load reads the file's head and its
/// heads, never the rest, and checks only their framing. Framing broken in
/// a head fails it, publishing nothing; a sample out of the heads it never
/// reads.
#[test]
fn streamed_load_trusts_the_seal() {
    let b = bed(5);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    let heads = heads_for(b.0.sref, &rig.params()).unwrap();
    let size = file_len(&b.0);
    rig.turn(1);
    rig.s.reads.clear();
    rig.load();
    assert_heads(&rig, &b, heads.as_slice());
    let read: u32 = rig.s.reads.iter().map(|r| r.1).sum();
    let want = heads.as_slice()[0].bytes().get();
    assert!(read <= 4_096 + want + 2 * 508, "read {read} of {size}");
    // A sample past the heads, damaged: never read, so it loads.
    let mut file = file_bytes(&b);
    let n = file.len();
    file[n - 100] ^= 0xFF;
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.s.inner.plant(source_file(b.0.sref), &file);
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.load();
    assert!(
        rig.ui.project().pool().streamed(b.0.sref),
        "the seal is trusted"
    );
    // A record's tag inside the head: framing, so the load fails.
    let mut file = file_bytes(&b);
    let first = 28 + 4 + 34;
    file[first + 508] ^= 0xFF;
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.s.inner.plant(source_file(b.0.sref), &file);
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.load();
    assert_eq!(rig.band(), READ_FAILED);
    assert_eq!(rig.ui.project().pool().find_source(b.0.sref), None);
    assert!(
        SourceId::ALL
            .iter()
            .all(|&s| rig.registry().get(s).is_none())
    );
}

/// The registry row goes in before the pool row is published: a full
/// registry refuses the load with nothing published; a row free, the load
/// publishes with its row there.
#[test]
fn registry_row_before_pool_row() {
    let b = bed(6);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    let reg = rig.registry();
    let other = bed(7).0;
    let l = SourceLayout::whole(other).unwrap();
    let len = source_file_min_len(other.sref).unwrap();
    let runs = synthetic_runs(source_file(other.sref), Bytes::new(len)).unwrap();
    let f = StreamFile::from_layout(&l, runs).unwrap();
    for n in 24..32 {
        reg.insert(SourceId::new(n).unwrap(), f).unwrap();
    }
    rig.turn(1);
    rig.load();
    assert_eq!(rig.band(), STREAMS_FULL);
    let pool = rig.ui.project().pool();
    assert_eq!(pool.find_source(b.0.sref), None);
    assert_eq!(pool.source_count(), 0, "no pool row");
    // SAFETY: no voice or card read has these test rows.
    assert!(unsafe { reg.clear(SourceId::new(31).unwrap()) });
    rig.turn(1);
    rig.load();
    let pool = rig.ui.project().pool();
    let id = pool.find_source(b.0.sref).expect("published");
    assert_eq!(
        reg.get(id).map(StreamFile::file),
        Some(source_file(b.0.sref))
    );
}

/// START turned on a streamed source: its heads move 200 ms after the
/// last turn, while no note holds it; a turn before then waits again.
#[test]
fn reheading_after_start_turn() {
    let b = bed(8);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.load();
    let before = heads_for(b.0.sref, &rig.params()).unwrap();
    assert_heads(&rig, &b, before.as_slice());
    let frames_200ms = (200 / FRAME_MS) as usize;
    rig.tape(|t| t.start = 0.3);
    for _ in 0..frames_200ms - 1 {
        rig.frame();
    }
    // A turn before the 200 ms: the wait starts again.
    rig.tape(|t| t.start = 0.5);
    for _ in 0..frames_200ms - 1 {
        rig.frame();
    }
    assert_heads(&rig, &b, before.as_slice());
    let after = heads_for(b.0.sref, &rig.params()).unwrap();
    assert_ne!(after, before);
    for _ in 0..20 {
        rig.frame();
    }
    assert_heads(&rig, &b, after.as_slice());
    let id = rig.ui.project().pool().find_source(b.0.sref).unwrap();
    assert!(rig.registry().get(id).is_some(), "its stream row stays");
    // The old heads are a pending hole, closed by the next pass.
    for _ in 0..4 {
        rig.frame();
    }
    let (_, _, pending) = rig.ui.project().pool().split();
    assert_eq!(pending, Cost::default(), "the old heads' hole closed");
}

/// #598: a note holding a streamed source reads its old heads every block,
/// never cut, while the turn's new heads go in beside them under a free
/// id on the same stream file, so a note struck now finds them at once
/// rather than waiting for the held note to end. Once it lets go, the old
/// heads drain: their pool slot, stream row and hole all freed. The audio
/// runs on its own thread, answering the gate as the chip's does.
#[test]
fn reheading_goes_beside_a_held_note() {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    fn now_ms() -> u32 {
        static T0: OnceLock<Instant> = OnceLock::new();
        T0.get_or_init(Instant::now).elapsed().as_millis() as u32
    }
    let b = bed(9);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.load();
    let before = heads_for(b.0.sref, &rig.params()).unwrap();
    let id = rig.ui.project().pool().find_source(b.0.sref).unwrap();
    rig.ui.project_mut().set_gate_clock(now_ms);
    let mut tok = rig.ui.project().pool().table().reader().unwrap();
    let (held, stop) = (
        Arc::new(AtomicU32::new(id.bit())),
        Arc::new(AtomicBool::new(false)),
    );
    // The head table the held note reads, each block it holds `id`.
    let reads = Arc::new(Mutex::new(Vec::<Vec<HeadSpec>>::new()));
    let audio = std::thread::spawn({
        let (held, stop, reads) = (held.clone(), stop.clone(), reads.clone());
        move || {
            while !stop.load(Ordering::Relaxed) {
                let h = held.load(Ordering::Relaxed);
                {
                    let v = tok.begin_block();
                    if h != 0 {
                        let t = v.bytes(id).and_then(head_table);
                        let t = t.map(|t| t.iter().collect()).unwrap_or_default();
                        reads.lock().unwrap().push(t);
                    }
                }
                tok.store_sounding(h);
                std::thread::sleep(Duration::from_micros(500));
            }
        }
    });
    let table = rig.ui.project().pool().table();
    while table.sounding() != id.bit() {
        std::thread::yield_now();
    }
    rig.tape(|t| t.start = 0.5);
    let after = heads_for(b.0.sref, &rig.params()).unwrap();
    let staged = |rig: &Rig| {
        head_table(rig.ui.project().pool().held(b.0.sref).unwrap())
            .unwrap()
            .iter()
            .collect::<Vec<_>>()
    };
    for _ in 0..200 {
        if staged(&rig) == after.as_slice() {
            break;
        }
        rig.frame();
    }
    assert_heads(&rig, &b, after.as_slice());
    let pool = rig.ui.project().pool();
    let new = pool.find_source(b.0.sref).unwrap();
    assert_ne!(new, id, "beside it, under a free id");
    assert_eq!(pool.source(id), None, "the old id retired");
    let reg = rig.registry();
    assert_eq!(
        reg.get(new).map(StreamFile::file),
        Some(source_file(b.0.sref))
    );
    assert!(reg.get(id).is_some(), "the held note's stream row stays");
    // Held on a while: still the old heads, and their row stays.
    while reads.lock().unwrap().len() < 100 {
        rig.frame();
        std::thread::sleep(Duration::from_micros(200));
    }
    assert!(reg.get(id).is_some());
    held.store(0, Ordering::Relaxed);
    while table.sounding() != 0 {
        std::thread::yield_now();
    }
    for _ in 0..200 {
        if reg.get(id).is_none() && rig.ui.project().pool().split().2 == Cost::default() {
            break;
        }
        rig.frame();
        // The drain waits two audio blocks with the id let go.
        std::thread::sleep(Duration::from_micros(500));
    }
    stop.store(true, Ordering::Relaxed);
    audio.join().unwrap();
    let reads = reads.lock().unwrap();
    assert!(reads.len() >= 100, "{} blocks held", reads.len());
    assert!(
        reads.iter().all(|t| t.as_slice() == before.as_slice()),
        "the held note read its old heads every block"
    );
    assert!(reg.get(id).is_none(), "the old stream row freed");
    let (_, _, pending) = rig.ui.project().pool().split();
    assert_eq!(pending, Cost::default(), "the old heads' hole closed");
    assert_heads(&rig, &b, after.as_slice());
}

/// One source a note holds doesn't stop another's heads moving: both
/// re-stage, the held one beside its note (#598).
#[test]
fn a_held_source_doesnt_stop_anothers_reheading() {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};
    fn now_ms() -> u32 {
        static T0: OnceLock<Instant> = OnceLock::new();
        T0.get_or_init(Instant::now).elapsed().as_millis() as u32
    }
    let (a, b) = (bed(20), bed(21));
    let mut rig = Rig::new(&[a.clone(), b.clone()]);
    rig.tape(|t| t.machine = TapeMachine::One);
    for _ in 0..2 {
        rig.turn(100);
        rig.load();
    }
    let pool = rig.ui.project().pool();
    assert!(pool.streamed(a.0.sref) && pool.streamed(b.0.sref));
    let ia = pool.find_source(a.0.sref).unwrap();
    // Part 1 plays A, Part 2 B.
    let p2 = PartId::ALL[1];
    let mut snd = rig.sound().clone();
    snd.source = Some(TapeSource::from(b.0.sref));
    **rig.ui.project_mut().edit_part(p2).sound = snd;
    rig.ui.project_mut().edit_part(P1).sound.source = Some(TapeSource::from(a.0.sref));
    let before = heads_for(a.0.sref, &rig.params()).unwrap();
    assert_heads(&rig, &a, before.as_slice());
    rig.ui.project_mut().set_gate_clock(now_ms);
    let mut tok = rig.ui.project().pool().table().reader().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let held = Arc::new(AtomicU32::new(ia.bit()));
    let audio = std::thread::spawn({
        let (held, stop) = (held.clone(), stop.clone());
        move || {
            while !stop.load(Ordering::Relaxed) {
                let _ = tok.begin_block();
                tok.store_sounding(held.load(Ordering::Relaxed));
                std::thread::sleep(Duration::from_micros(500));
            }
        }
    });
    let table = rig.ui.project().pool().table();
    while table.sounding() != ia.bit() {
        std::thread::yield_now();
    }
    rig.tape(|t| t.start = 0.5);
    rig.ui.project_mut().edit_part(p2).sound.params.tape.start = 0.5;
    let after = heads_for(b.0.sref, &rig.ui.project().part(p2).sound.params.tape).unwrap();
    let after_a = heads_for(a.0.sref, &rig.params()).unwrap();
    let moved = |rig: &Rig, src: &(SourceHead, Vec<u8>), want: &[HeadSpec]| {
        let t = head_table(rig.ui.project().pool().held(src.0.sref).unwrap()).unwrap();
        t.iter().eq(want.iter().copied())
    };
    for _ in 0..200 {
        if moved(&rig, &b, after.as_slice()) && moved(&rig, &a, after_a.as_slice()) {
            break;
        }
        rig.frame();
    }
    stop.store(true, Ordering::Relaxed);
    audio.join().unwrap();
    assert_heads(&rig, &b, after.as_slice());
    // A's held note keeps its old heads; A's new ones went in beside (#598).
    assert_ne!(after_a, before);
    assert_heads(&rig, &a, after_a.as_slice());
    assert!(rig.registry().get(ia).is_some(), "A's held row stays");
}

// Card read faults: the UI pass re-inits and mounts.

/// `b` loaded streamed on Part 1 at PLAY ONE.
fn streamed_rig(b: &(SourceHead, Vec<u8>)) -> Rig {
    let mut rig = Rig::new(std::slice::from_ref(b));
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.load();
    assert!(rig.ui.project().pool().streamed(b.0.sref));
    rig
}

/// While the card is faulted, a streamed source's band reads CARD READ
/// FAILED: a re-init that fails leaves it so, tried once; one the bus was
/// too busy for is tried again; one that mounts clears it.
#[test]
fn focus_band_reads_card_read_failed() {
    let b = bed(3);
    let mut rig = streamed_rig(&b);
    let fine = rig.band();
    assert_ne!(fine, READ_FAILED);

    (rig.s.fault, rig.s.recovery) = (CardFault::Read, Err(StoreError::Io));
    rig.work();
    assert_eq!(rig.band(), READ_FAILED);
    rig.work();
    assert_eq!(rig.s.recovers, 1, "a failed re-init isn't retried");
    assert_eq!(rig.band(), READ_FAILED);

    // A mount by any card op clears the fault, and the band.
    rig.s.fault = CardFault::Clear;
    rig.work();
    assert_eq!(rig.band(), fine);

    (rig.s.fault, rig.s.recovery) = (CardFault::Read, Err(StoreError::Busy));
    rig.work();
    assert_eq!(rig.band(), READ_FAILED);
    rig.s.recovery = Ok(());
    rig.work();
    assert_eq!(rig.s.recovers, 3, "busy: tried again");
    assert_eq!(rig.s.fault, CardFault::Clear);
    assert_eq!(rig.band(), fine);
}

/// A RAM source plays through a fault: its band is its own.
#[test]
fn a_held_source_reads_through_a_fault() {
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    rig.load();
    let fine = rig.band();
    (rig.s.fault, rig.s.recovery) = (CardFault::Read, Err(StoreError::Io));
    rig.work();
    assert_eq!(rig.band(), fine);
}

/// The re-init mounts another card: the stream registry empties and the
/// streamed source reads SOURCE MISSING.
#[test]
fn volume_change_clears_registry() {
    let b = bed(4);
    let mut rig = streamed_rig(&b);
    let id = rig.ui.project().pool().find_source(b.0.sref).unwrap();
    assert!(rig.registry().get(id).is_some());
    rig.s.fault = CardFault::Read;
    rig.s.inner.swap(2);
    rig.work();
    assert_eq!(rig.s.fault, CardFault::Clear, "mounted");
    assert!(rig.registry().get(id).is_none(), "its row gone");
    assert_eq!(rig.band(), SOURCE_MISSING);
}

/// The same card mounts again: its rows stay.
#[test]
fn same_card_keeps_its_rows() {
    let b = bed(5);
    let mut rig = streamed_rig(&b);
    let id = rig.ui.project().pool().find_source(b.0.sref).unwrap();
    let fine = rig.band();
    rig.s.fault = CardFault::Read;
    rig.work();
    assert!(rig.registry().get(id).is_some());
    assert_eq!(rig.band(), fine);
}

/// #609: a file the card condemned whole reads `BAD FILE · DROPS OUT` on
/// the focus band and `BAD FILE` in its cell, each inside its width.
#[test]
fn a_condemned_file_says_so() {
    use chimera_core::ui::draw::text_width;
    use chimera_core::ui::source_cell::FILE_DAMAGED;
    use chimera_core::ui::theme::{CELL_COL_W, CONTENT_W, FONT_VALUE, LABEL_TRACKING};
    let b = bed(30);
    let rig = streamed_rig(&b);
    let id = rig.ui.project().pool().find_source(b.0.sref).unwrap();
    assert_ne!(rig.band(), FILE_DAMAGED);
    rig.registry().hold(id).unwrap().mark_damaged();
    assert_eq!(rig.band(), FILE_DAMAGED);
    let mut cell = chimera_core::ui::fmt::FmtBuf::new();
    rig.ui.source_state().cell(&mut cell);
    assert_eq!(cell.as_str(), "BAD FILE");
    let band = text_width(&FONT_VALUE, FILE_DAMAGED, LABEL_TRACKING);
    assert!(band <= CONTENT_W, "band {band}px over {CONTENT_W}");
    let w = text_width(&FONT_VALUE, cell.as_str(), 0);
    assert!(w <= CELL_COL_W, "cell {w}px over {CELL_COL_W}");
}

/// #598: with every stream row taken, a held source's heads can't go in
/// beside it, so the re-stage waits (no card reads meanwhile); a row
/// freed, they go in beside it.
#[test]
fn reheading_waits_when_nothing_is_free_beside() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};
    fn now_ms() -> u32 {
        static T0: OnceLock<Instant> = OnceLock::new();
        T0.get_or_init(Instant::now).elapsed().as_millis() as u32
    }
    let b = bed(31);
    let mut rig = Rig::new(std::slice::from_ref(&b));
    rig.tape(|t| t.machine = TapeMachine::One);
    rig.turn(1);
    rig.load();
    let before = heads_for(b.0.sref, &rig.params()).unwrap();
    let id = rig.ui.project().pool().find_source(b.0.sref).unwrap();
    // The other seven rows taken.
    let reg = rig.registry();
    let f = *reg.get(id).unwrap();
    for n in 24..31 {
        reg.insert(SourceId::new(n).unwrap(), f).unwrap();
    }
    rig.ui.project_mut().set_gate_clock(now_ms);
    let mut tok = rig.ui.project().pool().table().reader().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let audio = std::thread::spawn({
        let stop = stop.clone();
        move || {
            while !stop.load(Ordering::Relaxed) {
                let _ = tok.begin_block();
                tok.store_sounding(id.bit());
                std::thread::sleep(Duration::from_micros(500));
            }
        }
    });
    let table = rig.ui.project().pool().table();
    while table.sounding() != id.bit() {
        std::thread::yield_now();
    }
    rig.tape(|t| t.start = 0.5);
    let reads = rig.s.reads.len();
    for _ in 0..40 {
        rig.frame();
    }
    assert_heads(&rig, &b, before.as_slice());
    assert_eq!(rig.s.reads.len(), reads, "no card reads while it waits");
    // SAFETY: no voice or card read has these test rows.
    assert!(unsafe { reg.clear(SourceId::new(30).unwrap()) });
    let after = heads_for(b.0.sref, &rig.params()).unwrap();
    for _ in 0..200 {
        let t = head_table(rig.ui.project().pool().held(b.0.sref).unwrap()).unwrap();
        if t.iter().eq(after.as_slice().iter().copied()) {
            break;
        }
        rig.frame();
    }
    stop.store(true, Ordering::Relaxed);
    audio.join().unwrap();
    assert_heads(&rig, &b, after.as_slice());
    assert_ne!(rig.ui.project().pool().find_source(b.0.sref), Some(id));
}
