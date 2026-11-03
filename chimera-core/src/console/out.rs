//! The host as the console sees it: where answers go (`Out`), and `put`'s
//! data read a packet at a time (`Rx`).

/// The host stopped taking bytes for STALL_MS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stalled;

/// The host: where answers go, and where `put`'s data comes from.
pub trait Out {
    fn put(&mut self, bytes: &[u8]) -> Result<(), Stalled>;
    /// The answer's clock restarts: after card work, before more text.
    fn fresh(&mut self) {}
    /// The next byte past the request line, or `None` once `wait_ms` pass
    /// without one. An `Out` that never hears from the host: always `None`.
    fn recv(&mut self, _wait_ms: u32) -> Option<u8> {
        None
    }
    /// The shell's clock. Required: a `put` times its waits by it.
    fn now(&self) -> chimera_hal::Ms;
}

/// `put`'s data as an `Out` reads it: a packet a `fill` (one USB hold),
/// handed out a byte a `pop`. A finished `put` leaves none (the host
/// waits on each answer); an aborted one drops up to 63 B, and the host
/// resyncs on its next request.
pub struct Rx {
    buf: [u8; Rx::PACKET],
    at: u8,
    len: u8,
}

impl Rx {
    /// The console's OUT packet.
    pub const PACKET: usize = 64;

    pub const fn new() -> Rx {
        Rx {
            buf: [0; Rx::PACKET],
            at: 0,
            len: 0,
        }
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.at >= self.len {
            return None;
        }
        let b = self.buf.get(usize::from(self.at)).copied();
        self.at += 1;
        b
    }

    /// Empty, a packet from `read` (its byte count, or `None`): true if
    /// it brought any.
    pub fn fill(&mut self, read: impl FnOnce(&mut [u8]) -> Option<usize>) -> bool {
        let n = read(&mut self.buf).unwrap_or(0);
        (self.at, self.len) = (0, n as u8);
        n > 0
    }
}

impl Default for Rx {
    fn default() -> Rx {
        Rx::new()
    }
}
