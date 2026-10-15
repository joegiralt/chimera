use chimera_engine::storage::twelve::pack12;
use chimera_engine::stream::layout::{CHUNK_BYTES, CHUNK_FRAMES, ChunkIdx};
use chimera_engine::stream::ring::{AudioTurn, CardSide, Guard, Rings};
use chimera_engine::stream::slot::SlotState;
use chimera_engine::stream::units::{AudioBlocks, Generation, SlotIdx};
use chimera_engine::voice_alloc::VoiceIdx;

const V: VoiceIdx = VoiceIdx::ALL[3];
const A: SlotIdx = SlotIdx::A;
const B: SlotIdx = SlotIdx::B;
const D: AudioBlocks = AudioBlocks::new(40);

fn ix(n: u32) -> ChunkIdx {
    ChunkIdx::new(n).unwrap()
}

fn rings() -> Box<Rings> {
    Box::new(Rings::new())
}

/// A chunk's frames, `base` up, wrapped into 12 bits.
fn ramp(base: i32) -> Vec<i16> {
    (0..CHUNK_FRAMES.get() as i32)
        .map(|i| ((base + i) % 4096 - 2048) as i16)
        .collect()
}

/// Wants, claims and fills `s` with `frames`, guarding it from its other slot.
fn fill(turn: &AudioTurn, card: &mut CardSide, s: SlotIdx, c: u32, frames: &[i16]) -> Generation {
    let g = turn.want(V, s, ix(c), D).unwrap();
    let claim = card.claim(V, s).unwrap();
    let buf = card.fill_buf(&claim).unwrap();
    assert_eq!(buf.len(), CHUNK_BYTES.get() as usize);
    pack12(frames, buf);
    card.write_guard(&claim);
    assert!(card.fill_done(claim));
    g
}

#[test]
fn protocol_moves() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let mut turn = audio.turn();
    assert_eq!(turn.seen(V, A), SlotState::Empty);
    let g = turn.want(V, A, ix(7), D).unwrap();
    assert_eq!(g, Generation::ZERO.next());
    assert_eq!(
        turn.seen(V, A),
        SlotState::Wanted {
            chunk: ix(7),
            generation: g,
            deadline: D
        }
    );
    assert_eq!(
        turn.want(V, A, ix(8), D),
        None,
        "only an Empty slot is wanted"
    );
    let claim = card.claim(V, A).unwrap();
    assert_eq!((claim.voice(), claim.slot(), claim.chunk()), (V, A, ix(7)));
    assert!(
        card.claim(V, A).is_none(),
        "a Filling slot isn't claimed twice"
    );
    assert_eq!(
        turn.seen(V, A),
        SlotState::Filling {
            chunk: ix(7),
            generation: g
        }
    );
    assert!(turn.ready(V, A, g).is_none());
    assert!(card.fill_done(claim));
    assert_eq!(
        turn.seen(V, A),
        SlotState::Ready {
            chunk: ix(7),
            generation: g
        }
    );
    assert_eq!(turn.ready(V, A, g).unwrap().chunk(), ix(7));
    assert_eq!(
        turn.seen(V, B),
        SlotState::Empty,
        "the other slot is untouched"
    );
    turn.release(V, A);
    assert_eq!(turn.seen(V, A), SlotState::Empty);
    assert_eq!(turn.want(V, A, ix(8), D), Some(g.next()));
}

#[test]
fn cancelled_fill_is_dropped() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let mut turn = audio.turn();
    let g = turn.want(V, A, ix(2), D).unwrap();
    let claim = card.claim(V, A).unwrap();
    turn.cancel(V, A);
    assert!(!card.fill_done(claim));
    assert_eq!(turn.seen(V, A), SlotState::Empty);
    assert_eq!(turn.want(V, A, ix(2), D), Some(g.next().next()));
}

#[test]
fn chunk_keeps_24_bits_in_the_word() {
    let r = rings();
    let (mut audio, _card) = r.sides().unwrap();
    let turn = audio.turn();
    let g = turn.want(V, B, ChunkIdx::MAX, D).unwrap();
    assert_eq!(
        turn.seen(V, B),
        SlotState::Wanted {
            chunk: ChunkIdx::MAX,
            generation: g,
            deadline: D
        }
    );
}

#[test]
fn requeue_keeps_the_generation() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let turn = audio.turn();
    let g = turn.want(V, A, ix(5), D).unwrap();
    let claim = card.claim(V, A).unwrap();
    card.requeue(claim);
    assert_eq!(
        turn.seen(V, A),
        SlotState::Wanted {
            chunk: ix(5),
            generation: g,
            deadline: D
        }
    );
    let again = card.claim(V, A).unwrap();
    assert_eq!(again.chunk(), ix(5));
    assert!(card.fill_done(again));
    assert!(turn.ready(V, A, g).is_some());
}

