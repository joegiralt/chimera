//! The boot's BKP14R marks over a series of boots (spec § 3.2, review B2,
//! N3): a fault in the guard or the install never loops the boot.

use chimera_image::crc::Crc32;
use chimera_image::slot::{SlotRead, Word, guard, own_words};
use chimera_image::*;

const LEN: usize = BANK2_LEN as usize;
const SEAL: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Erase(Bank2Sector),
    Program(u32),
}

/// Bank 2 with ECC-flagged words; `trap` faults the next erase or program
/// before it changes anything.
struct SimFlash {
    mem: Vec<u8>,
    bad: Vec<bool>,
    ops: Vec<Op>,
    trap: bool,
    tripped: bool,
}

impl SimFlash {
    fn new() -> Self {
        SimFlash {
            mem: vec![0xFF; LEN],
            bad: vec![false; LEN / 32],
            ops: Vec::new(),
            trap: false,
            tripped: false,
        }
    }

    /// A torn word at `addr`: some bits of `want` cleared, ECC flagged.
    fn torn(&mut self, addr: u32, want: &[u8; 32]) {
        let o = (addr - BANK2_BASE) as usize;
        for (m, w) in self.mem[o..o + 32].iter_mut().zip(want) {
            *m = w | 0x0F;
        }
        self.bad[o / 32] = true;
    }

    fn spring(&mut self) -> Result<(), FlashFail> {
        if self.trap {
            self.trap = false;
            self.tripped = true;
            return Err(FlashFail::Timeout);
        }
        Ok(())
    }
}

impl ImageRead for SimFlash {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let o = off as usize;
        let e = o + buf.len();
        if e > LEN || (e > o && self.bad[o / 32..=(e - 1) / 32].iter().any(|&b| b)) {
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
        self.spring()?;
        self.ops.push(Op::Erase(s));
        let base = (s.addr() - BANK2_BASE) as usize;
        self.mem[base..base + 128 * 1024].fill(0xFF);
        self.bad[base / 32..(base + 128 * 1024) / 32].fill(false);
        Ok(())
    }

    fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail> {
        self.spring()?;
        self.ops.push(Op::Program(w.get()));
        let o = (w.get() - BANK2_BASE) as usize;
        assert!(self.mem[o..o + 32].iter().all(|&b| b == 0xFF) && !self.bad[o / 32]);
        self.mem[o..o + 32].copy_from_slice(data);
        Ok(())
    }
}

impl SlotRead for SimFlash {
    fn word(&mut self, addr: u32) -> Word {
        let o = (addr - BANK2_BASE) as usize;
        Word {
            bytes: self.mem[o..o + 32].try_into().unwrap(),
            ecc: self.bad[o / 32],
            corrected: false,
        }
    }
}

struct Staged(Vec<u8>);

impl ImageRead for Staged {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let o = off as usize;
        let s = self.0.get(o..o + buf.len()).ok_or(ReadFail)?;
        buf.copy_from_slice(s);
        Ok(())
    }
}

/// A sealed image: a short dense body, 0xFF to the one size, slot words in.
fn image() -> Vec<u8> {
    let mut body: Vec<u8> = (0..4096).map(|i| (i as u8).wrapping_mul(7)).collect();
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
        seal: SEAL,
        version: Version::new("0.4.0 test"),
    };
    let mut v = h.to_bytes(true).to_vec();
    v.extend_from_slice(&body);
    v
}

/// Where a fault may land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    EraseS0,
    Judge,
    Guard(Slot),
    Install,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// A fault reset the chip, BKP14R as `fault_mark` left it.
    Reset,
    /// Installed and opened.
    Open,
    /// The guard refused: nothing written.
    Refused,
    /// `EraseS0NoInstall`: S0 erased, nothing installed.
    NoInstall,
    /// Flash already `Good`: nothing to install.
    Kept,
}

/// The guard's reads; the boot's slot mark goes in before each slot's
/// first read. A fault there resets: the rest of the guard is moot.
struct Guarded<'a> {
    flash: &'a mut SimFlash,
    bkp: &'a mut u32,
    faults: &'a mut dyn FnMut(Stage) -> bool,
    torn: SlotSet,
    reading: Option<Slot>,
    faulted: bool,
}

