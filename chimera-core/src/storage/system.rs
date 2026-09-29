//! SYSTEM: the theme and the last project, read at boot and written on
//! leaving System when the bytes changed (ADR 0045).

use chimera_hal::store::{ByteSink, Dir, Store, StoreError};

use crate::addr::{BlockRef, Blocks};
use crate::block::Block;
use crate::ui::theme_settings::ThemeSettings;

use super::block_codec::{ByteSet, decode_block, encode_block};
use super::card::Card;
use super::codes::MIGRATIONS;
use super::crc::Crc32;
use super::file::{AbFile, Check, Decode, LoadError, SaveError, load_ab, save_ab};
use super::frame::{Event, FileError, FileKind, ProjectId};
use super::record::{ReadTag, RecordBuf, RecordTag, RecordWriter};

/// Everything SYSTEM holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemSettings {
    pub theme: ThemeSettings,
    pub last_project: Option<ProjectId>,
}

impl SystemSettings {
    /// No card, no file or an error: the built-in theme, no project.
    pub const DEFAULT: Self = SystemSettings {
        theme: ThemeSettings::DEFAULT,
        last_project: None,
    };
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `Block(Theme)`, then `LastProject` when there is one.
pub fn encode_system(s: &SystemSettings, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    let mut r = RecordBuf::new();
    encode_block(BlockRef::Theme, &s.theme, &mut r);
    w.put(RecordTag::Block, r.as_slice())?;
    match s.last_project {
        Some(p) => w.put(RecordTag::LastProject, &p.get().to_le_bytes()),
        None => Ok(()),
    }
}

/// Hashes what it's given.
struct CrcSink(Crc32);

impl ByteSink for CrcSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.update(bytes);
        Ok(())
    }
}

/// The CRC of `encode_system`'s bytes: the body alone, so a new generation
/// or header never reads as a change.
pub fn body_crc(s: &SystemSettings) -> u32 {
    let mut sink = CrcSink(Crc32::new());
    encode_system(s, &mut RecordWriter::new(&mut sink)).expect("a CRC sink never fails");
    sink.0.finish()
}

/// The theme as the one block a `Block` record can reach.
struct ThemeBlocks<'a>(&'a mut ThemeSettings);

impl Blocks for ThemeBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        (b == BlockRef::Theme).then_some(&*self.0 as &dyn Block)
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        (b == BlockRef::Theme).then_some(&mut *self.0 as &mut dyn Block)
    }
}

/// Checks a SYSTEM file's events: pass 1, which touches nothing.
/// `SystemDecoder` runs the same checks in pass 2, onto the settings it
/// stages.
pub struct SystemCheck {
    /// Block codes read: each may come once.
    blocks: ByteSet,
    last_project: bool,
}

impl SystemCheck {
    pub fn new() -> Self {
        SystemCheck {
            blocks: ByteSet::new(),
            last_project: false,
        }
    }

    /// One event, applied to `staged` if there is one.
    ///
    /// `WrongKind`: not a SYSTEM file. `Corrupt`: a block or last project
    /// read twice, or a critical record SYSTEM doesn't read. `Bounds`: a
    /// payload of the wrong shape. A block other than THEME, and any other
    /// record, is skipped; a last project out of range is none.
    fn step(&mut self, e: Event<'_>, staged: Option<&mut SystemSettings>) -> Result<(), FileError> {
        let (tag, p) = match e {
            Event::Header(h) => {
                if h.kind != FileKind::System {
                    return Err(FileError::WrongKind);
                }
                *self = SystemCheck::new();
                return Ok(());
            }
            Event::Record(tag, p) => (tag, p),
        };
        match tag {
            ReadTag::Known(RecordTag::Block) => {
                if p.first().is_some_and(|&b| !self.blocks.insert(b)) {
                    return Err(FileError::Corrupt);
                }
                let mut theme = staged.map(|s| ThemeBlocks(&mut s.theme));
                decode_block(p, MIGRATIONS, theme.as_mut().map(|t| t as &mut dyn Blocks))
            }
            ReadTag::Known(RecordTag::LastProject) => {
                if core::mem::replace(&mut self.last_project, true) {
                    return Err(FileError::Corrupt);
                }
                let &[a, b, c, d] = p else {
                    return Err(FileError::Bounds);
                };
                if let Some(s) = staged {
                    s.last_project = ProjectId::new(u32::from_le_bytes([a, b, c, d]));
                }
                Ok(())
            }
            // The framer refuses an unknown critical record before it gets
            // here; a known one that isn't SYSTEM's is no SYSTEM we wrote.
            t if t.critical() => Err(FileError::Corrupt),
            _ => Ok(()),
        }
    }
}

