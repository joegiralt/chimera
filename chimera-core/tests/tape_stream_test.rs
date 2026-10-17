//! A streamed PLAY (slice 1 § 10.2): a source held as heads, the rest read
//! off a `TestCard` through the voice's ring, plays as the same source held
//! whole; a late card drops out and comes back cleanly; a chord's burst
//! holds behind 100 ms heads; a short loop in a head never asks the card.

mod common;

use core::mem::MaybeUninit;
use core::sync::atomic::Ordering;

use chimera_core::dsp::tape_engine::heads::{Bounds, shortest, window_end_min, window_start};
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    INC_MAX, PlayMode, SliceCode, TapeEngine, TapeEngineParams, TapeModel, inc,
};
use chimera_core::project::SourceId;
use chimera_core::stream::admit::head_frames;
use chimera_core::stream::port::StreamPort;
use chimera_core::stream::ring::{AudioSide, Rings, ThirdSlots};
use chimera_core::stream::test_card::TestCard;
use chimera_core::stream::units::AudioBlocks;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_hal::BLOCK_SIZE;
use common::XorShift;
use common::tape::{OneSource, held_plain, sine};

const SR: u32 = 48_000;
const RATE: u16 = 24_000;
/// 30 s at 24 kHz.
const FRAMES: u32 = 720_000;
/// `HEAD_MS`, and room for XFADE's 100 ms before the loop.
const HF: u32 = 4_800;
const V: VoiceIdx = VoiceIdx::ALL[0];

fn noise(frames: u32) -> Vec<f32> {
    let mut rng = XorShift(0x5EED_0019);
    (0..frames)
        .map(|_| (rng.next_u32() as f32 / u32::MAX as f32) * 1.8 - 0.9)
        .collect()
}

/// A deck on its voice's strip, latched to source 0 (`OneSource`'s).
fn deck(slot: &mut MaybeUninit<TapeEngine>) -> &mut TapeEngine {
    let e = TapeEngine::init_in_place(slot);
    e.seed(3);
    e.latch(SourceId::ALL[0]);
    e
}

/// The rings, one card serving them, and the block clock.
struct Stream<'r> {
    rings: &'r Rings,
    audio: AudioSide<'r>,
    card: TestCard<'r>,
    now: u32,
}

impl<'r> Stream<'r> {
    fn new(rings: &'r Rings, third: &'r ThirdSlots, whole: &[u8], latency: u32) -> Stream<'r> {
        let (audio, card) = rings.sides(third).unwrap();
        Stream {
            rings,
            audio,
            card: TestCard::new(card, whole, latency),
            now: 0,
        }
    }

    /// The card's turn between blocks.
    fn serve(&mut self) {
        self.card.serve(self.rings, AudioBlocks::new(self.now));
    }

    /// Voice `v`'s port this block.
    fn with<R>(&mut self, v: VoiceIdx, f: impl FnOnce(&mut StreamPort<'_, '_>) -> R) -> R {
        let mut turn = self.audio.turn();
        let mut port = self.card.port(&mut turn, v, AudioBlocks::new(self.now));
        f(&mut port)
    }

    fn stat(&self, f: impl Fn(&chimera_core::stream::stats::StreamStats) -> u32) -> u32 {
        f(&self.card.stats)
    }
}

fn rings() -> (Box<Rings>, Box<ThirdSlots>) {
    (Box::new(Rings::new()), Box::new(ThirdSlots::new()))
}

/// Where a note on `p` starts reading, and where its loop's fade first
/// reads (XFADE capped at 100 ms, a few frames early for the floor's
/// speed), as the deck works them out.
fn heads_at(p: &TapeEngineParams, frames: u32, note: u8) -> [u32; 2] {
    let ws = window_start(p.start, frames);
    let fast = inc(RATE, note, 0.0, p.root, SR) * 1.02;
    let floor = shortest(RATE, fast.min(INC_MAX));
    let we = window_end_min(ws, p.end, frames, floor);
    let b = Bounds::of_min(ws, we, (p.size, p.position, p.xfade), RATE, floor);
    let lx = b.lx.min(u32::from(RATE) / 10);
    [ws, b.ls.saturating_sub(lx + 8)]
}

fn params(mode: PlayMode, start: f32, end: f32) -> TapeEngineParams {
    TapeEngineParams {
        model: TapeModel::Play,
        mode,
        start,
        end,
        tape: 1.0,
        age: 0.5,
        slice: SliceCode::OFF,
        ..TapeEngineParams::default()
    }
}

