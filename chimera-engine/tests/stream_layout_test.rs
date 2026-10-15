//! Streamed chunks in a `.source` file: where a chunk lies, and de-framing
//! the blocks that hold it.

use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::name::SoundName;
use chimera_engine::storage::{
    Card, SaveError, SourceHead, SourceLayout, SourceRef, SourceZones, WholeShape, Zone,
    data_offset, source_crc, source_file, write_source, write_zone_set, zone_set_crc,
};
use chimera_engine::stream::layout::{
    CHUNK_BYTES, CHUNK_FRAMES, ChunkIdx, Deframe, FRAMED_CHUNK_BYTES, RAW_CHUNK_BLOCKS,
    RAW_CHUNK_BYTES, StreamFile, deframe,
};
use chimera_engine::stream::units::Frames;
use chimera_hal::store::{FileRuns, ReadSink, Runs, Store};
use chimera_hal::testkit::MemStore;
use chimera_hal::units::{BlockCount, Bytes, Lba};
use core::ops::ControlFlow;
use proptest::prelude::*;

const PAIRS: usize = 3;

fn packed(seed: u32, frames: u32) -> Vec<u8> {
    (0..PAIRS * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed) as u8)
        .collect()
}

/// A `Plain` source of `frames`, or a `ZoneSet` of `zones` equal ones.
fn layout(seed: u32, frames: u32, zones: u8) -> (SourceLayout, Option<SourceZones>, Vec<u8>) {
    let data = packed(seed, frames);
    let mut head = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("CHUNKS").unwrap(),
    };
    if zones == 0 {
        return (SourceLayout::whole(head).unwrap(), None, data);
    }
    let each = frames / u32::from(zones);
    let zs: Vec<Zone> = (0..u32::from(zones))
        .map(|i| Zone {
            start: i * each,
            frames: if i + 1 == u32::from(zones) {
                frames - i * each
            } else {
                each
            },
            key: 24 + 4 * i as u8,
            root: 3_600,
            period_q16: 0,
        })
        .collect();
    let z = SourceZones::new(&zs).unwrap();
    head.shape = Shape::ZoneSet;
    head.count = zones;
    head.sref.crc = zone_set_crc(&z, &data);
    (SourceLayout::zoned(head, z).unwrap(), Some(z), data)
}

struct Bytes_(Vec<u8>);

impl ReadSink for Bytes_ {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(b);
        ControlFlow::Continue(())
    }
}

/// `write_source` / `write_zone_set`'s file, read back whole.
fn written(l: &SourceLayout, z: Option<SourceZones>, data: &[u8]) -> Vec<u8> {
    let mut s = MemStore::new(1);
    let file = source_file(l.head().sref);
    Card::new()
        .run(&mut s, |s, r| {
            match &z {
                Some(z) => write_zone_set(s, r, l.head(), z, data),
                None => write_source(s, r, l.head(), data),
            }
            .unwrap();
            let mut out = Bytes_(Vec::new());
            s.read(r.volume(), file, &mut out).unwrap();
            Ok::<_, SaveError>(out.0)
        })
        .unwrap()
        .result
        .unwrap()
}

fn first_data_record(file: &[u8]) -> usize {
    let mut p = 28;
    while u16::from_le_bytes([file[p], file[p + 1]]) != 0x0018 {
        p += 4 + usize::from(u16::from_le_bytes([file[p + 2], file[p + 3]]));
    }
    p
}

/// The file's blocks split into `cuts` + 1 runs, laid on the device in
/// reverse so no two runs touch; the device image to match.
fn on_device(file: &[u8], cuts: &[u32]) -> (FileRuns, Vec<u8>) {
    let blocks = file.len().div_ceil(512) as u32;
    let mut at: Vec<u32> = cuts.iter().map(|c| 1 + c % (blocks.max(2) - 1)).collect();
    at.push(0);
    at.push(blocks);
    at.sort_unstable();
    at.dedup();
    let mut runs = Runs::new();
    let mut dev = vec![0u8; blocks as usize * 512];
    for w in at.windows(2) {
        let (a, b) = (w[0], w[1]);
        let lba = blocks - b;
        runs.push(Lba::new(lba), BlockCount::new(b - a).unwrap());
        let (src, dst) = (a as usize * 512, lba as usize * 512);
        let end = (b as usize * 512).min(file.len());
        dev[dst..dst + (end - src)].copy_from_slice(&file[src..end]);
    }
    let ext = runs.finish().unwrap();
    (
        FileRuns {
            ext,
            len: Bytes::new(file.len() as u32),
        },
        dev,
    )
}

fn raw_of(sf: &StreamFile, c: ChunkIdx, dev: &[u8]) -> (Vec<u8>, Bytes) {
    let b = sf.chunk_blocks(c);
    let mut raw = Vec::new();
    for (lba, n) in sf.reads(&b) {
        let at = lba.get() as usize * 512;
        raw.extend_from_slice(&dev[at..at + n.get() as usize * 512]);
    }
    assert!(b.count.get() <= RAW_CHUNK_BLOCKS);
    (raw, b.skip)
}

