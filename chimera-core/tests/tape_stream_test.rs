//! A streamed WALK (slice 1 § 10.2): a source held as heads, the rest read
//! off a `TestCard` through the voice's ring, plays as the same source held
//! whole; a late card drops out and comes back cleanly; a chord's burst
//! holds behind 100 ms heads; a short loop in a head never asks the card.

mod common;

use chimera_core::project::PartId;
use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::dsp::tape_engine::Way;
use chimera_core::stream::units::{Cents, SourceRate};
use core::mem::MaybeUninit;
use core::num::NonZeroU32;
use core::sync::atomic::Ordering;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::tape_engine::heads::{
    Bounds, Dir, Sizing, shortest, window_end, window_end_min, window_start,
};
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    COST_STREAM, Direction, FADE_HOLD, INC_MAX, Retrig, SliceCode, TapeEngine, TapeEngineParams,
    Wow, inc, ratio_max, tape_runs,
};
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::NoteOrigin;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::note_queue::{BendEvent, NoteEvent, NoteKind};
use chimera_core::orbit::ring::Harmony;
use chimera_core::params::{EngineType, PitchParams, Steal};
use chimera_core::part::{Held, LiveSnap, PartMode};
use chimera_core::project::SourceId;
use chimera_core::source_table::GateView;
use chimera_core::storage::SourceRef;
use chimera_core::stream::admit::{STREAM_BUDGET_BPS, WOW_CENTS, bill, head_frames};
use chimera_core::stream::cursor::{DROP_FADE, QUIET_FADE};
use chimera_core::stream::layout::CHUNK_FRAMES;
use chimera_core::stream::port::{StreamCard, StreamPort, Streams};
use chimera_core::stream::ring::{AudioSide, Rings, ThirdSlots};
use chimera_core::stream::stage::heads_for;
use chimera_core::stream::test_card::TestCard;
use chimera_core::stream::units::AudioBlocks;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::card_bus::{CardFault, Quiet};
use chimera_hal::{BLOCK_SIZE, Ms};
use chimera_theory::{Chord, Key, PitchClass, Quality, Scale};
use common::tape::{OneSource, held_plain, sine};
use common::{InstRig, XorShift, init_params};

const SR: u32 = 48_000;
const RATE: u16 = 24_000;
const HZ: SourceRate = SourceRate::new(RATE).unwrap();
/// 30 s at 24 kHz.
const FRAMES: u32 = 720_000;
const FILE: NonZeroU32 = NonZeroU32::new(FRAMES).unwrap();
/// `HEAD_MS`, and room for XFADE's 100 ms before the loop.
const HF: u32 = 4_800;
const V: VoiceIdx = VoiceIdx::ALL[0];
/// A script's START turn under the note's head (`streamed_equals_whole`).
const WARM: usize = usize::MAX;

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
    let ws = window_start(p.start, NonZeroU32::new(frames).unwrap());
    let fast = inc(
        HZ,
        MidiNote::clamped(note),
        Cents::ZERO,
        Cents::new(p.root),
        SR,
    )
    .get()
        * 1.02;
    let floor = shortest(HZ, fast.min(INC_MAX));
    let we = window_end_min(ws, p.end, frames, floor);
    let b = Bounds::of_min(ws, we, sizing(p), HZ, floor);
    let lx = b.lx().min(u32::from(RATE) / 10);
    [ws, b.ls().saturating_sub(lx + 8)]
}

fn sizing(p: &TapeEngineParams) -> Sizing {
    Sizing {
        size: p.size,
        place: p.position,
        xfade: p.xfade,
    }
}

