// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::Cell;

use chimera_engine::project::SourceId;
use chimera_engine::stream::cursor::{
    Cursor, DROP_FADE, FadeCmd, Flow, HeadIdx, Plan, QUIET_FADE, RamHead, SpanSrc, Streamed,
};
use chimera_engine::stream::layout::CHUNK_FRAMES;
use chimera_engine::stream::port::StreamPort;
use chimera_engine::stream::ring::{AudioSide, CardSide, Rings, ThirdSlots};
use chimera_engine::stream::slot::SlotState;
use chimera_engine::stream::stats::StreamStats;
use chimera_engine::stream::units::{AudioBlocks, Frames, Inc, SlotIdx, SourceFrame};
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_hal::card_bus::CardBus;
use chimera_hal::{BLOCK_SIZE, Ms};
use std::sync::atomic::Ordering;

const V: VoiceIdx = VoiceIdx::ALL[2];
const CF: u32 = CHUNK_FRAMES.get();
const FILE: Frames = Frames::new(40 * CF);
const SRC: Streamed = Streamed {
    id: SourceId::ALL[7],
    frames: FILE,
};
const HEAD: RamHead = RamHead {
    idx: HeadIdx::Start,
    start: SourceFrame::new(0),
    end: SourceFrame::new(4_800),
};
const H0: HeadIdx = HeadIdx::Start;
const DROP: u32 = DROP_FADE.get();
const QUIET: u32 = QUIET_FADE.get();

thread_local! {
    static PENDS: Cell<u32> = const { Cell::new(0) };
}

/// A plan's runs, as plain read positions.
fn runs(plan: &Plan) -> Vec<(SpanSrc, std::ops::Range<u32>)> {
    plan.runs()
        .iter()
        .map(|(s, r)| (*s, r.start.get()..r.end.get()))
        .collect()
}

fn pend() {
    PENDS.with(|p| p.set(p.get() + 1));
}

fn pends() -> u32 {
    PENDS.with(Cell::get)
}

struct Rig {
    rings: Box<Rings>,
    third: Box<ThirdSlots>,
    bus: CardBus,
    stats: StreamStats,
    loud: Cell<bool>,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            rings: Box::new(Rings::new()),
            third: Box::new(ThirdSlots::new()),
            bus: CardBus::new(),
            stats: StreamStats::new(),
            loud: Cell::new(false),
        }
    }

    fn play(&self) -> Play<'_> {
        let (audio, card) = self.rings.sides(&self.third).unwrap();
        Play {
            audio,
            card,
            rig: self,
            cur: Cursor::new(),
            pos: 0.0,
            now: 0,
        }
    }
}

/// One voice, its card, and the sample clock.
struct Play<'r> {
    audio: AudioSide<'r>,
    card: CardSide<'r>,
    rig: &'r Rig,
    cur: Cursor,
    pos: f64,
    now: u32,
}

