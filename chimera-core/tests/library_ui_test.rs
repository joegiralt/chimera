//! The Sound library in the UI (library spec § Browser, § Save, § Load):
//! the PROJECT/LIBRARY tab on B, the card jobs behind BUSY, the guard's
//! prompt, NAMING and SETTINGS › PART › SAVE TO LIBRARY; a pending load
//! blocks every other card start, and MENU cancels it.

mod common;
mod screen;

use chimera_core::dsp::tape_engine::read::{DESC_LEN, Shape};
use chimera_core::factory;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::set_free_to;
use chimera_core::project::{
    LoadLink, PartId, PartStatus, ProjectStatus, SourceRef, part_status, save_library_sound,
};
use chimera_core::storage::{
    BuildStamp, Card, FileKind, Generation, Header, LibId, RecordTag, SourceHead, SystemSettings,
    SystemSync, WholeShape, encode_library_sound, source_crc, write_file, write_source,
};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::library::Tab;
use chimera_core::ui::nav::Location;
use chimera_core::ui::settings::part::Offer;
use chimera_core::ui::settings::prompt::LibraryAnswer;
use chimera_core::ui::settings::{AskKind, CardCx};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use common::codec_util::{VecSink, fix_crc, record_offsets};
use common::tape::{held_plain, sine};
use screen::{Input, feed, tap};

const P1: PartId = PartId::ALL[0];
const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);

/// MemStore, its reads and listings counted.
struct Counted {
    inner: MemStore,
    reads: usize,
    /// TAPES listings so far; past `hide_tapes` they list nothing.
    tapes: usize,
    hide_tapes: usize,
    /// Reads past this many fail with `Io`.
    fail_after: usize,
}

