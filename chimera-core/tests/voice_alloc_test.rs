//! Digitone-style voice allocation (instrument-core spec § Voice allocation).

use chimera_core::hw::{Cost, AUDIO_CYCLE_BUDGET, MAX_VOICES};
use chimera_core::part::PartMode::{self, Mono, Poly};
use chimera_core::voice_alloc::{Alloc, Allocator};
use chimera_core::MidiNote;

const FM: Cost = Cost(610);
const NONE: Cost = Cost::ZERO;

fn n(v: u8) -> MidiNote {
    MidiNote::new(v).unwrap()
}

fn on(a: &mut Allocator, part: u8, mode: PartMode, note: u8) -> Alloc {
    a.note_on(part, mode, n(note), FM, NONE)
}

fn voice(r: Alloc) -> usize {
    match r {
        Alloc::Voice(v) => v,
        Alloc::Refused => panic!("refused"),
    }
}

#[test]
fn poly_takes_free_voices_round_robin() {
    let mut a = Allocator::new();
    let got: Vec<usize> = (0..3).map(|i| voice(on(&mut a, 0, Poly, 60 + i))).collect();
    assert_eq!(got, [0, 1, 2]);
    a.release(0);
    a.release_finished(0);
    // The next notes continue after the last voice used, wrapping, instead
    // of reusing the just-freed voice 0.
    let got: Vec<usize> = (70..74).map(|i| voice(on(&mut a, 0, Poly, i))).collect();
    assert_eq!(got, [3, 4, 5, 0]);
}

#[test]
fn full_pool_steals_the_oldest_voice_across_parts() {
    let mut a = Allocator::new();
    for i in 0..MAX_VOICES as u8 {
        on(&mut a, i % 2, Poly, 60 + i); // parts 0 and 1 interleaved
    }
    let v = voice(on(&mut a, 2, Poly, 90)); // part 2, pool full
    assert_eq!(v, 0, "voice 0 (part 0, note 60) is the oldest");
    assert_eq!((a.slots()[0].part(), a.slots()[0].note()), (Some(2), Some(n(90))));
}

#[test]
fn mono_voices_are_never_stolen() {
    let mut a = Allocator::new();
    let mono = voice(on(&mut a, 0, Mono, 40)); // oldest voice, but mono
    for i in 1..MAX_VOICES as u8 {
        on(&mut a, 1, Poly, 60 + i);
    }
    let stolen = voice(on(&mut a, 2, Poly, 90));
    assert_ne!(stolen, mono);
    assert_eq!(a.slots()[mono].note(), Some(n(40)));
}

#[test]
fn refuses_when_every_voice_is_mono() {
    let mut a = Allocator::new();
    for p in 0..MAX_VOICES as u8 {
        on(&mut a, p, Mono, 60);
    }
    assert_eq!(on(&mut a, 0, Poly, 61), Alloc::Refused);
    assert_eq!(a.refused(), 1);
}

#[test]
fn mono_retrigger_reuses_its_voice() {
    let mut a = Allocator::new();
    let v = voice(on(&mut a, 3, Mono, 60));
    assert_eq!(voice(on(&mut a, 3, Mono, 64)), v);
    assert_eq!(a.slots()[v].note(), Some(n(64)));
    assert_eq!(a.slots().iter().filter(|s| s.part() == Some(3)).count(), 1);
    assert!(a.slots()[v].held());
}

/// `release` un-holds exactly the voice named; releasing twice, a free
/// voice or an out-of-range index is a no-op.
#[test]
fn release_unholds_only_that_voice() {
    let mut a = Allocator::new();
    let v0 = voice(on(&mut a, 0, Poly, 60));
    let v1 = voice(on(&mut a, 1, Poly, 60)); // same note, other part
    a.release(v1);
    assert!(a.slots()[v0].held());
    assert!(!a.slots()[v1].held());
    a.release(v1); // already released
    a.release(5); // free
    a.release(MAX_VOICES); // out of range
    assert_eq!(a.slots()[v1].part(), Some(1), "released, tail still rings");
    assert!(a.slots()[5].is_free() && a.slots()[v0].held());
}

