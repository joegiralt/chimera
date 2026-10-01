//! CRC-32/ISO-HDLC: reflected poly 0xEDB88320, init and xorout 0xFFFF_FFFF.

use chimera_hal::store::{ByteSink, StoreError};

const TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// A running CRC; `finish` doesn't consume, so it can be read mid-stream.
#[derive(Clone, Copy, Debug)]
pub struct Crc32(u32);

impl Crc32 {
    pub const fn new() -> Self {
        Crc32(0xFFFF_FFFF)
    }

    pub fn update(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = TABLE[((self.0 ^ x as u32) & 0xFF) as usize] ^ (self.0 >> 8);
        }
    }

    pub fn finish(&self) -> u32 {
        !self.0
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashes what it's given.
pub(crate) struct CrcSink(pub(crate) Crc32);

impl ByteSink for CrcSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.update(bytes);
        Ok(())
    }
}
