//! The bank-1 state of the door: closed until a judged `Door` at this link's
//! door address opens it. A host has no bank 2, so nothing here is ever
//! dereferenced: `get` is only called closed.

use chimera_engine::bank2::{DOOR_ADDR, DoorCell};
use chimera_image::crc::Crc32;
use chimera_image::{BANK2_BASE, Bank2Memory, Door, Header, ImageRead, ReadFail, Seal, Version};

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
// cell here, and no test calls `get` while one is open.
unsafe impl Bank2Memory for Mem {}

const SEAL: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

fn door(door_off: u32) -> Door {
    let body = vec![0x5A; 1024];
    let mut c = Crc32::new();
    c.update(&body);
    let h = Header {
        body_len: body.len() as u32,
        body_crc: c.finish(),
        door_off,
        seal: SEAL,
        version: Version::new("0.2.0 test"),
    };
    let mut img = h.to_bytes(true).to_vec();
    img.extend_from_slice(&body);
    Door::open(&mut Mem(img), &Seal::new(SEAL).unwrap()).unwrap()
}

#[test]
fn door_addr_follows_the_header() {
    assert_eq!(DOOR_ADDR, BANK2_BASE + 0x100);
}

#[test]
fn starts_closed() {
    let c = DoorCell::new();
    assert!(!c.is_open());
    assert!(c.get().is_none());
}

#[test]
fn opens_on_this_links_door_and_closes() {
    let c = DoorCell::new();
    assert!(c.open(&door(0x100)));
    assert!(c.is_open());
    c.close();
    assert!(!c.is_open());
    assert!(c.get().is_none());
}

#[test]
fn a_door_elsewhere_stays_closed() {
    let c = DoorCell::new();
    assert!(!c.open(&door(0x200)));
    assert!(!c.is_open());
    assert!(c.get().is_none());
    assert!(c.open(&door(0x100)));
    assert!(!c.open(&door(0x200)));
    assert!(!c.is_open());
}
