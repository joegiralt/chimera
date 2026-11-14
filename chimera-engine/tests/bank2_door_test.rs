//! The bank-1 state of the door: closed until a judged `Door` at this link's
//! door address opens it. A host has no bank 2, so nothing here is ever
//! dereferenced: `with` is only called closed.

use chimera_engine::bank2::{DOOR_ADDR, DoorCell, Judged};
use chimera_image::crc::Crc32;
use chimera_image::{
    BANK2_BASE, Bank2Memory, Door, Header, IMAGE_BODY, ImageRead, ReadFail, Seal, Version,
};

struct Mem(Vec<u8>);

impl ImageRead for Mem {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let o = off as usize;
        let s = o.checked_add(buf.len()).and_then(|e| self.0.get(o..e));
        buf.copy_from_slice(s.ok_or(ReadFail)?);
        Ok(())
    }
}

// SAFETY: a test stand-in, not a chip: its `Door`s only open and close the
// cell here, and no test calls `with` while one is open.
unsafe impl Bank2Memory for Mem {}

const SEAL: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// Opens `c` on a door at `door_off`, if it is this link's.
fn open(c: &DoorCell, door_off: u32) -> bool {
    // SAFETY: the door was judged against `SEAL`, the stand-in build's seal.
    match door(door_off).and_then(|d| unsafe { Judged::of(&d) }) {
        Some(j) => {
            c.open(j.check(|_| true).expect("passes"));
            true
        }
        None => {
            c.close();
            false
        }
    }
}

/// The judged door of a full image whose header says `door_off`; `None`
/// once the header itself refuses it (format 2 fixes the door at 0x100).
fn door(door_off: u32) -> Option<Door> {
    let mut body = vec![0x5A; 1024];
    body.resize(IMAGE_BODY as usize, 0xFF);
    let mut c = Crc32::new();
    c.update(&body);
    let h = Header {
        body_len: IMAGE_BODY,
        body_crc: c.finish(),
        door_off,
        seal: SEAL,
        version: Version::new("0.2.0 test"),
    };
    let mut img = h.to_bytes(true).to_vec();
    img.extend_from_slice(&body);
    Door::open(&mut Mem(img), &Seal::new(SEAL).unwrap()).ok()
}

#[test]
fn door_addr_follows_the_header() {
    assert_eq!(DOOR_ADDR, BANK2_BASE + 0x100);
}

#[test]
fn starts_closed() {
    let c = DoorCell::new();
    assert!(!c.is_open());
    assert!(c.with(|_| ()).is_none());
}

#[test]
fn opens_on_this_links_door_and_closes() {
    let c = DoorCell::new();
    assert!(open(&c, 0x100));
    assert!(c.is_open());
    c.close();
    assert!(!c.is_open());
    assert!(c.with(|_| ()).is_none());
}

#[test]
fn a_door_elsewhere_stays_closed() {
    let c = DoorCell::new();
    assert!(!open(&c, 0x200));
    assert!(!c.is_open());
    assert!(c.with(|_| ()).is_none());
    assert!(open(&c, 0x100));
    assert!(!open(&c, 0x200));
    assert!(!c.is_open());
}

/// I2: a judged door is checked before it is published: judging leaves
/// the cell closed, so no interrupt calls through it until `open`.
#[test]
fn a_judged_door_is_not_open_till_opened() {
    let c = DoorCell::new();
    // SAFETY: as `open`.
    let j = unsafe { Judged::of(&door(0x100).unwrap()) }.expect("this link's door");
    assert!(!c.is_open());
    assert!(c.with(|_| ()).is_none());
    c.open(j.check(|_| true).expect("passes"));
    assert!(c.is_open());
}

/// A judged door that fails its check is never `Checked`, so never opens.
#[test]
fn a_door_that_fails_its_check_cannot_open() {
    // SAFETY: as `open`.
    let j = unsafe { Judged::of(&door(0x100).unwrap()) }.expect("this link's door");
    assert!(j.check(|_| false).is_none());
}
