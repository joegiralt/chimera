//! SYSTEM (ADR 0045): theme and last project, read at boot and written on
//! leaving System when RAM differs from what this card is known to hold.

mod common;

use chimera_core::storage::{
    AbFile, BootNote, Card, Exit, ExitPlan, FileError, LoadError, ProjectId, Side, SyncError,
    SystemSettings, SystemSync, exit_plan,
};
use chimera_core::ui::theme_settings::{Accent, Black, Bright, Gamma, ThemeSettings};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use common::codec_util::{
    SYSTEM_FIXTURE, fix_crc, record_offsets, system_file, system_fixture_settings,
};
use core::ops::ControlFlow;

/// BRIGHT 40, GAMMA SOFT, ACCENT index 3, BLACK +1, project 7.
fn settings() -> SystemSettings {
    SystemSettings {
        theme: ThemeSettings {
            bright: Bright::new(40),
            gamma: Gamma::Soft,
            accent: Accent::ALL[3],
            black: Black::new(1),
        },
        last_project: ProjectId::new(7),
    }
}

/// `bytes` as SYSTEM's `side`, `/CHIMERA` made if need be.
fn put<S: Store>(s: &mut S, side: Side, bytes: &[u8]) {
    let v = s.mount().unwrap();
    s.make_dir(v, Dir::Chimera).unwrap();
    s.write(v, AbFile::SYSTEM.side(side), &mut |k| k.put(bytes))
        .unwrap();
}

struct Collect(Vec<u8>);

impl ReadSink for Collect {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(b);
        ControlFlow::Continue(())
    }
}

fn get<S: Store>(s: &mut S, side: Side) -> Vec<u8> {
    let v = s.mount().unwrap();
    let mut c = Collect(Vec::new());
    s.read(v, AbFile::SYSTEM.side(side), &mut c).unwrap();
    c.0
}

/// `write` from a fresh boot on `s`.
fn save(s: &mut MemStore, want: &SystemSettings) {
    let mut card = Card::new();
    let (mut sync, ..) = SystemSync::boot(&mut card, s);
    sync.write(&mut card, s, want).unwrap();
}

fn boot<S: Store>(s: &mut S) -> (SystemSettings, Option<BootNote>, Card) {
    let mut card = Card::new();
    let (_, got, note) = SystemSync::boot(&mut card, s);
    (got, note, card)
}

#[test]
fn round_trip() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    let (got, note, card) = boot(&mut s);
    assert_eq!((got, note), (settings(), None));
    assert!(matches!(card, Card::Ready(_)), "{card:?}");

    // No last project is stored as none.
    let none = SystemSettings {
        last_project: None,
        ..settings()
    };
    save(&mut s, &none);
    assert_eq!(boot(&mut s).0, none);
}

#[test]
fn boot_no_card_defaults() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    s.eject();
    let (got, note, card) = boot(&mut s);
    assert_eq!(got, SystemSettings::DEFAULT);
    assert_eq!(note, Some(BootNote::NoCard));
    assert_eq!(card, Card::Absent);
}

#[test]
fn boot_no_file_defaults() {
    let mut s = MemStore::new(1);
    let (got, note, card) = boot(&mut s);
    assert_eq!(got, SystemSettings::DEFAULT);
    assert_eq!(note, Some(BootNote::NoFile));
    assert!(matches!(card, Card::Ready(_)), "{card:?}");
}

/// Both sides torn: the torn error, not "no file", and the defaults.
#[test]
fn boot_corrupt_defaults() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    save(&mut s, &settings());
    let mut a = get(&mut s, Side::A);
    let last = a.len() - 1;
    a[last] ^= 0x01;
    put(&mut s, Side::A, &a);
    let b = get(&mut s, Side::B);
    put(&mut s, Side::B, &b[..b.len() - 1]);

    let (got, note, card) = boot(&mut s);
    assert_eq!(got, SystemSettings::DEFAULT);
    assert!(
        matches!(note, Some(BootNote::Error(LoadError::File(e))) if e.is_torn()),
        "{note:?}"
    );
    assert!(matches!(card, Card::Ready(_)), "{card:?}");
}

