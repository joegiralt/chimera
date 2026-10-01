use chimera_core::name::Name;
use chimera_core::storage::{
    CRITICAL, Crc32, Event, FileError, FileKind, Framer, Generation, HEADER_LEN, Header,
    MAX_RECORD_LEN, ProjectId, ReadTag, RecordBuf, RecordTag, Side, TRAILER_LEN, write_file,
};
use chimera_hal::store::{ByteSink, StoreError};

struct VecSink(Vec<u8>);

impl ByteSink for VecSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
enum Got {
    Header(Header),
    Record(ReadTag, Vec<u8>),
}

fn header() -> Header {
    Header {
        kind: FileKind::Sound,
        generation: Generation::new(7),
        name: Some(Name::new("ACID 303").unwrap()),
    }
}

fn file(records: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &header(), &mut |w| {
        records.iter().try_for_each(|(t, p)| w.put(*t, p))
    })
    .unwrap();
    sink.0
}

/// Header, then raw record bytes, then a correct CRC trailer.
fn raw_file(body: &[u8]) -> Vec<u8> {
    let mut v = file(&[]);
    v.truncate(HEADER_LEN);
    v.extend_from_slice(body);
    refresh_crc(v)
}

fn refresh_crc(mut v: Vec<u8>) -> Vec<u8> {
    let mut crc = Crc32::new();
    crc.update(&v);
    v.extend_from_slice(&crc.finish().to_le_bytes());
    v
}

