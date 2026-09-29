//! A/B files on `MemStore` (ADR 0045): the pick rule, streamed saves and
//! two-pass loads that never touch the target on an error.

use chimera_core::factory::factory_sound;
use chimera_core::mod_path::MAX_REGISTRY_DESTS;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    AbFile, Card, CardFault, Crc32, FileError, FileKind, Generation, Header, LoadError,
    MAX_RECORD_LEN, Pick, Ready, RecordTag, SaveError, Side, SideState, SoundDecoder, check_file,
    check_frame, delete_ab, delete_order, encode_sound, load_ab, load_file, pick, save_ab,
    write_file, write_target,
};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use core::ops::ControlFlow;

const NNF: FileError = FileError::NeedsNewerFirmware;

fn file() -> AbFile {
    AbFile::new(Dir::Sounds, b"S0000001").unwrap()
}

fn store() -> MemStore {
    let mut s = MemStore::new(1);
    let v = s.mount().unwrap();
    s.make_dir(v, Dir::Chimera).unwrap();
    s.make_dir(v, Dir::Sounds).unwrap();
    s
}

/// `op` inside `Card::run` on a fresh `Card`.
fn run<S: Store, R, E: CardFault + From<StoreError> + core::fmt::Debug>(
    s: &mut S,
    op: impl FnOnce(&mut S, &Ready) -> Result<R, E>,
) -> Result<R, E> {
    Card::new().run(s, op).unwrap().result
}

fn sound(i: usize) -> Sound {
    factory_sound(i).unwrap()
}

/// Only pass 1 runs on a save's decoder: its target is scratch.
fn save<S: Store>(s: &mut S, snd: &Sound) -> Result<Generation, SaveError> {
    let mut scratch = Sound::neutral(EngineType::Algo);
    let mut d = SoundDecoder::new(&mut scratch);
    run(s, |s, r| {
        save_ab(s, r, file(), &mut d, Some(snd.name), &mut |w| {
            encode_sound(snd, w)
        })
    })
}

fn delete<S: Store>(s: &mut S) -> Result<(), StoreError> {
    let mut scratch = Sound::neutral(EngineType::Algo);
    run(s, |s, r| {
        delete_ab(s, r, file(), &mut SoundDecoder::new(&mut scratch))
    })
}

/// `load_ab` onto a copy of `into`.
fn load_onto<S: Store>(s: &mut S, into: &Sound) -> (Result<Header, LoadError>, Sound) {
    let mut t = into.clone();
    let r = run(s, |s, r| {
        load_ab(s, r, file(), &mut SoundDecoder::new(&mut t))
    });
    (r, t)
}

fn load<S: Store>(s: &mut S) -> Result<Sound, LoadError> {
    let (r, t) = load_onto(s, &Sound::neutral(EngineType::Algo));
    r.map(|_| t)
}

fn assert_loads<S: Store>(s: &mut S, want: &Sound) {
    let got = load(s).expect("a generation loads");
    assert!(got.bits_eq(want), "loaded another Sound");
}

#[derive(Default)]
struct Bytes(Vec<u8>);

impl ReadSink for Bytes {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(b);
        ControlFlow::Continue(())
    }
}

impl ByteSink for Bytes {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

fn vol(s: &mut MemStore) -> VolumeId {
    s.mount().unwrap()
}

fn raw(s: &mut MemStore, side: Side) -> Vec<u8> {
    let v = vol(s);
    let mut b = Bytes::default();
    s.read(v, file().side(side), &mut b).unwrap();
    b.0
}

fn put_raw(s: &mut MemStore, side: Side, bytes: &[u8]) {
    let v = vol(s);
    s.write(v, file().side(side), &mut |w| w.put(bytes))
        .unwrap();
}

fn generation_of(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[8..12].try_into().unwrap())
}

/// A Sound file of `snd` at `generation`, then `extra` records.
fn encoded(snd: &Sound, generation: u32, extra: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::new(generation),
        name: Some(snd.name),
    };
    let mut b = Bytes::default();
    write_file(&mut b, &h, &mut |w| {
        encode_sound(snd, w)?;
        extra.iter().try_for_each(|(t, p)| w.put(*t, p))
    })
    .unwrap();
    b.0
}

fn fix_crc(f: &mut [u8]) {
    let n = f.len() - 4;
    let mut c = Crc32::new();
    c.update(&f[..n]);
    f[n..].copy_from_slice(&c.finish().to_le_bytes());
}

