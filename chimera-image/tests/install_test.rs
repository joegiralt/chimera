use chimera_image::crc::Crc32;
use chimera_image::slot::{Evidence, SlotPlan, SlotRead, Word, guard, own_words};
use chimera_image::*;

const SECTOR: usize = 128 * 1024;
const LEN: usize = BANK2_LEN as usize;

/// Fixed-seed xorshift32: every run cuts the same way.
#[derive(Clone)]
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Erase(Bank2Sector),
    Program(u32),
}

/// What a torn erase leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tear {
    /// A prefix of the sector erased, the rest as it was: bits only rise.
    M,
    /// As M, and every word of the sector ECC-flagged, erased ones too.
    GAll,
    /// As M, and one word in eight of the sector ECC-flagged at random,
    /// erased ones too.
    GScatter,
}

const TEARS: [Tear; 3] = [Tear::M, Tear::GAll, Tear::GScatter];

/// Bank 2 as flash behaves: erase to 0xFF, program once, and a power cut
/// that tears the op it lands on. A torn program only clears bits (1 -> 0)
/// and may leave the word failing ECC on read, or silently corrected, until
/// it is erased. A second program of a word panics: the installer never
/// does one.
#[derive(Clone)]
struct SimFlash {
    mem: Vec<u8>,
    /// Words whose ECC a tear broke: reads over them fault.
    bad: Vec<bool>,
    /// Words SEC-DED corrected on read (SR2.SNECCERR).
    corrected: Vec<bool>,
    tear: Tear,
    /// Ops left before the power goes; `None` is unlimited.
    budget: Option<usize>,
    dead: bool,
    ops: Vec<Op>,
    rng: Rng,
}

impl SimFlash {
    fn new() -> Self {
        SimFlash {
            mem: vec![0xFF; LEN],
            bad: vec![false; LEN / 32],
            corrected: vec![false; LEN / 32],
            tear: Tear::M,
            budget: None,
            dead: false,
            ops: Vec::new(),
            rng: Rng(0x2545_F491),
        }
    }

    /// The op that would be number `n` tears; it and all after it fail.
    fn cut_after(&mut self, n: usize) {
        self.budget = Some(n);
    }

    /// Power back: unlimited budget, memory as the cut left it.
    fn reboot(&mut self) {
        self.budget = None;
        self.dead = false;
        self.ops.clear();
    }

    /// `true` if this op runs whole, `false` if it is the one the cut tears.
    fn spend(&mut self) -> Result<bool, FlashFail> {
        if self.dead {
            return Err(FlashFail::Timeout);
        }
        match &mut self.budget {
            Some(0) => {
                self.dead = true;
                Ok(false)
            }
            Some(n) => {
                *n -= 1;
                Ok(true)
            }
            None => Ok(true),
        }
    }

    fn image(&self, len: usize) -> &[u8] {
        &self.mem[..len]
    }
}

impl ImageRead for SimFlash {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let o = off as usize;
        let e = o
            .checked_add(buf.len())
            .filter(|&e| e <= LEN)
            .ok_or(ReadFail)?;
        if e > o && self.bad[o / 32..=(e - 1) / 32].iter().any(|&b| b) {
            return Err(ReadFail);
        }
        buf.copy_from_slice(&self.mem[o..e]);
        Ok(())
    }
}

// SAFETY: test stand-in, not a chip.
unsafe impl Bank2Memory for SimFlash {}

// SAFETY: test stand-in, not a chip.
unsafe impl Flash for SimFlash {
    fn erase(&mut self, s: Bank2Sector) -> Result<(), FlashFail> {
        let whole = self.spend()?;
        self.ops.push(Op::Erase(s));
        let base = (s.addr() - BANK2_BASE) as usize;
        assert!(base + SECTOR <= LEN);
        let n = if whole {
            SECTOR
        } else {
            self.rng.next() as usize % SECTOR
        };
        self.mem[base..base + n].fill(0xFF);
        let words = base / 32..(base + n) / 32;
        self.bad[words.clone()].fill(false);
        self.corrected[words].fill(false);
        if whole {
            return Ok(());
        }
        if self.tear != Tear::M {
            let all = self.tear == Tear::GAll;
            for w in base / 32..(base + SECTOR) / 32 {
                if all || self.rng.next().is_multiple_of(8) {
                    self.bad[w] = true;
                }
            }
        }
        Err(FlashFail::Timeout)
    }

    fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail> {
        if self.dead {
            return Err(FlashFail::Timeout);
        }
        let o = (w.get() - BANK2_BASE) as usize;
        assert!(o.is_multiple_of(32) && o + 32 <= LEN);
        let fresh = self.mem[o..o + 32].iter().all(|&b| b == 0xFF)
            && !self.bad[o / 32]
            && !self.corrected[o / 32];
        assert!(fresh, "second program of {:#x}", w.get());
        let whole = self.spend()?;
        self.ops.push(Op::Program(w.get()));
        if whole {
            self.mem[o..o + 32].copy_from_slice(data);
            return Ok(());
        }
        for (m, &d) in self.mem[o..o + 32].iter_mut().zip(data) {
            // Clear a random subset of the bits the program would clear.
            let r = self.rng.next() as u8;
            *m &= d | !r;
        }
        match self.rng.next() % 3 {
            0 => self.bad[o / 32] = true,
            1 => self.corrected[o / 32] = true,
            _ => {}
        }
        Err(FlashFail::Timeout)
    }
}

impl SlotRead for SimFlash {
    fn word(&mut self, addr: u32) -> Word {
        let o = (addr - BANK2_BASE) as usize;
        Word {
            bytes: self.mem[o..o + 32].try_into().unwrap(),
            ecc: self.bad[o / 32],
            corrected: self.corrected[o / 32],
        }
    }
}

