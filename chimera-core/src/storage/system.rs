//! SYSTEM: the theme and the last project, read at boot and written on
//! leaving System when RAM differs from what the card is known to hold (ADR 0045).

use chimera_hal::store::{Dir, Store, StoreError, VolumeId};

use crate::addr::{BlockRef, Blocks};
use crate::block::Block;
use crate::ui::theme_settings::ThemeSettings;

use super::block_codec::{ByteSet, decode_block, encode_block};
use super::card::{Card, CardFault, Ready};
use super::codes::{MIGRATIONS, TRANSLATIONS};
use super::file::{AbFile, Check, Decode, LoadError, SaveError, load_ab, save_ab};
use super::frame::{Event, FileError, FileKind, ProjectFile, ProjectId};
use super::record::records_crc;
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

/// The CRC of `encode_system`'s bytes: the body alone, so a new generation
/// or header never reads as a change.
pub fn body_crc(s: &SystemSettings) -> u32 {
    records_crc(&[], |w| encode_system(s, w))
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
    /// read twice, a critical record SYSTEM doesn't read, or a project's. `Bounds`: a
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
                decode_block(
                    p,
                    MIGRATIONS,
                    TRANSLATIONS,
                    theme.as_mut().map(|t| t as &mut dyn Blocks),
                )
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
            // A project's records are no SYSTEM we wrote, critical or not.
            ReadTag::Known(
                RecordTag::Slot | RecordTag::Part | RecordTag::Fx | RecordTag::Origin,
            ) => Err(FileError::Corrupt),
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

/// What leaving System does on the card in the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitPlan {
    Nothing,
    Write,
    /// Take the card's SYSTEM; write only if it has none.
    Load,
}

/// The rule: write exactly when RAM differs from what `vol` is known to
/// hold. `known` is the volume and `body_crc` last loaded or saved;
/// `untouched`, that RAM holds defaults no card or user gave it, which
/// never go over a SYSTEM this card may hold.
pub fn exit_plan(
    known: Option<(VolumeId, u32)>,
    untouched: bool,
    vol: VolumeId,
    crc: u32,
) -> ExitPlan {
    match known {
        Some((v, c)) if v == vol && c == crc => ExitPlan::Nothing,
        Some((v, _)) if v == vol => ExitPlan::Write,
        _ if untouched => ExitPlan::Load,
        _ => ExitPlan::Write,
    }
}

/// What `on_exit` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Unchanged,
    Wrote,
    /// The settings were replaced by the card's: apply them.
    Loaded,
}

/// Why `on_exit` did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncError {
    Store(StoreError),
    /// The card's SYSTEM can't be loaded (untouched defaults don't replace
    /// it), or needs newer firmware (a save would sit behind it).
    File(FileError),
}

impl From<StoreError> for SyncError {
    fn from(e: StoreError) -> Self {
        SyncError::Store(e)
    }
}

impl From<SaveError> for SyncError {
    fn from(e: SaveError) -> Self {
        match e {
            SaveError::Store(e) => SyncError::Store(e),
            SaveError::File(e) => SyncError::File(e),
        }
    }
}

impl CardFault for SyncError {
    fn store_error(&self) -> Option<StoreError> {
        match *self {
            SyncError::Store(e) => Some(e),
            SyncError::File(_) => None,
        }
    }
}

fn load<S: Store>(s: &mut S, r: &Ready) -> Result<SystemSettings, LoadError> {
    let mut local = SystemSettings::DEFAULT;
    load_ab(s, r, AbFile::SYSTEM, &mut SystemDecoder::new(&mut local))?;
    Ok(local)
}

fn save<S: Store>(s: &mut S, r: &Ready, v: &SystemSettings) -> Result<(), SaveError> {
    s.make_dir(r.volume(), Dir::Chimera)?;
    save_ab(
        s,
        r,
        AbFile::SYSTEM,
        &mut SystemCheck::new(),
        None,
        &mut |w| encode_system(v, w),
    )?;
    Ok(())
}

