//! Card files: framing, records and the CRC trailer (ADR 0045).

mod codes;
mod crc;
mod frame;
mod record;

pub use codes::{
    DiskValue, MIGRATIONS, Migration, RETIRED, RETIRED_CODES, RETIRED_SOURCES, ValidAddr,
    read_value,
};
pub use crc::Crc32;
pub use frame::{
    Event, FORMAT_VERSION, FileError, FileKind, Framer, Generation, HEADER_LEN, Header, MAGIC,
    ProjectId, Side, TRAILER_LEN,
};
pub use record::{
    CRITICAL, MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter, write_file,
};
