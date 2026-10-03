//! ORBIT's command channel (ADR 0082): the UI's one-byte command log, its
//! outbox, and the runner's receive. Pure: the shells only publish the log
//! and read the ack through their triple buffers.
//!
//! Seqs are `u8` and every compare between two is within `LOG` of each
//! other, so the wrap never aliases (spec § 1.8).

use core::num::NonZeroU8;

use super::runner::Capture;
use super::scene::{SCENES, SceneIdx};
use super::{MAX_SEGS, RINGS, RingIdx};

/// Entries in the log. A power of two dividing 256, so seq `s`'s slot
/// `s & 7` stays continuous across the u8 wrap.
pub const LOG: usize = 8;
const SLOT: u8 = LOG as u8 - 1;

const _: () = assert!(LOG.is_power_of_two() && 256 % LOG == 0);
// The byte map below is laid out for these.
const _: () = assert!(SCENES == 8 && RINGS == 5 && MAX_SEGS == 8);
// `SceneCmd::of_byte` takes the slot from the low bits past `CUE_FIRST`.
const _: () = assert!(KEEP_FIRST - CUE_FIRST == SCENES as u8);

/// A position in the log. Wraps; compared only within `LOG` of another.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Seq(u8);

impl Seq {
    pub const ZERO: Seq = Seq(0);

    pub const fn next(self) -> Seq {
        Seq(self.0.wrapping_add(1))
    }

    /// How far `self` is past `older`, wrapping.
    pub const fn since(self, older: Seq) -> u8 {
        self.0.wrapping_sub(older.0)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    const fn slot(self) -> usize {
        (self.0 & SLOT) as usize
    }
}

/// A set of rings, a bit each.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct RingMask(u8);

impl RingMask {
    pub const EMPTY: RingMask = RingMask(0);

    pub const fn with(self, r: RingIdx) -> RingMask {
        RingMask(self.0 | 1 << r.get())
    }

    pub const fn contains(self, r: RingIdx) -> bool {
        self.0 & 1 << r.get() != 0
    }