impl Store for Counted {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }
    fn list(
        &mut self,
        v: VolumeId,
        d: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.reads += 1;
        if d == Dir::Tapes {
            self.tapes += 1;
            if self.tapes > self.hide_tapes {
                return Ok(());
            }
        }
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
        self.reads += 1;
        if self.reads > self.fail_after {
            return Err(StoreError::Io);
        }
        self.inner.read_at(v, f, at, s)
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
    st: Counted,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl Rig {
    fn new() -> Rig {
        let mut st = Counted {
            inner: MemStore::new(1),
            reads: 0,
            tapes: 0,
            hide_tapes: usize::MAX,
            fail_after: usize::MAX,
        };
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut st);
        Rig {
            ui: Box::new(UiState::new()),
            st,
            card,
            sync,
            set,
            link: LoadLink::new(),
        }
    }

    /// One pass of card work.
    fn one_pass(&mut self) {
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.st,
            sync: &mut self.sync,
            settings: &mut self.set,
        };
        let _ = self.ui.card_work(cx, link, |swap, _| {
            let _ = swap.settle(link, || false);
        });
    }

    /// Passes until no card work is pending.
    fn work(&mut self) {
        for _ in 0..200 {
            if !self.ui.card_pending() {
                return;
            }
            self.one_pass();
        }
        panic!("card work never ended");
    }

    fn feed(&mut self, i: Input) {
        feed(&mut self.ui, i);
    }

    fn press(&mut self, b: ButtonId) {
        self.feed(Input::press(b));
    }

    fn turn(&mut self, e: EncoderId, d: i8) {
        self.feed(Input::turn(e, d));
    }

    /// Part 1's Sound rung, the LIBRARY tab, TAPE Sounds listed (no
    /// player sees them: ADR 0132).
    fn open_library(&mut self) {
        self.ui.list_tape_for_test();
        self.feed(Input::chord(ButtonId::Edit, ButtonId::B1));
        self.turn(EncoderId::B, 1);
        assert_eq!(self.ui.lib_tab(), Tab::Library);
    }

    fn toast(&mut self) -> String {
        match self.ui.step_toast(0) {
            ToastStep::Show(l) => l.as_str().into(),
            s => panic!("no toast: {s:?}"),
        }
    }

    fn reads(&self) -> usize {
        self.st.reads
    }

    fn sounds(&mut self) -> Vec<u32> {
        let v = self.st.inner.mount().unwrap();
        let mut ids = Vec::new();
        let _ = self.st.inner.list(v, Dir::Sounds, &mut |f, _| {
            if f.ext() == b"A" {
                let stem = core::str::from_utf8(f.stem()).unwrap();
                ids.push(stem[1..].parse().unwrap());
            }
        });
        ids.sort_unstable();
        ids
    }

    /// `s` saved to the library by the engine, as another build would.
    fn library_with(&mut self, s: &Sound) -> LibId {
        save_library_sound(&mut Card::new(), &mut self.st.inner, s, s.name, &ST)
            .out
            .unwrap()
    }

    /// `bytes` as `S<id>.A`.
    fn put_raw(&mut self, id: u32, bytes: &[u8]) {
        let v = self.st.inner.mount().unwrap();
        let _ = self.st.inner.make_dir(v, Dir::Chimera);
        let _ = self.st.inner.make_dir(v, Dir::Sounds);
        let id = LibId::new(id).unwrap();
        let f = FileName::new(Dir::Sounds, &id.stem(), b"A").unwrap();
        self.st
            .inner
            .write(v, f, &mut |s: &mut dyn ByteSink| s.put(bytes))
            .unwrap();
    }

    /// The LIBRARY row under the cursor, as its id.
    fn cursor_id(&self) -> Option<u32> {
        self.ui.library_cursor_id_for_test().map(LibId::get)
    }

    /// EDIT on LIBRARY's first row, then the job.
    fn start_load(&mut self) {
        self.open_library();
        self.work();
        self.press(ButtonId::Edit);
    }

    fn answer(&mut self, a: LibraryAnswer) {
        let i = [
            LibraryAnswer::SavePartFirst,
            LibraryAnswer::Replace,
            LibraryAnswer::Cancel,
        ]
        .iter()
        .position(|&x| x == a)
        .unwrap();
        self.turn(EncoderId::A, i as i8);
        tap(&mut self.ui, ButtonId::Seq);
        assert!(!self.ui.prompt_open());
    }

    fn edit_part(&mut self) {
        self.ui
            .project_mut()
            .edit_part(P1)
            .sound
            .params
            .filter
            .cutoff *= 0.5;
    }

    fn counts(&self) -> (usize, usize) {
        let pool = self.ui.project().pool();
        (pool.source_count(), pool.sound_count())
    }
}

/// Factory Sound `i` renamed, so NEW's copy of it doesn't dedupe.
fn factory_sound(i: usize) -> Sound {
    let mut s = factory::factory_sound(i).unwrap();
    s.name = SoundName::new(&format!("LIB {i}")).unwrap();
    s
}

const FRAMES: u32 = 9_600;

/// A Plain sine source of `period`, its head and samples.
fn source(period: f32) -> (SourceHead, Vec<u8>) {
    let data = held_plain(&sine(48_000, period, FRAMES), 48_000)[DESC_LEN..].to_vec();
    let h = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames: FRAMES,
            rate: 48_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("SINE").unwrap(),
    };
    (h, data)
}

/// A TAPE Sound on source `period` saved, its `.SRC` on the card.
fn tape_library(r: &mut Rig, period: f32, name: &str) -> SourceRef {
    let (h, data) = source(period);
    Card::new()
        .run(&mut r.st.inner, |s, rd| write_source(s, rd, &h, &data))
        .unwrap()
        .result
        .unwrap();
    let mut s = Sound::init(EngineType::Tape);
    s.pick_source(&h, None);
    s.name = SoundName::new(name).unwrap();
    r.library_with(&s);
    h.sref
}

