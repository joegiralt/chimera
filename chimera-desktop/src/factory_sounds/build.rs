//! The public set: every Sound Source in `tools/factory-sounds/` checked
//! and built into `<out>/CHIMERA/SOUNDS/S91xxxxx.A`, the demo projects in
//! `projects.toml` into `<out>/CHIMERA/PROJECTS/P000000n.A`, with
//! `manifest.json`.

use std::fmt::Write as _;
use std::path::Path;

use chimera_core::storage::{BuildStamp, FactoryId};

use super::project::{self, Demo, DemoId};
use super::source::{self, Source};

/// The Sound Sources, in curated order (spec § 2: VA, FM, MODAL).
/// `projects.toml` holds the demo projects.
pub const SOUND_FILES: [&str; 3] = ["va.toml", "fm.toml", "modal.toml"];

/// One built file, as the release lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: FactoryId,
    pub name: String,
    /// The file's CRC trailer.
    pub crc: u32,
    pub size: usize,
    /// The Sound's `sound_crc`, which `approved` names.
    pub sound: u32,
}

/// A built demo project, as the release lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct DemoEntry {
    pub id: DemoId,
    pub name: String,
    /// The file's CRC trailer.
    pub crc: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub stamp: BuildStamp,
    pub sounds: Vec<Entry>,
    pub projects: Vec<DemoEntry>,
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
        j.push_str("\n], \"projects\": [");
        for (i, e) in self.projects.iter().enumerate() {
            let _ = write!(
                j,
                "{}\n  {{\"id\": \"{}\", \"name\": \"{}\", \"crc\": \"{:08X}\"}}",
                if i == 0 { "" } else { "," },
                e.id.stem(),
                e.name,
                e.crc
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
        let crc = trailer(&bytes).ok_or_else(|| format!("{}: no CRC trailer", s.at(s.id_line)))?;
        sounds.push(Entry {
            id: s.id,
            name: s.name.as_str().into(),
            crc,
            size: bytes.len(),
            sound: source::crc(s)?,
        });
        files.push((format!("S{}.A", s.id.get()), bytes));
    }
    let stamp = *stamp;
    Ok((
        files,
        Manifest {
            stamp,
            sounds,
            projects: Vec::new(),
        },
    ))
}

/// `projects.toml`'s demos, parsed; a missing file holds none.
pub fn demos(dir: &Path) -> Result<Vec<Demo>, String> {
    let p = dir.join(project::FILE);
    match std::fs::read_to_string(&p) {
        Ok(t) => project::parse(&t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// Each demo but a held one as its project file (`P0000001.A`) and its
/// manifest entry, its Sounds from `all`; all checked. Pure: `public`
/// writes them.
pub fn demo_files(demos: &[Demo], all: &[Source]) -> Result<(Vec<File>, Vec<DemoEntry>), String> {
    project::check(demos, all)?;
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for d in demos.iter().filter(|d| d.held.is_none()) {
        let bytes = project::file_bytes(&*project::build(d, all)?)?;
        entries.push(DemoEntry {
            id: d.id,
            name: d.name.as_str().into(),
            crc: trailer(&bytes).ok_or_else(|| format!("{}: no CRC trailer", d.id.stem()))?,
        });
        files.push((format!("{}.A", d.id.stem()), bytes));
    }
    Ok((files, entries))
}

/// A card file's own trailer: the CRC of everything before it, as the
/// card checks it (hashing the trailer too gives the CRC residue).
fn trailer(bytes: &[u8]) -> Option<u32> {
    let i = bytes.len().checked_sub(4)?;
    Some(u32::from_le_bytes(bytes[i..].try_into().ok()?))
}

/// The public set from `dir` into `out` (emptied first): parse, check
/// (ids, names, descriptions, routes, a sweep's swell), refuse ports, build
/// the Sounds into `CHIMERA/SOUNDS/` and the demo projects (each Part a
/// public Sound) into `CHIMERA/PROJECTS/`.
pub fn public(dir: &Path, out: &Path, stamp: &BuildStamp) -> Result<Manifest, String> {
    let all = sources(dir)?;
    source::check(&all)?;
    super::qa::sweeps(&all)?;
    source::public(&all)?;
    let (files, mut man) = files(&all, stamp)?;
    let (projects, entries) = demo_files(&demos(dir)?, &all)?;
    man.projects = entries;
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
    let dir = out.join("CHIMERA/PROJECTS");
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    for (name, bytes) in &projects {
        let p = dir.join(name);
        std::fs::write(&p, bytes).map_err(|e| io(&p, e))?;
    }
    let p = out.join("manifest.json");
    std::fs::write(&p, man.json()).map_err(|e| io(&p, e))?;
    Ok(man)
}
