//! Judging bank 2 and the one door into it.

use crate::crc::Crc32;
use crate::header::{BANK2_BASE, HEADER_LEN, Header, Seal, Version};

/// A read the memory could not serve (a bus or ECC fault).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadFail;

/// Bytes of the image, `off` from the base of bank 2.
pub trait ImageRead {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Empty,
    Torn,
    Foreign,
    Format(u16),
    Corrupt,
    Unreadable,
    /// Sound, but sealed for another build.
    Stale {
        have: Version,
    },
    Good(Header),
}

/// The spec's order: commit word, magic, format, bounds, seal, body CRC.
pub fn judge(img: &mut impl ImageRead, want: &Seal) -> Verdict {
    let mut hb = [0u8; 256];
    if img.read(0, &mut hb).is_err() {
        return Verdict::Unreadable;
    }
    let h = match Header::parse(&hb) {
        Ok(h) => h,
        Err(v) => return v,
    };
    if h.seal != want.bytes() {
        return Verdict::Stale { have: h.version };
    }
    let mut crc = Crc32::new();
    let mut buf = [0u8; 512];
    let mut done = 0u32;
    while done < h.body_len {
        let n = (h.body_len - done).min(512) as usize;
        let Some(off) = HEADER_LEN.checked_add(done) else {
            return Verdict::Corrupt;
        };
        if img.read(off, &mut buf[..n]).is_err() {
            return Verdict::Unreadable;
        }
        crc.update(&buf[..n]);
        done += n as u32;
    }
    if crc.finish() == h.body_crc {
        Verdict::Good(h)
    } else {
        Verdict::Corrupt
    }
}

/// Memory that is this chip's bank 2.
///
/// # Safety
/// `read(off, ..)` must return the bytes at `BANK2_BASE + off` of the real
/// bank 2: `Door::open` turns a `Good` verdict on them into an address bank 1
/// will call.
pub unsafe trait Bank2Memory: ImageRead {}

/// Proof that bank 2 judged `Good` for this build's seal. The only way in.
#[derive(Debug)]
pub struct Door {
    door_addr: u32,
    version: Version,
    seal: Seal,
}

impl Door {
    pub fn open(m: &mut impl Bank2Memory, want: &Seal) -> Result<Door, Verdict> {
        match judge(m, want) {
            Verdict::Good(h) => Ok(Door {
                door_addr: BANK2_BASE + h.door_off,
                version: h.version,
                seal: *want,
            }),
            v => Err(v),
        }
    }

    pub fn door_addr(&self) -> u32 {
        self.door_addr
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn seal(&self) -> &Seal {
        &self.seal
    }
}