    pub const fn union(self, o: RingMask) -> RingMask {
        RingMask(self.0 | o.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// What a capture can be armed into: `Capture` without `Off`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmTarget {
    Ring(RingIdx),
    /// A chord segment, 0..MAX_SEGS.
    Segment(u8),
}

impl From<ArmTarget> for Capture {
    fn from(t: ArmTarget) -> Capture {
        match t {
            ArmTarget::Ring(r) => Capture::Ring(r),
            ArmTarget::Segment(s) => Capture::Segment(s),
        }
    }
}

/// The scene commands: the outbox keeps the newest one until it lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneCmd {
    /// Land slot n fresh at the boundary: a switch, CLEAR or RELOAD.
    Cue(SceneIdx),
    /// Play `live` as slot n: SCENE turned back.
    Keep(SceneIdx),
}

impl SceneCmd {
    /// A `Cue` or `Keep` byte, by range: they sit `SCENES` apart, so the
    /// low bits past `CUE_FIRST` are the slot. `None` for any other.
    const fn of_byte(b: u8) -> Option<SceneCmd> {
        let n = SceneIdx::ALL[b.wrapping_sub(CUE_FIRST) as usize & (SCENES - 1)];
        match b {
            CUE_FIRST..=CUE_LAST => Some(SceneCmd::Cue(n)),
            KEEP_FIRST..=KEEP_LAST => Some(SceneCmd::Keep(n)),
            _ => None,
        }
    }
}

impl From<SceneCmd> for Cmd {
    fn from(s: SceneCmd) -> Cmd {
        match s {
            SceneCmd::Cue(n) => Cmd::Cue(n),
            SceneCmd::Keep(n) => Cmd::Keep(n),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    TogglePlay,
    Cue(SceneIdx),
    Keep(SceneIdx),
    Arm(ArmTarget),
    Disarm,
    /// These rings' hits were replaced in `live`. Never empty.
    EditRhythm(RingMask),
    /// These rings' notes and pool were replaced. Never empty.
    EditNotes(RingMask),
}

/// A command on the wire, one byte: 1 TogglePlay, 2..=9 Cue, 10..=17 Keep,
/// 18..=22 Arm ring, 23..=30 Arm segment, 31 Disarm, 32..=62 EditRhythm,
/// 63..=93 EditNotes. Never 0, so `Option<Cmd8>` is a byte too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cmd8(NonZeroU8);

// The byte map, range by range. A scene, ring or segment index is added
// to its range's first byte; an edit's mask 1..=31 to its range's byte
// before the first.
const TOGGLE_PLAY: u8 = 1;
const CUE_FIRST: u8 = 2;
const CUE_LAST: u8 = 9;
const KEEP_FIRST: u8 = 10;
const KEEP_LAST: u8 = 17;
const ARM_RING_FIRST: u8 = 18;
const ARM_RING_LAST: u8 = 22;
const ARM_SEG_FIRST: u8 = 23;
const ARM_SEG_LAST: u8 = 30;
const DISARM: u8 = 31;
const RHYTHM_FIRST: u8 = 32;
const RHYTHM_LAST: u8 = 62;
const NOTES_FIRST: u8 = 63;
const NOTES_LAST: u8 = 93;

// Each range follows the last with no gap, and holds what it encodes.
const _: () = assert!(
    CUE_FIRST == TOGGLE_PLAY + 1
        && CUE_LAST - CUE_FIRST + 1 == SCENES as u8
        && KEEP_FIRST == CUE_LAST + 1
        && KEEP_LAST - KEEP_FIRST + 1 == SCENES as u8
        && ARM_RING_FIRST == KEEP_LAST + 1
        && ARM_RING_LAST - ARM_RING_FIRST + 1 == RINGS as u8
        && ARM_SEG_FIRST == ARM_RING_LAST + 1
        && ARM_SEG_LAST - ARM_SEG_FIRST + 1 == MAX_SEGS as u8
        && DISARM == ARM_SEG_LAST + 1
        && RHYTHM_FIRST == DISARM + 1
        && RHYTHM_LAST - RHYTHM_FIRST + 1 == (1 << RINGS) - 1
        && NOTES_FIRST == RHYTHM_LAST + 1
        && NOTES_LAST - NOTES_FIRST + 1 == (1 << RINGS) - 1
);

impl From<Cmd> for Cmd8 {
    fn from(c: Cmd) -> Cmd8 {
        let b = match c {
            Cmd::TogglePlay => TOGGLE_PLAY,
            Cmd::Cue(n) => CUE_FIRST + n.get(),
            Cmd::Keep(n) => KEEP_FIRST + n.get(),
            Cmd::Arm(ArmTarget::Ring(r)) => ARM_RING_FIRST + r.get(),
            Cmd::Arm(ArmTarget::Segment(s)) => {
                debug_assert!((s as usize) < MAX_SEGS);
                ARM_SEG_FIRST + (s & 7)
            }
            Cmd::Disarm => DISARM,
            Cmd::EditRhythm(m) => {
                debug_assert!(!m.is_empty());
                RHYTHM_FIRST - 1 + m.0
            }
            Cmd::EditNotes(m) => {
                debug_assert!(!m.is_empty());
                NOTES_FIRST - 1 + m.0
            }
        };
        // Every arm above is at least 1.
        Cmd8(NonZeroU8::new(b).unwrap_or(NonZeroU8::MIN))
    }
}

/// The byte map read back as a `Cmd`: the reference `receive`'s in-place
/// decode is tested against. The runner never needs it.
#[cfg(any(test, feature = "test-support"))]
impl Cmd8 {
    pub const fn from_byte(b: u8) -> Option<Cmd8> {
        match NonZeroU8::new(b) {
            Some(n) if b <= NOTES_LAST => Some(Cmd8(n)),
            _ => None,
        }
    }

    /// Range compares, no table: a table costs flash.
    pub fn cmd(self) -> Cmd {
        let b = self.0.get();
        match b {
            TOGGLE_PLAY => Cmd::TogglePlay,
            CUE_FIRST..=CUE_LAST => Cmd::Cue(SceneIdx::ALL[(b - CUE_FIRST) as usize]),
            KEEP_FIRST..=KEEP_LAST => Cmd::Keep(SceneIdx::ALL[(b - KEEP_FIRST) as usize]),
            ARM_RING_FIRST..=ARM_RING_LAST => {
                Cmd::Arm(ArmTarget::Ring(RingIdx::ALL[(b - ARM_RING_FIRST) as usize]))
            }
            ARM_SEG_FIRST..=ARM_SEG_LAST => Cmd::Arm(ArmTarget::Segment(b - ARM_SEG_FIRST)),
            DISARM => Cmd::Disarm,
            RHYTHM_FIRST..=RHYTHM_LAST => Cmd::EditRhythm(RingMask(b - (RHYTHM_FIRST - 1))),
            // NOTES_FIRST..=NOTES_LAST: `from_byte` admits nothing else.
            _ => Cmd::EditNotes(RingMask(b - (NOTES_FIRST - 1))),
        }
    }
}

/// The log the UI publishes: seq `s` sits in `ring[s & 7]`, and an entry
/// carries no seq of its own. 0 is never a command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CmdLog {
    head: Seq,
    ring: [u8; LOG],
}

impl CmdLog {
    pub const EMPTY: CmdLog = CmdLog {
        head: Seq::ZERO,
        ring: [0; LOG],
    };