/// A run's script: note-on at block 0, START turned at `turn` (the heads
/// re-staged for it), the note struck again at `again`.
struct Script {
    p: TapeEngineParams,
    turned: Option<f32>,
    turn: usize,
    again: usize,
    blocks: usize,
}

/// The same script on the source held whole: too big for the arena, so
/// read straight, as `play` reads a row (`start` is `note_on`, `play` is
/// `render`, on the row's view).
fn render_whole(s: &Script, note: u8, whole: &[u8]) -> Vec<f32> {
    let v = SourceView::new(whole).unwrap();
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    let mut p = s.p;
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    for k in 0..s.blocks {
        if k == s.turn
            && let Some(st) = s.turned
        {
            p.start = st;
        }
        if k == 0 || k == s.again {
            e.note_on(note, &p, &v, SR);
        }
        e.render(&mut b, &p, &v, SR);
        out.extend_from_slice(&b);
    }
    out
}

fn render_streamed(s: &Script, note: u8, whole: &[u8]) -> (Vec<f32>, [u32; 3]) {
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, whole, 0);
    let row = |p: &TapeEngineParams| st.card.held(&heads_at(p, FRAMES, note), HF);
    let mut srcs = [OneSource::new(&row(&s.p)), OneSource::new(&row(&s.p))];
    if let Some(t) = s.turned {
        srcs[1] = OneSource::new(&row(&TapeEngineParams { start: t, ..s.p }));
    }
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    let mut p = s.p;
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    let mut row = 0;
    for k in 0..s.blocks {
        if k == s.turn
            && let Some(t) = s.turned
        {
            p.start = t;
            row = 1;
        }
        st.serve();
        let src = &mut srcs[row];
        st.with(V, |port| {
            if k == 0 || k == s.again {
                e.start(note, &p, &src.view(), SR, Some(port));
            }
            e.play(&mut b, &p, &src.view(), SR, Some(port));
        });
        out.extend_from_slice(&b);
        st.now += 1;
    }
    let n = |f: fn(&chimera_core::stream::stats::StreamStats) -> &core::sync::atomic::AtomicU32| {
        st.stat(|s| f(s).load(Ordering::Relaxed))
    };
    (out, [n(|s| &s.underruns), n(|s| &s.cold), n(|s| &s.faults)])
}

/// The slice's key test: streamed with an on-time card, PLAY ONE and LOOP
/// render bit for bit as the source held whole, TAPE and AGE on, across a
/// START turn that re-stages the heads mid-note and a note struck after
/// it. LOOP's short loop sits past the ring at the window's end (its start
/// doesn't move with START); the long loop's passes read the ring and wrap
/// onto the loop-start head.
#[test]
fn streamed_equals_whole() {
    let whole = held_plain(&noise(FRAMES), RATE);
    let scripts = [
        Script {
            p: params(PlayMode::One, 0.30, 1.0),
            turned: Some(0.29),
            turn: 600,
            again: 1_200,
            blocks: 2_000,
        },
        Script {
            p: TapeEngineParams {
                size: 0.0,
                position: 1.0,
                xfade: 0.5,
                ..params(PlayMode::Loop, 0.30, 0.31)
            },
            turned: Some(0.29),
            turn: 600,
            again: 1_200,
            blocks: 2_000,
        },
        Script {
            p: TapeEngineParams {
                size: 0.8,
                position: 1.0,
                xfade: 0.25,
                ..params(PlayMode::Loop, 0.30, 0.32)
            },
            turned: None,
            turn: 0,
            again: 2_400,
            blocks: 3_200,
        },
    ];
    for (n, s) in scripts.iter().enumerate() {
        for note in [36, 48, 60, 72, 84, 96] {
            let w = render_whole(s, note, &whole);
            let (st, [under, cold, faults]) = render_streamed(s, note, &whole);
            assert!(
                w.iter().any(|&y| y != 0.0),
                "script {n} note {note}: silent"
            );
            let first = w
                .iter()
                .zip(&st)
                .position(|(a, b)| a.to_bits() != b.to_bits());
            assert_eq!(
                first,
                None,
                "script {n} note {note}: block {:?}",
                first.map(|i| i / BLOCK_SIZE)
            );
            assert_eq!([under, cold, faults], [0, 0, 0], "script {n} note {note}");
        }
    }
}

