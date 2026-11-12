//! A Scala file a Part names (ADR 0259): its short stem, and the identity
//! of the name it shows, the file's long name if it was copied with one.
//! Bank 1's, out of `tuning` (bank 2's link section): the project codec,
//! the TUNING cell and the walk call these and its derives.

use chimera_hal::store::{Dir, FileName};

/// The most of a long name kept and shown: as `chimera_fat::dir::LONG_MAX`.
pub const LONG_MAX: usize = 32;

/// A Scala file on the card, `/CHIMERA/TUNINGS/<stem>.SCL`: what a Part's
/// TUNING plays past the built-in ones. The stem is 1–8 of A–Z, 0–9 and
/// `~` (a computer's short name for a long one), zero padded. `id` is
/// `name_id` of the name the file shows (its long name less `.SCL`, else
/// its stem); 0 is unknown (a project from before long names).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScalaName {
    stem: [u8; 8],
    id: u32,
}

/// The stem's length: no iterator closure.
#[inline(always)]
fn len(b: &[u8; 8]) -> usize {
    let mut n = 0;
    while n < 8 && b[n] != 0 {
        n += 1;
    }
    n
}

/// A shown name's identity: FNV-1a, never 0. What a project keeps, so a
/// file copied again under a new short name (`~2` for `~1`) is still
/// found by its long one.
pub fn name_id(name: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
    for &b in name {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h.max(1)
}

/// What a file shows: its long name less `.<ext>`, else its stem.
pub fn shown<'a>(long: &'a [u8], f: &'a FileName) -> &'a [u8] {
    if long.is_empty() {
        return f.stem();
    }
    let ext = f.ext();
    match long.len().checked_sub(ext.len() + 1) {
        Some(n) if long[n] == b'.' && &long[n + 1..] == ext => &long[..n],
        _ => long,
    }
}

impl ScalaName {
    /// No file: a Part's field when it names none.
    pub const NONE: ScalaName = ScalaName {
        stem: [0; 8],
        id: 0,
    };

    /// `stem` as a name, if it is one; its id unknown.
    #[inline(always)]
    pub fn new(stem: &[u8]) -> Option<ScalaName> {
        FileName::new(Dir::Tunings, stem, b"SCL")?;
        let mut b = [0; 8];
        b[..stem.len()].copy_from_slice(stem);
        Some(ScalaName { stem: b, id: 0 })
    }

    /// File `f` showing `long` (`list_long`'s): its stem and `name_id`.
    pub fn of(f: &FileName, long: &[u8]) -> Option<ScalaName> {
        let n = ScalaName::new(f.stem())?;
        Some(n.with_id(name_id(shown(long, f))))
    }

    #[inline(always)]
    pub fn with_id(self, id: u32) -> ScalaName {
        ScalaName { id, ..self }
    }

    #[inline(always)]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The padded stem, as a project saves it; `None` if not a name.
    #[inline(always)]
    pub fn from_padded(b: &[u8; 8]) -> Option<ScalaName> {
        let n = len(b);
        let s = ScalaName::new(&b[..n])?;
        (s.stem == *b).then_some(s)
    }

    #[inline(always)]
    pub fn padded(&self) -> [u8; 8] {
        self.stem
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        let n = len(&self.stem);
        &self.stem[..n]
    }

    #[inline(always)]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.as_bytes()).unwrap_or_default()
    }

    /// Its stem's `.SCL` or `.KBM`.
    #[inline(always)]
    pub fn file(&self, ext: &[u8]) -> Option<FileName> {
        FileName::new(Dir::Tunings, self.as_bytes(), ext)
    }
}
