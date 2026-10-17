//! Streaming a sample from the card.

pub mod admit;
pub mod bench;
pub mod cursor;
pub mod edf;
pub mod layout;
pub mod port;
pub mod registry;
pub mod ring;
pub mod slot;
pub mod stats;
#[cfg(feature = "test-support")]
pub mod test_card;
pub mod units;