/// `f` with a raw record (any tag) before its trailer, CRC fixed.
fn with_raw_record(mut f: Vec<u8>, tag: u16, payload: &[u8]) -> Vec<u8> {
    let at = f.len() - 4;
    let mut rec = tag.to_le_bytes().to_vec();
    rec.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    rec.extend_from_slice(payload);
    f.splice(at..at, rec);
    fix_crc(&mut f);
    f
}

// The pure rule.

fn present(g: u32, err: Option<FileError>) -> SideState {
    SideState::Present {
        generation: Generation::new(g),
        err,
    }
}

#[test]
fn pick_and_write_target_table() {
    use Side::{A, B};
    let m = SideState::Missing;
    let t = SideState::Torn(FileError::BadCrc);
    let p1 = present(1, None);
    let p2 = present(2, None);
    let n2 = present(2, Some(NNF));
    let x2 = present(2, Some(FileError::Bounds));
    let bad = FileError::BadCrc;
    let bounds = FileError::Bounds;
    #[rustfmt::skip]
    let cases = [
        (m, m, Pick::Missing, (A, 1)),
        (m, t, Pick::Refuse(B, bad), (A, 1)),
        (m, p1, Pick::Load(B), (A, 2)),
        (m, p2, Pick::Load(B), (A, 3)),
        (m, n2, Pick::Refuse(B, NNF), (A, 3)),
        (m, x2, Pick::Refuse(B, bounds), (A, 3)),
        (t, m, Pick::Refuse(A, bad), (B, 1)),
        (t, t, Pick::Refuse(A, bad), (A, 1)),
        (t, p1, Pick::Load(B), (A, 2)),
        (t, p2, Pick::Load(B), (A, 3)),
        (t, n2, Pick::Refuse(B, NNF), (A, 3)),
        (t, x2, Pick::Refuse(B, bounds), (A, 3)),
        (p1, m, Pick::Load(A), (B, 2)),
        (p1, t, Pick::Load(A), (B, 2)),
        (p1, p1, Pick::Load(A), (B, 2)),
        (p1, p2, Pick::Load(B), (A, 3)),
        (p1, n2, Pick::Refuse(B, NNF), (A, 3)),
        (p1, x2, Pick::Load(A), (B, 3)),
        (p2, m, Pick::Load(A), (B, 3)),
        (p2, t, Pick::Load(A), (B, 3)),
        (p2, p1, Pick::Load(A), (B, 3)),
        (p2, p2, Pick::Load(A), (B, 3)),
        (p2, n2, Pick::Load(A), (B, 3)),
        (p2, x2, Pick::Load(A), (B, 3)),
        (n2, m, Pick::Refuse(A, NNF), (B, 3)),
        (n2, t, Pick::Refuse(A, NNF), (B, 3)),
        (n2, p1, Pick::Refuse(A, NNF), (B, 3)),
        (n2, p2, Pick::Refuse(A, NNF), (B, 3)),
        (n2, n2, Pick::Refuse(A, NNF), (B, 3)),
        (n2, x2, Pick::Refuse(A, NNF), (B, 3)),
        (x2, m, Pick::Refuse(A, bounds), (B, 3)),
        (x2, t, Pick::Refuse(A, bounds), (B, 3)),
        (x2, p1, Pick::Load(B), (A, 3)),
        (x2, p2, Pick::Load(B), (A, 3)),
        (x2, n2, Pick::Refuse(B, NNF), (A, 3)),
        (x2, x2, Pick::Refuse(A, bounds), (B, 3)),
        // The generation wraps: 0 is newer than u32::MAX.
        (present(u32::MAX, None), m, Pick::Load(A), (B, 0)),
        (present(u32::MAX, None), present(0, None), Pick::Load(B), (A, 1)),
        (present(0, None), present(u32::MAX, None), Pick::Load(A), (B, 1)),
        // No header to order by: a newer format is refused, never shadowed.
        (SideState::Headerless(NNF), p2, Pick::Refuse(A, NNF), (B, 3)),
        (p2, SideState::Headerless(NNF), Pick::Refuse(B, NNF), (A, 3)),
        (SideState::Headerless(FileError::BadMagic), p1, Pick::Load(B), (A, 2)),
        (SideState::Headerless(FileError::BadMagic), m, Pick::Refuse(A, FileError::BadMagic), (B, 1)),
        (t, SideState::Headerless(FileError::WrongKind), Pick::Refuse(B, FileError::WrongKind), (A, 1)),
    ];
    for (a, b, want_pick, (side, g)) in cases {
        assert_eq!(pick(a, b), want_pick, "pick({a:?}, {b:?})");
        assert_eq!(
            write_target(a, b),
            (side, Generation::new(g)),
            "write_target({a:?}, {b:?})"
        );
        assert_eq!(
            delete_order(a, b),
            [side, side.other()],
            "delete_order({a:?}, {b:?})"
        );
        if let Pick::Load(k) | Pick::Refuse(k, NNF) = want_pick {
            assert_ne!(side, k, "wrote the kept side: {a:?}, {b:?}");
        }
    }
}