impl Play<'_> {
    fn with<R>(&mut self, f: impl FnOnce(&mut Cursor, &mut StreamPort<'_, '_>) -> R) -> R {
        let mut turn = self.audio.turn();
        let mut port = StreamPort {
            turn: &mut turn,
            voice: V,
            frames: &|_| Some(FILE),
            now: AudioBlocks::new(self.now),
            bus: &self.rig.bus,
            stats: &self.rig.stats,
            pend,
            ratio_max: None,
            loud: &self.rig.loud,
        };
        f(&mut self.cur, &mut port)
    }

    fn start(&mut self, pos: u32, head: Option<RamHead>) {
        self.start_at(pos, 1.0, head);
    }

    fn start_at(&mut self, pos: u32, inc_max: f32, head: Option<RamHead>) {
        self.pos = f64::from(pos);
        let at = SourceFrame::new(pos);
        self.with(|c, p| c.start(at, Inc::new(inc_max), head, SRC, p));
    }

    /// Plans a block, then runs its samples at `inc`: each one's position
    /// and gain.
    fn block(&mut self, inc: f64) -> (Plan, Vec<(f64, f32)>) {
        let pos = self.pos;
        let plan = self.with(|c, p| c.plan(pos, p));
        let mut out = Vec::with_capacity(BLOCK_SIZE);
        for _ in 0..BLOCK_SIZE {
            out.push((self.pos, self.cur.gain()));
            self.pos += inc;
        }
        self.now += 1;
        (plan, out)
    }

    fn seen(&mut self, s: SlotIdx) -> SlotState {
        self.audio.turn().seen(V, s)
    }

    fn wanted(&mut self) -> Vec<(u32, SlotIdx)> {
        let mut w: Vec<_> = SlotIdx::ALL
            .into_iter()
            .filter_map(|s| match self.seen(s) {
                SlotState::Wanted { chunk, .. } => Some((chunk.get(), s)),
                _ => None,
            })
            .collect();
        w.sort_by_key(|&(c, _)| c);
        w
    }

    /// Fills every Wanted slot whose chunk `pick` takes, in chunk order.
    fn serve_if(&mut self, pick: impl Fn(u32) -> bool) {
        for (c, s) in self.wanted() {
            if pick(c) {
                let claim = self.card.claim(V, s).unwrap();
                self.card.write_guard(&claim);
                assert!(self.card.fill_done(claim));
            }
        }
    }

    fn serve(&mut self) {
        self.serve_if(|_| true);
    }

    fn slot_of(&mut self, chunk: u32) -> Option<SlotIdx> {
        SlotIdx::ALL.into_iter().find(|&s| match self.seen(s) {
            SlotState::Wanted { chunk: c, .. }
            | SlotState::Filling { chunk: c, .. }
            | SlotState::Ready { chunk: c, .. } => c.get() == chunk,
            SlotState::Empty => false,
        })
    }

    fn quiet_off(&self) {
        self.rig.bus.answer_quiet(Ms(0));
        assert!(self.rig.bus.settle(Ms(1_000)).is_some());
    }
}

/// Samples heard at a read no run of the plan takes.
fn audible_outside(plan: &Plan, samples: &[(f64, f32)]) -> usize {
    samples
        .iter()
        .filter(|&&(pos, g)| g > 0.0 && !runs(plan).iter().any(|r| r.1.contains(&(pos as u32))))
        .count()
}

fn underruns(r: &Rig) -> u32 {
    r.stats.underruns.load(Ordering::Relaxed)
}

fn colds(r: &Rig) -> u32 {
    r.stats.cold.load(Ordering::Relaxed)
}

fn gen_of(s: SlotState) -> Option<u32> {
    match s {
        SlotState::Wanted { generation, .. }
        | SlotState::Filling { generation, .. }
        | SlotState::Ready { generation, .. } => Some(generation.bits()),
        SlotState::Empty => None,
    }
}

#[test]
fn asks_every_slot_at_note_on() {
    let rig = Rig::new();
    let mut p = rig.play();
    let before = pends();
    p.start(0, Some(HEAD));
    assert_eq!(p.cur.flow(), Flow::Head(H0));
    // The chunk the head hands over to (it holds the head's last read,
    // 4,797), then the two after it, each a slot.
    let mut deadlines = vec![];
    for (s, want) in SlotIdx::ALL.into_iter().zip([1, 2, 3]) {
        match p.seen(s) {
            SlotState::Wanted {
                chunk, deadline, ..
            } => {
                assert_eq!(chunk.get(), want);
                deadlines.push(deadline.get());
            }
            other => panic!("{s:?} {other:?}"),
        }
    }
    // Chunk 1 is needed when the head runs out: (4,800 - 2) / 64 blocks.
    assert_eq!(deadlines, [74, 84, 126]);
    assert_eq!(pends() - before, 3, "every new want pends the card IRQ");
}

#[test]
fn cold_note_on_asks_from_under_the_head() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(3 * CF + 10, None);
    assert_eq!(p.cur.flow(), Flow::Cold);
    assert_eq!(
        p.wanted().iter().map(|w| w.0).collect::<Vec<_>>(),
        [3, 4, 5]
    );
}

