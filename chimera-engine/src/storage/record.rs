//! TLV records: `u16` tag, `u16` length, the payload, all little-endian.

use chimera_hal::store::{ByteSink, StoreError};

use super::Crc32;
use super::frame::Header;

/// The largest payload a known record may have; the framer buffers one.
/// ADR 0102: a full matrix's `Routes`, 8 sources × (16 + 4 fixed) columns.
pub const MAX_RECORD_LEN: usize = 640;

/// Tag bit 15, must-understand: a reader that doesn't know the tag refuses the file.
pub const CRITICAL: u16 = 0x8000;

/// The records this firmware writes. Codes are frozen (ADR 0045). Low 15
/// bits 0x0C–0x0F stay free for the reserved project records (AFX map, CC
/// map, set list, tag names), in either criticality; 0x0B is the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordTag {
    Block,
    Engine,
    Registry,
    ModDests,
    Routes,
    LastProject,
    /// A project's pool slot: slot `u8` (0..32), then the Sound's name, 16 B
    /// padded; its Sound's records follow.
    Slot,
    /// A project's Part: part `u8` (0..6), then the Sound's name, 16 B; its
    /// Sound's records, `Block(Part)` (the mix) and `Origin` follow.
    Part,
    /// A project's FX: empty; the FX `Block`s follow.
    Fx,
    /// A Part's source: `[0, slot]` or `[1, engine code]`.
    Origin,
    /// ORBIT's BPM, `f32` LE: the tempo slot the projects spec reserves.
    Tempo,
    /// An ORBIT scene's block: scene `u8`, then a `Block` payload.
    OrbitBlock,
    /// A ring's hits and notes (`orbit::records`).
    OrbitNotes,
    /// A segment's degree and played chord, as exact tones.
    OrbitChord,
    /// The playing scene and the nine seeds.
    Orbit,
    /// A scene's CUSTOM set (`orbit::scene::encode_scale`).
    OrbitScale,
    /// A project's pool source, at the top level before `Fx`: its
    /// `SourceRef`, 10 B. Never audio (ADR 0087).
    PoolSource,
    /// A Sound's source: its `SourceRef`, 10 B (ADR 0087).
    SourceRef,
    /// A source file's head: `SourceHead`, 34 B, first (ADR 0093).
    SourceHead,
    /// A source file's packed 12-bit samples, up to 504 B a record, in
    /// order after the head.
    SourceData,
    /// A `ZoneSet` source file's zones, right after its head: 15 B a zone
    /// (spec § 8).
    SourceZones,
    /// A Sound's zone set tuning: 6 B a zone, its `root` and `period`
    /// (spec § 8).
    ZoneTune,
    /// ALGO's OP 5 and OP 6 routes (filter-types spec § 6): 4-byte
    /// `(Jack code, block code, param id, amount)` entries. 0x1B: 0x19 and
    /// 0x1A are TAPE's zone records.
    TapRoutes,
    /// The build a library Sound was saved on (library spec § Files):
    /// `BuildStamp`, 21 B. Written second, skipped everywhere.
    Build,
    /// A factory Sound's one-line description (showcase spec § 1, ADR
    /// 0133): 1-40 B of text. Written third, read by the listing's peek,
    /// skipped everywhere else.
    Note,
    /// A Sound's MIDI map (midi-map spec § 2): 5-byte `(cc, block code,
    /// param id, min, max)` entries, up to 16. 0x1E: 0x1D is
    /// `showcase-library`'s `Note`. Not critical. Read only since
    /// `Bindings` (MIDI phase 1 spec § 6.4): each entry loads as JUMP.
    MidiMap,
    /// A Sound's bindings (MIDI phase 1 spec § 6.4, `midi_map::codec`): a
    /// version byte, then 8 B an entry, up to 16, written last and only
    /// when the map isn't empty. Not critical; wins over `MidiMap`.
    Bindings,
    /// The WHEEL and PRESSURE rows' routes (ADR 0236): `Routes`' 4-byte
    /// entries, written after it and only when there is one. Not
    /// critical: an older build loads the Sound without them, and `Routes`
    /// keeps its 640 B bound (ADR 0102).
    PerfRoutes,
    /// A project Part's controller map (#807, ADR 0226): its name, 16 B
    /// padded, after the Part's `Origin`. Not critical.
    Controller,
    /// A controller map file's section: its code (`midi_map::ctl::Section`),
    /// then a `Bindings` payload. Not critical.
    MapSection,
    /// A project Part's Scala file (ADR 0259): its stem, 8 B padded, then
    /// the id of the name it shows (`scala_name::name_id`, u32 LE; a
    /// record of 8 B, from before long names, has none), after the Part's
    /// `Origin`. Not critical.
    Scala,
}