/// Load and save agree on every pair of side states: the side a save
/// writes is never the one a load returns, or refuses as needing newer
/// firmware.
#[test]
fn save_never_writes_what_load_keeps() {
    let errs = [
        FileError::Truncated,
        FileError::BadMagic,
        FileError::BadCrc,
        NNF,
        FileError::WrongKind,
        FileError::Bounds,
        FileError::BadName,
        FileError::Corrupt,
    ];
    let mut states = vec![SideState::Missing];
    for e in errs {
        states.push(if e.is_torn() {
            SideState::Torn(e)
        } else {
            SideState::Headerless(e)
        });
    }
    for g in [0, 1, 2, u32::MAX] {
        states.push(present(g, None));
        states.extend(errs.map(|e| present(g, Some(e))));
    }
    for &a in &states {
        for &b in &states {
            let (side, g) = write_target(a, b);
            match pick(a, b) {
                Pick::Load(k) | Pick::Refuse(k, NNF) => {
                    assert_ne!(side, k, "{a:?}, {b:?}: wrote the kept side")
                }
                Pick::Refuse(..) | Pick::Missing => {}
            }
            for s in [a, b] {
                if let SideState::Present { generation, .. } = s {
                    assert!(g.is_newer_than(generation), "{a:?}, {b:?}: {g:?}");
                }
            }
        }
    }
}

// Saves and loads.

#[test]
fn save_thrice_alternates() {
    let mut s = store();
    for (i, (side, g)) in [(Side::A, 1), (Side::B, 2), (Side::A, 3)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(save(&mut s, &sound(i)), Ok(Generation::new(g)));
        assert_eq!(generation_of(&raw(&mut s, side)), g);
        assert_loads(&mut s, &sound(i));
    }
    assert_eq!(generation_of(&raw(&mut s, Side::B)), 2);
}

#[test]
fn tied_generations_prefer_a_then_write_b() {
    let mut s = store();
    put_raw(&mut s, Side::A, &encoded(&sound(0), 5, &[]));
    put_raw(&mut s, Side::B, &encoded(&sound(1), 5, &[]));
    assert_loads(&mut s, &sound(0));
    assert_eq!(save(&mut s, &sound(2)), Ok(Generation::new(6)));
    assert_eq!(generation_of(&raw(&mut s, Side::B)), 6);
    assert_eq!(generation_of(&raw(&mut s, Side::A)), 5);
    assert_loads(&mut s, &sound(2));
}

#[test]
fn torn_newest_falls_back() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    save(&mut s, &sound(1)).unwrap();
    let b = raw(&mut s, Side::B);
    put_raw(&mut s, Side::B, &b[..b.len() - 10]);
    assert_loads(&mut s, &sound(0));
}

/// A Sound file at `generation` of hand-built records.
fn records(generation: u32, recs: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::new(generation),
        name: None,
    };
    let mut b = Bytes::default();
    write_file(&mut b, &h, &mut |w| {
        recs.iter().try_for_each(|(t, p)| w.put(*t, p))
    })
    .unwrap();
    b.0
}

/// The decoder's `Bounds`, under a valid CRC: `load_ab` falls back to A,
/// and `save_ab`, judging by the same decode, writes over B, never A.
#[test]
fn invalid_newest_falls_back() {
    use chimera_core::block::DiskCode;
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let engine = (RecordTag::Engine, vec![EngineType::Algo.disk_code()]);
    let too_many = (RecordTag::Registry, vec![0; (MAX_REGISTRY_DESTS + 1) * 10]);
    put_raw(&mut s, Side::B, &records(2, &[engine, too_many]));
    let mut t = Sound::neutral(EngineType::Algo);
    let state = run(&mut s, |s, r| {
        check_file(s, r, file().side(Side::B), &mut SoundDecoder::new(&mut t))
    });
    assert_eq!(state, Ok(present(2, Some(FileError::Bounds))));
    let state = run(&mut s, |s, r| {
        check_frame(s, r, file().side(Side::B), FileKind::Sound)
    });
    assert_eq!(state, Ok(present(2, None)), "framing alone passes it");
    assert_loads(&mut s, &sound(0));
    let a = raw(&mut s, Side::A);
    assert_eq!(save(&mut s, &sound(2)), Ok(Generation::new(3)));
    assert_eq!(generation_of(&raw(&mut s, Side::B)), 3, "wrote over B");
    assert_eq!(raw(&mut s, Side::A), a, "A untouched");
    assert_loads(&mut s, &sound(2));
}