fn params(mode: Direction, start: f32, end: f32) -> TapeEngineParams {
    TapeEngineParams {
        machine: TapeMachine::Walk,
        direction: mode,
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
#[derive(Clone, Copy)]
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

/// The streamed render, its underruns, cold starts and faults, and the
/// reads the card took the block after the START turn.
fn render_streamed(s: &Script, note: u8, whole: &[u8]) -> (Vec<f32>, [u32; 4]) {
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
    let mut turned = [0; 2];
    for k in 0..s.blocks {
        if k == s.turn
            && let Some(t) = s.turned
        {
            p.start = t;
            row = 1;
        }
        st.serve();
        if k == s.turn || k == s.turn + 1 {
            turned[k - s.turn] = st.card.claims();
        }
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
    (
        out,
        [
            n(|s| &s.underruns),
            n(|s| &s.cold),
            n(|s| &s.unplanned),
            turned[1] - turned[0],
        ],
    )
}

/// The slice's key test: streamed with an on-time card, WALK ONE and LOOP
/// render bit for bit as the source held whole, TAPE and AGE on, across a
/// START turn that re-stages the heads mid-note and a note struck after
/// it. LOOP's short loop sits past the ring at the window's end (its start
/// doesn't move with START); the long loop's passes read the ring and wrap
/// onto the loop-start head. Warm: START nudged while the note still plays
/// its START head, whose re-staged copy still holds it, so the cursor starts
/// again on it (the card reads the ring anew) with no cold start.
#[test]
fn streamed_equals_whole() {
    let whole = held_plain(&noise(FRAMES), RATE);
    let scripts = [
        Script {
            p: params(Direction::Forward, 0.30, 1.0),
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
                ..params(Direction::Loop, 0.30, 0.31)
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
                ..params(Direction::Loop, 0.30, 0.32)
            },
            turned: None,
            turn: 0,
            again: 2_400,
            blocks: 3_200,
        },
        Script {
            p: params(Direction::Forward, 0.30, 1.0),
            turned: Some(0.3001),
            turn: WARM,
            again: 1_200,
            blocks: 1_600,
        },
        Script {
            p: TapeEngineParams {
                size: 0.0,
                position: 1.0,
                xfade: 0.5,
                ..params(Direction::Loop, 0.30, 0.31)
            },
            turned: Some(0.3001),
            turn: WARM,
            again: 1_200,
            blocks: 1_600,
        },
    ];
    for (n, s) in scripts.iter().enumerate() {
        for note in [36, 48, 60, 72, 84, 96] {
            // A warm turn comes 200 frames into the note, under its head.
            let inc = 0.5 * 2f32.powf((f32::from(note) - 60.0) / 12.0);
            let warm = (200.0 / (64.0 * inc)).ceil() as usize + 1;
            let s = &Script {
                turn: if s.turn == WARM { warm } else { s.turn },
                ..*s
            };
            let w = render_whole(s, note, &whole);
            let (st, [under, cold, faults, reread]) = render_streamed(s, note, &whole);
            if s.turn == warm {
                assert!(reread >= 3, "script {n} note {note}: no re-head ({reread})");
            }
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
        ..params(Direction::Forward, 0.1, 1.0)
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
/// in flight: 100 ms heads hold, 60 ms heads starve. Placed as a load
/// places them (`stage::heads_for`: just past a chunk's start, #583), they
/// hold at every START, the worst included: there a 100 ms head cut at its
/// length ends 2 frames short of a chunk, and starves.
#[test]
fn chord_burst_holds() {
    let whole = held_plain(&noise(FRAMES), RATE);
    let underruns = |at: u32, held: &dyn Fn(&TestCard<'_>, &TapeEngineParams) -> Vec<u8>| {
        let (rings, third) = rings();
        let mut st = Stream::new(&rings, &third, &whole, 5);
        let p = TapeEngineParams {
            tape: 0.0,
            age: 0.0,
            ..params(Direction::Forward, at as f32 / FRAMES as f32, 1.0)
        };
        let mut src = OneSource::new(&held(&st.card, &p));
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
    let cut = |head: u32| {
        move |c: &TestCard<'_>, p: &TapeEngineParams| c.held(&[window_start(p.start, FILE)], head)
    };
    let loaded = |c: &TestCard<'_>, p: &TapeEngineParams| {
        let r = SourceRef {
            crc: 0,
            frames: FRAMES,
            rate: RATE,
        };
        c.held_heads(heads_for(r, p).unwrap().as_slice())
    };
    let hf = head_frames(HZ);
    // Cut at its length, a head ending just past a chunk's start holds.
    let kind = 2_690 - hf + 4;
    assert_eq!(underruns(kind, &cut(hf)), 0);
    assert!(underruns(kind, &cut(hf * 6 / 10)) > 0);
    // The worst: cut at its length, it ends 2 frames short of chunk 2.
    let cf = CHUNK_FRAMES.get();
    let worst = 2 * cf - 2 - hf + 1;
    assert!(underruns(worst, &cut(hf)) > 0, "a cut head starves there");
    for at in [worst, worst + 2, kind, 3 * cf + 1_000, 1] {
        assert_eq!(underruns(at, &loaded), 0, "START at {at}");
    }
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
        ..params(Direction::Loop, 0.5, 1.0)
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let ws = window_start(p.start, FILE);
    let b = Bounds::of(ws, FRAMES, sizing(&p), HZ);
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

/// `tape_bounds_test`'s `reads_stay_in_row`, through spans: whatever WALK
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
            direction: [Direction::Forward, Direction::Loop][rng.below(2)],
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
        assert_eq!(
            st.card.stats.unplanned.load(Ordering::Relaxed),
            0,
            "run {run}"
        );
        assert_eq!(
            st.card.stats.cold_loud.load(Ordering::Relaxed),
            0,
            "run {run}"
        );
    }
}

/// START turned while a note plays (#588): the heads stay until no note
/// holds the source (`Pool::source_rehead`), so the note plays on as held
/// whole, never cut. A note at the new START before the re-stage starts
/// cold, reading nothing stale (strict views), and fades in to the whole
/// render; once re-staged, a note there starts warm.
#[test]
fn a_start_turn_never_cuts_a_note() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Forward, 0.3, 1.0)
    };
    let moved = TapeEngineParams { start: 0.2, ..p };
    let script = |p, turned| Script {
        p,
        turned,
        turn: 10,
        again: usize::MAX,
        blocks: 400,
    };
    let wa = render_whole(&script(p, Some(moved.start)), 60, &whole);
    let wb = render_whole(&script(moved, None), 60, &whole);
    let r = SourceRef {
        crc: 0,
        frames: FRAMES,
        rate: RATE,
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let row =
        |c: &TestCard<'_>, q: &TapeEngineParams| c.held_heads(heads_for(r, q).unwrap().as_slice());
    let mut stale = OneSource::new(&row(&st.card, &p));
    let mut slots: Vec<MaybeUninit<TapeEngine>> = (0..3).map(|_| MaybeUninit::uninit()).collect();
    let mut decks: Vec<&mut TapeEngine> = slots.iter_mut().map(deck).collect();
    decks.iter_mut().for_each(|e| e.strict_spans());
    let (mut oa, mut ob) = (Vec::new(), Vec::new());
    let mut b = [0.0; BLOCK_SIZE];
    for k in 0..420 {
        st.serve();
        let q = if k < 10 { p } else { moved };
        let (va, vb) = (VoiceIdx::ALL[0], VoiceIdx::ALL[1]);
        st.with(va, |port| {
            if k == 0 {
                decks[0].start(60, &q, &stale.view(), SR, Some(port));
            }
            decks[0].play(&mut b, &q, &stale.view(), SR, Some(port));
        });
        oa.extend_from_slice(&b);
        if k >= 20 {
            st.with(vb, |port| {
                if k == 20 {
                    decks[1].start(60, &moved, &stale.view(), SR, Some(port));
                }
                decks[1].play(&mut b, &moved, &stale.view(), SR, Some(port));
            });
            ob.extend_from_slice(&b);
        }
        st.now += 1;
    }
    assert_eq!(&oa[..400 * BLOCK_SIZE], &wa[..], "never cut");
    assert_eq!(
        st.card.stats.cold.load(Ordering::Relaxed),
        1,
        "the new START, cold"
    );
    assert!(
        ob[..BLOCK_SIZE].iter().all(|&y| y == 0.0),
        "silent until it lands"
    );
    let tail = 100 * BLOCK_SIZE;
    assert_eq!(&ob[tail..380 * BLOCK_SIZE], &wb[tail..380 * BLOCK_SIZE]);
    // Re-staged once no note holds it: a note at the new START, warm.
    let mut fresh = OneSource::new(&row(&st.card, &moved));
    let mut out = Vec::new();
    for k in 0..100 {
        st.serve();
        st.with(VoiceIdx::ALL[2], |port| {
            if k == 0 {
                decks[2].start(60, &moved, &fresh.view(), SR, Some(port));
            }
            decks[2].play(&mut b, &moved, &fresh.view(), SR, Some(port));
        });
        out.extend_from_slice(&b);
        st.now += 1;
    }
    assert_eq!(st.card.stats.cold.load(Ordering::Relaxed), 1, "warm");
    assert_eq!(&out[..], &wb[..100 * BLOCK_SIZE]);
}

/// DRIFT moves every pass off the heads: each wrap still lands warm, a
/// pass ahead's prefetch following the walk (#698), and no step passes 4×
/// the steady state's.
#[test]
fn walking_loop_lands_warm() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        size: 0.6,
        motion: 0.5,
        xfade: 0.2,
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Loop, 0.30, 0.40)
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    e.strict_spans();
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    for k in 0..3_000 {
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
    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    assert!(e.events() >= 8, "{} events", e.events());
    assert_eq!(st.card.stats.cold.load(Ordering::Relaxed), 0, "cold wraps");
    let steady = step(&out[10 * BLOCK_SIZE..100 * BLOCK_SIZE]);
    let worst = step(&out[100 * BLOCK_SIZE..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
}

/// POSITION moves the loop off its staged head mid-note: each wrap's
/// reads are prefetched a pass ahead into the slots its pass's end frees
/// (#698), so every wrap lands warm. At XFADE 0.25 no step passes 4× the
/// steady state's largest (`click_free_test`'s rule); at XFADE 0 (a hard
/// splice) it plays what the whole row plays, sample for sample. It used
/// to drop out at every wrap.
#[test]
fn moved_loop_lands_warm_at_the_wrap() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let row = SourceView::new(&whole).unwrap();
    for xfade in [0.0, 0.25] {
        let p = TapeEngineParams {
            size: 0.8,
            position: 1.0,
            xfade,
            tape: 0.0,
            age: 0.0,
            ..params(Direction::Loop, 0.30, 0.32)
        };
        let (rings, third) = rings();
        let mut st = Stream::new(&rings, &third, &whole, 0);
        let mut src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
        let mut slot = MaybeUninit::uninit();
        let e = deck(&mut slot);
        // No read, the fade's second head's either, leaves its span.
        e.strict_spans();
        let mut ram_slot = MaybeUninit::uninit();
        let ram = deck(&mut ram_slot);
        let moved = TapeEngineParams { position: 0.5, ..p };
        let (mut out, mut want) = (Vec::new(), Vec::new());
        let mut b = [0.0; BLOCK_SIZE];
        for k in 0..1_500 {
            let q = if k < 50 { &p } else { &moved };
            st.serve();
            st.with(V, |port| {
                if k == 0 {
                    e.start(60, q, &src.view(), SR, Some(port));
                }
                e.play(&mut b, q, &src.view(), SR, Some(port));
            });
            out.extend_from_slice(&b);
            if k == 0 {
                ram.note_on(60, q, &row, SR);
            }
            ram.render(&mut b, q, &row, SR);
            want.extend_from_slice(&b);
            st.now += 1;
        }
        let step = |x: &[f32]| {
            x.windows(2)
                .map(|d| (d[1] - d[0]).abs())
                .fold(0.0, f32::max)
        };
        let cold = st.card.stats.cold.load(Ordering::Relaxed);
        assert_eq!(cold, 0, "xfade {xfade}: cold wraps");
        if xfade > 0.0 {
            // The first wrap comes near block 296.
            let steady = step(&out[60 * BLOCK_SIZE..250 * BLOCK_SIZE]);
            let worst = step(&out[250 * BLOCK_SIZE..]);
            assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
        } else {
            let off = out
                .iter()
                .zip(&want)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(off < 1e-5, "{off} off the whole row's");
        }
        let tail = &out[1_000 * BLOCK_SIZE..];
        assert!(tail.iter().any(|&y| y.abs() > 0.5), "xfade {xfade}: silent");
    }
}

/// A streamed LOOP's XFADE stops at 100 ms of source from its first pass
/// (§ 7.3 step 6), which the loop head a load places holds (#584): every
/// wrap lands warm on it, none cold, nothing starves, every read in its
/// span.
#[test]
fn streamed_xfade_stops_at_what_the_loop_head_holds() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        size: 0.75,
        position: 0.5,
        xfade: 0.5,
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Loop, 0.30, 0.34)
    };
    let r = SourceRef {
        crc: 0,
        frames: FRAMES,
        rate: RATE,
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held_heads(heads_for(r, &p).unwrap().as_slice()));
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    e.strict_spans();
    let (mut b, mut out) = ([0.0; BLOCK_SIZE], Vec::new());
    for k in 0..2_000 {
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
    assert!(e.events() >= 4, "{} events: it wrapped", e.events());
    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    let steady = step(&out[60 * BLOCK_SIZE..400 * BLOCK_SIZE]);
    let worst = step(&out[400 * BLOCK_SIZE..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
    // The fade reads RAM: the voice never ducks (a 300 Hz sine is never
    // near 0 for a drop fade's 144 samples).
    let quiet = out[60 * BLOCK_SIZE..]
        .split(|y| y.abs() > 0.05)
        .map(<[f32]>::len)
        .max();
    assert!(
        quiet < Some(100),
        "a run of {quiet:?} quiet samples: it ducked"
    );
    let stats = &st.card.stats;
    assert_eq!(stats.cold.load(Ordering::Relaxed), 0, "every wrap warm");
    assert_eq!(stats.underruns.load(Ordering::Relaxed), 0);
    assert_eq!(stats.unplanned.load(Ordering::Relaxed), 0);
    assert_eq!(stats.cold_loud.load(Ordering::Relaxed), 0);
}

/// A loop moved a little earlier, its landing still in the loop head but
/// its fade's start not: the voice ducks before the wrap and lands warm,
/// rising from where the duck left it, never stepping to full.
#[test]
fn a_warm_landing_after_a_duck_rises() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        size: 0.75,
        position: 0.5,
        xfade: 0.3,
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Loop, 0.30, 0.34)
    };
    let r = SourceRef {
        crc: 0,
        frames: FRAMES,
        rate: RATE,
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held_heads(heads_for(r, &p).unwrap().as_slice()));
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    e.strict_spans();
    // About 1,000 frames earlier: lands in the head, the fade's start not.
    let moved = TapeEngineParams {
        position: 0.5 - 1_000.0 / 23_000.0,
        ..p
    };
    let (mut b, mut out) = ([0.0; BLOCK_SIZE], Vec::new());
    for k in 0..2_000 {
        let q = if k < 50 { &p } else { &moved };
        st.serve();
        st.with(V, |port| {
            if k == 0 {
                e.start(60, q, &src.view(), SR, Some(port));
            }
            e.play(&mut b, q, &src.view(), SR, Some(port));
        });
        out.extend_from_slice(&b);
        st.now += 1;
    }
    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    let steady = step(&out[60 * BLOCK_SIZE..400 * BLOCK_SIZE]);
    let worst = step(&out[400 * BLOCK_SIZE..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
    assert_eq!(st.card.stats.cold.load(Ordering::Relaxed), 0, "warm");
    assert!(e.events() >= 4, "{} events", e.events());
}

// Admission and refusals at note-on (slice 1 § 5.5, § 2.3).

/// An instrument whose voices stream through one card, on time.
struct Played<'r> {
    rig: InstRig,
    st: Stream<'r>,
    src: OneSource,
    /// A source the card's lookup gives 100 frames.
    short: Option<SourceId>,
}