impl RecordTag {
    pub const fn code(self) -> u16 {
        match self {
            RecordTag::Block => 0x0001,
            RecordTag::Engine => 0x8002,
            RecordTag::Registry => 0x0003,
            RecordTag::ModDests => 0x0004,
            RecordTag::Routes => 0x0005,
            RecordTag::LastProject => 0x0006,
            RecordTag::Slot => 0x8007,
            RecordTag::Part => 0x8008,
            RecordTag::Fx => 0x8009,
            RecordTag::Origin => 0x000A,
            RecordTag::Tempo => 0x000B,
            RecordTag::OrbitBlock => 0x0010,
            RecordTag::OrbitNotes => 0x0011,
            RecordTag::OrbitChord => 0x0012,
            RecordTag::Orbit => 0x0013,
            RecordTag::OrbitScale => 0x0014,
            RecordTag::PoolSource => 0x8015,
            RecordTag::SourceRef => 0x0016,
            RecordTag::SourceHead => 0x8017,
            RecordTag::SourceData => 0x0018,
            RecordTag::SourceZones => 0x0019,
            RecordTag::ZoneTune => 0x001A,
            RecordTag::TapRoutes => 0x001B,
            RecordTag::Build => 0x001C,
            RecordTag::Note => 0x001D,
            RecordTag::MidiMap => 0x001E,
            RecordTag::Bindings => 0x001F,
            RecordTag::PerfRoutes => 0x0020,
            RecordTag::Controller => 0x0021,
            RecordTag::MapSection => 0x0022,
            RecordTag::Scala => 0x0023,
        }
    }

    pub fn from_code(c: u16) -> Option<Self> {
        Some(match c {
            0x0001 => RecordTag::Block,
            0x8002 => RecordTag::Engine,
            0x0003 => RecordTag::Registry,
            0x0004 => RecordTag::ModDests,
            0x0005 => RecordTag::Routes,
            0x0006 => RecordTag::LastProject,
            0x8007 => RecordTag::Slot,
            0x8008 => RecordTag::Part,
            0x8009 => RecordTag::Fx,
            0x000A => RecordTag::Origin,
            0x000B => RecordTag::Tempo,
            0x0010 => RecordTag::OrbitBlock,
            0x0011 => RecordTag::OrbitNotes,
            0x0012 => RecordTag::OrbitChord,
            0x0013 => RecordTag::Orbit,
            0x0014 => RecordTag::OrbitScale,
            0x8015 => RecordTag::PoolSource,
            0x0016 => RecordTag::SourceRef,
            0x8017 => RecordTag::SourceHead,
            0x0018 => RecordTag::SourceData,
            0x0019 => RecordTag::SourceZones,
            0x001A => RecordTag::ZoneTune,
            0x001B => RecordTag::TapRoutes,
            0x001C => RecordTag::Build,
            0x001D => RecordTag::Note,
            0x001E => RecordTag::MidiMap,
            0x001F => RecordTag::Bindings,
            0x0020 => RecordTag::PerfRoutes,
            0x0021 => RecordTag::Controller,
            0x0022 => RecordTag::MapSection,
            0x0023 => RecordTag::Scala,
            _ => return None,
        })
    }

    /// ORBIT's, which a project keeps after its six Parts: no Sound or
    /// SYSTEM holds one.
    pub const fn is_orbit(self) -> bool {
        matches!(
            self,
            RecordTag::Tempo
                | RecordTag::OrbitBlock
                | RecordTag::OrbitNotes
                | RecordTag::OrbitChord
                | RecordTag::Orbit
                | RecordTag::OrbitScale
        )
    }
}