/// An Algo INIT stamped, with an unknown record after its routes: one
/// skip on load.
fn skip_file() -> Vec<u8> {
    let s = Sound::init(EngineType::Algo);
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(SoundName::new("SKIPS").unwrap()),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_library_sound(&s, &ST, w)).unwrap();
    let mut f = sink.0;
    let o = record_offsets(&f);
    let i = o[..o.len() - 1]
        .iter()
        .position(|&p| u16::from_le_bytes([f[p], f[p + 1]]) == RecordTag::Routes.code())
        .unwrap();
    let mut rec = 0x0070u16.to_le_bytes().to_vec();
    rec.extend(4u16.to_le_bytes());
    rec.extend([0; 4]);
    f.splice(o[i + 1]..o[i + 1], rec);
    fix_crc(&mut f);
    f
}

#[test]
fn b_turns_between_project_and_library_and_lists_once() {
    let mut r = Rig::new();
    assert_eq!(r.ui.lib_tab(), Tab::Project);
    r.open_library();
    let reads = r.reads();
    assert!(r.ui.card_pending(), "LIBRARY on screen lists");
    r.work();
    assert!(r.reads() > reads);
    r.turn(EncoderId::B, -1);
    assert_eq!(r.ui.lib_tab(), Tab::Project);
    r.turn(EncoderId::B, 1);
    assert!(!r.ui.card_pending(), "cached");
    let reads = r.reads();
    r.work();
    assert_eq!(r.reads(), reads);
}

#[test]
fn mix_edit_in_library_names_then_saves() {
    let mut r = Rig::new();
    r.open_library();
    r.work();
    r.feed(Input::chord(ButtonId::Mix, ButtonId::Edit));
    assert_eq!(r.ui.naming_title_for_test(), Some("SOUND NAME"));
    let status = r.ui.project_status();
    let part = r.ui.project().part(P1).sound.name;
    tap(&mut r.ui, ButtonId::Seq);
    r.work();
    assert_eq!(r.toast(), "SAVED TO LIBRARY");
    assert_eq!(r.sounds(), vec![1]);
    assert_eq!(r.ui.project_status(), status);
    assert_eq!(r.ui.project().part(P1).sound.name, part);
    assert!(r.ui.location().browse().is_some(), "back on the browser");
    assert_eq!(r.cursor_id(), Some(1));
}

#[test]
fn menu_cancels_library_naming_back_to_the_browser() {
    let mut r = Rig::new();
    r.open_library();
    r.work();
    r.feed(Input::chord(ButtonId::Mix, ButtonId::Edit));
    tap(&mut r.ui, ButtonId::Menu);
    assert!(r.ui.naming().is_none());
    assert!(r.ui.location().browse().is_some());
    assert!(!r.ui.card_pending());
    assert!(r.sounds().is_empty());
}

#[test]
fn save_to_library_row_in_settings_part() {
    let mut ui = UiState::new();
    screen::to_leaf(&mut ui, &["PART"]);
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.naming_title_for_test(), Some("SOUND NAME"));
    let crumbs = ui.crumbs().unwrap().to_string();
    assert!(crumbs.ends_with("› SAVE TO LIB"), "{crumbs}");
}

#[test]
fn edit_loads_into_the_part_and_the_project_reads_modified() {
    let mut r = Rig::new();
    r.library_with(&factory_sound(0));
    r.open_library();
    r.work();
    let slot = r.ui.project().pool().first_free().unwrap();
    r.press(ButtonId::Edit);
    r.work();
    assert_eq!(r.toast(), format!("LOADED TO SLOT {}", slot.index() + 1));
    let engine = r.ui.project().part(P1).sound.engine();
    assert_eq!(r.ui.location(), Location::part_home(P1, engine));
    assert!(
        r.ui.project()
            .pool()
            .get(slot)
            .unwrap()
            .matches(&factory_sound(0))
    );
    let p = r.ui.project();
    assert_eq!(part_status(p.part(P1), p.pool()), PartStatus::Clean);
    assert_eq!(r.ui.project_status(), ProjectStatus::Modified);
}

