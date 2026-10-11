//! ORBIT's QA rig (`orbit_qa_test.rs`): a NEW project, the real
//! `Instrument`, `FxBus` and `DacBlocks`, and an `OrbitRunner`, driven in
//! `Engine::block`'s order (the gate, then inside it the live keys and the
//! runner, then the render). Every block it checks the stuck-note
//! invariants and marks where sound can break discontinuously, for
//! `Tape::click` to judge.
//!
//! The UI side is idealised: the flush, the publish and `sync` run every
//! block, in place and with no latency, where the desktop runs them once a
//! frame (12–25 blocks) through triple buffers. A stale slot or a frame's
//! delay is not this rig's to catch.

use chimera_core::addr::BlockRef;
use chimera_core::block::Block;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{CPU_HZ_REV_V, DAC_PAIRS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument, NoteOrigin};
use chimera_core::note_queue::{InEvent, NoteEvent, NoteKind, RealTime};
use chimera_core::orbit::clock::SyncMode;
use chimera_core::orbit::engine::{OrbitEvent, PendingOff};
use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
use chimera_core::orbit::scene::OrbitSong;
use chimera_core::orbit::{RINGS, RingIdx};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{LoadGate, LoadLink, PartId, Project};
use chimera_core::scope::ScopeWriter;
use chimera_core::ui::orbit::OrbitUi;
use chimera_core::voice_alloc::VoiceSlot;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

use super::sweep::{
    BPS, DC_FROM, LEAK_PEAK, PRE_BLOCKS, Tape, steady_windows, transient_window, within_ceiling,
};
use super::{SR, scope_writer};

/// Blocks of steady state a mark needs on one side to be judged.
pub const STEADY: usize = 8;

/// A block where sound can break: never a note-on (an attack is legitimate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Ring `r`'s pending off went out.
    RingOff(u8),
    /// A sounding voice was taken for another note, or shed.
    Steal,
    Stop,
    /// MUTATE changed ring `r` at the loop boundary.
    Mutate(u8),
    Bpm,
}

impl Mark {
    pub const KINDS: [&'static str; 5] = ["off", "steal", "STOP", "MUTATE", "BPM"];

    pub fn kind(self) -> usize {
        match self {
            Mark::RingOff(_) => 0,
            Mark::Steal => 1,
            Mark::Stop => 2,
            Mark::Mutate(_) => 3,
            Mark::Bpm => 4,
        }
    }

    pub fn label(self) -> String {
        match self {
            Mark::RingOff(r) => format!("ring {} off", r + 1),
            Mark::Steal => "steal".into(),
            Mark::Stop => "STOP".into(),
            Mark::Mutate(r) => format!("MUTATE ring {}", r + 1),
            Mark::Bpm => "BPM jump".into(),
        }
    }
}

/// The ORBIT block of `song` the UI edits as `b`.
pub fn orbit_block(song: &mut OrbitSong, b: BlockRef) -> &mut dyn Block {
    match b {
        BlockRef::OrbitChords => &mut song.live_mut_for_test().chords,
        BlockRef::OrbitRing(r) => &mut song.live_mut_for_test().rings[r.get() as usize],
        BlockRef::Tempo => &mut song.bpm,
        _ => panic!("{b:?} is not ORBIT's"),
    }
}

pub struct OrbitRig {
    pub project: Box<Project>,
    pub ui: OrbitUi,
    pub shared: Box<AudioShared>,
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    gate: LoadGate,
    link: LoadLink,
    runner: Box<OrbitRunner>,
    pub tape: Tape,
    /// Each block's sum of the pairs, as the desktop's speakers mix them.
    pub sum: Vec<f32>,
    /// Samples of the sum past full scale: the desktop's clamp.
    pub clamped: usize,
    pub marks: Vec<(usize, Mark)>,
    /// Blocks holding an ORBIT note-on.
    pub ons: Vec<usize>,
    /// Stuck-note invariants broken: `(block, what)`.
    pub stuck: Vec<(usize, String)>,
    /// The block STOP was pressed at; `None` while playing.
    pub stopped_at: Option<usize>,
    /// Live keys held, by note; once released, no Live voice may stay held.
    live_released: bool,
    live: Vec<u8>,
    /// The next block's drain, as the MIDI queues hold it: `(source, event)`.
    keys: Vec<(usize, InEvent)>,
    /// What the runner sent the last block.
    pub sent: Vec<OrbitEvent>,
    slots: Vec<VoiceSlot>,
    last_view: OrbitView,
    /// MIDI CONFIG's SYNC, as SYSTEM holds it.
    pub sync: SyncMode,
}

impl OrbitRig {
    /// NEW, as `+ CREATE NEW` or an empty card makes it.
    pub fn new() -> Self {
        let (project, _) = Project::boxed();
        let shared = Box::new(AudioShared::from_performance(project.perf()));
        let runner = Box::new(OrbitRunner::new(
            core::num::NonZeroU32::new(SR).expect("a rate"),
        ));
        let last_view = runner.view();
        let inst = Box::new(Instrument::new(SR, SampleBudget::for_cpu(CPU_HZ_REV_V)));
        let slots = inst.allocator().slots().to_vec();
        OrbitRig {
            project,
            ui: OrbitUi::new(),
            shared,
            inst,
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: scope_writer(),
            gate: LoadGate::new(),
            link: LoadLink::new(),
            runner,
            tape: Tape::default(),
            sum: Vec::new(),
            clamped: 0,
            marks: Vec::new(),
            ons: Vec::new(),
            stuck: Vec::new(),
            stopped_at: None,
            live_released: false,
            live: Vec::new(),
            keys: Vec::new(),
            sent: Vec::new(),
            slots,
            last_view,
            sync: SyncMode::DEFAULT,
        }
    }