/// A staged file in memory; `flip` corrupts one byte from the given read on
/// (a card changing under the writer).
struct Staged {
    bytes: Vec<u8>,
    reads: usize,
    flip: Option<(usize, usize)>,
    fail_from: Option<usize>,
}

impl Staged {
    fn new(bytes: Vec<u8>) -> Self {
        Staged {
            bytes,
            reads: 0,
            flip: None,
            fail_from: None,
        }
    }
}

impl ImageRead for Staged {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        self.reads += 1;
        if self.fail_from.is_some_and(|n| self.reads >= n) {
            return Err(ReadFail);
        }
        if let Some((n, at)) = self.flip
            && self.reads == n
        {
            self.bytes[at] ^= 0x01;
        }
        let o = off as usize;
        let src = o.checked_add(buf.len()).and_then(|e| self.bytes.get(o..e));
        let s = src.ok_or(ReadFail)?;
        buf.copy_from_slice(s);
        Ok(())
    }
}

const SEAL_A: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const SEAL_B: [u8; 8] = [9, 10, 11, 12, 13, 14, 15, 16];

fn seal(b: [u8; 8]) -> Seal {
    Seal::new(b).unwrap()
}

/// `body` padded with 0xFF to the one image size, the six slot words in.
fn sealed(body: &[u8], s: [u8; 8]) -> Vec<u8> {
    let mut body = body.to_vec();
    body.resize(IMAGE_BODY as usize, 0xFF);
    for (a, w) in Slot::ALL.into_iter().flat_map(own_words) {
        let o = (a - BANK2_BASE - HEADER_LEN) as usize;
        body[o..o + 32].copy_from_slice(&w);
    }
    let mut c = Crc32::new();
    c.update(&body);
    let h = Header {
        body_len: IMAGE_BODY,
        body_crc: c.finish(),
        door_off: 0x100,
        seal: s,
        version: Version::new("0.2.0 02e440e2"),
    };
    let mut v = h.to_bytes(true).to_vec();
    v.extend_from_slice(&body);
    v
}

fn dense(n: usize, k: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(7) ^ k).collect()
}

/// A body reaching into S2 whose words are mostly erased (0xFF), so the cut
/// loop stays quick: dense runs at the start, around each sector edge and at
/// the end, one word in 64 elsewhere.
fn sparse_3_sector(k: u8) -> Vec<u8> {
    let n = 2 * SECTOR + 4096 - 256;
    let mut b = vec![0xFFu8; n];
    for w in 0..n / 32 {
        let off = 256 + w * 32;
        let near_edge = [0, SECTOR, 2 * SECTOR, 256 + n]
            .iter()
            .any(|&e| off + 512 > e && off < e + 512);
        if near_edge || w % 64 == 0 {
            for (i, x) in b[w * 32..w * 32 + 32].iter_mut().enumerate() {
                *x = (w as u8).wrapping_add(i as u8) ^ k;
            }
        }
    }
    b
}

fn header(img: &[u8]) -> Header {
    Header::parse(img[..256].try_into().unwrap()).unwrap()
}

fn slot_plan(f: &mut SimFlash) -> SlotPlan {
    guard(f, SlotSet::NONE).expect("slots ours")
}

/// The guard, then `install_with` and no progress.
fn install(
    f: &mut SimFlash,
    staged: &mut impl ImageRead,
    want: &Seal,
    buf: &mut [u8; 512],
) -> Result<Header, InstallFail> {
    let p = slot_plan(f);
    install_with(f, staged, want, buf, &mut (), &p)
}

fn run(flash: &mut SimFlash, img: &[u8], want: &Seal) -> Result<Header, InstallFail> {
    let mut buf = [0u8; 512];
    install(flash, &mut Staged::new(img.to_vec()), want, &mut buf)
}

/// The plan's erases of S0..S3, in order.
fn low_erases(p: &Plan) -> Vec<Bank2Sector> {
    p.ops()
        .iter()
        .filter_map(|o| match o {
            PlanOp::Erase(s) if s.index() < 4 => Some(*s),
            _ => None,
        })
        .collect()
}

#[test]
fn plan_spans_s0_first() {
    use Bank2Sector::*;
    let h = |n: u32| Header {
        body_len: n,
        ..header(&sealed(&dense(32, 0), SEAL_A))
    };
    let p = slot_plan(&mut SimFlash::new());
    assert_eq!(low_erases(&plan(&h(100 * 1024), &p)), [S0]);
    assert_eq!(low_erases(&plan(&h(128 * 1024 - 256), &p)), [S0]);
    assert_eq!(low_erases(&plan(&h(128 * 1024 - 224), &p)), [S0, S1]);
    assert_eq!(low_erases(&plan(&h(200 * 1024), &p)), [S0, S1]);
    let low = [S0, S1, S2, S3];
    assert_eq!(low_erases(&plan(&h(512 * 1024 - 256), &p)), low);
    assert_eq!(low_erases(&plan(&h(IMAGE_BODY), &p)), low);
    // A header never parsed is clamped to the bank.
    assert_eq!(low_erases(&plan(&h(u32::MAX), &p)), low);
    assert_eq!(plan(&h(IMAGE_BODY), &p).ops()[..4], low.map(PlanOp::Erase));
}

#[test]
fn sectors_are_bank_2() {
    use Bank2Sector::*;
    let all = [S0, S1, S2, S3, S4, S5, S6, S7];
    for (i, s) in all.iter().enumerate() {
        assert_eq!(s.addr(), 0x0810_0000 + i as u32 * 0x2_0000, "{s:?}");
        assert_eq!(s.index(), i as u8);
    }
}

