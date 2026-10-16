use chimera_engine::stream::edf::{Pick, URGENT_BLOCKS, Want, pick, wants};
use chimera_engine::stream::layout::ChunkIdx;
use chimera_engine::stream::ring::{Rings, ThirdSlots};
use chimera_engine::stream::units::{AudioBlocks, SlotIdx};
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_hal::units::UiWait;

const NOW: AudioBlocks = AudioBlocks::new(1_000);

fn want(v: usize, deadline: AudioBlocks) -> Want {
    Want {
        voice: VoiceIdx::ALL[v],
        slot: SlotIdx::A,
        deadline,
    }
}

fn ix(n: u32) -> ChunkIdx {
    ChunkIdx::new(n).unwrap()
}

#[test]
fn earliest_deadline_wins() {
    let (a, b) = (want(0, NOW.after(30)), want(1, NOW.after(20)));
    assert_eq!(pick([a, b].into_iter(), NOW, UiWait::Idle), Pick::Fill(b));
}

#[test]
fn ties_go_to_lower_voice() {
    let (lo, hi) = (want(2, NOW.after(9)), want(5, NOW.after(9)));
    assert_eq!(
        pick([hi, lo].into_iter(), NOW, UiWait::Idle),
        Pick::Fill(lo)
    );
}

#[test]
fn ui_waiting_yields_unless_urgent() {
    let far = want(0, NOW.after(URGENT_BLOCKS + 1));
    let near = want(0, NOW.after(URGENT_BLOCKS));
    assert_eq!(URGENT_BLOCKS, 11);
    assert_eq!(
        pick([far].into_iter(), NOW, UiWait::Waiting),
        Pick::YieldToUi
    );
    assert_eq!(
        pick([near].into_iter(), NOW, UiWait::Waiting),
        Pick::Fill(near)
    );
    assert_eq!(pick([far].into_iter(), NOW, UiWait::Idle), Pick::Fill(far));
    assert_eq!(
        pick([near].into_iter(), NOW, UiWait::Idle),
        Pick::Fill(near)
    );
}

#[test]
fn idle_without_wants() {
    assert_eq!(pick(core::iter::empty(), NOW, UiWait::Idle), Pick::Idle);
    assert_eq!(pick(core::iter::empty(), NOW, UiWait::Waiting), Pick::Idle);
}

#[test]
fn deadline_wraps() {
    let now = AudioBlocks::new(u32::MAX - 2);
    let w = want(0, AudioBlocks::new(3));
    assert_eq!(pick([w].into_iter(), now, UiWait::Waiting), Pick::Fill(w));
}

#[test]
fn overdue_is_most_urgent() {
    let now = AudioBlocks::new(100);
    let late = want(4, AudioBlocks::new(95));
    let soon = want(0, now.after(3));
    assert_eq!(
        pick([soon, late].into_iter(), now, UiWait::Idle),
        Pick::Fill(late)
    );
    assert_eq!(
        pick([soon, late].into_iter(), now, UiWait::Waiting),
        Pick::Fill(late)
    );
}

#[test]
fn wants_lists_only_wanted_slots() {
    let rings = Box::new(Rings::new());
    let third = Box::new(ThirdSlots::new());
    let (mut audio, mut card) = rings.sides(&third).unwrap();
    let turn = audio.turn();
    let (v, a, b, c) = (VoiceIdx::ALL[2], SlotIdx::A, SlotIdx::B, VoiceIdx::ALL[6]);
    let d = AudioBlocks::new(77);
    turn.want(v, a, ix(0), d).unwrap();
    turn.want(v, b, ix(1), d).unwrap();
    turn.want(c, a, ix(2), d).unwrap();
    turn.want(c, SlotIdx::C, ix(3), d.after(5)).unwrap();
    let filling = card.claim(v, a).unwrap();
    let done = card.claim(v, b).unwrap();
    assert!(card.fill_done(done));
    let got: Vec<_> = wants(&rings).collect();
    assert_eq!(
        got,
        [
            Want {
                voice: c,
                slot: a,
                deadline: d
            },
            Want {
                voice: c,
                slot: SlotIdx::C,
                deadline: d.after(5)
            }
        ]
    );
    card.requeue(filling);
    assert_eq!(wants(&rings).count(), 3);
}
