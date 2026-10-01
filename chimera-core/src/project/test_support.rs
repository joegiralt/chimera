//! Project fixtures for tests: a project that fills every record kind, a
//! comparison that says what differs, stores that fail on cue, and the
//! card suite every `Store` passes.

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, ReadSink, Store, StoreError, VolumeId};

use crate::storage::{
    Card, CardEvent, FileError, FileKind, Generation, Header, ProjectId, Side, write_file,
};

use crate::block::Block;
use crate::factory::{FACTORY_LEN, factory_sound};
use crate::name::{ProjectName, SoundName};
use crate::params::{EngineType, FilterParams, OutParams};
use crate::part::PartParams;
use crate::preset::Sound;

use super::{
    Line, LoadLink, LoadOutcome, Origin, PartFrom, PartId, PartSource, Project, ProjectEntry,
    ProjectFile, ProjectNote, ProjectSource, ProjectStatus, ReplaceGuard, SlotId, Subject,
    TemplateCrc, delete_project, list_projects, load_project, new_project_id, part_status,
    project_crc, project_file, project_status, save_project,
};

/// NEW with every slot filled: the factory Sounds and INIT, then edited
/// and renamed copies of them. Parts from slots 0, 3 and 9 and from INIT;
/// two Parts edited after loading, one of them renamed; non-default FX and
/// mix; the name `FULL`.
pub fn full() -> (Box<Project>, TemplateCrc) {
    let (mut p, t) = Project::boxed();
    for s in SlotId::ALL
        .into_iter()
        .skip(FACTORY_LEN + EngineType::ALL.len())
    {
        let i = s.index();
        let mut sound = match i % 3 {
            0 => Sound::init(EngineType::Modal),
            _ => factory_sound(i % FACTORY_LEN).expect("a factory Sound"),
        };
        sound.name = SoundName::new(&format!("COPY {i:02}")).expect("a valid name");
        sound
            .params
            .filter
            .set(FilterParams::CUTOFF, 200.0 + 100.0 * i as f32);
        sound.params.out.set(OutParams::VOLUME, i as f32 / 40.0);
        p.pool_store(s, sound);
    }
    let loads = [
        (0, PartFrom::Slot(SlotId::ALL[0])),
        (1, PartFrom::Slot(SlotId::ALL[3])),
        (2, PartFrom::Slot(SlotId::ALL[9])),
        (3, PartFrom::Init(EngineType::Modal)),
        (5, PartFrom::Slot(SlotId::ALL[3])),
    ];
    for (part, from) in loads {
        let src = PartSource {
            part: PartId::ALL[part],
            from,
        };
        let c = ReplaceGuard::check(&p, t, src).expect("a Clean Part");
        p.replace_part(c).expect("a filled slot");
    }
    let e = p.edit_part(PartId::ALL[1]);
    e.sound.params.filter.set(FilterParams::RESONANCE, 0.6);
    e.sound.name = SoundName::new("RENAMED").expect("a valid name");
    p.edit_part(PartId::ALL[3])
        .sound
        .params
        .out
        .set(OutParams::VOLUME, 0.25);
    let mix = p.edit_part(PartId::ALL[2]).mix;
    mix.set(PartParams::PAN, -0.5);
    mix.set(PartParams::SEND_DELAY, 0.4);
    mix.set(PartParams::LEVEL, 0.6);
    let fx = p.edit_fx();
    fx.delay.mix = 0.3;
    fx.reverb.mix = 0.45;
    fx.chorus.mode = 2;
    p.set_name(ProjectName::new("FULL").expect("a valid name"));
    (p, t)
}

/// Panics with the first difference: the names, each slot (`bits_eq`),
/// each Part's Sound (`bits_eq`), mix, Origin (slot and CRC, or engine;
/// never the generation) and `part_status`, the FX, then `project_crc`.
pub fn same(a: &Project, b: &Project) {
    assert_eq!(a.meta().name(), b.meta().name(), "project name");
    for s in SlotId::ALL {
        match (a.pool().get(s), b.pool().get(s)) {
            (None, None) => {}
            (Some(x), Some(y)) => assert!(
                x.bits_eq(y),
                "slot {}: {} vs {}",
                s.index(),
                x.name.as_str(),
                y.name.as_str()
            ),
            (x, y) => panic!(
                "slot {}: filled {} vs {}",
                s.index(),
                x.is_some(),
                y.is_some()
            ),
        }
    }
    for id in PartId::ALL {
        let (x, y) = (a.part(id), b.part(id));
        let n = id.index();
        assert!(x.sound.bits_eq(&y.sound), "Part {n} sound");
        assert_eq!(x.mix, y.mix, "Part {n} mix");
        match (x.origin(), y.origin()) {
            (
                Origin::Slot {
                    slot: s1, crc: c1, ..
                },
                Origin::Slot {
                    slot: s2, crc: c2, ..
                },
            ) => assert_eq!((s1, c1), (s2, c2), "Part {n} origin"),
            (o1, o2) => assert_eq!(o1, o2, "Part {n} origin"),
        }
        assert_eq!(
            part_status(x, a.pool()),
            part_status(y, b.pool()),
            "Part {n} status"
        );
    }
    assert_eq!(a.perf().fx, b.perf().fx, "FX");
    assert_eq!(project_crc(a), project_crc(b), "project_crc");
}

