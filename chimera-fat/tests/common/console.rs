//! The console's `put` as the shells drive it: a fake unit and host,
//! base64 lines, and a plain TAPE source's file.
// Each test binary uses its own part of this module.
#![allow(dead_code)]

use chimera_core::console::{
    CardAnswer, Console, Frame, Out, PUT_CHUNK_BYTES, Stalled, Stats, Unit, answer,
};
use chimera_core::dsp::tape_engine::read::Shape;
use chimera_core::name::SoundName;
use chimera_core::storage::{
    Crc32, FileKind, Generation, Header, RecordTag, SOURCE_DATA_MAX, SourceHead, SourceRef,
    WholeShape, source_crc, write_file,
};
use chimera_core::ui::UiState;
use chimera_core::ui::theme_settings::{ThemeSettings, palette};
use chimera_hal::store::{ByteSink, Files, StoreError};
use chimera_hal::{FB_SIZE, Ms};
use std::collections::VecDeque;

pub struct Fake<'a, S> {
    pub ui: Box<UiState>,
    pub fb: Box<[u16; FB_SIZE]>,
    pub files: &'a mut S,
    /// `forget_library` calls: a Sound `put` or `rm` drops the page.
    pub forgotten: usize,
}

impl<'a, S> Fake<'a, S> {
    pub fn new(files: &'a mut S) -> Self {
        Fake {
            ui: Box::new(UiState::new()),
            fb: Box::new([0; FB_SIZE]),
            files,
            forgotten: 0,
        }
    }
}

impl<S: Files> Unit for Fake<'_, S> {
    fn ui(&self) -> &UiState {
        &self.ui
    }
    fn stats(&mut self) -> Option<Stats> {
        None
    }
    fn bench(&self) -> Option<&str> {
        None
    }
    fn card(&mut self) -> Option<CardAnswer> {
        None
    }
    fn frame(&self) -> Frame<'_> {
        Frame {
            fb: &self.fb,
            palette: palette(&ThemeSettings::DEFAULT),
        }
    }
    fn dfu(&mut self) -> Option<()> {
        None
    }
    fn boot(&self) -> Option<chimera_core::boot::BootSeen> {
        None
    }
    fn usb_regs(&self) -> Option<chimera_core::boot::UsbRegs> {
        None
    }
    type Files = S;
    fn files(&mut self) -> Option<&mut S> {
        Some(&mut *self.files)
    }
    fn forget_library(&mut self) {
        self.forgotten += 1;
    }
}

/// The host: what it sends after the request line, and a clock in µs that
/// moves `per_byte_us` a byte and the whole wait when nothing comes.
pub struct Host {
    pub input: VecDeque<u8>,
    pub out: Vec<u8>,
    us: u64,
    per_byte_us: u64,
}

impl Host {
    pub fn new(data: &[u8], per_byte_us: u64) -> Host {
        Host {
            input: data.iter().copied().collect(),
            out: Vec::new(),
            us: 0,
            per_byte_us,
        }
    }
}

impl Out for Host {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let b = self.input.pop_front();
        self.us += match b {
            Some(_) => self.per_byte_us,
            None => u64::from(wait_ms) * 1_000,
        };
        b
    }
    fn now(&self) -> Ms {
        Ms((self.us / 1_000) as u32)
    }
}

/// `line`, then `data`, through `answer`: what the unit said, and what of
/// the host's data it left unread.
pub fn ask_on<S: Files>(s: &mut S, line: &str, data: &[u8], per_byte_us: u64) -> (String, usize) {
    ask_unit(&mut Fake::new(s), line, data, per_byte_us)
}

/// `ask_on` through a unit the test keeps.
pub fn ask_unit<S: Files>(
    unit: &mut Fake<'_, S>,
    line: &str,
    data: &[u8],
    per_byte_us: u64,
) -> (String, usize) {
    let mut host = Host::new(data, per_byte_us);
    let mut c = Console::new();
    for &b in line.as_bytes() {
        if let Some(r) = c.push(b) {
            answer(r, unit, &mut host).unwrap();
        }
    }
    (String::from_utf8(host.out).unwrap(), host.input.len())
}

pub fn ask<S: Files>(s: &mut S, line: &str, data: &[u8]) -> String {
    ask_on(s, line, data, 0).0
}

pub fn base64(b: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for c in b.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &x)| n | u32::from(x) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() {
                A[(n >> (18 - 6 * i) & 63) as usize]
            } else {
                b'='
            });
        }
    }
    out
}

/// The data as the host tool sends it: 48 bytes a line.
pub fn lines(b: &[u8]) -> Vec<u8> {
    b.chunks(48)
        .flat_map(|c| [base64(c), b"\n".to_vec()].concat())
        .collect()
}

pub fn crc(b: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(b);
    c.finish()
}

pub struct Bytes(pub Vec<u8>);

impl ByteSink for Bytes {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

/// A plain source of `frames`: its head and whole file.
pub fn source_file_bytes(seed: u8, frames: u32) -> (SourceHead, Vec<u8>) {
    let data: Vec<u8> = (0..3 * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(31).wrapping_add(u32::from(seed)) as u8)
        .collect();
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("FAT").unwrap(),
    };
    let h = Header {
        kind: FileKind::Source,
        generation: Generation::FIRST,
        name: None,
    };
    let mut out = Bytes(Vec::new());
    write_file(&mut out, &h, &mut |w| {
        w.put(RecordTag::SourceHead, &head.to_bytes())?;
        data.chunks(SOURCE_DATA_MAX)
            .try_for_each(|c| w.put(RecordTag::SourceData, c))
    })
    .unwrap();
    (head, out.0)
}

pub fn src_line(h: &SourceHead, file: &[u8]) -> String {
    format!(
        "put /CHIMERA/TAPES/{:08X}.SRC {} {:08x}\n",
        h.sref.crc,
        file.len(),
        crc(file)
    )
}

/// The file as the host tool sends it: each chunk's lines, then its end.
pub fn chunked(file: &[u8]) -> Vec<u8> {
    file.chunks(PUT_CHUNK_BYTES as usize)
        .flat_map(|c| [lines(c), format!("end {:08x}\n", crc(c)).into_bytes()].concat())
        .collect()
}
