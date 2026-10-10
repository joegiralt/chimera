//! The Sound library on the card (library spec § Save): a Part's Sound to
//! `/CHIMERA/SOUNDS/S#######.A`, stamped with the build.

use chimera_hal::store::{Dir, Store, StoreError};

use super::store::{CardOut, split};
use crate::name::SoundName;
use crate::preset::Sound;
use crate::storage::{
    BuildStamp, Card, CardFault, FileError, ID_WINDOW, LibId, NextId, SaveError, Side, SideState,
    SoundCheck, check_file, encode_library_sound, lib_id, library_file, next_user_id, save_ab,
};

/// What a library operation says to the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LibNote {
    Store(StoreError),
    File(FileError),
    NoIds,
    /// A listed file is not what the card has now.
    Changed,
    Gone,
}

impl LibNote {
    pub fn message(self) -> &'static str {
        match self {
            LibNote::Store(e) => e.message(),
            LibNote::File(e) => e.message(),
            LibNote::NoIds => "NO SOUND IDS LEFT",
            LibNote::Changed => "FILE CHANGED",
            LibNote::Gone => "FILE IS GONE",
        }
    }
}

impl From<StoreError> for LibNote {
    fn from(e: StoreError) -> Self {
        LibNote::Store(e)
    }
}

impl CardFault for LibNote {
    fn store_error(&self) -> Option<StoreError> {
        match self {
            LibNote::Store(e) => Some(*e),
            _ => None,
        }
    }
}

/// The user ids in `[base, base + ID_WINDOW)` there are, as a bitmap.
fn window<S: Store>(
    s: &mut S,
    r: &crate::storage::Ready,
    base: u32,
) -> Result<[u8; 512], StoreError> {
    let mut taken = [0; 512];
    s.list(r.volume(), Dir::Sounds, &mut |f, _| {
        if let Some(id) = lib_id(f)
            && let Some(i) = id.get().checked_sub(base).filter(|&i| i < ID_WINDOW)
        {
            taken[i as usize / 8] |= 1 << (i % 8);
        }
    })?;
    Ok(taken)
}

/// The next free user id: `top + 1` from one pass, else the lowest gap.
fn free_id<S: Store>(s: &mut S, r: &crate::storage::Ready) -> Result<LibId, LibNote> {
    let mut top = 0;
    s.list(r.volume(), Dir::Sounds, &mut |f, _| {
        if let Some(id) = lib_id(f).filter(|id| !id.is_factory()) {
            top = top.max(id.get());
        }
    })?;
    let mut base = 1;
    loop {
        let taken = if top < LibId::USER_MAX {
            [0; 512]
        } else {
            window(s, r, base)?
        };
        match next_user_id(top, base, &taken) {
            NextId::Id(id) => return Ok(id),
            NextId::Scan(b) => base = b,
            NextId::NoIds => return Err(LibNote::NoIds),
        }
    }
}

/// `s` as a new library file named `name`: the directories made first, the
/// id the next free one, the file read back whole. Any failure after the
/// write began deletes it.
pub fn save_library_sound<S: Store>(
    card: &mut Card,
    store: &mut S,
    s: &Sound,
    name: SoundName,
    stamp: &BuildStamp,
) -> CardOut<Result<LibId, LibNote>> {
    let run = card.run(store, |st, r| {
        st.make_dir(r.volume(), Dir::Chimera)?;
        st.make_dir(r.volume(), Dir::Sounds)?;
        let id = free_id(st, r)?;
        let file = library_file(id);
        let saved = save_ab(st, r, file, &mut SoundCheck::new(), Some(name), &mut |w| {
            encode_library_sound(s, stamp, w)
        })
        .map_err(|e| match e {
            SaveError::Store(e) => LibNote::Store(e),
            SaveError::File(e) => LibNote::File(e),
            SaveError::Damaged => LibNote::File(FileError::Corrupt),
        })
        .and_then(|_| {
            match check_file(st, r, file.side(Side::A), &mut SoundCheck::new())? {
                SideState::Present { err: None, .. } => Ok(()),
                SideState::Torn(e)
                | SideState::Headerless(e)
                | SideState::Present { err: Some(e), .. } => Err(LibNote::File(e)),
                SideState::Missing => Err(LibNote::File(FileError::Truncated)),
            }
        });
        if saved.is_err() {
            // Best effort: nothing else names this id.
            let _ = st.delete(r.volume(), file.side(Side::A));
        }
        saved.map(|()| id)
    });
    let (out, event) = split(run);
    CardOut { out, event }
}
