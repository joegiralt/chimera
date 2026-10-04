//! The command channel through real triple buffers (spec § 4.3): random
//! UI frames, runner blocks, ack reads, loads and stalls. The reference
//! numbers every seq with a `u32` that never wraps, and models the outbox
//! and the window on those numbers, so it shares no code with the channel.
#![cfg(feature = "orbit")]

use chimera_core::orbit::channel::{
    ArmTarget, Batch, Channel, Cmd, CmdLog, LOG, RingMask, SceneCmd, Seq, receive, receive_each,
};
use chimera_core::orbit::rng::Rng;
use chimera_core::orbit::runner::Capture;
use chimera_core::orbit::scene::SceneIdx;
use chimera_core::orbit::{MAX_SEGS, RingIdx};
use chimera_core::triple::{Reader, TripleBuffer, Writer};

const SEEDS: [u32; 4] = [0x0B17_0001, 0xC0FF_EE00, 0xDEAD_BEEF, 1];
const STEPS: u32 = 1_000_000;
const MAX_STALL: u32 = 50;
/// Step odds out of 100: a UI frame below FRAME, a runner block below
/// BLOCK, ack reads below ACKS, a load at ACKS, a stall above.
const FRAME: u32 = 30;
const BLOCK: u32 = 52;
const ACKS: u32 = 80;

/// What the UI publishes: `log` and `load` as the shell will. `epoch`
/// stands in for `AudioShared.epoch`, the runner's load detector. The
/// rest is the reference's: the publish's number and its full-width
/// head and load.
#[derive(Clone, Copy)]
struct Wire {
    log: CmdLog,
    load: Seq,
    epoch: u32,
    n: u32,
    head: u32,
    load_at: u32,
}

const WIRE: Wire = Wire {
    log: CmdLog::EMPTY,
    load: Seq::ZERO,
    epoch: 0,
    n: 0,
    head: 0,
    load_at: 0,
};

/// What the runner publishes back: `acked` as the view will, and, for the
/// reference, the load it was under and its full-width value.
#[derive(Clone, Copy)]
struct Ack {
    acked: Seq,
    epoch: u32,
    at: u32,
}

const ACK: Ack = Ack {
    acked: Seq::ZERO,
    epoch: 0,
    at: 0,
};

/// The state the commands since a load leave, by the spec's folds.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Fold {
    parity: bool,
    scene: Option<SceneCmd>,
    arm: Option<Capture>,
    rhythm: RingMask,
    notes: RingMask,
}

impl Fold {
    fn take(&mut self, b: Batch) {
        self.parity ^= b.flip;
        self.scene = b.scene.or(self.scene);
        self.arm = b.arm.or(self.arm);
        self.rhythm = self.rhythm.union(b.rhythm);
        self.notes = self.notes.union(b.notes);
    }
}

/// The UI side on full-width seqs: the outbox, the fixed flush order and
/// the window, written from the spec (§ 1.4, § 3).
#[derive(Default)]
struct Model {
    head: u32,
    acked: u32,
    load: u32,
    scene: Option<SceneCmd>,
    sent: bool,
    arm: Option<Capture>,
    rhythm: RingMask,
    notes: RingMask,
    flip: bool,
}

impl Model {
    /// Pushes what fits; returns the full-width seq and command of each.
    fn flush(&mut self, out: &mut Vec<(u32, Cmd)>) {
        let scene = match self.scene {
            Some(SceneCmd::Cue(n)) if !self.sent => Some(Cmd::Cue(n)),
            Some(SceneCmd::Keep(n)) if !self.sent => Some(Cmd::Keep(n)),
            _ => None,
        };
        let arm = self.arm.map(|a| match a {
            Capture::Off => Cmd::Disarm,
            Capture::Ring(r) => Cmd::Arm(ArmTarget::Ring(r)),
            Capture::Segment(s) => Cmd::Arm(ArmTarget::Segment(s)),
        });
        let edit = |m: RingMask, f: fn(RingMask) -> Cmd| (!m.is_empty()).then(|| f(m));
        let queue = [
            scene,
            arm,
            edit(self.rhythm, Cmd::EditRhythm),
            edit(self.notes, Cmd::EditNotes),
            self.flip.then_some(Cmd::TogglePlay),
        ];
        for (i, c) in queue.into_iter().enumerate() {
            let Some(c) = c else { continue };
            if self.head + 1 - self.acked > LOG as u32 {
                return;
            }
            self.head += 1;
            out.push((self.head, c));
            match i {
                0 => self.sent = true,
                1 => self.arm = None,
                2 => self.rhythm = RingMask::EMPTY,
                3 => self.notes = RingMask::EMPTY,
                _ => self.flip = false,
            }
        }
    }

    fn idle(&self) -> bool {
        self.acked == self.head
            && self.arm.is_none()
            && self.rhythm.is_empty()
            && self.notes.is_empty()
            && !self.flip
            && (self.scene.is_none() || self.sent)
    }

