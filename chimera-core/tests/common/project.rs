//! Project files as bytes: `encode` and the two-pass `decode`, and the
//! `full`/`same` fixtures.

#[allow(unused_imports)] // each test binary uses some of these
pub use super::codec_util::{decode_project as decode, encode_project as encode};
#[allow(unused_imports)]
pub use chimera_core::project::test_support::{full, same};
