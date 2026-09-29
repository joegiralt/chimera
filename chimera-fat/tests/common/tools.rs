//! dosfstools as a second opinion: `mkfs.fat` makes images, `fsck.fat -n`
//! checks them. A missing tool fails the test; it never skips.

use crate::image::{PART_LBA, RamDisk, mbr};
use chimera_fat::volume::FsKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// `name` on `$PATH`, then in `/usr/sbin` and `/sbin`.
pub fn tool(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/usr/sbin".into(), "/sbin".into()])
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            panic!(
                "dosfstools missing: install dosfstools (e.g. `apt install dosfstools`); \
                 `just test` needs it"
            )
        })
}

/// A fresh image file's path, unique across the tests running at once.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{tag}-{}-{n}.img", std::process::id()))
}

fn run(cmd: &mut Command) -> (i32, String) {
    let out = cmd.output().expect("run a dosfstools tool");
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().expect("exit code"), text)
}

/// A `blocks`-block card: the MBR `image::mbr` writes, and `mkfs.fat`'s
/// volume from `PART_LBA`, labelled `CHIMERA`.
pub fn mkfs(kind: FsKind, blocks: u32, spc: u8, serial: u32) -> RamDisk {
    let path = scratch("mkfs");
    std::fs::File::create(&path)
        .and_then(|f| f.set_len(u64::from(blocks) * 512))
        .unwrap();
    let (fat, type_byte) = match kind {
        FsKind::Fat16 => ("16", 0x0E),
        FsKind::Fat32 => ("32", 0x0C),
    };
    let (lba, spc, serial) = (
        PART_LBA.to_string(),
        spc.to_string(),
        format!("{serial:08X}"),
    );
    let args = [
        "--offset", &lba, "-h", &lba, "-F", fat, "-s", &spc, "-i", &serial,
    ];
    let (code, out) = run(Command::new(tool("mkfs.fat"))
        .args(args)
        .args(["-n", "CHIMERA"])
        .arg(&path));
    assert_eq!(code, 0, "mkfs.fat failed: {out}");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let disk = RamDisk::zeroed(blocks);
    {
        let mut d = disk.0.borrow_mut();
        for (block, bytes) in d.iter_mut().zip(bytes.as_chunks::<512>().0) {
            *block = *bytes;
        }
        mbr(&mut d, type_byte);
    }
    disk
}

/// `fsck.fat -n` on a partition: its exit code, its output, and its summary
/// line's `used/total clusters`.
pub struct Fsck {
    pub code: i32,
    pub out: String,
    pub used: u32,
    pub total: u32,
}

/// Runs `fsck.fat -n` on `disk`'s partition alone.
pub fn fsck(disk: &RamDisk) -> Fsck {
    let path = scratch("fsck");
    let part: Vec<u8> = disk.0.borrow()[PART_LBA as usize..].concat();
    std::fs::write(&path, part).unwrap();
    let (code, out) = run(Command::new(tool("fsck.fat")).arg("-n").arg(&path));
    std::fs::remove_file(&path).unwrap();
    // "<path>: N files, used/total clusters"
    let (used, total) = out
        .lines()
        .find_map(|l| {
            let counts = l.strip_suffix(" clusters")?.rsplit(' ').next()?;
            let (used, total) = counts.split_once('/')?;
            Some((used.parse().ok()?, total.parse().ok()?))
        })
        .unwrap_or_else(|| panic!("no summary line in fsck.fat's output: {out}"));
    Fsck {
        code,
        out,
        used,
        total,
    }
}
