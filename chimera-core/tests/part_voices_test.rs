// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A Part's MAX VOICES and RESERVE (ADR 0281, #966): at its cap a Part
//! steals its own voice, free voices or not; at or below its reserve
//! another Part's note can't steal its voices; a shed takes guarded
//! voices last; AUTO and 0 pick as before; DRUM's own-voice steal works
//! inside the cap; the cells refuse what can't be honoured; saved with
//! the Part.

mod common;
mod screen;

use std::sync::{Mutex, MutexGuard, OnceLock};

use chimera_core::MidiNote;
use chimera_core::addr::{BlockRead, BlockRef, BlockView, BlockViewMut, Blocks};
use chimera_core::block::Block;
use chimera_core::dsp::drum::types::{Kit, PadId};
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::hw::{AUDIO_BUDGET_PERCENT, Cost, MAX_PARTS, MAX_VOICES, SampleBudget};
use chimera_core::part::PartMode::Poly;
use chimera_core::part::PartParams;
use chimera_core::project::{PartId, Project};
use chimera_core::voice_alloc::{Alloc, Allocator, Steal, VoiceIdx, VoiceSet};
use chimera_core::voice_limit::{
    Asked, Booked, Limits, MaxVoices, Refusal, Reserve, Shares, VoiceCap, VoiceCount, VoiceLimit,
};
use common::drum::{PLAY_BUDGET, drum_hits, drum_rig};

const BUDGET: SampleBudget =
    SampleBudget::for_cpu((7_000 * 4_800_000 / AUDIO_BUDGET_PERCENT as u64) as u32);
const NOTE_COST: Cost = Cost(610);
const NONE: Cost = Cost::ZERO;

fn n(v: u8) -> MidiNote {
    MidiNote::new(v).unwrap()
}

fn on(a: &mut Allocator, p: u8, note: u8) -> Alloc {
    a.note_on(part(usize::from(p)), Poly, n(note), NOTE_COST, NONE)
}

fn voice(r: Alloc) -> usize {
    match r {
        Alloc::Voice(v) => v.index(),
        Alloc::Refused => panic!("refused"),
    }
}

fn cap(c: u8) -> MaxVoices {
    MaxVoices::Cap(VoiceCap::new(c).unwrap())
}

fn reserve(r: u8) -> Reserve {
    Reserve::new(VoiceCount::new(r).unwrap())
}

fn part(i: usize) -> PartId {
    PartId::ALL[i]
}

/// Each Part's pair: Part `p`'s from `l`, the rest AUTO.
fn asks(l: &[(usize, Asked)]) -> [Asked; MAX_PARTS] {
    let mut a = [Asked::AUTO; MAX_PARTS];
    for &(p, x) in l {
        a[p] = x;
    }
    a
}

/// An allocator whose Part `p` has `l`, the rest AUTO.
fn limited(l: &[(usize, Asked)]) -> Allocator {
    let mut a = Allocator::new(BUDGET);
    a.set_shares(Shares::from_parts(&asks(l)));
    a
}

fn capped(p: usize, c: u8) -> (usize, Asked) {
    (
        p,
        Asked {
            max: cap(c),
            reserve: Reserve::NONE,
        },
    )
}

fn reserved(p: usize, r: u8) -> (usize, Asked) {
    (
        p,
        Asked {
            max: MaxVoices::Auto,
            reserve: reserve(r),
        },
    )
}

// ---------------------------------------------------------------------------
// The types
// ---------------------------------------------------------------------------

#[test]
fn counts_and_caps_are_bounded() {
    assert_eq!(VoiceCount::new(MAX_VOICES as u8), Some(VoiceCount::POOL));
    assert_eq!(VoiceCount::new(MAX_VOICES as u8 + 1), None);
    assert_eq!(VoiceCap::new(0), None, "a Part always gets a voice");
    assert!(VoiceCap::new(1).is_some() && VoiceCap::new(9).is_none());
    for c in 0..=VoiceCount::POOL.get() {
        assert_eq!(MaxVoices::from_code(c).unwrap().code(), c);
        assert_eq!(Reserve::from_code(c).unwrap().code(), c);
    }
    assert_eq!(MaxVoices::from_code(0), Some(MaxVoices::Auto));
    assert_eq!(MaxVoices::from_code(9), None);
    assert_eq!(Reserve::from_code(9), None);
    assert_eq!(VoiceLimit::default(), VoiceLimit::AUTO);
    assert_eq!(PartParams::for_part(3).voices(), Asked::AUTO);
}

#[test]
fn a_cap_is_reached_at_its_count_and_a_reserve_guards_up_to_it() {
    let c = |n| VoiceCount::new(n).unwrap();
    assert!(!MaxVoices::Auto.reached(VoiceCount::POOL));
    assert!(!cap(2).reached(c(1)) && cap(2).reached(c(2)));
    assert!(reserve(2).guards(c(2)) && !reserve(2).guards(c(3)));
    assert!(!Reserve::NONE.guards(c(1)), "0 guards no sounding Part");
}