    pub const fn head(&self) -> Seq {
        self.head
    }

    pub fn push(&mut self, c: Cmd8) -> Seq {
        self.head = self.head.next();
        self.ring[self.head.slot()] = c.0.get();
        self.head
    }

    /// Takes `head + 1` for a load's `L`; no entry carries it.
    pub fn reserve(&mut self) -> Seq {
        self.head = self.head.next();
        self.head
    }
}

/// What one receive brings, folded: flips by parity, scene and arm
/// commands last-wins, edit masks by union. `Disarm` is `Capture::Off`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Batch {
    pub flip: bool,
    pub scene: Option<SceneCmd>,
    pub arm: Option<Capture>,
    pub rhythm: RingMask,
    pub notes: RingMask,
}

impl Batch {
    /// `receive`'s fold, one `Cmd` at a time: the reference it is tested
    /// against.
    #[cfg(test)]
    fn fold(&mut self, c: Cmd) {
        match c {
            Cmd::TogglePlay => self.flip = !self.flip,
            Cmd::Cue(n) => self.scene = Some(SceneCmd::Cue(n)),
            Cmd::Keep(n) => self.scene = Some(SceneCmd::Keep(n)),
            Cmd::Arm(t) => self.arm = Some(t.into()),
            Cmd::Disarm => self.arm = Some(Capture::Off),
            Cmd::EditRhythm(m) => self.rhythm = self.rhythm.union(m),
            Cmd::EditNotes(m) => self.notes = self.notes.union(m),
        }
    }
}

/// Hands `f` each command in `(applied, head]`, in order, and sets
/// `applied = head`. At most `LOG` entries, whatever the log says. The
/// runner calls `receive`; this is its per-command form, for tests.
#[cfg(any(test, feature = "test-support"))]
pub fn receive_each(log: &CmdLog, applied: &mut Seq, mut f: impl FnMut(Cmd)) {
    let n = log.head.since(*applied);
    debug_assert!(n as usize <= LOG, "a gap of {n} outruns the log");
    let mut s = *applied;
    for _ in 0..n.min(LOG as u8) {
        s = s.next();
        match Cmd8::from_byte(log.ring[s.slot()]) {
            Some(c) => f(c.cmd()),
            None => debug_assert!(false, "no command at seq {}", s.0),
        }
    }
    *applied = log.head;
}

/// The runner's side: folds the gap into one `Batch`, as `Batch::fold`
/// does `receive_each`'s commands, decoding each byte by its range in
/// place: no `Cmd` between costs flash.
pub fn receive(log: &CmdLog, applied: &mut Seq) -> Batch {
    let mut b = Batch::default();
    let n = log.head.since(*applied);
    debug_assert!(n as usize <= LOG, "a gap of {n} outruns the log");
    let mut s = *applied;
    for _ in 0..n.min(LOG as u8) {
        s = s.next();
        let c = log.ring[s.slot()];
        match c {
            TOGGLE_PLAY => b.flip = !b.flip,
            CUE_FIRST..=KEEP_LAST => b.scene = SceneCmd::of_byte(c),
            ARM_RING_FIRST..=ARM_RING_LAST => {
                b.arm = Some(Capture::Ring(
                    RingIdx::ALL[usize::from(c - ARM_RING_FIRST) % RINGS],
                ));
            }
            ARM_SEG_FIRST..=ARM_SEG_LAST => b.arm = Some(Capture::Segment(c - ARM_SEG_FIRST)),
            DISARM => b.arm = Some(Capture::Off),
            RHYTHM_FIRST..=RHYTHM_LAST => b.rhythm.0 |= c - (RHYTHM_FIRST - 1),
            NOTES_FIRST..=NOTES_LAST => b.notes.0 |= c - (NOTES_FIRST - 1),
            _ => debug_assert!(false, "no command at seq {}", s.0),
        }
    }
    *applied = log.head;
    b
}

/// The commands a frame issued that haven't reached the log: each class
/// folded into one. An empty mask is no edit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outbox {
    arm: Option<Cmd8>,
    rhythm: RingMask,
    notes: RingMask,
}

impl Outbox {
    const EMPTY: Outbox = Outbox {
        arm: None,
        rhythm: RingMask::EMPTY,
        notes: RingMask::EMPTY,
    };