#[test]
fn bank2_word_refuses_outside() {
    for a in [
        0x0820_0000,
        0x080F_FFE0,
        0x0810_0010,
        0,
        0x0800_0000,
        u32::MAX,
    ] {
        assert!(Bank2Word::new(a).is_none(), "{a:#x}");
    }
    for a in [
        0x0810_0000,
        0x0817_FFE0,
        0x0818_0000,
        0x081F_FFE0,
        0x0812_0020,
    ] {
        assert_eq!(Bank2Word::new(a).map(|w| w.get()), Some(a));
    }
}

#[test]
fn install_opens() {
    let img = sealed(&dense(200 * 1024, 0x5A), SEAL_A);
    let mut f = SimFlash::new();
    assert_eq!(run(&mut f, &img, &seal(SEAL_A)), Ok(header(&img)));
    assert!(Door::open(&mut f, &seal(SEAL_A)).is_ok());
    assert_eq!(f.image(img.len()), &img[..]);
    assert!(f.mem[img.len()..].iter().all(|&b| b == 0xFF));
}

#[test]
fn erased_body_words_are_skipped() {
    let img = sealed(&sparse_3_sector(0), SEAL_A);
    let mut f = SimFlash::new();
    assert!(run(&mut f, &img, &seal(SEAL_A)).is_ok());
    let programs = f.ops.iter().filter(|o| matches!(o, Op::Program(_))).count();
    let live = img[256..]
        .chunks(32)
        .filter(|w| w.iter().any(|&b| b != 0xFF));
    assert_eq!(programs, live.count() + 8);
    assert_eq!(f.image(img.len()), &img[..]);
}

#[test]
fn stale_staged_touches_nothing() {
    let img = sealed(&dense(4096, 1), SEAL_B);
    let mut f = SimFlash::new();
    let r = run(&mut f, &img, &seal(SEAL_A));
    assert!(
        matches!(r, Err(InstallFail::Staged(Verdict::Stale { .. }))),
        "{r:?}"
    );
    assert!(f.ops.is_empty());
}

#[test]
fn bad_staged_touches_nothing() {
    let mut img = sealed(&dense(4096, 1), SEAL_A);
    img[300] ^= 1;
    let mut f = SimFlash::new();
    let r = run(&mut f, &img, &seal(SEAL_A));
    assert_eq!(r, Err(InstallFail::Staged(Verdict::Corrupt)));
    let r = run(&mut f, &[0xFF; 256], &seal(SEAL_A));
    assert_eq!(r, Err(InstallFail::Staged(Verdict::Empty)));
    assert!(f.ops.is_empty());
}

#[test]
fn staged_changed_under_the_writer_never_commits() {
    let img = sealed(&dense(8192, 3), SEAL_A);
    let mut f = SimFlash::new();
    let mut s = Staged::new(img.clone());
    // The judge reads the header then 16 body blocks; the copy's 3rd read flips.
    s.flip = Some((1 + 16 + 3, 256 + 1100));
    let r = install(&mut f, &mut s, &seal(SEAL_A), &mut [0u8; 512]);
    assert_eq!(r, Err(InstallFail::Readback(Verdict::Corrupt)));
    assert!(!f.ops.contains(&Op::Program(BANK2_BASE + COMMIT_OFF)));
    assert_eq!(judge(&mut f, &seal(SEAL_A)), Verdict::Empty);
}

#[test]
fn staged_failing_mid_copy_never_commits() {
    let img = sealed(&dense(8192, 3), SEAL_A);
    let mut f = SimFlash::new();
    let mut s = Staged::new(img);
    s.fail_from = Some(1 + IMAGE_BODY.div_ceil(512) as usize + 5);
    let r = install(&mut f, &mut s, &seal(SEAL_A), &mut [0u8; 512]);
    assert_eq!(r, Err(InstallFail::Io));
    assert_eq!(judge(&mut f, &seal(SEAL_A)), Verdict::Empty);
}

#[test]
fn commit_word_is_last_op() {
    let img = sealed(&dense(300 * 1024, 7), SEAL_A);
    let mut f = SimFlash::new();
    assert!(run(&mut f, &img, &seal(SEAL_A)).is_ok());
    use Bank2Sector::*;
    // Fresh slots read erased: marker, tag, code each, no erase.
    assert_eq!(f.ops[..4], [S0, S1, S2, S3].map(Op::Erase));
    let slot_words: Vec<Op> = Slot::ALL
        .into_iter()
        .flat_map(|s| [s.marker_addr(), s.tag_addr(), s.base()])
        .map(Op::Program)
        .collect();
    assert_eq!(f.ops[4..10], slot_words[..]);
    assert_eq!(f.ops.last(), Some(&Op::Program(0x0810_00E0)));
    let progs: Vec<u32> = f.ops[10..]
        .iter()
        .map(|o| match o {
            Op::Program(a) => *a,
            Op::Erase(_) => panic!("erase after a program"),
        })
        .collect();
    // Body upward from 0x100, then header words 0x00..0xDF, then the commit.
    let n = progs.len();
    assert!(progs[..n - 8].windows(2).all(|w| w[0] < w[1]));
    assert_eq!(progs[0], 0x0810_0100);
    let hdr: Vec<u32> = (0..7).map(|i| 0x0810_0000 + 32 * i).collect();
    assert_eq!(progs[n - 8..n - 1], hdr[..]);
}

#[test]
fn install_over_committed_flash_reinstalls() {
    let old = sealed(&dense(300 * 1024, 1), SEAL_A);
    let new = sealed(&dense(4096, 2), SEAL_B);
    let mut f = SimFlash::new();
    assert!(run(&mut f, &old, &seal(SEAL_A)).is_ok());
    assert!(run(&mut f, &new, &seal(SEAL_B)).is_ok());
    assert_eq!(f.image(new.len()), &new[..]);
    assert!(Door::open(&mut f, &seal(SEAL_A)).is_err());
}