#[test]
fn reserves_are_booked_within_the_pool() {
    let mut l = Limits::AUTO;
    assert_eq!(l.booked().count(), VoiceCount::ZERO);
    let two = VoiceLimit::new(MaxVoices::Auto, reserve(2)).unwrap();
    for p in PartId::ALL.into_iter().take(4) {
        assert_eq!(l.try_set(p, two), Ok(()));
    }
    assert_eq!(l.booked().count(), VoiceCount::POOL);
    let refused = l.try_set(part(4), two).unwrap_err();
    assert!(matches!(refused, Refusal::Overbooked(b) if b.sum().get() == 10));
    assert_eq!(l.booked().count(), VoiceCount::POOL, "nothing written");
    assert_eq!(Booked::new(9), None);
    assert_eq!(
        Booked::sum(&[
            reserve(5),
            reserve(4),
            Reserve::NONE,
            Reserve::NONE,
            Reserve::NONE,
            Reserve::NONE
        ])
        .map_err(|o| o.sum().get()),
        Err(9)
    );
}

#[test]
fn a_reserve_over_its_cap_is_unlawful() {
    let o = VoiceLimit::new(cap(2), reserve(3)).unwrap_err();
    assert_eq!((o.reserve(), o.cap().count().get()), (reserve(3), 2));
    assert!(VoiceLimit::new(cap(2), reserve(2)).is_ok());
    assert!(VoiceLimit::new(MaxVoices::Auto, reserve(8)).is_ok());
    let a = Asked {
        max: cap(1),
        reserve: reserve(4),
    };
    assert_eq!(a.over_cap(), VoiceCount::new(3).unwrap());
    assert!(a.lawful().is_err());
}

#[test]
fn shares_are_lawful_or_over() {
    assert!(matches!(
        Shares::from_parts(&asks(&[reserved(0, 8)])),
        Shares::Lawful(_)
    ));
    assert!(matches!(
        Shares::from_parts(&asks(&[reserved(0, 5), reserved(1, 4)])),
        Shares::Over(_)
    ));
    let over_cap = Asked {
        max: cap(1),
        reserve: reserve(2),
    };
    let s = Shares::from_parts(&asks(&[(3, over_cap)]));
    assert!(matches!(s, Shares::Over(_)));
    assert_eq!(s.ask(part(3)), over_cap, "loaded as saved");
}

#[test]
fn the_cells_steps_saturate_and_end() {
    use chimera_core::voice_limit::Edge;
    assert_eq!(MaxVoices::Auto.stepped(-1), MaxVoices::Auto);
    assert_eq!(MaxVoices::Auto.stepped(1), cap(1));
    assert_eq!(cap(7).stepped(5), cap(8));
    assert_eq!(MaxVoices::ended(Edge::High), cap(8));
    assert_eq!(MaxVoices::ended(Edge::Low), MaxVoices::Auto);
    assert_eq!(reserve(1).stepped(-3), Reserve::NONE);
    assert_eq!(Reserve::ended(Edge::High), reserve(8));
    assert_eq!(MaxVoices::from_f32(f32::NAN), MaxVoices::Auto);
    assert_eq!(MaxVoices::from_f32(-2.0), MaxVoices::Auto);
    assert_eq!(MaxVoices::from_f32(3.9), cap(3));
    assert_eq!(MaxVoices::from_f32(300.0), cap(8));
    assert_eq!(Reserve::from_f32(9.0), reserve(8));
}

/// The cells' names are their types' codes: `names[code]` is the code's
/// value as the cell shows it.
#[test]
fn the_cells_names_follow_their_codes() {
    use chimera_core::part::{MAX_VOICES_NAMES, RESERVE_NAMES};
    for c in 0..VoiceCount::VALUES {
        let m = MaxVoices::from_code(c).unwrap();
        let want = match m {
            MaxVoices::Auto => "AUTO".to_string(),
            MaxVoices::Cap(k) => k.count().get().to_string(),
        };
        assert_eq!(MAX_VOICES_NAMES[usize::from(c)], want);
        let r = Reserve::from_code(c).unwrap();
        assert_eq!(RESERVE_NAMES[usize::from(c)], r.count().get().to_string());
    }
    assert_eq!(MAX_VOICES_NAMES.len(), usize::from(VoiceCount::VALUES));
}

// ---------------------------------------------------------------------------
// MAX VOICES
// ---------------------------------------------------------------------------

#[test]
fn at_its_cap_a_part_steals_its_own_oldest_with_voices_free() {
    let mut a = limited(&[capped(0, 2)]);
    assert_eq!(voice(on(&mut a, 0, 60)), 0);
    assert_eq!(voice(on(&mut a, 0, 61)), 1);
    assert_eq!(
        voice(on(&mut a, 0, 62)),
        0,
        "its own oldest, not a free one"
    );
    assert_eq!(a.slots()[0].note(), Some(n(62)));
    // Another Part is not capped by it.
    assert_eq!(voice(on(&mut a, 1, 70)), 2);
}

#[test]
fn at_its_cap_its_tail_goes_before_its_held_note() {
    let mut a = limited(&[capped(0, 2)]);
    on(&mut a, 0, 60);
    on(&mut a, 0, 61);
    a.release(VoiceIdx::ALL[1]);
    assert_eq!(voice(on(&mut a, 0, 62)), 1);
}