/// How the failing read of SYSTEM.A fails.
#[derive(Clone, Copy, Debug)]
enum Fail {
    /// The card goes away mid-read.
    Io,
    /// Other bytes: the card changed between the passes. Every record
    /// arrives, and only the trailer differs.
    Tampered,
}

/// Fails read number `at` of SYSTEM.A; every other read goes through.
struct ReadFails {
    inner: MemStore,
    reads: u32,
    at: u32,
    how: Fail,
}

impl ReadFails {
    fn new(inner: MemStore, at: u32, how: Fail) -> Self {
        ReadFails {
            inner,
            reads: 0,
            at,
            how,
        }
    }
}

impl Store for ReadFails {
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
    fn read(
        &mut self,
        v: VolumeId,
        f: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if f != AbFile::SYSTEM.side(Side::A) {
            return self.inner.read(v, f, sink);
        }
        self.reads += 1;
        if self.reads != self.at {
            return self.inner.read(v, f, sink);
        }
        let mut c = Collect(Vec::new());
        self.inner.read(v, f, &mut c)?;
        match self.how {
            Fail::Io => Err(StoreError::Io),
            Fail::Tampered => {
                let last = c.0.len() - 1;
                c.0[last] ^= 0x01;
                let _ = sink.begin(c.0.len() as u32);
                let _ = sink.chunk(&c.0);
                Ok(())
            }
        }
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

#[test]
fn boot_pass_two_failure_keeps_defaults() {
    for (how, want) in [
        (Fail::Io, LoadError::Store(StoreError::Io)),
        (Fail::Tampered, LoadError::File(FileError::BadCrc)),
    ] {
        let mut inner = MemStore::new(1);
        save(&mut inner, &settings());
        // Pass 2 of the side pass 1 picked.
        let mut s = ReadFails::new(inner, 2, how);
        let (got, note, _) = boot(&mut s);
        assert_eq!(s.reads, 2, "{how:?}");
        assert_eq!(got, SystemSettings::DEFAULT, "{how:?}");
        assert_eq!(note, Some(BootNote::Error(want)), "{how:?}");
    }
}

/// `f` with a record of `tag` and `payload` inserted at `at`.
fn with_record(f: &[u8], at: usize, tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = f.to_vec();
    let mut rec = tag.to_le_bytes().to_vec();
    rec.extend((payload.len() as u16).to_le_bytes());
    rec.extend(payload);
    f.splice(at..at, rec);
    fix_crc(&mut f);
    f
}

fn boot_bytes(bytes: &[u8]) -> (SystemSettings, Option<BootNote>) {
    let mut s = MemStore::new(1);
    put(&mut s, Side::A, bytes);
    let (got, note, _) = boot(&mut s);
    (got, note)
}

#[test]
fn unknown_record_kept_theme() {
    let f = system_file(&settings());
    for at in record_offsets(&f) {
        let g = with_record(&f, at, 0x0071, &[0xA5; 40]);
        assert_eq!(boot_bytes(&g), (settings(), None), "at {at}");
    }
}

/// What SYSTEM does with records it doesn't read, and with repeats.
#[test]
fn record_rules() {
    let f = system_file(&settings());
    let end = *record_offsets(&f).last().unwrap();
    let corrupt = Some(BootNote::Error(LoadError::File(FileError::Corrupt)));

    // A Sound record: skipped when non-critical, refused when critical.
    let g = with_record(&f, end, 0x0003, &[]);
    assert_eq!(boot_bytes(&g), (settings(), None));
    let g = with_record(&f, end, 0x8002, &[0]);
    assert_eq!(boot_bytes(&g), (SystemSettings::DEFAULT, corrupt));

    // A project's records, Origin (non-critical) too, are refused.
    for (tag, payload) in [
        (0x8007, vec![0; 17]),
        (0x8008, vec![0; 17]),
        (0x8009, vec![]),
        (0x000A, vec![0, 0]),
    ] {
        let g = with_record(&f, end, tag, &payload);
        assert_eq!(
            boot_bytes(&g),
            (SystemSettings::DEFAULT, corrupt),
            "{tag:#x}"
        );
    }

    // A second THEME block or last project is refused.
    let first = record_offsets(&f)[0];
    let theme_len = 4 + u16::from_le_bytes([f[first + 2], f[first + 3]]) as usize;
    let theme = f[first + 4..first + theme_len].to_vec();
    let g = with_record(&f, end, 0x0001, &theme);
    assert_eq!(boot_bytes(&g), (SystemSettings::DEFAULT, corrupt));
    let g = with_record(&f, end, 0x0006, &9u32.to_le_bytes());
    assert_eq!(boot_bytes(&g), (SystemSettings::DEFAULT, corrupt));

    // A last project of the wrong length is `Bounds`; one out of range is none.
    let no_last = system_file(&SystemSettings {
        last_project: None,
        ..settings()
    });
    let end = *record_offsets(&no_last).last().unwrap();
    let g = with_record(&no_last, end, 0x0006, &[7, 0]);
    let bounds = Some(BootNote::Error(LoadError::File(FileError::Bounds)));
    assert_eq!(boot_bytes(&g), (SystemSettings::DEFAULT, bounds));
    for id in [0, ProjectId::MAX + 1] {
        let g = with_record(&no_last, end, 0x0006, &id.to_le_bytes());
        let want = SystemSettings {
            last_project: None,
            ..settings()
        };
        assert_eq!(boot_bytes(&g), (want, None), "{id}");
    }

    // A Sound file under SYSTEM's name is not SYSTEM.
    let snd = common::codec_util::encode(&chimera_core::factory::factory_sound(0).unwrap());
    let wrong = Some(BootNote::Error(LoadError::File(FileError::WrongKind)));
    assert_eq!(boot_bytes(&snd), (SystemSettings::DEFAULT, wrong));
}

/// Both sides' bytes, `None` for a missing one.
fn sides<S: Store>(s: &mut S) -> [Option<Vec<u8>>; 2] {
    let v = s.mount().unwrap();
    [Side::A, Side::B].map(|side| {
        let mut c = Collect(Vec::new());
        match s.read(v, AbFile::SYSTEM.side(side), &mut c) {
            Ok(()) => Some(c.0),
            Err(StoreError::NotFound) => None,
            Err(e) => panic!("{e:?}"),
        }
    })
}

/// Enter System, then leave it: the leaving frame's verdict.
fn visit(sync: &mut SystemSync, s: &SystemSettings) -> bool {
    assert!(!sync.left_system(true, s));
    sync.left_system(false, s)
}

#[test]
fn left_system_is_the_exit_edge() {
    let mut s = MemStore::new(1);
    let (mut sync, cur, _) = SystemSync::boot(&mut Card::new(), &mut s);
    for _ in 0..3 {
        assert!(!sync.left_system(false, &cur), "out of System");
    }
    for _ in 0..3 {
        assert!(!sync.left_system(true, &cur), "in System");
    }
    assert!(sync.left_system(false, &cur), "the exit");
    assert!(!sync.left_system(false, &cur), "the next frame");
}

/// On the card it came from: a change is written, no change is not.
#[test]
fn same_card_writes_only_a_change() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    let mut card = Card::new();
    let (mut sync, mut cur, note) = SystemSync::boot(&mut card, &mut s);
    assert_eq!((cur, note), (settings(), None));

    let before = sides(&mut s);
    assert!(visit(&mut sync, &cur));
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut cur),
        Ok(Exit::Unchanged)
    );
    assert_eq!(sides(&mut s), before, "no change, no write");

    assert!(!sync.left_system(true, &cur));
    cur.theme.bright = Bright::new(90);
    assert!(sync.left_system(false, &cur));
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Wrote));
    assert_eq!(boot(&mut s).0, cur);

    let before = sides(&mut s);
    assert!(visit(&mut sync, &cur));
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut cur),
        Ok(Exit::Unchanged)
    );
    assert_eq!(
        sides(&mut s),
        before,
        "saved: the next visit writes nothing"
    );
}

