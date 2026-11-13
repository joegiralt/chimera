//! A small file whole, for bank 2's readers (ADR 0226): a trait object
//! bank 2 builds of its own type gets an anonymous vtable no glob places,
//! so the sink, the body and the listing's callback are these, bank 1's.

use core::ops::ControlFlow;

use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};

/// Fills `buf` from the start; a file past it is `Corrupt`.
struct Fill<'a> {
    buf: &'a mut [u8],
    len: usize,
    over: bool,
}

impl ReadSink for Fill<'_> {
    #[optimize(size)]
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.over = len as usize > self.buf.len();
        if self.over {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    #[optimize(size)]
    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let Some(to) = self.buf.get_mut(self.len..self.len + bytes.len()) else {
            self.over = true;
            return ControlFlow::Break(());
        };
        to.copy_from_slice(bytes);
        self.len += bytes.len();
        ControlFlow::Continue(())
    }
}

/// `f` whole into `buf`: its length.
#[inline(never)]
#[optimize(size)]
pub fn read_small(
    s: &mut dyn Store,
    vol: VolumeId,
    f: FileName,
    buf: &mut [u8],
) -> Result<usize, StoreError> {
    let mut sink = Fill {
        buf,
        len: 0,
        over: false,
    };
    s.read(vol, f, &mut sink)?;
    if sink.over {
        return Err(StoreError::Corrupt);
    }
    Ok(sink.len)
}

/// `bytes` as the whole of `f`.
#[inline(never)]
#[optimize(size)]
pub fn write_small(
    s: &mut dyn Store,
    vol: VolumeId,
    f: FileName,
    bytes: &[u8],
) -> Result<u32, StoreError> {
    s.write(vol, f, &mut |sink: &mut dyn ByteSink| sink.put(bytes))
}

/// The stems in `dir` with one of `exts`, each once, into `out`: how many
/// (at most `out.len()`). No folder is none. A computer's hidden files and
/// any other extension are never listed (ADR 0259).
#[inline(never)]
#[optimize(size)]
pub fn list_stems(
    s: &mut dyn Store,
    vol: VolumeId,
    dir: Dir,
    exts: &[&[u8]],
    out: &mut [([u8; 8], u8)],
) -> Result<usize, StoreError> {
    let mut n = 0;
    let r = s.list(vol, dir, &mut |f, _| {
        let st = f.stem();
        if n == out.len()
            || !exts.contains(&f.ext())
            || out[..n].iter().any(|(b, l)| &b[..usize::from(*l)] == st)
        {
            return;
        }
        out[n].0[..st.len()].copy_from_slice(st);
        out[n].1 = st.len() as u8;
        n += 1;
    });
    match r {
        Ok(()) => Ok(n),
        Err(StoreError::NotFound) => Ok(0),
        Err(e) => Err(e),
    }
}
