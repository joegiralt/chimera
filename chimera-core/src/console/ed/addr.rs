//! A parameter's editor address: `<disk_ident>.<ident>`, e.g. `OP_C.CRSE`.

use core::fmt;

use chimera_engine::addr::{BlockRef, ParamAddr};

/// The block's token. One call site for `disk_ident` in this module, so the
/// compiler inlines it here, into bank 2, and not a shared copy in bank 1.
#[inline(never)]
fn block_token(b: BlockRef) -> Option<&'static str> {
    b.disk_ident()
}

/// Both halves are frozen literals (`BlockRef::disk_ident`, `ParamSpec::ident`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdAddr(pub ParamAddr);

impl EdAddr {
    pub fn parse(word: &[u8]) -> Option<EdAddr> {
        let dot = word.iter().position(|&b| b == b'.')?;
        let (block, ident) = (&word[..dot], &word[dot + 1..]);
        if ident.is_empty() {
            return None;
        }
        let block = BlockRef::ALL
            .into_iter()
            .find(|&b| block_token(b).is_some_and(|i| i.as_bytes() == block))?;
        let spec = block.specs().iter().find(|s| s.ident.as_bytes() == ident)?;
        Some(EdAddr(ParamAddr::new(block, spec.id)))
    }
}

impl fmt::Display for EdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Some(block), Some(spec)) = (block_token(self.0.block), self.0.spec()) else {
            return Err(fmt::Error);
        };
        write!(f, "{block}.{}", spec.ident)
    }
}