/// A tag as read: only a known one can be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadTag {
    Known(RecordTag),
    Unknown(u16),
}

impl ReadTag {
    pub fn critical(self) -> bool {
        let code = match self {
            ReadTag::Known(t) => t.code(),
            ReadTag::Unknown(c) => c,
        };
        code & CRITICAL != 0
    }
}

/// One record's payload, built on the stack.
///
/// # Panics
/// Past `MAX_RECORD_LEN` bytes: an encoder bug, never card data.
#[derive(Clone, Debug)]
pub struct RecordBuf {
    buf: [u8; MAX_RECORD_LEN],
    len: usize,
}

impl RecordBuf {
    pub fn new() -> Self {
        RecordBuf {
            buf: [0; MAX_RECORD_LEN],
            len: 0,
        }
    }

    pub fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }

    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn bytes(&mut self, b: &[u8]) {
        let end = self.len + b.len();
        assert!(end <= MAX_RECORD_LEN, "record over MAX_RECORD_LEN");
        self.buf[self.len..end].copy_from_slice(b);
        self.len = end;
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Empty, for the next record.
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl Default for RecordBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// Streams records into a sink, keeping the file's running CRC.
pub struct RecordWriter<'s> {
    sink: &'s mut dyn ByteSink,
    crc: Crc32,
}

impl<'s> RecordWriter<'s> {
    /// A writer with a fresh CRC: the file's, or a body's alone.
    fn new(sink: &'s mut dyn ByteSink) -> Self {
        RecordWriter {
            sink,
            crc: Crc32::new(),
        }
    }

    /// A writer of records alone, no header: a pool entry's.
    pub(crate) fn over(sink: &'s mut dyn ByteSink) -> Self {
        Self::new(sink)
    }

    fn raw(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.crc.update(b);
        self.sink.put(b)
    }

    /// Records copied as they are, hashed into this file's CRC like any
    /// other: a pool entry's, or a whole file the console was sent. The
    /// kind's check judges them once they are on the card.
    pub fn copy(&mut self, records: &[u8]) -> Result<(), StoreError> {
        self.raw(records)
    }

    /// Only a `RecordTag` goes to disk, never a tag read back:
    ///
    /// ```compile_fail,E0308
    /// # use chimera_engine::storage::{ReadTag, RecordWriter};
    /// fn f(w: &mut RecordWriter<'_>) { let _ = w.put(ReadTag::Unknown(7), &[]); }
    /// ```
    ///
    /// # Panics
    /// When `payload` is over `MAX_RECORD_LEN`, which no reader would accept.
    pub fn put(&mut self, tag: RecordTag, payload: &[u8]) -> Result<(), StoreError> {
        assert!(
            payload.len() <= MAX_RECORD_LEN,
            "record over MAX_RECORD_LEN"
        );
        let mut head = [0; 4];
        head[..2].copy_from_slice(&tag.code().to_le_bytes());
        head[2..].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        self.raw(&head)?;
        self.raw(payload)
    }
}

/// Takes bytes and keeps none.
struct Discard;

impl ByteSink for Discard {
    fn put(&mut self, _: &[u8]) -> Result<(), StoreError> {
        Ok(())
    }
}

/// The CRC of `prefix`, then of the records `body` puts: one pass, the
/// writer's own CRC, nothing kept.
pub(crate) fn records_crc(
    prefix: &[u8],
    body: impl FnOnce(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> u32 {
    let mut sink = Discard;
    let mut w = RecordWriter::new(&mut sink);
    w.crc.update(prefix);
    // `Discard::put` is never Err, so neither is `body`.
    let _ = body(&mut w);
    w.crc.finish()
}

/// Header, then whatever `body` puts, then the CRC trailer.
pub fn write_file(
    sink: &mut dyn ByteSink,
    h: &Header,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let mut w = RecordWriter::new(sink);
    w.raw(&h.encode())?;
    body(&mut w)?;
    let trailer = w.crc.finish().to_le_bytes();
    w.sink.put(&trailer)
}