#[test]
fn an_edited_part_asks_and_replace_loads() {
    let mut r = Rig::new();
    r.library_with(&factory_sound(0));
    r.edit_part();
    r.start_load();
    assert_eq!(r.ui.prompt_kind_for_test(), Some(AskKind::LoadLibrary));
    let (q, why) = r.ui.prompt_words_for_test().unwrap();
    assert_eq!(
        (q.as_str(), why.as_str()),
        ("LOAD INTO P1?", "P1 IS EDITED")
    );
    r.answer(LibraryAnswer::Replace);
    r.work();
    assert!(r.toast().starts_with("LOADED TO SLOT "));
}

#[test]
fn cancel_loads_nothing() {
    let mut r = Rig::new();
    r.library_with(&factory_sound(0));
    r.edit_part();
    r.start_load();
    let counts = r.counts();
    r.answer(LibraryAnswer::Cancel);
    assert!(!r.ui.card_pending());
    assert_eq!(r.counts(), counts);
}

#[test]
fn save_part_first_that_takes_the_room_refuses_and_keeps_the_save() {
    let mut r = Rig::new();
    r.library_with(&factory_sound(0));
    r.edit_part();
    let edited = r.ui.project().part(P1).sound.clone();
    let needs = Offer::of(r.ui.project(), P1).save_needs();
    set_free_to(r.ui.project_mut(), needs);
    r.start_load();
    r.answer(LibraryAnswer::SavePartFirst);
    r.work();
    let p = r.ui.project();
    assert_eq!(
        part_status(p.part(P1), p.pool()),
        PartStatus::Clean,
        "saved"
    );
    assert!(p.part(P1).sound.bits_eq(&edited), "not replaced");
    let why = r.ui.refusal().expect("POOL FULL in the band");
    assert!(why.starts_with("NEEDS "), "{why}");
}

#[test]
fn a_midi_edit_during_the_source_read_refuses() {
    let mut r = Rig::new();
    tape_library(&mut r, 100.0, "TAPE");
    r.start_load();
    r.one_pass(); // the read; the stream starts
    r.one_pass(); // a chunk
    assert!(r.ui.card_pending(), "still streaming");
    let counts = r.counts();
    // A MIDI CC on Part 1.
    r.ui.project_mut().edit_part(P1).sound.params.out.volume = 0.1;
    r.work();
    assert_eq!(r.toast(), "PART CHANGED");
    assert_eq!(r.counts(), counts, "no source, no slot");
}

#[test]
fn a_tape_load_streams_its_source_then_adopts() {
    let mut r = Rig::new();
    let sref = tape_library(&mut r, 100.0, "TAPE");
    r.start_load();
    let (srcs, snds) = r.counts();
    r.work();
    assert!(r.toast().starts_with("LOADED TO SLOT "));
    assert_eq!(r.counts(), (srcs + 1, snds + 1));
    let pool = r.ui.project().pool();
    assert!(pool.find_source(sref).is_some());
    assert_eq!(r.ui.project().part(P1).sound.source_ref(), Some(sref));
}

#[test]
fn a_source_not_on_the_card_loads_absent_and_says_so() {
    let mut r = Rig::new();
    let (h, _) = source(100.0);
    let mut s = Sound::init(EngineType::Tape);
    s.pick_source(&h, None);
    r.library_with(&s);
    r.start_load();
    r.work();
    assert_eq!(r.toast(), "SOURCE MISSING");
    assert_eq!(r.ui.project().part(P1).sound.source_ref(), Some(h.sref));
}

