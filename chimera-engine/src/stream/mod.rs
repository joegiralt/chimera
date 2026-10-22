//! Streaming a sample from the card.

/// TAPE is in the build: a voice may stream. Without it the shells run
/// their blocks with no ports.
pub const STREAMS: bool = cfg!(feature = "tape");

pub mod admit;
pub mod bench;
pub mod cursor;
pub mod edf;
pub mod layout;
pub mod port;
pub mod registry;
pub mod ring;
pub mod serve;
pub mod slot;
pub mod stage;
pub mod stats;
#[cfg(feature = "test-support")]
pub mod test_card;
pub mod units;