#[test]
fn a_restrike_at_the_cap_takes_its_key_back() {
    let mut a = limited(&[capped(0, 2)]);
    on(&mut a, 0, 60);
    on(&mut a, 0, 61);
    assert_eq!(voice(on(&mut a, 0, 61)), 1);
}

/// I2: MODE switched MONO→POLY while its one voice rang. At a cap of 1
/// that mono voice is the Part's only one: a Poly note reuses it, never
/// refused with voices free.
#[test]
fn a_capped_part_reuses_its_leftover_mono_voice() {
    let mut a = limited(&[capped(0, 1)]);
    let mono = voice(a.note_on(
        part(0),
        chimera_core::part::PartMode::Mono,
        n(60),
        NOTE_COST,
        NONE,
    ));
    let got = a.note_on(part(0), Poly, n(62), NOTE_COST, NONE);
    assert_eq!(
        got,
        Alloc::Voice(VoiceIdx::ALL[mono]),
        "its own voice, not refused"
    );
    assert_eq!(a.slots()[mono].note(), Some(n(62)));
    // Another Part's mono voice is still never stolen.
    let mut b = limited(&[capped(0, 1)]);
    b.note_on(
        part(1),
        chimera_core::part::PartMode::Mono,
        n(50),
        NOTE_COST,
        NONE,
    );
    let v = voice(b.note_on(part(0), Poly, n(62), NOTE_COST, NONE));
    assert_ne!(b.slots()[v].part(), Some(part(1)));
}

/// MODE switched POLY→MONO with three voices ringing on a cap of 2:
/// the Mono note takes one of the Part's own, the others' untouched.
#[test]
fn a_mode_switch_with_voices_sounding_keeps_to_the_part() {
    let mut a = limited(&[capped(0, 2)]);
    on(&mut a, 1, 40);
    on(&mut a, 0, 60);
    on(&mut a, 0, 61);
    let v = voice(a.note_on(
        part(0),
        chimera_core::part::PartMode::Mono,
        n(70),
        NOTE_COST,
        NONE,
    ));
    assert_eq!(a.slots()[v].part(), Some(part(0)));
    assert!(v == 1 || v == 2, "one of its own: {v}");
    assert_eq!(a.slots()[0].note(), Some(n(40)), "Part 2's voice kept");
}

/// A cap lowered under the voices already sounding ends none of them:
/// the next note steals the Part's own; raised again, it grows.
#[test]
fn a_cap_lowered_mid_note_takes_effect_at_the_next_note() {
    let mut a = limited(&[]);
    for i in 0..4 {
        on(&mut a, 0, 60 + i);
    }
    a.set_shares(Shares::from_parts(&asks(&[capped(0, 2)])));
    assert_eq!(
        a.slots()
            .iter()
            .filter(|s| s.part() == Some(part(0)))
            .count(),
        4
    );
    assert_eq!(voice(on(&mut a, 0, 70)), 0, "its own oldest");
    a.set_shares(Shares::from_parts(&asks(&[capped(0, 6)])));
    assert_eq!(voice(on(&mut a, 0, 71)), 4, "a free voice again");
}

/// A reserve set while the Part sounds guards it from the next steal on.
#[test]
fn a_reserve_raised_mid_note_guards_from_then_on() {
    let mut a = limited(&[]);
    for i in 0..4 {
        on(&mut a, 0, 60 + i);
    }
    for i in 0..4 {
        on(&mut a, 1, 70 + i);
    }
    assert_eq!(voice(on(&mut a, 1, 80)), 0, "unguarded: Part 1's oldest");
    a.set_shares(Shares::from_parts(&asks(&[reserved(0, 3)])));
    assert_eq!(voice(on(&mut a, 1, 81)), 4, "guarded: Part 2's own");
}

/// A dying voice no longer counts: under the cap again, the note takes a
/// free voice.
#[test]
fn a_shed_voice_leaves_the_cap() {
    let mut a = limited(&[capped(0, 2)]);
    on(&mut a, 0, 60);
    on(&mut a, 0, 61);
    a.kill(VoiceIdx::ALL[0]);
    assert_eq!(voice(on(&mut a, 0, 62)), 2);
}

/// Its own oldest frees too little for the note: refused, nothing else
/// taken.
#[test]
fn at_its_cap_a_steal_that_doesnt_fit_is_refused() {
    let mut a = limited(&[capped(0, 1)]);
    let fx = Cost(1_000);
    a.note_on(part(0), Poly, n(60), Cost(500), fx);
    let room = a.budget().as_cost().0 - 1_000 - 500;
    assert_eq!(a.pick(part(0), Poly, n(61), Cost(room + 600), fx), None);
    assert_eq!(
        a.pick(part(0), Poly, n(61), Cost(room + 400), fx),
        Some(VoiceIdx::ALL[0])
    );
}

// ---------------------------------------------------------------------------
// RESERVE
// ---------------------------------------------------------------------------