/// A framing `Bounds` (a record length past the body) is invalid to
/// `save_ab` too: the next save writes over it.
#[test]
fn invalid_newest_by_bounds_is_written_over() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let mut bad = encoded(&sound(1), 2, &[]);
    // A record length past the end of the body: `Bounds` under a valid CRC.
    let at = bad.len() - 4;
    bad.splice(at..at, [0x71, 0x00, 0xFF, 0x00]);
    fix_crc(&mut bad);
    put_raw(&mut s, Side::B, &bad);
    let state = run(&mut s, |s, r| {
        check_frame(s, r, file().side(Side::B), FileKind::Sound)
    });
    assert_eq!(state, Ok(present(2, Some(FileError::Bounds))));
    assert_loads(&mut s, &sound(0));
    assert_eq!(save(&mut s, &sound(2)), Ok(Generation::new(3)));
    assert_eq!(generation_of(&raw(&mut s, Side::B)), 3, "wrote over B");
    assert_eq!(generation_of(&raw(&mut s, Side::A)), 1);
    assert_loads(&mut s, &sound(2));
}

#[test]
fn newer_firmware_newest_does_not_fall_back() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let newer = with_raw_record(encoded(&sound(1), 2, &[]), 0x8077, &[1, 2, 3]);
    put_raw(&mut s, Side::B, &newer);
    assert_eq!(load(&mut s).err(), Some(LoadError::File(NNF)));
    assert_eq!(save(&mut s, &sound(2)), Ok(Generation::new(3)));
    assert_eq!(generation_of(&raw(&mut s, Side::A)), 3, "wrote A");
    assert_eq!(raw(&mut s, Side::B), newer, "B untouched");
}

/// A format version this firmware doesn't know has no header it can read:
/// refused, never shadowed, and never written over.
#[test]
fn newer_version_is_refused_and_kept() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let mut v2 = encoded(&sound(1), 2, &[]);
    v2[4] = 2;
    fix_crc(&mut v2);
    put_raw(&mut s, Side::B, &v2);
    let state = run(&mut s, |s, r| {
        check_frame(s, r, file().side(Side::B), FileKind::Sound)
    });
    assert_eq!(state, Ok(SideState::Headerless(NNF)));
    assert_eq!(load(&mut s).err(), Some(LoadError::File(NNF)));
    save(&mut s, &sound(2)).unwrap();
    assert_eq!(raw(&mut s, Side::B), v2, "B untouched");
}

#[test]
fn both_torn_is_the_error_not_missing() {
    let mut s = store();
    assert_eq!(load(&mut s).err(), Some(LoadError::Missing));
    save(&mut s, &sound(0)).unwrap();
    save(&mut s, &sound(1)).unwrap();
    for side in [Side::A, Side::B] {
        let b = raw(&mut s, side);
        put_raw(&mut s, side, &b[..b.len() - 10]);
    }
    assert_eq!(load(&mut s).err(), Some(LoadError::File(FileError::BadCrc)));
}

#[test]
fn a_short_file_is_torn() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    put_raw(&mut s, Side::B, &[0; 31]);
    let state = run(&mut s, |s, r| {
        check_frame(s, r, file().side(Side::B), FileKind::Sound)
    });
    assert_eq!(state, Ok(SideState::Torn(FileError::Truncated)));
    assert_loads(&mut s, &sound(0));
}

#[test]
fn wrong_kind_is_invalid() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let h = Header {
        kind: FileKind::System,
        generation: Generation::new(2),
        name: None,
    };
    let mut b = Bytes::default();
    write_file(&mut b, &h, &mut |_| Ok(())).unwrap();
    put_raw(&mut s, Side::B, &b.0);
    let state = run(&mut s, |s, r| {
        check_frame(s, r, file().side(Side::B), FileKind::Sound)
    });
    assert_eq!(state, Ok(present(2, Some(FileError::WrongKind))));
    assert_loads(&mut s, &sound(0));
}

