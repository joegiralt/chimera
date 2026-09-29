//! TLV records: `u16` tag, `u16` length, the payload, all little-endian.

use chimera_hal::store::{ByteSink, StoreError};

use super::crc::Crc32;
use super::frame::Header;

/// The largest payload a known record may have; the framer buffers one.
pub const MAX_RECORD_LEN: usize = 512;

/// Tag bit 15, must-understand: a reader that doesn't know the tag refuses the file.
pub const CRITICAL: u16 = 0x8000;

/// The records this firmware writes. Codes are frozen (ADR 0045).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordTag {
    Block,
    Engine,
    Registry,
    ModDests,
    Routes,
    LastProject,
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
            _ => return None,
        })
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
    pub(super) fn new(sink: &'s mut dyn ByteSink) -> Self {
        RecordWriter {
            sink,
            crc: Crc32::new(),
        }
    }

    fn raw(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.crc.update(b);
        self.sink.put(b)
    }

    /// Only a `RecordTag` goes to disk, never a tag read back:
    ///
    /// ```compile_fail,E0308
    /// # use chimera_core::storage::{ReadTag, RecordWriter};
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
