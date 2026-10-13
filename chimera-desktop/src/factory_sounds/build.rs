//! The public set: every Sound Source in `tools/factory-sounds/` checked
//! and built into `<out>/CHIMERA/SOUNDS/S91xxxxx.A`, the demo projects in
//! `projects.toml` into `<out>/CHIMERA/PROJECTS/P000000n.A`, with
//! `manifest.json`. The private set: the TX81Z port Sources in
//! `$CHIMERA_TX_PORTS`, never in the repo, into `private/` beside it.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
    write_set(out, &files, Some(&projects), &man)?;
    Ok(man)
}

/// `out` emptied, then its Sounds into `CHIMERA/SOUNDS/`, its projects
/// (the public set's; the private set has none) into `CHIMERA/PROJECTS/`,
/// and `manifest.json`.
fn write_set(
    out: &Path,
    sounds: &[File],
    projects: Option<&[File]>,
    man: &Manifest,
) -> Result<(), String> {
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    match std::fs::remove_dir_all(out) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(out, e)),
    }
    let sets = [
        ("CHIMERA/SOUNDS", Some(sounds)),
        ("CHIMERA/PROJECTS", projects),
    ];
    for (sub, files) in sets.into_iter().filter_map(|(d, f)| Some((d, f?))) {
        let dir = out.join(sub);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        for (name, bytes) in files {
            let p = dir.join(name);
            std::fs::write(&p, bytes).map_err(|e| io(&p, e))?;
        }
    }
    let p = out.join("manifest.json");
    std::fs::write(&p, man.json()).map_err(|e| io(&p, e))?;
    Ok(())
}

/// Where the port Sources live (spec § 3): never in the repo.
pub const PORTS_VAR: &str = "CHIMERA_TX_PORTS";
/// Where the TX81Z banks live, for the edits check (spec § 6).
pub const BANKS_VAR: &str = "CHIMERA_TX_BANKS";
/// Everything private by default: `ports/`, `banks/` and `qa/`, a sibling
/// of the showcase ledger, which is published (never a port in it).
pub const PRIVATE_LEDGER: &str = "~/dev/chimera/.superpowers/ledgers/showcase-private";

/// `$CHIMERA_TX_PORTS` (`var`), else `PRIVATE_LEDGER/ports`.
pub fn ports_dir(var: Option<OsString>) -> PathBuf {
    var.map_or_else(|| Path::new(PRIVATE_LEDGER).join("ports"), PathBuf::from)
}

/// `$CHIMERA_TX_BANKS` (`var`), else `PRIVATE_LEDGER/banks`.
pub fn banks_dir(var: Option<OsString>) -> PathBuf {
    var.map_or_else(|| Path::new(PRIVATE_LEDGER).join("banks"), PathBuf::from)
}

/// `qa --private`'s ledger: `PRIVATE_LEDGER/qa`.
pub fn private_qa_ledger() -> PathBuf {
    Path::new(PRIVATE_LEDGER).join("qa")
}

/// What a private run says until the importer verifies `edits`.
pub fn unverified(banks: &Path) -> String {
    format!(
        "edits not verified against the banks in {} until the TX81Z importer (#302)",
        banks.display()
    )
}

/// The port Source files in `dir` (`*.toml`), by name; none, or no
/// `dir`, is an error naming `$CHIMERA_TX_PORTS`.
pub fn port_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let none = |why: String| {
        format!(
            "{}: {why} (${PORTS_VAR} names the ports' folder)",
            dir.display()
        )
    };
    let mut files = std::fs::read_dir(dir)
        .map_err(|e| none(e.to_string()))?
        .map(|e| e.map(|e| e.path()).map_err(|e| none(e.to_string())))
        .filter(|p| {
            p.as_ref().map_or(true, |p| {
                p.is_file() && p.extension().is_some_and(|x| x == "toml")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if files.is_empty() {
        return Err(none("no port Sources (*.toml)".into()));
    }
    files.sort();
    Ok(files)
}

/// Every port Source in `dir`, parsed (each error names its file).
pub fn ports(dir: &Path) -> Result<Vec<Source>, String> {
    let mut all = Vec::new();
    for p in port_files(dir)? {
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        all.extend(source::parse(&text, &name)?);
    }
    Ok(all)
}

/// The Sounds a run reads, checked together (unique ids and names): the
/// public Sources in `dir` behind the public walls, then with `--private`
/// the ports in `ports` behind theirs.
pub fn sounds(dir: &Path, ports: Option<&Path>) -> Result<Vec<Source>, String> {
    let mut all = sources(dir)?;
    source::public(&all)?;
    if let Some(p) = ports {
        let ported = self::ports(p)?;
        source::ports(&ported)?;
        all.extend(ported);
    }
    source::check(&all)?;
    Ok(all)
}

/// Spec § 6 "Edits verified": a port, against its voice re-imported from
/// its bank (`$CHIMERA_TX_BANKS`), must change something in every
/// category its `edits` names. The TX81Z importer (#302) implements it;
/// until one is passed, no build claims the check.
pub trait EditsCheck {
    /// Err at the port's `edits` line when it changes nothing in a
    /// category it names.
    fn verify(&self, port: &Source) -> Result<(), String>;
}

/// The ports into `<root>/private/` (removed before anything is checked)
/// and nowhere else:
/// `CHIMERA/SOUNDS/S91000xx.A` and `manifest.json`. Checked with the
/// public Sources in `dir` (ids and names unique across both), and each
/// against its bank by `edits` when one is given.
pub fn private(
    dir: &Path,
    ports: &Path,
    root: &Path,
    stamp: &BuildStamp,
    edits: Option<&dyn EditsCheck>,
) -> Result<Manifest, String> {
    // A failed build leaves no old set behind to install.
    let out = root.join("private");
    match std::fs::remove_dir_all(&out) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", out.display()));
        }
        _ => {}
    }
    let all = sounds(dir, Some(ports))?;
    let ported: Vec<Source> = all.into_iter().filter(|s| s.from.is_some()).collect();
    super::qa::sweeps(&ported)?;
    if let Some(check) = edits {
        for p in &ported {
            check.verify(p)?;
        }
    }
    let (files, man) = files(&ported, stamp)?;
    write_set(&out, &files, None, &man)?;
    Ok(man)
}