impl SlotRead for Guarded<'_> {
    fn word(&mut self, addr: u32) -> Word {
        let s = Slot::ALL
            .into_iter()
            .find(|s| (s.base()..s.base() + 0x4_0000).contains(&addr))
            .unwrap();
        if !self.faulted && self.reading != Some(s) {
            self.reading = Some(s);
            *self.bkp = guard_marks(self.torn, s);
            if (self.faults)(Stage::Guard(s)) {
                *self.bkp = fault_mark(*self.bkp);
                self.faulted = true;
            }
        }
        self.flash.word(addr)
    }
}

/// One boot, in the board's order: `boot_action` -> (erase S0 if asked) ->
/// judge -> `BANK2_INSTALLING` -> each slot's mark and reads ->
/// `BANK2_INSTALLING` -> install -> 0.
fn boot(flash: &mut SimFlash, bkp: &mut u32, faults: &mut dyn FnMut(Stage) -> bool) -> Outcome {
    flash.ops.clear();
    let seal = Seal::new(SEAL).unwrap();
    let action = boot_action(*bkp);
    let over = match action {
        BootAction::EraseS0ThenJudge | BootAction::EraseS0NoInstall => {
            if faults(Stage::EraseS0) {
                *bkp = fault_mark(*bkp);
                return Outcome::Reset;
            }
            flash.erase(Bank2Sector::S0).unwrap();
            *bkp = 0;
            match action.after_erase() {
                Some(v) => v,
                None => return Outcome::NoInstall,
            }
        }
        BootAction::Judge | BootAction::InstallSlotsOurs(_) => {
            if faults(Stage::Judge) {
                *bkp = fault_mark(*bkp);
                return Outcome::Reset;
            }
            judge(flash, &seal)
        }
    };
    assert!(action.may_install());
    if !wants_staged(&over) {
        *bkp = 0;
        return Outcome::Kept;
    }
    let torn = match action {
        BootAction::InstallSlotsOurs(t) => t,
        _ => SlotSet::NONE,
    };
    *bkp = BANK2_INSTALLING;
    let mut g = Guarded {
        flash,
        bkp,
        faults,
        torn,
        reading: None,
        faulted: false,
    };
    let plan = guard(&mut g, torn);
    if g.faulted {
        return Outcome::Reset;
    }
    let Guarded {
        flash, bkp, faults, ..
    } = g;
    let Ok(plan) = plan else {
        *bkp = 0;
        return Outcome::Refused;
    };
    *bkp = BANK2_INSTALLING;
    flash.trap = faults(Stage::Install);
    let mut buf = [0u8; 512];
    let r = install_with(flash, &mut Staged(image()), &seal, &mut buf, &mut (), &plan);
    if flash.tripped {
        flash.tripped = false;
        *bkp = fault_mark(*bkp);
        return Outcome::Reset;
    }
    r.expect("install");
    *bkp = 0;
    Outcome::Open
}

/// Slots each with a torn, ECC-flagged tag word, the rest erased.
fn torn_slots() -> SimFlash {
    let mut f = SimFlash::new();
    for s in Slot::ALL {
        let [_, (a, w), _] = own_words(s);
        f.torn(a, &w);
    }
    f
}

#[test]
fn slot_marks_are_their_own() {
    for b in 0..=3u32 {
        let m = SLOT_TORN | b;
        assert_ne!(m, BANK2_BAD);
        assert_ne!(m, BANK2_INSTALLING);
        let set = slot_mark_set(m).unwrap();
        assert_eq!(slot_mark(set), m);
        assert_eq!(fault_mark(m), m, "a fault keeps the slot mark");
        assert_eq!(boot_action(m), BootAction::InstallSlotsOurs(set));
        assert!(BootAction::InstallSlotsOurs(set).may_install());
        assert_eq!(BootAction::InstallSlotsOurs(set).after_erase(), None);
    }
    for m in [
        0,
        SLOT_TORN | 4,
        SLOT_TORN ^ 0x100,
        BANK2_BAD,
        BANK2_INSTALLING,
    ] {
        assert_eq!(slot_mark_set(m), None, "{m:08x}");
    }
    assert_eq!(
        guard_marks(SlotSet::NONE.with(Slot::Mixer), Slot::Edit),
        slot_mark(SlotSet::ALL)
    );
}

