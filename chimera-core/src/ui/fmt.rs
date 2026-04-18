/// Fixed-size buffer for no_std text formatting via core::fmt::Write.
pub struct FmtBuf {
    buf: [u8; 32],
    pos: usize,
}

impl Default for FmtBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl FmtBuf {
    pub fn new() -> Self {
        Self {
            buf: [0; 32],
            pos: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // Safe: we only write valid UTF-8 via core::fmt::Write
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.pos]) }
    }

    pub fn clear(&mut self) {
        self.pos = 0;
    }
}

use crate::ui::page::ValFmt;

/// Format a normalized 0..1 value for display.
pub fn fmt_val(buf: &mut FmtBuf, val: f32, fmt: ValFmt) {
    use core::fmt::Write;
    match fmt {
        ValFmt::Uni => {
            let midi = (val * 127.0 + 0.5) as i32;
            let _ = write!(buf, "{}", midi);
        }
        ValFmt::Bi => {
            let midi = (val * 127.0 + 0.5) as i32;
            let v = midi - 64;
            if v > 0 {
                let _ = write!(buf, "+{}", v);
            } else {
                let _ = write!(buf, "{}", v);
            }
        }
        ValFmt::Int(max) => {
            let v = (val * max as f32 + 0.5) as u8;
            let v = if v > max { max } else { v };
            let _ = write!(buf, "{}", v);
        }
    }
}

impl core::fmt::Write for FmtBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - self.pos;
        let len = if bytes.len() < remaining {
            bytes.len()
        } else {
            remaining
        };
        self.buf[self.pos..self.pos + len].copy_from_slice(&bytes[..len]);
        self.pos += len;
        Ok(())
    }
}