/// Rule 5: a released voice keeps its slot (its tail rings) until the
/// engine reports inactive; a held voice is never freed that way.
#[test]
fn tails_keep_the_voice_until_finished() {
    let mut a = Allocator::new();
    let v = voice(on(&mut a, 0, Poly, 60));
    a.release_finished(v); // still held: ignored
    assert_eq!(a.slots()[v].part(), Some(0));
    a.release(v);
    assert_eq!(a.slots()[v].part(), Some(0), "tail rings");
    a.release_finished(v);
    assert!(a.slots()[v].is_free());
}

/// Rule 4: over budget, steal one voice if that makes room, else refuse
/// without stealing.
#[test]
fn cpu_budget_steals_or_refuses() {
    let modal = Cost(1_210);
    let mut a = Allocator::new();
    let fx = Cost(1_000);
    // 4 Modal voices + FX = 5,840; a 5th would be 7,050 > 7,000.
    for i in 0..4 {
        assert!(matches!(a.note_on(0, Poly, n(60 + i), modal, fx), Alloc::Voice(_)));
    }
    assert_eq!(a.note_on(1, Poly, n(70), modal, fx), Alloc::Voice(0), "steals the oldest");
    assert_eq!(a.sounding_cost(), Cost(4 * 1_210));
    // Nothing to steal that frees enough: a voice costing more than the
    // whole budget is refused and nothing is stolen.
    let before: Vec<_> = a.slots().iter().map(|s| (s.part(), s.note())).collect();
    assert_eq!(a.note_on(2, Poly, n(80), Cost(6_500), fx), Alloc::Refused);
    let after: Vec<_> = a.slots().iter().map(|s| (s.part(), s.note())).collect();
    assert_eq!(before, after);
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Seeded property test: random notes on/off across parts and modes.
/// Steals take a released tail before a held note and never a mono voice.
#[test]
fn random_play_keeps_the_pool_invariants() {
    const COSTS: [Cost; 3] = [Cost(610), Cost(710), Cost(1_210)];
    const FX: Cost = Cost(1_000);
    let (mut refusals, mut steals) = (0u32, 0u32);
    for seed in 1..=20u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let modes: [PartMode; 6] = core::array::from_fn(|_| if rng.below(3) == 0 { Mono } else { Poly });
        let mut a = Allocator::new();
        // Model: every held (part, note) and the voice playing it.
        let mut held: Vec<(u8, u8, usize)> = Vec::new();
        for step in 0..2_000 {
            let part = rng.below(6) as u8;
            let note = 48 + rng.below(12) as u8;
            let ctx = format!("seed {seed} step {step}");
            match rng.below(4) {
                0 | 1 => {
                    if held.iter().any(|h| h.0 == part && h.1 == note) {
                        continue;
                    }
                    let before: Vec<_> = a.slots().iter().map(|s| (s.part(), s.note(), s.held())).collect();
                    let cost = COSTS[part as usize % 3];
                    if let Alloc::Voice(v) = a.note_on(part, modes[part as usize], n(note), cost, FX) {
                        // The voice's previous held note, if any, was stolen or
                        // (same mono part) retriggered: it is no longer held.
                        held.retain(|h| h.2 != v);
                        held.push((part, note, v));
                        if let (Some(p), _, was_held) = before[v] {
                            assert!(modes[p as usize] == Poly || p == part, "{ctx}: mono voice of part {p} stolen");
                            let stolen = p != part || modes[p as usize] == Poly;
                            steals += stolen as u32;
                            // Tails go first: a held voice is stolen only
                            // when no non-mono voice was ringing out.
                            let tail_ringing = before
                                .iter()
                                .any(|&(q, _, h)| q.is_some_and(|q| modes[q as usize] == Poly) && !h);
                            assert!(!(stolen && was_held && tail_ringing), "{ctx}: held voice stolen over a tail");
                        }
                    }
                }
                2 => {
                    if let Some(i) = held.iter().position(|h| h.0 == part && h.1 == note) {
                        let (_, _, v) = held.remove(i);
                        a.release(v);
                    }
                }
                _ => {
                    let v = rng.below(MAX_VOICES as u64) as usize;
                    a.release_finished(v); // the engine went quiet
                }
            }
            // Every held note is still on its voice, held; nothing else is held.
            for &(p, nn, v) in &held {
                let s = &a.slots()[v];
                assert_eq!((s.part(), s.note(), s.held()), (Some(p), Some(n(nn)), true), "{ctx}");
            }
            assert_eq!(a.slots().iter().filter(|s| s.held()).count(), held.len(), "{ctx}");
            // A mono part plays at most one voice.
            for p in 0..6u8 {
                if modes[p as usize] == Mono {
                    assert!(a.slots().iter().filter(|s| s.part() == Some(p)).count() <= 1, "{ctx}");
                }
            }
            // The sounding total never exceeds the budget.
            assert!(a.sounding_cost() + FX <= AUDIO_CYCLE_BUDGET, "{ctx}");
        }
        refusals += a.refused();
    }
    // The random play reached both the steal and the refuse paths.
    assert!(steals > 0 && refusals > 0, "steals {steals}, refusals {refusals}");
}