    fn is_empty(&self) -> bool {
        *self == Outbox::EMPTY
    }
}

const FLIP: u8 = 1;
const SENT: u8 = 2;
const LOAD_PENDING: u8 = 4;

/// The UI's side: the log it publishes, what waits to enter it, and the
/// runner's ack.
#[derive(Clone, Copy, Debug)]
pub struct Channel {
    log: CmdLog,
    outbox: Outbox,
    /// The newest scene command, until it lands; `SENT` once in the log.
    scene: Option<Cmd8>,
    flags: u8,
    load: Seq,
    acked: Seq,
}

impl Default for Channel {
    fn default() -> Channel {
        Channel::new()
    }
}

impl Channel {
    pub const fn new() -> Channel {
        Channel {
            log: CmdLog::EMPTY,
            outbox: Outbox::EMPTY,
            scene: None,
            flags: 0,
            load: Seq::ZERO,
            acked: Seq::ZERO,
        }
    }

    /// B6. Two in a frame cancel.
    pub fn flip(&mut self) {
        self.flags ^= FLIP;
    }

    /// Replaces any scene command not yet landed.
    pub fn scene(&mut self, c: SceneCmd) {
        self.scene = Some(Cmd::from(c).into());
        self.flags &= !SENT;
    }

    /// `None` disarms. The byte is written directly, as `Cmd8::from`
    /// writes it.
    pub fn arm(&mut self, to: Option<ArmTarget>) {
        let b = match to {
            None => DISARM,
            Some(ArmTarget::Ring(r)) => ARM_RING_FIRST + r.get(),
            Some(ArmTarget::Segment(s)) => {
                debug_assert!((s as usize) < MAX_SEGS);
                ARM_SEG_FIRST + (s & 7)
            }
        };
        self.outbox.arm = NonZeroU8::new(b).map(Cmd8);
    }

    pub fn edit(&mut self, rhythm: RingMask, notes: RingMask) {
        self.outbox.rhythm = self.outbox.rhythm.union(rhythm);
        self.outbox.notes = self.outbox.notes.union(notes);
    }

    /// Moves the outbox into the log, in the fixed order scene, arm,
    /// rhythm, notes, flip, while the log has room past `acked`. What
    /// doesn't fit waits, folded, for a later frame.
    pub fn flush(&mut self) {
        if let Some(c) = self.scene
            && self.flags & SENT == 0
        {
            if !Self::push(&mut self.log, self.acked, c) {
                return;
            }
            self.flags |= SENT;
        }
        if let Some(c) = self.outbox.arm {
            if !Self::push(&mut self.log, self.acked, c) {
                return;
            }
            self.outbox.arm = None;
        }
        // An edit's byte is its mask past the class's base.
        for (mask, base) in [
            (&mut self.outbox.rhythm, RHYTHM_FIRST - 1),
            (&mut self.outbox.notes, NOTES_FIRST - 1),
        ] {
            if let Some(b) = NonZeroU8::new(mask.0) {
                if !Self::push(&mut self.log, self.acked, Cmd8(b.saturating_add(base))) {
                    return;
                }
                *mask = RingMask::EMPTY;
            }
        }
        if self.flags & FLIP != 0 && Self::push(&mut self.log, self.acked, Cmd::TogglePlay.into()) {
            self.flags &= !FLIP;
        }
    }

    /// Pushes `c` if its seq stays within `LOG` of `acked`.
    fn push(log: &mut CmdLog, acked: Seq, c: Cmd8) -> bool {
        let room = log.head.next().since(acked) as usize <= LOG;
        if room {
            log.push(c);
        }
        room
    }

