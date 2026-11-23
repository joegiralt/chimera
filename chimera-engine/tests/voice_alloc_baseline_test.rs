// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ADR 0281's "AUTO and 0 pick as before", against the allocator as it was
//! before MAX VOICES and RESERVE: `baseline/voice_alloc_33794f79.rs` is
//! `voice_alloc.rs` at 33794f79 (drum-groups), its imports pointed at the
//! crate, kept as a fixture. A seeded walk drives both through the same
//! calls (mono and poly, several budgets, re-strike masks, `pick_hit`
//! with `Any` and `Only`, release, shed, `shed_one` with filters, kill,
//! recost, `fade_from`) and checks every pick and every slot, every step.
//! With no limit, and with every Part capped at the whole pool, the two
//! must never differ.

#[allow(dead_code)]
#[path = "baseline/voice_alloc_33794f79.rs"]
mod old;

use chimera_engine::MidiNote;
use chimera_engine::hw::{AUDIO_BUDGET_PERCENT, Cost, MAX_PARTS, MAX_VOICES, SampleBudget};
use chimera_engine::part::PartMode;
use chimera_engine::project::PartId;
use chimera_engine::voice_alloc as new;
use chimera_engine::voice_limit::{Asked, MaxVoices, Reserve, Shares, VoiceCap};

/// `k` cycles a sample at the share audio gets.
fn budget(k: u64) -> SampleBudget {
    SampleBudget::for_cpu((k * 4_800_000 / AUDIO_BUDGET_PERCENT as u64) as u32)
}

/// xorshift64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

type Slot = (Option<usize>, Option<MidiNote>, u32, bool, bool, u32);

fn slots_new(a: &new::Allocator) -> Vec<Slot> {
    a.slots()
        .iter()
        .map(|s| {
            let p = s.part().map(PartId::index);
            (p, s.note(), s.age(), s.held(), s.dying(), s.cost().0)
        })
        .collect()
}

fn slots_old(a: &old::Allocator) -> Vec<Slot> {
    a.slots()
        .iter()
        .map(|s| {
            let p = s.part().map(usize::from);
            (p, s.note(), s.age(), s.held(), s.dying(), s.cost().0)
        })
        .collect()
}

#[derive(Default)]
struct Seen {
    picks: u32,
    refused: u32,
    steals: u32,
    hits: u32,
    sheds: u32,
}