#[test]
fn flash_errors_never_open() {
    let img = sealed(&dense(4096, 2), SEAL_A);
    let mut f = SimFlash::new();
    f.dead = true;
    let r = run(&mut f, &img, &seal(SEAL_A));
    assert_eq!(
        r,
        Err(InstallFail::Flash(
            FlashFail::Timeout,
            FlashAt::Sector(Bank2Sector::S0)
        ))
    );
    assert!(Door::open(&mut f, &seal(SEAL_A)).is_err());
}

/// A failed op names its sector or word: S1's erase, then the first body
/// word after the four erases and the six slot words.
#[test]
fn a_flash_failure_says_where() {
    let img = sealed(&dense(4096, 2), SEAL_A);
    let mut f = SimFlash::new();
    f.cut_after(1);
    let r = run(&mut f, &img, &seal(SEAL_A));
    let s1 = FlashAt::Sector(Bank2Sector::S1);
    assert_eq!(r, Err(InstallFail::Flash(FlashFail::Timeout, s1)));
    let mut f = SimFlash::new();
    f.cut_after(10);
    let r = run(&mut f, &img, &seal(SEAL_A));
    let at = Bank2Word::new(BANK2_BASE + HEADER_LEN).unwrap();
    assert_eq!(
        r,
        Err(InstallFail::Flash(FlashFail::Timeout, FlashAt::Word(at)))
    );
}

/// After the cut, a verdict of `Good` for `seal` means exactly `img`; and,
/// without leaning on the CRC, `img`'s header committed in flash means its
/// body is all there too.
fn good_means_exact(f: &mut SimFlash, img: &[u8], s: [u8; 8]) {
    if let Verdict::Good(_) = judge(f, &seal(s)) {
        assert_eq!(f.image(img.len()), img, "Good with wrong bytes");
    }
    if f.mem[..256] == img[..256] {
        assert_eq!(f.image(img.len()), img, "committed before the body");
    }
}

fn cut_loop(old: Option<&[u8]>) {
    let new = sealed(&sparse_3_sector(0x33), SEAL_B);
    let want = seal(SEAL_B);
    let base = || {
        let mut f = SimFlash::new();
        if let Some(o) = old {
            assert!(run(&mut f, o, &seal(SEAL_A)).is_ok());
            f.ops.clear();
        }
        f
    };
    let mut whole = base();
    assert!(run(&mut whole, &new, &want).is_ok());
    let total = whole.ops.len();
    assert!(matches!(whole.ops[2], Op::Erase(Bank2Sector::S2)));
    let mut opened = 0;
    for n in 0..=total {
        let mut f = base();
        f.rng = Rng(0x9E37_79B9 ^ n as u32);
        f.cut_after(n);
        let r = run(&mut f, &new, &want);
        assert_eq!(r.is_ok(), n == total, "cut {n}: {r:?}");
        f.reboot();
        good_means_exact(&mut f, &new, SEAL_B);
        if let Some(o) = old {
            good_means_exact(&mut f, o, SEAL_A);
        }
        if Door::open(&mut f, &want).is_ok() {
            opened += 1;
        }
        assert!(run(&mut f, &new, &want).is_ok(), "reinstall after cut {n}");
        assert!(Door::open(&mut f, &want).is_ok());
        assert_eq!(f.image(new.len()), &new[..]);
    }
    // Only the uncut install opens: the commit word is the last op.
    assert_eq!(opened, 1);
}

#[test]
fn power_cut_never_opens_wrong() {
    cut_loop(None);
}

#[test]
fn power_cut_over_another_image_never_opens_wrong() {
    let old = sealed(&dense(3 * SECTOR + 1024, 0xC3), SEAL_A);
    cut_loop(Some(&old));
}

#[test]
fn boot_action_table() {
    assert_eq!(boot_action(BANK2_BAD), BootAction::EraseS0ThenJudge);
    assert_eq!(boot_action(0), BootAction::Judge);
    assert_eq!(boot_action(0xFFFF_FFFF), BootAction::Judge);
    assert_eq!(boot_action(BANK2_BAD ^ 1), BootAction::Judge);
    assert_eq!(boot_action(BANK2_INSTALLING), BootAction::EraseS0NoInstall);
    assert_ne!(BANK2_BAD, BANK2_INSTALLING);
    for a in [BootAction::Judge, BootAction::EraseS0ThenJudge] {
        assert!(a.may_install(), "{a:?}");
    }
    assert!(!BootAction::EraseS0NoInstall.may_install());
    // Erased, bank 2 is never read that boot: Empty by construction.
    assert_eq!(
        BootAction::EraseS0ThenJudge.after_erase(),
        Some(Verdict::Empty)
    );
    assert_eq!(BootAction::EraseS0NoInstall.after_erase(), None);
}

/// A fault outside the boot's own install marks bank 2 bad; one inside it
/// keeps the install's mark, so the next boot erases S0 and installs
/// nothing: a cell that faults again costs one more reset, never a loop.
#[test]
fn a_fault_marks_by_what_ran() {
    for before in [0, 0xFFFF_FFFF, BANK2_BAD, 0x1234_5678] {
        assert_eq!(fault_mark(before), BANK2_BAD, "{before:08x}");
    }
    assert_eq!(fault_mark(BANK2_INSTALLING), BANK2_INSTALLING);
    for before in [0, BANK2_BAD, BANK2_INSTALLING] {
        assert_ne!(boot_action(fault_mark(before)), BootAction::Judge);
    }
}