#[test]
fn note_on_drops_a_last_note_s_slots() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(10 * CF, None);
    p.serve();
    p.start(0, Some(HEAD));
    assert_eq!(
        p.wanted().iter().map(|w| w.0).collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn ready_slot_released_when_head_passes_and_next_wanted() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    p.serve();
    let a = p.slot_of(1).unwrap();
    let g = gen_of(p.seen(a)).unwrap();
    // Chunk 1's last frame is a tap until the read passes 2 x CF + 1.
    while p.pos < f64::from(2 * CF) - 64.0 {
        p.block(1.0);
    }
    assert!(matches!(p.seen(a), SlotState::Ready { chunk, .. } if chunk.get() == 1));
    let before = pends();
    while p.pos < f64::from(2 * CF) + 2.0 {
        p.block(1.0);
    }
    p.block(1.0);
    match p.seen(a) {
        SlotState::Wanted {
            chunk, generation, ..
        } => {
            assert_eq!(chunk.get(), 4);
            assert_ne!(generation.bits(), g);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(pends() - before, 1);
    assert_eq!(underruns(&rig), 0);
}

#[test]
fn block_split_at_span_end() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    p.serve();
    let (a, b) = (p.slot_of(1).unwrap(), p.slot_of(2).unwrap());

    // The head until chunk 1's slot (unguarded: the head was before it)
    // holds a read, from CF + 1.
    let (plan, _) = p.block(1.0);
    assert_eq!(runs(&plan), [(SpanSrc::Head(H0), 0..65)]);
    p.pos = f64::from(CF) - 38.0;
    let (plan, _) = p.block(1.0);
    assert_eq!(
        runs(&plan),
        [
            (SpanSrc::Head(H0), CF - 38..CF + 1),
            (SpanSrc::Slot(a), CF + 1..CF + 27)
        ]
    );
    assert_eq!(p.cur.flow(), Flow::Ring(a));
    {
        let turn = p.audio.turn();
        let ready = p.cur.ready(&turn, V, a).unwrap();
        assert_eq!(ready.chunk().get(), 1);
    }

    // Chunk 2's slot, guarded from chunk 1's, takes over at its first
    // read whose taps it holds: 2 x CF - 3.
    p.pos = f64::from(2 * CF) - 30.0;
    let (plan, _) = p.block(1.0);
    assert_eq!(
        runs(&plan),
        [
            (SpanSrc::Slot(a), 2 * CF - 30..2 * CF - 3),
            (SpanSrc::Slot(b), 2 * CF - 3..2 * CF + 35)
        ]
    );
    assert_eq!(plan.fade, FadeCmd::Hold);
}

#[test]
fn runs_cover_every_block_at_speed() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start_at(0, 16.0, Some(HEAD));
    // On past END too: the last chunk holds every read after it.
    for _ in 0..200 {
        p.serve();
        let i0 = p.pos as u32;
        let (plan, gains) = p.block(16.0);
        let runs = runs(&plan);
        assert!(!runs.is_empty() && runs.len() <= 2);
        assert_eq!(runs[0].1.start, i0);
        if let [x, y] = &runs[..] {
            assert_eq!(x.1.end, y.1.start);
        }
        assert_eq!(runs[runs.len() - 1].1.end, i0 + 16 * 64 + 1);
        assert!(gains.iter().all(|&(_, g)| g == 1.0));
    }
    assert_eq!(underruns(&rig), 0);
}

/// Plays until a block plans a fade out: that block and the rest of the
/// samples to silence.
fn starve(p: &mut Play, inc: f64) -> (u32, Vec<(f64, f32)>) {
    for _ in 0..2_000 {
        let (plan, mut gains) = p.block(inc);
        assert_eq!(audible_outside(&plan, &gains), 0);
        if let FadeCmd::Out { end_frame } = plan.fade {
            let end_frame = end_frame.get();
            for r in runs(&plan) {
                assert!(r.1.end <= end_frame + 1, "reads past the held frames");
            }
            for _ in 0..3 {
                let (plan, more) = p.block(inc);
                assert_eq!(audible_outside(&plan, &more), 0);
                gains.extend(more);
            }
            return (end_frame, gains);
        }
    }
    panic!("never starved");
}