impl Default for SystemCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl Check for SystemCheck {
    const KIND: FileKind = FileKind::System;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.step(e, None)
    }

    fn end(&mut self) -> Result<(), FileError> {
        Ok(())
    }
}

/// Decodes SYSTEM onto `target`: pass 1 checks, pass 2 stages onto the
/// defaults, and `commit` copies the result into `target`.
pub struct SystemDecoder<'a> {
    check: SystemCheck,
    target: &'a mut SystemSettings,
    staged: SystemSettings,
}

impl<'a> SystemDecoder<'a> {
    pub fn new(target: &'a mut SystemSettings) -> Self {
        SystemDecoder {
            check: SystemCheck::new(),
            target,
            staged: SystemSettings::DEFAULT,
        }
    }
}

impl Check for SystemDecoder<'_> {
    const KIND: FileKind = FileKind::System;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.event(e)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.check.end()
    }
}

impl Decode for SystemDecoder<'_> {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError> {
        if let Event::Header(_) = e {
            self.staged = SystemSettings::DEFAULT;
        }
        self.check.step(e, Some(&mut self.staged))
    }

    fn commit(&mut self) -> Result<(), FileError> {
        self.check.end()?;
        *self.target = self.staged;
        Ok(())
    }
}

/// Why boot kept the defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootNote {
    NoCard,
    NoFile,
    Error(LoadError),
}

/// When SYSTEM is written: on leaving System, when the body differs from
/// the one last loaded or saved.
#[derive(Debug)]
pub struct SystemSync {
    /// `body_crc` of what the card holds, as far as we know: `None` after
    /// a boot that loaded nothing, so the first exit writes.
    saved: Option<u32>,
    was_in: bool,
}

impl SystemSync {
    /// Reads SYSTEM into a local and returns it only when the whole load
    /// is Ok; otherwise the defaults, and why.
    pub fn boot<S: Store>(
        card: &mut Card,
        store: &mut S,
    ) -> (SystemSync, SystemSettings, Option<BootNote>) {
        let loaded = card
            .run(store, |s, r| {
                let mut local = SystemSettings::DEFAULT;
                load_ab(s, r, AbFile::SYSTEM, &mut SystemDecoder::new(&mut local)).map(|_| local)
            })
            .and_then(|o| o.result);
        let (settings, note) = match loaded {
            Ok(s) => (s, None),
            Err(LoadError::Missing) => (SystemSettings::DEFAULT, Some(BootNote::NoFile)),
            Err(LoadError::Store(StoreError::NoCard)) => {
                (SystemSettings::DEFAULT, Some(BootNote::NoCard))
            }
            Err(e) => (SystemSettings::DEFAULT, Some(BootNote::Error(e))),
        };
        let sync = SystemSync {
            saved: note.is_none().then(|| body_crc(&settings)),
            was_in: false,
        };
        (sync, settings, note)
    }

    /// Once a frame: true only on the frame System is left, and only when
    /// `s` isn't what the card holds.
    pub fn wants_write(&mut self, in_system: bool, s: &SystemSettings) -> bool {
        let left = self.was_in && !in_system;
        self.was_in = in_system;
        left && self.saved != Some(body_crc(s))
    }

    /// Saves `s` to the side a load wouldn't keep. Only an Ok marks it
    /// saved, so a failed write is tried again on the next exit.
    pub fn write<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        s: &SystemSettings,
    ) -> Result<(), SaveError> {
        card.run(store, |st, r| {
            st.make_dir(r.volume(), Dir::Chimera)?;
            save_ab(
                st,
                r,
                AbFile::SYSTEM,
                &mut SystemCheck::new(),
                None,
                &mut |w| encode_system(s, w),
            )
        })
        .and_then(|o| o.result)?;
        self.saved = Some(body_crc(s));
        Ok(())
    }
}