    /// Takes a view's `acked`. While a load is pending, a view from before
    /// it is refused (`false`): only views with `acked` in `[L, L + LOG]`
    /// pass, and the first that does ends the check.
    pub fn ack(&mut self, acked: Seq) -> bool {
        if self.flags & LOAD_PENDING != 0 {
            if acked.since(self.load) as usize > LOG {
                return false;
            }
            self.flags &= !LOAD_PENDING;
        }
        self.acked = acked;
        true
    }

    /// The runner has applied every command issued.
    pub fn idle(&self) -> bool {
        self.acked == self.log.head
            && self.outbox.is_empty()
            && self.flags & FLIP == 0
            && (self.scene.is_none() || self.flags & SENT != 0)
    }

    /// The scene command in the log and not yet landed.
    pub fn awaiting(&self) -> Option<SceneCmd> {
        match self.scene {
            Some(c) if self.flags & SENT != 0 => SceneCmd::of_byte(c.0.get()),
            _ => None,
        }
    }

    /// `c` has landed. Only the command awaited lands: a newer one, sent
    /// or not, stays.
    pub fn landed(&mut self, c: SceneCmd) {
        if self.awaiting() == Some(c) {
            self.scene = None;
            self.flags &= !SENT;
        }
    }

    /// The newest scene command has landed: the UI's `live` is the base.
    pub fn synced(&self) -> bool {
        self.scene.is_none()
    }

    /// A project load: takes `L = head + 1` with no entry, so every
    /// earlier command is passed over, and voids what waits to be sent.
    /// The load publish then carries `L` as `load`.
    pub fn loaded(&mut self) -> Seq {
        let l = self.log.reserve();
        self.load = l;
        self.acked = l;
        self.flags = LOAD_PENDING;
        self.outbox = Outbox::EMPTY;
        self.scene = None;
        l
    }

    pub fn log(&self) -> &CmdLog {
        &self.log
    }

    pub fn load(&self) -> Seq {
        self.load
    }