#[test]
fn underrun_fades_out_ending_at_last_held_frame() {
    for inc in [0.25, 1.0, 3.84, 16.0] {
        let rig = Rig::new();
        let mut p = rig.play();
        p.start_at(0, inc as f32, Some(HEAD));
        let (end_frame, gains) = starve(&mut p, inc);
        // The head's last read whose taps it holds.
        assert_eq!(end_frame, HEAD.end.get() - 3);
        let zero = gains.iter().position(|&(_, g)| g == 0.0).unwrap();
        assert!(
            zero < DROP as usize,
            "{inc}: silent within DROP_FADE: {zero}"
        );
        assert!(gains[zero].0 <= f64::from(end_frame), "{inc}");
        assert!(gains[zero..].iter().all(|&(_, g)| g == 0.0));
        assert!(matches!(p.cur.flow(), Flow::Starved));
        assert_eq!(underruns(&rig), 1);
    }
}

#[test]
fn head_holds_from_the_frame_after_its_start() {
    let start = 3 * CF + 100;
    let head = RamHead {
        idx: HeadIdx::Loop,
        start: SourceFrame::new(start),
        end: SourceFrame::new(start + 4_800),
    };
    // The read at START takes the frame before it, which the head lacks.
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(start, Some(head));
    assert_eq!(p.cur.flow(), Flow::Cold);

    let rig = Rig::new();
    let mut p = rig.play();
    p.start(start + 1, Some(head));
    assert_eq!(p.cur.flow(), Flow::Head(HeadIdx::Loop));
    let (plan, _) = p.block(1.0);
    assert_eq!(
        runs(&plan),
        [(SpanSrc::Head(HeadIdx::Loop), start + 1..start + 66)]
    );
    // The handoff chunk holds the head's last read.
    assert_eq!(p.wanted().first().map(|w| w.0), Some((start + 4_797) / CF));
}

#[test]
fn jump_without_start_counts_a_fault() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    p.block(1.0);
    p.pos = f64::from(20 * CF);
    let (plan, _) = p.block(1.0);
    assert!(plan.runs().is_empty());
    assert_eq!(rig.stats.unplanned.load(Ordering::Relaxed), 1);
}

#[test]
fn starved_reasks_under_head_and_cancels_stale() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    starve(&mut p, 1.0);
    let one = p.slot_of(1).unwrap();
    let g1 = gen_of(p.seen(one)).unwrap();
    // Tape time runs on: once the head is in chunk 3, chunks 1 and 2 are
    // stale.
    while p.pos < f64::from(3 * CF) + 100.0 {
        p.block(1.0);
    }
    let w: Vec<u32> = p.wanted().iter().map(|w| w.0).collect();
    assert_eq!(w, [3, 4, 5]);
    assert!(
        gen_of(p.seen(one)).unwrap() != g1,
        "the generation moves on"
    );
    let s3 = p.slot_of(3).unwrap();
    match p.seen(s3) {
        SlotState::Wanted { deadline, .. } => {
            assert!(
                AudioBlocks::new(p.now).until(deadline) <= 0,
                "under the head: due now"
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(p.cur.flow(), Flow::Starved));
    assert_eq!(underruns(&rig), 1, "one drop-out, one count");
}

#[test]
fn fades_in_over_drop_fade_when_ready() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    starve(&mut p, 1.0);
    for _ in 0..20 {
        p.block(1.0);
    }
    p.serve();
    let (plan, mut gains) = p.block(1.0);
    assert_eq!(plan.fade, FadeCmd::In);
    gains.extend(p.block(1.0).1);
    gains.extend(p.block(1.0).1);
    for (k, &(_, g)) in gains.iter().enumerate() {
        let want = ((k + 1) as f32 / DROP as f32).min(1.0);
        assert!((g - want).abs() < 1e-4, "{k}: {g} vs {want}");
    }
    assert!(matches!(p.cur.flow(), Flow::Ring(_)));
    assert_eq!(underruns(&rig), 1);
}

