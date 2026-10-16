//! SPI-mode SD protocol, pure: command frames, CRC7/CRC16, the data-token
//! scan and the CMD18 multi-block walk. The shell only moves bytes: it
//! clocks what the walk asks for and hands them back.

use chimera_hal::units::{BlockCount, Bytes, BytesPerSec, Millis};

use crate::sd_addr::CmdArg;

/// A command index (0..=63).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdIndex(u8);

pub const CMD0: CmdIndex = CmdIndex(0);
pub const CMD8: CmdIndex = CmdIndex(8);
pub const CMD12: CmdIndex = CmdIndex(12);
pub const CMD17: CmdIndex = CmdIndex(17);
pub const CMD18: CmdIndex = CmdIndex(18);

/// Start of a single or multi-block read's data.
pub const DATA_TOKEN: u8 = 0xFE;

pub const TOKEN_TIMEOUT_MS: Millis = Millis::new(100);

/// A block and its CRC16.
pub const BLOCK_BYTES: usize = 512;
pub const CRC_BYTES: usize = 2;
pub const BLOCK_WITH_CRC: usize = BLOCK_BYTES + CRC_BYTES;
/// A busy drain's burst, after CMD12.
pub const SCAN_BYTES: usize = 128;

/// Command, argument (big-endian), CRC7 with the end bit.
pub const fn command(idx: CmdIndex, arg: CmdArg) -> [u8; 6] {
    let a = arg.get().to_be_bytes();
    let head = [0x40 | idx.0, a[0], a[1], a[2], a[3]];
    [head[0], a[0], a[1], a[2], a[3], crc7(&head) << 1 | 1]
}