#[test]
fn an_ecc_fault_is_bank2s_only_with_all_three() {
    const DBECC: u32 = 1 << 26;
    const SNECC: u32 = 1 << 25;
    const PRECISERR: u32 = 1 << 9;
    const IMPRECISERR: u32 = 1 << 10;
    const IBUSERR: u32 = 1 << 8;
    const MMARVALID: u32 = 1 << 7 | 1 << 1;
    const UNDEFINSTR: u32 = 1 << 16;
    // FA2R: the 256-bit word's index in bank 2; S0-S7 are words 0..32768.
    let last = (1 << 20) / 32 - 1;
    for cfsr in [PRECISERR, IMPRECISERR, IBUSERR, PRECISERR | 1 << 15] {
        for far in [0, 7, last] {
            assert!(bank2_ecc_fault(cfsr, DBECC, far), "{cfsr:x} {far}");
            assert!(bank2_ecc_fault(cfsr, DBECC | SNECC, far));
            assert!(!bank2_ecc_fault(cfsr, SNECC, far));
            assert!(!bank2_ecc_fault(cfsr, 0, far));
        }
        // Every index FAIL_ECC_ADDR2[14:0] holds is in bank 2.
        assert_eq!(last, 0x7fff);
        // Bits above FAIL_ECC_ADDR2[14:0] are reserved.
        assert!(bank2_ecc_fault(cfsr, DBECC, 0xffff_8000));
    }
    // Not a bus fault: an MPU or usage fault with a stale flag.
    for cfsr in [0, MMARVALID, UNDEFINSTR] {
        assert!(!bank2_ecc_fault(cfsr, DBECC, 0));
    }
}

#[test]
fn auto_install_table() {
    use AutoInstall::*;
    let h = header(&sealed(&dense(64, 0), SEAL_A));
    let good = Verdict::Good(h);
    let stale = Verdict::Stale { have: h.version };
    let all = [
        Verdict::Empty,
        Verdict::Torn,
        Verdict::Foreign,
        Verdict::Format(2),
        Verdict::Corrupt,
        Verdict::Unreadable,
        stale,
        good,
    ];
    for f in &all {
        let want = if *f == good { Skip } else { Install };
        assert_eq!(auto_install(f, &good), want, "{f:?}");
        assert_eq!(wants_staged(f), want == Install, "{f:?}");
        for s in &all[..7] {
            assert_eq!(auto_install(f, s), Skip, "{f:?} {s:?}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ev {
    Op(Op),
    Phase(Phase),
    Mark(Mark),
}

type Log = std::rc::Rc<std::cell::RefCell<Vec<Ev>>>;

/// SimFlash whose ops land in a log the progress shares.
struct Logged {
    f: SimFlash,
    log: Log,
}

impl Logged {
    fn note(&mut self) {
        let mut log = self.log.borrow_mut();
        log.extend(self.f.ops.drain(..).map(Ev::Op));
    }
}

impl ImageRead for Logged {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        self.f.read(off, buf)
    }
}

// SAFETY: test stand-in, not a chip.
unsafe impl Bank2Memory for Logged {}

// SAFETY: test stand-in, not a chip.
unsafe impl Flash for Logged {
    fn erase(&mut self, s: Bank2Sector) -> Result<(), FlashFail> {
        let r = self.f.erase(s);
        self.note();
        r
    }

    fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail> {
        let r = self.f.program(w, data);
        self.note();
        r
    }
}

struct Rec(Log);

impl Progress for Rec {
    fn phase(&mut self, p: Phase) {
        self.0.borrow_mut().push(Ev::Phase(p));
    }

    fn mark(&mut self, m: Mark) {
        self.0.borrow_mut().push(Ev::Mark(m));
    }
}

/// Phases and marks land between the ops at fixed points: Erasing before
/// the first erase, EraseStarted after S0's, Writing before the body,
/// HalfBody after the 512-byte block holding the body's middle word,
/// Sealing before the header words, BeforeCommit just before the commit.
#[test]
fn progress_lands_between_the_ops() {
    let body = dense(300 * 1024, 7);
    let img = sealed(&body, SEAL_A);
    let log = Log::default();
    let mut f = Logged {
        f: SimFlash::new(),
        log: log.clone(),
    };
    let mut buf = [0u8; 512];
    let mut st = Staged::new(img.clone());
    let p = slot_plan(&mut f.f);
    let r = install_with(
        &mut f,
        &mut st,
        &seal(SEAL_A),
        &mut buf,
        &mut Rec(log.clone()),
        &p,
    );
    assert_eq!(r, Ok(header(&img)));

    use Bank2Sector::*;
    let mut want = vec![
        Ev::Phase(Phase::Erasing),
        Ev::Op(Op::Erase(S0)),
        Ev::Mark(Mark::EraseStarted),
        Ev::Op(Op::Erase(S1)),
        Ev::Op(Op::Erase(S2)),
        Ev::Op(Op::Erase(S3)),
    ];
    for s in Slot::ALL {
        want.extend([s.marker_addr(), s.tag_addr(), s.base()].map(|a| Ev::Op(Op::Program(a))));
        want.push(Ev::Mark(Mark::SlotWritten(s)));
    }
    want.push(Ev::Phase(Phase::Writing));
    let len = body.len() as u32;
    let half_block_end = (IMAGE_BODY / 2 / 512 + 1) * 512;
    for off in (0..len).step_by(32) {
        want.push(Ev::Op(Op::Program(BANK2_BASE + HEADER_LEN + off)));
    }
    // The body's middle is in the 0xFF padding: no op after the mark.
    assert!(half_block_end > len);
    want.push(Ev::Mark(Mark::HalfBody));
    want.push(Ev::Phase(Phase::Sealing));
    want.extend((0..7).map(|i| Ev::Op(Op::Program(BANK2_BASE + 32 * i))));
    want.push(Ev::Mark(Mark::BeforeCommit));
    want.push(Ev::Op(Op::Program(BANK2_BASE + COMMIT_OFF)));
    assert_eq!(*log.borrow(), want);
}

/// An image whose body is 0xFF bar the six slot words and two words in
/// each of S4..S7, near its start and its end (clear of the reserves and
/// markers), so a torn erase there leaves old bytes as a reinstall's would.
/// An install is a handful of ops, so every pair of tears can be tried.
fn img(s: [u8; 8]) -> Vec<u8> {
    let mut body = vec![0xFFu8; IMAGE_BODY as usize];
    for sector in 4..8 {
        for off in [0x800, SECTOR - 0x40] {
            let o = sector * SECTOR + off - HEADER_LEN as usize;
            for (i, b) in body[o..o + 32].iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(13) ^ s[0] ^ sector as u8;
            }
        }
    }
    sealed(&body, s)
}

/// The staged file, borrowed.
struct Img<'a>(&'a [u8]);