#[test]
fn cold_start_silent_until_ready() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(10 * CF + 500, None);
    assert_eq!(colds(&rig), 1);
    for _ in 0..10 {
        let (plan, gains) = p.block(1.0);
        assert!(plan.runs().is_empty());
        assert!(gains.iter().all(|&(_, g)| g == 0.0));
    }
    assert_eq!(p.cur.flow(), Flow::Cold);
    p.serve();
    let (plan, gains) = p.block(1.0);
    assert_eq!(plan.fade, FadeCmd::In);
    assert!((gains[0].1 - 1.0 / DROP as f32).abs() < 1e-6);
    assert!(matches!(p.cur.flow(), Flow::Ring(_)));
    assert_eq!(colds(&rig), 1);
    assert_eq!(underruns(&rig), 0);
}

#[test]
fn quiet_fades_over_quiet_fade_and_cancels_wants() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    p.serve_if(|c| c < 3);
    for _ in 0..4 {
        p.block(1.0);
    }
    let three = p.slot_of(3).unwrap();
    rig.bus.ask_quiet(Ms(0));
    let (_, mut gains) = p.block(1.0);
    assert_eq!(p.cur.flow(), Flow::Quiet);
    assert_eq!(p.seen(three), SlotState::Empty, "wants are cancelled");
    for _ in 0..(QUIET / 64 + 1) {
        gains.extend(p.block(1.0).1);
    }
    for (k, &(_, g)) in gains.iter().enumerate() {
        let want = (1.0 - (k + 1) as f32 / QUIET as f32).max(0.0);
        assert!((g - want).abs() < 1e-4, "{k}: {g} vs {want}");
    }
    // Silent: nothing held, nothing asked; long enough that tape time
    // passes the head.
    let before = pends();
    for _ in 0..80 {
        let (plan, gains) = p.block(1.0);
        assert!(plan.runs().is_empty());
        assert!(gains.iter().all(|&(_, g)| g == 0.0));
    }
    assert!(SlotIdx::ALL.iter().all(|&s| p.seen(s) == SlotState::Empty));
    assert_eq!(pends(), before);
    assert_eq!(colds(&rig), 0);

    // Released past the head: re-asked from where tape time is, cold.
    p.quiet_off();
    p.block(1.0);
    assert_eq!(p.cur.flow(), Flow::Cold);
    assert_eq!(colds(&rig), 1);
    assert_eq!(pends() - before, 3);
    p.serve();
    assert_eq!(p.block(1.0).0.fade, FadeCmd::In);
    assert_eq!(underruns(&rig), 0);
}

/// #608: a quiet shorter than the head plays on from it once released,
/// warm: no cold start, no card read, the gain rising over `DROP_FADE`.
#[test]
fn a_short_quiet_returns_warm_from_its_head() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    p.serve_if(|c| c < 3);
    for _ in 0..4 {
        p.block(1.0);
    }
    rig.bus.ask_quiet(Ms(0));
    for _ in 0..(QUIET / 64 + 4) {
        p.block(1.0);
    }
    assert!(
        p.pos < f64::from(HEAD.end.get()) - 1_000.0,
        "still on the head"
    );
    let before = pends();
    p.quiet_off();
    let (plan, gains) = p.block(1.0);
    assert_eq!(p.cur.flow(), Flow::Head(H0));
    assert_eq!(colds(&rig), 0, "never cold");
    assert!(!plan.runs().is_empty(), "the head reads at once");
    assert!(
        (gains[0].1 - 1.0 / DROP as f32).abs() < 1e-6,
        "{}",
        gains[0].1
    );
    assert_eq!(audible_outside(&plan, &gains), 0);
    for _ in 0..(DROP / 64 + 1) {
        p.block(1.0);
    }
    assert_eq!(p.block(1.0).1[0].1, 1.0, "full again, no card read");
    assert!(pends() >= before, "its slots asked again for what follows");
    assert_eq!(underruns(&rig), 0);
}

/// #1083: a quiet released just short of the head's end, where the head
/// can't hold the block and its fade ahead, starts cold: no underrun
/// counted for a drop-out that never sounded.
#[test]
fn a_quiet_released_at_the_heads_end_starts_cold() {
    let rig = Rig::new();
    let mut p = rig.play();
    rig.bus.ask_quiet(Ms(0));
    p.start(4_500, Some(HEAD));
    for _ in 0..3 {
        p.block(1.0);
    }
    let at = p.pos as u32;
    assert!(
        at < HEAD.end.get() && at + 64 + DROP > HEAD.end.get(),
        "{at}"
    );
    p.quiet_off();
    p.block(1.0);
    assert_eq!(p.cur.flow(), Flow::Cold);
    assert_eq!(underruns(&rig), 0);
}