/// Part 0 holds 4 voices on RESERVE 4; Part 1 fills the pool. Part 1's
/// next note takes its own oldest, not Part 0's older voice.
#[test]
fn a_reserved_part_keeps_its_voices() {
    let mut a = limited(&[reserved(0, 4)]);
    for i in 0..4 {
        on(&mut a, 0, 60 + i);
    }
    for i in 0..4 {
        on(&mut a, 1, 70 + i);
    }
    assert_eq!(voice(on(&mut a, 1, 80)), 4);
    assert_eq!(voice(on(&mut a, 2, 90)), 5, "nor does any other Part");
    assert!((0..4).all(|v| a.slots()[v].part() == Some(part(0))));
}

/// Above its reserve a Part loses voices one at a time, down to it.
#[test]
fn a_part_above_its_reserve_loses_down_to_it() {
    let mut a = limited(&[reserved(0, 2)]);
    for i in 0..5 {
        on(&mut a, 0, 60 + i);
    }
    for i in 0..3 {
        on(&mut a, 1, 70 + i);
    }
    let got: Vec<usize> = (0..4).map(|i| voice(on(&mut a, 1, 80 + i))).collect();
    assert_eq!(got, [0, 1, 2, 5], "three of Part 0's, then Part 1's own");
}

/// ADR 0281 (2026-11-18): RESERVE guards held notes only. A
/// guarded Part's released tail is fair game for another Part, and only
/// its held notes count toward the reserve.
#[test]
fn a_tail_of_a_guarded_part_is_fair_game() {
    let mut a = limited(&[reserved(0, 1)]);
    on(&mut a, 0, 60);
    a.release(VoiceIdx::ALL[0]);
    for i in 0..7 {
        on(&mut a, 1, 70 + i);
    }
    assert_eq!(voice(on(&mut a, 1, 80)), 0, "Part 1's tail, oldest first");
}

/// Two held notes and a tail on RESERVE 2: the held ones count, so the
/// Part is at its reserve and both held notes are guarded; the tail goes.
#[test]
fn only_held_notes_count_toward_the_reserve() {
    let mut a = limited(&[reserved(0, 2)]);
    for i in 0..3 {
        on(&mut a, 0, 60 + i);
    }
    a.release(VoiceIdx::ALL[2]);
    for i in 0..5 {
        on(&mut a, 1, 70 + i);
    }
    assert_eq!(voice(on(&mut a, 1, 80)), 2, "the tail");
    assert_eq!(
        voice(on(&mut a, 1, 81)),
        3,
        "then Part 2's own: both held guarded"
    );
}

#[test]
fn every_voice_guarded_refuses_another_part() {
    let mut a = limited(&[reserved(0, 8)]);
    for i in 0..8 {
        on(&mut a, 0, 60 + i);
    }
    assert_eq!(on(&mut a, 1, 80), Alloc::Refused);
    assert_eq!(voice(on(&mut a, 0, 80)), 0, "its own note still steals");
}

/// A reserve holds no free voice back.
#[test]
fn a_reserve_takes_nothing_while_voices_are_free() {
    let mut a = limited(&[reserved(0, 8)]);
    let got: Vec<usize> = (0..8).map(|i| voice(on(&mut a, 1, 60 + i))).collect();
    assert_eq!(got, (0..8).collect::<Vec<_>>());
    assert_eq!(voice(on(&mut a, 0, 80)), 0, "Part 0 steals Part 1's oldest");
}

// ---------------------------------------------------------------------------
// Shed
// ---------------------------------------------------------------------------

/// Part 1's older held notes, Part 0's newer ones on RESERVE 4: a shed
/// takes Part 1's newest held note before any guarded one (today's order
/// would take Part 0's newest first), and a guarded voice only once none
/// else is left.
#[test]
fn a_shed_takes_guarded_voices_last() {
    let fill = |a: &mut Allocator| {
        for i in 0..4 {
            on(a, 1, 70 + i);
        }
        for i in 0..4 {
            on(a, 0, 60 + i);
        }
    };
    let mut plain = Allocator::new(BUDGET);
    fill(&mut plain);
    assert_eq!(
        plain.shed_one(|_| true),
        VoiceIdx::new(7),
        "today: the newest"
    );
    let mut a = limited(&[reserved(0, 4)]);
    fill(&mut a);
    let order: Vec<usize> = std::iter::from_fn(|| a.shed_one(|_| true))
        .map(VoiceIdx::index)
        .collect();
    assert_eq!(order, [3, 2, 1, 0, 7, 6, 5, 4]);
}

/// A guarded Part's tail is shed as any tail is, ahead of every held note.
#[test]
fn a_guarded_parts_tail_sheds_first() {
    let mut a = limited(&[reserved(0, 4)]);
    for i in 0..3 {
        on(&mut a, 0, 60 + i);
    }
    a.release(VoiceIdx::ALL[0]);
    on(&mut a, 1, 70);
    assert_eq!(a.shed_one(|_| true), VoiceIdx::new(0));
}

// ---------------------------------------------------------------------------
// DRUM's hits (ADR 0280) inside the cap
// ---------------------------------------------------------------------------

