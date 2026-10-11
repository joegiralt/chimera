//! CRC-32/ISO-HDLC: reflected poly 0xEDB88320, init and xorout 0xFFFF_FFFF.

/// Sixteen entries, a nibble at a time: 64 bytes of flash for the 1 KB a
/// byte table takes, at about twice the cycles per byte (storage only, off
/// the audio path). A `static`, so it is in flash once.
static TABLE: [u32; 16] = {
    let mut t = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 4 {
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
            let c = self.0 ^ x as u32;
            let c = TABLE[(c & 0xF) as usize] ^ (c >> 4);
            self.0 = TABLE[(c & 0xF) as usize] ^ (c >> 4);
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
