//! Card files: framing, records and the CRC trailer (ADR 0045).

mod block_codec;
mod card;
mod codes;
mod file;
mod frame;
mod library;
mod record;
mod sound;
mod source_file;
mod source_ref;
mod sweep;
mod system;
pub mod twelve;

pub(crate) use block_codec::ENTRY_LEN;
pub use block_codec::{decode_block, encode_block};
pub use card::{Card, CardError, CardEvent, CardFault, Outcome, Ready, after_error, after_mount};
pub use chimera_image::crc::Crc32;
pub use codes::{
    DiskAddr, DiskValue, FILTER_DRIVE, MIGRATIONS, Migration, READDRESS, RETIRED, RETIRED_BLOCKS,
    RETIRED_CODES, RETIRED_SOURCES, Retired, TRANSLATIONS, Translation, ValidAddr, read_value,
    readdress,
};
pub use file::{
    AbFile, Check, Decode, DecodeInPlace, InPlaceError, LoadError, Pick, SaveError, SideState,
    check_file, check_frame, delete_ab, delete_order, load_ab, load_ab_in_place, load_file, pick,
    save_ab, save_target, write_target,
};
pub(crate) use frame::RECORD_HEAD_LEN;
pub use frame::{
    Event, FORMAT_VERSION, FileError, FileKind, Framer, Generation, HEADER_LEN, Header, MAGIC,
    ProjectFile, ProjectId, Side, TRAILER_LEN, peek_header,
};
pub use library::{
    BUILD_LEN, BuildStamp, FactoryId, ID_WINDOW, LIB_PEEK, LibId, LibKind, LibraryDecoder, NextId,
    Note, Peek, UserId, encode_library_sound, lib_id, library_file, next_user_id, peek_library,
    user_file,
};
pub(crate) use record::records_crc;
pub use record::{
    CRITICAL, MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter, write_file,
};
pub use sound::{MAX_SOUND_COST, SoundCheck, SoundDecoder, encode_sound, sound_crc};
pub use source_file::{
    MAX_TAPES, PutRefusal, SOURCE_DATA_MAX, SOURCE_HEAD_LEN, SourceCheck, SourceHead, SourceLayout,
    SourceReadError, SourceZones, TapeList, WholeShape, ZONE_LEN, Zone, data_offset,
    held_for_file_len, held_len, held_len_zoned, listed_zones, read_source_into, source_crc,
    source_file, source_file_min_len, source_name, write_descriptor, zone_set_crc,
};
#[cfg(feature = "source-writer")]
pub use source_file::{Wrote, source_target, verify_and_seal, write_source, write_zone_set};
pub use source_ref::{SourceRef, TapeSource, ZoneTune};
pub use sweep::boot_sweep;
pub use system::{
    BootNote, Exit, ExitPlan, SyncError, SystemCheck, SystemDecoder, SystemSettings, SystemSync,
    body_crc, encode_system, exit_plan,
};