/// One read 200 blocks late mid-note: the voice fades out before the
/// frames run out, stays silent, and fades back in at the frame tape time
/// reached, where it plays the whole render again bit for bit. Neither edge
/// steps more than 4× the steady state's largest (`click_free_test`'s
/// rule); one underrun.
#[test]
fn underrun_drops_out_cleanly() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        tape: 0.0,
        age: 0.0,
        ..params(PlayMode::One, 0.1, 1.0)
    };
    let s = Script {
        p,
        turned: None,
        turn: 0,
        again: usize::MAX,
        blocks: 1_200,
    };
    let w = render_whole(&s, 60, &whole);

    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    let mut late = None;
    for k in 0..s.blocks {
        // From block 300, the next read is late; the reads after it aren't.
        if k == 300 {
            late = Some(st.card.claims());
            st.card.set_latency(200);
        }
        if late.is_some_and(|c| st.card.claims() > c) {
            st.card.set_latency(0);
            late = None;
        }
        st.serve();
        st.with(V, |port| {
            if k == 0 {
                e.start(60, &p, &src.view(), SR, Some(port));
            }
            e.play(&mut b, &p, &src.view(), SR, Some(port));
        });
        out.extend_from_slice(&b);
        st.now += 1;
    }
    assert_eq!(st.card.stats.underruns.load(Ordering::Relaxed), 1);
    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    let steady = step(&w[100 * BLOCK_SIZE..300 * BLOCK_SIZE]);
    let differs = |i: usize| w[i].to_bits() != out[i].to_bits();
    let gone = (0..out.len()).find(|&i| differs(i)).expect("a drop-out");
    let back = (gone..out.len()).rev().find(|&i| differs(i)).unwrap() + 1;
    assert!(back < out.len() - 100 * BLOCK_SIZE, "never came back");
    let silent = out[gone..back].iter().filter(|&&y| y == 0.0).count();
    assert!(silent > 16 * BLOCK_SIZE, "{silent} silent samples");
    let worst = step(&out[gone - 1..back + 1]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
}

/// Eight notes in one block at TRON's speed (×1.214: +3 st and 33 cents of
/// wow), the card at M1's worst (6.66 ms a read, 5 blocks) behind one read
/// in flight: 100 ms heads hold, 60 ms heads starve. The heads end just
/// past a chunk's start, as the burst's sums assume (spec § 5.4: the second
/// chunks due 92 ms after the head).
#[test]
fn chord_burst_holds() {
    let whole = held_plain(&noise(FRAMES), RATE);
    let underruns = |head: u32| {
        let (rings, third) = rings();
        let mut st = Stream::new(&rings, &third, &whole, 5);
        let at = 2_690 - head + 4;
        let p = TapeEngineParams {
            tape: 0.0,
            age: 0.0,
            ..params(PlayMode::One, at as f32 / FRAMES as f32, 1.0)
        };
        let start = window_start(p.start, FRAMES);
        let mut src = OneSource::new(&st.card.held(&[start], head));
        let mut slots: Vec<MaybeUninit<TapeEngine>> =
            (0..8).map(|_| MaybeUninit::uninit()).collect();
        let mut decks: Vec<&mut TapeEngine> = slots.iter_mut().map(deck).collect();
        let mut b = [0.0; BLOCK_SIZE];
        for k in 0..600 {
            // The read in flight holds the card for the burst's first 5.
            if k >= 5 {
                st.serve();
            }
            for (v, e) in VoiceIdx::ALL.into_iter().zip(decks.iter_mut()) {
                st.with(v, |port| {
                    if k == 0 {
                        e.set_pitch(333.0);
                        e.start(60, &p, &src.view(), SR, Some(port));
                    }
                    e.play(&mut b, &p, &src.view(), SR, Some(port));
                });
                assert!(k > 0 || e.is_active());
            }
            st.now += 1;
        }
        st.card.stats.underruns.load(Ordering::Relaxed)
    };
    assert_eq!(underruns(head_frames(RATE)), 0);
    assert!(underruns(head_frames(RATE) * 6 / 10) > 0);
}