#[test]
fn a_fault_during_install_never_loops() {
    let mut f = SimFlash::new();
    let mut bkp = 0;
    let mut always = |s: Stage| s == Stage::Install;
    for n in 0..6 {
        let o = boot(&mut f, &mut bkp, &mut always);
        assert_ne!(bkp, BANK2_BAD, "boot {n}");
        if n % 2 == 0 {
            assert_eq!((o, bkp), (Outcome::Reset, BANK2_INSTALLING), "boot {n}");
        } else {
            assert_eq!((o, bkp), (Outcome::NoInstall, 0), "boot {n}");
            assert_eq!(f.ops, [Op::Erase(Bank2Sector::S0)], "boot {n}");
        }
    }
    // Once the fault stops, the next boot installs.
    assert_eq!(boot(&mut f, &mut bkp, &mut |_| false), Outcome::Open);
    assert_eq!(bkp, 0);
    assert_eq!(boot(&mut f, &mut bkp, &mut |_| false), Outcome::Kept);
}

#[test]
fn a_masked_guard_never_resets() {
    let mut f = torn_slots();
    let mut bkp = 0;
    assert_eq!(boot(&mut f, &mut bkp, &mut |_| false), Outcome::Open);
    assert_eq!(bkp, 0);
}

#[test]
fn one_torn_slot_unmasked_installs_on_boot_two() {
    let mut f = torn_slots();
    let mut bkp = 0;
    let mut seen = Vec::new();
    let mut once = true;
    let mut faults = |s: Stage| {
        seen.push(s);
        let hit = s == Stage::Guard(Slot::Mixer) && once;
        once &= !hit;
        hit
    };
    assert_eq!(boot(&mut f, &mut bkp, &mut faults), Outcome::Reset);
    assert_eq!(bkp, slot_mark(SlotSet::NONE.with(Slot::Mixer)));
    assert_eq!(boot(&mut f, &mut bkp, &mut faults), Outcome::Open);
    assert_eq!(bkp, 0);
    // Boot 2 never read MIXER: the mark took it as ours.
    let guarded: Vec<_> = seen
        .iter()
        .filter(|s| matches!(s, Stage::Guard(_)))
        .collect();
    assert_eq!(
        guarded,
        [&Stage::Guard(Slot::Mixer), &Stage::Guard(Slot::Edit)]
    );
}

/// N3: every guard read faults, forever. MIXER faults boot 1, EDIT boot 2;
/// the bits accumulate, and boot 3 reads no slot and opens.
#[test]
fn double_torn_installs_on_boot_three() {
    let mut f = torn_slots();
    let mut bkp = 0;
    let mut faults = |s: Stage| matches!(s, Stage::Guard(_));
    assert_eq!(boot(&mut f, &mut bkp, &mut faults), Outcome::Reset);
    assert_eq!(bkp, slot_mark(SlotSet::NONE.with(Slot::Mixer)));
    assert_eq!(boot(&mut f, &mut bkp, &mut faults), Outcome::Reset);
    assert_eq!(bkp, slot_mark(SlotSet::ALL));
    assert_eq!(boot(&mut f, &mut bkp, &mut faults), Outcome::Open);
    assert_eq!(bkp, 0);
    assert_eq!(boot(&mut f, &mut bkp, &mut faults), Outcome::Kept);
}

/// Review Focus 4: a foreign word in S5 with S4 erased is refused on every
/// boot, and no boot ever erases or programs.
#[test]
fn refused_boot_repeats_without_writing() {
    let mut f = SimFlash::new();
    let mid = (Slot::Mixer.marker_addr() - BANK2_BASE - 0x1_0000) as usize;
    f.mem[mid] = 0x42;
    let mut bkp = 0;
    for n in 0..5 {
        assert_eq!(
            boot(&mut f, &mut bkp, &mut |_| false),
            Outcome::Refused,
            "boot {n}"
        );
        assert_eq!(f.ops, [], "boot {n}");
        assert_eq!(bkp, 0, "boot {n}");
    }
}