// Deletes.

/// Logs deletes; fails every one after the first `allow`.
struct DeleteLog {
    inner: MemStore,
    deleted: Vec<FileName>,
    allow: usize,
}

impl Store for DeleteLog {
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
        self.inner.read(v, f, s)
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
        if self.deleted.len() == self.allow {
            return Err(StoreError::Io);
        }
        self.deleted.push(f);
        self.inner.delete(v, f)
    }
    fn make_dir(&mut self, v: VolumeId, d: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(v, d)
    }
}

#[test]
fn delete_older_first() {
    for (older, newer, saves) in [(Side::A, Side::B, 2), (Side::B, Side::A, 3)] {
        let mut s = DeleteLog {
            inner: store(),
            deleted: vec![],
            allow: 1,
        };
        for i in 0..saves {
            save(&mut s, &sound(i)).unwrap();
        }
        assert_eq!(
            delete(&mut s),
            Err(StoreError::Io),
            "cut after the first delete"
        );
        assert_eq!(s.deleted, [file().side(older)]);
        assert_loads(&mut s, &sound(saves - 1));

        s.allow = 2;
        delete(&mut s).unwrap();
        assert_eq!(s.deleted, [file().side(older), file().side(newer)]);
        assert_eq!(load(&mut s).err(), Some(LoadError::Missing));
        assert_eq!(delete(&mut s), Ok(()));
    }
}

// Streaming.

/// Records the size of every `put` a save makes.
struct PutLog {
    inner: MemStore,
    puts: Vec<usize>,
}

struct LogSink<'a> {
    inner: &'a mut dyn ByteSink,
    puts: &'a mut Vec<usize>,
}

impl ByteSink for LogSink<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.puts.push(b.len());
        self.inner.put(b)
    }
}

impl Store for PutLog {
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
        self.inner.read(v, f, s)
    }
    fn write(
        &mut self,
        v: VolumeId,
        f: FileName,
        b: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        let puts = &mut self.puts;
        self.inner
            .write(v, f, &mut |w| b(&mut LogSink { inner: w, puts }))
    }
    fn delete(&mut self, v: VolumeId, f: FileName) -> Result<(), StoreError> {
        self.inner.delete(v, f)
    }
    fn make_dir(&mut self, v: VolumeId, d: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(v, d)
    }
}

#[test]
fn save_streams_in_chunks() {
    let mut s = PutLog {
        inner: store(),
        puts: vec![],
    };
    save(&mut s, &sound(0)).unwrap();
    let total: usize = s.puts.iter().sum();
    assert!(s.puts.len() > 3, "streamed: {:?}", s.puts);
    assert!(
        s.puts.iter().all(|&n| n <= MAX_RECORD_LEN + 4),
        "{:?}",
        s.puts
    );
    assert_eq!(total, raw(&mut s.inner, Side::A).len());
}

// Two passes.

#[test]
fn load_leaves_target_on_error() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let mut a = raw(&mut s, Side::A);
    a[40] ^= 1;
    put_raw(&mut s, Side::A, &a);
    let target = sound(3);
    let (r, t) = load_onto(&mut s, &target);
    assert_eq!(r.err(), Some(LoadError::File(FileError::BadCrc)));
    assert!(t.bits_eq(&target), "an Err touched the target");
}

/// Hands out `swap` in place of the file on its `at`th read (0-based):
/// the card changed between the passes.
struct ChangeOnRead {
    inner: MemStore,
    reads: usize,
    at: usize,
    swap: Vec<u8>,
}

impl Store for ChangeOnRead {
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
        let n = self.reads;
        self.reads += 1;
        if n != self.at {
            return self.inner.read(v, f, s);
        }
        if s.begin(self.swap.len() as u32).is_continue() {
            for c in self.swap.chunks(512) {
                if s.chunk(c).is_break() {
                    break;
                }
            }
        }
        Ok(())
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
}