impl<'r> Played<'r> {
    /// Every Part's wheel full up: its notes bill their whole bend range
    /// (admission bills the bend in use, #698).
    fn wheel_up(&mut self) {
        for i in 0..self.rig.shared.parts.len() {
            let channel = self.rig.shared.parts[i].mix.channel;
            self.rig.inst.bend(BendEvent {
                channel,
                value: 8191,
            });
        }
    }

    /// Parts `parts` play TAPE `p` on `src`'s streamed row, one MIDI
    /// channel each; the CPU never refuses.
    fn new(st: Stream<'r>, src: OneSource, parts: &[(usize, TapeEngineParams)]) -> Played<'r> {
        let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
        for &(q, p) in parts {
            *rig.shared.parts[q].params = init_params(EngineType::Tape);
            rig.shared.parts[q].params.tape = p;
            // No routes: a bill's reach is the bend's alone.
            *rig.shared.parts[q].mod_state = ModState::from_registry(&ModDestRegistry::new(), 8);
            rig.shared.parts[q].source = Some(src.id);
        }
        Played {
            rig,
            st,
            src,
            short: None,
        }
    }

    /// `f` with this block's view and the voices' ports.
    fn ported<R>(
        &mut self,
        f: impl FnOnce(&mut InstRig, &GateView<'_>, &mut Streams<'_, '_>) -> R,
    ) -> R {
        let (frames, short) = (self.st.card.frames(), self.short);
        let lookup = move |id: SourceId| {
            Some(if Some(id) == short {
                chimera_core::stream::units::Frames::new(100)
            } else {
                frames
            })
        };
        let view = self.src.view();
        let mut s = Streams {
            audio: &mut self.st.audio,
            card: StreamCard {
                frames: &lookup,
                now: AudioBlocks::new(self.st.now),
                bus: &self.st.card.bus,
                stats: &self.st.card.stats,
                pend: || {},
                loud: Default::default(),
            },
        };
        f(&mut self.rig, &view, &mut s)
    }

    fn note(&mut self, part: usize, note: u8, kind: NoteKind) {
        self.ported(|rig, view, s| {
            let ev = NoteEvent {
                channel: rig.shared.parts[part].mix.channel,
                note: MidiNote::new(note).unwrap(),
                kind,
            };
            rig.inst
                .handle_streamed(ev, &rig.shared, NoteOrigin::Live, view, s);
        });
    }

    fn on(&mut self, part: usize, note: u8) {
        self.note(part, note, NoteKind::On(Velocity::DEFAULT));
    }

    fn off(&mut self, part: usize, note: u8) {
        self.note(part, note, NoteKind::Off);
    }

    fn block(&mut self) {
        self.st.serve();
        let now = Ms(self.st.now);
        self.ported(|rig, view, s| {
            rig.inst.render_streamed(
                &mut rig.fx,
                &mut rig.dac,
                &rig.shared,
                &mut rig.scope,
                view,
                s,
            );
            s.answer_quiet(now);
        });
        self.st.now += 1;
    }

    /// The voice booked for `note` on Part `part`.
    fn voice_of(&self, part: usize, note: u8) -> Option<usize> {
        self.rig
            .inst
            .allocator()
            .slots()
            .iter()
            .position(|s| s.part() == Some(PartId::ALL[part as usize]) && s.note().map(MidiNote::get) == Some(note))
    }

    fn booked(&self) -> usize {
        let slots = self.rig.inst.allocator().slots();
        slots.iter().filter(|s| !s.is_free()).count()
    }

    /// What the voices' bills sum to.
    fn billed(&self) -> u32 {
        self.rig
            .inst
            .stream_bills()
            .iter()
            .flatten()
            .map(|b| b.get())
            .sum()
    }
}

/// WALK ONE at FADE HOLD from START 0: note-off doesn't end it.
fn one_hold() -> TapeEngineParams {
    TapeEngineParams {
        drag: FADE_HOLD,
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Forward, 0.0, 1.0)
    }
}

/// What a note-on of `note` bills on `p` with only the bend's range, and
/// wow while TAPE runs.
fn bill_of(p: &TapeEngineParams, note: u8, bend: u8) -> u32 {
    let wow = if tape_runs(p.tape, false) {
        WOW_CENTS
    } else {
        Cents::ZERO
    };
    let n = MidiNote::new(note).unwrap();
    bill(
        HZ,
        ratio_max(p, n, wow, Cents::new(f32::from(bend) * 100.0)),
    )
    .get()
}

/// Eight notes that fit the card's budget; a ninth that doesn't, unless a
/// voice goes. The oldest released goes (umbrella open question 2), not
/// the allocator's oldest note-on: 50 started first and let go last.
#[test]
fn ninth_stream_steals_oldest_released() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    // TAPE runs: wow billed.
    let p = TapeEngineParams {
        tape: 0.2,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    pl.wheel_up();
    let bend = pl.rig.shared.parts[0].mix.bend;
    let notes = 50..58u8;
    let eight: u32 = notes.clone().map(|n| bill_of(&p, n, bend)).sum();
    let ninth = bill_of(&p, 63, bend);
    assert!(eight <= STREAM_BUDGET_BPS.get() && eight + ninth > STREAM_BUDGET_BPS.get());
    assert!(eight - bill_of(&p, 51, bend) + ninth <= STREAM_BUDGET_BPS.get());

    for n in notes.clone() {
        pl.on(0, n);
    }
    assert_eq!(pl.booked(), 8);
    assert_eq!(pl.billed(), eight);
    for k in 0..60 {
        match k {
            10 => pl.off(0, 51),
            40 => pl.off(0, 50),
            _ => {}
        }
        pl.block();
    }
    let (first, early) = (pl.voice_of(0, 50).unwrap(), pl.voice_of(0, 51).unwrap());
    pl.on(0, 63);
    let slots = pl.rig.inst.allocator().slots();
    assert_eq!(pl.voice_of(0, 63), Some(early), "51, let go first, goes");
    assert_eq!(slots[first].note().map(MidiNote::get), Some(50));
    assert!(!slots[first].dying());
    for _ in 0..10 {
        pl.block();
        assert!(
            pl.billed() <= STREAM_BUDGET_BPS.get(),
            "{} over",
            pl.billed()
        );
    }
    assert!(
        pl.rig.inst.active().iter().all(|&a| a),
        "eight streams play"
    );
}

/// The stolen voice, alone on its Part's bus, fades out through the
/// allocator's fade, then the note that took it plays: no step passes 4×
/// the steady state's largest (`click_free_test`'s rule).
#[test]
fn steal_fades_through_allocator() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = one_hold();
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p), (1, p)]);
    pl.on(1, 50);
    for n in 51..58 {
        pl.on(0, n);
    }
    let victim = pl.voice_of(1, 50).unwrap();
    let mut bus = Vec::new();
    for k in 0..100 {
        if k == 5 {
            pl.off(1, 50);
        }
        if k == 60 {
            pl.on(0, 63);
            assert_eq!(pl.voice_of(0, 63), Some(victim), "the steal takes it");
        }
        pl.block();
        bus.extend_from_slice(pl.rig.inst.part_bus(1));
    }
    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    let steady = step(&bus[10 * BLOCK_SIZE..60 * BLOCK_SIZE]);
    assert!(steady > 1e-3, "the victim sounds");
    let worst = step(&bus[60 * BLOCK_SIZE..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
    // Gone within the voice's fade and a block, and silent after.
    assert!(bus[64 * BLOCK_SIZE..].iter().all(|&y| y == 0.0));
    assert!(pl.rig.inst.active()[victim], "63 plays on it");
}

/// The machines that seek, and PING, refuse a streamed source before the
/// allocator is asked; ONE, LOOP and TRON forward play it (#698).
#[test]
fn ping_on_streamed_source_refused_before_allocation() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    for (way, plays) in [
        (Way::One, true),
        (Way::Loop, true),
        (Way::Ping, false),
        (Way::Scrub, false),
        (Way::Stretch, false),
        (Way::Wave, false),
        (Way::Tron, true),
    ] {
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let p = params(Direction::Loop, 0.0, 1.0).with_way(way);
        let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
        let mut pl = Played::new(st, src, &[(0, p)]);
        pl.on(0, 60);
        assert_eq!(pl.booked(), usize::from(plays), "{way:?}");
        assert_eq!(pl.rig.inst.allocator().refused(), 0, "never asked");
        pl.block();
        assert_eq!(pl.rig.inst.active()[0], plays, "{way:?}");
    }
}

/// END before START reverses ONE and LOOP: a stream can't read back yet.
#[test]
fn reverse_on_streamed_refused() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    for (mode, (start, end)) in [
        (Direction::Forward, (0.9, 0.1)),
        (Direction::Loop, (0.9, 0.1)),
        (Direction::Reverse, (0.1, 0.9)),
    ] {
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let p = params(mode, start, end);
        let src = OneSource::new(&st.card.held(&[0, 0], HF));
        let mut pl = Played::new(st, src, &[(0, p)]);
        pl.on(0, 60);
        assert_eq!(pl.booked(), 0, "{mode:?}");
    }
}