/// Part 0 capped at 3: another Part's voice is older, voices are free.
fn drum_at_cap() -> (Allocator, VoiceSet) {
    let mut a = limited(&[capped(0, 3)]);
    on(&mut a, 1, 50);
    let mut own = VoiceSet::EMPTY;
    for i in 0..3 {
        let v = voice(on(&mut a, 0, 10 + i));
        if i > 0 {
            own = own.with(VoiceIdx::new(v).unwrap());
        }
    }
    (a, own)
}

#[test]
fn a_stack_pad_at_the_cap_ends_its_own_oldest() {
    let (a, own) = drum_at_cap();
    assert_eq!(
        a.pick_hit(part(0), Poly, n(0), NOTE_COST, NONE, Steal::Only(own))
            .map(VoiceIdx::index),
        Some(2)
    );
}

#[test]
fn a_pad_with_no_voice_at_the_cap_ends_the_parts_oldest() {
    let (a, _) = drum_at_cap();
    assert_eq!(
        a.pick_hit(part(0), Poly, n(0), NOTE_COST, NONE, Steal::Any)
            .map(VoiceIdx::index),
        Some(1),
        "the Part's oldest, not Part 1's older voice 0 nor a free one"
    );
}

// ---------------------------------------------------------------------------
// Through the instrument
// ---------------------------------------------------------------------------

fn writer() -> MutexGuard<'static, KitWriter> {
    static W: OnceLock<Mutex<KitWriter>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(KitWriter::take().expect("the kit writer, once")))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// A pad on INIT DRUM: its key.
fn key(p: u8) -> u8 {
    PadId::new(p - 1).unwrap().index() as u8
}

/// The dspmax case (chimera-releases#5) on a kit: DRUM Part 1 on RESERVE 3
/// rings three pads, each pad's note-off sent at once; Part 2 then plays
/// eight notes. A hit counts toward its reserve until its voice finishes
/// (ADR 0281, 2026-11-18): the kit's three voices are never
/// taken.
#[test]
fn a_reserved_drum_part_keeps_its_hits_through_a_busy_part() {
    let mut w = writer();
    let k = Kit::DEFAULT;
    let mut r = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    r.shared.parts[0].mix.set_voices(reserved(0, 3).1);
    r.shared.parts[1].mix.channel = chimera_core::MidiChannel::new(1).unwrap();
    for p in [1, 2, 5] {
        r.note_on_vel(0, key(p), 127);
        r.note_off(0, key(p));
        r.render(1);
    }
    let kit: Vec<usize> = drum_hits(&r.inst).map(|(v, _)| v).collect();
    assert_eq!(kit.len(), 3);
    let slots = r.inst.allocator().slots();
    assert!(kit.iter().all(|&v| !slots[v].held()), "every pad's key up");
    for i in 0..8 {
        r.note_on_vel(1, 60 + i, 100);
        r.render(1);
    }
    let slots = r.inst.allocator().slots();
    for v in kit {
        assert_eq!(slots[v].part(), Some(part(0)), "voice {v} kept");
        assert!(!slots[v].dying());
    }
}

/// R1, "until its voice finishes": a hit whose voice has ended frees its
/// slot and its guard though its pad's key is still down (a source with
/// no note-off). The kit's three hits, keys never let go, end; another
/// Part's eight notes then get all eight voices, and the late note-offs
/// change nothing.
#[test]
fn a_finished_hit_frees_its_slot_with_its_key_down() {
    let mut w = writer();
    let k = Kit::DEFAULT;
    let mut r = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    r.shared.parts[0].mix.set_voices(reserved(0, 3).1);
    r.shared.parts[1].mix.channel = chimera_core::MidiChannel::new(1).unwrap();
    for p in [1, 2, 5] {
        r.note_on_vel(0, key(p), 127);
        r.render(1);
    }
    r.render(20_000);
    assert_eq!(drum_hits(&r.inst).count(), 0, "every hit has ended");
    for i in 0..8 {
        r.note_on_vel(1, 60 + i, 100);
        r.render(1);
    }
    let part1 = |r: &common::InstRig| {
        r.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| s.part() == Some(part(1)) && !s.dying())
            .count()
    };
    assert_eq!(part1(&r), 8, "the kit's ended hits hold nothing");
    for p in [1, 2, 5] {
        r.note_off(0, key(p));
    }
    r.render(1);
    assert_eq!(part1(&r), 8, "a late note-off is a no-op");
}

/// A project load mid-note (ADR 0046): the held voices fade, and the
/// loaded project's limits (AUTO here, the old Part capped at 1) rule the
/// next notes: three keys take three voices.
#[test]
fn a_project_load_mid_note_plays_by_the_loaded_limits() {
    let mut r = common::InstRig::with_budget(PLAY_BUDGET);
    r.shared.parts[0].mix.set_voices(capped(0, 1).1);
    for k in [60, 64, 67] {
        r.note_on(0, k);
        r.render(1);
    }
    let live = |r: &common::InstRig| {
        r.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| s.part() == Some(part(0)) && !s.dying())
            .count()
    };
    assert_eq!(live(&r), 1, "capped at 1");
    r.inst.kill_all();
    r.shared.parts[0].mix.set_voices(Asked::AUTO);
    r.render(4);
    for k in [48, 52, 55] {
        r.note_on(0, k);
        r.render(1);
    }
    assert_eq!(live(&r), 3, "the loaded project's AUTO");
}