/// Pass 2 reads other bytes than pass 1 checked: a torn file, or another
/// valid one. Either is `BadCrc`, and the target is untouched.
#[test]
fn a_file_changed_between_passes_is_bad_crc() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    let good = raw(&mut s, Side::A);
    let mut torn = good.clone();
    let last = torn.len() - 5;
    torn[last] ^= 0x40;
    let other = encoded(&sound(1), 1, &[]);
    for swap in [torn, other] {
        // load_ab reads A, then B (missing), then A again.
        let mut s = ChangeOnRead {
            inner: store(),
            reads: 0,
            at: 2,
            swap,
        };
        put_raw(&mut s.inner, Side::A, &good);
        let target = sound(3);
        let (r, t) = load_onto(&mut s, &target);
        assert_eq!(r.err(), Some(LoadError::File(FileError::BadCrc)));
        assert!(t.bits_eq(&target), "pass 2 touched the target");
    }
}

#[test]
fn load_file_reads_one_side() {
    let mut s = store();
    save(&mut s, &sound(0)).unwrap();
    save(&mut s, &sound(1)).unwrap();
    let mut t = Sound::neutral(EngineType::Algo);
    let h = run(&mut s, |s, r| {
        load_file(s, r, file().side(Side::A), &mut SoundDecoder::new(&mut t))
    })
    .unwrap();
    assert_eq!(h.generation, Generation::FIRST);
    assert!(t.bits_eq(&sound(0)));
    let r = run(&mut s, |s, r| {
        load_file(
            s,
            r,
            FileName::new(Dir::Sounds, b"NOPE", b"A").unwrap(),
            &mut SoundDecoder::new(&mut t),
        )
    });
    assert_eq!(r, Err(LoadError::Missing));
}

/// Fails every read with `err`.
struct FailRead {
    inner: MemStore,
    err: StoreError,
}

impl Store for FailRead {
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
    fn read(&mut self, _: VolumeId, _: FileName, _: &mut dyn ReadSink) -> Result<(), StoreError> {
        Err(self.err)
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
}

/// A broken chain (`Corrupt`) is a torn side; `NotFound` is missing; any
/// other store error is a card fault that reaches the `Card`.
#[test]
fn store_errors_classify_sides() {
    let cases = [
        (
            StoreError::Corrupt,
            Ok(SideState::Torn(FileError::Truncated)),
        ),
        (StoreError::NotFound, Ok(SideState::Missing)),
        (StoreError::Io, Err(StoreError::Io)),
        (StoreError::Timeout, Err(StoreError::Timeout)),
    ];
    for (err, want) in cases {
        let mut s = FailRead {
            inner: store(),
            err,
        };
        let f = file().side(Side::A);
        let got = run(&mut s, |s, r| check_frame(s, r, f, FileKind::Sound));
        assert_eq!(got, want, "{err:?}");
        let mut t = Sound::neutral(EngineType::Algo);
        let got = run(&mut s, |s, r| {
            check_file(s, r, f, &mut SoundDecoder::new(&mut t))
        });
        assert_eq!(got, want, "{err:?}");
    }

    let mut s = FailRead {
        inner: store(),
        err: StoreError::Io,
    };
    let mut card = Card::new();
    let mut t = Sound::neutral(EngineType::Algo);
    let out = card
        .run(&mut s, |s, r| {
            load_ab(s, r, file(), &mut SoundDecoder::new(&mut t))
        })
        .unwrap();
    assert_eq!(out.result, Err(LoadError::Store(StoreError::Io)));
    assert!(matches!(card, Card::Failed { .. }), "{card:?}");
}

#[test]
fn errors_expose_only_store_faults() {
    assert_eq!(
        LoadError::Store(StoreError::Io).store_error(),
        Some(StoreError::Io)
    );
    assert_eq!(LoadError::File(FileError::BadCrc).store_error(), None);
    assert_eq!(LoadError::Missing.store_error(), None);
    assert_eq!(
        SaveError::Store(StoreError::Full).store_error(),
        Some(StoreError::Full)
    );
}

#[test]
fn ab_file_names() {
    let f = file();
    assert_eq!(
        f.side(Side::A),
        FileName::new(Dir::Sounds, b"S0000001", b"A").unwrap()
    );
    assert_eq!(
        f.side(Side::B),
        FileName::new(Dir::Sounds, b"S0000001", b"B").unwrap()
    );
    assert_eq!(
        AbFile::SYSTEM.side(Side::B),
        FileName::new(Dir::Chimera, b"SYSTEM", b"B").unwrap()
    );
    assert_eq!(AbFile::new(Dir::Sounds, b""), None);
    assert_eq!(AbFile::new(Dir::Sounds, b"TOOLONG99"), None);
    assert_eq!(AbFile::new(Dir::Sounds, b"lower"), None);
}
