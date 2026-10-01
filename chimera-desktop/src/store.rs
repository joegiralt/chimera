//! The desktop simulator's card: a host directory behind `Store`.

use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// `root` is the card; `root/VOLUME` holds its id as `serial label`.
pub struct DirStore {
    root: PathBuf,
}

fn io_err(e: io::Error) -> StoreError {
    match e.kind() {
        io::ErrorKind::NotFound => StoreError::NotFound,
        io::ErrorKind::StorageFull => StoreError::Full,
        _ => StoreError::Io,
    }
}

impl DirStore {
    pub fn new(root: PathBuf) -> Self {
        DirStore { root }
    }

    fn dir_path(&self, dir: Dir) -> PathBuf {
        match dir {
            Dir::Chimera => self.root.join("CHIMERA"),
            Dir::Projects => self.root.join("CHIMERA/PROJECTS"),
            Dir::Sounds => self.root.join("CHIMERA/SOUNDS"),
        }
    }

    fn file_path(&self, f: FileName) -> PathBuf {
        let mut n = String::from_utf8_lossy(f.stem()).into_owned();
        if !f.ext().is_empty() {
            n.push('.');
            n.push_str(&String::from_utf8_lossy(f.ext()));
        }
        self.dir_path(f.dir()).join(n)
    }

    fn write_volume(&self, serial: u32, label: [u8; 11]) -> Result<(), StoreError> {
        let mut v = format!("{serial:08X} ").into_bytes();
        v.extend_from_slice(&label);
        fs::write(self.root.join("VOLUME"), v).map_err(io_err)
    }

    fn read_volume(&self) -> Result<VolumeId, StoreError> {
        if !self.root.is_dir() {
            return Err(StoreError::NoCard);
        }
        let v = fs::read(self.root.join("VOLUME")).map_err(|_| StoreError::Io)?;
        let serial = v
            .get(..8)
            .and_then(|s| u32::from_str_radix(std::str::from_utf8(s).ok()?, 16).ok())
            .ok_or(StoreError::Io)?;
        let label = v
            .get(9..20)
            .and_then(|l| l.try_into().ok())
            .ok_or(StoreError::Io)?;
        Ok(VolumeId { serial, label })
    }

    /// The card is there and is the one `vol` names.
    fn open(&self, vol: VolumeId) -> Result<(), StoreError> {
        let now = self.read_volume()?;
        if now == vol {
            Ok(())
        } else {
            Err(StoreError::VolumeChanged(now))
        }
    }

    fn has_dir(&self, dir: Dir) -> Result<(), StoreError> {
        if self.dir_path(dir).is_dir() {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }
}

struct FileSink<'a>(&'a mut File);

impl ByteSink for FileSink<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.write_all(bytes).map_err(io_err)
    }
}