/// MODE turned to PING under a sounding LOOP: that note plays on, the
/// next refuses.
#[test]
fn mode_changed_to_ping_refuses_later_notes() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = params(Direction::Loop, 0.0, 1.0);
    let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    pl.on(0, 60);
    for _ in 0..10 {
        pl.block();
    }
    pl.rig.shared.parts[0].params.tape.set_way(Way::Ping);
    pl.on(0, 64);
    assert_eq!(pl.booked(), 1);
    assert_eq!(pl.voice_of(0, 64), None);
    for _ in 0..10 {
        pl.block();
    }
    assert!(
        pl.rig.inst.active()[pl.voice_of(0, 60).unwrap()],
        "60 plays on"
    );
}

/// #585: a streamed note's cap covers every speed it can reach mid-note, a
/// PITCH route at full swing, the wheel at the bend's range, STEAL GLIDE up
/// an octave and back, MOTION walking the loop and the wow, so the deck
/// never caps its pitch: the speed it wants never passes `Plan::inc_max`.
#[test]
fn streamed_speed_stays_under_its_cap() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    for wow in [Wow::Capstan, Wow::Warble] {
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let p = TapeEngineParams {
            motion: 1.0,
            tape: 1.0,
            wow,
            ..params(Direction::Loop, 0.0, 1.0)
        };
        let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 48), HF));
        let mut pl = Played::new(st, src, &[(0, p)]);
        {
            let part = &mut pl.rig.shared.parts[0];
            part.mix.set_mode(PartMode::Mono);
            part.params.pitch.steal = Steal::Glide;
            let mut ms = (*part.mod_state).clone();
            let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
            let d = ms.find(pitch).or_else(|| ms.push(pitch)).unwrap();
            ms.set_route(ModSource::Lfo1.index(), d, 127);
            *part.mod_state = ms;
        }
        for k in 0..400 {
            match k {
                0 => pl.on(0, 48),
                100 => pl.on(0, 60),
                150 => pl.rig.inst.bend(BendEvent {
                    channel: pl.rig.shared.parts[0].mix.channel,
                    value: 8191,
                }),
                250 => pl.on(0, 50),
                _ => {}
            }
            pl.block();
            let v = pl
                .rig
                .inst
                .allocator()
                .slots()
                .iter()
                .position(|s| !s.is_free());
            let EngineSlot::Tape(t) = pl.rig.inst.voice(v.unwrap()).slot() else {
                panic!("a TAPE voice");
            };
            let (wants, cap) = t.speed().expect("a deck");
            assert!(
                wants <= cap * 1.000_01,
                "{wow:?} block {k}: {wants} over {cap}"
            );
            // The retrigger's glide is admitted mid-note: the cap is paid for.
            let (_, cap, paid) = speeds(&pl, v.unwrap());
            assert!(
                (cap - paid).abs() <= paid * 1e-4,
                "{wow:?} block {k}: {cap} {paid}"
            );
        }
    }
}

/// A streamed note books WALK's bill and the stream's term, and keeps it
/// once the voices are re-costed each block.
#[test]
fn streamed_note_bills_its_stream_term() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = one_hold();
    let cost = |streamed: bool| {
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let row = if streamed {
            st.card.held(&[0], HF)
        } else {
            held_plain(&sine(RATE, 300.0, 24_000), RATE)
        };
        let src = OneSource::new(&row);
        let mut pl = Played::new(st, src, &[(0, p)]);
        pl.on(0, 60);
        let on = pl.rig.inst.allocator().slots()[0].cost();
        pl.block();
        (on, pl.rig.inst.allocator().slots()[0].cost())
    };
    let ((ram_on, ram), (on, played)) = (cost(false), cost(true));
    assert_eq!(on.0, ram_on.0 + COST_STREAM.0);
    assert_eq!(played.0, ram.0 + COST_STREAM.0);
}

/// A held key on a HELD RE-SNAP Part glides to the next chord's tone when
/// ORBIT's chord changes: its bill covers that reach from the note-on, the
/// cap is what it pays for, and the pitch follows the chord, never capped.
#[test]
fn resnapped_note_bill_covers_its_cap() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = params(Direction::Loop, 0.0, 1.0);
    let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 64), HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    pl.rig.shared.parts[0].mix.snap = LiveSnap::Chord;
    pl.rig.shared.parts[0].mix.held = Held::Resnap;
    pl.rig
        .inst
        .set_harmony(Some(chord(PitchClass::A, Quality::Min)));
    pl.on(0, 66);
    let v = pl.voice_of(0, 64).expect("66 snaps to E4");
    let mut before = 0.0;
    for k in 0..80 {
        if k == 20 {
            pl.rig
                .inst
                .set_harmony(Some(chord(PitchClass::G, Quality::Maj)));
            pl.rig.inst.resnap(&pl.rig.shared);
            assert_eq!(pl.voice_of(0, 67), Some(v), "it re-snaps to G4");
        }
        pl.block();
        let (wants, cap, paid) = speeds(&pl, v);
        assert!(
            (cap - paid).abs() <= paid * 1e-4,
            "block {k}: cap {cap}, paid {paid}"
        );
        assert!(wants <= cap * 1.000_01, "block {k}: {wants} over {cap}");
        if k == 19 {
            before = wants;
        }
    }
    let (wants, ..) = speeds(&pl, v);
    let st = (wants / before).log2() * 12.0;
    assert!((st - 3.0).abs() < 0.2, "E4 to G4: {st} st");
}

fn chord(root: PitchClass, q: Quality) -> Harmony {
    Harmony {
        key: Key::new(PitchClass::A, Scale::MINOR),
        chord: Chord::new(root, q),
    }
}

/// Voice `v`'s speed, cap and paid ratio, all as read ratios.
fn speeds(pl: &Played<'_>, v: usize) -> (f32, f32, f32) {
    let EngineSlot::Tape(t) = pl.rig.inst.voice(v).slot() else {
        panic!("a TAPE voice");
    };
    let (wants, cap) = t.speed().expect("a streamed deck");
    let paid = pl.rig.inst.stream_bills()[v].unwrap().get() as f32;
    let k = SR as f32 / f32::from(RATE);
    (
        wants * k,
        cap * k,
        paid / (f32::from(RATE) * 1.5 * 4_608.0 / 4_032.0),
    )
}

/// The voice booked for `note` on Part `part`, and its deck's speeds.
fn sounding(pl: &Played<'_>, note: u8) -> usize {
    pl.voice_of(0, note).expect("booked")
}

/// #595 (a): a held streamed note's reach rises mid-note, PITCH, TUNING,
/// the bend's range, a route's depth or SNAP turned on: its bill is run
/// again and admitted, its cap is what it pays, and the pitch follows the
/// knob, never capped.
#[test]
fn raised_reach_is_readmitted_and_followed() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    type Turn = fn(&mut chimera_core::instrument::AudioShared);
    // The pitch it then moves by, where the knob moves it.
    let turns: [(&str, Turn, Option<f32>); 5] = [
        ("pitch", |s| s.parts[0].params.pitch.pitch = 7.0, Some(7.0)),
        ("tuning", |s| s.tune = 50, Some(0.5)),
        (
            "bend range, the wheel up",
            |s| s.parts[0].mix.bend = 12,
            None,
        ),
        (
            "route",
            |s| {
                let mut ms = (*s.parts[0].mod_state).clone();
                let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
                let d = ms.find(pitch).or_else(|| ms.push(pitch)).unwrap();
                ms.set_route(ModSource::Lfo1.index(), d, 64);
                *s.parts[0].mod_state = ms;
            },
            None,
        ),
        ("snap", |s| s.parts[0].mix.snap = LiveSnap::Chord, None),
    ];
    for (what, turn, st_up) in turns {
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let p = TapeEngineParams {
            tape: 0.0,
            ..params(Direction::Loop, 0.0, 1.0)
        };
        let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
        let mut pl = Played::new(st, src, &[(0, p)]);
        pl.wheel_up();
        pl.on(0, 60);
        let v = sounding(&pl, 60);
        // The wheel's glide lands first.
        for _ in 0..200 {
            pl.block();
        }
        let (was, _, paid) = speeds(&pl, v);
        turn(&mut pl.rig.shared);
        for k in 0..60 {
            pl.block();
            let (wants, cap, now) = speeds(&pl, v);
            assert!(
                (cap - now).abs() <= now * 1e-4,
                "{what} {k}: cap {cap}, paid {now}"
            );
            assert!(wants <= cap * 1.000_01, "{what} {k}: {wants} over {cap}");
        }
        let (wants, _, now) = speeds(&pl, v);
        assert!(now > paid * 1.01, "{what}: paid {paid} then {now}");
        if let Some(up) = st_up {
            let st = (wants / was).log2() * 12.0;
            assert!((st - up).abs() < 0.05, "{what}: {st} st, not {up}");
        }
    }
}

