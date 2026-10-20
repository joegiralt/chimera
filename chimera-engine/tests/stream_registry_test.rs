//! The stream registry (slice 1 § 5.5): a row per streamed source, eight
//! at most.

use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::name::SoundName;
use chimera_engine::project::SourceId;
use chimera_engine::storage::{SourceHead, SourceLayout, SourceRef, data_offset};
use chimera_engine::stream::layout::StreamFile;
use chimera_engine::stream::registry::{RowRefused, STREAM_ROWS, StreamRegistry};
use chimera_engine::stream::units::Frames;
use chimera_hal::store::{FileRuns, Runs};
use chimera_hal::units::{BlockCount, Bytes, Lba};

/// A `Plain` file of `frames` in one run: its frames tell rows apart.
fn file(frames: u32) -> StreamFile {
    let head = SourceHead {
        sref: SourceRef {
            crc: frames,
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("ROW").unwrap(),
    };
    let l = SourceLayout::whole(head).unwrap();
    let data = l.data_len() as u32;
    let len = data_offset(&l).get() + data + 4 * data.div_ceil(504);
    let mut runs = Runs::new();
    runs.push(Lba::new(1_000), BlockCount::new(len.div_ceil(512)).unwrap());
    let runs = FileRuns {
        ext: runs.finish().unwrap(),
        len: Bytes::new(len),
    };
    StreamFile::from_layout(&l, runs).unwrap()
}

fn id(n: u8) -> SourceId {
    SourceId::new(n).unwrap()
}

fn frames(r: &StreamRegistry, n: u8) -> Option<u32> {
    r.get(id(n)).map(|f| f.frames().get())
}

#[test]
fn registry_insert_get_clear() {
    let r = StreamRegistry::new();
    assert_eq!(frames(&r, 3), None);
    r.insert(id(3), file(5_000)).unwrap();
    r.insert(id(31), file(7_000)).unwrap();
    assert_eq!(frames(&r, 3), Some(5_000));
    assert_eq!(frames(&r, 31), Some(7_000));
    assert_eq!(frames(&r, 0), None);
    // SAFETY: nothing reads `r` but this test, between calls.
    assert!(unsafe { r.clear(id(3)) });
    assert_eq!(frames(&r, 3), None);
    assert_eq!(frames(&r, 31), Some(7_000), "another row stays");
    r.insert(id(3), file(6_000)).unwrap();
    assert_eq!(frames(&r, 3), Some(6_000), "a cleared row takes a new file");
}

#[test]
fn ninth_insert_is_full() {
    let r = StreamRegistry::new();
    for n in 0..STREAM_ROWS as u8 {
        r.insert(id(n), file(1_000 + u32::from(n))).unwrap();
    }
    assert_eq!(r.insert(id(20), file(9_000)), Err(RowRefused::Full));
    assert_eq!(frames(&r, 20), None);
    // SAFETY: as above.
    assert!(unsafe { r.clear(id(5)) });
    r.insert(id(20), file(9_000)).unwrap();
    assert_eq!(frames(&r, 20), Some(9_000));
    assert_eq!(frames(&r, 7), Some(1_007));
}

#[test]
fn clear_all_empties() {
    let r = StreamRegistry::new();
    for n in 0..STREAM_ROWS as u8 {
        r.insert(id(n), file(2_000)).unwrap();
    }
    // SAFETY: as above.
    assert!(unsafe { r.clear_all() });
    assert!((0..STREAM_ROWS as u8).all(|n| frames(&r, n).is_none()));
    for n in 0..STREAM_ROWS as u8 {
        r.insert(id(n + 8), file(3_000)).unwrap();
    }
}

/// The board keeps it NOLOAD in D2 and zeroes it at boot.
#[test]
fn zeroed_registry_is_empty() {
    // SAFETY: all-zero bytes are an empty registry (`StreamRegistry`'s doc).
    let r: StreamRegistry = unsafe { core::mem::zeroed() };
    assert!(
        chimera_engine::project::SourceId::ALL
            .iter()
            .all(|&s| r.get(s).is_none())
    );
    r.insert(id(0), file(4_000)).unwrap();
    assert_eq!(
        r.get(id(0)).map(StreamFile::frames),
        Some(Frames::new(4_000))
    );
}

/// What the store's guard asks: a live row's `.SRC`, and no other.
#[test]
fn a_row_streams_its_file() {
    use chimera_engine::storage::source_name;
    let r = StreamRegistry::new();
    let f = file(5_000);
    assert!(!r.streams(f.file()));
    r.insert(id(2), f).unwrap();
    assert!(r.streams(source_name(5_000)));
    assert!(!r.streams(source_name(5_001)));
    // SAFETY: nothing reads `r` but this test.
    assert!(unsafe { r.clear(id(2)) });
    assert!(!r.streams(f.file()));
}

/// One row an id (ruling 6): a second insert under it is refused, and the
/// first row is untouched.
#[test]
fn a_taken_id_is_refused() {
    let r = StreamRegistry::new();
    r.insert(id(4), file(5_000)).unwrap();
    assert_eq!(r.insert(id(4), file(6_000)), Err(RowRefused::Taken));
    assert_eq!(frames(&r, 4), Some(5_000));
    r.insert(id(5), file(6_000)).unwrap();
    // SAFETY: nothing reads `r` but this test.
    assert!(unsafe { r.clear(id(4)) });
    r.insert(id(4), file(7_000)).unwrap();
    assert_eq!(frames(&r, 4), Some(7_000));
}

/// #597: the card IRQ's hold keeps a row from being freed under its read.
/// `clear` leaves it (the pool's drain waits a pass), and frees it once
/// the hold drops; the row's file stays guarded meanwhile.
#[test]
fn a_held_row_is_not_freed() {
    let r = StreamRegistry::new();
    let f = file(5_000);
    r.insert(id(3), f).unwrap();
    r.insert(id(4), file(6_000)).unwrap();
    let held = r.hold(id(3)).unwrap();
    assert_eq!(held.file().frames(), Frames::new(5_000));
    // SAFETY: nothing reads `r` but this test and its hold.
    assert!(!unsafe { r.clear(id(3)) }, "held: left");
    assert_eq!(frames(&r, 3), Some(5_000));
    assert!(r.streams(f.file()));
    assert!(unsafe { r.clear(id(4)) }, "another row frees");
    assert_eq!(r.insert(id(3), file(7_000)), Err(RowRefused::Taken));
    drop(held);
    assert!(unsafe { r.clear(id(3)) });
    assert_eq!(frames(&r, 3), None);
}

/// One hold at a time, and none on a row that isn't there.
#[test]
fn one_hold_at_a_time() {
    let r = StreamRegistry::new();
    r.insert(id(1), file(1_000)).unwrap();
    r.insert(id(2), file(2_000)).unwrap();
    assert!(r.hold(id(9)).is_none());
    let a = r.hold(id(1)).unwrap();
    assert!(r.hold(id(2)).is_none(), "the card holds one row");
    drop(a);
    let b = r.hold(id(2)).unwrap();
    assert_eq!(b.file().frames(), Frames::new(2_000));
}

/// `clear_all` leaves a held row too, its read finishing on it; but the
/// row is the last card's (M5): no `get` or `hold` finds it again, so no
/// later read lands its blocks on the new card, and its id stays taken
/// until a later `clear` frees it.
#[test]
fn clear_all_leaves_a_held_row_stale() {
    let r = StreamRegistry::new();
    for n in 0..3 {
        r.insert(id(n), file(2_000 + u32::from(n))).unwrap();
    }
    let held = r.hold(id(1)).unwrap();
    // SAFETY: nothing reads `r` but this test and its hold.
    assert!(!unsafe { r.clear_all() });
    assert_eq!(frames(&r, 0), None);
    assert_eq!(
        held.file().frames(),
        Frames::new(2_001),
        "the read in flight"
    );
    drop(held);
    assert_eq!(frames(&r, 1), None, "the last card's");
    assert!(r.hold(id(1)).is_none());
    assert_eq!(r.insert(id(1), file(3_000)), Err(RowRefused::Taken));
    // SAFETY: as above.
    assert!(unsafe { r.clear(id(1)) });
    r.insert(id(1), file(3_000)).unwrap();
    assert_eq!(frames(&r, 1), Some(3_000));
}