/// When SYSTEM is read and written. A thin shell over `exit_plan`: it
/// mounts once per exit, so the plan always sees the card in the slot.
#[derive(Debug)]
pub struct SystemSync {
    /// The volume and `body_crc` last loaded from or saved to it.
    known: Option<(VolumeId, u32)>,
    /// RAM's theme is the default no card gave and the user hasn't changed.
    /// The last project doesn't count: writing it to a card with no SYSTEM
    /// leaves the theme untouched for the next card.
    untouched: bool,
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
            .run(store, |s, r| load(s, r).map(|v| (r.volume(), v)))
            .and_then(|o| o.result);
        let (known, settings, note) = match loaded {
            Ok((vol, v)) => (Some((vol, body_crc(&v))), v, None),
            Err(LoadError::Missing) => (None, SystemSettings::DEFAULT, Some(BootNote::NoFile)),
            Err(LoadError::Store(StoreError::NoCard)) => {
                (None, SystemSettings::DEFAULT, Some(BootNote::NoCard))
            }
            Err(e) => (None, SystemSettings::DEFAULT, Some(BootNote::Error(e))),
        };
        let sync = SystemSync {
            known,
            untouched: known.is_none(),
            was_in: false,
        };
        (sync, settings, note)
    }

    /// Once a frame: true on the frame System is left. Any theme change
    /// from the defaults, even one undone, marks them touched; the last
    /// project is no user setting.
    pub fn left_system(&mut self, in_system: bool, s: &SystemSettings) -> bool {
        self.untouched &= s.theme == SystemSettings::DEFAULT.theme;
        let left = self.was_in && !in_system;
        self.was_in = in_system;
        left
    }

    /// On leaving System: mounts, then does what `exit_plan` says for that
    /// volume. `Loaded` replaced `s` with the card's, to apply. `current`
    /// is the loaded project's file: SYSTEM names it only on its own card.
    pub fn on_exit<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        s: &mut SystemSettings,
        current: Option<ProjectFile>,
    ) -> Result<Exit, SyncError> {
        self.sync(card, store, s, current, false)
    }

    /// A project save or load: `saved` becomes the last project of its own
    /// card, by the same plan as `on_exit`, so untouched defaults still
    /// never go over that card's theme. Another card in the slot is left
    /// alone (`Unchanged`).
    pub fn write<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        s: &mut SystemSettings,
        saved: ProjectFile,
    ) -> Result<Exit, SyncError> {
        self.sync(card, store, s, Some(saved), true)
    }

    fn sync<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        s: &mut SystemSettings,
        current: Option<ProjectFile>,
        bound: bool,
    ) -> Result<Exit, SyncError> {
        let (known, untouched, ram) = (self.known, self.untouched, *s);
        let done = card
            .run(store, |st, r| {
                let vol = r.volume();
                let mine = current.filter(|f| f.vol() == vol).map(ProjectFile::id);
                if bound && mine.is_none() {
                    return Ok(None);
                }
                // The card's own SYSTEM, unless RAM already mirrors it.
                let theirs = (known.map(|(v, _)| v) != Some(vol)).then(|| load(st, r));
                let own = match theirs {
                    None => ram.last_project,
                    Some(Ok(v)) => v.last_project,
                    Some(Err(_)) => None,
                };
                let want = SystemSettings {
                    last_project: mine.or(own),
                    ..ram
                };
                let done = match (exit_plan(known, untouched, vol, body_crc(&want)), theirs) {
                    (ExitPlan::Nothing, _) => (Exit::Unchanged, want),
                    (_, Some(Err(LoadError::Store(e)))) => return Err(SyncError::Store(e)),
                    (ExitPlan::Load, Some(Ok(mut v))) => {
                        if mine.is_some_and(|m| v.last_project != Some(m)) {
                            v.last_project = mine;
                            save(st, r, &v)?;
                        }
                        (Exit::Loaded, v)
                    }
                    (ExitPlan::Load, Some(Err(LoadError::File(e)))) => {
                        return Err(SyncError::File(e));
                    }
                    // A card with no SYSTEM, or one RAM is written over.
                    _ => {
                        save(st, r, &want)?;
                        (Exit::Wrote, want)
                    }
                };
                Ok(Some((vol, done)))
            })
            .and_then(|o| o.result)?;
        let Some((vol, (exit, now))) = done else {
            return Ok(Exit::Unchanged);
        };
        *s = now;
        self.known = Some((vol, body_crc(s)));
        self.untouched &= exit != Exit::Loaded;
        Ok(exit)
    }
}
