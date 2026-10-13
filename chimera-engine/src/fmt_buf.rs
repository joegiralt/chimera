//! A short text buffer, for labels and the console's numbers.

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

    /// `args` written out, cut at 32 bytes.
    pub fn of(args: core::fmt::Arguments<'_>) -> Self {
        let mut b = Self::new();
        let _ = b.write_fmt(args);
        b
    }

    /// `write!` finds this before `fmt::Write`'s provided method, so every
    /// crate calls this one copy instead of building its own.
    #[inline(never)]
    pub fn write_fmt(&mut self, args: core::fmt::Arguments<'_>) -> core::fmt::Result {
        core::fmt::write(self, args)
    }
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
