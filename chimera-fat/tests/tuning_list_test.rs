//! A folder as a Mac leaves it after the bootloader's card mode (#930, ADR
//! 0259): AppleDouble `._FOO.<ext>` (short name `_FOO~1`, not hidden),
//! `.DS_Store` and a hidden file beside the real one, and a file of another
//! extension. The TUNINGS (`.SCL`) and MAPS (`.A`/`.B`) listings hold only
//! the real file.

#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tree.rs"]
mod tree;

use chimera_core::storage::list_stems;
use chimera_hal::store::{Dir, Store};
use image::with_clusters;
use probe::probed;
use std::rc::Rc;
use tree::Tree;

const FILE: u8 = 0x20;
const HIDDEN_FILE: u8 = 0x22;

fn short(stem: &str, ext: &str) -> [u8; 11] {
    let mut n = [b' '; 11];
    n[..stem.len()].copy_from_slice(stem.as_bytes());
    n[8..8 + ext.len()].copy_from_slice(ext.as_bytes());
    n
}

fn listed(dir: Dir, folder: [u8; 11], ext: &str, other: &str, exts: &[&[u8]]) -> Vec<Vec<u8>> {
    let disk = Rc::new(with_clusters(4_200, 0x7E57));
    {
        let mut t = Tree::new(&disk);
        let mut root = t.root();
        let mut chimera = t.mkdir(&mut root, *b"CHIMERA    ", None);
        let mut d = t.mkdir(&mut chimera, folder, None);
        t.file(&mut d, short("FOO", ext), None, (40, 0), 1, None);
        t.file(&mut d, short("FOO", other), None, (40, 0), 2, None);
        for (name, attr, long) in [
            (short("_FOO~1", ext), FILE, format!("._FOO.{ext}")),
            (short("DS_STO~1", ""), HIDDEN_FILE, ".DS_Store".into()),
            (short("BAR", ext), HIDDEN_FILE, format!("BAR.{ext}")),
        ] {
            t.put_named(&mut d, tree::raw(name, attr, 0, 0), Some(&long));
        }
    }
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let mut out = [([0u8; 8], 0u8); 8];
    let n = list_stems(&mut s, vol, dir, exts, &mut out).unwrap();
    out[..n]
        .iter()
        .map(|(b, l)| b[..usize::from(*l)].to_vec())
        .collect()
}

#[test]
fn a_mac_folder_lists_only_its_real_scl_files() {
    let got = listed(Dir::Tunings, *b"TUNINGS    ", "SCL", "KBM", &[b"SCL"]);
    assert_eq!(got, [b"FOO".to_vec()]);
}

#[test]
fn a_mac_folder_lists_only_its_real_controller_maps() {
    let got = listed(Dir::Maps, *b"MAPS       ", "A", "TXT", &[b"A", b"B"]);
    assert_eq!(got, [b"FOO".to_vec()]);
}
