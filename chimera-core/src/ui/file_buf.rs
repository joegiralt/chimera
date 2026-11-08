//! A small file read whole into a fixed buffer (a tuning, ADR 0240). Bank
//! 1's: `Store::read` takes a `&mut dyn ReadSink`, and the vtable a bank-2
//! caller builds is anonymous rodata that lands in bank 1, so the methods
//! it names must be bank 1's too.

use core::ops::ControlFlow;

use chimera_hal::store::ReadSink;

/// The largest file read: a 128-step scale with its comments.
pub const FILE_MAX: usize = 4096;

pub struct FileBuf {
    pub b: [u8; FILE_MAX],
    pub n: usize,
    /// Longer than `FILE_MAX`: nothing kept.
    pub over: bool,
}

impl FileBuf {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            b: [0; FILE_MAX],
            n: 0,
            over: false,
        }
    }
}

impl Default for FileBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadSink for FileBuf {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.n = 0;
        self.over = len as usize > FILE_MAX;
        if self.over {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let Some(to) = self.b.get_mut(self.n..self.n + bytes.len()) else {
            self.over = true;
            return ControlFlow::Break(());
        };
        to.copy_from_slice(bytes);
        self.n += bytes.len();
        ControlFlow::Continue(())
    }
}
