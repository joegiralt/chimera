//! The build a library Sound was saved on (library spec § Files).

use core::fmt;

use chimera_hal::store::StoreError;

use super::record::{RecordTag, RecordWriter};
use super::sound::encode_sound_body;
use crate::block::DiskCode;
use crate::preset::Sound;

const VERSION_LEN: usize = 12;
const HASH_LEN: usize = 8;
/// `Build` payload: version, hash, a flags byte (bit 0 dirty).
pub const BUILD_LEN: usize = VERSION_LEN + HASH_LEN + 1;

/// A build's identity, NUL-padded as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildStamp {
    version: [u8; VERSION_LEN],
    hash: [u8; HASH_LEN],
    dirty: bool,
}

const fn pad<const N: usize>(s: &str) -> [u8; N] {
    let b = s.as_bytes();
    assert!(b.len() <= N, "build stamp field too long");
    let mut out = [0; N];
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    out
}

/// Stored bytes up to the first NUL, a non-printable one as `?`.
fn clean<const N: usize>(p: &[u8]) -> [u8; N] {
    let mut out = [0; N];
    for (o, &c) in out.iter_mut().zip(p).take_while(|(_, c)| **c != 0) {
        *o = if (0x20..0x7F).contains(&c) { c } else { b'?' };
    }
    out
}

fn text(b: &[u8]) -> &str {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    // Printable ASCII by construction (`new` takes a `&str`, `parse` cleans).
    core::str::from_utf8(&b[..n]).unwrap_or("?")
}

impl BuildStamp {
    pub const fn new(version: &str, hash: &str, dirty: bool) -> BuildStamp {
        BuildStamp {
            version: pad(version),
            hash: pad(hash),
            dirty,
        }
    }

    pub fn to_bytes(&self) -> [u8; BUILD_LEN] {
        let mut b = [0; BUILD_LEN];
        b[..VERSION_LEN].copy_from_slice(&self.version);
        b[VERSION_LEN..VERSION_LEN + HASH_LEN].copy_from_slice(&self.hash);
        b[BUILD_LEN - 1] = self.dirty as u8;
        b
    }

    /// `None` under `BUILD_LEN` bytes; the rest is read leniently.
    pub fn parse(p: &[u8]) -> Option<BuildStamp> {
        (p.len() >= BUILD_LEN).then(|| BuildStamp {
            version: clean(&p[..VERSION_LEN]),
            hash: clean(&p[VERSION_LEN..VERSION_LEN + HASH_LEN]),
            dirty: p[BUILD_LEN - 1] & 1 != 0,
        })
    }

    pub fn version(&self) -> &str {
        text(&self.version)
    }

    pub fn hash(&self) -> &str {
        text(&self.hash)
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Both clean, one real hash: a `dev` or `unknown` build names no commit.
    pub fn is_this_build(&self, this: &BuildStamp) -> bool {
        self.hash == this.hash
            && !self.dirty
            && !this.dirty
            && !matches!(self.hash(), "dev" | "unknown")
    }
}

/// `0.1.0 02e440e2`, with `+` after the hash when dirty.
impl fmt::Display for BuildStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.version(), self.hash())?;
        if self.dirty {
            f.write_str("+")?;
        }
        Ok(())
    }
}

/// `Engine`, `Build`, then the rest of the Sound.
pub fn encode_library_sound(
    s: &Sound,
    stamp: &BuildStamp,
    w: &mut RecordWriter<'_>,
) -> Result<(), StoreError> {
    w.put(RecordTag::Engine, &[s.engine().disk_code()])?;
    w.put(RecordTag::Build, &stamp.to_bytes())?;
    encode_sound_body(s, w)
}