#[test]
fn a_pending_load_blocks_every_other_card_start() {
    let mut r = Rig::new();
    let first = tape_library(&mut r, 100.0, "ONE");
    tape_library(&mut r, 50.0, "TWO");
    r.open_library();
    r.work();
    // Newest first: TWO, then ONE.
    r.turn(EncoderId::A, 1);
    r.press(ButtonId::Edit);
    r.one_pass();
    assert!(r.ui.card_pending(), "streaming");
    // A second EDIT on the other row, LOAD PROJECT's listing, a SOURCE
    // card row: none queues.
    r.turn(EncoderId::A, -1);
    r.press(ButtonId::Edit);
    r.ui.queue_load_list_for_test();
    r.ui.start_source_list_for_test(P1);
    assert!(!r.ui.job_queued_for_test());
    r.work();
    // A SOURCE job would have taken the slot the stream is in, failing it.
    assert!(r.toast().starts_with("LOADED TO SLOT "));
    assert_eq!(r.ui.project().part(P1).sound.source_ref(), Some(first));
}

#[test]
fn a_source_load_blocks_a_library_load() {
    let mut r = Rig::new();
    r.library_with(&factory_sound(0));
    r.open_library();
    r.work();
    r.ui.start_source_list_for_test(P1);
    assert!(r.ui.src_busy_for_test());
    r.ui.library_edit_for_test();
    assert!(!r.ui.job_queued_for_test(), "refused while SOURCE works");
}

#[test]
fn menu_during_the_source_read_cancels() {
    let mut r = Rig::new();
    tape_library(&mut r, 100.0, "TAPE");
    r.start_load();
    r.one_pass();
    assert!(r.ui.card_pending());
    let counts = r.counts();
    tap(&mut r.ui, ButtonId::Menu);
    assert_eq!(r.toast(), "CANCELLED");
    assert!(!r.ui.in_settings(), "MENU's tap only cancels");
    r.work();
    assert_eq!(r.counts(), counts);
}

#[test]
fn skipped_params_say_so() {
    let mut r = Rig::new();
    r.put_raw(1, &skip_file());
    r.start_load();
    r.work();
    assert_eq!(r.toast(), "LOADED · SOME PARAMS SKIPPED");
}

#[test]
fn a_file_gone_since_listed_refuses_and_relists() {
    let mut r = Rig::new();
    let id = r.library_with(&factory_sound(0));
    r.open_library();
    r.work();
    let v = r.st.inner.mount().unwrap();
    let f = FileName::new(Dir::Sounds, &id.stem(), b"A").unwrap();
    r.st.inner.delete(v, f).unwrap();
    let counts = r.counts();
    r.press(ButtonId::Edit);
    r.work();
    assert_eq!(r.toast(), "FILE IS GONE");
    assert_eq!(r.counts(), counts);
    assert_eq!(r.cursor_id(), None, "re-listed: empty");
}

#[test]
fn mix_minus_acts_in_project_only() {
    let mut r = Rig::new();
    let p = r.ui.project();
    let unused = chimera_core::project::SlotId::ALL
        .into_iter()
        .find(|&s| p.pool().get(s).is_some() && p.users(s).is_empty())
        .unwrap();
    r.feed(Input::chord(ButtonId::Edit, ButtonId::B1));
    r.turn(EncoderId::A, unused.index() as i8);
    r.turn(EncoderId::B, 1);
    r.work();
    r.feed(Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert!(!r.ui.prompt_open(), "LIBRARY: nothing");
    r.turn(EncoderId::B, -1);
    r.feed(Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(r.ui.prompt_kind_for_test(), Some(AskKind::ClearSlot));
}

#[test]
fn a_forgotten_page_relists_on_the_next_pass() {
    let mut r = Rig::new();
    r.open_library();
    r.work();
    r.ui.forget_library();
    let reads = r.reads();
    r.work();
    assert!(r.reads() > reads);
}

#[test]
fn a_lowest_free_save_lands_on_its_page() {
    let mut r = Rig::new();
    let s = factory_sound(1);
    r.put_raw(1, &stamped(&s));
    r.put_raw(LibId::USER_MAX, &stamped(&s));
    r.open_library();
    r.work();
    r.feed(Input::chord(ButtonId::Mix, ButtonId::Edit));
    tap(&mut r.ui, ButtonId::Seq);
    r.work();
    assert_eq!(r.toast(), "SAVED TO LIBRARY");
    assert_eq!(r.cursor_id(), Some(2));
}

fn stamped(s: &Sound) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_library_sound(s, &ST, w)).unwrap();
    sink.0
}