impl ImageRead for Img<'_> {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let o = off as usize;
        let src = o.checked_add(buf.len()).and_then(|e| self.0.get(o..e));
        buf.copy_from_slice(src.ok_or(ReadFail)?);
        Ok(())
    }
}

/// An install planned from the guard's evidence; `Err` if the guard refused.
fn guarded(f: &mut SimFlash, img: &[u8], s: [u8; 8]) -> Result<Result<Header, InstallFail>, Slot> {
    let p = guard(f, SlotSet::NONE).map_err(|e| e.0)?;
    Ok(install_with(
        f,
        &mut Img(img),
        &seal(s),
        &mut [0u8; 512],
        &mut (),
        &p,
    ))
}

/// A unit that installed `s`'s image uncut.
fn installed(s: [u8; 8]) -> SimFlash {
    installed_with(&img(s))
}

/// A unit that installed `img` uncut.
fn installed_with(img: &[u8]) -> SimFlash {
    let mut f = SimFlash::new();
    assert!(guarded(&mut f, img, header(img).seal).unwrap().is_ok());
    f.ops.clear();
    f
}

/// A cut that tears op `n` of a guarded install of `img`, then power back.
fn cut(f: &mut SimFlash, n: usize, img: &[u8], seed: u32) {
    f.rng = Rng(seed | 1);
    f.cut_after(n);
    guarded(f, img, header(img).seal)
        .expect("slots ours before the cut")
        .ok();
    f.reboot();
}

/// After whatever came before, the guard reads both slots ours and an
/// install ends open with exactly the image.
fn ends_open(f: &mut SimFlash, img: &[u8], what: &str) {
    let s = header(img).seal;
    let r = guarded(f, img, s).unwrap_or_else(|s| panic!("{s:?} foreign: {what}"));
    assert!(r.is_ok(), "{what}: {r:?}");
    assert!(Door::open(f, &seal(s)).is_ok(), "{what}");
    assert_eq!(f.image(img.len()), img, "{what}");
    for s in Slot::ALL {
        assert_eq!(guard(f, SlotSet::NONE).unwrap().evidence(s), Evidence::Tag);
    }
}

/// Ops of an uncut install over `f`.
fn ops_in_full_install(f: &SimFlash, img: &[u8]) -> usize {
    full_install_ops(f, img).len()
}

/// The ops of an uncut install of `img` over `f`, in order.
fn full_install_ops(f: &SimFlash, img: &[u8]) -> Vec<Op> {
    let mut f = f.clone();
    f.ops.clear();
    assert!(guarded(&mut f, img, header(img).seal).unwrap().is_ok());
    f.ops
}

#[derive(Clone, Copy)]
enum K {
    E4,
    E5,
    Tag,
    Code,
    Marker,
}

fn slot_seq(s: Slot, ks: &[K]) -> Vec<PlanOp> {
    let (s4, s5) = s.sectors();
    let [code, tag, marker] =
        own_words(s).map(|(a, w)| PlanOp::Program(Bank2Word::new(a).unwrap(), w));
    ks.iter()
        .map(|k| match k {
            K::E4 => PlanOp::Erase(s4),
            K::E5 => PlanOp::Erase(s5),
            K::Tag => tag,
            K::Code => code,
            K::Marker => marker,
        })
        .collect()
}

/// The slots' part of the plan for `f`'s evidence.
fn slot_part(f: &mut SimFlash, ours: SlotSet) -> (SlotPlan, Vec<PlanOp>) {
    let p = guard(f, ours).unwrap();
    let ops = plan(&header(&img(SEAL_B)), &p).ops()[4..].to_vec();
    (p, ops)
}

fn both(ks: &[K]) -> Vec<PlanOp> {
    let mut v = slot_seq(Slot::Mixer, ks);
    v.extend(slot_seq(Slot::Edit, ks));
    v
}

/// Flags the word at `addr` as failing ECC.
fn flag(f: &mut SimFlash, addr: u32) {
    f.bad[((addr - BANK2_BASE) / 32) as usize] = true;
}

#[test]
fn plan_orders_the_slots() {
    use K::*;
    let (p, ops) = slot_part(&mut installed(SEAL_A), SlotSet::NONE);
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), Evidence::Tag);
    }
    assert_eq!(ops, both(&[E5, Marker, E4, Tag, Code]));
}

/// Both sectors erased: marker, tag, code and no erase. An erased sector is
/// never erased again.
#[test]
fn plan_skips_an_erased_end_sector() {
    use K::*;
    let (p, ops) = slot_part(&mut SimFlash::new(), SlotSet::NONE);
    let erased = Evidence::Scan {
        s4_erased: true,
        s5_erased: true,
    };
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), erased);
    }
    assert_eq!(ops, both(&[Marker, Tag, Code]));
}