/// DRUM Part 1 capped at 2, STACK on pad 1: three hits keep two voices.
#[test]
fn a_capped_drum_part_sounds_at_most_its_cap() {
    let mut w = writer();
    let mut k = Kit::DEFAULT;
    k.pad_mut(PadId::new(0).unwrap()).set_retrig(Retrig::Stack);
    let mut r = drum_rig(&mut w, PLAY_BUDGET, &k, &k);
    r.shared.parts[0].mix.set_voices(capped(0, 2).1);
    for p in [1, 1, 2, 5] {
        r.note_on_vel(0, key(p), 127);
        r.render(1);
    }
    let live = r
        .inst
        .allocator()
        .slots()
        .iter()
        .filter(|s| s.part() == Some(part(0)) && !s.dying())
        .count();
    assert_eq!(live, 2);
    assert_eq!(r.inst.allocator().refused(), 0);
}

// ---------------------------------------------------------------------------
// Saved with the Part
// ---------------------------------------------------------------------------

#[test]
fn the_part_block_reads_and_writes_both() {
    let mut m = PartParams::for_part(0);
    m.set(PartParams::MAX_VOICES, 3.0);
    m.set(PartParams::RESERVE, 2.0);
    assert_eq!(m.voices().max, cap(3));
    assert_eq!(m.voices().reserve, reserve(2));
    assert_eq!(m.get(PartParams::MAX_VOICES), 3.0);
    assert_eq!(m.enum_ident(PartParams::MAX_VOICES), Some("3"));
    m.set(PartParams::MAX_VOICES, 0.0);
    assert_eq!(m.voices().max, MaxVoices::Auto);
    assert_eq!(m.enum_ident(PartParams::MAX_VOICES), Some("AUTO"));
    assert!(!m.set_enum_code(PartParams::RESERVE, 9), "past the pool");
    assert_eq!(m.voices().reserve, reserve(2));
}

#[test]
fn a_projects_limits_round_trip() {
    let (mut p, _) = Project::boxed();
    p.edit_part(part(4)).mix.set_voices(Asked {
        max: cap(5),
        reserve: reserve(3),
    });
    let f = common::project::encode(&p);
    let (mut q, _) = Project::boxed();
    q.edit_part(part(1)).mix.set_voices(capped(0, 1).1);
    common::project::decode(&f, &mut q).unwrap();
    assert_eq!(q.part(part(4)).mix.voices().max, cap(5));
    assert_eq!(q.part(part(4)).mix.voices().reserve, reserve(3));
    assert_eq!(q.part(part(1)).mix.voices(), Asked::AUTO, "the file's");
}

struct Mix<'a>(&'a mut PartParams);

impl BlockRead for Mix<'_> {
    fn block(&self, b: BlockRef) -> Option<BlockView<'_>> {
        (b == BlockRef::Part).then_some(BlockView::Plain(&*self.0))
    }
}

impl Blocks for Mix<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>> {
        (b == BlockRef::Part).then_some(BlockViewMut::Plain(&mut *self.0))
    }
}

/// An old project's Part record, ids 0–12 only, loads into the Part's
/// fresh mix (the codec resets it first): AUTO and 0.
#[test]
fn an_old_part_record_loads_auto_and_zero() {
    let mut sent = PartParams::for_part(2);
    sent.set_voices(Asked {
        max: cap(2),
        reserve: reserve(1),
    });
    let mut r = chimera_core::storage::RecordBuf::new();
    chimera_core::storage::encode_block(BlockRef::Part, &sent, None, &mut r);
    let new = r.as_slice();
    let (code, body) = new.split_first().unwrap();
    let mut old = vec![*code];
    // Ids 0–12 and the EQ's (ADR 0284), not ADR 0281's 13 and 14.
    let voices = PartParams::MAX_VOICES.0..=PartParams::RESERVE.0;
    for e in body.chunks(5).filter(|e| !voices.contains(&e[0])) {
        old.extend_from_slice(e);
    }
    assert_eq!(old.len(), new.len() - 10, "both entries were written");
    let mut got = PartParams::for_part(2);
    chimera_core::storage::decode_block(&old, &[], &[], Some(&mut Mix(&mut got))).unwrap();
    assert_eq!(got.voices(), Asked::AUTO);
    let mut got = PartParams::for_part(2);
    chimera_core::storage::decode_block(new, &[], &[], Some(&mut Mix(&mut got))).unwrap();
    assert_eq!(got, sent);
}

// ---------------------------------------------------------------------------
// MIXER › PRT › VOICES
// ---------------------------------------------------------------------------

use chimera_core::ui::block_registry as reg;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::held::{self, Reason};
use chimera_core::ui::{UiState, draw, theme, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed};

/// MIX + B`p`, back to PRT, EDIT down to VOICES; RESERVE touched.
fn voices_page(b: ButtonId, setup: impl FnOnce(&mut Project)) -> UiState {
    let mut ui = screen::stack_ui();
    setup(ui.project_mut());
    feed(&mut ui, Input::chord(ButtonId::Mix, b));
    feed(&mut ui, Input::press(ButtonId::Minus));
    assert_eq!(ui.page_def().id, reg::PART.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, reg::PART_VOICES.id);
    ui.step_toast(60_000);
    ui
}