/// `steps` seeded steps on a budget of `k`, the new allocator under
/// `shares`, each checked against the old.
fn run(seed: u64, shares: Shares, steps: usize, k: u64, seen: &mut Seen) {
    let mut r = Rng(seed);
    let mut a = new::Allocator::new(budget(k));
    a.set_shares(shares);
    let mut b = old::Allocator::new(budget(k));
    for step in 0..steps {
        let x = r.next();
        let p = (x % MAX_PARTS as u64) as usize;
        let (pn, po) = (PartId::ALL[p], p as u8);
        let v = ((x >> 8) % MAX_VOICES as u64) as usize;
        let mode = if (x >> 12) & 3 == 0 {
            PartMode::Mono
        } else {
            PartMode::Poly
        };
        let note = MidiNote::new(36 + ((x >> 20) % 24) as u8).unwrap();
        let cost = Cost(200 + ((x >> 30) % 1500) as u32);
        let reserved = Cost(((x >> 40) % 800) as u32);
        let mask = (x >> 48) as u8;
        let ctx = format!("seed {seed:#x} k {k} step {step}");
        match (x >> 16) % 12 {
            0..=2 => {
                let ra = a.pick_where(
                    pn,
                    mode,
                    note,
                    (cost, reserved),
                    &|w| (mask >> w.index()) & 1 == 0,
                    new::Steal::Any,
                );
                let rb = b.pick_where(
                    po,
                    mode,
                    note,
                    (cost, reserved),
                    &|w| (mask >> w.index()) & 1 == 0,
                    old::Steal::Any,
                );
                assert_eq!(ra.map(new::VoiceIdx::index), rb, "pick: {ctx}");
                seen.picks += 1;
                match rb {
                    None => seen.refused += 1,
                    Some(w) => {
                        seen.steals += u32::from(!b.slots()[w].is_free());
                        a.book(new::VoiceIdx::ALL[w], pn, mode, note, cost);
                        b.book(w, po, mode, note, cost);
                    }
                }
            }
            3 => {
                let only = (0..MAX_VOICES).filter(|w| (mask >> w) & 1 == 1);
                let (sa, sb) = if (x >> 60) & 1 == 0 {
                    (new::Steal::Any, old::Steal::Any)
                } else {
                    let sa = only
                        .clone()
                        .fold(new::VoiceSet::EMPTY, |s, w| s.with(new::VoiceIdx::ALL[w]));
                    let sb = only.fold(old::VoiceSet::EMPTY, |s, w| s.with(old::VoiceIdx::ALL[w]));
                    (new::Steal::Only(sa), old::Steal::Only(sb))
                };
                let ra = a.pick_hit(pn, mode, note, cost, reserved, sa);
                let rb = b.pick_hit(po, mode, note, cost, reserved, sb);
                assert_eq!(ra.map(new::VoiceIdx::index), rb, "hit: {ctx}");
                seen.hits += 1;
                if let Some(w) = rb {
                    a.book(new::VoiceIdx::ALL[w], pn, mode, note, cost);
                    b.book(w, po, mode, note, cost);
                }
            }
            4 | 5 => {
                a.release(new::VoiceIdx::ALL[v]);
                b.release(v);
            }
            6 | 7 => {
                a.release_finished(new::VoiceIdx::ALL[v]);
                b.release_finished(v);
            }
            8 => {
                let (sa, sb) = (a.shed(reserved), b.shed(reserved));
                assert_eq!(sa.map(new::VoiceIdx::index), sb, "shed: {ctx}");
                seen.sheds += u32::from(sb.is_some());
            }
            9 => {
                let sa = a.shed_one(|w| (mask >> w.index()) & 1 == 0);
                let sb = b.shed_one(|w| (mask >> w.index()) & 1 == 0);
                assert_eq!(
                    sa.map(new::VoiceIdx::index),
                    sb.map(old::VoiceIdx::index),
                    "shed_one: {ctx}"
                );
                seen.sheds += u32::from(sb.is_some());
            }
            10 => {
                a.kill(new::VoiceIdx::ALL[v]);
                b.kill(v);
            }
            _ => {
                a.recost(new::VoiceIdx::ALL[v], cost);
                b.recost(v, cost);
                a.fade_from(new::VoiceIdx::ALL[v], reserved);
                b.fade_from(v, reserved);
            }
        }
        assert_eq!(slots_new(&a), slots_old(&b), "slots: {ctx}");
        assert_eq!(a.refused(), b.refused(), "refused: {ctx}");
        assert_eq!(a.sounding_cost(), b.sounding_cost(), "cost: {ctx}");
    }
}

const BUDGETS: [u64; 4] = [2_000, 5_000, 7_000, 20_000];

fn walk(shares: Shares) -> Seen {
    let mut seen = Seen::default();
    for seed in 1..200u64 {
        for k in BUDGETS {
            run(
                seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
                shares,
                3_000,
                k,
                &mut seen,
            );
        }
    }
    // The walk reaches every path it means to: refusals, steals, hits,
    // sheds.
    assert!(
        seen.picks > 0 && seen.refused > 0 && seen.steals > 0,
        "picks"
    );
    assert!(seen.hits > 0 && seen.sheds > 0, "hits and sheds");
    seen
}

#[test]
fn auto_and_zero_pick_as_the_baseline() {
    walk(Shares::AUTO);
}

/// A cap of the whole pool caps nothing: a Part with eight live voices
/// owns them all, so its own oldest is the pool's (a Part's mono voices
/// are at most one, so one of its own is never mono).
#[test]
fn a_cap_of_the_pool_picks_as_the_baseline() {
    let eight = Asked {
        max: MaxVoices::Cap(VoiceCap::new(MAX_VOICES as u8).unwrap()),
        reserve: Reserve::NONE,
    };
    walk(Shares::from_parts(&[eight; MAX_PARTS]));
}
