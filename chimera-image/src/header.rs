//! The 256-byte header at the base of bank 2, little-endian.

use crate::judge::Verdict;

pub const BANK2_BASE: u32 = 0x0810_0000;
/// Sectors S0..S3; S4..S7 are the bootloader's.
pub const BANK2_LEN: u32 = 512 * 1024;
pub const HEADER_LEN: u32 = 256;
/// One flash word: the programming and body-length granule.
pub const FLASH_WORD: u32 = 32;
pub const MAGIC: [u8; 8] = *b"CHIMERA2";
pub const FORMAT: u16 = 1;
/// The commit word, eight of these, is programmed last.
pub const COMMIT: u32 = 0xB2C0_FFEE;
pub const COMMIT_OFF: u32 = 0xE0;

/// The header the link puts at 0x08100000: magic, format and the commit
/// word, the rest zero. The seal tool fills the length, CRC, door, seal and
/// version; until then it judges `Corrupt`.
pub const UNSEALED_HEADER: [u8; 256] = {
    let mut b = [0u8; 256];
    let mut i = 0;
    while i < 8 {
        b[i] = MAGIC[i];
        i += 1;
    }
    let f = FORMAT.to_le_bytes();
    b[FORMAT_OFF] = f[0];
    b[FORMAT_OFF + 1] = f[1];
    let c = COMMIT.to_le_bytes();
    let mut i = 0;
    while i < 32 {
        b[COMMIT_OFF as usize + i] = c[i % 4];
        i += 1;
    }
    b
};

const FORMAT_OFF: usize = 0x08;
const FLAGS_OFF: usize = 0x0A;
const LEN_OFF: usize = 0x0C;
const CRC_OFF: usize = 0x10;
const DOOR_OFF: usize = 0x14;
const SEAL_OFF: usize = 0x18;
const VERSION_OFF: usize = 0x20;
const SPARE_OFF: usize = 0x40;

/// The first 8 bytes of the body's SHA-256. Never the build's placeholder
/// nor an erased or zeroed word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seal([u8; 8]);

impl Seal {
    /// What a build with no seal step carries.
    pub const UNSET: [u8; 8] = *b"UNSEALED";

    pub const fn new(b: [u8; 8]) -> Option<Seal> {
        let u = u64::from_le_bytes(b);
        if u == u64::from_le_bytes(Self::UNSET) || u == 0 || u == u64::MAX {
            None
        } else {
            Some(Seal(b))
        }
    }

    pub fn bytes(&self) -> [u8; 8] {
        self.0
    }
}

/// The build's version text: ASCII up to the first NUL, anything else `?`.
/// Sanitised on the way in, so `as_str` is always valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version([u8; 32]);

impl Version {
    pub fn from_bytes(b: [u8; 32]) -> Version {
        let mut v = [0u8; 32];
        for (d, &s) in v.iter_mut().zip(b.iter()) {
            if s == 0 {
                break;
            }
            *d = if (0x20..0x7F).contains(&s) { s } else { b'?' };
        }
        Version(v)
    }

    /// Truncated to 32 bytes.
    pub fn new(s: &str) -> Version {
        let mut b = [0u8; 32];
        for (d, &c) in b.iter_mut().zip(s.as_bytes()) {
            *d = c;
        }
        Version::from_bytes(b)
    }

    pub fn as_str(&self) -> &str {
        let n = self.0.iter().position(|&c| c == 0).unwrap_or(32);
        // Printable ASCII only (from_bytes), so this is always Some.
        core::str::from_utf8(&self.0[..n]).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub body_len: u32,
    pub body_crc: u32,
    pub door_off: u32,
    pub seal: [u8; 8],
    pub version: Version,
}

fn u32_at(b: &[u8; 256], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Header {
    /// The verdicts that need only the header, in the spec's order: commit,
    /// magic, format, bounds. The seal and the CRC are `judge`'s. The flags and
    /// the spare bytes (0x40..0xE0) must be zero, so no byte of the header is
    /// free to change unseen.
    pub fn parse(b: &[u8; 256]) -> Result<Header, Verdict> {
        let c = COMMIT_OFF as usize;
        let word = |i: usize| u32_at(b, c + 4 * i);
        if (0..8).all(|i| word(i) == u32::MAX) {
            return Err(Verdict::Empty);
        }
        if (0..8).any(|i| word(i) != COMMIT) {
            return Err(Verdict::Torn);
        }
        if b[..8] != MAGIC {
            return Err(Verdict::Foreign);
        }
        let format = u16::from_le_bytes([b[FORMAT_OFF], b[FORMAT_OFF + 1]]);
        if format != FORMAT {
            return Err(Verdict::Format(format));
        }
        let body_len = u32_at(b, LEN_OFF);
        let door_off = u32_at(b, DOOR_OFF);
        let end = HEADER_LEN.checked_add(body_len);
        let ok = b[FLAGS_OFF..FLAGS_OFF + 2] == [0, 0]
            && b[SPARE_OFF..c].iter().all(|&x| x == 0)
            && body_len != 0
            && body_len <= BANK2_LEN - HEADER_LEN
            && body_len.is_multiple_of(FLASH_WORD)
            && door_off >= HEADER_LEN
            && end.is_some_and(|e| door_off < e)
            && door_off.is_multiple_of(FLASH_WORD);
        if !ok {
            return Err(Verdict::Corrupt);
        }
        let mut seal = [0u8; 8];
        seal.copy_from_slice(&b[SEAL_OFF..SEAL_OFF + 8]);
        let mut v = [0u8; 32];
        v.copy_from_slice(&b[VERSION_OFF..VERSION_OFF + 32]);
        Ok(Header {
            body_len,
            body_crc: u32_at(b, CRC_OFF),
            door_off,
            seal,
            version: Version::from_bytes(v),
        })
    }

    /// The header as flash holds it: the commit word is `COMMIT`x8 when
    /// `committed`, else erased (0xFF). Flags and spare bytes are zero.
    pub fn to_bytes(&self, committed: bool) -> [u8; 256] {
        let mut b = [0u8; 256];
        b[..8].copy_from_slice(&MAGIC);
        b[FORMAT_OFF..FORMAT_OFF + 2].copy_from_slice(&FORMAT.to_le_bytes());
        b[LEN_OFF..LEN_OFF + 4].copy_from_slice(&self.body_len.to_le_bytes());
        b[CRC_OFF..CRC_OFF + 4].copy_from_slice(&self.body_crc.to_le_bytes());
        b[DOOR_OFF..DOOR_OFF + 4].copy_from_slice(&self.door_off.to_le_bytes());
        b[SEAL_OFF..SEAL_OFF + 8].copy_from_slice(&self.seal);
        b[VERSION_OFF..VERSION_OFF + 32].copy_from_slice(&self.version.0);
        let c = COMMIT_OFF as usize;
        if committed {
            for i in 0..8 {
                b[c + 4 * i..c + 4 * i + 4].copy_from_slice(&COMMIT.to_le_bytes());
            }
        } else {
            b[c..c + 32].fill(0xFF);
        }
        b
    }
}
