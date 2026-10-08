//! TAPE's SOURCE cell (spec § 3, ADR 0093, TAPE Task 17): turned through
//! the pool's sources, a divider, then the card's; MIX + PLUS loads a card
//! row 4 KiB a UI pass behind BUSY, and MENU cancels it; per-source memory
//! in RAM. The Digitakt's SAMPLE SLOT, with the Octatrack's audio pool.

mod common;
mod screen;

use core::ops::ControlFlow;

use chimera_core::dsp::tape_engine::read::Shape;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::Cost;
use chimera_core::project::{
    LoadLink, PartId, ProjectSource, ReplaceGuard, SourceRef, project_crc,
};
use chimera_core::storage::{
    Card, SourceHead, SourceLayout, SystemSettings, SystemSync, TapeSource, WholeShape,
    held_for_file_len, source_crc, source_file_min_len, write_descriptor, write_source,
};
use chimera_core::ui::UiState;
use chimera_core::ui::meter::MeterView;
use chimera_core::ui::settings::CardCx;
use chimera_core::ui::source_cell::{
    Memo, READ_FAILED, SourceMemo, SourceRow, card_cost, rows, wont_fit_words,
};
use chimera_core::ui::view::SourceState;
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, load_init, tap};

const P1: PartId = PartId::ALL[0];

/// A plain source of `frames` at 24 kHz.
fn source(seed: u8, frames: u32, name: &str) -> (SourceHead, Vec<u8>) {
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
        (c.0.sref.crc, file_len(&c.0)),
        (a.0.sref.crc, file_len(&a.0)),
        (d.0.sref.crc, file_len(&d.0)),
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
                stem: c.0.sref.crc,
                cost: cost(&c.0)
            },
            SourceRow::Card {
                stem: d.0.sref.crc,
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

/// The console's sweep never borrows the gap a load is staging in: no
/// scratch from the load's first pass to its last.
#[test]
fn no_sweep_scratch_while_a_load_stages() {
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    assert!(rig.ui.sweep_scratch().is_some_and(|g| !g.is_empty()));
    rig.mix_plus();
    let mut passes = 0;
    while rig.ui.card_pending() {
        assert!(rig.ui.sweep_scratch().is_none(), "pass {passes}");
        rig.work();
        passes += 1;
    }
    assert!(passes > 1);
    assert!(rig.ui.sweep_scratch().is_some());
}

#[test]
fn loaded_row_moves_above_divider() {
    let (h, _) = choir();
    let mut rig = Rig::new(&[choir()]);
    rig.turn(1);
    rig.load();
    let pool = rig.ui.project().pool();
    let id = pool.find_source(h.sref).expect("published");
    let card = [(h.sref.crc, file_len(&h))];
    assert_eq!(rows(pool, &card).collect::<Vec<_>>(), [SourceRow::Pool(id)]);
    let s = rig.sound();
    assert_eq!(s.source_ref(), Some(h.sref), "the Part plays it");
    assert_eq!(s.params.tape.root, f32::from(h.root));
    assert_eq!(s.params.tape.period, 91.0);
    assert!(pool.source_view(h.sref).is_some());
}

#[test]
fn wont_fit_alerts() {
    let mut rig = Rig::new(&[choir()]);
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
    let before = MeterView::of(rig.ui.project().pool(), None);
    rig.mix_plus();
    assert!(!rig.ui.card_pending(), "loads nothing");
    assert_eq!(MeterView::of(rig.ui.project().pool(), None), before);
    // The band draws it in ALERT.
    let fb = screen::render_ui(&rig.ui);
    let alert = chimera_core::ui::theme::ALERT;
    let y = chimera_core::ui::hint::FOCUS_Y;
    assert!((y - 8..=y).any(|y| (12..200).any(|x| fb.at(x, y) == alert)));
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
    assert_eq!(m.get(1), None, "the oldest went");
    assert_eq!(
        m.get(2),
        Some(Memo {
            crc: core::num::NonZeroU32::new(2).unwrap(),
            start: 0.02,
            end: 1.0,
            pitch: 0,
            fine: 0
        })
    );
    assert!(m.get(33).is_some());

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
