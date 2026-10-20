//! Where a streamed source's heads go (slice 1 § 5.6): START's, and in
//! LOOP the loop's. Each starts a frame before its first read, made even
//! (a read takes the frame before: `stream::cursor`), and ends just past
//! the chunk boundary after its reach, so the first chunk it asks for
//! holds most of a chunk past it and a chord's second chunks aren't due
//! behind its first (#583). The loop's starts XFADE early and holds XFADE
//! and `HEAD_MS` (#584). `Heads` is bank 1's; with `bank2` the placement
//! (`place`) is bank 2's, reached through `LoadDoor`.

use crate::dsp::tape_engine::params::TapeEngineParams;
use crate::dsp::tape_engine::read::{HeadSpec, streamed_held_len};
use crate::storage::SourceRef;

mod place;
pub(crate) use place::heads_in;
pub use place::xfade_max;

/// A streamed loop's longest crossfade, in ms of source (§ 7.3 step 6):
/// what its loop head holds before the loop.
pub const XFADE_MAX_MS: u32 = 100;
// The loop head holds the fade before the loop and `HEAD_MS` after it: a
// longer fade would read past what the head holds.
const _: () = assert!(XFADE_MAX_MS <= super::admit::HEAD_MS);

/// A streamed source's heads: START's, then the loop's in LOOP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heads {
    One(HeadSpec),
    /// START's, then the loop's.
    Looped([HeadSpec; 2]),
}

impl Heads {
    /// In table order: START's first.
    pub fn as_slice(&self) -> &[HeadSpec] {
        match self {
            Heads::One(h) => core::slice::from_ref(h),
            Heads::Looped(hs) => hs,
        }
    }

    /// The row's `held`: the descriptor, the table, the heads.
    pub fn held_len(&self) -> usize {
        streamed_held_len(self.as_slice())
    }
}

/// `r`'s heads for `p` (`place`, through `LoadDoor` with `bank2`); `None`
/// where `p` needs the source in RAM, or with bank 2 shut.
pub fn heads_for(r: SourceRef, p: &TapeEngineParams) -> Option<Heads> {
    crate::project::LoadDoor::with(|d| (d.heads_for)(r, p)).flatten()
}
