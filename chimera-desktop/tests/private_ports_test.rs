//! Privacy guard: no TX81Z port, bank or private Source ever in the index.

use chimera_desktop::privacy::offences;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `git` in `dir`, blind to the user's and the system's config: a test
/// repo behaves the same on any machine.
fn git(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// Offences among the files in `dir`'s index, each read as staged (its
/// blob), not as the working tree has it now.
fn scan_index(dir: &Path) -> Vec<String> {
    let list = git(dir, &["ls-files", "-s", "-z"]);
    let files: Vec<(String, Vec<u8>)> = list
        .split(|&b| b == 0)
        .filter(|e| !e.is_empty())
        .map(|e| {
            // `<mode> <blob> <stage>\t<path>`
            let e = String::from_utf8(e.to_vec()).unwrap();
            let (meta, path) = e.split_once('\t').unwrap();
            let blob = meta.split(' ').nth(1).unwrap();
            // Only a Source's content is checked; any other file by its path.
            let bytes = if path.ends_with(".toml") {
                git(dir, &["cat-file", "blob", blob])
            } else {
                Vec::new()
            };
            (path.to_string(), bytes)
        })
        .collect();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    offences(&refs)
}

/// A scratch repo, removed when dropped, a failed assertion included.
struct TempRepo(PathBuf);

impl TempRepo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("chimera-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        TempRepo(dir)
    }

    /// `text` into `path`, staged.
    fn stage(&self, path: &str, text: &str) {
        std::fs::write(self.0.join(path), text).unwrap();
        git(&self.0, &["add", path]);
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn no_private_ports_in_index() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(scan_index(&root), Vec::<String>::new());
}

/// ADR 0138: an inspired-by port Source is public; only raw bank files
/// are offences, staged or not.
#[test]
fn staged_port_source_is_fine_but_a_staged_bank_is_caught() {
    let r = TempRepo::new("privacy-staged");
    r.stage(
        "x.toml",
        "id = \"S9100010\"\nfrom = \"tx81z 1a2b3c4d #7\"\n",
    );
    assert!(scan_index(&r.0).is_empty());
    std::fs::write(r.0.join("bank.syx"), b"\xf0\xf7").unwrap();
    git(&r.0, &["add", "bank.syx"]);
    let found = scan_index(&r.0);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("bank.syx"));
}

#[test]
fn content_does_not_matter_only_the_path() {
    let f: [(&str, &[u8]); 1] = [("tools/factory-sounds/fm.toml", b"from = \"tx81z ab #1\"")];
    assert!(offences(&f).is_empty());
    let f: [(&str, &[u8]); 1] = [("tools/factory-sounds/private/fm.toml", b"")];
    assert_eq!(offences(&f).len(), 1);
}

/// A port's id and `from` line are ordinary content now.
#[test]
fn a_port_id_or_from_is_not_an_offence() {
    for ok in [
        "id = \"S9100010\"",
        "id='S9100025'",
        "from = 'tx81z 1a2b3c4d #7'",
        "from = \"tx81z 1a2b3c4d #7\"",
    ] {
        assert!(offences(&[("a.toml", ok.as_bytes())]).is_empty(), "{ok}");
    }
}

#[test]
fn bank_files_and_private_dir_are_offences() {
    for p in [
        "a.syx",
        "B.SYX",
        "c.bnk",
        "d.mix",
        "e.wav",
        "tools/factory-sounds/private/x.toml",
    ] {
        assert_eq!(offences(&[(p, b"")]).len(), 1, "{p}");
    }
    let ok: [(&str, &[u8]); 3] = [
        ("tools/factory-sounds/va.toml", b"name = \"Pad\"\n"),
        ("chimera-desktop/fuzz/seeds/read_wav/float32.wav", b""),
        ("docs/n.md", b"from = \"tx81z"),
    ];
    assert!(offences(&ok).is_empty());
}
