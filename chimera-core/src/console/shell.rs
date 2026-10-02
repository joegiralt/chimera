//! The shells' pure helpers: loop timing, the bench report, the serial number.

/// Whether a loop iteration answered a console request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Served {
    Idle,
    Answered,
}

/// The UI loop's time between loop tops, since the last `take`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoopTimer {
    sum_us: u64,
    laps: u32,
    peak_us: u32,
}

impl LoopTimer {
    pub const fn new() -> Self {
        LoopTimer {
            sum_us: 0,
            laps: 0,
            peak_us: 0,
        }
    }

    /// One loop top `us` after the last. An iteration that answered a request is not timed.
    pub fn lap(&mut self, us: u32, served: Served) {
        if served == Served::Answered {
            return;
        }
        self.sum_us = self.sum_us.saturating_add(u64::from(us));
        self.laps = self.laps.saturating_add(1);
        self.peak_us = self.peak_us.max(us);
    }

    /// (avg, peak) in µs, then reset. (0, 0) with no laps.
    pub fn take(&mut self) -> (u32, u32) {
        let out = match self.sum_us.checked_div(u64::from(self.laps)) {
            Some(avg) => (u32::try_from(avg).unwrap_or(u32::MAX), self.peak_us),
            None => (0, 0),
        };
        *self = Self::new();
        out
    }
}

/// Report text in a fixed buffer. The last `TRUNCATED.len()` bytes are kept
/// back, so a report that runs out of room always ends in `# TRUNCATED`.
pub struct Report<const N: usize> {
    buf: [u8; N],
    len: usize,
    cut: bool,
}

impl<const N: usize> Default for Report<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Report<N> {
    pub const TRUNCATED: &'static str = "# TRUNCATED\n";

    pub const fn new() -> Self {
        const { assert!(N > Self::TRUNCATED.len()) };
        Report {
            buf: [0; N],
            len: 0,
            cut: false,
        }
    }

    pub fn heading(&mut self, title: &str) {
        self.put("# ", title);
    }

    /// One screen line; a line that won't fit cuts the report.
    pub fn line(&mut self, text: &str) {
        self.put("", text);
    }

    /// Records `text` and draws it with `draw`: the two can't drift apart.
    pub fn drawn<R>(&mut self, text: &str, draw: impl FnOnce(&str) -> R) -> R {
        self.line(text);
        draw(text)
    }

    fn put(&mut self, prefix: &str, text: &str) {
        if self.cut {
            return;
        }
        let end = self.len + prefix.len() + text.len() + 1;
        if end + Self::TRUNCATED.len() > N {
            let t = Self::TRUNCATED.as_bytes();
            if let Some(room) = self.buf.get_mut(self.len..self.len + t.len()) {
                room.copy_from_slice(t);
            }
            self.cut = true;
            return;
        }
        let mut at = self.len;
        for part in [prefix.as_bytes(), text.as_bytes(), b"\n"] {
            if let Some(room) = self.buf.get_mut(at..at + part.len()) {
                room.copy_from_slice(part);
            }
            at += part.len();
        }
        self.len = end;
    }

    pub fn as_str(&self) -> &str {
        let n = if self.cut {
            self.len + Self::TRUNCATED.len()
        } else {
            self.len
        };
        self.buf
            .get(..n)
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

/// The chip's 96-bit UID as 24 uppercase hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialNumber([u8; 24]);

impl SerialNumber {
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or("")
    }
}

/// Hex of the UID bytes in order (the HAL's `Uid::read()` gives them so).
pub fn serial_hex(uid: &[u8; 12]) -> SerialNumber {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [b'0'; 24];
    let nibbles = uid.iter().flat_map(|&b| [b >> 4, b & 15]);
    for (slot, nibble) in out.iter_mut().zip(nibbles) {
        *slot = DIGITS.get(usize::from(nibble)).copied().unwrap_or(b'0');
    }
    SerialNumber(out)
}
