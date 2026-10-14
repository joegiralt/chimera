use chimera_image::crc::Crc32;
use chimera_image::*;

struct Mem(Vec<u8>, bool);

impl ImageRead for Mem {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let o = off as usize;
        let src = o.checked_add(buf.len()).and_then(|e| self.0.get(o..e));
        match src {
            Some(s) if !self.1 => {
                buf.copy_from_slice(s);
                Ok(())
            }
            _ => Err(ReadFail),
        }
    }
}

// SAFETY: test stand-in, not a chip.
unsafe impl Bank2Memory for Mem {}

const SEAL: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

fn seal() -> Seal {
    Seal::new(SEAL).unwrap()
}

fn version(s: &str) -> Version {
    Version::new(s)
}

fn header_for(body: &[u8]) -> Header {
    let mut c = Crc32::new();
    c.update(body);
    Header {
        body_len: body.len() as u32,
        body_crc: c.finish(),
        door_off: 0x100,
        seal: SEAL,
        version: version("0.2.0 02e440e2"),
    }
}

fn sealed(body: &[u8]) -> (Vec<u8>, Seal) {
    let mut v = header_for(body).to_bytes(true).to_vec();
    v.extend_from_slice(body);
    (v, seal())
}

fn body(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + 3) as u8).collect()
}

fn judged(img: Vec<u8>, want: &Seal) -> Verdict {
    judge(&mut Mem(img, false), want)
}

#[test]
fn good_image_is_good() {
    let (img, s) = sealed(&body(1024));
    assert!(matches!(judged(img, &s), Verdict::Good(_)));
}

#[test]
fn erased_is_empty() {
    assert_eq!(judged(vec![0xFF; 256], &seal()), Verdict::Empty);
}

#[test]
fn uncommitted_is_torn() {
    let (mut img, s) = sealed(&body(1024));
    for i in 0..32 {
        img[0xE0 + i] = 0xFF;
    }
    img[0xE0 + 5] = 0xFE;
    assert_eq!(judged(img, &s), Verdict::Torn);
    let (mut img, s) = sealed(&body(1024));
    img[0xE0..0x100].fill(0xFF);
    assert_eq!(judged(img, &s), Verdict::Empty);
}

#[test]
fn foreign_magic() {
    let (mut img, s) = sealed(&body(1024));
    img[0] = b'X';
    assert_eq!(judged(img, &s), Verdict::Foreign);
}

#[test]
fn future_format() {
    let (mut img, s) = sealed(&body(1024));
    img[8] = 2;
    assert_eq!(judged(img, &s), Verdict::Format(2));
}

#[test]
fn bounds_are_corrupt() {
    let b = body(1024);
    for len in [0, BANK2_LEN - 224, 33] {
        let mut h = header_for(&b);
        h.body_len = len;
        let mut img = h.to_bytes(true).to_vec();
        img.extend_from_slice(&b);
        assert_eq!(judged(img, &seal()), Verdict::Corrupt, "len {len}");
    }
    for off in [0x80, 0x100 + 1024, 0x104] {
        let mut h = header_for(&b);
        h.door_off = off;
        let mut img = h.to_bytes(true).to_vec();
        img.extend_from_slice(&b);
        assert_eq!(judged(img, &seal()), Verdict::Corrupt, "off {off:#x}");
    }
}

#[test]
fn other_seal_is_stale_with_version() {
    let (img, _) = sealed(&body(1024));
    let other = Seal::new([9; 8]).unwrap();
    match judged(img, &other) {
        Verdict::Stale { have } => assert_eq!(have.as_str(), "0.2.0 02e440e2"),
        v => panic!("{v:?}"),
    }
}

#[test]
fn every_single_byte_flip_is_never_good() {
    let (img, s) = sealed(&body(1024));
    for i in (0..img.len()).filter(|i| !(0x20..0x40).contains(i)) {
        let mut f = img.clone();
        f[i] ^= 1;
        let v = judged(f, &s);
        assert!(!matches!(v, Verdict::Good(_)), "byte {i:#x}");
    }
}

#[test]
fn read_fail_is_unreadable() {
    let (img, s) = sealed(&body(1024));
    assert_eq!(judge(&mut Mem(img.clone(), true), &s), Verdict::Unreadable);
    // header reads, body does not
    assert_eq!(
        judge(&mut Mem(img[..300].to_vec(), false), &s),
        Verdict::Unreadable
    );
}

#[test]
fn seal_rejects_placeholders() {
    assert!(Seal::new(*b"UNSEALED").is_none());
    assert!(Seal::new([0; 8]).is_none());
    assert!(Seal::new([0xFF; 8]).is_none());
}

#[test]
fn version_is_printable_up_to_nul() {
    let mut raw = [0u8; 32];
    raw[..4].copy_from_slice(&[b'a', 0xC3, 0x01, b'b']);
    raw[5] = b'z';
    assert_eq!(Version::from_bytes(raw).as_str(), "a??b");
    assert_eq!(Version::from_bytes([b'x'; 32]).as_str().len(), 32);
}

#[test]
fn door_opens_only_on_good() {
    let (img, s) = sealed(&body(1024));
    let mut m = Mem(img.clone(), false);
    let d = Door::open(&mut m, &s).unwrap();
    assert_eq!(d.door_addr(), BANK2_BASE + 0x100);
    assert_eq!(d.version().as_str(), "0.2.0 02e440e2");
    assert_eq!(d.seal().bytes(), SEAL);
    assert_eq!(judged(img.clone(), &s), Verdict::Good(*d.header()));
    let other = Seal::new([9; 8]).unwrap();
    assert!(matches!(
        Door::open(&mut Mem(img.clone(), false), &other),
        Err(Verdict::Stale { .. })
    ));
    let mut bad = img;
    bad[0x120] ^= 1;
    assert_eq!(
        Door::open(&mut Mem(bad, false), &s).err(),
        Some(Verdict::Corrupt)
    );
    assert_eq!(
        Door::open(&mut Mem(vec![0xFF; 256], false), &s).err(),
        Some(Verdict::Empty)
    );
}

#[test]
fn crc_known_answer() {
    let mut c = Crc32::new();
    c.update(b"123456789");
    assert_eq!(c.finish(), 0xCBF4_3926);
}

#[test]
fn unsealed_header_is_the_zero_header_committed() {
    let h = Header {
        body_len: 0,
        body_crc: 0,
        door_off: 0,
        seal: [0; 8],
        version: version(""),
    };
    assert_eq!(UNSEALED_HEADER, h.to_bytes(true));
    let mut img = UNSEALED_HEADER.to_vec();
    img.extend_from_slice(&body(1024));
    assert_eq!(judged(img, &seal()), Verdict::Corrupt);
}