#[test]
fn plan_goes_s4_first_after_a_marker() {
    use K::*;
    let mut f = installed(SEAL_A);
    f.erase(Bank2Sector::S4).unwrap();
    f.erase(Bank2Sector::S6).unwrap();
    let (p, ops) = slot_part(&mut f, SlotSet::NONE);
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), Evidence::Marker);
    }
    let s4_first = both(&[E4, Tag, Code, E5, Marker]);
    assert_eq!(ops, s4_first);
    // The boot's slot mark, unread: the same.
    let (p, ops) = slot_part(&mut SimFlash::new(), SlotSet::ALL);
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), Evidence::Mark);
    }
    assert_eq!(ops, s4_first);
    // A scan with neither sector erased: the same.
    let mut f = SimFlash::new();
    for s in Slot::ALL {
        flag(&mut f, s.tag_addr());
        flag(&mut f, s.marker_addr());
    }
    let (p, ops) = slot_part(&mut f, SlotSet::NONE);
    let neither = Evidence::Scan {
        s4_erased: false,
        s5_erased: false,
    };
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), neither);
    }
    assert_eq!(ops, s4_first);
}

#[test]
fn plan_writes_the_tag_first_onto_an_erased_s4() {
    use K::*;
    let mut f = SimFlash::new();
    for s in Slot::ALL {
        flag(&mut f, s.marker_addr());
    }
    let (p, ops) = slot_part(&mut f, SlotSet::NONE);
    let s4_erased = Evidence::Scan {
        s4_erased: true,
        s5_erased: false,
    };
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), s4_erased);
    }
    assert_eq!(ops, both(&[Tag, Code, E5, Marker]));
}

#[test]
fn plan_writes_the_marker_first_into_an_erased_s5() {
    use K::*;
    let mut f = SimFlash::new();
    for s in Slot::ALL {
        flag(&mut f, s.tag_addr());
    }
    let (p, ops) = slot_part(&mut f, SlotSet::NONE);
    let s5_erased = Evidence::Scan {
        s4_erased: false,
        s5_erased: true,
    };
    for s in Slot::ALL {
        assert_eq!(p.evidence(s), s5_erased);
    }
    assert_eq!(ops, both(&[Marker, E4, Tag, Code]));
}

/// SimFlash panics on a second program; each slot word is programmed once,
/// by the plan, on a first install and over an installed unit.
#[test]
fn the_body_never_reprograms_a_slot_word() {
    let b = img(SEAL_B);
    for mut f in [SimFlash::new(), installed(SEAL_A)] {
        assert!(guarded(&mut f, &b, SEAL_B).unwrap().is_ok());
        for (a, _) in Slot::ALL.into_iter().flat_map(own_words) {
            let n = f.ops.iter().filter(|o| **o == Op::Program(a)).count();
            assert_eq!(n, 1, "{a:#x}");
        }
        assert_eq!(f.image(b.len()), &b[..]);
    }
}

#[test]
fn a_reinstall_with_slots_already_ours_ends_open() {
    let b = img(SEAL_B);
    let mut f = installed(SEAL_A);
    let p = guard(&mut f, SlotSet::ALL).unwrap();
    let r = install_with(
        &mut f,
        &mut Img(&b),
        &seal(SEAL_B),
        &mut [0u8; 512],
        &mut (),
        &p,
    );
    assert!(r.is_ok(), "{r:?}");
    assert!(Door::open(&mut f, &seal(SEAL_B)).is_ok());
    for s in Slot::ALL {
        assert_eq!(
            guard(&mut f, SlotSet::NONE).unwrap().evidence(s),
            Evidence::Tag
        );
    }
}

#[test]
fn every_tear_leaves_both_slots_ours() {
    let b = img(SEAL_B);
    let starts = [SimFlash::new(), installed(SEAL_A)];
    for tear in TEARS {
        for start in &starts {
            for n in 0..ops_in_full_install(start, &b) {
                let mut f = start.clone();
                f.tear = tear;
                cut(&mut f, n, &b, 0x9E37_79B9 ^ n as u32);
                assert!(guard(&mut f, SlotSet::NONE).is_ok(), "cut {n} {tear:?}");
                ends_open(&mut f, &b, &format!("cut {n} {tear:?}"));
            }
        }
    }
}

/// One cut from a clean tag or marker leaves a clean tag or marker: never
/// a scan, so a second cut still has evidence to keep.
#[test]
fn a_cut_from_a_tag_or_marker_keeps_one() {
    let b = img(SEAL_B);
    let tag = installed(SEAL_A);
    let mut marker = tag.clone();
    marker.erase(Bank2Sector::S4).unwrap();
    marker.erase(Bank2Sector::S6).unwrap();
    for tear in TEARS {
        for start in [&tag, &marker] {
            for n in 0..ops_in_full_install(start, &b) {
                let mut f = start.clone();
                f.tear = tear;
                cut(&mut f, n, &b, 0x6A09_E667 ^ n as u32);
                let p = guard(&mut f, SlotSet::NONE).unwrap();
                for s in Slot::ALL {
                    let e = p.evidence(s);
                    assert!(
                        matches!(e, Evidence::Tag | Evidence::Marker),
                        "cut {n} {tear:?} {s:?}: {e:?}"
                    );
                }
            }
        }
    }
}