    fn loaded(&mut self) {
        self.head += 1;
        *self = Model {
            head: self.head,
            acked: self.head,
            load: self.head,
            ..Model::default()
        };
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Check {
    /// Every logged command once, in order, per load, against the model.
    Log,
    /// The runner's fold against a fold of the unfolded acts.
    Fold,
}

#[derive(Default)]
struct Stats {
    repeats: u64,
    skips: u64,
    stale_acks: u64,
    refused: u64,
    full: u64,
    loads: u64,
    checkpoints: u64,
}

struct Sim {
    rng: Rng,
    check: Check,
    ui: Channel,
    model: Model,
    epoch: u32,
    published: u32,
    up: Writer<Wire>,
    acks: Reader<Ack>,
    wire: Reader<Wire>,
    down: Writer<Ack>,
    applied: Seq,
    applied_at: u32,
    r_epoch: u32,
    r_seen: u32,
    r_fold: Fold,
    /// Every command the model logged: load, full-width seq, command.
    issued: Vec<(u32, u32, Cmd)>,
    /// Where each load's commands start in `issued`.
    starts: Vec<usize>,
    /// The index in `issued` the runner applies next.
    next: usize,
    want: Fold,
    stats: Stats,
}

fn mask(bits: u32) -> RingMask {
    RingIdx::ALL
        .into_iter()
        .filter(|r| bits & 1 << r.get() != 0)
        .fold(RingMask::EMPTY, RingMask::with)
}

impl Sim {
    fn new(seed: u32, check: Check) -> Sim {
        let (up, wire) = Box::leak(Box::new(TripleBuffer::new(WIRE, WIRE, WIRE))).split();
        let (down, acks) = Box::leak(Box::new(TripleBuffer::new(ACK, ACK, ACK))).split();
        Sim {
            rng: Rng::new(seed),
            check,
            ui: Channel::new(),
            model: Model::default(),
            epoch: 0,
            published: 0,
            up,
            acks,
            wire,
            down,
            applied: Seq::ZERO,
            applied_at: 0,
            r_epoch: 0,
            r_seen: 0,
            r_fold: Fold::default(),
            issued: Vec::new(),
            starts: vec![0],
            next: 0,
            want: Fold::default(),
            stats: Stats::default(),
        }
    }

    fn below(&mut self, n: u32) -> u32 {
        self.rng.below(n)
    }

    fn act(&mut self) {
        let m = &mut self.model;
        match self.rng.below(4) {
            0 => {
                self.ui.flip();
                m.flip ^= true;
                self.want.parity ^= true;
            }
            1 => {
                let n = SceneIdx::ALL[self.rng.below(8) as usize];
                let c = if self.rng.below(2) == 0 {
                    SceneCmd::Cue(n)
                } else {
                    SceneCmd::Keep(n)
                };
                self.ui.scene(c);
                (m.scene, m.sent) = (Some(c), false);
                self.want.scene = Some(c);
            }
            2 => {
                let t = match self.rng.below(4) {
                    0 => None,
                    1 => Some(ArmTarget::Ring(RingIdx::ALL[self.rng.below(5) as usize])),
                    _ => Some(ArmTarget::Segment(self.rng.below(MAX_SEGS as u32) as u8)),
                };
                self.ui.arm(t);
                let to = t.map_or(Capture::Off, Capture::from);
                m.arm = Some(to);
                self.want.arm = Some(to);
            }
            _ => {
                let (r, n) = (mask(self.rng.below(32)), mask(self.rng.below(32)));
                self.ui.edit(r, n);
                m.rhythm = m.rhythm.union(r);
                m.notes = m.notes.union(n);
                self.want.rhythm = self.want.rhythm.union(r);
                self.want.notes = self.want.notes.union(n);
            }
        }
    }

    fn publish(&mut self) {
        self.published += 1;
        let w = Wire {
            log: *self.ui.log(),
            load: self.ui.load(),
            epoch: self.epoch,
            n: self.published,
            head: self.model.head,
            load_at: self.model.load,
        };
        self.up.publish(|s| *s = w);
    }

    /// 0–6 acts, flush, publish.
    fn ui_frame(&mut self, acts: bool) {
        if acts {
            for _ in 0..self.below(7) {
                self.act();
            }
        }
        self.ui.flush();
        let mut out = Vec::new();
        self.model.flush(&mut out);
        let epoch = self.epoch;
        self.issued
            .extend(out.into_iter().map(|(s, c)| (epoch, s, c)));
        if self.check == Check::Log {
            assert_eq!(self.ui.log().head().get(), self.model.head as u8, "head");
            assert_eq!(self.ui.idle(), self.model.idle(), "idle");
        }
        if self.model.head + 1 - self.model.acked > LOG as u32 {
            self.stats.full += 1;
        }
        self.publish();
    }

    fn runner_block(&mut self) {
        let w = *self.wire.read();
        match w.n.cmp(&(self.r_seen + 1)) {
            core::cmp::Ordering::Less => self.stats.repeats += 1,
            core::cmp::Ordering::Greater => self.stats.skips += 1,
            core::cmp::Ordering::Equal => {}
        }
        self.r_seen = w.n;
        if w.epoch != self.r_epoch {
            self.r_epoch = w.epoch;
            (self.applied, self.applied_at) = (w.load, w.load_at);
            self.next = self.starts[w.epoch as usize];
            self.r_fold = Fold::default();
        }
        let gap = w.head - self.applied_at;
        assert!(gap as usize <= LOG, "a gap of {gap}");
        assert_eq!(w.log.head().since(self.applied), gap as u8, "the u8 gap");
        if self.check == Check::Log {
            let mut a = self.applied;
            let mut at = self.applied_at;
            let (issued, next, epoch) = (&self.issued, &mut self.next, w.epoch);
            receive_each(&w.log, &mut a, |c| {
                at += 1;
                assert_eq!(issued.get(*next), Some(&(epoch, at, c)), "command #{next}");
                *next += 1;
            });
            assert_eq!(at, w.head, "the whole gap");
        }
        let b = receive(&w.log, &mut self.applied);
        self.applied_at = w.head;
        assert_eq!(self.applied, w.log.head());
        self.r_fold.take(b);
        let ack = Ack {
            acked: self.applied,
            epoch: self.r_epoch,
            at: self.applied_at,
        };
        self.down.publish(|s| *s = ack);
    }

    fn ui_ack(&mut self) {
        let a = *self.acks.read();
        // The UI's u32 epoch gate, and the seq window that agrees with it.
        let fresh = a.epoch == self.epoch;
        #[cfg(debug_assertions)]
        let admits = self.ui.admits(a.acked);
        // The window is a debug check: a release build has only the gate.
        #[cfg(not(debug_assertions))]
        let admits = fresh;
        if fresh {
            self.ui.ack(a.acked);
            self.model.acked = a.at;
        } else {
            self.stats.stale_acks += 1;
        }
        if !admits {
            self.stats.refused += 1;
        }
        assert_eq!(admits, fresh, "the load window");
        if self.check == Check::Log {
            assert_eq!(self.ui.acked().get(), self.model.acked as u8, "acked");
        }
    }

    fn load(&mut self) {
        self.ui.loaded();
        self.model.loaded();
        self.epoch += 1;
        self.starts.push(self.issued.len());
        self.want = Fold::default();
        self.stats.loads += 1;
        self.publish();
    }

    /// With every command acked and the runner past the load, the two
    /// sides agree.
    fn agree(&mut self) {
        if !(self.model.idle() && self.r_epoch == self.epoch) {
            return;
        }
        self.stats.checkpoints += 1;
        match self.check {
            Check::Log => assert_eq!(self.next, self.issued.len(), "all applied"),
            Check::Fold => assert_eq!(self.r_fold, self.want),
        }
    }

    fn step(&mut self) {
        match self.below(100) {
            0..FRAME => self.ui_frame(true),
            FRAME..BLOCK => self.runner_block(),
            BLOCK..ACKS => {
                for _ in 0..self.below(3) {
                    self.ui_ack();
                }
            }
            ACKS => self.load(),
            _ => {
                let k = 1 + self.below(MAX_STALL);
                let frames = self.below(2) == 0;
                for _ in 0..k {
                    if frames {
                        self.ui_frame(true);
                    } else {
                        self.runner_block();
                    }
                }
            }
        }
        self.agree();
    }

    fn drain(&mut self) {
        for _ in 0..64 {
            if self.model.idle() && self.r_epoch == self.epoch {
                break;
            }
            self.ui_frame(false);
            self.runner_block();
            self.ui_ack();
        }
        assert!(self.model.idle() && self.r_epoch == self.epoch, "drained");
        assert!(self.ui.idle());
        self.agree();
    }
}

fn run(check: Check) {
    let mut all = Stats::default();
    for seed in SEEDS {
        let mut sim = Sim::new(seed, check);
        for _ in 0..STEPS {
            sim.step();
        }
        sim.drain();
        let wraps = sim.model.head / 256;
        assert!(
            wraps >= 1_000,
            "seed {seed:#x}: {wraps} wraps of the u8 seq"
        );
        let s = sim.stats;
        all.repeats += s.repeats;
        all.skips += s.skips;
        all.stale_acks += s.stale_acks;
        all.refused += s.refused;
        all.full += s.full;
        all.loads += s.loads;
        all.checkpoints += s.checkpoints;
    }
    // The harness reaches what the proofs lean on: skipped and repeated
    // publishes (TB c), a full window, pre-load views refused, and (per
    // seed, above) the u8 wrap.
    assert!(all.repeats > 0 && all.skips > 0, "TB(c) exercised");
    assert!(all.full > 0, "the window filled");
    assert!(
        all.stale_acks > 0 && all.refused > 0,
        "pre-load views refused"
    );
    assert!(all.checkpoints > 100_000, "{} checkpoints", all.checkpoints);
}

/// Also the wrap test: every seed wraps the u8 seq at least 1,000 times.
#[test]
fn log_applies_every_command_once() {
    run(Check::Log);
}

#[test]
fn outbox_folds_like_the_runner() {
    run(Check::Fold);
}