#[test]
fn a_failed_listing_says_why_and_relists_on_the_next_entry() {
    let mut r = Rig::new();
    r.library_with(&factory_sound(0));
    r.st.inner.eject();
    r.open_library();
    r.work();
    assert_eq!(r.ui.library_err_for_test(), Some("NO CARD"));
    assert!(!r.ui.card_pending(), "not retried every pass");
    r.st.inner.insert();
    r.turn(EncoderId::B, -1);
    r.turn(EncoderId::B, 1);
    let reads = r.reads();
    r.work();
    assert!(r.reads() > reads);
    assert_eq!(r.ui.library_err_for_test(), None);
    assert_eq!(r.cursor_id(), Some(1));
}

#[test]
fn a_source_gone_before_its_stream_loads_absent() {
    let mut r = Rig::new();
    let sref = tape_library(&mut r, 100.0, "TAPE");
    r.open_library();
    r.work();
    // The read's TAPES listing sees it; the stream's doesn't.
    r.st.tapes = 0;
    r.st.hide_tapes = 1;
    r.press(ButtonId::Edit);
    r.work();
    assert_eq!(r.toast(), "SOURCE MISSING");
    let p = r.ui.project();
    assert_eq!(p.part(P1).sound.source_ref(), Some(sref));
    let id = p.pool().find_source(sref).unwrap();
    assert!(p.pool().source(id).unwrap().absent());
}

#[test]
fn save_part_first_asks_about_stale_parts_after_the_load() {
    use chimera_core::project::{PartFrom, PartSource, ReplaceGuard};
    let mut r = Rig::new();
    tape_library(&mut r, 100.0, "TAPE");
    // P2 plays P1's slot: a save over it leaves P2 Stale.
    let slot = r.ui.project().pool().first_free().unwrap();
    let t = r.ui.template();
    let p = r.ui.project_mut();
    p.pool_store(slot, &factory_sound(3)).unwrap();
    for part in [P1, PartId::ALL[1]] {
        let src = PartSource {
            part,
            from: PartFrom::Slot(slot),
        };
        let c = ReplaceGuard::check(p, t, src).unwrap();
        p.replace_part(c).unwrap();
    }
    r.edit_part();
    r.start_load();
    r.answer(LibraryAnswer::SavePartFirst);
    r.one_pass(); // the read; the stream starts
    assert!(r.ui.card_pending(), "streaming");
    assert!(!r.ui.prompt_open(), "not under the stream");
    r.work();
    assert!(r.toast().starts_with("LOADED TO SLOT "));
    let engine = r.ui.project().part(P1).sound.engine();
    assert_eq!(r.ui.location(), Location::part_home(P1, engine));
    assert_eq!(r.ui.prompt_kind_for_test(), Some(AskKind::UpdateStale));
}

#[test]
fn a_save_from_settings_drops_the_page_and_lists_on_entry() {
    let mut r = Rig::new();
    r.open_library();
    r.work();
    screen::to_leaf(&mut r.ui, &["PART"]);
    for _ in 0..5 {
        r.press(ButtonId::Plus);
    }
    r.press(ButtonId::Edit);
    tap(&mut r.ui, ButtonId::Seq);
    r.one_pass();
    assert_eq!(r.toast(), "SAVED TO LIBRARY");
    assert!(!r.ui.card_pending(), "no page listed off the browser");
    while r.ui.in_settings() {
        tap(&mut r.ui, ButtonId::Menu);
    }
    r.feed(Input::chord(ButtonId::Edit, ButtonId::B1));
    assert!(r.ui.card_pending());
    r.work();
    assert_eq!(r.cursor_id(), Some(1));
}