impl Store for DirStore {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        if !self.root.is_dir() {
            return Err(StoreError::NoCard);
        }
        if !self.root.join("VOLUME").exists() {
            let t = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            self.write_volume(t as u32, *b"CHIMERA    ")?;
        }
        self.read_volume()
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.open(vol)?;
        self.has_dir(dir)?;
        for e in fs::read_dir(self.dir_path(dir)).map_err(io_err)? {
            let e = e.map_err(io_err)?;
            let meta = e.metadata().map_err(io_err)?;
            let raw = e.file_name();
            let Some(raw) = raw.to_str() else { continue };
            let (stem, ext) = raw.split_once('.').unwrap_or((raw, ""));
            // Foreign names on the host are not ours: skip them.
            if let Some(n) = FileName::new(dir, stem.as_bytes(), ext.as_bytes())
                && meta.is_file()
            {
                f(n, meta.len() as u32);
            }
        }
        Ok(())
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.open(vol)?;
        let mut fh = File::open(self.file_path(file)).map_err(io_err)?;
        let len = fh.metadata().map_err(io_err)?.len();
        if sink.begin(len as u32).is_break() {
            return Ok(());
        }
        let mut buf = [0u8; CHUNK];
        let mut left = len as usize;
        while left > 0 {
            // Full chunks except the last, as the sink expects.
            let n = left.min(CHUNK);
            fh.read_exact(&mut buf[..n]).map_err(|_| StoreError::Io)?;
            left -= n;
            if sink.chunk(&buf[..n]).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.open(vol)?;
        self.has_dir(file.dir())?;
        let mut fh = File::create(self.file_path(file)).map_err(io_err)?;
        // What the body wrote stays on Err.
        let r = body(&mut FileSink(&mut fh));
        let synced = fh.sync_all().map_err(io_err);
        r?;
        synced?;
        let len = fh.metadata().map_err(io_err)?.len();
        Ok(len as u32)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.open(vol)?;
        fs::remove_file(self.file_path(file)).map_err(io_err)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.open(vol)?;
        if dir != Dir::Chimera {
            self.has_dir(Dir::Chimera)?;
        }
        match fs::create_dir(self.dir_path(dir)) {
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(io_err(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_hal::testkit::store_suite;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_root() -> PathBuf {
        // Tests run on their own threads: one counter for the process.
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed) + 1;
        let root = std::env::temp_dir().join(format!("chimera-card-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn dir_store_passes_suite() {
        let mut roots = Vec::new();
        store_suite(
            &mut || {
                let root = unique_root();
                roots.push(root.clone());
                DirStore::new(root)
            },
            &mut |s| {
                let old = s.mount().unwrap();
                s.write_volume(old.serial.wrapping_add(1), *b"SWAPPED    ")
                    .unwrap();
                let _ = fs::remove_dir_all(s.root.join("CHIMERA"));
            },
            &mut |s| fs::remove_dir_all(&s.root).unwrap(),
            &mut |_| {},
        );
        for r in roots {
            let _ = fs::remove_dir_all(r);
        }
    }

    /// The sim's SYSTEM wiring: a theme changed in System and left behind
    /// comes back at the next launch, from the files alone.
    #[test]
    fn theme_survives_a_relaunch() {
        use chimera_core::storage::{BootNote, Card, Exit, SystemSync};
        use chimera_core::ui::theme_settings::Bright;
        let root = unique_root();
        let mut store = DirStore::new(root.clone());
        let (mut sync, mut s, note) = SystemSync::boot(&mut Card::new(), &mut store);
        assert_eq!(note, Some(BootNote::NoFile));
        let mut card = Card::new();
        assert!(!sync.left_system(true, &s));
        s.theme.bright = Bright::new(40);
        assert!(sync.left_system(false, &s));
        assert_eq!(sync.on_exit(&mut card, &mut store, &mut s), Ok(Exit::Wrote));

        let (_, again, note) = SystemSync::boot(&mut Card::new(), &mut DirStore::new(root.clone()));
        assert_eq!((again, note), (s, None));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_store_suite_on_dir_store() {
        let mut roots = Vec::new();
        chimera_core::project::test_support::project_store_suite(&mut || {
            let root = unique_root();
            roots.push(root.clone());
            DirStore::new(root)
        });
        for r in roots {
            let _ = fs::remove_dir_all(r);
        }
    }

    /// The sim's boot: a project saved, then the next launch loads it from
    /// the files alone.
    #[test]
    fn project_survives_a_relaunch() {
        use chimera_core::block::Block;
        use chimera_core::name::ProjectName;
        use chimera_core::params::{EngineType, FilterParams};
        use chimera_core::preset::Sound;
        use chimera_core::project::test_support::same;
        use chimera_core::project::{
            PartId, ProjectStatus, SlotId, new_project_id, project_status,
        };
        use chimera_core::storage::{Card, SystemSync};
        use chimera_core::ui::UiState;
        let root = unique_root();
        let mut store = DirStore::new(root.clone());
        let mut card = Card::new();
        let (mut sync, mut s, _) = SystemSync::boot(&mut card, &mut store);
        let mut ui = Box::new(UiState::new());
        let p = ui.project_mut();
        p.edit_part(PartId::ALL[2])
            .sound
            .params
            .filter
            .set(FilterParams::RESONANCE, 0.7);
        p.pool_store(SlotId::ALL[20], Sound::init(EngineType::Modal));
        p.edit_fx().reverb.mix = 0.6;
        p.set_name(ProjectName::new("RELAUNCH").unwrap());
        let file = new_project_id(&mut card, &mut store).unwrap();
        ui.save_project(&mut card, &mut store, &mut sync, &mut s, file);

        let mut store = DirStore::new(root.clone());
        let mut card = Card::new();
        let (_, s, _) = SystemSync::boot(&mut card, &mut store);
        let mut again = Box::new(UiState::new());
        again.boot_project(&mut card, &mut store, s.last_project);
        same(ui.project(), again.project());
        assert_eq!(
            project_status(again.project(), again.template()),
            ProjectStatus::Saved
        );
        let _ = fs::remove_dir_all(root);
    }

    /// `CHIMERA_CARD` naming no directory: boot and every exit are NoCard.
    #[test]
    fn missing_card_dir_keeps_defaults() {
        use chimera_core::storage::{BootNote, Card, SyncError, SystemSettings, SystemSync};
        let mut store = DirStore::new(unique_root().join("absent"));
        let mut card = Card::new();
        let (mut sync, mut s, note) = SystemSync::boot(&mut card, &mut store);
        assert_eq!((s, note), (SystemSettings::DEFAULT, Some(BootNote::NoCard)));
        sync.left_system(true, &s);
        assert!(sync.left_system(false, &s));
        assert_eq!(
            sync.on_exit(&mut card, &mut store, &mut s),
            Err(SyncError::Store(StoreError::NoCard))
        );
    }

    #[test]
    fn missing_root_is_no_card() {
        let mut s = DirStore::new(unique_root().join("absent"));
        assert_eq!(s.mount().err(), Some(StoreError::NoCard));
    }
}