/// A store whose writes, while `.1`, put half the body, then fail `Full`.
pub struct FullOnWrite<S>(pub S, pub bool);

/// Counts a body's bytes, keeping none.
struct Count(usize);

impl ByteSink for Count {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0 += b.len();
        Ok(())
    }
}

/// Passes `left` bytes on, then fails `Full`.
struct Half<'a> {
    inner: &'a mut dyn ByteSink,
    left: usize,
}

impl ByteSink for Half<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        let n = b.len().min(self.left);
        self.inner.put(&b[..n])?;
        self.left -= n;
        if n < b.len() {
            return Err(StoreError::Full);
        }
        Ok(())
    }
}

impl<S: Store> Store for FullOnWrite<S> {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.0.mount()
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.0.list(vol, dir, f)
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.0.read(vol, file, sink)
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        if !self.1 {
            return self.0.write(vol, file, body);
        }
        let mut count = Count(0);
        body(&mut count)?;
        let left = count.0 / 2;
        self.0.write(vol, file, &mut |w| {
            body(&mut Half { inner: w, left })?;
            Err(StoreError::Full)
        })
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.0.delete(vol, file)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.0.make_dir(vol, dir)
    }
}

/// A store that flips one byte, mid-file, of `file` from its second read
/// on: the card changed between a load's two passes.
pub struct FlipOnSecondRead<S> {
    pub inner: S,
    pub file: FileName,
    reads: u8,
}

impl<S> FlipOnSecondRead<S> {
    pub fn new(inner: S, file: FileName) -> Self {
        FlipOnSecondRead {
            inner,
            file,
            reads: 0,
        }
    }

    /// Reads of `file` so far.
    pub fn reads(&self) -> u8 {
        self.reads
    }
}

/// Flips the byte at `at`.
struct Flip<'a> {
    inner: &'a mut dyn ReadSink,
    at: usize,
    pos: usize,
}

impl ReadSink for Flip<'_> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.at = len as usize / 2;
        self.inner.begin(len)
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let (start, end) = (self.pos, self.pos + bytes.len());
        self.pos = end;
        if !(start..end).contains(&self.at) {
            return self.inner.chunk(bytes);
        }
        let mut c = [0; CHUNK];
        c[..bytes.len()].copy_from_slice(bytes);
        c[self.at - start] ^= 0x10;
        self.inner.chunk(&c[..bytes.len()])
    }
}

impl<S: Store> Store for FlipOnSecondRead<S> {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.inner.list(vol, dir, f)
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if file != self.file {
            return self.inner.read(vol, file, sink);
        }
        self.reads = self.reads.saturating_add(1);
        if self.reads < 2 {
            return self.inner.read(vol, file, sink);
        }
        let mut flip = Flip {
            inner: sink,
            at: 0,
            pos: 0,
        };
        self.inner.read(vol, file, &mut flip)
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.inner.write(vol, file, body)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.inner.delete(vol, file)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(vol, dir)
    }
}

/// A store whose second read of `file` on fails with `err` before any
/// byte: the card pulled (`NoCard`) or swapped (`VolumeChanged`) between
/// a load's two passes.
pub struct FailOnSecondRead<S> {
    pub inner: S,
    pub file: FileName,
    pub err: StoreError,
    reads: u8,
}

impl<S> FailOnSecondRead<S> {
    pub fn new(inner: S, file: FileName, err: StoreError) -> Self {
        FailOnSecondRead {
            inner,
            file,
            err,
            reads: 0,
        }
    }

    /// Reads of `file` so far.
    pub fn reads(&self) -> u8 {
        self.reads
    }
}

impl<S: Store> Store for FailOnSecondRead<S> {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.inner.list(vol, dir, f)
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if file == self.file {
            self.reads = self.reads.saturating_add(1);
            if self.reads >= 2 {
                return Err(self.err);
            }
        }
        self.inner.read(vol, file, sink)
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.inner.write(vol, file, body)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.inner.delete(vol, file)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(vol, dir)
    }
}

fn pid(n: u32) -> ProjectId {
    ProjectId::new(n).expect("an id")
}