const fn crc7_table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        // left-aligned: the 7-bit register sits in bits 7..1, poly 0x09 << 1
        let mut c = i as u8;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x80 != 0 { c << 1 ^ 0x12 } else { c << 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
const CRC7_TABLE: [u8; 256] = crc7_table();

/// CRC7 (poly 0x09, init 0), the 7-bit value.
pub const fn crc7(data: &[u8]) -> u8 {
    let mut c = 0u8;
    let mut i = 0;
    while i < data.len() {
        c = CRC7_TABLE[(c ^ data[i]) as usize];
        i += 1;
    }
    c >> 1
}

const fn crc16_table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u16) << 8;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x8000 != 0 {
                c << 1 ^ 0x1021
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC16_TABLE: [u16; 256] = crc16_table();

/// CRC16-CCITT (poly 0x1021, init 0).
pub fn crc16(data: &[u8]) -> u16 {
    crc16_update(0, data)
}

pub fn crc16_update(crc: u16, data: &[u8]) -> u16 {
    data.iter().fold(crc, |c, &b| {
        c << 8 ^ CRC16_TABLE[usize::from((c >> 8) as u8 ^ b)]
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scan {
    Waiting,
    /// The data token, at this offset in the slice given to `feed`.
    Token {
        at: usize,
    },
    /// Any other non-idle byte: a data error token (0b000xxxxx).
    Error(u8),
}

/// Waits for a block's data token over idle 0xFF bytes.
fn bytes(n: usize) -> Bytes {
    Bytes::new(u32::try_from(n).unwrap_or(u32::MAX))
}

#[derive(Clone, Copy, Debug)]
pub struct TokenScan {
    waited: Bytes,
}

impl TokenScan {
    pub const fn new() -> Self {
        Self {
            waited: Bytes::new(0),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Scan {
        match bytes.iter().position(|&b| b != 0xFF) {
            None => {
                self.wait(bytes.len());
                Scan::Waiting
            }
            Some(at) if bytes[at] == DATA_TOKEN => Scan::Token { at },
            Some(at) => Scan::Error(bytes[at]),
        }
    }

    fn wait(&mut self, n: usize) {
        self.waited = self
            .waited
            .checked_add(bytes(n))
            .unwrap_or(Bytes::new(u32::MAX));
    }

    /// True once the scan has clocked `TOKEN_TIMEOUT_MS` of bytes.
    pub fn timed_out(&self, clock: BytesPerSec) -> bool {
        self.waited >= clock.bytes_in(TOKEN_TIMEOUT_MS)
    }
}

impl Default for TokenScan {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtoError {
    Crc,
    ErrorToken(u8),
    Timeout,
    R1(u8),
}

/// After init, a read command's R1 must be all clear: the idle bit too.
pub const fn check_r1(r1: u8) -> Result<(), ProtoError> {
    if r1 == 0 {
        Ok(())
    } else {
        Err(ProtoError::R1(r1))
    }
}

/// CMD12's stuff byte, then NCR bytes to find R1 in.
pub const STOP_BYTES: usize = 9;

/// The answer to CMD12. Byte 0 is the stuff byte (it may be a data byte
/// still clocking out); R1 is the first byte after it with bit 7 clear.
pub fn stop_response(bytes: &[u8; STOP_BYTES]) -> Result<(), ProtoError> {
    match bytes[1..].iter().find(|&&b| b & 0x80 == 0) {
        Some(&r1) => check_r1(r1),
        None => Err(ProtoError::Timeout),
    }
}

/// A block as the card clocks it in a read: its token, data and CRC16.
pub const STREAM_BLOCK: usize = 1 + BLOCK_WITH_CRC;

/// Where a CMD18 read is, or its stop. Plain data, so the card IRQ can
/// keep it.
#[derive(Clone, Copy, Debug)]
pub enum Walk {
    /// Clock `Reading::need` bytes (or fewer), then `Reading::landed`.
    Read(Reading),
    Stop,
}

impl Walk {
    /// A read of `count` blocks, at its first token.
    pub const fn new(count: BlockCount) -> Walk {
        Walk::Read(Reading {
            block: 0,
            count,
            at: At::Gap(TokenScan::new()),
        })
    }
}

/// A read of `count` blocks, at `block`. The shell lands whatever the card
/// clocks, idle bytes, tokens and CRCs too, right after the data so far;
/// `landed` packs each block's data down onto it, so a chunk's blocks lie
/// whole however many bursts and gaps they came in.
#[derive(Clone, Copy, Debug)]
pub struct Reading {
    block: u32,
    count: BlockCount,
    at: At,
}

#[derive(Clone, Copy, Debug)]
enum At {
    /// Idle bytes before the block's token.
    Gap(TokenScan),
    /// `have` bytes of data in, their CRC16 so far.
    Data { have: u16, crc: u16 },
    /// The data in, the CRC16's first byte if clocked.
    Crc { crc: u16, hi: Option<u8> },
}

const BLOCK: u16 = BLOCK_BYTES as u16;
const _: () = assert!(BLOCK_BYTES <= u16::MAX as usize);

impl Reading {
    /// The fewest bytes the read still clocks: as if no idle byte came
    /// before any token, so a burst of it never runs past the last CRC.
    /// Landed at the data's end, it ends within `count × STREAM_BLOCK` of
    /// the read's start (the staging's bound, I4).
    pub fn need(&self) -> Bytes {
        const STREAM: u32 = STREAM_BLOCK as u32;
        const CRC: u32 = CRC_BYTES as u32;
        let after = self.count.get() - self.block - 1;
        let here = match self.at {
            At::Gap(_) => STREAM,
            At::Data { have, .. } => u32::from(BLOCK - have) + CRC,
            At::Crc { hi: None, .. } => CRC,
            At::Crc { hi: Some(_), .. } => 1,
        };
        Bytes::new(after.saturating_mul(STREAM).saturating_add(here))
    }

    /// The burst at `buf[from..]` just landed after the data at
    /// `buf[..from]`: its blocks' data packed onto it, each block's CRC16
    /// checked. The walk on, and where the data now ends.
    pub fn landed(mut self, buf: &mut [u8], from: usize) -> Result<(Walk, usize), ProtoError> {
        let (mut r, mut w) = (from, from);
        while r < buf.len() {
            self.at = match self.at {
                At::Gap(mut scan) => match scan.feed(&buf[r..]) {
                    Scan::Waiting => {
                        r = buf.len();
                        At::Gap(scan)
                    }
                    Scan::Token { at } => {
                        r += at + 1;
                        At::Data { have: 0, crc: 0 }
                    }
                    Scan::Error(b) => return Err(ProtoError::ErrorToken(b)),
                },
                At::Data { have, crc } => {
                    let n = usize::from(BLOCK - have).min(buf.len() - r);
                    let crc = crc16_update(crc, &buf[r..r + n]);
                    buf.copy_within(r..r + n, w);
                    (r, w) = (r + n, w + n);
                    // `n` is at most `BLOCK - have`.
                    let have = have + n as u16;
                    if have == BLOCK {
                        At::Crc { crc, hi: None }
                    } else {
                        At::Data { have, crc }
                    }
                }
                At::Crc { crc, hi: None } => {
                    r += 1;
                    At::Crc {
                        crc,
                        hi: Some(buf[r - 1]),
                    }
                }
                At::Crc { crc, hi: Some(hi) } => {
                    if u16::from_be_bytes([hi, buf[r]]) != crc {
                        return Err(ProtoError::Crc);
                    }
                    r += 1;
                    self.block += 1;
                    if self.block == self.count.get() {
                        return Ok((Walk::Stop, w));
                    }
                    At::Gap(TokenScan::new())
                }
            };
        }
        Ok((Walk::Read(self), w))
    }

    /// Waiting on a token for `TOKEN_TIMEOUT_MS` of bytes.
    pub fn timed_out(&self, clock: BytesPerSec) -> bool {
        matches!(self.at, At::Gap(s) if s.timed_out(clock))
    }
}
