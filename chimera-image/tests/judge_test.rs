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

/// `body` padded with 0xFF to the one image size.
fn padded(body: &[u8]) -> Vec<u8> {
    let mut b = body.to_vec();
    b.resize(IMAGE_BODY as usize, 0xFF);
    b
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
    let body = padded(body);
    let mut v = header_for(&body).to_bytes(true).to_vec();
    v.extend_from_slice(&body);
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
    img[8] = 3;
    assert_eq!(judged(img, &s), Verdict::Format(3));
}

#[test]
fn bounds_are_corrupt() {
    let b = padded(&body(1024));
    for len in [0, BANK2_LEN - 224, IMAGE_BODY - 32, IMAGE_BODY + 32, 33] {
        let mut h = header_for(&b);
        h.body_len = len;
        let mut img = h.to_bytes(true).to_vec();
        img.extend_from_slice(&b);
        assert_eq!(judged(img, &seal()), Verdict::Corrupt, "len {len}");
    }
    for off in [0x80, 0x104, 0x200, BANK2_LEN, u32::MAX - 31] {
        let mut h = header_for(&b);
        h.door_off = off;
        let mut img = h.to_bytes(true).to_vec();
        img.extend_from_slice(&b);
        assert_eq!(judged(img, &seal()), Verdict::Corrupt, "off {off:#x}");
    }
}

/// Review Focus 1: a 0.3 image (format 1, S0..S3 only) or a short one never
/// installs over the slots; only the full 1 MB format-2 image does.
#[test]
fn old_or_short_images_never_install() {
    let (mut old, s) = sealed(&body(1024));
    old[8] = 1;
    assert_eq!(judged(old, &s), Verdict::Format(1));
    let short = body(512 * 1024 - 256);
    let mut h = header_for(&short);
    h.body_len = 512 * 1024 - 256;
    let mut img = h.to_bytes(true).to_vec();
    img.extend_from_slice(&short);
    assert_eq!(judged(img, &s), Verdict::Corrupt);
    let full = body(IMAGE_BODY as usize);
    let mut img = header_for(&full).to_bytes(true).to_vec();
    img.extend_from_slice(&full);
    assert_eq!(img.len(), 1 << 20);
    assert!(matches!(judged(img, &s), Verdict::Good(_)));
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
    // The header, the live body, and the padding's edges: a full 1 MB
    // sweep is a CRC of 1 MB per byte.
    let n = img.len();
    let at = (0..0x100 + 1024 + 64).chain(n - 64..n);
    for i in at.filter(|i| !(0x20..0x40).contains(i)) {
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

#[test]
fn pick_staged_takes_the_first_good() {
    let h = header_for(&body(64));
    let stale = Verdict::Stale {
        have: version("0.1.0"),
    };
    assert_eq!(pick_staged([]), None);
    assert_eq!(pick_staged([Verdict::Empty, stale, Verdict::Corrupt]), None);
    assert_eq!(pick_staged([Verdict::Good(h)]), Some((0, h)));
    let mut later = h;
    later.body_len += 32;
    assert_eq!(
        pick_staged([
            stale,
            Verdict::Foreign,
            Verdict::Good(h),
            Verdict::Good(later)
        ]),
        Some((2, h))
    );
}

#[test]
fn pick_staged_stops_at_the_first_good() {
    let h = header_for(&body(64));
    let mut judged = 0;
    let verdicts = [
        Verdict::Torn,
        Verdict::Good(h),
        Verdict::Empty,
        Verdict::Empty,
    ];
    let got = pick_staged(verdicts.into_iter().inspect(|_| judged += 1));
    assert_eq!(got, Some((1, h)));
    assert_eq!(judged, 2, "nothing after the pick is judged");
}

/// Bank 2 past S0's header, unreadable: a torn install's S4..S7 (spec § 4).
/// Logs each read.
struct HeaderOnly(Vec<u8>, Vec<(u32, usize)>);

impl ImageRead for HeaderOnly {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        self.1.push((off, buf.len()));
        if off as usize + buf.len() > HEADER_LEN as usize {
            return Err(ReadFail);
        }
        buf.copy_from_slice(&self.0[off as usize..off as usize + buf.len()]);
        Ok(())
    }
}

/// Until the commit word is good the judge reads S0's header and nothing
/// else, so a torn install's slot words, anywhere in the 1 MB, are only
/// ever read by the guard (spec § 4).
#[test]
fn the_judge_reads_only_the_header_until_committed() {
    let (img, s) = sealed(&body(1024));
    let c = COMMIT_OFF as usize;
    let mut erased = img.clone();
    erased[c..c + 32].fill(0xFF);
    let mut torn = img.clone();
    torn[c + 9] &= 0x7F;
    for (what, img, want) in [
        ("erased", erased, Verdict::Empty),
        ("torn", torn, Verdict::Torn),
    ] {
        let mut m = HeaderOnly(img, Vec::new());
        assert_eq!(judge(&mut m, &s), want, "{what}");
        assert_eq!(m.1, [(0, HEADER_LEN as usize)], "{what}");
    }
    // A committed header reads on, past the header: the body's CRC.
    let mut m = HeaderOnly(img, Vec::new());
    assert_eq!(judge(&mut m, &s), Verdict::Unreadable);
    assert_eq!(m.1, [(0, 256), (HEADER_LEN, 512)]);
}