/// Review Focus 5: a loop of about 1,000 frames inside the loop-start head
/// plays 10 s and never asks the card after the note-on's reads.
#[test]
fn short_loop_on_streamed_source_stays_in_ram() {
    let whole = held_plain(&noise(FRAMES), RATE);
    let p = TapeEngineParams {
        size: 0.3403,
        position: 0.0,
        xfade: 0.1,
        ..params(PlayMode::Loop, 0.5, 1.0)
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let ws = window_start(p.start, FRAMES);
    let b = Bounds::of(ws, FRAMES, (p.size, p.position, p.xfade), RATE);
    assert!((990..=1_010).contains(&b.frames()), "{b:?}");
    let mut src = OneSource::new(&st.card.held(&[ws, ws], HF));
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    let mut out = [0.0; BLOCK_SIZE];
    let mut after_on = 0;
    for k in 0..7_500 {
        st.serve();
        if k == 1 {
            after_on = st.card.claims();
        }
        st.with(V, |port| {
            if k == 0 {
                e.start(60, &p, &src.view(), SR, Some(port));
            }
            e.play(&mut out, &p, &src.view(), SR, Some(port));
        });
        st.now += 1;
    }
    assert!(e.is_active());
    assert!(e.events() > 200, "{} events", e.events());
    assert_eq!(st.card.claims(), after_on);
    assert_eq!(st.card.stats.underruns.load(Ordering::Relaxed), 0);
}

/// `tape_bounds_test`'s `reads_stay_in_row`, through spans: whatever PLAY
/// ONE or forward LOOP's params, every tap lands inside the span it reads
/// (strict views panic on one outside).
#[test]
fn span_reads_stay_in_bounds() {
    let mut rng = XorShift(0x5BA2_0019);
    let unit = |rng: &mut XorShift| rng.next_u32() as f32 / u32::MAX as f32;
    for run in 0..300 {
        let frames = 2 * (6_000 + rng.below(100_000) as u32);
        let s: Vec<f32> = (0..frames).map(|_| unit(&mut rng) * 1.8 - 0.9).collect();
        let whole = held_plain(&s, RATE);
        let (a, z) = (unit(&mut rng), unit(&mut rng));
        let p = TapeEngineParams {
            model: TapeModel::Play,
            mode: [PlayMode::One, PlayMode::Loop][rng.below(2)],
            start: a.min(z),
            end: a.max(z),
            position: unit(&mut rng),
            size: unit(&mut rng),
            xfade: 0.5 * unit(&mut rng),
            slice: SliceCode::new(rng.below(18) as u8),
            root: 12_700.0 * unit(&mut rng),
            ..TapeEngineParams::default()
        };
        let (rings, third) = rings();
        let mut st = Stream::new(&rings, &third, &whole, 0);
        let note = rng.below(128) as u8;
        let mut src = OneSource::new(&st.card.held(&heads_at(&p, frames, note), HF));
        let mut slot = MaybeUninit::uninit();
        let e = deck(&mut slot);
        e.strict_spans();
        let mut b = [0.0; BLOCK_SIZE];
        for k in 0..60 {
            st.serve();
            st.with(V, |port| {
                if k == 0 {
                    e.start(note, &p, &src.view(), SR, Some(port));
                }
                e.play(&mut b, &p, &src.view(), SR, Some(port));
            });
            assert!(b.iter().all(|y| y.is_finite()), "run {run}: {p:?}");
            st.now += 1;
        }
        assert_eq!(st.card.stats.faults.load(Ordering::Relaxed), 0, "run {run}");
    }
}

/// START turned while the note still plays its head: the heads re-staged
/// for the new START move under it, so the cursor starts again where the
/// note is, cold (no head holds it there), and never reads the moved
/// head's bytes as its own (strict views). It comes back as the whole
/// render, tape time having run on.
#[test]
fn reheading_under_the_head_starts_cold() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        tape: 0.0,
        age: 0.0,
        ..params(PlayMode::One, 0.3, 1.0)
    };
    let s = Script {
        p,
        turned: None,
        turn: 0,
        again: usize::MAX,
        blocks: 400,
    };
    let w = render_whole(&s, 60, &whole);
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let moved = TapeEngineParams { start: 0.2, ..p };
    let mut srcs = [
        OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF)),
        OneSource::new(&st.card.held(&heads_at(&moved, FRAMES, 60), HF)),
    ];
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    e.strict_spans();
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    for k in 0..s.blocks {
        st.serve();
        let src = &mut srcs[usize::from(k >= 10)];
        st.with(V, |port| {
            if k == 0 {
                e.start(60, &p, &src.view(), SR, Some(port));
            }
            e.play(&mut b, &p, &src.view(), SR, Some(port));
        });
        out.extend_from_slice(&b);
        st.now += 1;
    }
    assert_eq!(st.card.stats.cold.load(Ordering::Relaxed), 1);
    assert_eq!(&out[..10 * BLOCK_SIZE], &w[..10 * BLOCK_SIZE]);
    assert!(
        out[10 * BLOCK_SIZE..11 * BLOCK_SIZE]
            .iter()
            .all(|&y| y == 0.0)
    );
    let tail = 300 * BLOCK_SIZE;
    assert_eq!(&out[tail..], &w[tail..]);
}