fn record(tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut v = tag.to_le_bytes().to_vec();
    v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

fn read_with(bytes: &[u8], len: u32, chunk: usize) -> Result<Vec<Got>, FileError> {
    let mut got = Vec::new();
    let mut f = Framer::new(len)?;
    for c in bytes.chunks(chunk.max(1)) {
        f.push(c, &mut |e| {
            got.push(match e {
                Event::Header(h) => Got::Header(h),
                Event::Record(t, p) => Got::Record(t, p.to_vec()),
            });
            Ok(())
        })?;
    }
    f.finish()?;
    Ok(got)
}

fn read(bytes: &[u8], chunk: usize) -> Result<Vec<Got>, FileError> {
    read_with(bytes, bytes.len() as u32, chunk)
}

#[test]
fn crc_known_answer() {
    let mut c = Crc32::new();
    c.update(b"123456789");
    assert_eq!(c.finish(), 0xCBF4_3926);
    let mut split = Crc32::new();
    split.update(b"1234");
    split.update(b"56789");
    assert_eq!(split.finish(), 0xCBF4_3926);
}

#[test]
fn generation_wraps() {
    let last = Generation::new(u32::MAX);
    assert_eq!(last.next(), Generation::new(0));
    assert!(last.next().is_newer_than(last));
    assert!(!last.is_newer_than(last.next()));
    assert!(!last.is_newer_than(last));
    assert_eq!(Generation::FIRST.next().get(), Generation::FIRST.get() + 1);
}

#[test]
fn side_ext_and_other() {
    assert_eq!(Side::A.ext(), b"A");
    assert_eq!(Side::B.ext(), b"B");
    assert_eq!(Side::A.other(), Side::B);
    assert_eq!(Side::B.other(), Side::A);
}

#[test]
fn project_id_bounds_and_stem() {
    assert_eq!(ProjectId::new(0), None);
    assert_eq!(ProjectId::new(10_000_000), None);
    assert_eq!(ProjectId::new(42).unwrap().stem(), *b"P0000042");
    assert_eq!(ProjectId::new(9_999_999).unwrap().stem(), *b"P9999999");
    assert_eq!(ProjectId::new(1).unwrap().get(), 1);
}

#[test]
fn header_round_trip() {
    let bytes = file(&[]);
    assert_eq!(bytes.len(), HEADER_LEN + TRAILER_LEN);
    assert_eq!(bytes.len(), 32);
    assert_eq!(&bytes[..4], b"CHIM");
    for chunk in [1, bytes.len()] {
        assert_eq!(read(&bytes, chunk), Ok(vec![Got::Header(header())]));
    }
}

#[test]
fn unnamed_header_round_trips() {
    let h = Header {
        kind: FileKind::System,
        generation: Generation::FIRST,
        name: None,
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |_| Ok(())).unwrap();
    assert!(sink.0[12..28].iter().all(|&b| b == 0));
    assert_eq!(read(&sink.0, 5), Ok(vec![Got::Header(h)]));
}

#[test]
fn records_straddle_chunks() {
    let pay = |n: usize| (0..n).map(|i| (i * 7 + n) as u8).collect::<Vec<u8>>();
    let records = [
        (RecordTag::Engine, pay(1)),
        (RecordTag::Block, pay(300)),
        (RecordTag::Routes, pay(MAX_RECORD_LEN)),
    ];
    let bytes = file(&records);
    let mut want = vec![Got::Header(header())];
    want.extend(
        records
            .iter()
            .map(|(t, p)| Got::Record(ReadTag::Known(*t), p.clone())),
    );
    for chunk in [1, 7, 512] {
        assert_eq!(read(&bytes, chunk).as_ref(), Ok(&want), "chunk {chunk}");
    }
}

#[test]
fn record_buf_is_little_endian() {
    let mut b = RecordBuf::new();
    b.u8(9);
    b.u32(0x0102_0304);
    b.f32(1.0);
    b.bytes(b"AB");
    assert_eq!(
        b.as_slice(),
        &[9, 4, 3, 2, 1, 0x00, 0x00, 0x80, 0x3F, b'A', b'B']
    );
}

#[test]
fn codes_are_frozen() {
    let table = [
        (RecordTag::Block, 0x0001),
        (RecordTag::Engine, 0x8002),
        (RecordTag::Registry, 0x0003),
        (RecordTag::ModDests, 0x0004),
        (RecordTag::Routes, 0x0005),
        (RecordTag::LastProject, 0x0006),
        (RecordTag::Slot, 0x8007),
        (RecordTag::Part, 0x8008),
        (RecordTag::Fx, 0x8009),
        (RecordTag::Origin, 0x000A),
    ];
    for (t, c) in table {
        assert_eq!(t.code(), c);
        assert_eq!(RecordTag::from_code(c), Some(t));
        assert_eq!(ReadTag::Known(t).critical(), c & CRITICAL != 0);
    }
    assert!(ReadTag::Known(RecordTag::Engine).critical());
    assert_eq!(RecordTag::from_code(0x0077), None);
    assert!(ReadTag::Unknown(0x8077).critical());
    assert!(!ReadTag::Unknown(0x0077).critical());
    assert_eq!(FileKind::Sound as u8, 1);
    assert_eq!(FileKind::Project as u8, 2);
    assert_eq!(FileKind::System as u8, 3);
}

#[test]
fn unknown_non_critical_skipped_even_if_long() {
    let mut body = record(0x0077, &[0xAB; 4_000]);
    body.extend(record(RecordTag::Block.code(), &[1, 2]));
    let bytes = raw_file(&body);
    for chunk in [1, 512] {
        assert_eq!(
            read(&bytes, chunk),
            Ok(vec![
                Got::Header(header()),
                Got::Record(ReadTag::Unknown(0x0077), vec![]),
                Got::Record(ReadTag::Known(RecordTag::Block), vec![1, 2]),
            ])
        );
    }
}

#[test]
fn unknown_critical_needs_newer_firmware() {
    let bytes = raw_file(&record(0x8077, &[0; 4_000]));
    assert_eq!(read(&bytes, 512), Err(FileError::NeedsNewerFirmware));
}

#[test]
fn known_record_over_max_is_bounds() {
    let bytes = raw_file(&record(RecordTag::Block.code(), &[0; MAX_RECORD_LEN + 1]));
    assert_eq!(read(&bytes, 512), Err(FileError::Bounds));
}

#[test]
fn length_past_end_is_bounds() {
    let mut body = record(RecordTag::Block.code(), &[0; 8]);
    body[2..4].copy_from_slice(&9u16.to_le_bytes());
    assert_eq!(read(&raw_file(&body), 512), Err(FileError::Bounds));
    // A head that doesn't fit before the trailer.
    assert_eq!(read(&raw_file(&[1, 0]), 1), Err(FileError::Bounds));
}

#[test]
fn bad_magic() {
    let mut bytes = file(&[]);
    bytes[0] = b'X';
    assert_eq!(
        read(&refresh_crc(bytes[..28].to_vec()), 512),
        Err(FileError::BadMagic)
    );
}

#[test]
fn bad_crc() {
    let bytes = file(&[(RecordTag::Block, vec![1, 2, 3])]);
    for i in [0, 8, 32, 34, bytes.len() - 1] {
        let mut b = bytes.clone();
        b[i] ^= 0x01;
        assert_eq!(read(&b, 512), Err(FileError::BadCrc), "byte {i}");
    }
}

/// Every verdict waits for the CRC: a flipped bit is torn, never a verdict
/// that would shadow the other side.
#[test]
fn stale_crc_beats_every_verdict() {
    let bytes = file(&[(RecordTag::Block, vec![1, 2, 3])]);
    let flip = |i: usize, x: u8| {
        let mut b = bytes.clone();
        b[i] ^= x;
        b
    };
    // Tag 0x0001 → 0x8001, an unknown critical tag.
    assert_eq!(read(&flip(29, 0x80), 1), Err(FileError::BadCrc));
    // Version 1 → 3.
    assert_eq!(read(&flip(4, 0x02), 1), Err(FileError::BadCrc));
    // Length 3 → 0x0103, past the end.
    assert_eq!(read(&flip(31, 0x01), 1), Err(FileError::BadCrc));
    // Kind, flags and name.
    for i in [6, 7, 12] {
        assert_eq!(read(&flip(i, 0x40), 7), Err(FileError::BadCrc), "byte {i}");
    }
}

#[test]
fn every_single_bit_flip_is_torn() {
    let bytes = file(&[
        (RecordTag::Engine, vec![2]),
        (RecordTag::Block, (0..200).map(|i| i as u8).collect()),
        (RecordTag::LastProject, 42u32.to_le_bytes().to_vec()),
    ]);
    for bit in 0..bytes.len() * 8 {
        let mut b = bytes.clone();
        b[bit / 8] ^= 1 << (bit % 8);
        for chunk in [1, 512] {
            assert_eq!(read(&b, chunk), Err(FileError::BadCrc), "bit {bit}");
        }
    }
}

#[test]
fn truncated() {
    let bytes = file(&[(RecordTag::Block, vec![1, 2, 3])]);
    let short = &bytes[..bytes.len() - 1];
    assert_eq!(
        read_with(short, bytes.len() as u32, 1),
        Err(FileError::Truncated)
    );
    let empty = file(&[]);
    assert_eq!(
        read(&empty[..empty.len() - 1], 512),
        Err(FileError::Truncated)
    );
}

#[test]
fn bytes_past_the_length_are_corrupt() {
    let mut bytes = file(&[]);
    let len = bytes.len() as u32;
    bytes.push(0);
    assert_eq!(read_with(&bytes, len, 512), Err(FileError::Corrupt));
}

#[test]
fn newer_version() {
    let mut bytes = file(&[]);
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    let bytes = refresh_crc(bytes[..28].to_vec());
    assert_eq!(read(&bytes, 512), Err(FileError::NeedsNewerFirmware));
}

#[test]
fn flags_and_kind() {
    let mut b = file(&[]);
    b[7] = 1;
    assert_eq!(
        read(&refresh_crc(b[..28].to_vec()), 512),
        Err(FileError::Corrupt)
    );
    // 4 Tags is reserved, not yet read.
    let mut b = file(&[]);
    b[6] = 4;
    assert_eq!(
        read(&refresh_crc(b[..28].to_vec()), 512),
        Err(FileError::WrongKind)
    );
}

#[test]
fn bad_name() {
    let mut bytes = file(&[]);
    bytes[12] = b'(';
    let bytes = refresh_crc(bytes[..28].to_vec());
    assert_eq!(read(&bytes, 512), Err(FileError::BadName));
}

#[test]
fn errors_stick_and_callbacks_can_stop() {
    let bytes = file(&[(RecordTag::Block, vec![1])]);
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    let records = std::cell::Cell::new(0);
    let (head, tail) = bytes.split_at(34);
    let mut on = |e: Event<'_>| match e {
        Event::Record(..) => {
            records.set(records.get() + 1);
            Err(FileError::Bounds)
        }
        Event::Header(_) => Ok(()),
    };
    // The verdict is deferred to `finish`, which checks the CRC first.
    assert_eq!(f.push(head, &mut on), Ok(()));
    assert_eq!(f.push(tail, &mut on), Ok(()));
    assert_eq!(records.get(), 1);
    assert_eq!(f.finish(), Err(FileError::Bounds));
    // Bytes past the length stick.
    assert_eq!(f.push(&[0], &mut on), Err(FileError::Corrupt));
    assert_eq!(f.push(&[], &mut on), Err(FileError::Corrupt));
    assert_eq!(f.finish(), Err(FileError::Corrupt));
}

#[test]
fn torn_errors_and_messages() {
    use FileError::*;
    let all = [
        Truncated,
        BadMagic,
        BadCrc,
        NeedsNewerFirmware,
        WrongKind,
        Bounds,
        BadName,
        Corrupt,
    ];
    for e in all {
        assert_eq!(e.is_torn(), matches!(e, Truncated | BadCrc));
        let m = e.message();
        assert!(!m.is_empty() && m.bytes().all(|b| !b.is_ascii_lowercase()));
    }
    assert_eq!(NeedsNewerFirmware.message(), "NEEDS NEWER FIRMWARE");
}
