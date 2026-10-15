use chimera_image::crc::Crc32;
use chimera_image::*;

const SECTOR: usize = 128 * 1024;
const LEN: usize = BANK2_LEN as usize;

/// Fixed-seed xorshift32: every run cuts the same way.
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

/// Bank 2 as flash behaves: erase to 0xFF, program once, and a power cut
/// that tears the op it lands on. A torn program only clears bits (1 -> 0)
/// and may leave the word failing ECC on read until it is erased.
struct SimFlash {
    mem: Vec<u8>,
    /// Words whose ECC a torn program broke: reads over them fault.
    bad: Vec<bool>,
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
        self.bad[words].fill(false);
        if whole {
            Ok(())
        } else {
            Err(FlashFail::Timeout)
        }
    }

    fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail> {
        if self.dead {
            return Err(FlashFail::Timeout);
        }
        let o = (w.get() - BANK2_BASE) as usize;
        assert!(o.is_multiple_of(32) && o + 32 <= LEN);
        if self.mem[o..o + 32].iter().any(|&b| b != 0xFF) {
            return Err(FlashFail::NotErased);
        }
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
        self.bad[o / 32] = self.rng.next() & 1 != 0;
        Err(FlashFail::Timeout)
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

fn sealed(body: &[u8], s: [u8; 8]) -> Vec<u8> {
    let mut c = Crc32::new();
    c.update(body);
    let h = Header {
        body_len: body.len() as u32,
        body_crc: c.finish(),
        door_off: 0x100,
        seal: s,
        version: Version::new("0.2.0 02e440e2"),
    };
    let mut v = h.to_bytes(true).to_vec();
    v.extend_from_slice(body);
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

fn run(flash: &mut SimFlash, img: &[u8], want: &Seal) -> Result<Header, InstallFail> {
    let mut buf = [0u8; 512];
    install(flash, &mut Staged::new(img.to_vec()), want, &mut buf)
}

#[test]
fn plan_spans_s0_first() {
    use Bank2Sector::*;
    let h = |n: u32| Header {
        body_len: n,
        ..header(&sealed(&dense(32, 0), SEAL_A))
    };
    assert_eq!(plan(&h(100 * 1024)).sectors, &[S0]);
    assert_eq!(plan(&h(128 * 1024 - 256)).sectors, &[S0]);
    assert_eq!(plan(&h(128 * 1024 - 224)).sectors, &[S0, S1]);
    assert_eq!(plan(&h(200 * 1024)).sectors, &[S0, S1]);
    assert_eq!(plan(&h(512 * 1024 - 256)).sectors, &[S0, S1, S2, S3]);
    // A header never parsed can't reach S4.
    assert_eq!(plan(&h(u32::MAX)).sectors, &[S0, S1, S2, S3]);
}

#[test]
fn sectors_are_bank_2() {
    use Bank2Sector::*;
    let a: Vec<u32> = [S0, S1, S2, S3].iter().map(|s| s.addr()).collect();
    assert_eq!(a, [0x0810_0000, 0x0812_0000, 0x0814_0000, 0x0816_0000]);
    assert_eq!([S0, S1, S2, S3].map(|s| s.index()), [0, 1, 2, 3]);
}

#[test]
fn bank2_word_refuses_outside() {
    for a in [
        0x0818_0000,
        0x080F_FFE0,
        0x0810_0010,
        0,
        0x0800_0000,
        u32::MAX,
    ] {
        assert!(Bank2Word::new(a).is_none(), "{a:#x}");
    }
    for a in [0x0810_0000, 0x0817_FFE0, 0x0812_0020] {
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
    s.fail_from = Some(1 + 16 + 5);
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
    assert_eq!(f.ops[..3], [Op::Erase(S0), Op::Erase(S1), Op::Erase(S2)]);
    assert_eq!(f.ops.last(), Some(&Op::Program(0x0810_00E0)));
    let progs: Vec<u32> = f.ops[3..]
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

/// A failed op names its sector or word: the first body word after S0.
#[test]
fn a_flash_failure_says_where() {
    let img = sealed(&dense(4096, 2), SEAL_A);
    let mut f = SimFlash::new();
    f.cut_after(1);
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
    // FA2R: the 256-bit word's index in bank 2; S0-S3 are words 0..16384.
    let last = BANK2_LEN / FLASH_WORD - 1;
    for cfsr in [PRECISERR, IMPRECISERR, IBUSERR, PRECISERR | 1 << 15] {
        for far in [0, 7, last] {
            assert!(bank2_ecc_fault(cfsr, DBECC, far), "{cfsr:x} {far}");
            assert!(bank2_ecc_fault(cfsr, DBECC | SNECC, far));
            assert!(!bank2_ecc_fault(cfsr, SNECC, far));
            assert!(!bank2_ecc_fault(cfsr, 0, far));
        }
        // S4-S7: the bootloader's, never mapped.
        assert!(!bank2_ecc_fault(cfsr, DBECC, last + 1));
        assert!(!bank2_ecc_fault(cfsr, DBECC, 0x7fff));
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
    let r = install_with(
        &mut f,
        &mut st,
        &seal(SEAL_A),
        &mut buf,
        &mut Rec(log.clone()),
    );
    assert_eq!(r, Ok(header(&img)));

    use Bank2Sector::*;
    let mut want = vec![
        Ev::Phase(Phase::Erasing),
        Ev::Op(Op::Erase(S0)),
        Ev::Mark(Mark::EraseStarted),
        Ev::Op(Op::Erase(S1)),
        Ev::Op(Op::Erase(S2)),
        Ev::Phase(Phase::Writing),
    ];
    let len = body.len() as u32;
    let half_block_end = (len / 2 / 512 + 1) * 512;
    for off in (0..len).step_by(32) {
        want.push(Ev::Op(Op::Program(BANK2_BASE + HEADER_LEN + off)));
        if off + 32 == half_block_end {
            want.push(Ev::Mark(Mark::HalfBody));
        }
    }
    want.push(Ev::Phase(Phase::Sealing));
    want.extend((0..7).map(|i| Ev::Op(Op::Program(BANK2_BASE + 32 * i))));
    want.push(Ev::Mark(Mark::BeforeCommit));
    want.push(Ev::Op(Op::Program(BANK2_BASE + COMMIT_OFF)));
    assert_eq!(*log.borrow(), want);
}

/// `install` is `install_with` and no progress: the same ops, the same
/// bytes.
#[test]
fn install_is_install_with_nothing() {
    let img = sealed(&sparse_3_sector(5), SEAL_A);
    let mut a = SimFlash::new();
    let mut b = SimFlash::new();
    assert!(run(&mut a, &img, &seal(SEAL_A)).is_ok());
    let mut st = Staged::new(img.clone());
    let r = install_with(&mut b, &mut st, &seal(SEAL_A), &mut [0u8; 512], &mut ());
    assert!(r.is_ok());
    assert_eq!(a.ops, b.ops);
    assert_eq!(a.mem, b.mem);
}