/// Eight held streamed notes that fill the card, and two let go.
fn eight_held(released: &[u8]) -> (Played<'static>, u32) {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let (rings, third) = (Box::leak(rings), Box::leak(third));
    let st = Stream::new(rings, third, &whole, 0);
    let p = TapeEngineParams {
        tape: 0.0,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    for n in 50..58 {
        pl.on(0, n);
    }
    for _ in 0..10 {
        pl.block();
    }
    for &n in released {
        pl.off(0, n);
        pl.block();
    }
    let billed = pl.billed();
    (pl, billed)
}

/// #595: a knob turn never steals a held voice. Past what the card takes,
/// the rest hold at their caps, counted.
#[test]
fn a_knob_turn_never_steals_a_held_voice() {
    let (mut pl, _) = eight_held(&[]);
    pl.rig.shared.parts[0].params.pitch.pitch = 7.0;
    for _ in 0..20 {
        pl.block();
        assert!(
            pl.billed() <= STREAM_BUDGET_BPS.get(),
            "{} over",
            pl.billed()
        );
    }
    let slots = pl.rig.inst.allocator().slots();
    assert!(
        slots.iter().all(|s| s.held() && !s.dying()),
        "all eight held"
    );
    assert!(pl.rig.inst.active().iter().all(|&a| a), "all eight play");
    assert!(pl.st.card.stats.held_at_cap.load(Ordering::Relaxed) > 0);
    let capped = (0..8).filter(|&v| {
        let (wants, cap, _) = speeds(&pl, v);
        wants > cap * 1.001
    });
    assert!(capped.count() > 0, "one at least holds at its cap");
}

/// #595: a knob turn steals what it needs from released voices only, the
/// oldest first.
#[test]
fn a_knob_turn_steals_released_voices_only() {
    let (mut pl, _) = eight_held(&[50, 51]);
    let gone = [pl.voice_of(0, 50).unwrap(), pl.voice_of(0, 51).unwrap()];
    pl.rig.shared.parts[0].params.pitch.pitch = 6.0;
    for k in 0..20 {
        pl.block();
        // A stolen voice's bill counts until its fade ends.
        if k > 3 {
            assert!(
                pl.billed() <= STREAM_BUDGET_BPS.get(),
                "{} over",
                pl.billed()
            );
        }
    }
    let slots = pl.rig.inst.allocator().slots();
    for (v, s) in slots.iter().enumerate() {
        if !gone.contains(&v) {
            assert!(s.held() && !s.dying(), "voice {v} held on");
        }
    }
    assert!(
        slots[gone[0]].is_free() || slots[gone[0]].dying(),
        "50 went first"
    );
}

/// #595 (b): a note started while its source moves is admitted once it
/// reads, and plays from where tape time has reached, as the same note
/// started on time does.
#[test]
fn a_note_on_a_moving_source_plays_once_it_reads() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        tape: 0.0,
        age: 0.0,
        ..one_hold()
    };
    let run = |wait: usize| {
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let src = OneSource::new(&st.card.held(&[0], HF));
        let mut pl = Played::new(st, src, &[(0, p)]);
        if wait > 0 {
            pl.src.close();
        }
        pl.on(0, 60);
        let mut bus = Vec::new();
        for k in 0..80 {
            if k == wait && wait > 0 {
                assert_eq!(pl.billed(), 0, "unbilled while it moves");
                pl.src.open();
            }
            pl.block();
            bus.extend_from_slice(pl.rig.inst.part_bus(0));
        }
        assert!(pl.billed() > 0, "admitted once it reads");
        bus
    };
    let (on_time, late) = (run(0), run(10));
    assert!(late[..10 * BLOCK_SIZE].iter().all(|&y| y == 0.0));
    let worst = (20 * BLOCK_SIZE..late.len())
        .map(|i| (late[i] - on_time[i]).abs())
        .fold(0.0, f32::max);
    assert!(worst < 0.02, "{worst} off tape time");
}

/// A streamed note that waits on its moving source past its own end is
/// over when the gate opens (`catch_up`): no cursor starts on it, so the
/// ring never names its stream nor is asked for a slot.
#[test]
fn a_note_that_waited_past_its_end_starts_no_stream() {
    const SHORT: u32 = 48_000;
    let whole = held_plain(&sine(RATE, 300.0, SHORT), RATE);
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held(&[0], HF));
    let p = TapeEngineParams {
        tape: 0.0,
        age: 0.0,
        ..one_hold()
    };
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    let mut b = [0.0; BLOCK_SIZE];
    src.close();
    // At ROOT a 24 kHz source plays half a frame a sample: past its end
    // well inside this many blocks.
    let wait = 2 * SHORT as usize / BLOCK_SIZE + 20;
    for k in 0..wait {
        st.with(V, |port| {
            if k == 0 {
                e.start(60, &p, &src.view(), SR, Some(port));
            }
            e.play(&mut b, &p, &src.view(), SR, Some(port));
        });
        st.now += 1;
    }
    assert!(e.is_active() && !e.streams(), "gated");
    src.open();
    st.with(V, |port| e.play(&mut b, &p, &src.view(), SR, Some(port)));
    assert!(!e.is_active() && !e.streams(), "over on its first block");
    assert!(b.iter().all(|&y| y == 0.0));
    assert_eq!(rings.source(V), None, "no stream named on the ring");
    assert_eq!(rings.wants().count(), 0, "no slot asked");
}

/// #595 (b): one that doesn't fit when its source reads is refused, the
/// voice freed, counted; the held notes play on.
#[test]
fn a_late_note_that_doesnt_fit_is_refused() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = TapeEngineParams {
        tape: 0.0,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    pl.wheel_up();
    let bend = pl.rig.shared.parts[0].mix.bend;
    let held: u32 = (51..58).map(|n| bill_of(&p, n, bend)).sum();
    assert!(held + bill_of(&p, 72, bend) > STREAM_BUDGET_BPS.get());
    for n in 51..58 {
        pl.on(0, n);
    }
    pl.src.close();
    pl.on(0, 72);
    let late = pl.voice_of(0, 72).unwrap();
    for _ in 0..3 {
        pl.block();
    }
    pl.src.open();
    for _ in 0..10 {
        pl.block();
    }
    assert_eq!(pl.st.card.stats.late_refused.load(Ordering::Relaxed), 1);
    assert!(pl.rig.inst.allocator().slots()[late].is_free(), "freed");
    assert_eq!(pl.billed(), held);
    for n in 51..58 {
        assert!(
            pl.rig.inst.active()[pl.voice_of(0, n).unwrap()],
            "{n} plays on"
        );
    }
}

/// SLICE NOTE plays every key at ROOT: a glide moves no pitch, held whole
/// or streamed.
#[test]
#[ignore = "SLICE is on hold (ADR 0199, tape-slice, #748): no note plays it until it returns"]
fn a_slice_note_glide_holds_pitch() {
    for streamed in [false, true] {
        let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let p = TapeEngineParams {
            slice: SliceCode::NOTE,
            ..one_hold()
        };
        let row = if streamed {
            st.card.held(&[0], HF)
        } else {
            held_plain(&sine(RATE, 300.0, 24_000), RATE)
        };
        let src = OneSource::new(&row);
        let mut pl = Played::new(st, src, &[(0, p)]);
        pl.rig.shared.parts[0].mix.set_mode(PartMode::Mono);
        pl.rig.shared.parts[0].params.pitch.steal = Steal::Glide;
        pl.on(0, 60);
        for _ in 0..5 {
            pl.block();
        }
        pl.on(0, 67);
        let v = pl.voice_of(0, 67).expect("the glide takes 60's voice");
        for k in 0..20 {
            pl.block();
            let EngineSlot::Tape(t) = pl.rig.inst.voice(v).slot() else {
                panic!("a TAPE voice");
            };
            assert_eq!(t.slide(), 1.0, "streamed {streamed} block {k}");
        }
    }
}

/// #594: admission sheds a released voice for a note on its own Part,
/// whose SOURCE changed since; the note queues behind the shed's fade. The
/// fade reads its own file's length, so it fades rather than stops.
#[test]
fn a_fading_note_reads_its_own_source_length() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = TapeEngineParams {
        tape: 0.2,
        ..one_hold()
    };
    let held = st.card.held(&[0], HF);
    let (src, other) = OneSource::with(&held, &held);
    let mut pl = Played::new(st, src, &[(0, p), (1, p)]);
    pl.short = Some(other);
    pl.wheel_up();
    let bend = pl.rig.shared.parts[0].mix.bend;
    let eight: u32 = (50..58).map(|n| bill_of(&p, n, bend)).sum();
    assert!(
        eight <= STREAM_BUDGET_BPS.get() && eight + bill_of(&p, 63, bend) > STREAM_BUDGET_BPS.get()
    );
    pl.on(0, 57);
    for n in 50..57 {
        pl.on(1, n);
    }
    let victim = pl.voice_of(0, 57).unwrap();
    let mut bus = Vec::new();
    for k in 0..64 {
        match k {
            5 => pl.off(0, 57),
            60 => {
                pl.rig.shared.parts[0].source = Some(other);
                pl.on(0, 63);
                assert_eq!(pl.voice_of(0, 63), Some(victim), "the shed takes it");
            }
            _ => {}
        }
        pl.block();
        bus.extend_from_slice(pl.rig.inst.part_bus(0));
    }
    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    let steady = step(&bus[10 * BLOCK_SIZE..60 * BLOCK_SIZE]);
    assert!(steady > 1e-3, "the victim sounds");
    let worst = step(&bus[59 * BLOCK_SIZE..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
}

/// The voice's ring slots once it plays no stream.
fn ring_empty(pl: &mut Played<'_>, v: usize) -> bool {
    use chimera_core::stream::slot::SlotState;
    use chimera_core::stream::units::SlotIdx;
    let turn = pl.st.audio.turn();
    SlotIdx::ALL
        .iter()
        .all(|&s| turn.seen(VoiceIdx::ALL[v], s) == SlotState::Empty)
}

/// #586: a killed voice (a project load's fade) drops its ring's slots
/// once its fade ends.
#[test]
fn a_killed_voice_drops_its_slots() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, one_hold())]);
    pl.on(0, 60);
    let v = pl.voice_of(0, 60).unwrap();
    for _ in 0..20 {
        pl.block();
    }
    assert!(!ring_empty(&mut pl, v), "it streams");
    pl.rig.inst.kill_all();
    for _ in 0..6 {
        pl.block();
    }
    assert!(!pl.rig.inst.active()[v]);
    assert!(ring_empty(&mut pl, v), "every slot dropped");
}