#[test]
fn a_listing_that_fails_partway_keeps_its_rows_and_says_why() {
    use chimera_core::ui::browser::INFO_Y;
    let mut r = Rig::new();
    for i in 0..3 {
        r.library_with(&factory_sound(i));
    }
    // The directory pass, one peek, then the card fails.
    r.st.fail_after = r.reads() + 2;
    r.open_library();
    r.work();
    let msg = r.ui.library_err_for_test().expect("the failure is kept");
    let fb = screen::render_ui(&r.ui);
    let warn = |y0: i32, y1: i32| {
        (y0..y1).any(|y| (12..228).any(|x| fb.at(x, y) == chimera_core::ui::theme::WARN))
    };
    assert!(warn(INFO_Y - 9, INFO_Y + 1), "{msg} in WARN at INFO_Y");
    assert!(r.cursor_id().is_some(), "the rows that were read stay");
    let row0 = chimera_core::ui::browser::row_y(0);
    assert!(
        (row0 - 12..row0 + 2)
            .any(|y| (12..228).any(|x| fb.at(x, y) == chimera_core::ui::theme::BG)),
        "row 0 drawn on its pill"
    );
}

// The card changed over USB between the listing and EDIT (review focus
// 4): the console's `rm` and `put --over` against a listed page.
#[cfg(feature = "card-put")]
mod console_edits {
    use super::*;
    use chimera_core::console::{CardAnswer, Console, Frame, Out, Stalled, Stats, Unit, answer};
    use chimera_core::storage::Crc32;
    use chimera_core::ui::theme_settings::{ThemeSettings, palette};
    use chimera_hal::FB_SIZE;
    use chimera_hal::store::Files;
    use std::collections::VecDeque;

    impl Files for Counted {
        fn cap(&mut self, _: Option<u32>) {}
    }