fn limit(m: u8, r: u8) -> Asked {
    Asked {
        max: MaxVoices::from_code(m).unwrap(),
        reserve: reserve(r),
    }
}

fn fits_band(s: &str) {
    let w = draw::text_width(&theme::FONT_LABEL, s, 0);
    assert!(w <= theme::CONTENT_W, "{s}: {w} px");
}

#[test]
fn the_labels_fit_their_cells() {
    for id in [PartParams::MAX_VOICES, PartParams::RESERVE] {
        let s = chimera_core::part::PART_SPECS
            .iter()
            .find(|s| s.id == id)
            .unwrap();
        let w = draw::text_width(&theme::FONT_LABEL, s.label, theme::LABEL_TRACKING);
        assert!(w <= theme::CELL_BAR_W, "{} is {w} px", s.label);
    }
    assert_eq!(
        reg::PART_VOICES.params.map(|s| s.binding).first().copied(),
        reg::PART.params.map(|s| s.binding).get(1).copied(),
        "MODE beside them"
    );
}

#[test]
fn the_rule_refuses_a_reserve_over_its_cap_and_reserves_past_the_pool() {
    let p = part(0);
    let s = Shares::from_parts(&asks(&[]));
    assert_eq!(s.refusal(p, limit(2, 2)), None);
    assert!(matches!(
        s.refusal(p, limit(2, 3)),
        Some(Refusal::OverCap(o)) if o.reserve() == reserve(3) && o.cap() == VoiceCap::new(2).unwrap()
    ));
    let s = Shares::from_parts(&asks(&[(1, limit(0, 6))]));
    assert_eq!(s.refusal(p, limit(0, 2)), None, "8 fits");
    assert!(matches!(
        s.refusal(p, limit(0, 3)),
        Some(Refusal::Overbooked(b)) if b.sum().get() == 9
    ));
    // Past the rule already (a file): coming down lands, going up doesn't.
    let s = Shares::from_parts(&asks(&[(1, limit(0, 6)), (0, limit(0, 5))]));
    assert!(matches!(s, Shares::Over(_)));
    assert_eq!(s.refusal(p, limit(0, 4)), None);
    assert!(s.refusal(p, limit(0, 6)).is_some());
    let s = Shares::from_parts(&asks(&[(0, limit(1, 4))]));
    assert_eq!(s.refusal(p, limit(2, 4)), None, "cap up, the excess down");
    assert!(s.refusal(p, limit(1, 5)).is_some(), "the excess up");
}

/// Part 2 reserves 6; Part 1's RESERVE turned to 3 would book 9: held
/// dimmed with its reason, nothing written; back to 2 it lands.
#[test]
fn a_reserve_past_the_pool_is_held_with_its_reason() {
    let mut ui = voices_page(ButtonId::B1, |p| {
        p.edit_part(part(1)).mix.set_voices(limit(0, 6))
    });
    feed(&mut ui, Input::turn(EncoderId::C, 2));
    assert_eq!(ui.project().part(part(0)).mix.voices(), limit(0, 2));
    assert!(ui.held_choice().is_none());
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    assert_eq!(
        ui.project().part(part(0)).mix.voices(),
        limit(0, 2),
        "nothing written"
    );
    let shown = ui.held_choice().expect("3 held");
    assert_eq!(shown.held.code(), 3);
    assert!(matches!(shown.why, Reason::Voices(Refusal::Overbooked(b)) if b.sum().get() == 9));
    let why = held::reason(shown.why);
    assert_eq!(why.as_str(), "RESERVES 9 · POOL HOLDS 8");
    fits_band(why.as_str());
    let shot = screen::render_ui(&ui);
    assert!(screen::cell_reads(&shot, 2, "3"), "the held 3 shows");
    feed(&mut ui, Input::turn(EncoderId::C, -1));
    assert!(ui.held_choice().is_none());
    assert_eq!(ui.project().part(part(0)).mix.voices(), limit(0, 2));
}

/// MAX VOICES 2, then RESERVE 3: held; MAX VOICES down under a RESERVE
/// of 2 is held too.
#[test]
fn a_reserve_over_its_cap_is_held_either_way() {
    let mut ui = voices_page(ButtonId::B1, |_| {});
    feed(&mut ui, Input::turn(EncoderId::B, 2));
    feed(&mut ui, Input::turn(EncoderId::C, 2));
    assert_eq!(ui.project().part(part(0)).mix.voices(), limit(2, 2));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    let shown = ui.held_choice().expect("RESERVE 3 held");
    let why = held::reason(shown.why);
    assert_eq!(why.as_str(), "RESERVE 3 · VOICES 2");
    fits_band(why.as_str());
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    let shown = ui.held_choice().expect("MAX VOICES 1 held");
    assert_eq!(shown.held.code(), 1);
    assert_eq!(held::reason(shown.why).as_str(), "RESERVE 2 · VOICES 1");
    assert_eq!(ui.project().part(part(0)).mix.voices(), limit(2, 2));
    // AUTO is below 1 in the list but caps nothing: it lands.
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(ui.project().part(part(0)).mix.voices(), limit(0, 2));
}