/// #586: a voice whose next note plays another engine drops its slots
/// once the old note's fade ends.
#[test]
fn an_engine_change_drops_the_slots() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, one_hold())]);
    pl.on(0, 60);
    let v = pl.voice_of(0, 60).unwrap();
    for _ in 0..20 {
        pl.block();
    }
    assert!(!ring_empty(&mut pl, v), "it streams");
    *pl.rig.shared.parts[0].params = init_params(EngineType::Algo);
    pl.on(0, 60);
    assert_eq!(pl.voice_of(0, 60), Some(v), "the re-strike takes it");
    for _ in 0..6 {
        pl.block();
    }
    assert!(pl.rig.inst.active()[v], "ALGO plays on it");
    assert!(ring_empty(&mut pl, v), "every slot dropped");
}

/// A block with no port ends a streamed note there and its stream with it
/// (#697 H1): `streams` reads false, so the instrument drops the voice's
/// ring (`run_block`'s rule after each voice), with no cursor left to stop.
#[test]
fn a_portless_block_ends_the_stream() {
    use chimera_core::stream::slot::SlotState;
    use chimera_core::stream::units::SlotIdx;
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held(&[0], HF));
    let p = one_hold();
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    let mut b = [0.0; BLOCK_SIZE];
    for k in 0..20 {
        st.serve();
        st.with(V, |port| {
            if k == 0 {
                e.start(60, &p, &src.view(), SR, Some(port));
            }
            e.play(&mut b, &p, &src.view(), SR, Some(port));
        });
        st.now += 1;
    }
    let empty = |st: &mut Stream<'_>| {
        let turn = st.audio.turn();
        SlotIdx::ALL
            .iter()
            .all(|&s| turn.seen(V, s) == SlotState::Empty)
    };
    assert!(e.streams() && e.is_active(), "it streams");
    assert!(!empty(&mut st), "its ring holds slots");
    e.play(&mut b, &p, &src.view(), SR, None);
    assert!(b.iter().all(|&y| y == 0.0), "nothing plays");
    assert!(
        !e.is_active() && !e.streams(),
        "the note and its stream end"
    );
    if !e.streams() {
        let mut turn = st.audio.turn();
        for s in SlotIdx::ALL {
            turn.cancel(V, s);
        }
    }
    assert!(empty(&mut st), "the ring dropped");
}

/// A shell's whole block through the voices' ports (`run_block_ported`):
/// a note the drain hands it starts streamed on its port, and the render
/// reads on through it.
#[test]
fn a_ported_block_starts_and_plays_a_streamed_note() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, one_hold())]);
    let mut heard = 0.0f32;
    for k in 0..20 {
        pl.st.serve();
        let frames = pl.st.card.frames();
        let lookup = move |_: SourceId| Some(frames);
        let mut s = Streams {
            audio: &mut pl.st.audio,
            card: StreamCard {
                frames: &lookup,
                now: AudioBlocks::new(pl.st.now),
                bus: &pl.st.card.bus,
                stats: &pl.st.card.stats,
                pend: || {},
                loud: Default::default(),
            },
        };
        let rig = &mut pl.rig;
        let channel = rig.shared.parts[0].mix.channel;
        rig.inst.run_block_ported(
            pl.src.token(),
            (&mut rig.fx, &mut rig.dac, &mut rig.scope),
            &rig.shared,
            &mut s,
            |inst, reads, ports| {
                if k == 0 {
                    let ev = NoteEvent {
                        channel,
                        note: MidiNote::new(60).unwrap(),
                        kind: NoteKind::On(Velocity::DEFAULT),
                    };
                    inst.handle_streamed(ev, &rig.shared, NoteOrigin::Live, reads, ports);
                }
            },
        );
        pl.st.now += 1;
        heard = heard.max(rig.inst.part_bus(0).iter().fold(0.0, |m, y| m.max(y.abs())));
    }
    assert_eq!(pl.rig.inst.streaming().map(|c| c.get()), Some(1));
    assert!(heard > 0.1, "it plays: {heard}");
    assert!(pl.st.card.claims() > 0, "read through its ring");
}

/// One run of `quiet_fades_and_returns`: a streamed note on Part 0, ALGO
/// on Part 1; with `quiet`, a write's quiet asked at block `ASK` and
/// settled at `OFF`. Each block's Part 0 and Part 1 buses, and the block
/// the quiet was held.
fn quiet_run(quiet: bool) -> (Vec<f32>, Vec<f32>, Option<u32>) {
    const ASK: u32 = 100;
    const OFF: u32 = 200;
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, one_hold())]);
    *pl.rig.shared.parts[1].params = init_params(EngineType::Algo);
    pl.on(0, 60);
    pl.on(1, 64);
    let (mut tape, mut ram, mut held) = (Vec::new(), Vec::new(), None);
    for k in 0..400 {
        let bus = &pl.st.card.bus;
        if quiet && k == ASK {
            bus.ask_quiet(Ms(k));
        }
        if quiet && k == OFF {
            assert!(bus.settle(Ms(k)).is_some(), "settles");
        }
        pl.block();
        if held.is_none() && pl.st.card.bus.quiet() == Quiet::Held {
            held = Some(k);
        }
        tape.extend_from_slice(pl.rig.inst.part_bus(0));
        ram.extend_from_slice(pl.rig.inst.part_bus(1));
    }
    (tape, ram, held)
}

/// Decision 8: a write's quiet fades the streamed voice to 0 within
/// `QUIET_FADE` and two blocks, and the audio holds it; the RAM voice is
/// bit-identical to a run without quiet; on release the stream re-asks
/// from where tape time reached and fades back in, click-free, onto the
/// run without quiet.
#[test]
fn quiet_fades_and_returns() {
    let (tape, ram, held) = quiet_run(true);
    let (tape0, ram0, _) = quiet_run(false);
    assert!(ram0.iter().any(|&y| y != 0.0), "ALGO plays");
    assert_eq!(ram, ram0, "the RAM voice untouched");

    let b = BLOCK_SIZE;
    let by = 100 + QUIET_FADE.get() as usize / b + 2;
    let held = held.expect("held") as usize;
    let faded = 100 + QUIET_FADE.get() as usize / b;
    assert!(
        (faded..by).contains(&held),
        "held at block {held}, not once faded"
    );
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, y| m.max(y.abs()));
    // The deck's gain is 0 by `by`: what's left is the voice chain's own
    // tail, 40 dB down and only falling.
    let playing = peak(&tape[99 * b..100 * b]);
    assert!(playing > 0.1, "it played");
    let tail: Vec<f32> = (by..200).map(|k| peak(&tape[k * b..(k + 1) * b])).collect();
    assert!(tail[0] < playing / 100.0, "{} at block {by}", tail[0]);
    assert!(
        tail.windows(2).all(|t| t[1] <= t[0]),
        "only falling: {tail:?}"
    );

    let step = |x: &[f32]| {
        x.windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0, f32::max)
    };
    let steady = step(&tape0[50 * b..100 * b]);
    let worst = step(&tape[60 * b..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
    // Back where tape time reached: the run without quiet, once faded in.
    // A frame off would differ by ~0.1 (300 Hz at 24 kHz); the chain's
    // settling leaves ~1e-3.
    let off = tape[300 * b..]
        .iter()
        .zip(&tape0[300 * b..])
        .fold(0.0f32, |m, (y, y0)| m.max((y - y0).abs()));
    assert!(off < 1e-2, "off the unquieted run by {off}");
}

/// One run of `fault_starves_and_recovers`: a streamed note on Part 0 and
/// ALGO on Part 1; the card's next `fails` reads fail from block 100. Once
/// the card faults, a streamed note-on on Part 2, then the fault cleared
/// (the UI's re-init and mount) `CLEAR` blocks later and the note again.
struct FaultRun {
    tape: Vec<f32>,
    ram: Vec<f32>,
    fault_at: Option<usize>,
    /// Voices booked before and after the note-on under the fault.
    booked: (usize, usize),
    refused: u32,
    /// Part 2's note after the clear plays.
    again: bool,
    claims: u32,
    underruns: u32,
}

const CLEAR: usize = 100;

fn fault_run(fails: u32) -> FaultRun {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, one_hold()), (2, one_hold())]);
    *pl.rig.shared.parts[1].params = init_params(EngineType::Algo);
    pl.on(0, 60);
    pl.on(1, 64);
    let mut run = FaultRun {
        tape: Vec::new(),
        ram: Vec::new(),
        fault_at: None,
        booked: (0, 0),
        refused: 0,
        again: false,
        claims: 0,
        underruns: 0,
    };
    for k in 0..600 {
        if k == 100 {
            pl.st.card.fail_next(fails);
        }
        match run.fault_at.map(|f| k - f) {
            Some(20) => {
                run.booked.0 = pl.booked();
                pl.on(2, 67);
                run.booked.1 = pl.booked();
                run.refused = pl.rig.inst.allocator().refused();
            }
            Some(CLEAR) => pl.st.card.bus.set_fault(CardFault::Clear),
            Some(d) if d == CLEAR + 20 => pl.on(2, 67),
            Some(d) if d == CLEAR + 60 => {
                run.again = pl.rig.inst.streaming().map(|c| c.get()) == Some(2)
            }
            _ => {}
        }
        pl.block();
        if run.fault_at.is_none() && pl.st.card.bus.fault() == CardFault::Read {
            run.fault_at = Some(k);
        }
        run.tape.extend_from_slice(pl.rig.inst.part_bus(0));
        run.ram.extend_from_slice(pl.rig.inst.part_bus(1));
    }
    run.claims = pl.st.card.claims();
    run.underruns = pl.st.card.stats.underruns.load(Ordering::Relaxed);
    run
}