/// A note-on under a write's quiet, on a head, starts silent: it never
/// sounds mid-save, nor keeps the quiet waiting (`loud`); released, it
/// fades in from where its tape reached.
#[test]
fn a_start_under_quiet_stays_silent() {
    let rig = Rig::new();
    let mut p = rig.play();
    rig.bus.ask_quiet(Ms(0));
    p.start(0, Some(HEAD));
    for _ in 0..10 {
        let (plan, gains) = p.block(1.0);
        assert!(plan.runs().is_empty());
        assert!(gains.iter().all(|&(_, g)| g == 0.0));
        assert!(!rig.loud.get(), "never loud");
    }
    // Still on its head: warm from it, rising over `DROP_FADE` (#608).
    p.quiet_off();
    let (plan, gains) = p.block(1.0);
    assert_eq!(p.cur.flow(), Flow::Head(H0));
    assert!(!plan.runs().is_empty());
    assert!((gains[0].1 - 1.0 / DROP as f32).abs() < 1e-6);
}

/// A warm jump at full gain just as a quiet is asked keeps its level: the
/// quiet's fade takes it down from there, never a snap to 0.
#[test]
fn a_jump_under_quiet_keeps_its_level() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start(0, Some(HEAD));
    for _ in 0..4 {
        p.block(1.0);
    }
    rig.bus.ask_quiet(Ms(0));
    let at = p.pos as u32;
    p.with(|c, port| c.jump(SourceFrame::new(at), Inc::new(1.0), Some(HEAD), port));
    let (_, gains) = p.block(1.0);
    let step = 1.0 / QUIET as f32;
    assert!((gains[0].1 - (1.0 - step)).abs() < 1e-4, "{}", gains[0].1);
    assert!(
        (gains[1].1 - (1.0 - 2.0 * step)).abs() < 1e-4,
        "{}",
        gains[1].1
    );
}

#[test]
fn gain_never_steps() {
    let rig = Rig::new();
    let mut p = rig.play();
    p.start_at(0, 2.0, Some(HEAD));
    let mut last = p.cur.gain();
    let mut outside = 0;
    let mut worst = 0f32;
    let mut levels = (false, false);
    for b in 0..1_500u32 {
        // An on-time card, an outage, a late card, a quiet mid-note.
        match b {
            0..=99 | 400..=599 | 900.. => p.serve(),
            200..=399 if b % 30 == 0 => p.serve(),
            _ => {}
        }
        if b == 700 {
            rig.bus.ask_quiet(Ms(0));
        }
        if b == 800 {
            p.quiet_off();
        }
        // Under the note's inc_max, varying block to block.
        let inc = 0.5 + f64::from(b % 7) * 0.25;
        let (plan, samples) = p.block(inc);
        outside += audible_outside(&plan, &samples);
        for (_, g) in samples {
            worst = worst.max((g - last).abs());
            levels.0 |= g == 0.0;
            levels.1 |= g == 1.0;
            last = g;
        }
    }
    assert!(levels.0 && levels.1);
    assert!(underruns(&rig) > 0 && colds(&rig) > 0);
    assert!(worst <= 1.0 / DROP as f32 + 1e-6, "{worst}");
    assert_eq!(outside, 0, "heard outside the runs");
}

/// A start names its note's source on the ring, before its wants, so the
/// card reads them from that file; a jump keeps it.
#[test]
fn a_start_names_its_source_on_the_ring() {
    let rig = Rig::new();
    let mut p = rig.play();
    assert_eq!(rig.rings.source(V), None);
    p.start(0, None);
    assert_eq!(rig.rings.source(V), Some(SRC.id));
    assert!(!p.wanted().is_empty());
    p.with(|c, port| c.jump(SourceFrame::new(CF), Inc::new(1.0), None, port));
    assert_eq!(rig.rings.source(V), Some(SRC.id));
}