/// A set no turn held: refused aloud, nothing written.
#[test]
fn a_set_past_the_pool_is_refused_aloud() {
    let mut ui = voices_page(ButtonId::B1, |p| {
        p.edit_part(part(1)).mix.set_voices(limit(0, 8))
    });
    assert!(!ui.edit_to(part(0), held::RESERVE, 1.0));
    assert_eq!(ui.project().part(part(0)).mix.voices(), Asked::AUTO);
    match ui.step_toast(0) {
        ToastStep::Show(l) => assert_eq!(l.as_str(), "RESERVES 9 · POOL HOLDS 8"),
        s => panic!("no toast: {s:?}"),
    }
    assert!(ui.edit_to(part(0), held::MAX_VOICES, 4.0));
    assert_eq!(ui.project().part(part(0)).mix.voices().max, cap(4));
}

#[test]
fn the_reserve_cell_shows_the_reserves_booked() {
    let ui = voices_page(ButtonId::B1, |p| {
        p.edit_part(part(3)).mix.set_voices(limit(0, 3))
    });
    let parts = ui.project().perf().parts();
    let note = view::voices_note(held::RESERVE, parts).unwrap();
    assert_eq!(note.as_str(), "RESERVES 3/8");
    assert!(view::voices_note(held::MAX_VOICES, parts).is_none());
}

/// Under STACK: EDIT from PRT is VOICES, SEQ back up; EDIT on from
/// VOICES is EQ, then the Sound rung (ADR 0284).
#[test]
fn voices_sits_between_prt_and_the_sound_rung() {
    let mut ui = voices_page(ButtonId::B2, |_| {});
    screen::tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.page_def().id, reg::PART.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert_eq!(ui.page_def().id, reg::SENDS.id);
    feed(&mut ui, Input::press(ButtonId::Minus));
    assert_eq!(ui.page_def().id, reg::PART.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::press(ButtonId::Edit));
    // EQ under VOICES (ADR 0284), then the Sound rung.
    assert_eq!(ui.page_def().id, reg::PART_EQ.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert!(ui.location().browse().is_some(), "the Sound rung");
}

/// MODE, MAX VOICES and RESERVE share one byte, every combination round
/// trips, and the Part block holds them in one byte (ADR 0281); 20 B with
/// the EQ's byte (ADR 0284), once the snapshots left AXI (ADR 0286).
#[test]
fn voicing_packs_mode_cap_and_reserve() {
    use chimera_core::part::{PartMode, Voicing};
    for mode in [PartMode::Mono, PartMode::Poly] {
        for m in 0..VoiceCount::VALUES {
            for r in 0..VoiceCount::VALUES {
                let l = Asked {
                    max: MaxVoices::from_code(m).unwrap(),
                    reserve: Reserve::from_code(r).unwrap(),
                };
                let v = Voicing::new(mode, l);
                assert_eq!((v.mode(), v.asked()), (mode, l));
            }
        }
    }
    assert_eq!(core::mem::size_of::<Voicing>(), 1);
    assert_eq!(core::mem::size_of::<PartParams>(), 20);
}

/// A held RESERVE stays held through a CC on any Part (here Part 3's
/// LEVEL and Part 1's own PAN): what it was judged against is unchanged.
/// Another Part's RESERVE moving drops it.
#[test]
fn a_held_value_stays_through_other_edits_and_drops_on_a_rule_change() {
    use chimera_core::part::PartParams as P;
    let mut ui = voices_page(ButtonId::B1, |p| {
        p.edit_part(part(1)).mix.set_voices(limit(0, 6))
    });
    feed(&mut ui, Input::turn(EncoderId::C, 3));
    assert!(ui.held_choice().is_some(), "3 held");
    let lvl = chimera_core::addr::ParamAddr::new(BlockRef::Part, P::LEVEL);
    let pan = chimera_core::addr::ParamAddr::new(BlockRef::Part, P::PAN);
    assert!(ui.edit_to(part(2), lvl, 0.5));
    assert!(ui.edit_to(part(0), pan, 0.25));
    assert!(ui.held_choice().is_some(), "still held after other edits");
    assert!(ui.edit_to(part(1), held::RESERVE, 4.0));
    assert!(
        ui.held_choice().is_none(),
        "Part 2's RESERVE moved: dropped"
    );
}

/// A slot's freeing rule and its reserve rule, side by side on `Strike`:
/// a held note keeps its slot and reserves; a released one frees and
/// doesn't; a DRUM hit frees with its voice and reserves until then.
#[test]
fn a_strike_says_when_its_slot_frees_and_whether_it_reserves() {
    use chimera_core::voice_alloc::{Key, Strike};
    let table = [
        (Strike::Note(Key::Down), false, true),
        (Strike::Note(Key::Up), true, false),
        (Strike::Hit(Key::Down), true, true),
        (Strike::Hit(Key::Up), true, true),
    ];
    for (s, frees, reserves) in table {
        assert_eq!(
            (s.ends_with_voice(), s.reserves()),
            (frees, reserves),
            "{s:?}"
        );
    }
}