/// `P000000n` on the card in the slot.
fn at<S: Store>(s: &mut S, n: u32) -> ProjectFile {
    ProjectFile {
        id: pid(n),
        vol: s.mount().expect("a card"),
    }
}

fn save_at<S: Store>(card: &mut Card, s: &mut S, p: &mut Project, n: u32) -> ProjectNote {
    let f = at(s, n);
    save_project(card, s, p, f)
}

/// Every entry `list_projects` gives, and its note.
fn listed<S: Store>(card: &mut Card, s: &mut S) -> Vec<ProjectEntry> {
    let mut v = Vec::new();
    let out = list_projects(card, s, &mut |e| v.push(e));
    assert_eq!(out.note, None, "list");
    assert!(out.event.is_some());
    v
}

fn saved(note: ProjectNote) {
    assert!(matches!(note, ProjectNote::Saved(_)), "{note:?}");
}

/// A file listed on this card, confirmed (no prompt) and loaded into `q`.
fn load_listed<S: Store>(
    card: &mut Card,
    s: &mut S,
    q: &mut Project,
    t: TemplateCrc,
    id: ProjectId,
    link: &LoadLink,
) -> LoadOutcome {
    let vol = listed(card, s)
        .iter()
        .find(|e| e.id == id)
        .map_or_else(|| s.mount().expect("a card"), |e| e.vol);
    let go = ReplaceGuard::check(q, t, ProjectSource::File { id, vol }).expect("no prompt");
    load_project(card, s, q, go, link)
}

/// Raw bytes as one side of `id`'s pair.
fn put_side<S: Store>(card: &mut Card, s: &mut S, id: ProjectId, side: Side, bytes: &[u8]) {
    let out = card.run(s, |s, r| {
        s.make_dir(r.volume(), Dir::Chimera)?;
        s.make_dir(r.volume(), Dir::Projects)?;
        s.write(r.volume(), project_file(id).side(side), &mut |w| {
            w.put(bytes)
        })
    });
    out.and_then(|o| o.result).expect("a raw write");
}

/// A header-only file of `kind`, its version and name as given.
fn header_bytes(kind: FileKind, generation: u32, name: &str) -> Vec<u8> {
    struct V(Vec<u8>);
    impl ByteSink for V {
        fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
            self.0.extend_from_slice(b);
            Ok(())
        }
    }
    let h = Header {
        kind,
        generation: Generation::new(generation),
        name: Some(ProjectName::new(name).expect("a name")),
    };
    let mut v = V(Vec::new());
    write_file(&mut v, &h, &mut |_| Ok(())).expect("a file");
    v.0
}

/// The save, load, list and delete cases every `Store` passes (a
/// `MemStore`, the desktop's `DirStore`): each on a fresh store.
pub fn project_store_suite<S: Store>(make: &mut dyn FnMut() -> S) {
    save_then_load_is_bit_identical(&mut make());
    first_save_makes_the_dirs(make);
    list_and_next_id(&mut make());
    delete_rules(&mut make());
    missing_file(&mut make());
}

fn save_then_load_is_bit_identical<S: Store>(store: &mut S) {
    let mut card = Card::new();
    let (mut p, _) = full();
    let file = new_project_id(&mut card, store).expect("an id");
    let id = file.id;
    assert_eq!(file, at(store, 1));
    saved(save_project(&mut card, store, &mut p, file));
    assert_eq!(p.meta().file(), Some(file));
    assert_eq!(p.meta().id(), Some(id));
    let (mut q, t) = Project::boxed();
    let link = LoadLink::new();
    let out = load_listed(&mut card, store, &mut q, t, id, &link);
    assert!(out.swap.is_some() && out.note.is_none(), "{:?}", out.note);
    assert_eq!(out.event, Some(CardEvent::Same));
    assert_eq!(link.epoch(), 1);
    same(&p, &q);
    assert_eq!(project_status(&q, t), ProjectStatus::Saved);
    assert_eq!(q.meta().id(), Some(id));
}

fn first_save_makes_the_dirs<S: Store>(make: &mut dyn FnMut() -> S) {
    // A bare card, and one with /CHIMERA only.
    for chimera in [false, true] {
        let mut s = make();
        let mut card = Card::new();
        if chimera {
            let out = card.run(&mut s, |s, r| s.make_dir(r.volume(), Dir::Chimera));
            out.and_then(|o| o.result).expect("/CHIMERA");
        }
        assert_eq!(
            new_project_id(&mut card, &mut s),
            Ok(at(&mut s, 1)),
            "no PROJECTS"
        );
        assert!(listed(&mut card, &mut s).is_empty());
        let (mut p, _) = Project::boxed();
        saved(save_at(&mut card, &mut s, &mut p, 1));
        let got = listed(&mut card, &mut s);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, Some(p.meta().name()));
    }
}