/// No card at boot, then one with SYSTEM: leaving System loads it, and the
/// untouched defaults never go over it.
#[test]
fn late_card_loads_its_system() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    s.eject();
    let mut card = Card::new();
    let (mut sync, mut cur, note) = SystemSync::boot(&mut card, &mut s);
    assert_eq!(
        (cur, note),
        (SystemSettings::DEFAULT, Some(BootNote::NoCard))
    );

    s.insert();
    let before = sides(&mut s);
    assert!(visit(&mut sync, &cur));
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Loaded));
    assert_eq!(sides(&mut s), before, "the card's SYSTEM is untouched");
    assert_eq!(cur, settings(), "RAM holds the card's");

    // Loaded from this card: no change is no write.
    assert!(visit(&mut sync, &cur));
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut cur),
        Ok(Exit::Unchanged)
    );
}

/// Loaded from card A, then a fresh card B: A's settings go to B.
#[test]
fn swap_writes_to_the_new_card() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);

    s.swap(2);
    assert!(visit(&mut sync, &cur));
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Wrote));
    assert_eq!(cur, settings());
    assert_eq!(boot(&mut s), (settings(), None, card));
}

/// A read that fails at boot leaves the defaults untouched: leaving System
/// with no change writes nothing and loads the card's SYSTEM; a change is
/// written.
#[test]
fn transient_boot_error_writes_nothing() {
    let mut inner = MemStore::new(1);
    save(&mut inner, &settings());
    let before = sides(&mut inner);
    let mut s = ReadFails::new(inner, 1, Fail::Io);
    let mut card = Card::new();
    let (mut sync, mut cur, note) = SystemSync::boot(&mut card, &mut s);
    assert_eq!(cur, SystemSettings::DEFAULT);
    assert_eq!(
        note,
        Some(BootNote::Error(LoadError::Store(StoreError::Io)))
    );

    assert!(visit(&mut sync, &cur));
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Loaded));
    assert_eq!(sides(&mut s.inner), before, "nothing written");
    assert_eq!(cur, settings());

    assert!(!sync.left_system(true, &cur));
    cur.theme.black = Black::new(4);
    assert!(sync.left_system(false, &cur));
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Wrote));
    assert_eq!(boot(&mut s.inner).0, cur);
}

