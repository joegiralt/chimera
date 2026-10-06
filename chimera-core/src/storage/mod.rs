//! Card files: framing, records and the CRC trailer (ADR 0045).

mod block_codec;
mod card;
mod codes;
mod crc;
mod file;
mod frame;
mod record;
mod sound;
mod source_file;
mod source_ref;
mod system;
pub mod twelve;

pub use block_codec::{decode_block, encode_block};
pub use card::{Card, CardError, CardEvent, CardFault, Outcome, Ready, after_error, after_mount};
pub use codes::{
    DiskValue, MIGRATIONS, Migration, RETIRED, RETIRED_BLOCKS, RETIRED_CODES, RETIRED_SOURCES,
    Retired, TRANSLATIONS, Translation, ValidAddr, read_value,
};
pub use crc::Crc32;
pub use file::{
    AbFile, Check, Decode, DecodeInPlace, InPlaceError, LoadError, Pick, SaveError, SideState,
    check_file, check_frame, delete_ab, delete_order, load_ab, load_ab_in_place, load_file, pick,
    save_ab, save_target, write_target,
};
pub use frame::{
    Event, FORMAT_VERSION, FileError, FileKind, Framer, Generation, HEADER_LEN, Header, MAGIC,
    ProjectFile, ProjectId, Side, TRAILER_LEN, peek_header,
};
pub(crate) use record::records_crc;
pub use record::{
    CRITICAL, MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter, write_file,
};
pub use sound::{MAX_SOUND_COST, SoundCheck, SoundDecoder, encode_sound, sound_crc};
pub use source_file::{
    PutRefusal, SOURCE_DATA_MAX, SOURCE_HEAD_LEN, SourceCheck, SourceHead, SourceLayout,
    SourceZones, ZONE_LEN, Zone, held_for_file_len, held_len, read_source_into, source_crc,
    source_file, source_file_min_len, write_descriptor, zone_set_crc,
};
#[cfg(feature = "source-writer")]
pub use source_file::{Wrote, write_source, write_zone_set};
pub use source_ref::{SourceRef, ZoneTune};
pub use system::{
    BootNote, Exit, ExitPlan, SyncError, SystemCheck, SystemDecoder, SystemSettings, SystemSync,
    body_crc, encode_system, exit_plan,
};