fn worst_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|d| (d[1] - d[0]).abs())
        .fold(0.0, f32::max)
}

/// C1, I2: a read that fails twice (a retry, then the fault) drops every
/// stream out over `DROP_FADE`; a streamed note-on under the fault is
/// refused before a voice is booked; the RAM voice plays on untouched;
/// the fault cleared, the stream fades back in where tape time reached,
/// and new streamed notes play. Nothing steps more than 4× steady state.
#[test]
fn fault_starves_and_recovers() {
    let r = fault_run(2);
    let ok = fault_run(0);
    assert_eq!(ok.fault_at, None);
    let at = r.fault_at.expect("the card faulted");
    assert!(at > 100, "only after the reads failed");
    assert_eq!(r.ram, ok.ram, "the RAM voice untouched");

    let b = BLOCK_SIZE;
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, y| m.max(y.abs()));
    let playing = peak(&r.tape[(at - 1) * b..at * b]);
    assert!(playing > 0.1, "it played");
    // The deck's gain is 0 within `DROP_FADE` and a block: what's left is
    // the voice chain's own tail, only falling.
    let by = at + 1 + DROP_FADE.get() as usize / b + 1;
    let tail: Vec<f32> = (by..at + CLEAR)
        .map(|k| peak(&r.tape[k * b..(k + 1) * b]))
        .collect();
    assert!(tail[0] < playing / 10.0, "{} at block {by}", tail[0]);
    assert!(tail.windows(2).all(|t| t[1] <= t[0]), "only falling");

    assert_eq!(r.booked.1, r.booked.0, "refused before allocation");
    assert!(r.refused > 0, "counted");
    assert!(r.again, "a streamed note plays once cleared");

    let steady = worst_step(&ok.tape[50 * b..100 * b]);
    let worst = worst_step(&r.tape[60 * b..]);
    assert!(worst <= 4.0 * steady, "step {worst} over 4 x {steady}");
    // Back where tape time reached: the run without a fault, once the
    // chain settles (`quiet_fades_and_returns`'s bound).
    let back = (at + CLEAR + 100) * b;
    let off = r.tape[back..(at + CLEAR + 150) * b]
        .iter()
        .zip(&ok.tape[back..])
        .fold(0.0f32, |m, (y, y0)| m.max((y - y0).abs()));
    assert!(off < 1e-2, "off the run without a fault by {off}");
}

/// One CRC error is read again at once: no fault, no drop-out, the same
/// output bit for bit, one claim more.
#[test]
fn one_crc_error_retries_silently() {
    let r = fault_run(1);
    let ok = fault_run(0);
    assert_eq!(r.fault_at, None, "no fault");
    assert_eq!(r.underruns, 0);
    assert_eq!(r.claims, ok.claims + 1, "read again");
    assert_eq!(r.tape, ok.tape);
}

/// A chunk that won't de-frame is counted and faults the card, unretried.
#[test]
fn frame_error_counts() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, one_hold())]);
    pl.on(0, 60);
    for k in 0..300 {
        if k == 100 {
            pl.st.card.garble_next();
        }
        pl.block();
    }
    let s = &pl.st.card.stats;
    assert_eq!(s.frame_errors.load(Ordering::Relaxed), 1);
    assert_eq!(pl.st.card.bus.fault(), CardFault::Read);
}

/// #609 (I1): a LOOP bed over a cluster of 9 rotted chunks, for several
/// laps. The card is blamed for the first two failures (each a fault and a
/// re-init); their second failures condemn the file, and from then on no
/// lap faults or reads a bad chunk again: the voice drops out over them
/// and comes back each lap.
#[test]
fn a_rotted_cluster_in_a_loop_faults_twice_then_never() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        size: 1.0,
        position: 0.0,
        xfade: 0.0,
        drag: FADE_HOLD,
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Loop, 0.0, 0.05)
    };
    let ws = window_start(p.start, FILE);
    let we = window_end(ws, p.end, FRAMES, HZ);
    let b = Bounds::of(ws, we, sizing(&p), HZ);
    let lap = (b.frames() / 32) as usize;
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    st.card.rot_chunks(2..11);
    let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    pl.on(0, 60);
    let mut faults = Vec::new();
    let mut cleared_at = None;
    let mut level = Vec::new();
    for k in 0..5 * lap {
        let was = pl.st.card.bus.fault();
        pl.block();
        if was == CardFault::Clear && pl.st.card.bus.fault() == CardFault::Read {
            faults.push(k);
            cleared_at = Some(k + 10);
        }
        // The UI's re-init and mount, a few blocks on.
        if cleared_at == Some(k) {
            pl.st.card.bus.set_fault(CardFault::Clear);
        }
        let x = pl.rig.inst.part_bus(0);
        level.push(x.iter().fold(0.0f32, |m, y| m.max(y.abs())));
    }
    let stats = &pl.st.card.stats;
    eprintln!(
        "DBG lap {lap} faults {faults:?} fe {} un {}",
        stats.frame_errors.load(Ordering::Relaxed),
        stats.unreadable.load(Ordering::Relaxed)
    );
    assert_eq!(
        faults.len(),
        2,
        "two suspects, then the verdict: {faults:?}"
    );
    assert!(faults[1] < lap, "both in the first lap");
    assert_eq!(
        stats.frame_errors.load(Ordering::Relaxed),
        4,
        "each read twice"
    );
    assert_eq!(pl.st.card.bus.fault(), CardFault::Clear);
    // Every later lap still plays (its good chunks and heads) and drops out.
    for l in 2..5 {
        let w = &level[l * lap..(l + 1) * lap];
        assert!(w.iter().any(|&x| x > 0.1), "lap {l} plays");
        assert!(w.iter().any(|&x| x < 0.05), "lap {l} drops out");
    }
}

/// M1 (#761): a LOOP bed over a bad sector, whose every read answers an
/// error token, for several laps. The card faults once for it (the token
/// read again, then the fault and the UI's re-init and mount); its next
/// failure, after the re-init, is the chunk's: passed over, so no later
/// lap faults or remounts again, and the voice drops out over it and
/// comes back each lap.
#[test]
fn a_bad_sector_in_a_loop_faults_once_then_never() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let p = TapeEngineParams {
        size: 1.0,
        position: 0.0,
        xfade: 0.0,
        drag: FADE_HOLD,
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Loop, 0.0, 0.05)
    };
    let ws = window_start(p.start, FILE);
    let we = window_end(ws, p.end, FRAMES, HZ);
    let b = Bounds::of(ws, we, sizing(&p), HZ);
    let lap = (b.frames() / 32) as usize;
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    st.card.bad_sectors(4..5);
    let src = OneSource::new(&st.card.held(&heads_at(&p, FRAMES, 60), HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    pl.on(0, 60);
    let (mut remounts, mut cleared_at) = (Vec::new(), None);
    let mut level = Vec::new();
    for k in 0..5 * lap {
        let was = pl.st.card.bus.fault();
        pl.block();
        if was == CardFault::Clear && pl.st.card.bus.fault() == CardFault::Read {
            cleared_at = Some(k + 10);
        }
        // The UI's re-init and mount, a few blocks on.
        if cleared_at == Some(k) {
            pl.st.card.bus.set_fault(CardFault::Clear);
            remounts.push(k);
        }
        let x = pl.rig.inst.part_bus(0);
        level.push(x.iter().fold(0.0f32, |m, y| m.max(y.abs())));
    }
    let stats = &pl.st.card.stats;
    assert_eq!(remounts.len(), 1, "one fault and remount: {remounts:?}");
    assert!(remounts[0] < lap, "in the first lap");
    assert_eq!(stats.frame_errors.load(Ordering::Relaxed), 0);
    assert!(stats.unreadable.load(Ordering::Relaxed) > 0, "passed over");
    assert_eq!(pl.st.card.bus.fault(), CardFault::Clear);
    for l in 2..5 {
        let w = &level[l * lap..(l + 1) * lap];
        assert!(w.iter().any(|&x| x > 0.1), "lap {l} plays");
        assert!(w.iter().any(|&x| x < 0.05), "lap {l} drops out");
    }
}

/// #609: a rotted record faults the card the first time it fails to
/// de-frame (the card may have sent the wrong blocks); once the re-init
/// clears that, the same chunk failing again is the file's: no second
/// fault, counted, passed over. Only the voice reading it drops out over
/// the bad chunk and comes back past it; another stream plays on.
#[test]
fn a_rotted_chunk_drops_out_its_voice_not_the_card() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    st.card.rot_chunks(4..5);
    let src = OneSource::new(&st.card.held(&[0], HF));
    let far = TapeEngineParams {
        start: 0.5,
        ..one_hold()
    };
    let mut pl = Played::new(st, src, &[(0, one_hold()), (2, far)]);
    pl.on(0, 60);
    pl.on(2, 60);
    let (mut faults, mut fault_at, mut cleared) = (0, None, None);
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, y| m.max(y.abs()));
    let (mut near, mut far_out) = (Vec::new(), Vec::new());
    for k in 0..900 {
        let was = pl.st.card.bus.fault();
        pl.block();
        if was == CardFault::Clear && pl.st.card.bus.fault() == CardFault::Read {
            faults += 1;
            fault_at.get_or_insert(k);
        }
        // The UI's re-init and mount, a few blocks on.
        if fault_at.is_some_and(|f| k == f + 10) {
            pl.st.card.bus.set_fault(CardFault::Clear);
            cleared = Some(k);
        }
        near.push(peak(pl.rig.inst.part_bus(0)));
        far_out.push(peak(pl.rig.inst.part_bus(2)));
    }
    let stats = &pl.st.card.stats;
    assert_eq!(faults, 1, "the card faults once, for the first failure");
    assert_eq!(stats.frame_errors.load(Ordering::Relaxed), 2);
    assert!(stats.unreadable.load(Ordering::Relaxed) > 0, "passed over");
    assert_eq!(pl.st.card.bus.fault(), CardFault::Clear);
    // After the clear both play; then the near voice drops out over the
    // rotted chunk (to the voice chain's falling tail) while the far one
    // plays on, and the near one returns.
    let back = cleared.unwrap() + 30;
    let gap: Vec<usize> = (back..near.len()).filter(|&k| near[k] < 0.05).collect();
    assert!(
        gap.len() > 50,
        "dropped out over the chunk: {} blocks",
        gap.len()
    );
    let (&g0, &g1) = (gap.first().expect("a drop-out"), gap.last().unwrap());
    assert!(near[back] > 0.1, "playing again after the clear");
    // A few blocks of fade in, then playing to the end.
    assert!(
        near[g1 + 5..].iter().all(|&p| p > 0.1),
        "back past the chunk"
    );
    assert!(g1 + 5 < near.len() - 50, "back well before the end");
    assert!(
        far_out[g0..=g1].iter().all(|&p| p > 0.1),
        "the other stream plays through"
    );
}

