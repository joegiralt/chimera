//! Card files: framing, records and the CRC trailer (ADR 0045).

mod crc;
mod frame;
mod record;

pub use crc::Crc32;
pub use frame::{
    Event, FORMAT_VERSION, FileError, FileKind, Framer, Generation, HEADER_LEN, Header, MAGIC,
    ProjectId, Side, TRAILER_LEN,
};
pub use record::{
    CRITICAL, MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter, write_file,
};
