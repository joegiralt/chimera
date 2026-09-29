//! Display names stored in file headers.

/// A display name: 1..=N bytes of A-Z a-z 0-9, space and '-', with no
/// leading or trailing space. Only `new` and `from_padded` make one.
///
/// ```compile_fail,E0451
/// use chimera_core::name::Name;
/// let _ = Name::<16> { bytes: [b'A'; 16], len: 16 };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name<const N: usize> {
    /// NUL past `len`, so the derived equality is the text's.
    bytes: [u8; N],
    len: u8,
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

    pub fn new(s: &str) -> Result<Self, NameError> {
        let () = Self::FITS_U8;
        let b = s.as_bytes();
        check(b, N)?;
        let mut bytes = [0; N];
        bytes[..b.len()].copy_from_slice(b);
        Ok(Name {
            bytes,
            len: b.len() as u8,
        })
    }

    /// The disk form: the name, then NULs to N bytes.
    pub fn from_padded(b: &[u8; N]) -> Result<Self, NameError> {
        let () = Self::FITS_U8;
        let len = b.iter().position(|&c| c == 0).unwrap_or(N);
        if b[len..].iter().any(|&c| c != 0) {
            return Err(NameError::BadChar(0));
        }
        check(&b[..len], N)?;
        Ok(Name {
            bytes: *b,
            len: len as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // ASCII by construction, so this never falls back.
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }

    pub fn padded(&self) -> [u8; N] {
        self.bytes
    }
}

fn check(b: &[u8], max: usize) -> Result<(), NameError> {
    if b.is_empty() {
        return Err(NameError::Empty);
    }
    if b.len() > max {
        return Err(NameError::TooLong);
    }
    if let Some(&c) = b
        .iter()
        .find(|&&c| !(c.is_ascii_alphanumeric() || c == b' ' || c == b'-'))
    {
        return Err(NameError::BadChar(c));
    }
    if b[0] == b' ' || b[b.len() - 1] == b' ' {
        return Err(NameError::EdgeSpace);
    }
    Ok(())
}