    /// The unit as the console sees it, over the rig's UI and card. `forget`:
    /// whether a Sound `put` or `rm` drops the page, as the shells do.
    struct Usb<'a> {
        ui: &'a mut UiState,
        st: &'a mut Counted,
        fb: Box<[u16; FB_SIZE]>,
        forget: bool,
    }

    impl Unit for Usb<'_> {
        fn ui(&self) -> &UiState {
            self.ui
        }
        fn stats(&mut self) -> Option<Stats> {
            None
        }
        fn bench(&self) -> Option<&str> {
            None
        }
        fn card(&mut self) -> Option<CardAnswer> {
            None
        }
        fn frame(&self) -> Frame<'_> {
            Frame {
                fb: &self.fb,
                palette: palette(&ThemeSettings::DEFAULT),
            }
        }
        fn dfu(&mut self) -> Option<()> {
            None
        }
        fn boot(&self) -> Option<chimera_core::boot::BootSeen> {
            None
        }
        fn usb_regs(&self) -> Option<chimera_core::boot::UsbRegs> {
            None
        }
        type Files = Counted;
        fn files(&mut self) -> Option<&mut Counted> {
            Some(&mut *self.st)
        }
        fn forget_library(&mut self) {
            if self.forget {
                self.ui.forget_library();
            }
        }
    }

    /// The host: `input` after the request line, a clock that never stalls it.
    struct Host<'a>(&'a mut VecDeque<u8>, &'a mut Vec<u8>);

    impl Out for Host<'_> {
        fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
            self.1.extend_from_slice(b);
            Ok(())
        }
        fn recv(&mut self, _: u32) -> Option<u8> {
            self.0.pop_front()
        }
        fn now(&self) -> chimera_hal::Ms {
            chimera_hal::Ms(0)
        }
    }

    impl Rig {
        /// `line`, then `data`, through the console: what it answered.
        fn console(&mut self, line: &str, data: &[u8], forget: bool) -> String {
            let mut u = Usb {
                ui: &mut self.ui,
                st: &mut self.st,
                fb: vec![0u16; FB_SIZE].try_into().unwrap(),
                forget,
            };
            let (mut input, mut out) = (data.iter().copied().collect(), Vec::new());
            let mut c = Console::new();
            for &b in line.as_bytes() {
                if let Some(req) = c.push(b) {
                    answer(req, &mut u, &mut Host(&mut input, &mut out)).unwrap();
                }
            }
            String::from_utf8(out).unwrap()
        }

        /// `put --over` of `bytes` onto `id`, as the host tool sends it.
        fn put_over(&mut self, id: LibId, bytes: &[u8], forget: bool) -> String {
            let mut crc = Crc32::new();
            crc.update(bytes);
            let line = format!(
                "put /CHIMERA/SOUNDS/{}.A {} {:08x} --over\n",
                std::str::from_utf8(&id.stem()).unwrap(),
                bytes.len(),
                crc.finish()
            );
            let data: Vec<u8> = bytes
                .chunks(48)
                .flat_map(|c| [base64(c), b"\n".to_vec()].concat())
                .collect();
            self.console(&line, &data, forget)
        }
    }

    fn base64(b: &[u8]) -> Vec<u8> {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for c in b.chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &x)| n | u32::from(x) << (16 - 8 * i));
            for i in 0..4 {
                out.push(match i <= c.len() {
                    true => A[(n >> (18 - 6 * i) & 63) as usize],
                    false => b'=',
                });
            }
        }
        out
    }

    /// P1's Sound is neither `a` nor `b`: nothing loaded.
    fn loaded_neither(r: &Rig, a: &Sound, b: &Sound) {
        let s = &r.ui.project().part(P1).sound;
        assert!(!s.bits_eq(a) && !s.bits_eq(b), "nothing loaded");
    }

    #[test]
    fn a_console_rm_after_listing_never_loads() {
        for forget in [true, false] {
            let mut r = Rig::new();
            let s = factory_sound(0);
            let id = r.library_with(&s);
            r.open_library();
            r.work();
            assert_eq!(r.cursor_id(), Some(id.get()));
            let path = format!(
                "rm /CHIMERA/SOUNDS/{}\n",
                std::str::from_utf8(&id.stem()).unwrap()
            );
            assert_eq!(r.console(&path, b"", forget), "OK\n");
            let counts = r.counts();
            r.press(ButtonId::Edit);
            r.work();
            if forget {
                // The page went with the rm: re-listed empty before EDIT.
                assert_eq!(r.cursor_id(), None);
            } else {
                assert_eq!(r.toast(), "FILE IS GONE");
            }
            assert_eq!(r.counts(), counts);
            loaded_neither(&r, &s, &s);
        }
    }

    #[test]
    fn a_console_put_over_after_listing_never_loads_the_other_sound() {
        let mut r = Rig::new();
        let old = factory_sound(0);
        let new = factory_sound(1);
        let id = r.library_with(&old);
        r.open_library();
        r.work();
        // The page kept: EDIT on the row as listed refuses.
        assert_eq!(r.put_over(id, &stamped(&new), false), "READY\nOK\n");
        let counts = r.counts();
        r.press(ButtonId::Edit);
        r.work();
        assert_eq!(r.toast(), "FILE CHANGED");
        assert_eq!(r.counts(), counts);
        loaded_neither(&r, &old, &new);
        // As the shells do it: the page goes with the put, so the row EDIT
        // finds is the new file's, listed afresh.
        let mut r = Rig::new();
        let id = r.library_with(&old);
        r.open_library();
        r.work();
        assert_eq!(r.put_over(id, &stamped(&new), true), "READY\nOK\n");
        assert!(r.ui.card_pending(), "re-lists");
        r.work();
        assert_eq!(r.cursor_id(), Some(id.get()));
        loaded_neither(&r, &old, &new);
    }
}