fn frames_strategy() -> impl Strategy<Value = u32> {
    // 4 MiB of packed bytes is 2,796,202 frames; most cases stay small.
    prop_oneof![
        8 => 64u32..6_000,
        3 => 6_000u32..120_000,
        1 => 120_000u32..2_796_000,
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn chunks_match_whole_deframe(
        frames in frames_strategy(),
        zones in prop_oneof![Just(0u8), 1u8..=8],
        seed in any::<u32>(),
        cuts in proptest::collection::vec(any::<u32>(), 0..32),
    ) {
        let zones = zones.min((frames / 64) as u8);
        let (l, z, data) = layout(seed, frames, zones);
        let file = written(&l, z, &data);
        let (runs, dev) = on_device(&file, &cuts);
        let sf = StreamFile::from_layout(&l, runs).unwrap();
        let mut whole = vec![0; data.len()];
        let n = deframe(&file, data_offset(&l), &mut whole).unwrap();
        prop_assert_eq!(n.get() as usize, data.len());
        prop_assert_eq!(&whole, &data);
        let mut c = ChunkIdx::new(0).unwrap();
        loop {
            let at = c.get() as usize * CHUNK_BYTES.get() as usize;
            let len = (data.len() - at).min(CHUNK_BYTES.get() as usize);
            let (raw, skip) = raw_of(&sf, c, &dev);
            let mut out = vec![0; len];
            let got = deframe(&raw, skip, &mut out).unwrap();
            prop_assert_eq!(got.get() as usize, len);
            prop_assert_eq!(&out[..], &whole[at..at + len]);
            if c == sf.last_chunk() { break; }
            c = c.next().unwrap();
        }
        prop_assert_eq!(sf.frames(), Frames::new(frames));
    }
}

#[test]
fn data_offset_matches_the_writer() {
    for zones in [0, 3] {
        let (l, z, data) = layout(7, 1_000, zones);
        let file = written(&l, z, &data);
        assert_eq!(
            data_offset(&l),
            Bytes::new(first_data_record(&file) as u32),
            "{zones} zones"
        );
    }
}

fn framed(payloads: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in payloads {
        v.extend_from_slice(&0x0018u16.to_le_bytes());
        v.extend_from_slice(&(p.len() as u16).to_le_bytes());
        v.extend_from_slice(p);
    }
    v
}

#[test]
fn corrupt_tag_is_an_error() {
    let p = [5u8; 504];
    let mut raw = framed(&[&p, &p]);
    let mut out = [0; 1008];
    assert_eq!(deframe(&raw, Bytes::new(0), &mut out), Ok(Bytes::new(1008)));
    raw[508..510].copy_from_slice(&0x0019u16.to_le_bytes());
    assert_eq!(deframe(&raw, Bytes::new(0), &mut out), Err(Deframe::Tag));
    raw[508..510].copy_from_slice(&0x0018u16.to_le_bytes());
    raw[510..512].copy_from_slice(&505u16.to_le_bytes());
    assert_eq!(deframe(&raw, Bytes::new(0), &mut out), Err(Deframe::Length));
    let ok = framed(&[&p, &p]);
    assert_eq!(
        deframe(&ok[..600], Bytes::new(0), &mut out),
        Err(Deframe::Short)
    );
}

#[test]
fn last_chunk_is_short() {
    let f = CHUNK_FRAMES.get() * 2 + 10;
    let (l, _, data) = layout(1, f, 0);
    let file = written(&l, None, &data);
    let (runs, _) = on_device(&file, &[]);
    let sf = StreamFile::from_layout(&l, runs).unwrap();
    let last = sf.last_chunk();
    assert_eq!(last.get(), 2);
    let r = sf.chunk_frames(last);
    assert_eq!(r.start, Frames::new(CHUNK_FRAMES.get() * 2));
    assert_eq!(r.end, Frames::new(f));
    assert_eq!(sf.chunk_of(Frames::new(f - 1)), Some(last));
    assert_eq!(sf.chunk_of(Frames::new(f)), None);
}

#[test]
fn chunk_index_holds_24_bits() {
    assert!(ChunkIdx::new(0xFF_FFFF).is_some());
    assert!(ChunkIdx::new(0x100_0000).is_none());
    assert_eq!(ChunkIdx::MAX.next(), None);
    assert_eq!(ChunkIdx::new(4).unwrap().next(), ChunkIdx::new(5));
}

#[test]
fn chunk_constants_derive_from_the_record_count() {
    assert_eq!(CHUNK_BYTES, Bytes::new(4_032));
    assert_eq!(CHUNK_FRAMES, Frames::new(2_688));
    assert_eq!(FRAMED_CHUNK_BYTES, Bytes::new(4_064));
    assert_eq!(RAW_CHUNK_BLOCKS, 9);
    assert_eq!(RAW_CHUNK_BYTES, 4_608);
}