/// A Sound change re-costs its sounding voices; over the budget, the newest
/// non-mono voices are shed until it fits.
#[test]
fn recost_sheds_the_newest_voices_over_budget() {
    let mut a = Allocator::new();
    let fx = Cost(600);
    for i in 0..6 {
        on(&mut a, 0, Poly, 60 + i); // 6 × 610 + 600 = 4,260
    }
    for v in 0..6 {
        a.recost(v, Cost(1_210)); // the part switched to Modal: 7,860
    }
    let mut shed = Vec::new();
    while let Some(v) = a.shed(fx) {
        shed.push(v);
    }
    assert_eq!(shed, [5], "newest first, only as many as needed");
    assert!(a.slots()[5].is_free());
    assert_eq!(a.sounding_cost() + fx, Cost(5 * 1_210 + 600));
}

/// Review Focus: a Part switched from Poly to Mono while a chord is held
/// still releases the chord (no stuck notes); new notes share one voice.
#[test]
fn poly_to_mono_switch_releases_the_held_chord() {
    let mut a = Allocator::new();
    let chord: Vec<usize> = (0..3).map(|i| voice(on(&mut a, 0, Poly, 60 + i))).collect();
    let m = voice(on(&mut a, 0, Mono, 72));
    assert!(!chord.contains(&m));
    assert_eq!(voice(on(&mut a, 0, Mono, 74)), m);
    for (i, &v) in chord.iter().enumerate() {
        assert_eq!(a.slots()[v].note(), Some(n(60 + i as u8)));
        a.release(v);
        assert!(!a.slots()[v].held());
    }
}

/// Rule 3: a full pool steals a released tail before a held note — a held
/// drone survives newer tails.
#[test]
fn full_pool_steals_a_tail_before_a_held_drone() {
    let mut a = Allocator::new();
    let drone = voice(on(&mut a, 0, Poly, 36)); // oldest, held
    let tails: Vec<usize> = (1..MAX_VOICES as u8).map(|i| voice(on(&mut a, 1, Poly, 60 + i))).collect();
    for &v in &tails {
        a.release(v); // tails ring
    }
    let v = voice(on(&mut a, 2, Poly, 90));
    assert_eq!(v, tails[0], "the oldest tail, not the drone");
    assert_eq!(a.slots()[drone].note(), Some(n(36)));
    assert!(a.slots()[drone].held());
}

/// Rule 4 prefers a tail too: over the CPU budget, the oldest released
/// voice is stolen before an older held one.
#[test]
fn cpu_budget_steals_a_tail_before_a_held_note() {
    let modal = Cost(1_210);
    let fx = Cost(1_000);
    let mut a = Allocator::new();
    let drone = voice(a.note_on(0, Poly, n(36), modal, fx));
    let rest: Vec<usize> = (1..4).map(|i| voice(a.note_on(0, Poly, n(60 + i), modal, fx))).collect();
    a.release(rest[1]);
    a.release(rest[2]);
    assert_eq!(a.note_on(1, Poly, n(70), modal, fx), Alloc::Voice(rest[1]));
    assert!(a.slots()[drone].held());
}