    /// NEW with every Part on INIT Algo, as NEW was before its factory
    /// Sounds: a sine, for the checks set by it.
    pub fn on_init() -> Self {
        let mut r = Self::new();
        for p in PartId::ALL {
            **r.project.edit_part(p).sound = Sound::init(EngineType::Algo);
        }
        r
    }

    pub fn song(&mut self) -> &mut OrbitSong {
        self.project.orbit_mut()
    }

    /// B6, pressed if ORBIT isn't already `on` as last seen.
    pub fn play(&mut self, on: bool) {
        if on == self.last_view.running {
            return;
        }
        self.stopped_at = (!on).then(|| self.blocks());
        self.ui.play_stop();
    }

    pub fn blocks(&self) -> usize {
        self.tape.busy.len()
    }

    pub fn view(&self) -> &OrbitView {
        &self.last_view
    }

    pub fn inst(&self) -> &Instrument {
        &self.inst
    }

    /// A key on Part `p`'s channel, as a keyboard plays it: queued for the
    /// next block, which drains it inside the load gate.
    pub fn key(&mut self, p: usize, note: u8, on: bool) {
        let channel = self.project.part(PartId::ALL[p]).mix.channel;
        let ev = NoteEvent {
            channel,
            note: MidiNote::new(note).expect("a note"),
            kind: if on {
                NoteKind::On(Velocity::DEFAULT)
            } else {
                NoteKind::Off
            },
        };
        self.keys.push((0, InEvent::Note(ev)));
        if on {
            self.live.push(note);
            self.live_released = false;
        } else {
            self.live.retain(|&n| n != note);
            self.live_released = self.live.is_empty();
        }
    }

    /// A MIDI real-time message from source `src`, for the next block's
    /// drain.
    pub fn rt(&mut self, src: usize, rt: RealTime) {
        self.keys.push((src, InEvent::Rt(rt)));
    }

    /// A project load as the shells run it: `loaded` before the load
    /// publish, which the next block carries.
    pub fn load(&mut self) {
        let _swap = self.link.bump_for_test();
        self.ui.loaded(self.link.epoch(), self.project.orbit_mut());
    }

    /// Each ring's off still to send.
    /// The runner's playing scene.
    pub fn working(&self) -> &chimera_core::orbit::scene::Scene {
        self.runner.working()
    }

    pub fn runner_pending(&self) -> [Option<PendingOff>; RINGS] {
        self.runner.pending()
    }

    /// Stuck-note invariants broken so far.
    pub fn stuck_notes(&self) -> &[(usize, String)] {
        &self.stuck
    }

    /// One block: the publish, `Engine::block`, the view absorbed, the
    /// tape and the checks.
    pub fn block(&mut self) {
        self.step(true);
    }

    /// `blocks` blocks while the UI reads the card: the audio runs on and
    /// the UI takes no view.
    pub fn card_read(&mut self, blocks: usize) {
        for _ in 0..blocks {
            self.step(false);
        }
    }