/// The same with a card that stays bad: nothing loads and nothing is written.
#[test]
fn unreadable_card_with_untouched_defaults_writes_nothing() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    save(&mut s, &settings());
    for side in [Side::A, Side::B] {
        let mut f = get(&mut s, side);
        let last = f.len() - 1;
        f[last] ^= 0x01;
        put(&mut s, side, &f);
    }
    let before = sides(&mut s);
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    assert!(visit(&mut sync, &cur));
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut cur),
        Err(SyncError::File(FileError::BadCrc))
    );
    assert_eq!(sides(&mut s), before);
    assert_eq!(cur, SystemSettings::DEFAULT);
}

/// Case 8: a card whose SYSTEM needs newer firmware, with no header this
/// firmware reads (version 2) or with one (an unknown critical record).
/// Neither untouched defaults nor a change go over it, and RAM keeps its own.
#[test]
fn newer_firmware_system_is_never_written() {
    let f = system_file(&settings());
    let mut headerless = f.clone();
    headerless[4..6].copy_from_slice(&2u16.to_le_bytes());
    fix_crc(&mut headerless);
    let end = *record_offsets(&f).last().unwrap();
    let present = with_record(&f, end, 0x8071, &[0; 4]);
    let nnf = Err(SyncError::File(FileError::NeedsNewerFirmware));

    for (shape, bytes) in [("headerless", headerless), ("present", present)] {
        for change in [false, true] {
            let mut s = MemStore::new(1);
            put(&mut s, Side::A, &bytes);
            let before = sides(&mut s);
            let mut card = Card::new();
            let (mut sync, mut cur, note) = SystemSync::boot(&mut card, &mut s);
            assert_eq!(
                note,
                Some(BootNote::Error(LoadError::File(
                    FileError::NeedsNewerFirmware
                ))),
                "{shape}"
            );

            assert!(!sync.left_system(true, &cur));
            if change {
                cur.theme.bright = Bright::new(90);
            }
            let want = cur;
            assert!(sync.left_system(false, &cur));
            assert_eq!(
                sync.on_exit(&mut card, &mut s, &mut cur),
                nnf,
                "{shape} {change}"
            );
            assert_eq!(sides(&mut s), before, "{shape} {change}: written");
            assert_eq!(cur, want, "{shape} {change}: RAM changed");
            assert!(matches!(card, Card::Ready(_)), "{shape} {change}: {card:?}");
        }
    }
}