    pub fn acked(&self) -> Seq {
        self.acked
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::orbit::RingIdx;
    use crate::orbit::runner::Capture;
    use crate::orbit::scene::SceneIdx;
    use core::mem::size_of;
    use std::vec;
    use std::vec::Vec;

    fn every_cmd() -> Vec<Cmd> {
        let mut all = Vec::new();
        all.push(Cmd::TogglePlay);
        all.extend(SceneIdx::ALL.map(Cmd::Cue));
        all.extend(SceneIdx::ALL.map(Cmd::Keep));
        all.extend(RingIdx::ALL.map(|r| Cmd::Arm(ArmTarget::Ring(r))));
        all.extend((0..8).map(|s| Cmd::Arm(ArmTarget::Segment(s))));
        all.push(Cmd::Disarm);
        all.extend((1..32).map(|m| Cmd::EditRhythm(RingMask(m))));
        all.extend((1..32).map(|m| Cmd::EditNotes(RingMask(m))));
        all
    }

    fn mask(m: u8) -> RingMask {
        RingMask(m)
    }

    /// Every command of the gap, in order, without moving `applied`.
    fn gap(log: &CmdLog, mut applied: Seq) -> Vec<(Seq, Cmd)> {
        let mut out = Vec::new();
        let mut s = applied;
        receive_each(log, &mut applied, |c| {
            s = s.next();
            out.push((s, c));
        });
        out
    }

    #[test]
    fn every_cmd_round_trips_through_one_byte() {
        let all = every_cmd();
        assert_eq!(all.len(), 93);
        let mut bytes: Vec<u8> = all.iter().map(|&c| Cmd8::from(c).0.get()).collect();
        for &c in &all {
            assert_eq!(Cmd8::from(c).cmd(), c);
        }
        bytes.sort_unstable();
        assert_eq!(bytes, (1..=93).collect::<Vec<u8>>());
    }

    #[test]
    fn zero_and_past_93_are_not_commands() {
        assert!(Cmd8::from_byte(0).is_none());
        for b in 94..=255 {
            assert!(Cmd8::from_byte(b).is_none(), "{b}");
        }
        for b in 1..=93 {
            assert_eq!(Cmd8::from_byte(b).map(|c| c.0.get()), Some(b));
        }
    }

    #[test]
    fn option_cmd8_is_one_byte() {
        assert_eq!(size_of::<Option<Cmd8>>(), 1);
        assert_eq!(size_of::<CmdLog>(), 9);
        assert_eq!(size_of::<Outbox>(), 3);
        assert_eq!(size_of::<Channel>(), 16);
        assert_eq!(core::mem::align_of::<Channel>(), 1);
    }

    #[test]
    fn a_full_window_holds_back_the_rest() {
        let mut ch = Channel::new();
        let acked = ch.acked();
        for i in 0..LOG as u8 {
            ch.edit(mask(1 << (i % 5)), RingMask::EMPTY);
            ch.flush();
        }
        assert_eq!(ch.log().head().since(acked) as usize, LOG);
        let head = ch.log().head();
        ch.edit(mask(1), RingMask::EMPTY);
        ch.flush();
        assert_eq!(
            ch.log().head(),
            head,
            "a 9th entry would overwrite an unread one"
        );
        assert!(!ch.idle());
        assert!(ch.ack(head));
        ch.flush();
        assert_eq!(ch.log().head(), head.next());
        assert_eq!(
            gap(ch.log(), head),
            [(head.next(), Cmd::EditRhythm(mask(1)))]
        );
    }

    #[test]
    fn two_flips_in_a_frame_cancel() {
        let mut ch = Channel::new();
        let head = ch.log().head();
        ch.flip();
        ch.flip();
        ch.flush();
        assert_eq!(ch.log().head(), head);
        assert!(ch.idle());
    }

    #[test]
    fn a_frame_flushes_in_the_fixed_order() {
        let mut ch = Channel::new();
        let s3 = SceneIdx::new(3).unwrap();
        let r2 = RingIdx::new(2).unwrap();
        let h = ch.log().head();
        ch.flip();
        ch.edit(RingMask::EMPTY, mask(0b100));
        ch.edit(mask(0b10), RingMask::EMPTY);
        ch.arm(Some(ArmTarget::Ring(r2)));
        ch.scene(SceneCmd::Cue(s3));
        ch.flush();
        let mut seq = h;
        let want: Vec<(Seq, Cmd)> = [
            Cmd::Cue(s3),
            Cmd::Arm(ArmTarget::Ring(r2)),
            Cmd::EditRhythm(mask(0b10)),
            Cmd::EditNotes(mask(0b100)),
            Cmd::TogglePlay,
        ]
        .into_iter()
        .map(|c| {
            seq = seq.next();
            (seq, c)
        })
        .collect();
        assert_eq!(gap(ch.log(), h), want);
        let mut applied = h;
        let b = receive(ch.log(), &mut applied);
        assert_eq!(applied, ch.log().head());
        assert_eq!(
            b,
            Batch {
                flip: true,
                scene: Some(SceneCmd::Cue(s3)),
                arm: Some(Capture::Ring(r2)),
                rhythm: mask(0b10),
                notes: mask(0b100),
            }
        );
        assert_eq!(ch.awaiting(), Some(SceneCmd::Cue(s3)));
    }

    #[test]
    fn load_gate_refuses_views_before_l() {
        let mut ch = Channel::new();
        for _ in 0..3 {
            ch.flip();
            ch.flush();
        }
        let l = ch.loaded();
        assert_eq!((ch.load(), ch.acked(), ch.log().head()), (l, l, l));
        assert!(ch.idle());
        assert!(!ch.ack(Seq(l.0.wrapping_sub(1))));
        assert!(!ch.ack(Seq(l.0.wrapping_sub(1 + LOG as u8))));
        assert!(
            !ch.ack(Seq(l.0.wrapping_add(1 + LOG as u8))),
            "past the window"
        );
        assert!(!ch.ack(Seq(l.0.wrapping_add(127))));
        assert_eq!(ch.acked(), l, "a refused view moves nothing");
        assert!(
            ch.ack(Seq(l.0.wrapping_add(LOG as u8))),
            "the window's top edge"
        );
        assert_eq!(ch.flags & LOAD_PENDING, 0);
        let l = ch.loaded();
        assert!(ch.ack(l));
        assert_eq!(ch.flags & LOAD_PENDING, 0);
        assert!(ch.ack(Seq(l.0.wrapping_sub(1))));
        assert!(ch.ack(Seq(l.0.wrapping_add(200))));
    }

    #[test]
    fn a_load_voids_the_outbox_and_the_scene() {
        let mut ch = Channel::new();
        ch.scene(SceneCmd::Keep(SceneIdx::new(1).unwrap()));
        ch.arm(Some(ArmTarget::Segment(2)));
        ch.edit(mask(1), mask(2));
        ch.flip();
        let l = ch.loaded();
        assert!(ch.synced() && ch.idle());
        ch.flush();
        assert_eq!(ch.log().head(), l);
    }

    #[test]
    fn a_scene_waits_until_landed() {
        let mut ch = Channel::new();
        let s = SceneCmd::Keep(SceneIdx::new(5).unwrap());
        ch.scene(s);
        assert_eq!(ch.awaiting(), None, "not sent yet");
        assert!(!ch.idle() && !ch.synced());
        ch.flush();
        assert_eq!(ch.awaiting(), Some(s));
        let head = ch.log().head();
        ch.flush();
        assert_eq!(ch.log().head(), head, "a sent scene is not sent again");
        assert!(ch.ack(head));
        assert!(ch.idle() && !ch.synced());
        ch.landed(SceneCmd::Cue(SceneIdx::new(5).unwrap()));
        assert_eq!(
            ch.awaiting(),
            Some(s),
            "another command's landing lands nothing"
        );
        ch.landed(s);
        assert!(ch.synced() && ch.awaiting().is_none());
    }

    #[test]
    fn a_landing_keeps_a_newer_scene_command() {
        let mut ch = Channel::new();
        let (a, b) = (
            SceneCmd::Cue(SceneIdx::new(1).unwrap()),
            SceneCmd::Cue(SceneIdx::new(2).unwrap()),
        );
        ch.scene(a);
        ch.flush();
        ch.scene(b);
        assert_eq!(ch.awaiting(), None, "b is not sent yet");
        ch.landed(a);
        assert!(!ch.synced(), "b survives a's landing");
        let h = ch.log().head();
        ch.flush();
        assert_eq!(
            gap(ch.log(), h),
            [(h.next(), Cmd::Cue(SceneIdx::new(2).unwrap()))]
        );
        ch.landed(b);
        assert!(ch.synced());
    }

    /// The gate's alias bound. Each load with no view accepted moves `L`
    /// at most `LOG + 1` past the runner's last ack, so a pre-load view
    /// is refused for 27 back-to-back loads and aliases at 28 (9 × 28 =
    /// 252). The shell's u32 load epoch in the view closes that.
    #[test]
    fn back_to_back_loads_refuse_the_old_view_until_the_alias() {
        let mut ch = Channel::new();
        let old = ch.acked();
        for k in 1..=28 {
            for _ in 0..LOG {
                ch.flip();
                ch.flush();
            }
            assert_eq!(ch.log().head().since(ch.acked()) as usize, LOG);
            let l = ch.loaded();
            assert_eq!(l.since(old) as usize, (LOG + 1) * k % 256);
            assert_eq!(ch.ack(old), k == 28, "{k} loads");
        }
    }

    /// `receive`'s in-place decode folds every byte as `fold` folds its
    /// `Cmd`: each command alone, then random runs of eight.
    #[test]
    fn receive_folds_like_each() {
        let all = every_cmd();
        let mut x = 0x9E37_79B9u32;
        let mut pick = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            all[x as usize % all.len()]
        };
        let runs = all
            .iter()
            .map(|&c| vec![c])
            .chain((0..4000).map(|_| (0..LOG).map(|_| pick()).collect()));
        for run in runs {
            let mut log = CmdLog::EMPTY;
            for &c in &run {
                log.push(c.into());
            }
            let (mut a, mut b) = (Seq::ZERO, Seq::ZERO);
            let mut want = Batch::default();
            receive_each(&log, &mut a, |c| want.fold(c));
            assert_eq!(receive(&log, &mut b), want, "{run:?}");
            assert_eq!(a, b);
        }
    }

    #[test]
    fn arm_off_is_disarm() {
        let mut ch = Channel::new();
        let t = ArmTarget::Segment(7);
        ch.arm(Some(t));
        let h = ch.log().head();
        ch.flush();
        assert_eq!(gap(ch.log(), h), [(h.next(), Cmd::Arm(t))]);
        ch.arm(None);
        ch.flush();
        assert_eq!(gap(ch.log(), h.next()), [(h.next().next(), Cmd::Disarm)]);
    }
}