    fn step(&mut self, sync: bool) {
        let b = self.blocks();
        self.shared
            .update_from(self.project.perf(), self.link.epoch());
        self.ui.flush(&mut self.project);
        self.ui
            .publish(self.project.orbit(), self.sync, &mut self.shared.orbit);
        let bpm_before = self.last_view.bpm;
        let pending_before = self.runner.pending();

        if self
            .gate
            .before_block(&self.link, &mut self.inst, &self.shared)
        {
            let now = self.runner.now();
            for (src, ev) in self.keys.drain(..) {
                match ev {
                    InEvent::Note(n) => {
                        if matches!(n.kind, NoteKind::On(_)) {
                            self.ons.push(b);
                        }
                        self.runner.live(n);
                        self.inst.handle(n, &self.shared);
                    }
                    InEvent::Bend(b) => self.inst.bend(b),
                    InEvent::Sustain(p) => self.inst.sustain(p),
                    InEvent::Rt(rt) => self.runner.realtime(src, rt, now),
                }
            }
            let sent = self.runner.block(&mut self.inst, &self.shared);
            self.sent.clear();
            self.sent.extend(sent.iter());
        } else {
            self.sent.clear();
        }
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        let view = self.runner.view();
        if sync {
            self.ui.sync(&view, &mut self.project);
        }

        self.record();
        self.check(b, pending_before);
        if view.loops != self.last_view.loops {
            for r in 0..RINGS {
                let (a, z) = (&self.last_view.rings[r], &view.rings[r]);
                if a.hits != z.hits || a.pool != z.pool || a.notes != z.notes {
                    self.marks.push((b, Mark::Mutate(r as u8)));
                }
            }
        }
        if view.running && self.last_view.running && view.bpm != bpm_before {
            self.marks.push((b, Mark::Bpm));
        }
        if self.stopped_at == Some(b) {
            self.marks.push((b, Mark::Stop));
        }
        self.last_view = view;
    }

    pub fn run(&mut self, blocks: usize) {
        for _ in 0..blocks {
            self.block();
        }
    }

    /// `n` whole loops more. PLAY's first block is a boundary that starts
    /// a loop and ends none, so from PLAY it takes `n + 1` boundaries.
    pub fn run_loops(&mut self, n: u32) {
        let to = if self.last_view.running {
            self.last_view.loops + n
        } else {
            n + 1
        };
        while self.last_view.loops < to {
            self.block();
        }
    }

    fn record(&mut self) {
        let (out, pre) = (self.dac.out(), self.dac.input());
        for k in 0..DAC_PAIRS {
            self.tape.out[k].extend_from_slice(&out[k]);
            self.tape.pre_peak[k] = pre[k]
                .iter()
                .fold(self.tape.pre_peak[k], |m, x| m.max(x.abs()));
        }
        for i in 0..2 * BLOCK_SIZE {
            let x: f32 = out.iter().map(|pair| pair[i]).sum();
            self.clamped += (x.abs() > 1.0) as usize;
            self.sum.push(x);
        }
        // `finite` reads the bus too: every Part's, summed.
        let bus: Vec<f32> = (0..BLOCK_SIZE)
            .map(|i| (0..6).map(|p| self.inst.part_bus(p)[i]).sum())
            .collect();
        self.tape.bus.extend_from_slice(&bus);
        let a = self.inst.allocator();
        let live: u32 = a
            .slots()
            .iter()
            .filter(|s| !s.dying())
            .map(|s| s.cost().0)
            .sum();
        self.tape.bill.push(FxBus::COST.0 + live);
        self.tape
            .busy
            .push(a.slots().iter().filter(|s| !s.is_free()).count() as u8);
        self.tape.budget = a.budget().as_cost().0;
        self.tape.refused = a.refused();
    }

