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
        // SAFETY: `pos` only ever advances in `write_str`, which copies a
        // prefix of a `&str` (valid UTF-8) cut at a char boundary, so
        // `buf[..pos]` is always a concatenation of whole UTF-8 chars.
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
        ValFmt::Pan => {
            let v = (val * 127.0 + 0.5) as i32 - 64;
            let _ = match v {
                0 => buf.write_str("C"),
                v if v < 0 => write!(buf, "L{}", -v),
                v => write!(buf, "R{}", v),
            };
        }
        ValFmt::Int(max) => {
            let _ = write!(buf, "{}", discrete(val, max));
        }
        ValFmt::OneBased(max) => {
            let _ = write!(buf, "{}", discrete(val, max) as u16 + 1);
        }
        ValFmt::Names(names) => {
            if let Some(name) = names.get(discrete(val, fmt.max_int()) as usize) {
                let _ = buf.write_str(name);
            }
        }
    }
}

/// Normalized `val` rounded to the nearest of 0..=max.
fn discrete(val: f32, max: u8) -> u8 {
    let v = (val * max as f32 + 0.5) as u8;
    if v > max { max } else { v }
}

impl core::fmt::Write for FmtBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let mut len = bytes.len().min(self.buf.len() - self.pos);
        // Truncate whole chars only: `as_str` relies on `buf[..pos]` being
        // valid UTF-8.
        while !s.is_char_boundary(len) {
            len -= 1;
        }
        self.buf[self.pos..self.pos + len].copy_from_slice(&bytes[..len]);
        self.pos += len;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FmtBuf;
    use core::fmt::Write;

    /// A multi-byte char straddling the capacity is dropped whole, never
    /// split, so `as_str`'s unchecked conversion stays sound.
    #[test]
    fn truncation_never_splits_a_char() {
        let mut buf = FmtBuf::new();
        let _ = buf.write_str("0123456789012345678901234567890"); // 31 bytes
        let _ = buf.write_str("é"); // 2 bytes: would straddle byte 32
        assert!(core::str::from_utf8(&buf.buf[..buf.pos]).is_ok());
        assert_eq!(buf.as_str(), "0123456789012345678901234567890");
        let _ = buf.write_str("x"); // the one byte left still fits
        assert_eq!(buf.as_str().len(), 32);
    }
}
