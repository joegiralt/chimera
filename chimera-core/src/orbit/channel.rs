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

impl ArmTarget {
    /// `None` for `Off`, and for a segment past `MAX_SEGS`.
    pub const fn of(c: Capture) -> Option<ArmTarget> {
        match c {
            Capture::Off => None,
            Capture::Ring(r) => Some(ArmTarget::Ring(r)),
            Capture::Segment(s) if (s as usize) < MAX_SEGS => Some(ArmTarget::Segment(s)),
            Capture::Segment(_) => None,
        }
    }
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

const CUE: u8 = 2;
const KEEP: u8 = 10;
const ARM_RING: u8 = 18;
const ARM_SEG: u8 = 23;
const DISARM: u8 = 31;
const RHYTHM: u8 = DISARM; // + a mask of 1..=31
const NOTES: u8 = 62; // + a mask of 1..=31
const LAST: u8 = 93;

impl From<Cmd> for Cmd8 {
    fn from(c: Cmd) -> Cmd8 {
        let b = match c {
            Cmd::TogglePlay => 1,
            Cmd::Cue(n) => CUE + n.get(),
            Cmd::Keep(n) => KEEP + n.get(),
            Cmd::Arm(ArmTarget::Ring(r)) => ARM_RING + r.get(),
            Cmd::Arm(ArmTarget::Segment(s)) => {
                debug_assert!((s as usize) < MAX_SEGS);
                ARM_SEG + (s & 7)
            }
            Cmd::Disarm => DISARM,
            Cmd::EditRhythm(m) => {
                debug_assert!(!m.is_empty());
                RHYTHM + m.0
            }
            Cmd::EditNotes(m) => {
                debug_assert!(!m.is_empty());
                NOTES + m.0
            }
        };
        // Every arm above is at least 1.
        Cmd8(NonZeroU8::new(b).unwrap_or(NonZeroU8::MIN))
    }
}

impl Cmd8 {
    pub const fn from_byte(b: u8) -> Option<Cmd8> {
        match NonZeroU8::new(b) {
            Some(n) if b <= LAST => Some(Cmd8(n)),
            _ => None,
        }
    }

    /// Range compares, no table: a table costs flash.
    pub fn cmd(self) -> Cmd {
        let b = self.0.get();
        match b {
            1 => Cmd::TogglePlay,
            CUE..KEEP => Cmd::Cue(SceneIdx::ALL[(b - CUE) as usize]),
            KEEP..ARM_RING => Cmd::Keep(SceneIdx::ALL[(b - KEEP) as usize]),
            ARM_RING..ARM_SEG => Cmd::Arm(ArmTarget::Ring(RingIdx::ALL[(b - ARM_RING) as usize])),
            ARM_SEG..DISARM => Cmd::Arm(ArmTarget::Segment(b - ARM_SEG)),
            DISARM => Cmd::Disarm,
            32..=NOTES => Cmd::EditRhythm(RingMask(b - RHYTHM)),
            _ => Cmd::EditNotes(RingMask(b - NOTES)),
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
/// `applied = head`. At most `LOG` entries, whatever the log says.
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

/// The runner's side: folds the gap into one `Batch`.
pub fn receive(log: &CmdLog, applied: &mut Seq) -> Batch {
    let mut b = Batch::default();
    receive_each(log, applied, |c| b.fold(c));
    b
}

/// The commands a frame issued that haven't reached the log: each class
/// folded into one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outbox {
    arm: Option<Cmd8>,
    rhythm: Option<Cmd8>,
    notes: Option<Cmd8>,
}

impl Outbox {
    const EMPTY: Outbox = Outbox {
        arm: None,
        rhythm: None,
        notes: None,
    };

    fn is_empty(&self) -> bool {
        *self == Outbox::EMPTY
    }
}

const FLIP: u8 = 1;
const SENT: u8 = 2;
const LOAD_PENDING: u8 = 4;

/// One edit class's outbox slot after `m` joins it.
fn unite(slot: Option<Cmd8>, m: RingMask, edit: fn(RingMask) -> Cmd) -> Option<Cmd8> {
    let old = match slot.map(Cmd8::cmd) {
        Some(Cmd::EditRhythm(o) | Cmd::EditNotes(o)) => o,
        _ => RingMask::EMPTY,
    };
    let m = old.union(m);
    if m.is_empty() {
        None
    } else {
        Some(edit(m).into())
    }
}

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

    /// `Off` (or a segment past `MAX_SEGS`) disarms.
    pub fn arm(&mut self, to: Capture) {
        debug_assert!(to == Capture::Off || ArmTarget::of(to).is_some());
        self.outbox.arm = Some(ArmTarget::of(to).map_or(Cmd::Disarm, Cmd::Arm).into());
    }

    pub fn edit(&mut self, rhythm: RingMask, notes: RingMask) {
        self.outbox.rhythm = unite(self.outbox.rhythm, rhythm, Cmd::EditRhythm);
        self.outbox.notes = unite(self.outbox.notes, notes, Cmd::EditNotes);
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
        for slot in [
            &mut self.outbox.arm,
            &mut self.outbox.rhythm,
            &mut self.outbox.notes,
        ] {
            if let Some(c) = *slot {
                if !Self::push(&mut self.log, self.acked, c) {
                    return;
                }
                *slot = None;
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
        if self.flags & SENT == 0 {
            return None;
        }
        match self.scene.map(Cmd8::cmd) {
            Some(Cmd::Cue(n)) => Some(SceneCmd::Cue(n)),
            Some(Cmd::Keep(n)) => Some(SceneCmd::Keep(n)),
            _ => None,
        }
    }

    pub fn landed(&mut self) {
        self.scene = None;
        self.flags &= !SENT;
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
        ch.arm(Capture::Ring(r2));
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
        assert_eq!(ch.acked(), l, "a refused view moves nothing");
        assert!(ch.ack(l));
        assert_eq!(ch.flags & LOAD_PENDING, 0);
        assert!(ch.ack(Seq(l.0.wrapping_sub(1))));
        assert!(ch.ack(Seq(l.0.wrapping_add(200))));
    }

    #[test]
    fn a_load_voids_the_outbox_and_the_scene() {
        let mut ch = Channel::new();
        ch.scene(SceneCmd::Keep(SceneIdx::new(1).unwrap()));
        ch.arm(Capture::Segment(2));
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
        ch.landed();
        assert!(ch.synced() && ch.awaiting().is_none());
    }

    #[test]
    fn arm_off_is_disarm() {
        let mut ch = Channel::new();
        ch.arm(Capture::Segment(7));
        ch.arm(Capture::Off);
        let h = ch.log().head();
        ch.flush();
        assert_eq!(gap(ch.log(), h), [(h.next(), Cmd::Disarm)]);
        assert_eq!(ArmTarget::of(Capture::Off), None);
        assert_eq!(ArmTarget::of(Capture::Segment(8)), None);
        let t = ArmTarget::Segment(7);
        assert_eq!(ArmTarget::of(t.into()), Some(t));
    }
}