fn list_and_next_id<S: Store>(s: &mut S) {
    let mut card = Card::new();
    let vol = s.mount().expect("a card");
    let named = |n: &str| {
        let (mut p, _) = Project::boxed();
        p.set_name(ProjectName::new(n).expect("a name"));
        p
    };
    for (n, name) in [(1, "ONE"), (2, "TWO"), (5, "FIVE")] {
        assert!(new_project_id(&mut card, s).expect("an id").id.get() <= n);
        saved(save_at(&mut card, s, &mut named(name), n));
    }
    let six = at(s, 6);
    assert_eq!(new_project_id(&mut card, s), Ok(six));
    // A second save under a new name: the newer side's name lists.
    saved(save_at(&mut card, s, &mut named("ONE AGAIN"), 1));
    // Headers this firmware refuses, alone or beside a readable side.
    let mut newer = header_bytes(FileKind::Project, 1, "NEWER");
    newer[4] = 2;
    put_side(&mut card, s, pid(7), Side::A, &newer);
    let mut magic = header_bytes(FileKind::Project, 1, "MAGIC");
    magic[0] = b'X';
    put_side(&mut card, s, pid(8), Side::A, &magic);
    put_side(
        &mut card,
        s,
        pid(9),
        Side::B,
        &header_bytes(FileKind::Sound, 1, "A SOUND"),
    );
    put_side(&mut card, s, pid(2), Side::B, &newer);
    // Not a project's name: skipped.
    let out = card.run(s, |s, r| {
        s.write(
            r.volume(),
            FileName::new(Dir::Projects, b"README", b"TXT").expect("a name"),
            &mut |w| w.put(b"hi"),
        )
    });
    out.and_then(|o| o.result).expect("a write");

    let entry = |n: u32, name: Option<&str>, err| ProjectEntry {
        id: pid(n),
        vol,
        name: name.map(|n| ProjectName::new(n).expect("a name")),
        err,
    };
    assert_eq!(
        listed(&mut card, s),
        [
            entry(1, Some("ONE AGAIN"), None),
            entry(2, Some("TWO"), Some(FileError::NeedsNewerFirmware)),
            entry(5, Some("FIVE"), None),
            entry(7, None, Some(FileError::NeedsNewerFirmware)),
            entry(8, None, Some(FileError::BadMagic)),
            entry(9, None, Some(FileError::WrongKind)),
        ]
    );
    let ten = at(s, 10);
    assert_eq!(new_project_id(&mut card, s), Ok(ten));

    // The last id: the next is refused.
    saved(save_at(&mut card, s, &mut named("LAST"), ProjectId::MAX));
    assert_eq!(new_project_id(&mut card, s), Err(ProjectNote::NoIds));
}

fn delete_rules<S: Store>(s: &mut S) {
    let mut card = Card::new();
    let (at1, two) = (at(s, 1), at(s, 2));
    let (mut a, _) = Project::boxed();
    let (mut b, _) = Project::boxed();
    saved(save_at(&mut card, s, &mut a, 1));
    saved(save_at(&mut card, s, &mut b, 2));
    saved(save_at(&mut card, s, &mut b, 2));
    assert_eq!(
        delete_project(&mut card, s, &b, two),
        Err(ProjectNote::IsLoaded)
    );
    assert_eq!(listed(&mut card, s).len(), 2, "nothing deleted");
    assert_eq!(delete_project(&mut card, s, &b, at1), Ok(()));
    let left: Vec<_> = listed(&mut card, s).iter().map(|e| e.id).collect();
    assert_eq!(left, [pid(2)]);
    // A NEW project has no id: it guards nothing.
    let (fresh, _) = Project::boxed();
    assert_eq!(delete_project(&mut card, s, &fresh, two), Ok(()));
    assert!(listed(&mut card, s).is_empty());
    let one = at(s, 1);
    assert_eq!(new_project_id(&mut card, s), Ok(one));
    assert_eq!(
        delete_project(&mut card, s, &fresh, two),
        Ok(()),
        "already gone"
    );
}

fn missing_file<S: Store>(s: &mut S) {
    let mut card = Card::new();
    let (mut p, _) = Project::boxed();
    saved(save_at(&mut card, s, &mut p, 1));
    let (mut q, t) = full();
    q.mark_saved_for_test();
    let before = project_crc(&q);
    let link = LoadLink::new();
    let out = load_listed(&mut card, s, &mut q, t, pid(3), &link);
    assert_eq!(out.note, Some(ProjectNote::Missing(Subject::File(pid(3)))));
    assert!(out.swap.is_none());
    assert_eq!(project_crc(&q), before, "RAM untouched");
    assert_eq!(link.epoch(), 0);
    assert_eq!(
        out.note.map(|n| n.line()),
        Some(Line::new("PROJECT NOT FOUND: P0000003"))
    );
}