    fn check(&mut self, b: usize, before: [Option<PendingOff>; RINGS]) {
        let after = self.runner.pending();
        let slots = self.inst.allocator().slots();
        let origins = self.inst.origins();
        // A voice `kill_all` marked dying may still read held: it is going.
        let held = |v: usize| slots[v].held() && !slots[v].dying();
        for r in RingIdx::ALL {
            let i = r.get() as usize;
            let held: Vec<MidiNote> = (0..slots.len())
                .filter(|&v| held(v) && origins[v] == NoteOrigin::Orbit(r))
                .filter_map(|v| slots[v].note())
                .collect();
            let pend = after[i];
            if held.len() > pend.is_some() as usize {
                self.stuck.push((
                    b,
                    format!("ring {} holds {held:?}, pending {pend:?}", i + 1),
                ));
            } else if let (Some(&n), Some(p)) = (held.first(), pend)
                && n.get() != p.note.get()
            {
                self.stuck.push((
                    b,
                    format!("ring {} holds {n:?}, pending {:?}", i + 1, p.note),
                ));
            }
            // An off went out: the pending note before is gone or replaced.
            if let Some(p) = before[i]
                && after[i] != Some(p)
            {
                self.marks.push((b, Mark::RingOff(i as u8)));
            }
            if after[i].is_some_and(|p| before[i] != Some(p)) {
                self.ons.push(b);
            }
        }
        if self.live_released && (0..slots.len()).any(|v| held(v) && origins[v] == NoteOrigin::Live)
        {
            self.stuck
                .push((b, "a Live voice held after release".into()));
        }
        // A sounding voice re-booked for another note, or newly shed.
        if self.slots.iter().zip(slots).any(|(old, new)| {
            let taken = !old.is_free() && !new.is_free() && old.age() != new.age();
            taken || (!old.dying() && new.dying())
        }) {
            self.marks.push((b, Mark::Steal));
        }
        self.slots.copy_from_slice(slots);
        self.ons.dedup();
        self.marks.dedup();
    }

    /// Blocks from STOP until every voice was free: `None` if still busy,
    /// or not stopped.
    pub fn freed_after_stop(&self) -> Option<usize> {
        self.stopped_at.and_then(|s| self.tape.freed(s))
    }
}

/// What a render measured, per pair and summed.
#[derive(Debug)]
pub struct Report {
    pub finite: bool,
    pub peak: [f32; DAC_PAIRS],
    pub dc: [f32; DAC_PAIRS],
    pub lufs: [f32; DAC_PAIRS],
    pub sum_lufs: f32,
    pub clamped: usize,
    /// `(block, mark, pair, ratio)`.
    pub clicks: Vec<(usize, String, usize, f32)>,
    /// Marks judged (no note-on in their window), by `Mark::KINDS`.
    pub judged: [usize; 5],
    /// Marks seen, judged or not.
    pub marked: [usize; 5],
    pub stuck: Vec<(usize, String)>,
    pub over_blocks: usize,
    pub bill_max: u32,
    pub budget: u32,
    pub refused: u32,
}

/// Whether `Tape::click` can judge event block `e`, between the events
/// `prev` and `next`: its before window is past the start-up, its
/// transient is on the tape and holds no note-on
/// (an attack is legitimate), and one of its steady windows is `STEADY`
/// blocks long on the tape (an attack just before and the next event just
/// after leave nothing to judge against).
pub fn judgeable(t: &Tape, e: usize, (prev, next): (usize, usize), ons: &[usize]) -> bool {
    let end = t.blocks();
    let heard = transient_window(e);
    // Earlier, the before window would reach into the start-up.
    if e <= PRE_BLOCKS + 1 || heard.end >= end || ons.iter().any(|&o| o + 1 >= e && o <= heard.end)
    {
        return false;
    }
    let (pre, post) = steady_windows(e, (prev, next), true);
    pre.len().max((post.start..post.end.min(end)).len()) >= STEADY
}

/// Judges the render from block `from` on.
pub fn report(rig: &OrbitRig, from: usize) -> Report {
    let t = &rig.tape;
    let end = t.blocks();
    let mut clicks = Vec::new();
    let mut judged = [0; 5];
    let mut marked = [0; 5];
    // Steady windows stop short of any note-on and any other mark.
    let mut edges: Vec<usize> = rig
        .ons
        .iter()
        .copied()
        .chain(rig.marks.iter().map(|m| m.0))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    for &(e, mark) in &rig.marks {
        marked[mark.kind()] += 1;
        let prev = edges.iter().rev().find(|&&x| x < e).copied().unwrap_or(0);
        let next = edges
            .iter()
            .find(|&&x| x > e)
            .copied()
            .unwrap_or(usize::MAX);
        if e < from || !judgeable(t, e, (prev, next), &rig.ons) {
            continue;
        }
        judged[mark.kind()] += 1;
        for k in 0..DAC_PAIRS {
            if let Some(r) = t.click(k, e, (prev, next), true) {
                clicks.push((e, mark.label(), k, r));
            }
        }
    }
    let span = |k: usize| t.pair(k, from + 1, end);
    let sum = &rig.sum[(2 * BLOCK_SIZE * (from + 1)).min(rig.sum.len())..];
    Report {
        finite: t.finite(),
        peak: core::array::from_fn(|k| t.peak(k)),
        dc: core::array::from_fn(|k| t.dc(k, from.max(DC_FROM), end)),
        lufs: core::array::from_fn(|k| super::sweep::lufs(span(k))),
        sum_lufs: super::sweep::lufs(sum),
        clamped: rig.clamped,
        clicks,
        judged,
        marked,
        stuck: rig.stuck.clone(),
        over_blocks: t.bill.iter().filter(|&&b| b > t.budget).count(),
        bill_max: t.bill.iter().copied().max().unwrap_or(0),
        budget: t.budget,
        refused: t.refused,
    }
}

impl Report {
    /// The pairs that carry sound. NEW routes every Part to P1.
    pub fn carrying(&self) -> [bool; DAC_PAIRS] {
        core::array::from_fn(|k| self.peak[k] > LEAK_PEAK)
    }

