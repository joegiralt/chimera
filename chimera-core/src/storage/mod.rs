//! Card files: framing, records and the CRC trailer (ADR 0045).

mod block_codec;
mod card;
mod codes;
mod crc;
mod frame;
mod record;
mod sound;

pub use block_codec::{decode_block, encode_block};
pub use card::{Card, CardError, CardEvent, CardFault, Outcome, Ready, after_error, after_mount};
pub use codes::{
    DiskValue, MIGRATIONS, Migration, RETIRED, RETIRED_BLOCKS, RETIRED_CODES, RETIRED_SOURCES,
    ValidAddr, read_value,
};
pub use crc::Crc32;
pub use frame::{
    Event, FORMAT_VERSION, FileError, FileKind, Framer, Generation, HEADER_LEN, Header, MAGIC,
    ProjectId, Side, TRAILER_LEN,
};
pub use record::{
    CRITICAL, MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter, write_file,
};
pub use sound::{SoundDecoder, encode_sound};