/// #698 finding 1: INIT's reach in use is the note's own. Its wheel
/// vibrato (LFO 1 → PITCH at DEPTH 0) reaches nothing, the wheel at rest
/// bends nothing, and TAPE at 0 adds no wow, so a seven-note cluster about
/// ROOT on a 24 kHz source (289 KB/s) streams whole; under the old bill
/// (the bend's range, the route at full swing and wow, ×1.17: 339 KB/s)
/// its last note stole the first.
#[test]
fn an_init_chord_about_root_streams_whole() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = TapeEngineParams {
        tape: 0.0,
        root: 6_000.0,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    let init = chimera_core::preset::Sound::init(EngineType::Tape);
    *pl.rig.shared.parts[0].mod_state = init.mod_state.clone();
    pl.rig.shared.parts[0].params.lfos = init.params.lfos;
    assert_eq!(pl.rig.shared.parts[0].params.lfos[0].depth, 0.0);
    for n in 57..64 {
        pl.on(0, n);
    }
    pl.block();
    assert_eq!(pl.booked(), 7);
    for n in 57..64 {
        let v = pl.voice_of(0, n).expect("booked");
        assert!(pl.rig.inst.active()[v], "{n} sounds");
    }
    assert!(pl.billed() <= STREAM_BUDGET_BPS.get());
}

/// #876: INIT's wheel is a route, WHEEL → LFO 1 DEPTH, so the reach
/// counts the wheel where it sits: a held note bills its vibrato once the
/// wheel goes up (`refresh_in`), and none of it at rest.
#[test]
fn inits_wheel_up_bills_the_vibrato() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = TapeEngineParams {
        tape: 0.0,
        root: 6_000.0,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    let init = chimera_core::preset::Sound::init(EngineType::Tape);
    *pl.rig.shared.parts[0].mod_state = init.mod_state.clone();
    pl.rig.shared.parts[0].params.lfos = init.params.lfos;
    pl.on(0, 60);
    pl.block();
    let rest = pl.billed();
    assert!(rest > 0);
    pl.rig.inst.expr(chimera_core::note_queue::ExprEvent {
        channel: chimera_core::MidiChannel::new(0).unwrap(),
        expr: chimera_core::note_queue::Expr::Wheel(chimera_midi::value::U7::new(127).unwrap()),
    });
    for _ in 0..200 {
        pl.block();
    }
    let up = pl.billed();
    assert!(up > rest, "{rest} → {up}");
    // LFO 1 → PITCH at 2 at full DEPTH: ±0.38 semitone, about 2.2 %.
    let ratio = up as f32 / rest as f32;
    assert!((1.01..1.04).contains(&ratio), "{ratio}");
}

/// #698 finding 11: ONE's RETRIG CUT chokes the Part's other notes (a
/// closed hat cutting an open one); STACK lets them ring.
#[test]
fn retrig_cut_chokes_the_part() {
    for (retrig, rings_on) in [(Retrig::Stack, true), (Retrig::Cut, false)] {
        let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
        let (rings, third) = rings();
        let st = Stream::new(&rings, &third, &whole, 0);
        let p = TapeEngineParams {
            retrig,
            ..one_hold()
        };
        let src = OneSource::new(&st.card.held(&[0], HF));
        let mut pl = Played::new(st, src, &[(0, p)]);
        pl.on(0, 60);
        for _ in 0..10 {
            pl.block();
        }
        let open = pl.voice_of(0, 60).unwrap();
        pl.on(0, 62);
        for _ in 0..10 {
            pl.block();
        }
        assert_eq!(pl.rig.inst.active()[open], rings_on, "{retrig:?}");
        let closed = pl.voice_of(0, 62).unwrap();
        assert!(
            pl.rig.inst.active()[closed],
            "{retrig:?}: the new hit plays"
        );
    }
}

/// #706: a routed START (VEL → START) past END on a streamed ONE stays on
/// its side of END: the hit plays forward from END's side, every read
/// inside its span (`strict_spans`), and never clicks.
#[test]
fn a_routed_start_past_end_never_reverses_a_stream() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let stored = TapeEngineParams {
        tape: 0.0,
        age: 0.0,
        ..params(Direction::Forward, 0.3, 0.5)
    };
    let routed = TapeEngineParams {
        start: 0.7,
        ..stored
    };
    let (rings, third) = rings();
    let mut st = Stream::new(&rings, &third, &whole, 0);
    let mut src = OneSource::new(&st.card.held(&heads_at(&stored, FRAMES, 60), HF));
    let mut slot = MaybeUninit::uninit();
    let e = deck(&mut slot);
    e.strict_spans();
    let (mut out, mut b) = (Vec::new(), [0.0; BLOCK_SIZE]);
    for k in 0..300 {
        st.serve();
        st.with(V, |port| {
            if k == 0 {
                e.start(60, &stored, &src.view(), SR, Some(port));
            }
            e.play(&mut b, &routed, &src.view(), SR, Some(port));
            assert_eq!(e.dirs()[0], Dir::Fwd, "block {k}");
        });
        out.extend_from_slice(&b);
        st.now += 1;
    }
    // The sine's own slope at half speed, 2π · 0.5 / 300, bounds a step.
    assert!(out.iter().any(|y| y.abs() > 0.1), "the hit sounds");
    let worst = out
        .windows(2)
        .map(|d| (d[1] - d[0]).abs())
        .fold(0.0, f32::max);
    assert!(worst < 0.02, "a step of {worst}");
}

/// #706: the bill is the reach in use. INIT's wheel vibrato (LFO 1 →
/// PITCH, DEPTH 0) and the wheel at rest bill nothing past the note; DEPTH
/// up re-bills it by the route's swing, and the wheel up by the bend.
#[test]
fn the_live_reach_is_what_bills() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let p = TapeEngineParams {
        tape: 0.0,
        root: 6_000.0,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, p)]);
    let init = chimera_core::preset::Sound::init(EngineType::Tape);
    *pl.rig.shared.parts[0].mod_state = init.mod_state.clone();
    pl.rig.shared.parts[0].params.lfos = init.params.lfos;
    pl.on(0, 60);
    pl.block();
    let at_rest = pl.billed();
    assert_eq!(at_rest, bill_of(&p, 60, 0), "nothing past the note");
    pl.rig.shared.parts[0].params.lfos[0].depth = 1.0;
    pl.block();
    let vibrato = pl.billed();
    assert!(vibrato > at_rest, "DEPTH up re-bills: {vibrato}");
    pl.wheel_up();
    pl.block();
    let bend = pl.rig.shared.parts[0].mix.bend;
    assert!(pl.billed() > vibrato, "the wheel up re-bills");
    assert!(
        pl.billed() >= bill_of(&p, 60, bend),
        "the bend's range at the wheel's top"
    );
}

/// #706: RETRIG CUT cuts before its note is admitted: on a full card the
/// note takes the room of the note it chokes, never another Part's.
#[test]
fn a_cut_takes_the_room_it_frees() {
    let whole = held_plain(&sine(RATE, 300.0, FRAMES), RATE);
    let (rings, third) = rings();
    let st = Stream::new(&rings, &third, &whole, 0);
    let cut = TapeEngineParams {
        retrig: Retrig::Cut,
        root: 6_000.0,
        ..one_hold()
    };
    let stack = TapeEngineParams {
        root: 6_000.0,
        ..one_hold()
    };
    let src = OneSource::new(&st.card.held(&[0], HF));
    let mut pl = Played::new(st, src, &[(0, cut), (1, stack)]);
    // Part 2's notes first: the oldest, which a steal would take.
    let others: Vec<u8> = (56..62).collect();
    for &n in &others {
        pl.on(1, n);
    }
    pl.block();
    pl.on(0, 62);
    for _ in 0..60 {
        pl.block();
    }
    // Full: the next note fits only in the room the cut frees.
    assert!(pl.billed() + bill_of(&cut, 63, 0) > STREAM_BUDGET_BPS.get());
    pl.on(0, 63);
    for _ in 0..4 {
        pl.block();
    }
    for &n in &others {
        let v = pl.voice_of(1, n).expect("Part 2 keeps its notes");
        assert!(pl.rig.inst.active()[v], "{n} plays on");
    }
    let v = pl.voice_of(0, 63).expect("the cut note");
    assert!(pl.rig.inst.active()[v]);
}