#[test]
fn no_file_boot_creates_on_first_exit() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut cur, note) = SystemSync::boot(&mut card, &mut s);
    assert_eq!(note, Some(BootNote::NoFile));
    assert!(visit(&mut sync, &cur));
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Wrote));
    assert_eq!(boot(&mut s), (SystemSettings::DEFAULT, None, card));
    assert!(visit(&mut sync, &cur));
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut cur),
        Ok(Exit::Unchanged)
    );
}

/// A change undone is still the user's: it goes over another card's SYSTEM.
#[test]
fn a_reverted_change_is_still_a_change() {
    let mut s = MemStore::new(1);
    save(&mut s, &settings());
    s.eject();
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    assert!(!sync.left_system(true, &cur));
    cur.theme.accent = Accent::Ice;
    assert!(!sync.left_system(true, &cur));
    cur.theme.accent = SystemSettings::DEFAULT.theme.accent;
    assert!(sync.left_system(false, &cur));

    s.insert();
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Wrote));
    assert_eq!(boot(&mut s).0, SystemSettings::DEFAULT);
}

#[test]
fn exit_plan_table() {
    let (a, b) = (vol(1), vol(2));
    let (crc, other) = (0x1234, 0x5678);
    let cases = [
        // known on this card
        (Some((a, crc)), false, ExitPlan::Nothing),
        (Some((a, other)), false, ExitPlan::Write),
        // another card, or none known
        (Some((b, crc)), false, ExitPlan::Write),
        (None, false, ExitPlan::Write),
        (Some((b, crc)), true, ExitPlan::Load),
        (None, true, ExitPlan::Load),
    ];
    for (known, untouched, want) in cases {
        assert_eq!(
            exit_plan(known, untouched, a, crc),
            want,
            "{known:?} {untouched}"
        );
    }
}

fn vol(serial: u32) -> VolumeId {
    VolumeId {
        serial,
        label: *b"NO NAME    ",
    }
}

/// Review Focus 3: with no card, leaving System tries once, and the next
/// exit tries again.
#[test]
fn no_card_exit_tries_once() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut cur, note) = SystemSync::boot(&mut card, &mut s);
    assert_eq!(note, Some(BootNote::NoFile));
    s.eject();

    assert!(visit(&mut sync, &cur), "the exit edge");
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut cur),
        Err(SyncError::Store(StoreError::NoCard))
    );
    assert_eq!(card, Card::Absent);
    for _ in 0..10 {
        assert!(!sync.left_system(false, &cur), "out of System");
    }
    assert!(visit(&mut sync, &cur), "the next exit");

    // The card back: that exit creates the file.
    s.insert();
    assert_eq!(sync.on_exit(&mut card, &mut s, &mut cur), Ok(Exit::Wrote));
    assert!(matches!(card, Card::Ready(_)), "{card:?}");
    assert_eq!(boot(&mut s), (cur, None, card));
}

#[test]
fn system_fixture_loads() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v1")
        .join(SYSTEM_FIXTURE);
    let f = std::fs::read(p).unwrap();
    assert_eq!(boot_bytes(&f), (system_fixture_settings(), None));
}
