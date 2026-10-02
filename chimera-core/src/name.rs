//! Display names stored in file headers.

use core::num::NonZeroU8;

/// A display name: 1..=N bytes of A-Z a-z 0-9, space and '-', with no
/// leading or trailing space. Only `new` and `from_padded` make one.
///
/// ```compile_fail,E0451
/// use chimera_core::name::Name;
/// let _ = Name::<16> { bytes: [b'A'; 16], len: core::num::NonZeroU8::MIN };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name<const N: usize> {
    /// NUL past `len`, so the derived equality is the text's.
    bytes: [u8; N],
    /// Never 0, so `Option<Name>` costs no tag.
    len: NonZeroU8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    BadChar(u8),
    EdgeSpace,
}

pub type SoundName = Name<16>;
pub type ProjectName = Name<16>;

impl<const N: usize> Name<N> {
    const FITS_U8: () = assert!(N >= 1 && N <= u8::MAX as usize);

    /// `const`, so a literal name is checked at compile time.
    pub const fn new(s: &str) -> Result<Self, NameError> {
        let () = Self::FITS_U8;
        let b = s.as_bytes();
        let len = match check(b, N) {
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        let mut bytes = [0; N];
        let mut i = 0;
        while i < b.len() {
            bytes[i] = b[i];
            i += 1;
        }
        Ok(Name { bytes, len })
    }

    /// The disk form: the name, then NULs to N bytes.
    pub fn from_padded(b: &[u8; N]) -> Result<Self, NameError> {
        let () = Self::FITS_U8;
        let len = b.iter().position(|&c| c == 0).unwrap_or(N);
        if b[len..].iter().any(|&c| c != 0) {
            return Err(NameError::BadChar(0));
        }
        let len = check(&b[..len], N)?;
        Ok(Name { bytes: *b, len })
    }

    pub fn as_str(&self) -> &str {
        // ASCII by construction, so this never falls back.
        core::str::from_utf8(&self.bytes[..self.len.get() as usize]).unwrap_or("")
    }

    pub fn padded(&self) -> [u8; N] {
        self.bytes
    }
}

/// A-Z a-z 0-9, space or '-'.
pub const fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b' ' || c == b'-'
}

/// The length of a valid name: 1..=`max`, `max` within a byte.
const fn check(b: &[u8], max: usize) -> Result<NonZeroU8, NameError> {
    if b.len() > max {
        return Err(NameError::TooLong);
    }
    let Some(len) = NonZeroU8::new(b.len() as u8) else {
        return Err(NameError::Empty);
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !is_name_byte(c) {
            return Err(NameError::BadChar(c));
        }
        i += 1;
    }
    if b[0] == b' ' || b[b.len() - 1] == b' ' {
        return Err(NameError::EdgeSpace);
    }
    Ok(len)
}