    /// Everything wrong: not finite, over the ceiling, DC, the sum clamped,
    /// a click, a stuck note, billed over budget unshed, or a pair more
    /// than 6 dB from the others that carry sound.
    pub fn faults(&self) -> Vec<String> {
        let carrying = self.carrying();
        let mut f = Vec::new();
        if !self.finite {
            f.push("NaN or inf".into());
        }
        for k in 0..DAC_PAIRS {
            if !within_ceiling(self.peak[k]) {
                f.push(format!(
                    "P{} peak {:.4} over the ceiling",
                    k + 1,
                    self.peak[k]
                ));
            }
            if self.dc[k] > super::sweep::DC_MAX {
                f.push(format!("P{} DC {:.4}", k + 1, self.dc[k]));
            }
        }
        if self.clamped > 0 {
            f.push(format!("{} samples of the sum clamped", self.clamped));
        }
        for (b, m, k, r) in &self.clicks {
            f.push(format!("click at {m} (block {b}) on P{}: {r:.1}×", k + 1));
        }
        for (b, s) in self.stuck.iter().take(8) {
            f.push(format!("stuck at block {b}: {s}"));
        }
        if self.over_blocks > 0 {
            f.push(format!(
                "billed {} over the budget {} for {} blocks, unshed",
                self.bill_max, self.budget, self.over_blocks
            ));
        }
        let lufs: Vec<(usize, f32)> = (0..DAC_PAIRS)
            .filter(|&k| carrying[k])
            .map(|k| (k, self.lufs[k]))
            .collect();
        for &(k, l) in &lufs {
            let others: Vec<f32> = lufs.iter().filter(|o| o.0 != k).map(|o| o.1).collect();
            if !others.is_empty() {
                let mean = others.iter().sum::<f32>() / others.len() as f32;
                if (l - mean).abs() > 6.0 {
                    f.push(format!("P{} at {l:.1} LUFS, {mean:.1} the others", k + 1));
                }
            }
        }
        f
    }

    pub fn print(&self, name: &str) {
        println!(
            "{name}: LUFS P1 {:.1} P2 {:.1} P3 {:.1} sum {:.1}; peak {:.3}/{:.3}/{:.3}; DC {:.4}/{:.4}/{:.4}; \
             clamped {}; refused {}; bill max {} of {}",
            self.lufs[0],
            self.lufs[1],
            self.lufs[2],
            self.sum_lufs,
            self.peak[0],
            self.peak[1],
            self.peak[2],
            self.dc[0],
            self.dc[1],
            self.dc[2],
            self.clamped,
            self.refused,
            self.bill_max,
            self.budget,
        );
        println!("  marks judged/seen: {}", self.marks());
    }

    /// Each kind of mark, judged of seen: `off 986/1142, steal 0/906, …`.
    pub fn marks(&self) -> String {
        let m: Vec<String> = Mark::KINDS
            .iter()
            .enumerate()
            .map(|(i, k)| format!("{k} {}/{}", self.judged[i], self.marked[i]))
            .collect();
        m.join(", ")
    }
}

/// Seconds as blocks.
pub const fn secs(s: f32) -> usize {
    (s * BPS as f32) as usize
}

/// 48 kHz 16-bit stereo PCM with a hand-written 44-byte header.
pub fn write_wav(path: &std::path::Path, stereo: &[f32]) {
    let data = (stereo.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&2u16.to_le_bytes()); // stereo
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for &x in stereo {
        let s = (chimera_core::dsp::clampf(x, -1.0, 1.0) * 32767.0).round() as i16;
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).expect("the WAV");
}
