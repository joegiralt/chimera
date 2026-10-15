//! SPI-mode SD protocol, pure: command frames, CRC7/CRC16, the data-token
//! scan and the CMD18 multi-block walk. The shell only moves bytes: it
//! clocks what `step` asks for and hands them back.

use core::ops::Range;

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
/// A token scan burst: shorter than a block, so what follows the token is
/// always one range of one block.
pub const SCAN_BYTES: usize = 128;
const _: () = assert!(SCAN_BYTES < BLOCK_WITH_CRC);

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

/// Where a CMD18 read is: each state has the one call that moves it on,
/// and an `Err` ends the walk. Plain data, so the card IRQ can keep it.
#[derive(Clone, Copy, Debug)]
pub enum Walk {
    /// Clock `SCAN_BYTES` and hand them to `Scanning::scanned`.
    Scan(Scanning),
    /// Land `Resting::want` bytes after the `have` in place.
    Rest(Resting),
    Stop,
}

impl Walk {
    /// A read of `count` blocks, at its first scan.
    pub const fn new(count: BlockCount) -> Walk {
        Walk::Scan(Scanning {
            block: 0,
            count,
            scan: TokenScan::new(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Scanning {
    block: u32,
    count: BlockCount,
    scan: TokenScan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanOutcome<S, R> {
    Waiting(S),
    /// The token is found: `tail` of the burst is the block's first bytes.
    Token {
        tail: Range<usize>,
        rest: R,
    },
}

impl Scanning {
    pub fn scanned(
        mut self,
        burst: &[u8; SCAN_BYTES],
    ) -> Result<ScanOutcome<Scanning, Resting>, ProtoError> {
        match self.scan.feed(burst) {
            Scan::Waiting => Ok(ScanOutcome::Waiting(self)),
            Scan::Error(b) => Err(ProtoError::ErrorToken(b)),
            Scan::Token { at } => Ok(ScanOutcome::Token {
                tail: at + 1..SCAN_BYTES,
                rest: Resting {
                    block: self.block,
                    count: self.count,
                    have: SCAN_BYTES - at - 1,
                },
            }),
        }
    }

    pub fn timed_out(&self, clock: BytesPerSec) -> bool {
        self.scan.timed_out(clock)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resting {
    block: u32,
    count: BlockCount,
    have: usize,
}

impl Resting {
    pub fn block(&self) -> u32 {
        self.block
    }

    /// Block bytes already in place.
    pub fn have(&self) -> Bytes {
        bytes(self.have)
    }

    /// What the shell's DMA lands straight after them.
    pub fn want(&self) -> Bytes {
        bytes(BLOCK_WITH_CRC - self.have)
    }

    /// The whole block and its CRC16 are in place.
    pub fn landed(self, block: &[u8; BLOCK_WITH_CRC]) -> Result<Walk, ProtoError> {
        let (data, crc) = block.split_at(BLOCK_BYTES);
        if crc16(data) != u16::from_be_bytes([crc[0], crc[1]]) {
            return Err(ProtoError::Crc);
        }
        let block = self.block + 1;
        Ok(if block < self.count.get() {
            Walk::Scan(Scanning {
                block,
                count: self.count,
                scan: TokenScan::new(),
            })
        } else {
            Walk::Stop
        })
    }
}
