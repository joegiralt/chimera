//! Privacy guard: no TX81Z port, bank or private Source ever in the index.

use chimera_desktop::privacy::offences;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .current_dir(dir)
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

/// Offences among the files in `dir`'s index.
fn scan_index(dir: &Path) -> Vec<String> {
    let list = git(dir, &["ls-files", "-z"]);
    let files: Vec<(String, Vec<u8>)> = list
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let path = String::from_utf8(p.to_vec()).unwrap();
            let bytes = std::fs::read(dir.join(&path)).unwrap_or_default();
            (path, bytes)
        })
        .collect();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    offences(&refs)
}

#[test]
fn no_private_ports_in_index() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(scan_index(&root), Vec::<String>::new());
}

#[test]
fn staged_port_source_is_caught() {
    let dir = std::env::temp_dir().join(format!("chimera-privacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "user.email", "redacted"]);
    std::fs::write(dir.join("x.toml"), "from = \"tx81z 1a2b3c4d #7\"\n").unwrap();
    git(&dir, &["add", "x.toml"]);
    let found = scan_index(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("x.toml"));
}

#[test]
fn content_match_beats_path() {
    let f: [(&str, &[u8]); 1] = [("tools/factory-sounds/fm.toml", b"from = \"tx81z ab #1\"")];
    assert_eq!(offences(&f).len(), 1);
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