#[test]
fn guard_copies_previous_chunk() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let mut turn = audio.turn();
    let prev = ramp(100);
    let ga = fill(&turn, &mut card, A, 4, &prev);
    let gb = fill(&turn, &mut card, B, 5, &ramp(900));
    {
        let a = turn.ready(V, A, ga).unwrap();
        assert_eq!(a.guard(), Guard::Unguarded, "chunk 3 isn't held");
        let b = turn.ready(V, B, gb).unwrap();
        assert_eq!(b.guard(), Guard::Guarded);
        let n = prev.len();
        let guard: Vec<_> = (-4..0).map(|i| b.sample(i).unwrap()).collect();
        assert_eq!(guard, prev[n - 4..]);
        assert_eq!(b.sample(-5), None);
        assert_eq!(b.sample(0), Some(ramp(900)[0]));
    }

    turn.release(V, A);
    turn.release(V, B);
    let ga = fill(&turn, &mut card, A, 9, &prev);
    let gb = fill(&turn, &mut card, B, 5, &ramp(900));
    assert_eq!(turn.ready(V, A, ga).unwrap().chunk(), ix(9));
    let b = turn.ready(V, B, gb).unwrap();
    assert_eq!(b.guard(), Guard::Unguarded);
    assert_eq!(b.sample(-1), None);
    assert_eq!(b.sample(0), Some(ramp(900)[0]));
}

#[test]
fn samples_unpack_a_ramp() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let turn = audio.turn();
    let frames = ramp(0);
    let g = fill(&turn, &mut card, B, 0, &frames);
    let ready = turn.ready(V, B, g).unwrap();
    let back: Vec<_> = (0..CHUNK_FRAMES.get() as i32)
        .map(|i| ready.sample(i).unwrap())
        .collect();
    assert_eq!(back, frames);
    assert_eq!(ready.sample(CHUNK_FRAMES.get() as i32), None);
    assert_eq!(ready.sample(i32::MIN), None);
    assert_eq!(ready.sample(i32::MAX), None);
}

#[test]
fn ready_needs_own_generation() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let mut turn = audio.turn();
    let old = fill(&turn, &mut card, A, 1, &ramp(0));
    turn.release(V, A);
    let new = turn.want(V, A, ix(1), D).unwrap();
    let claim = card.claim(V, A).unwrap();
    assert!(card.fill_done(claim));
    assert!(turn.ready(V, A, old).is_none());
    assert!(turn.ready(V, A, new).is_some());
}

#[test]
fn stale_claim_gets_no_buffer() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let mut turn = audio.turn();
    turn.want(V, A, ix(3), D).unwrap();
    let first = card.claim(V, A).unwrap();
    turn.cancel(V, A);
    turn.want(V, A, ix(3), D).unwrap();
    let second = card.claim(V, A).unwrap();
    assert!(card.fill_buf(&first).is_none());
    assert!(card.fill_buf(&second).is_some());
    assert!(!card.fill_done(first));
    assert!(card.fill_done(second));
}

#[test]
fn sides_are_taken_once() {
    let r = rings();
    assert!(r.sides().is_some());
    assert!(r.sides().is_none());
}

#[test]
fn wants_lists_wanted_slots_with_their_deadlines() {
    let r = rings();
    let (mut audio, mut card) = r.sides().unwrap();
    let turn = audio.turn();
    let w = VoiceIdx::ALL[7];
    turn.want(V, B, ix(1), AudioBlocks::new(9)).unwrap();
    turn.want(w, A, ix(2), AudioBlocks::new(3)).unwrap();
    turn.want(w, B, ix(3), D).unwrap();
    card.claim(w, B).unwrap();
    let wants: Vec<_> = r.wants().collect();
    assert_eq!(
        wants,
        [(V, B, AudioBlocks::new(9)), (w, A, AudioBlocks::new(3))]
    );
}

#[test]
fn zeroed_rings_are_empty() {
    // SAFETY: all-zero bytes are a valid `Rings` (every slot Empty at
    // generation 0), as the board relies on.
    let r: Box<Rings> = unsafe { Box::<Rings>::new_zeroed().assume_init() };
    let (mut audio, _card) = r.sides().unwrap();
    assert!(r.sides().is_none());
    let turn = audio.turn();
    for v in VoiceIdx::ALL {
        for s in SlotIdx::ALL {
            assert_eq!(turn.seen(v, s), SlotState::Empty);
        }
    }
    assert_eq!(turn.want(V, A, ix(0), D), Some(Generation::ZERO.next()));
}
