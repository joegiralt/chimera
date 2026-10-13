//! The public set: every Sound Source in `tools/factory-sounds/` checked
//! and built into `<out>/CHIMERA/SOUNDS/S91xxxxx.A`, with `manifest.json`.

use std::fmt::Write as _;
use std::path::Path;

use chimera_core::storage::{BuildStamp, Crc32, FactoryId};

use super::source::{self, Source};

/// The Sound Sources, in curated order (spec § 2: VA, FM, MODAL).
/// `projects.toml` holds the demo projects.
pub const SOUND_FILES: [&str; 3] = ["va.toml", "fm.toml", "modal.toml"];

/// One built file, as the release lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: FactoryId,
    pub name: String,
    /// The file's CRC.
    pub crc: u32,
    pub size: usize,
    /// The Sound's `sound_crc`, which `approved` names.
    pub sound: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub stamp: BuildStamp,
    pub sounds: Vec<Entry>,
}

impl Manifest {
    pub fn json(&self) -> String {
        let mut j = format!("{{\"stamp\": \"{}\", \"sounds\": [", self.stamp);
        for (i, e) in self.sounds.iter().enumerate() {
            let _ = write!(
                j,
                "{}\n  {{\"id\": \"S{}\", \"name\": \"{}\", \"crc\": \"{:08X}\", \"size\": {}, \"sound\": \"{:08X}\"}}",
                if i == 0 { "" } else { "," },
                e.id.get(),
                e.name,
                e.crc,
                e.size,
                e.sound
            );
        }
        j.push_str("\n]}\n");
        j
    }
}

/// `dir`'s Sound Sources, parsed; a missing file is an empty one.
pub fn sources(dir: &Path) -> Result<Vec<Source>, String> {
    let mut all = Vec::new();
    for f in SOUND_FILES {
        let p = dir.join(f);
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", p.display())),
        };
        all.extend(source::parse(&text, f)?);
    }
    Ok(all)
}

/// A card file: its name (`S9100001.A`) and bytes.
pub type File = (String, Vec<u8>);

/// Each Source but the placeholders as a card file, and their manifest.
/// Pure: `public` writes them.
pub fn files(all: &[Source], stamp: &BuildStamp) -> Result<(Vec<File>, Manifest), String> {
    let mut files = Vec::new();
    let mut sounds = Vec::new();
    for s in all.iter().filter(|s| !s.is_placeholder()) {
        let bytes = source::file_bytes(s, stamp)?;
        let mut crc = Crc32::new();
        crc.update(&bytes);
        sounds.push(Entry {
            id: s.id,
            name: s.name.as_str().into(),
            crc: crc.finish(),
            size: bytes.len(),
            sound: source::crc(s)?,
        });
        files.push((format!("S{}.A", s.id.get()), bytes));
    }
    let stamp = *stamp;
    Ok((files, Manifest { stamp, sounds }))
}

/// The public set from `dir` into `out` (emptied first): parse, check
/// (ids, names, descriptions, routes, a sweep's swell), refuse ports, build.
pub fn public(dir: &Path, out: &Path, stamp: &BuildStamp) -> Result<Manifest, String> {
    let all = sources(dir)?;
    source::check(&all)?;
    super::qa::sweeps(&all)?;
    source::public(&all)?;
    let (files, man) = files(&all, stamp)?;
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    match std::fs::remove_dir_all(out) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(out, e)),
    }
    let sounds = out.join("CHIMERA/SOUNDS");
    std::fs::create_dir_all(&sounds).map_err(|e| io(&sounds, e))?;
    for (name, bytes) in &files {
        let p = sounds.join(name);
        std::fs::write(&p, bytes).map_err(|e| io(&p, e))?;
    }
    let p = out.join("manifest.json");
    std::fs::write(&p, man.json()).map_err(|e| io(&p, e))?;
    Ok(man)
}