/// Install 1 torn at every op, then install 2 (planned from the guard's
/// evidence) torn at every op, under both tear models, from a first install
/// and from an installed unit: the guard reads both slots ours and a third
/// install ends open.
///
/// Not tested, and the one accepted lockout (spec § 3.1's residual): a
/// `Scan` with neither sector erased keeps no clean evidence. Reaching it
/// takes a torn first-install marker and then a torn tag program; a third
/// cut, tearing the next erase under model G, can read foreign. Every pair
/// here must pass.
#[test]
fn two_tears_on_consecutive_installs_leave_both_slots_ours() {
    let pairs = pair_matrix(&img(SEAL_B), installed(SEAL_A));
    eprintln!("{pairs} pairs");
}

/// The pair matrix over `b`, from a first install and from `old`; the
/// number of pairs tried.
fn pair_matrix(b: &[u8], old: SimFlash) -> usize {
    let starts = [SimFlash::new(), old];
    let mut pairs = 0;
    for tear in TEARS {
        for (i, start) in starts.iter().enumerate() {
            let n1s = ops_in_full_install(start, b);
            for n1 in 0..n1s {
                let mut once = start.clone();
                once.tear = tear;
                cut(&mut once, n1, b, 0xBB67_AE85 ^ n1 as u32);
                let n2s = ops_in_full_install(&once, b);
                for n2 in 0..n2s {
                    let mut f = once.clone();
                    let what = format!("{tear:?} start {i} cuts {n1} {n2}");
                    cut(&mut f, n2, b, 0x3C6E_F372 ^ (n1 << 8 | n2) as u32);
                    ends_open(&mut f, b, &what);
                    pairs += 1;
                }
            }
        }
    }
    pairs
}

/// `img`'s words and three more in each of S0..S7: past the header or the
/// reserve, mid-sector and just short of the sector's end (clear of the
/// end markers), so torn erases in S0..S3 leave old bytes too.
fn sparse_8_sector(s: [u8; 8]) -> Vec<u8> {
    let mut body = img(s)[HEADER_LEN as usize..].to_vec();
    for sector in 0..8 {
        for off in [0x440, SECTOR / 2, SECTOR - 0x80] {
            let o = sector * SECTOR + off - HEADER_LEN as usize;
            for (i, b) in body[o..o + 32].iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(29) ^ s[1] ^ (sector as u8) << 4;
            }
        }
    }
    sealed(&body, s)
}

/// The pair matrix again with body words in every sector, from an erased
/// bank and from a unit holding another such image. Tens of thousands of
/// 1 MB installs: `just tear-matrix`, before any PR touching chimera-image,
/// the slot layout or bank2.x.
#[test]
#[ignore = "minutes even in release: `just tear-matrix` runs it"]
fn two_tears_with_words_in_every_sector_leave_both_slots_ours() {
    let t = std::time::Instant::now();
    let pairs = pair_matrix(
        &sparse_8_sector(SEAL_B),
        installed_with(&sparse_8_sector(SEAL_A)),
    );
    eprintln!("{pairs} pairs in {:.1} s", t.elapsed().as_secs_f64());
}

/// The linked image (`CHIMERA_BK2`, or `just firmware-orbit`'s
/// target/chimera-orbit.bk2), and the same with words added through S4..S7
/// (its S6 group is still empty; S4 holds the console's ed), each cut once
/// at every erase, at each of the six slot words and at a sample of
/// S4..S7 body programs, under every tear model, from a first install and
/// over an installed unit: the guard reads both slots ours and a reinstall
/// ends open.
#[test]
#[ignore = "needs a linked .bk2: just check runs it after release-bin"]
fn single_cuts_through_the_linked_image_leave_both_slots_ours() {
    let path = std::env::var("CHIMERA_BK2").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../target/chimera-orbit.bk2").into()
    });
    let real = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(real.len(), LEN, "{path}");
    let h = header(&real);
    let mut body = real[HEADER_LEN as usize..].to_vec();
    for w in (0x8_0000..LEN).step_by(0x2000) {
        let o = w + 0x800 - HEADER_LEN as usize;
        body[o..o + 32].fill(w as u8 ^ 0x5A);
    }
    let high = sealed(&body, h.seal);
    let slot_words: Vec<u32> = Slot::ALL
        .into_iter()
        .flat_map(own_words)
        .map(|(a, _)| a)
        .collect();
    let t = std::time::Instant::now();
    let mut n_cuts = 0;
    for (name, b) in [("linked", &real), ("linked + S4..S7 words", &high)] {
        for (i, start) in [SimFlash::new(), installed(SEAL_A)].iter().enumerate() {
            let ops = full_install_ops(start, b);
            let body: Vec<usize> = (0..ops.len())
                .filter(|&n| matches!(ops[n], Op::Program(a) if a >= Slot::Mixer.base() && !slot_words.contains(&a)))
                .collect();
            let sample = body.iter().step_by(body.len().div_ceil(16).max(1)).copied();
            let cuts: Vec<usize> = (0..ops.len())
                .filter(|&n| match ops[n] {
                    Op::Erase(_) => true,
                    Op::Program(a) => slot_words.contains(&a),
                })
                .chain(sample)
                .collect();
            eprintln!(
                "{name}, start {i}: {} ops, {} S4..S7 body programs, {} cuts",
                ops.len(),
                body.len(),
                cuts.len()
            );
            for tear in TEARS {
                for &n in &cuts {
                    let mut f = start.clone();
                    f.tear = tear;
                    let what = format!("{name} start {i} cut {n} ({:?}) {tear:?}", ops[n]);
                    cut(&mut f, n, b, 0x510E_527F ^ n as u32);
                    assert!(guard(&mut f, SlotSet::NONE).is_ok(), "{what}");
                    ends_open(&mut f, b, &what);
                    n_cuts += 1;
                }
            }
        }
    }
    eprintln!("{n_cuts} cuts in {:.1} s", t.elapsed().as_secs_f64());
}
