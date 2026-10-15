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
        let n = Bytes::new(u32::try_from(n).unwrap_or(u32::MAX));
        self.waited = self.waited.checked_add(n).unwrap_or(Bytes::new(u32::MAX));
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

/// A read command's R1 must be all clear.
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

/// What the shell does next in a multi-block read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Clock `SCAN_BYTES` and hand them to `scanned`.
    Scan,
    /// `have` bytes of block `block` are in place; land `want` after them.
    Rest {
        block: u32,
        have: Bytes,
        want: Bytes,
    },
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scanned {
    Waiting,
    /// The token is found: `tail` of the burst is block bytes.
    Token {
        tail: Range<usize>,
    },
}

#[derive(Clone, Copy, Debug)]
enum State {
    Scan(TokenScan),
    Rest { have: usize },
    Stop,
}

/// A CMD18 read of `count` blocks, as a walk of scans and block landings.
#[derive(Clone, Copy, Debug)]
pub struct MultiRead {
    block: u32,
    count: BlockCount,
    state: State,
}

impl MultiRead {
    pub const fn new(count: BlockCount) -> Self {
        Self {
            block: 0,
            count,
            state: State::Scan(TokenScan::new()),
        }
    }

    pub fn step(&self) -> Step {
        match self.state {
            State::Scan(_) => Step::Scan,
            State::Rest { have } => Step::Rest {
                block: self.block,
                have: Bytes::new(have as u32),
                want: Bytes::new((BLOCK_WITH_CRC - have) as u32),
            },
            State::Stop => Step::Stop,
        }
    }

    /// A scan burst. Out of `Scan` it is ignored.
    pub fn scanned(&mut self, burst: &[u8; SCAN_BYTES]) -> Result<Scanned, ProtoError> {
        let State::Scan(scan) = &mut self.state else {
            return Ok(Scanned::Waiting);
        };
        match scan.feed(burst) {
            Scan::Waiting => Ok(Scanned::Waiting),
            Scan::Error(b) => Err(ProtoError::ErrorToken(b)),
            Scan::Token { at } => {
                self.state = State::Rest {
                    have: SCAN_BYTES - at - 1,
                };
                Ok(Scanned::Token {
                    tail: at + 1..SCAN_BYTES,
                })
            }
        }
    }

    /// A whole block and its CRC16 are in place. Out of `Rest` it is ignored.
    pub fn landed(&mut self, block: &[u8; BLOCK_WITH_CRC]) -> Result<(), ProtoError> {
        if !matches!(self.state, State::Rest { .. }) {
            return Ok(());
        }
        let (data, crc) = block.split_at(BLOCK_BYTES);
        if crc16(data) != u16::from_be_bytes([crc[0], crc[1]]) {
            return Err(ProtoError::Crc);
        }
        self.block += 1;
        self.state = if self.block < self.count.get() {
            State::Scan(TokenScan::new())
        } else {
            State::Stop
        };
        Ok(())
    }

    pub fn timed_out(&self, clock: BytesPerSec) -> bool {
        matches!(self.state, State::Scan(s) if s.timed_out(clock))
    }
}
